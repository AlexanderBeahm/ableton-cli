use std::collections::HashSet;
use std::path::PathBuf;

use crate::output::WriterTarget;
use crate::project::{AudioClip, Project};
use crate::project::{loader, parser};

pub mod metadata;
pub mod template;

use template::Template;

const DEFAULT_TEMPLATE: &str = "{ARTIST} - {TITLE}";
const FULL_PATHS_TEMPLATE: &str = "{PATH}";

#[derive(Debug, Clone)]
pub struct TracklistOptions {
    pub project_path: PathBuf,
    pub output: Option<PathBuf>,
    pub full_paths: bool,
    pub track_template: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TracklistEntry {
    pub index: usize,
    pub start_seconds: f64,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tracklist {
    pub entries: Vec<TracklistEntry>,
    pub total_seconds: f64,
}

/// End-to-end command implementation.
pub fn run(opts: TracklistOptions) -> anyhow::Result<()> {
    let als_path = loader::resolve_als_path(&opts.project_path)?;
    let xml = loader::read_als_xml(&als_path)?;
    let project = parser::parse(&xml)?;
    let template = resolve_template(&opts);
    let tracklist = build_tracklist(&project, &template);
    let formatted = format_tracklist(&tracklist);
    WriterTarget::from_optional_path(opts.output).write(&formatted)?;
    Ok(())
}

fn resolve_template(opts: &TracklistOptions) -> Template {
    let raw = match (opts.full_paths, opts.track_template.as_deref()) {
        (_, Some(s)) => s,
        (true, None) => FULL_PATHS_TEMPLATE,
        (false, None) => DEFAULT_TEMPLATE,
    };
    Template::parse(raw)
}

/// Build a deduplicated tracklist from a parsed project.
///
/// Collects every audio clip across every audio track, sorts by start beat,
/// and emits one entry per unique sample (first occurrence wins). Total length
/// is the latest end-beat across **all** clips, including duplicates.
pub fn build_tracklist(project: &Project, template: &Template) -> Tracklist {
    let mut clips: Vec<&AudioClip> = project.all_clips().collect();
    clips.sort_by(|a, b| {
        a.start_beats
            .partial_cmp(&b.start_beats)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    let mut idx = 1;
    for clip in &clips {
        let id = clip.sample.identity().to_string();
        if !seen.insert(id) {
            continue;
        }
        let start_seconds = project.tempo.seconds_at(clip.start_beats);
        let meta = metadata::extract(&clip.sample);
        let label = template.render(&meta);
        entries.push(TracklistEntry {
            index: idx,
            start_seconds,
            label,
        });
        idx += 1;
    }

    let total_seconds = project.tempo.seconds_at(project.last_clip_end_beats());
    Tracklist {
        entries,
        total_seconds,
    }
}

/// Format a tracklist using the standard `MM:SS:mmm` timestamp format.
pub fn format_tracklist(tracklist: &Tracklist) -> String {
    let mut out = String::new();
    for entry in &tracklist.entries {
        out.push_str(&format!(
            "{}. {} - {}\n",
            entry.index,
            format_timestamp(entry.start_seconds),
            entry.label
        ));
    }
    out.push('\n');
    out.push_str(&format!(
        "Total Length: {}\n",
        format_timestamp(tracklist.total_seconds)
    ));
    out
}

/// Format seconds as `MM:SS:mmm`. Minutes width grows beyond 99 if needed;
/// negative inputs clamp to zero.
pub fn format_timestamp(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let minutes = total_ms / 60_000;
    let secs = (total_ms % 60_000) / 1000;
    let millis = total_ms % 1000;
    format!("{minutes:02}:{secs:02}:{millis:03}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{AudioTrack, SampleRef};
    use crate::time::Tempo;
    use std::path::PathBuf;

    fn sample(name: &str, abs: Option<&str>) -> SampleRef {
        SampleRef {
            name: name.to_string(),
            absolute_path: abs.map(PathBuf::from),
            relative_path: None,
        }
    }

    fn clip(name: &str, start: f64, end: f64, sample_name: &str, abs: Option<&str>) -> AudioClip {
        AudioClip {
            name: name.to_string(),
            start_beats: start,
            end_beats: end,
            sample: sample(sample_name, abs),
        }
    }

    fn project_with_clips(tempo: Tempo, clips: Vec<AudioClip>) -> Project {
        Project {
            tempo,
            audio_tracks: vec![AudioTrack {
                id: "1".into(),
                name: "t".into(),
                clips,
            }],
            all_sample_refs: vec![],
        }
    }

    fn default_template() -> Template {
        Template::parse(DEFAULT_TEMPLATE)
    }

    fn full_paths_template() -> Template {
        Template::parse(FULL_PATHS_TEMPLATE)
    }

    #[test]
    fn format_timestamp_basic() {
        assert_eq!(format_timestamp(0.0), "00:00:000");
        assert_eq!(format_timestamp(1.3), "00:01:300");
        assert_eq!(format_timestamp(63.236), "01:03:236");
    }

    #[test]
    fn format_timestamp_clamps_negative() {
        assert_eq!(format_timestamp(-5.0), "00:00:000");
    }

    #[test]
    fn format_timestamp_supports_long_durations() {
        // 100 minutes 1 second 5 ms.
        assert_eq!(format_timestamp(100.0 * 60.0 + 1.005), "100:01:005");
    }

    #[test]
    fn build_tracklist_dedupes_by_sample_identity() {
        let project = project_with_clips(
            Tempo::Constant(120.0),
            vec![
                clip("A1", 0.0, 4.0, "a.wav", Some("/a.wav")),
                clip("A2", 8.0, 12.0, "a.wav", Some("/a.wav")),
                clip("B", 16.0, 20.0, "b.wav", Some("/b.wav")),
            ],
        );
        let tl = build_tracklist(&project, &default_template());
        assert_eq!(tl.entries.len(), 2);
        assert_eq!(tl.entries[0].index, 1);
        // No metadata on disk → falls back to filename without extension.
        assert_eq!(tl.entries[0].label, "a");
        assert_eq!(tl.entries[0].start_seconds, 0.0);
        assert_eq!(tl.entries[1].index, 2);
        assert_eq!(tl.entries[1].label, "b");
        assert_eq!(tl.entries[1].start_seconds, 16.0 * 60.0 / 120.0);
        assert_eq!(tl.total_seconds, 20.0 * 60.0 / 120.0);
    }

    #[test]
    fn build_tracklist_sorts_by_start_beats() {
        let project = project_with_clips(
            Tempo::Constant(120.0),
            vec![
                clip("late", 32.0, 36.0, "c.wav", Some("/c.wav")),
                clip("early", 0.0, 4.0, "a.wav", Some("/a.wav")),
                clip("mid", 16.0, 20.0, "b.wav", Some("/b.wav")),
            ],
        );
        let tl = build_tracklist(&project, &default_template());
        assert_eq!(tl.entries[0].label, "a");
        assert_eq!(tl.entries[1].label, "b");
        assert_eq!(tl.entries[2].label, "c");
    }

    #[test]
    fn build_tracklist_full_paths_template_uses_absolute() {
        let project = project_with_clips(
            Tempo::Constant(120.0),
            vec![clip("only", 0.0, 4.0, "a.wav", Some("/proj/a.wav"))],
        );
        let tl = build_tracklist(&project, &full_paths_template());
        assert_eq!(tl.entries[0].label, "/proj/a.wav");
    }

    #[test]
    fn build_tracklist_handles_empty_project() {
        let project = Project {
            tempo: Tempo::Constant(120.0),
            audio_tracks: vec![],
            all_sample_refs: vec![],
        };
        let tl = build_tracklist(&project, &default_template());
        assert!(tl.entries.is_empty());
        assert_eq!(tl.total_seconds, 0.0);
    }

    #[test]
    fn resolve_template_defaults_to_artist_title() {
        let opts = TracklistOptions {
            project_path: PathBuf::new(),
            output: None,
            full_paths: false,
            track_template: None,
        };
        let t = resolve_template(&opts);
        // Render with empty metadata → falls back to filename.
        let m = metadata::TrackMetadata::for_test("song", None);
        assert_eq!(t.render(&m), "song");
    }

    #[test]
    fn resolve_template_full_paths_uses_path_token() {
        let opts = TracklistOptions {
            project_path: PathBuf::new(),
            output: None,
            full_paths: true,
            track_template: None,
        };
        let t = resolve_template(&opts);
        let m = metadata::TrackMetadata::for_test("song", Some("/proj/song.mp3".into()));
        assert_eq!(t.render(&m), "/proj/song.mp3");
    }

    #[test]
    fn resolve_template_user_supplied_overrides_default() {
        let opts = TracklistOptions {
            project_path: PathBuf::new(),
            output: None,
            full_paths: false,
            track_template: Some("{FILENAME}".into()),
        };
        let t = resolve_template(&opts);
        let m = metadata::TrackMetadata::for_test("song", None);
        assert_eq!(t.render(&m), "song");
    }

    #[test]
    fn format_tracklist_matches_expected_layout() {
        let tl = Tracklist {
            entries: vec![
                TracklistEntry {
                    index: 1,
                    start_seconds: 0.0,
                    label: "sometrack.mp3".into(),
                },
                TracklistEntry {
                    index: 2,
                    start_seconds: 1.3,
                    label: "sometrack2.m4a".into(),
                },
                TracklistEntry {
                    index: 3,
                    start_seconds: 3.236,
                    label: "somtrakc3.wav".into(),
                },
            ],
            total_seconds: 6.303,
        };
        let formatted = format_tracklist(&tl);
        let expected = "1. 00:00:000 - sometrack.mp3\n\
            2. 00:01:300 - sometrack2.m4a\n\
            3. 00:03:236 - somtrakc3.wav\n\
            \n\
            Total Length: 00:06:303\n";
        assert_eq!(formatted, expected);
    }
}
