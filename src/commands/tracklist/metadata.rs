//! Audio file metadata extraction (artist/title/album/etc) via `lofty`.
//!
//! Errors and missing files are silent — extraction always returns a
//! `TrackMetadata` populated at least with the filename, so the template
//! renderer can always fall back to that.

use std::path::{Path, PathBuf};

use lofty::prelude::*;
use lofty::tag::Tag;

use crate::project::SampleRef;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackMetadata {
    pub artist: Option<String>,
    pub title: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<String>,
    pub track: Option<String>,
    pub genre: Option<String>,
    pub composer: Option<String>,
    pub comment: Option<String>,
    /// Always populated; basename of any path on `SampleRef` with the
    /// extension stripped, falling back to `SampleRef::name`.
    pub filename: String,
    pub path: Option<String>,
}

pub fn extract(sample: &SampleRef) -> TrackMetadata {
    let filename = derive_filename(sample);
    let path = derive_path(sample);

    let mut meta = TrackMetadata {
        filename,
        path,
        ..TrackMetadata::default()
    };

    let Some(readable) = readable_path(sample) else {
        return meta;
    };
    let Ok(tagged) = lofty::read_from_path(&readable) else {
        return meta;
    };

    // Note: `Tag::artist()` and friends return the first value for tag
    // formats that support multiple — that matches our tracklist needs.
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return meta;
    };

    meta.artist = clean_string(tag.artist().map(|c| c.into_owned()));
    meta.title = clean_string(tag.title().map(|c| c.into_owned()));
    meta.album = clean_string(tag.album().map(|c| c.into_owned()));
    meta.album_artist = clean_string(item_string(tag, ItemKey::AlbumArtist));
    meta.year = clean_string(tag.year().map(|y| y.to_string()));
    meta.track = clean_string(tag.track().map(|n| n.to_string()));
    meta.genre = clean_string(tag.genre().map(|c| c.into_owned()));
    meta.composer = clean_string(item_string(tag, ItemKey::Composer));
    meta.comment = clean_string(tag.comment().map(|c| c.into_owned()));

    meta
}

fn item_string(tag: &Tag, key: ItemKey) -> Option<String> {
    tag.get_string(&key).map(|s| s.to_string())
}

fn clean_string(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn derive_filename(sample: &SampleRef) -> String {
    let from_path = sample
        .absolute_path
        .as_ref()
        .and_then(|p| basename_no_ext(p))
        .or_else(|| sample.relative_path.as_ref().and_then(|p| basename_no_ext(p)));
    from_path.unwrap_or_else(|| strip_extension(&sample.name))
}

fn derive_path(sample: &SampleRef) -> Option<String> {
    if let Some(p) = sample.absolute_path.as_ref().and_then(|p| p.to_str()) {
        return Some(p.to_string());
    }
    sample
        .relative_path
        .as_ref()
        .and_then(|p| p.to_str())
        .map(|s| s.to_string())
}

fn basename_no_ext(p: &Path) -> Option<String> {
    p.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string())
}

fn strip_extension(name: &str) -> String {
    PathBuf::from(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| name.to_string())
}

fn readable_path(sample: &SampleRef) -> Option<PathBuf> {
    if let Some(p) = sample.absolute_path.as_ref() {
        if p.is_file() {
            return Some(p.clone());
        }
    }
    if let Some(p) = sample.relative_path.as_ref() {
        if p.is_file() {
            return Some(p.clone());
        }
    }
    None
}

#[cfg(test)]
impl TrackMetadata {
    pub fn for_test(filename: &str, path: Option<String>) -> Self {
        TrackMetadata {
            filename: filename.to_string(),
            path,
            ..TrackMetadata::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_with_paths(name: &str, abs: Option<&str>, rel: Option<&str>) -> SampleRef {
        SampleRef {
            name: name.to_string(),
            absolute_path: abs.map(PathBuf::from),
            relative_path: rel.map(PathBuf::from),
        }
    }

    #[test]
    fn filename_from_absolute_path_strips_extension() {
        let s = sample_with_paths("ignored", Some("/proj/track.mp3"), None);
        let m = extract(&s);
        assert_eq!(m.filename, "track");
    }

    #[test]
    fn filename_from_relative_path_when_no_absolute() {
        let s = sample_with_paths("ignored", None, Some("Samples/clip.wav"));
        let m = extract(&s);
        assert_eq!(m.filename, "clip");
    }

    #[test]
    fn filename_falls_back_to_name_when_no_paths() {
        let s = sample_with_paths("just.flac", None, None);
        let m = extract(&s);
        assert_eq!(m.filename, "just");
    }

    #[test]
    fn filename_falls_back_to_name_without_extension() {
        let s = sample_with_paths("plain_name", None, None);
        let m = extract(&s);
        assert_eq!(m.filename, "plain_name");
    }

    #[test]
    fn path_prefers_absolute_over_relative() {
        let s = sample_with_paths("x", Some("/a/b.mp3"), Some("rel/b.mp3"));
        let m = extract(&s);
        assert_eq!(m.path.as_deref(), Some("/a/b.mp3"));
    }

    #[test]
    fn path_uses_relative_when_no_absolute() {
        let s = sample_with_paths("x", None, Some("rel/b.mp3"));
        let m = extract(&s);
        assert_eq!(m.path.as_deref(), Some("rel/b.mp3"));
    }

    #[test]
    fn extract_returns_tagless_when_file_missing() {
        let s = sample_with_paths("ignored", Some("/definitely/does/not/exist.mp3"), None);
        let m = extract(&s);
        assert_eq!(m.filename, "exist");
        assert!(m.artist.is_none());
        assert!(m.title.is_none());
    }

    #[test]
    fn clean_string_trims_and_drops_empty() {
        assert_eq!(clean_string(Some("  Alice  ".into())).as_deref(), Some("Alice"));
        assert_eq!(clean_string(Some("   ".into())), None);
        assert_eq!(clean_string(Some("".into())), None);
        assert_eq!(clean_string(None), None);
    }
}
