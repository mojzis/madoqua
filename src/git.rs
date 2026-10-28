//! The git seam.
//!
//! Every `git` invocation in the crate is in this file. Callers get parsed
//! data — a path, a list of paths, a short sha — and never a `Command`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, bail};

/// The working tree root of the repository containing `start`.
pub fn repo_root(start: &Path) -> Result<PathBuf> {
    let output = run(start, &["rev-parse", "--show-toplevel"])?;
    let text = stdout_text(&output);
    let line = text.trim();
    if line.is_empty() {
        bail!("git did not report a repository root for {}", start.display());
    }
    Ok(PathBuf::from(line))
}

/// Git's common directory for the repository at `root`: the one directory
/// every worktree of a clone shares.
///
/// `<root>/.git` for an ordinary checkout. In a linked worktree `<root>/.git`
/// is a *file* naming the metadata directory instead, so anything madoqua
/// keeps "in `.git`" has to be resolved through git rather than joined onto
/// the working tree — a joined path is a path through a file, and reading it
/// fails with `Not a directory` before a single check has run.
///
/// `--git-common-dir` answers relative to the working directory, which is why
/// this is run from `root` and not from wherever the caller stood: the answer
/// is then `.git` for a clone and an absolute path for a worktree.
pub fn common_dir(root: &Path) -> Result<PathBuf> {
    let output = run(root, &["rev-parse", "--git-common-dir"])?;
    let text = stdout_text(&output);
    let line = text.trim();
    if line.is_empty() {
        bail!("git did not report a git directory for {}", root.display());
    }
    Ok(root.join(line))
}

/// Every staged file, relative to the repository root, in git's order.
///
/// All of them, not just Python: which ones a step runs on is the step's `on`
/// list, decided downstream over this list rather than by a pathspec here.
/// `--diff-filter=ACMR` skips deletions — running a formatter on a path that
/// is no longer there would fail for an uninteresting reason — and `-z` keeps
/// paths with spaces or newlines in them intact.
pub fn staged_files(root: &Path) -> Result<Vec<String>> {
    let output = run(root, &["diff", "--cached", "--name-only", "-z", "--diff-filter=ACMR"])?;
    Ok(nul_separated(&output))
}

/// Every file a commit could contain, relative to the repository root: the
/// tracked files and the untracked ones git does not ignore, minus those not
/// on disk, sorted by path.
///
/// This is the whole-repo run's answer to [`staged_files`]. Untracked files
/// are in it because `ruff check .` would see them, and a new module is
/// exactly the file nobody has staged yet. Files deleted from the working
/// tree, and files a sparse checkout left out of it, are not: a tool handed a
/// path that is not there fails for an uninteresting reason, which is why
/// `--diff-filter=ACMR` leaves deletions out of the staged list too.
pub fn repo_files(root: &Path) -> Result<Vec<String>> {
    // `-t` tags each path; `S` is skip-worktree, which is how a sparse
    // checkout marks a tracked file it did not write, and which `--deleted`
    // does not report.
    let listed =
        run(root, &["ls-files", "-z", "-t", "--cached", "--others", "--exclude-standard"])?;
    let deleted = run(root, &["ls-files", "-z", "--deleted"])?;
    let deleted: HashSet<String> = nul_separated(&deleted).into_iter().collect();

    let mut files: Vec<String> = nul_separated(&listed)
        .into_iter()
        .filter_map(|entry| match entry.split_once(' ') {
            Some(("S", _)) | None => None,
            Some((_, path)) => Some(path.to_owned()),
        })
        .collect();
    files.sort_unstable();
    // A conflicted path is listed once per stage it has.
    files.dedup();
    files.retain(|path| !deleted.contains(path));
    Ok(files)
}

fn nul_separated(output: &Output) -> Vec<String> {
    String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Re-stage the files the fix phase may have rewritten.
pub fn add(root: &Path, files: &[String]) -> Result<()> {
    if files.is_empty() {
        return Ok(());
    }
    let mut args = vec!["add", "--"];
    args.extend(files.iter().map(String::as_str));
    run(root, &args)?;
    Ok(())
}

/// The short sha of `HEAD` — the parent of the commit being made, since that
/// commit does not exist yet. `None` in a repository with no commits.
pub fn head_short_sha(root: &Path) -> Option<String> {
    let output = run(root, &["rev-parse", "--short", "HEAD"]).ok()?;
    let sha = stdout_text(&output).trim().to_owned();
    (!sha.is_empty()).then_some(sha)
}

/// Point git at the `hooks/` directory in the working tree.
pub fn set_hooks_path(root: &Path, value: &str) -> Result<()> {
    run(root, &["config", "core.hooksPath", value])?;
    Ok(())
}

/// Run git in `cwd` and fail with its stderr when it does.
fn run(cwd: &Path, args: &[&str]) -> Result<Output> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("cannot run `git {}`", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("`git {}` failed: {}", args.join(" "), stderr.trim());
    }
    Ok(output)
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}
