use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::commands::prune::{self, PruneOptions};
use crate::commands::tracklist::{self, TracklistOptions};

/// CLI utility for inspecting and transforming Ableton Live projects.
#[derive(Debug, Parser)]
#[command(name = "ableton-cli", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Build a tracklist of unique samples used in arrangement view, sorted
    /// by their first occurrence on the timeline.
    Tracklist(TracklistArgs),

    /// List (and optionally delete) audio files in the project directory
    /// that are not referenced by the project. Dry-run by default.
    Prune(PruneArgs),
}

#[derive(Debug, clap::Args)]
pub struct TracklistArgs {
    /// Path to an .als file or a folder containing exactly one .als file.
    pub path: PathBuf,

    /// Write the tracklist to a file instead of stdout.
    #[arg(short = 'o', long = "output")]
    pub output: Option<PathBuf>,

    /// Use absolute sample paths instead of file basenames. Equivalent to
    /// `--track-template "{PATH}"`.
    #[arg(long = "full-paths", conflicts_with = "track_template")]
    pub full_paths: bool,

    /// Template for each entry's label. Tokens: {ARTIST}, {TITLE}, {ALBUM},
    /// {ALBUMARTIST}, {YEAR}, {TRACK}, {GENRE}, {COMPOSER}, {COMMENT},
    /// {FILENAME}, {PATH}. Default: "{ARTIST} - {TITLE}".
    #[arg(long = "track-template", value_name = "STRING")]
    pub track_template: Option<String>,
}

#[derive(Debug, clap::Args)]
pub struct PruneArgs {
    /// Path to an .als file or a folder containing exactly one .als file.
    /// The .als parent directory is scanned for unreferenced audio files.
    pub path: PathBuf,

    /// Write the report to a file instead of stdout.
    #[arg(short = 'o', long = "output")]
    pub output: Option<PathBuf>,

    /// Actually delete unreferenced files. Without this flag, prune only
    /// lists candidates (dry-run).
    #[arg(long = "delete")]
    pub delete: bool,
}

impl Cli {
    pub fn execute(self) -> anyhow::Result<()> {
        match self.command {
            Command::Tracklist(args) => tracklist::run(TracklistOptions {
                project_path: args.path,
                output: args.output,
                full_paths: args.full_paths,
                track_template: args.track_template,
            }),
            Command::Prune(args) => prune::run(PruneOptions {
                project_path: args.path,
                output: args.output,
                delete: args.delete,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_tracklist_minimal() {
        let cli = Cli::try_parse_from(["ableton-cli", "tracklist", "/some/path.als"]).unwrap();
        match cli.command {
            Command::Tracklist(args) => {
                assert_eq!(args.path, PathBuf::from("/some/path.als"));
                assert!(args.output.is_none());
                assert!(!args.full_paths);
                assert!(args.track_template.is_none());
            }
            _ => panic!("expected tracklist"),
        }
    }

    #[test]
    fn parses_tracklist_with_flags() {
        let cli = Cli::try_parse_from([
            "ableton-cli",
            "tracklist",
            "/p.als",
            "-o",
            "/out.txt",
            "--full-paths",
        ])
        .unwrap();
        match cli.command {
            Command::Tracklist(args) => {
                assert_eq!(args.output, Some(PathBuf::from("/out.txt")));
                assert!(args.full_paths);
            }
            _ => panic!("expected tracklist"),
        }
    }

    #[test]
    fn parses_tracklist_with_track_template() {
        let cli = Cli::try_parse_from([
            "ableton-cli",
            "tracklist",
            "/p.als",
            "--track-template",
            "{TITLE}",
        ])
        .unwrap();
        match cli.command {
            Command::Tracklist(args) => {
                assert_eq!(args.track_template.as_deref(), Some("{TITLE}"));
            }
            _ => panic!("expected tracklist"),
        }
    }

    #[test]
    fn errors_on_full_paths_and_track_template_together() {
        let err = Cli::try_parse_from([
            "ableton-cli",
            "tracklist",
            "/p.als",
            "--full-paths",
            "--track-template",
            "{TITLE}",
        ])
        .unwrap_err();
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("cannot be used with") || msg.contains("conflict"),
            "expected conflict error, got: {}",
            err
        );
    }

    #[test]
    fn parses_prune_minimal() {
        let cli = Cli::try_parse_from(["ableton-cli", "prune", "/some/proj"]).unwrap();
        match cli.command {
            Command::Prune(args) => {
                assert_eq!(args.path, PathBuf::from("/some/proj"));
                assert!(args.output.is_none());
                assert!(!args.delete);
            }
            _ => panic!("expected prune"),
        }
    }

    #[test]
    fn parses_prune_with_delete_flag() {
        let cli =
            Cli::try_parse_from(["ableton-cli", "prune", "/p.als", "--delete"]).unwrap();
        match cli.command {
            Command::Prune(args) => assert!(args.delete),
            _ => panic!("expected prune"),
        }
    }

    #[test]
    fn requires_path() {
        let err = Cli::try_parse_from(["ableton-cli", "tracklist"]).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("required"));
    }

    #[test]
    fn requires_subcommand() {
        let err = Cli::try_parse_from(["ableton-cli"]).unwrap_err();
        assert!(!err.to_string().is_empty());
    }
}
