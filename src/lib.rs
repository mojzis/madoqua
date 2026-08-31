//! madoqua — TODO: one-line description.
//!
//! The crate is split so that the rules are testable without touching the
//! world: [`cli`] holds the command bodies, [`config`] reads project settings,
//! [`report`] renders the wire format. Anything that spawns a process or reads
//! the filesystem lives behind a named seam, and everything downstream of a
//! seam takes already-parsed data.
//!
//! See `docs/dev/ARCHITECTURE.md`, and `docs/adr/` for why the shape is what
//! it is.

pub mod cli;
pub mod config;
pub mod report;
