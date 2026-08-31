//! Argument parsing and command bodies.
//!
//! `main.rs` stays thin: it parses, sets up tracing and maps [`Outcome`] onto
//! an exit code. Everything a command actually *does* belongs here, or in the
//! module the command delegates to.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::config::Config;
use crate::{clock, git, hook, install, stats, timelog};

/// What a command decided, before it becomes an exit code.
///
/// The three codes are part of the contract: `0` clean, `1` findings, `2` the
/// run could not complete. `2` is the `Err` arm in `main`, and collapsing `1`
/// into it would make "the commit is not ready" indistinguishable from
/// "madoqua is broken" — the first is the developer's problem, the second is
/// ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing to report — exit `0`.
    Clean,
    /// The command found something worth reporting — exit `1`.
    FindingsReported,
}

/// The overlay semantics, spelled out where someone stuck at a keyboard will
/// find them.
const OVERLAY_HELP: &str = "\
Configuration is [tool.madoqua] in the repo's pyproject.toml, with a personal
overlay at .git/hooks.local.toml layered on top:

  check / fix               replace that list entirely
  extend_check / extend_fix append to the repo's list
  scalar keys (log, ...)    the overlay wins
  MADOQUA_SKIP=\"a,b\"        drops checks by name, for one run only";

#[derive(Debug, Parser)]
#[command(name = "madoqua", version, about, long_about = None)]
pub struct Cli {
    /// Raise the default log level to `debug` (`RUST_LOG` still wins).
    #[arg(long, short, global = true)]
    pub verbose: bool,

    /// Where to start looking for the repository. Defaults to the current directory.
    #[arg(long, global = true)]
    pub root: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the pre-commit hook. This is also what a bare `madoqua` does.
    #[command(long_about = OVERLAY_HELP)]
    Run,

    /// Write the hooks/pre-commit shim and point git at it. Idempotent.
    Install,

    /// Summarise the timing log: per-step n / p50 / p95 / max, in milliseconds.
    Stats {
        /// How far back to look.
        #[arg(long, default_value_t = 30)]
        days: u32,

        /// Emit JSON instead of a table.
        #[arg(long)]
        json: bool,

        /// Only count runs from this repository. Useful when `log` points at a
        /// shared path outside the repo.
        #[arg(long)]
        repo: Option<String>,
    },
}

impl Cli {
    /// Dispatch. A missing subcommand means [`Command::Run`], so the shim can
    /// be a bare `exec madoqua`.
    pub fn run(&self, out: &mut impl Write) -> Result<Outcome> {
        let start = match &self.root {
            Some(root) => root.clone(),
            None => std::env::current_dir()?,
        };

        match self.command.as_ref().unwrap_or(&Command::Run) {
            Command::Run => hook::run(&start, out),
            Command::Install => Self::install(out, &start),
            Command::Stats { days, json, repo } => {
                Self::stats(out, &start, *days, *json, repo.as_deref())
            }
        }
    }

    fn install(out: &mut impl Write, start: &Path) -> Result<Outcome> {
        let root = git::repo_root(start)?;
        let shim = install::install(&root)?;
        writeln!(
            out,
            "installed {} and set core.hooksPath={}",
            shim.display(),
            install::HOOKS_DIR
        )?;
        Ok(Outcome::Clean)
    }

    /// `stats` is an inventory, not a verdict: it reports `0` or fails with
    /// `2`, and never `1`. "Your checks are slow" is not a finding.
    fn stats(
        out: &mut impl Write,
        start: &Path,
        days: u32,
        json: bool,
        repo: Option<&str>,
    ) -> Result<Outcome> {
        let root = git::repo_root(start)?;
        let config = Config::resolve(&root)?;
        let target = timelog::resolve(&root, config.log.as_deref());

        let records = timelog::read(&target.path)?;
        let report = stats::summarise(&records, days, repo, clock::now_unix());

        if json {
            serde_json::to_writer_pretty(&mut *out, &report)?;
            writeln!(out)?;
        } else {
            write!(out, "{}", stats::render(&report))?;
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
    fn no_subcommand_means_run() {
        let cli = Cli::parse_from(["madoqua"]);
        assert!(cli.command.is_none(), "a bare invocation parses");
        assert!(
            matches!(cli.command.as_ref().unwrap_or(&Command::Run), Command::Run),
            "and falls back to the hook, so the shim can be `exec madoqua`"
        );
    }

    #[test]
    fn stats_defaults_to_thirty_days_of_table_output() {
        let cli = Cli::parse_from(["madoqua", "stats"]);
        let Some(Command::Stats { days, json, repo }) = cli.command else {
            panic!("stats parses as stats");
        };
        assert_eq!(days, 30, "the documented default window");
        assert!(!json, "the table is the default rendering");
        assert_eq!(repo, None);
    }

    #[test]
    fn the_run_help_documents_the_overlay_semantics() {
        for phrase in ["extend_check", "replace that list entirely", "MADOQUA_SKIP"] {
            assert!(
                OVERLAY_HELP.contains(phrase),
                "`{phrase}` is the kind of thing people look up in --help"
            );
        }
    }
}
