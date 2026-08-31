//! Argument parsing and command bodies.
//!
//! `main.rs` stays thin: it parses, sets up tracing and maps [`Outcome`] onto
//! an exit code. Everything a command actually *does* belongs here, or in the
//! module the command delegates to.

use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::config::Config;
use crate::report::VersionReport;

/// What a command decided, before it becomes an exit code.
///
/// The three codes are part of the contract: `0` clean, `1` findings, `2` the
/// run could not complete. `2` is the `Err` arm in `main`, and collapsing `1`
/// into it would make "we looked and found something" indistinguishable from
/// "we could not look".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing to report — exit `0`.
    Clean,
    /// The command found something worth reporting — exit `1`.
    FindingsReported,
}

#[derive(Debug, Parser)]
#[command(name = "madoqua", version, about, long_about = None)]
pub struct Cli {
    /// Raise the default log level to `debug` (`RUST_LOG` still wins).
    #[arg(long, short, global = true)]
    pub verbose: bool,

    /// Project root. Defaults to the current directory.
    #[arg(long, global = true)]
    pub root: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Report the resolved configuration and version — a placeholder command
    /// that proves the wiring end to end until the real ones land.
    Doctor {
        /// Emit JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
    },
}

impl Cli {
    /// Resolve the project root, load its config, and dispatch.
    pub fn run(&self, out: &mut impl Write) -> Result<Outcome> {
        let root = match &self.root {
            Some(root) => root.clone(),
            None => std::env::current_dir()?,
        };
        let config = Config::load(&root)?;

        match &self.command {
            Command::Doctor { json } => Self::doctor(out, &config, *json),
        }
    }

    fn doctor(out: &mut impl Write, config: &Config, json: bool) -> Result<Outcome> {
        let report = VersionReport::new(config);
        if json {
            serde_json::to_writer_pretty(&mut *out, &report)?;
            writeln!(out)?;
        } else {
            writeln!(out, "madoqua {}", report.version)?;
        }
        Ok(Outcome::Clean)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn doctor_reports_the_crate_version_and_stays_clean() {
        let cli = Cli::parse_from(["madoqua", "doctor"]);
        let mut out = Vec::new();
        let outcome = cli.run(&mut out).expect("doctor runs in any directory");

        assert_eq!(outcome, Outcome::Clean, "doctor reports nothing to fix");
        assert_eq!(
            String::from_utf8(out).expect("output is utf-8"),
            format!("madoqua {}\n", env!("CARGO_PKG_VERSION")),
            "the human-readable form is one line naming the version"
        );
    }
}
