pub mod loader;
pub mod parser;

use std::path::PathBuf;

use crate::time::Tempo;

#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub tempo: Tempo,
    pub audio_tracks: Vec<AudioTrack>,
    /// Every `<SampleRef>` in the document, deduped by identity. Includes
    /// (and supersedes) the per-clip `SampleRef`s — covers instrument
    /// samples (Sampler/Simpler/Impulse), drum racks, convolution IRs, etc.
    pub all_sample_refs: Vec<SampleRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioTrack {
    pub id: String,
    pub name: String,
    pub clips: Vec<AudioClip>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioClip {
    pub name: String,
    pub start_beats: f64,
    pub end_beats: f64,
    pub sample: SampleRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SampleRef {
    pub name: String,
    pub absolute_path: Option<PathBuf>,
    pub relative_path: Option<PathBuf>,
}

impl SampleRef {
    /// Stable identity used for deduplication. Prefers absolute path, falls
    /// back to relative path, finally to the sample's name.
    pub fn identity(&self) -> &str {
        if let Some(p) = self.absolute_path.as_ref().and_then(|p| p.to_str()) {
            return p;
        }
        if let Some(p) = self.relative_path.as_ref().and_then(|p| p.to_str()) {
            return p;
        }
        &self.name
    }

    /// Display label for output. With `full_paths=true`, returns the absolute
    /// path string when known; otherwise returns the file basename.
    pub fn display_label(&self, full_paths: bool) -> String {
        if full_paths {
            if let Some(p) = self.absolute_path.as_ref().and_then(|p| p.to_str()) {
                return p.to_string();
            }
            if let Some(p) = self.relative_path.as_ref().and_then(|p| p.to_str()) {
                return p.to_string();
            }
        }
        if let Some(p) = self.absolute_path.as_ref() {
            if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                return name.to_string();
            }
        }
        if let Some(p) = self.relative_path.as_ref() {
            if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                return name.to_string();
            }
        }
        self.name.clone()
    }
}

impl Project {
    /// Latest end position across every audio clip in arrangement view.
    pub fn last_clip_end_beats(&self) -> f64 {
        self.audio_tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .map(|c| c.end_beats)
            .fold(0.0_f64, f64::max)
    }

    /// All audio clips across all tracks, in their original order.
    pub fn all_clips(&self) -> impl Iterator<Item = &AudioClip> {
        self.audio_tracks.iter().flat_map(|t| t.clips.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample(abs: Option<&str>, rel: Option<&str>, name: &str) -> SampleRef {
        SampleRef {
            name: name.to_string(),
            absolute_path: abs.map(PathBuf::from),
            relative_path: rel.map(PathBuf::from),
        }
    }

    #[test]
    fn identity_prefers_absolute_path() {
        let s = sample(Some("/a/b.wav"), Some("b.wav"), "b.wav");
        assert_eq!(s.identity(), "/a/b.wav");
    }

    #[test]
    fn identity_falls_back_to_relative_then_name() {
        let s = sample(None, Some("b.wav"), "b.wav");
        assert_eq!(s.identity(), "b.wav");
        let s = sample(None, None, "only-name");
        assert_eq!(s.identity(), "only-name");
    }

    #[test]
    fn display_label_basename_default() {
        let s = sample(Some("/a/b.wav"), None, "b.wav");
        assert_eq!(s.display_label(false), "b.wav");
    }

    #[test]
    fn display_label_full_path_when_requested() {
        let s = sample(Some("/a/b.wav"), None, "b.wav");
        assert_eq!(s.display_label(true), "/a/b.wav");
    }

    #[test]
    fn display_label_full_path_falls_back_to_relative() {
        let s = sample(None, Some("rel/b.wav"), "b.wav");
        assert_eq!(s.display_label(true), "rel/b.wav");
    }

    #[test]
    fn display_label_falls_back_to_name_when_no_paths() {
        let s = sample(None, None, "synthetic");
        assert_eq!(s.display_label(false), "synthetic");
        assert_eq!(s.display_label(true), "synthetic");
    }

    #[test]
    fn last_clip_end_beats_returns_max_or_zero() {
        let project = Project {
            tempo: Tempo::Constant(120.0),
            audio_tracks: vec![],
            all_sample_refs: vec![],
        };
        assert_eq!(project.last_clip_end_beats(), 0.0);

        let project = Project {
            tempo: Tempo::Constant(120.0),
            audio_tracks: vec![AudioTrack {
                id: "1".into(),
                name: "t".into(),
                clips: vec![
                    AudioClip {
                        name: "a".into(),
                        start_beats: 0.0,
                        end_beats: 4.0,
                        sample: sample(Some("/a.wav"), None, "a.wav"),
                    },
                    AudioClip {
                        name: "b".into(),
                        start_beats: 8.0,
                        end_beats: 12.0,
                        sample: sample(Some("/b.wav"), None, "b.wav"),
                    },
                ],
            }],
            all_sample_refs: vec![],
        };
        assert_eq!(project.last_clip_end_beats(), 12.0);
    }
}
