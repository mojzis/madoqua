//! madoqua — a pre-commit hook runner for Python repos: fix, stage, check,
//! and say one line about it.
//!
//! The crate is split so that the rules are testable without touching the
//! world. [`config`] decides what to run, [`hook`] decides what to say,
//! [`stats`] decides what the log means — none of them spawn anything. The
//! impure parts are named after what they touch: [`git`] is every `git`
//! invocation, [`runner`] is every other process, [`venv`] is `PATH`,
//! [`timelog`] is the log file, [`clock`] is the wall clock.
//!
//! See `docs/dev/ARCHITECTURE.md`, and `docs/adr/` for why the shape is what
//! it is.

pub mod cli;
pub mod clock;
pub mod config;
pub mod git;
pub mod hook;
pub mod install;
pub mod report;
pub mod runner;
pub mod stats;
pub mod timelog;
pub mod venv;
