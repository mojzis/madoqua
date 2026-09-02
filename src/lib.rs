//! madoqua — a pre-commit hook runner for Python repos: fix, stage, check,
//! and say one line about it.
//!
//! The crate is split so that the rules are testable without touching the
//! world. [`hook`] is the pipeline: it decides what the run means and what to
//! say about it, and it reaches the world only through the seams. The seams
//! are named after what they touch: [`git`] is every `git` invocation,
//! [`runner`] is every other process, [`venv`] is `PATH`, [`timelog`] is the
//! log file, [`clock`] is the wall clock, [`config`] is everywhere the
//! configuration is written down, [`install`] writes the shim, and
//! [`guide`] reads a directory to see whether madoqua is wired into it.
//! [`stats`] and [`report`] touch nothing at all.
//!
//! See `docs/dev/ARCHITECTURE.md`, and `docs/adr/` for why the shape is what
//! it is.

pub mod cli;
pub mod clock;
pub mod config;
pub mod git;
pub mod guide;
pub mod hook;
pub mod install;
pub mod report;
pub mod runner;
pub mod stats;
pub mod timelog;
pub mod venv;
