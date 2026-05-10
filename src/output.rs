use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use crate::error::{Error, Result};

/// Where command output should be written.
pub enum WriterTarget {
    Stdout,
    File(PathBuf),
}

impl WriterTarget {
    pub fn from_optional_path(path: Option<PathBuf>) -> Self {
        match path {
            Some(p) => WriterTarget::File(p),
            None => WriterTarget::Stdout,
        }
    }

    /// Write `contents` to the target, creating or truncating the file when
    /// targeting a path.
    pub fn write(&self, contents: &str) -> Result<()> {
        match self {
            WriterTarget::Stdout => {
                let stdout = std::io::stdout();
                let mut handle = stdout.lock();
                handle.write_all(contents.as_bytes()).map_err(Error::Output)?;
                handle.flush().map_err(Error::Output)
            }
            WriterTarget::File(path) => {
                let file = File::create(path).map_err(|source| Error::Io {
                    path: path.clone(),
                    source,
                })?;
                let mut writer = BufWriter::new(file);
                writer.write_all(contents.as_bytes()).map_err(Error::Output)?;
                writer.flush().map_err(Error::Output)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn from_optional_path_picks_target() {
        match WriterTarget::from_optional_path(None) {
            WriterTarget::Stdout => {}
            _ => panic!("expected Stdout"),
        }
        match WriterTarget::from_optional_path(Some(PathBuf::from("/x"))) {
            WriterTarget::File(p) => assert_eq!(p, PathBuf::from("/x")),
            _ => panic!("expected File"),
        }
    }

    #[test]
    fn write_to_file_overwrites() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("out.txt");
        std::fs::write(&p, "old contents that is longer than new").unwrap();
        let target = WriterTarget::File(p.clone());
        target.write("hello").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "hello");
    }

    #[test]
    fn write_to_file_creates_new() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("new.txt");
        let target = WriterTarget::File(p.clone());
        target.write("hi").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "hi");
    }

    #[test]
    fn write_to_file_returns_io_error_for_invalid_dir() {
        let target = WriterTarget::File(PathBuf::from("/no/such/dir/out.txt"));
        let err = target.write("x").unwrap_err();
        assert!(matches!(err, Error::Io { .. }));
    }
}
