use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;

use crate::error::{Error, Result};

/// Resolve a user-supplied path to a concrete `.als` file.
///
/// - If `path` is a file ending in `.als`, returns it directly.
/// - If `path` is a directory, looks for exactly one `.als` file inside it
///   (non-recursive). Errors on zero or multiple matches.
pub fn resolve_als_path(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        return Err(Error::PathNotFound(path.to_path_buf()));
    }

    if path.is_file() {
        return if has_als_extension(path) {
            Ok(path.to_path_buf())
        } else {
            Err(Error::NotAnAlsTarget(path.to_path_buf()))
        };
    }

    if path.is_dir() {
        let entries = std::fs::read_dir(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;

        let mut hits: Vec<PathBuf> = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| Error::Io {
                path: path.to_path_buf(),
                source,
            })?;
            let p = entry.path();
            if p.is_file() && has_als_extension(&p) {
                hits.push(p);
            }
        }

        return match hits.len() {
            0 => Err(Error::NoAlsInFolder(path.to_path_buf())),
            1 => Ok(hits.pop().expect("checked length")),
            _ => Err(Error::MultipleAlsInFolder(path.to_path_buf())),
        };
    }

    Err(Error::NotAnAlsTarget(path.to_path_buf()))
}

fn has_als_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("als"))
}

/// Read and gunzip an `.als` file into its raw XML string.
pub fn read_als_xml(path: &Path) -> Result<String> {
    let file = File::open(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut decoder = GzDecoder::new(file);
    let mut xml = String::new();
    decoder.read_to_string(&mut xml).map_err(|source| Error::Gunzip {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(xml)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_gzipped_als(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        let file = File::create(&path).unwrap();
        let mut encoder = GzEncoder::new(file, Compression::default());
        encoder.write_all(contents.as_bytes()).unwrap();
        encoder.finish().unwrap();
        path
    }

    #[test]
    fn resolve_direct_file() {
        let dir = TempDir::new().unwrap();
        let p = write_gzipped_als(dir.path(), "song.als", "<x/>");
        let resolved = resolve_als_path(&p).unwrap();
        assert_eq!(resolved, p);
    }

    #[test]
    fn resolve_uppercase_extension() {
        let dir = TempDir::new().unwrap();
        let p = write_gzipped_als(dir.path(), "Song.ALS", "<x/>");
        let resolved = resolve_als_path(&p).unwrap();
        assert_eq!(resolved, p);
    }

    #[test]
    fn resolve_folder_with_single_als() {
        let dir = TempDir::new().unwrap();
        let expected = write_gzipped_als(dir.path(), "song.als", "<x/>");
        let resolved = resolve_als_path(dir.path()).unwrap();
        assert_eq!(resolved, expected);
    }

    #[test]
    fn resolve_folder_with_no_als_errors() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("readme.txt"), "hi").unwrap();
        let err = resolve_als_path(dir.path()).unwrap_err();
        assert!(matches!(err, Error::NoAlsInFolder(_)));
    }

    #[test]
    fn resolve_folder_with_multiple_als_errors() {
        let dir = TempDir::new().unwrap();
        write_gzipped_als(dir.path(), "a.als", "<x/>");
        write_gzipped_als(dir.path(), "b.als", "<x/>");
        let err = resolve_als_path(dir.path()).unwrap_err();
        assert!(matches!(err, Error::MultipleAlsInFolder(_)));
    }

    #[test]
    fn resolve_nonexistent_path_errors() {
        let err = resolve_als_path(Path::new("/definitely/does/not/exist.als")).unwrap_err();
        assert!(matches!(err, Error::PathNotFound(_)));
    }

    #[test]
    fn resolve_non_als_file_errors() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("notes.txt");
        std::fs::write(&p, "hello").unwrap();
        let err = resolve_als_path(&p).unwrap_err();
        assert!(matches!(err, Error::NotAnAlsTarget(_)));
    }

    #[test]
    fn read_als_xml_returns_decompressed_contents() {
        let dir = TempDir::new().unwrap();
        let payload = "<Ableton><LiveSet/></Ableton>";
        let p = write_gzipped_als(dir.path(), "song.als", payload);
        let xml = read_als_xml(&p).unwrap();
        assert_eq!(xml, payload);
    }

    #[test]
    fn read_als_xml_errors_on_invalid_gzip() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("bad.als");
        std::fs::write(&p, b"not gzip data").unwrap();
        let err = read_als_xml(&p).unwrap_err();
        assert!(matches!(err, Error::Gunzip { .. }));
    }

    #[test]
    fn read_als_xml_errors_on_missing_file() {
        let err = read_als_xml(Path::new("/nope/no/no.als")).unwrap_err();
        assert!(matches!(err, Error::Io { .. }));
    }
}
