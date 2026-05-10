use std::collections::HashSet;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::error::Error;
use crate::output::WriterTarget;
use crate::project::{Project, SampleRef};
use crate::project::{loader, parser};

/// Audio extensions considered as prune candidates (case-insensitive).
const AUDIO_EXTENSIONS: &[&str] = &["wav", "aiff", "aif", "flac", "mp3", "ogg", "m4a"];

/// Directory names skipped during the walk. Ableton owns these.
const SKIP_DIRS: &[&str] = &["Backup", "Ableton Project Info"];

#[derive(Debug, Clone)]
pub struct PruneOptions {
    pub project_path: PathBuf,
    pub output: Option<PathBuf>,
    pub delete: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PruneCandidate {
    /// Absolute (canonical) path of the unreferenced file on disk.
    pub absolute: PathBuf,
    /// Display path relative to the project directory.
    pub relative: PathBuf,
    pub size_bytes: u64,
}

pub fn run(opts: PruneOptions) -> anyhow::Result<()> {
    let als_path = loader::resolve_als_path(&opts.project_path)?;
    let project_dir = als_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("resolved .als has no parent directory: {}", als_path.display()))?
        .to_path_buf();
    let xml = loader::read_als_xml(&als_path)?;
    let project = parser::parse(&xml)?;

    let project_dir_canonical = std::fs::canonicalize(&project_dir).map_err(|source| Error::Io {
        path: project_dir.clone(),
        source,
    })?;

    let referenced = referenced_paths(&project, &project_dir_canonical);
    let mut candidates = find_candidates(&project_dir_canonical, &referenced);
    candidates.sort_by(|a, b| a.relative.cmp(&b.relative));

    let formatted = if opts.delete {
        delete_candidates(&candidates)?;
        format_deleted(&candidates)
    } else {
        format_dry_run(&candidates)
    };

    WriterTarget::from_optional_path(opts.output).write(&formatted)?;
    Ok(())
}

/// Build the set of canonical paths that the project references and that
/// live inside `project_dir`. Also includes any sibling `<file>.asd`
/// analysis files Ableton may keep next to each referenced audio file.
pub fn referenced_paths(project: &Project, project_dir: &Path) -> HashSet<PathBuf> {
    let mut out: HashSet<PathBuf> = HashSet::new();
    for sample in &project.all_sample_refs {
        if let Some(p) = resolve_sample_path(sample, project_dir) {
            add_with_asd(&mut out, p);
        }
    }
    out
}

fn resolve_sample_path(sample: &SampleRef, project_dir: &Path) -> Option<PathBuf> {
    if let Some(rel) = sample.relative_path.as_ref() {
        let joined = project_dir.join(rel);
        if let Ok(canon) = std::fs::canonicalize(&joined) {
            if canon.starts_with(project_dir) {
                return Some(canon);
            }
        }
    }
    if let Some(abs) = sample.absolute_path.as_ref() {
        if let Ok(canon) = std::fs::canonicalize(abs) {
            if canon.starts_with(project_dir) {
                return Some(canon);
            }
        }
    }
    None
}

fn add_with_asd(set: &mut HashSet<PathBuf>, path: PathBuf) {
    let mut asd = path.clone().into_os_string();
    asd.push(".asd");
    set.insert(path);
    set.insert(PathBuf::from(asd));
}

/// Walk `project_dir` and return every audio file (or orphan `.asd`) that
/// is not in `referenced`. Skips Ableton-owned subdirectories.
pub fn find_candidates(project_dir: &Path, referenced: &HashSet<PathBuf>) -> Vec<PruneCandidate> {
    let mut out = Vec::new();
    let walker = WalkDir::new(project_dir).into_iter().filter_entry(|e| {
        if !e.file_type().is_dir() {
            return true;
        }
        e.path() == project_dir
            || !SKIP_DIRS
                .iter()
                .any(|n| e.file_name().to_str() == Some(*n))
    });

    for entry in walker.flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !is_prune_eligible_extension(path) {
            continue;
        }
        let Ok(canon) = std::fs::canonicalize(path) else {
            continue;
        };
        if referenced.contains(&canon) {
            continue;
        }
        let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let relative = canon
            .strip_prefix(project_dir)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| canon.clone());
        out.push(PruneCandidate {
            absolute: canon,
            relative,
            size_bytes,
        });
    }
    out
}

fn is_prune_eligible_extension(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
        return false;
    };
    let lower = ext.to_ascii_lowercase();
    if lower == "asd" {
        return true;
    }
    AUDIO_EXTENSIONS.iter().any(|e| *e == lower)
}

fn delete_candidates(candidates: &[PruneCandidate]) -> Result<(), Error> {
    for c in candidates {
        std::fs::remove_file(&c.absolute).map_err(|source| Error::Io {
            path: c.absolute.clone(),
            source,
        })?;
    }
    Ok(())
}

pub fn format_dry_run(candidates: &[PruneCandidate]) -> String {
    if candidates.is_empty() {
        return "No unreferenced audio files found.\n".to_string();
    }
    let mut out = String::from("Unreferenced files:\n");
    let mut total: u64 = 0;
    for c in candidates {
        out.push_str(&format!(
            "  {} ({})\n",
            c.relative.display(),
            human_size(c.size_bytes)
        ));
        total += c.size_bytes;
    }
    out.push('\n');
    out.push_str(&format!(
        "{} file{}, {} total.\n",
        candidates.len(),
        if candidates.len() == 1 { "" } else { "s" },
        human_size(total)
    ));
    out.push_str("Run with --delete to remove.\n");
    out
}

pub fn format_deleted(candidates: &[PruneCandidate]) -> String {
    if candidates.is_empty() {
        return "No unreferenced audio files found.\n".to_string();
    }
    let mut out = String::from("Deleted files:\n");
    let mut total: u64 = 0;
    for c in candidates {
        out.push_str(&format!(
            "  {} ({})\n",
            c.relative.display(),
            human_size(c.size_bytes)
        ));
        total += c.size_bytes;
    }
    out.push('\n');
    out.push_str(&format!(
        "Deleted {} file{} ({}).\n",
        candidates.len(),
        if candidates.len() == 1 { "" } else { "s" },
        human_size(total)
    ));
    out
}

fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Project, SampleRef};
    use crate::time::Tempo;
    use std::fs;
    use tempfile::TempDir;

    fn write(path: &Path, contents: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    fn sample_ref(abs: Option<&str>, rel: Option<&str>, name: &str) -> SampleRef {
        SampleRef {
            name: name.to_string(),
            absolute_path: abs.map(PathBuf::from),
            relative_path: rel.map(PathBuf::from),
        }
    }

    #[test]
    fn human_size_thresholds() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2 * 1024), "2.0 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.00 MB");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3.00 GB");
    }

    #[test]
    fn is_prune_eligible_recognizes_audio_and_asd() {
        assert!(is_prune_eligible_extension(Path::new("x.wav")));
        assert!(is_prune_eligible_extension(Path::new("x.WAV")));
        assert!(is_prune_eligible_extension(Path::new("x.aiff")));
        assert!(is_prune_eligible_extension(Path::new("x.aif")));
        assert!(is_prune_eligible_extension(Path::new("x.flac")));
        assert!(is_prune_eligible_extension(Path::new("x.mp3")));
        assert!(is_prune_eligible_extension(Path::new("x.ogg")));
        assert!(is_prune_eligible_extension(Path::new("x.m4a")));
        assert!(is_prune_eligible_extension(Path::new("x.wav.asd")));
        assert!(!is_prune_eligible_extension(Path::new("x.txt")));
        assert!(!is_prune_eligible_extension(Path::new("noext")));
        assert!(!is_prune_eligible_extension(Path::new("x.als")));
    }

    #[test]
    fn referenced_paths_resolves_relative_inside_project() {
        let dir = TempDir::new().unwrap();
        let project_dir = fs::canonicalize(dir.path()).unwrap();
        let kept = project_dir.join("Samples/kept.wav");
        write(&kept, b"x");

        let project = Project {
            tempo: Tempo::Constant(120.0),
            audio_tracks: vec![],
            all_sample_refs: vec![sample_ref(None, Some("Samples/kept.wav"), "kept.wav")],
        };

        let set = referenced_paths(&project, &project_dir);
        assert!(set.contains(&fs::canonicalize(&kept).unwrap()));
        let mut asd_path = kept.into_os_string();
        asd_path.push(".asd");
        assert!(set.contains(&PathBuf::from(asd_path)));
    }

    #[test]
    fn referenced_paths_excludes_external_absolute_refs() {
        let dir = TempDir::new().unwrap();
        let project_dir = fs::canonicalize(dir.path()).unwrap();

        let outside_dir = TempDir::new().unwrap();
        let outside = outside_dir.path().join("library.wav");
        write(&outside, b"x");
        let outside_canon = fs::canonicalize(&outside).unwrap();

        let project = Project {
            tempo: Tempo::Constant(120.0),
            audio_tracks: vec![],
            all_sample_refs: vec![sample_ref(outside_canon.to_str(), None, "library.wav")],
        };

        let set = referenced_paths(&project, &project_dir);
        assert!(set.is_empty(), "external refs must not be added: {set:?}");
    }

    #[test]
    fn referenced_paths_ignores_unresolvable_refs() {
        let dir = TempDir::new().unwrap();
        let project_dir = fs::canonicalize(dir.path()).unwrap();

        let project = Project {
            tempo: Tempo::Constant(120.0),
            audio_tracks: vec![],
            all_sample_refs: vec![
                sample_ref(None, Some("Samples/missing.wav"), "missing.wav"),
                sample_ref(Some("C:/Windows/style/path.wav"), None, "path.wav"),
            ],
        };

        let set = referenced_paths(&project, &project_dir);
        assert!(set.is_empty());
    }

    #[test]
    fn find_candidates_flags_orphan_only() {
        let dir = TempDir::new().unwrap();
        let project_dir = fs::canonicalize(dir.path()).unwrap();

        let kept = project_dir.join("Samples/kept.wav");
        let kept_asd = project_dir.join("Samples/kept.wav.asd");
        let orphan = project_dir.join("Samples/orphan.wav");
        let notes = project_dir.join("Samples/notes.txt");
        write(&kept, b"k");
        write(&kept_asd, b"a");
        write(&orphan, b"o");
        write(&notes, b"n");

        let project = Project {
            tempo: Tempo::Constant(120.0),
            audio_tracks: vec![],
            all_sample_refs: vec![sample_ref(None, Some("Samples/kept.wav"), "kept.wav")],
        };
        let set = referenced_paths(&project, &project_dir);
        let cands = find_candidates(&project_dir, &set);
        let names: Vec<_> = cands.iter().map(|c| c.relative.clone()).collect();
        assert_eq!(names, vec![PathBuf::from("Samples/orphan.wav")]);
    }

    #[test]
    fn find_candidates_skips_backup_directory() {
        let dir = TempDir::new().unwrap();
        let project_dir = fs::canonicalize(dir.path()).unwrap();

        // A bogus .wav inside Backup must not be flagged.
        let in_backup = project_dir.join("Backup/snapshot.wav");
        let outside = project_dir.join("Samples/orphan.wav");
        write(&in_backup, b"x");
        write(&outside, b"x");

        let project = Project {
            tempo: Tempo::Constant(120.0),
            audio_tracks: vec![],
            all_sample_refs: vec![],
        };
        let cands = find_candidates(&project_dir, &referenced_paths(&project, &project_dir));
        let names: Vec<_> = cands.iter().map(|c| c.relative.clone()).collect();
        assert_eq!(names, vec![PathBuf::from("Samples/orphan.wav")]);
    }

    #[test]
    fn format_dry_run_renders_summary() {
        let c = vec![PruneCandidate {
            absolute: PathBuf::from("/abs/orphan.wav"),
            relative: PathBuf::from("Samples/orphan.wav"),
            size_bytes: 2048,
        }];
        let s = format_dry_run(&c);
        assert!(s.contains("Unreferenced files:"));
        assert!(s.contains("Samples/orphan.wav"));
        assert!(s.contains("2.0 KB"));
        assert!(s.contains("1 file,"));
        assert!(s.contains("Run with --delete"));
    }

    #[test]
    fn format_dry_run_empty_says_so() {
        let s = format_dry_run(&[]);
        assert_eq!(s, "No unreferenced audio files found.\n");
    }

    #[test]
    fn format_deleted_renders_summary() {
        let c = vec![PruneCandidate {
            absolute: PathBuf::from("/abs/a.wav"),
            relative: PathBuf::from("a.wav"),
            size_bytes: 100,
        }];
        let s = format_deleted(&c);
        assert!(s.contains("Deleted files:"));
        assert!(s.contains("Deleted 1 file"));
    }

    #[test]
    fn format_deleted_empty_says_so() {
        let s = format_deleted(&[]);
        assert_eq!(s, "No unreferenced audio files found.\n");
    }
}
