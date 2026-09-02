//! Argument parsing and command bodies.
//!
//! `main.rs` stays thin: it parses, sets up tracing and maps [`Outcome`] onto
//! an exit code. Everything a command actually *does* belongs here, or in the
//! module the command delegates to.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use crate::config::Config;
use crate::guide::{self, Topic};
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

    /// Print setup, triage or tune instructions for this repository
    ///
    /// With no topic, prints `setup` when madoqua is not wired into the
    /// directory it is pointed at and `triage` when it is; `tune` is never
    /// auto-selected. That directory is `--root` when given and the current
    /// one otherwise; detection looks at it alone and never walks up, so point
    /// this at the repository root.
    Guide {
        /// Which instructions to print. Omit to let madoqua choose.
        #[arg(value_enum)]
        topic: Option<Topic>,
    },
}

impl Cli {
    /// Dispatch. A missing subcommand means [`Command::Run`], so the shim can
    /// be a bare `exec madoqua`.
    pub fn run(&self, out: &mut impl Write) -> Result<Outcome> {
        let start = match &self.root {
            Some(root) => root.clone(),
            None => std::env::current_dir()
                .context("cannot determine the current directory; pass --root")?,
        };

        match self.command.as_ref().unwrap_or(&Command::Run) {
            Command::Run => hook::run(&start, out),
            Command::Install => Self::install(out, &start),
            Command::Stats { days, json, repo } => {
                Self::stats(out, &start, *days, *json, repo.as_deref())
            }
            Command::Guide { topic } => Self::guide_in(out, &start, *topic),
        }
    }

    /// Print a guide, resolving the topic against `dir` when none was named.
    ///
    /// `dir` is where the caller pointed us — `--root`, or the current
    /// directory — and is used as given rather than walked up from: the
    /// question auto-selection answers is "is madoqua set up *here*", and the
    /// guide tells its reader to point it at the repository root. Like
    /// `stats`, this is an inventory and never returns `1`.
    fn guide_in(out: &mut impl Write, dir: &Path, topic: Option<Topic>) -> Result<Outcome> {
        let text = if let Some(topic) = topic {
            guide::render(topic, guide::Selection::Explicit)
        } else {
            let source = guide::detect(dir);
            guide::render(guide::auto_topic(source), guide::Selection::Auto(source))
        };
        write!(out, "{text}").context("cannot write to stdout")?;
        Ok(Outcome::Clean)
    }

    fn install(out: &mut impl Write, start: &Path) -> Result<Outcome> {
        let root = git::repo_root(start)?;
        let shim = install::install(&root)?;
        writeln!(out, "installed {} and set core.hooksPath={}", shim.display(), install::HOOKS_DIR)
            .context("cannot write to stdout")?;
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
            serde_json::to_writer_pretty(&mut *out, &report)
                .context("cannot write the report to stdout")?;
            writeln!(out).context("cannot write to stdout")?;
        } else {
            write!(out, "{}", stats::render(&report)).context("cannot write to stdout")?;
        }
        Ok(Outcome::Clean)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    use crate::guide;

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

    /// Every madoqua command the guides show must be one the CLI actually
    /// accepts.
    ///
    /// This lives here rather than in `src/guide.rs` because `Cli` is defined
    /// here: the check is only worth anything against the real `Command`,
    /// aliases, conflicts and all. The extraction it relies on is tested in
    /// `guide::tests::extraction_finds_every_command_the_guides_show`, so an
    /// empty list cannot pass as "everything valid".
    #[test]
    fn every_command_shown_in_a_guide_parses() {
        let mut checked = 0_usize;
        for topic in Topic::all() {
            for argv in guide::embedded_invocations(topic) {
                checked += 1;
                let parsed = Cli::command().try_get_matches_from(&argv);
                assert!(
                    parsed.is_ok(),
                    "guide `{}` shows `{}`, which the CLI rejects: {}",
                    topic.name(),
                    argv.join(" "),
                    parsed.err().map_or_else(String::new, |e| e.to_string()),
                );
            }
        }
        assert!(checked >= 6, "expected several commands across the guides, found {checked}");
    }

    /// Guard the guard: an invocation the CLI would reject must fail the check
    /// above.
    #[test]
    fn an_unknown_flag_would_be_caught() {
        assert!(
            Cli::command().try_get_matches_from(["madoqua", "stats", "--not-a-flag"]).is_err(),
            "the command check would pass anything if clap accepted unknown flags",
        );
    }

    /// The auto-selection rule has to be discoverable from `--help`, since an
    /// agent that ran `madoqua guide` with no topic needs to know why it got
    /// what it got before it trusts it.
    #[test]
    fn guide_help_states_the_auto_selection_rule() {
        let help = Cli::command()
            .get_subcommands()
            .find(|c| c.get_name() == "guide")
            .and_then(clap::Command::get_long_about)
            .map(std::string::ToString::to_string)
            .unwrap_or_default();
        for phrase in ["setup", "triage", "never auto-selected", "repository root", "--root"] {
            assert!(help.contains(phrase), "guide --help should say `{phrase}`: {help}");
        }
    }

    #[test]
    fn guide_topics_parse_as_values() {
        for topic in ["setup", "triage", "tune"] {
            assert!(
                Cli::command().try_get_matches_from(["madoqua", "guide", topic]).is_ok(),
                "`madoqua guide {topic}` should parse",
            );
        }
        assert!(
            Cli::command().try_get_matches_from(["madoqua", "guide", "how"]).is_err(),
            "an unknown topic should be rejected rather than silently defaulted",
        );
    }

    #[test]
    fn an_explicit_topic_is_printed_verbatim_without_looking_at_the_directory() {
        let mut out = Vec::new();
        // A path that does not exist: an explicit topic has no filesystem
        // question to answer, so this must not matter.
        let outcome =
            Cli::guide_in(&mut out, Path::new("/nonexistent/madoqua"), Some(Topic::Tune)).unwrap();
        assert_eq!(outcome, Outcome::Clean, "the guide is an inventory, never a verdict");
        assert_eq!(
            String::from_utf8(out).unwrap(),
            guide::render(Topic::Tune, guide::Selection::Explicit),
        );
    }

    /// `guide.md` promises `2` when the output cannot be written, and a
    /// promise nothing exercises is a guess.
    #[test]
    fn a_guide_that_cannot_be_written_could_not_complete() {
        struct Broken;

        impl Write for Broken {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let err = Cli::guide_in(&mut Broken, Path::new("."), Some(Topic::Tune))
            .expect_err("a write that fails is madoqua unable to do its job");
        assert!(
            format!("{err:#}").contains("cannot write to stdout"),
            "the error must name the operation, got: {err:#}"
        );
    }

    #[test]
    fn no_topic_is_resolved_from_the_directory_it_is_pointed_at() {
        let dir = tempfile::tempdir().unwrap();
        let mut out = Vec::new();
        Cli::guide_in(&mut out, dir.path(), None).unwrap();
        let printed = String::from_utf8(out).unwrap();
        assert!(
            printed.starts_with("# madoqua guide: not configured here -> setup"),
            "an empty directory gets setup, and is told why: {printed}",
        );

        std::fs::write(dir.path().join("pyproject.toml"), "[tool.madoqua]\n").unwrap();
        let mut out = Vec::new();
        Cli::guide_in(&mut out, dir.path(), None).unwrap();
        let printed = String::from_utf8(out).unwrap();
        assert!(
            printed.starts_with("# madoqua guide: configured via pyproject.toml"),
            "a configured repo gets triage, and is told why: {printed}",
        );
    }
}
