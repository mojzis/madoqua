//! Shared helpers for the integration tests.
//!
//! Integration tests drive the built binary through `assert_cmd`, so they
//! exercise argument parsing and exit codes — the parts unit tests cannot see.
//! Anything a test needs in more than one file belongs here.

#![allow(dead_code, reason = "each integration test file uses a different subset")]
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "these are test helpers; a failed setup step should abort loudly"
)]

use std::path::Path;

use assert_cmd::Command;

/// A `madoqua` invocation rooted at `dir`, with a clean environment.
///
/// `RUST_LOG` is cleared so a developer's shell setting cannot change what the
/// assertions see on stderr.
pub fn madoqua(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("madoqua").expect("the binary is built by `cargo test`");
    cmd.current_dir(dir).env_remove("RUST_LOG");
    cmd
}
