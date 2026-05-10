pub mod cli;
pub mod commands;
pub mod error;
pub mod output;
pub mod project;
pub mod time;

pub use error::{Error, Result};

use clap::Parser;

pub fn run() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    cli.execute()
}
