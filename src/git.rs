//! The git seam.
//!
//! Every `git` invocation in the crate is in this file. Callers get parsed
//! data — a path, a list of paths, a short sha — and never a `Command`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, bail};

/// The extensions the hook considers its business, matching the bash version's
/// pathspec exactly.
const PY_PATHSPECS: [&str; 2] = ["*.py", "*.pyi"];

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

/// The staged Python files, relative to the repository root.
///
/// `--diff-filter=ACMR` skips deletions — running a formatter on a path that
/// is no longer there would fail for an uninteresting reason — and `-z` keeps
/// paths with spaces or newlines in them intact.
pub fn staged_python_files(root: &Path) -> Result<Vec<String>> {
    let mut args = vec!["diff", "--cached", "--name-only", "-z", "--diff-filter=ACMR", "--"];
    args.extend(PY_PATHSPECS);

    let output = run(root, &args)?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(ToOwned::to_owned)
        .collect())
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
