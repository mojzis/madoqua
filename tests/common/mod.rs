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
use assert_cmd::assert::Assert;
use tempfile::TempDir;

/// A throwaway checkout with a stub virtualenv: either a git repository of its
/// own, or a linked worktree of one.
///
/// `root` is the working tree the tests drive; it is the temporary directory
/// itself for a clone, and a subdirectory of it for a worktree, since
/// `git worktree add` insists on creating the directory it is given.
pub struct Repo {
    dir: TempDir,
    root: PathBuf,
}

impl Repo {
    /// An initialised repository with one commit, so `HEAD` resolves.
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("a temp dir");
        let root = dir.path().to_path_buf();
        let repo = Self { dir, root };
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
        &self.root
    }

    /// A linked worktree of this repository, checked out on a new branch.
    ///
    /// Its `.git` is a *file* pointing into this clone's metadata directory,
    /// which is the whole point: every path madoqua resolves under `.git` has
    /// to survive that. The stub venv is copied rather than inherited, because
    /// `.venv` is ignored and a fresh checkout of the repository has none.
    ///
    /// The repository it came from has to outlive the worktree, which in a
    /// test means holding on to both.
    pub fn worktree(&self, branch: &str) -> Self {
        let dir = tempfile::tempdir().expect("a temp dir");
        let root = dir.path().join("wt");
        self.git(&["worktree", "add", "-q", "-b", branch, root.to_str().expect("a utf-8 path")]);

        let worktree = Self { dir, root };
        copy_dir(&self.path().join(".venv/bin"), &worktree.path().join(".venv/bin"));
        worktree
    }

    /// Git's common directory for this checkout: `<root>/.git` for a clone,
    /// and the clone's `.git` for a linked worktree.
    pub fn git_dir(&self) -> PathBuf {
        let reported = self.git(&["rev-parse", "--git-common-dir"]);
        self.path().join(reported.trim())
    }

    fn with_venv(&self) -> &Self {
        self.tool("python", "exit 0");
        self.write(".venv/bin/activate", "# stub\n");
        self
    }

    /// Write an executable stub into the venv's `bin`, where the hook will
    /// find it once the guard has put that directory on `PATH`.
    pub fn tool(&self, name: &str, body: &str) -> &Self {
        write_executable(
            &self.path().join(".venv/bin").join(name),
            &format!("#!/bin/sh\n{body}\n"),
        );
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

    /// Commit through git, with the built binary on `PATH` so the shim's
    /// `exec madoqua run` resolves. Returns the raw output: whether the commit
    /// was allowed through is the thing under test.
    pub fn commit(&self, message: &str) -> std::process::Output {
        self.commit_with_path(message, &path_with_binary())
    }

    /// Commit through git with an explicit `PATH`, for tests about what the
    /// shim resolves `madoqua` to.
    pub fn commit_with_path(&self, message: &str, path: &str) -> std::process::Output {
        self.git_command()
            .args(["commit", "-m", message])
            .env("PATH", path)
            .output()
            .expect("git is runnable")
    }

    /// Where the default timing log lives for this checkout.
    pub fn log_path(&self) -> PathBuf {
        self.git_dir().join("hook-timings.jsonl")
    }

    /// The parsed timing log, one entry per line.
    pub fn log_records(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.log_path())
            .unwrap_or_default()
            .lines()
            .map(|line| {
                serde_json::from_str(line)
                    .unwrap_or_else(|err| panic!("bad log line: {err}\n{line}"))
            })
            .collect()
    }

    pub fn log_exists(&self) -> bool {
        self.log_path().exists()
    }
}

/// Copy a flat directory's files, keeping their permissions.
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        std::fs::copy(entry.path(), &target).unwrap();
        std::fs::set_permissions(&target, entry.metadata().unwrap().permissions()).unwrap();
    }
}

/// Write `content` to `path`, creating parents, and mark it executable.
pub fn write_executable(path: &Path, content: &str) {
    Repo::write_path(path, content);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
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

/// A finished command's stdout.
pub fn stdout(assert: &Assert) -> String {
    String::from_utf8_lossy(&assert.get_output().stdout).into_owned()
}

/// A finished command's stderr.
pub fn stderr(assert: &Assert) -> String {
    String::from_utf8_lossy(&assert.get_output().stderr).into_owned()
}

/// The single line a clean run prints, with the trailing newline removed.
pub fn only_line(stdout: &[u8]) -> String {
    let text = String::from_utf8_lossy(stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1, "a clean run says one thing; got: {text:?}");
    lines[0].to_owned()
}
