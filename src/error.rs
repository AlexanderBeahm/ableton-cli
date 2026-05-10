use std::path::PathBuf;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("path does not exist: {0}")]
    PathNotFound(PathBuf),

    #[error("no .als file found in folder: {0}")]
    NoAlsInFolder(PathBuf),

    #[error("multiple .als files found in folder; expected exactly one: {0}")]
    MultipleAlsInFolder(PathBuf),

    #[error("expected an .als file or a folder containing one: {0}")]
    NotAnAlsTarget(PathBuf),

    #[error("failed to read file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to gunzip .als file {path}: {source}")]
    Gunzip {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid XML in .als file: {0}")]
    Xml(#[from] roxmltree::Error),

    #[error("malformed Ableton project: {0}")]
    Malformed(String),

    #[error("missing required element: {0}")]
    MissingElement(&'static str),

    #[error("failed to write output: {0}")]
    Output(#[source] std::io::Error),
}
