//! Shared helpers for the integration tests.
//!
//! Integration tests drive the built binary against a real temporary git
//! repository, so they exercise argument parsing, exit codes, the git
//! invocations and the log — the parts unit tests deliberately cannot see.
//!
//! The repository gets a stub `.venv` whose `bin` directory is where the fake
//! tools live. That is not a shortcut: it means the venv guard's activation
//! path is what puts those tools on `PATH`, so every test that runs a tool
//! also proves the guard works.

#![allow(dead_code, reason = "each integration test file uses a different subset")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "these are test helpers; a failed setup step should abort loudly"
)]

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use assert_cmd::Command;
use tempfile::TempDir;

/// A throwaway git repository with a stub virtualenv.
pub struct Repo {
    dir: TempDir,
}

impl Repo {
    /// An initialised repository with one commit, so `HEAD` resolves.
    pub fn new() -> Self {
        let repo = Self { dir: tempfile::tempdir().expect("a temp dir") };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.email", "test@example.invalid"]);
        repo.git(&["config", "user.name", "madoqua tests"]);
        repo.write(".gitignore", ".venv/\n");
        repo.git(&["add", ".gitignore"]);
        repo.git(&["commit", "-qm", "base"]);
        repo.with_venv();
        repo
    }

    /// The same, minus the virtualenv — for testing the guard's failure.
    pub fn without_venv() -> Self {
        let repo = Self::new();
        std::fs::remove_dir_all(repo.path().join(".venv")).unwrap();
        repo
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    fn with_venv(&self) -> &Self {
        self.tool("python", "exit 0");
        self.write(".venv/bin/activate", "# stub\n");
        self
    }

    /// Write an executable stub into the venv's `bin`, where the hook will
    /// find it once the guard has put that directory on `PATH`.
    pub fn tool(&self, name: &str, body: &str) -> &Self {
        let path = self.path().join(".venv/bin").join(name);
        Self::write_path(&path, &format!("#!/bin/sh\n{body}\n"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        self
    }

    pub fn write(&self, relative: &str, content: &str) -> &Self {
        Self::write_path(&self.path().join(relative), content);
        self
    }

    fn write_path(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().expect("a parent directory")).unwrap();
        std::fs::write(path, content).unwrap();
    }

    /// Write a file and stage it, which is the state the hook cares about.
    pub fn stage(&self, relative: &str, content: &str) -> &Self {
        self.write(relative, content);
        self.git(&["add", "--", relative]);
        self
    }

    pub fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.path().join(relative)).unwrap_or_default()
    }

    /// The content of a path as it is currently staged.
    pub fn staged_content(&self, relative: &str) -> String {
        self.git(&["show", &format!(":{relative}")])
    }

    /// Run git in the repository and return its stdout, panicking on failure.
    pub fn git(&self, args: &[&str]) -> String {
        let output = self
            .git_command()
            .args(args)
            .output()
            .unwrap_or_else(|err| panic!("cannot run git {args:?}: {err}"));
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// A git invocation isolated from the developer's own git configuration,
    /// which could otherwise install hooks or rename branches under the tests.
    pub fn git_command(&self) -> StdCommand {
        let mut cmd = StdCommand::new("git");
        cmd.current_dir(self.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null");
        cmd
    }

    /// A `madoqua` invocation rooted at this repository.
    pub fn madoqua(&self) -> Command {
        madoqua(self.path())
    }

    /// The parsed timing log, one entry per line.
    pub fn log_records(&self) -> Vec<serde_json::Value> {
        self.read(".git/hook-timings.jsonl")
            .lines()
            .map(|line| {
                serde_json::from_str(line)
                    .unwrap_or_else(|err| panic!("bad log line: {err}\n{line}"))
            })
            .collect()
    }

    pub fn log_exists(&self) -> bool {
        self.path().join(".git/hook-timings.jsonl").exists()
    }
}

/// A `madoqua` invocation rooted at `dir`, with a clean environment.
///
/// `RUST_LOG` and `MADOQUA_SKIP` are cleared so a developer's shell settings
/// cannot change what the assertions see.
pub fn madoqua(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("madoqua").expect("the binary is built by `cargo test`");
    cmd.current_dir(dir).env_remove("RUST_LOG").env_remove("MADOQUA_SKIP");
    cmd
}

/// The directory the built binary lives in, for tests that need `madoqua` on
/// `PATH` — the shim `git` runs says `exec madoqua run`.
pub fn binary_dir() -> PathBuf {
    let binary = assert_cmd::cargo::cargo_bin("madoqua");
    binary.parent().expect("the binary has a parent directory").to_path_buf()
}

/// `PATH` with the built binary's directory in front.
pub fn path_with_binary() -> String {
    let existing = std::env::var("PATH").unwrap_or_default();
    format!("{}:{existing}", binary_dir().display())
}

/// The single line a clean run prints, with the trailing newline removed.
pub fn only_line(stdout: &[u8]) -> String {
    let text = String::from_utf8_lossy(stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1, "a clean run says one thing; got: {text:?}");
    lines[0].to_owned()
}
