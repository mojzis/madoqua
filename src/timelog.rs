//! The timing log: one JSON object per run, appended to a `.jsonl` file.
//!
//! The default lives in the repository's git directory, where it is per-project
//! and gets thrown away with the clone. Pointing `log` at a path under `$HOME`
//! collects every repo's runs in one place instead, and records written there
//! carry a `repo` field so [`crate::stats`] can tell them apart.
//!
//! Writing the log is best-effort by design: a hook that blocks a commit
//! because it could not write a timing record would be a worse tool than one
//! that has no timings.

use std::io::Write;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};

use crate::report::RunRecord;

/// What the log is called when nothing says otherwise.
///
/// It sits in the repository's git directory — `<root>/.git` for an ordinary
/// checkout, and the clone's `.git` for every one of its linked worktrees, so
/// one repository has one history however many worktrees it is checked out
/// in.
pub const DEFAULT_LOG_NAME: &str = "hook-timings.jsonl";

/// A resolved log destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogTarget {
    /// The absolute path to append to.
    pub path: PathBuf,
    /// The repository's name, set only when the log is shared between repos.
    pub repo: Option<String>,
}

/// Resolve the configured log path for the working tree at `root`, whose git
/// directory is `git_dir`.
///
/// With nothing configured the log is [`DEFAULT_LOG_NAME`] in the git
/// directory. Otherwise `~` expands to `home` and a relative path is relative
/// to the repository root — the working tree is what a developer writing
/// `build/timings.jsonl` is thinking about.
///
/// A path that lands outside the repository turns on the `repo` field.
/// "Outside" means outside both directories: a linked worktree's git directory
/// is not under its working tree, and the runs of every worktree of one clone
/// are the runs of one repository, with nothing to disambiguate.
pub fn target(
    root: &Path,
    git_dir: &Path,
    configured: Option<&str>,
    home: Option<&Path>,
) -> LogTarget {
    let git_dir = normalise(git_dir);
    let path = match configured {
        None => git_dir.join(DEFAULT_LOG_NAME),
        Some(raw) => {
            let expanded = expand_home(raw, home);
            normalise(&if expanded.is_absolute() { expanded } else { root.join(expanded) })
        }
    };

    let inside = path.starts_with(root) || path.starts_with(&git_dir);
    let repo = (!inside)
        .then(|| root.file_name().map(|name| name.to_string_lossy().into_owned()))
        .flatten();

    LogTarget { path, repo }
}

/// Fold `.` and `..` away without touching the filesystem.
///
/// `Path::starts_with` compares components, so `<root>/../shared.jsonl` starts
/// with `root` and would be taken for a log inside the repository — leaving
/// every record in a genuinely shared log without a `repo` field, and
/// `stats --repo` with nothing to report and nothing to say about why.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            // Popping past the root, or past a leading `..`, would invent a
            // path; keep the component instead.
            Component::ParentDir if out.components().next_back().is_some_and(is_named) => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

fn is_named(component: Component<'_>) -> bool {
    matches!(component, Component::Normal(_))
}

/// [`target`] against the real environment.
pub fn resolve(root: &Path, git_dir: &Path, configured: Option<&str>) -> LogTarget {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    target(root, git_dir, configured, home.as_deref())
}

fn expand_home(raw: &str, home: Option<&Path>) -> PathBuf {
    let Some(home) = home else { return PathBuf::from(raw) };
    match raw {
        "~" => home.to_path_buf(),
        _ => match raw.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => PathBuf::from(raw),
        },
    }
}

/// Append one record, creating the parent directories if they are missing.
pub fn append(target: &LogTarget, record: &RunRecord) -> Result<()> {
    if let Some(parent) = target.path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }

    let mut line = serde_json::to_string(record).context("cannot serialise the run record")?;
    line.push('\n');

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&target.path)
        .with_context(|| format!("cannot open {}", target.path.display()))?;

    file.write_all(line.as_bytes())
        .with_context(|| format!("cannot write to {}", target.path.display()))
}

/// Read every record the log holds, skipping lines that will not parse.
///
/// A log is appended to by whichever madoqua version is installed at the time,
/// so a line from a future schema is something to step over rather than
/// something to fail on.
pub fn read(path: &Path) -> Result<Vec<RunRecord>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("cannot read {}", path.display())),
    };

    Ok(text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/dev";

    fn root() -> PathBuf {
        PathBuf::from("/home/dev/src/project")
    }

    /// The git directory of an ordinary checkout of [`root`].
    fn git_dir() -> PathBuf {
        root().join(".git")
    }

    /// [`target`] for an ordinary checkout, where the two directories are one.
    fn in_clone(configured: Option<&str>) -> LogTarget {
        target(&root(), &git_dir(), configured, Some(Path::new(HOME)))
    }

    fn record() -> RunRecord {
        RunRecord {
            ts: "2026-08-31T12:03:22Z".to_owned(),
            repo: None,
            head: "abc1234".to_owned(),
            files: 1,
            total_ms: 10,
            venv_auto_activated: false,
            steps: Vec::new(),
        }
    }

    #[test]
    fn the_default_log_lives_in_the_git_directory_and_needs_no_repo_field() {
        let target = in_clone(None);
        assert_eq!(target.path, git_dir().join(DEFAULT_LOG_NAME));
        assert_eq!(target.repo, None, "a per-repo log does not need to say which repo");
    }

    /// A linked worktree's git directory belongs to the clone and sits outside
    /// the worktree entirely. Joining `.git/` onto the working tree would aim
    /// the log at a path through a *file*, which is the whole bug.
    #[test]
    fn a_worktrees_default_log_goes_to_the_clones_git_directory() {
        let worktree = PathBuf::from("/home/dev/src/task-42");
        let target = target(&worktree, &git_dir(), None, Some(Path::new(HOME)));

        assert_eq!(target.path, git_dir().join(DEFAULT_LOG_NAME));
        assert_eq!(
            target.repo, None,
            "every worktree of a clone is the same repository, so there is nothing to \
             disambiguate — one repo, one history, however many checkouts of it there are"
        );
    }

    #[test]
    fn a_worktrees_relative_log_is_still_relative_to_its_own_working_tree() {
        let worktree = PathBuf::from("/home/dev/src/task-42");
        let target = target(&worktree, &git_dir(), Some("build/t.jsonl"), Some(Path::new(HOME)));

        assert_eq!(
            target.path,
            worktree.join("build/t.jsonl"),
            "a configured relative path is about the files being checked, not the metadata"
        );
        assert_eq!(target.repo, None, "and it landed inside the checkout it came from");
    }

    #[test]
    fn a_tilde_path_expands_and_turns_on_the_repo_field() {
        let target = in_clone(Some("~/.local/state/madoqua/timings.jsonl"));
        assert_eq!(target.path, PathBuf::from("/home/dev/.local/state/madoqua/timings.jsonl"));
        assert_eq!(
            target.repo.as_deref(),
            Some("project"),
            "a shared log mixes repos, so each line has to name its own"
        );
    }

    #[test]
    fn a_path_that_climbs_out_of_the_repo_still_names_the_repo() {
        let target = in_clone(Some("../shared.jsonl"));
        assert_eq!(target.path, PathBuf::from("/home/dev/src/shared.jsonl"));
        assert_eq!(
            target.repo.as_deref(),
            Some("project"),
            "`..` is how sibling checkouts share one log, and a shared log has to name its repos"
        );
    }

    #[test]
    fn a_path_that_climbs_out_and_back_in_is_inside_the_repo() {
        let target = in_clone(Some("../project/build/t.jsonl"));
        assert_eq!(target.path, root().join("build/t.jsonl"));
        assert_eq!(target.repo, None, "the long way round still lands in the same repo");
    }

    #[test]
    fn a_relative_path_is_relative_to_the_repo_root() {
        let target = in_clone(Some("build/timings.jsonl"));
        assert_eq!(target.path, root().join("build/timings.jsonl"));
        assert_eq!(target.repo, None);
    }

    #[test]
    fn an_absolute_path_outside_the_repo_names_the_repo() {
        let target = in_clone(Some("/var/log/madoqua.jsonl"));
        assert_eq!(target.path, PathBuf::from("/var/log/madoqua.jsonl"));
        assert_eq!(target.repo.as_deref(), Some("project"));
    }

    #[test]
    fn a_bare_tilde_is_the_home_directory() {
        let target = in_clone(Some("~"));
        assert_eq!(target.path, PathBuf::from(HOME));
    }

    #[test]
    fn without_a_home_a_tilde_stays_literal_rather_than_guessing() {
        let target = target(&root(), &git_dir(), Some("~/timings.jsonl"), None);
        assert_eq!(target.path, root().join("~/timings.jsonl"));
    }

    #[test]
    fn appending_creates_missing_parents_and_keeps_earlier_lines() {
        let dir = tempfile::tempdir().unwrap();
        let target = LogTarget { path: dir.path().join("deep/nested/log.jsonl"), repo: None };

        append(&target, &record()).unwrap();
        append(&target, &record()).unwrap();

        let records = read(&target.path).unwrap();
        assert_eq!(records.len(), 2, "the log appends rather than replaces");
        assert_eq!(records[0], record(), "and what comes back is what went in");
    }

    #[test]
    fn reading_a_missing_log_is_an_empty_history_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            read(&dir.path().join("nope.jsonl")).unwrap(),
            Vec::new(),
            "stats before the first commit is a fair thing to ask for"
        );
    }

    #[test]
    fn an_unreadable_line_is_stepped_over() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.jsonl");
        let good = serde_json::to_string(&record()).unwrap();
        std::fs::write(&path, format!("not json\n{good}\n{{\"from\":\"the future\"}}\n")).unwrap();

        assert_eq!(read(&path).unwrap(), vec![record()], "one bad line must not lose the rest");
    }
}
