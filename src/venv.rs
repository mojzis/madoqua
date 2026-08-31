//! The virtualenv guard, and the environment child processes inherit.
//!
//! The hook refuses to run tools from outside the repo's `.venv`: a `ruff`
//! picked up from the system would lint with a different version than CI, and
//! the failure would look like the code's fault. "Activating" here is not
//! sourcing anything — it is prepending `.venv/bin` to the `PATH` the child
//! processes get, and setting `VIRTUAL_ENV`, which is all `activate` does that
//! a child can observe.
//!
//! [`plan`] is pure: it takes the current `PATH` and a filesystem oracle, so
//! every branch is testable on a machine that has no virtualenv at all.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

/// The filesystem questions the guard needs answered.
pub trait Fs {
    /// Is there a file here, and may we execute it?
    fn is_executable(&self, path: &Path) -> bool;
    /// Does this path exist at all?
    fn exists(&self, path: &Path) -> bool;
    /// The path with symlinks resolved, or the path itself if it cannot be.
    fn canonical(&self, path: &Path) -> PathBuf;
}

/// The real filesystem.
pub struct RealFs;

impl Fs for RealFs {
    fn is_executable(&self, path: &Path) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(path)
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        }
        #[cfg(not(unix))]
        {
            path.is_file()
        }
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn canonical(&self, path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }
}

/// The environment overrides every child process of the run receives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildEnv {
    /// The `PATH` to run tools with, `.venv/bin` first when we activated it.
    pub path: OsString,
    /// `VIRTUAL_ENV`, set only when we activated the venv ourselves.
    pub virtual_env: Option<PathBuf>,
}

/// A passing guard: which environment to use, and whether we had to fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Venv {
    /// True when `.venv` was not already on `PATH` and we put it there. The
    /// verdict says so, because it means the developer's shell is not set up
    /// the way they probably think it is.
    pub auto_activated: bool,
    /// What to hand to [`crate::runner`].
    pub env: ChildEnv,
}

/// A guard failure. Distinct from `anyhow::Error` because it is a verdict —
/// the commit is blocked — rather than an internal fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VenvError(String);

impl fmt::Display for VenvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for VenvError {}

/// Decide which environment to run in, given the current `PATH`.
///
/// If `python` already resolves to `<root>/.venv/bin/python`, nothing happens.
/// Otherwise, if the venv exists, `.venv/bin` goes to the front of `PATH` and
/// the check is repeated — an activation that does not actually win is a
/// failure, not a shrug.
pub fn plan(root: &Path, path_value: &OsStr, fs: &impl Fs) -> Result<Venv, VenvError> {
    let venv = root.join(".venv");
    let bin = venv.join("bin");
    let python = bin.join("python");

    if resolves_to(path_value, &python, fs) {
        return Ok(Venv {
            auto_activated: false,
            env: ChildEnv { path: path_value.to_os_string(), virtual_env: None },
        });
    }

    if !fs.exists(&bin.join("activate")) {
        return Err(VenvError(format!(
            "madoqua: no virtualenv at {}\n  \
             create one and install the tools the hook runs:\n      \
             uv venv && uv sync\n  \
             (or: python -m venv .venv && .venv/bin/pip install -e '.[dev]')",
            venv.display()
        )));
    }

    let activated = prepend(&bin, path_value);
    if !resolves_to(&activated, &python, fs) {
        return Err(VenvError(format!(
            "madoqua: {} exists but its python is not the one that would run\n  \
             expected: {}\n  \
             found:    {}\n  \
             the hook puts .venv/bin first on PATH; something there is shadowing it",
            venv.display(),
            python.display(),
            which(path_value, "python", fs)
                .map_or_else(|| "no python on PATH".to_owned(), |p| p.display().to_string()),
        )));
    }

    Ok(Venv { auto_activated: true, env: ChildEnv { path: activated, virtual_env: Some(venv) } })
}

/// The guard against the real filesystem and the real `PATH`.
pub fn guard(root: &Path) -> Result<Venv, VenvError> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    plan(root, &path, &RealFs)
}

fn resolves_to(path_value: &OsStr, expected: &Path, fs: &impl Fs) -> bool {
    which(path_value, "python", fs)
        .is_some_and(|found| found == expected || fs.canonical(&found) == fs.canonical(expected))
}

/// The first executable named `program` on `path_value`, as `command -v` would
/// report it.
fn which(path_value: &OsStr, program: &str, fs: &impl Fs) -> Option<PathBuf> {
    std::env::split_paths(path_value)
        .map(|dir| dir.join(program))
        .find(|candidate| fs.is_executable(candidate))
}

fn prepend(dir: &Path, path_value: &OsStr) -> OsString {
    let entries = std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(path_value));
    std::env::join_paths(entries).unwrap_or_else(|_| path_value.to_os_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// A filesystem described by the set of paths that exist, of which the
    /// executable ones are named separately.
    struct FakeFs {
        executables: HashSet<PathBuf>,
        files: HashSet<PathBuf>,
    }

    impl FakeFs {
        fn new(executables: &[&str], files: &[&str]) -> Self {
            Self {
                executables: executables.iter().map(PathBuf::from).collect(),
                files: files.iter().chain(executables.iter()).map(PathBuf::from).collect(),
            }
        }
    }

    impl Fs for FakeFs {
        fn is_executable(&self, path: &Path) -> bool {
            self.executables.contains(path)
        }
        fn exists(&self, path: &Path) -> bool {
            self.files.contains(path)
        }
        fn canonical(&self, path: &Path) -> PathBuf {
            path.to_path_buf()
        }
    }

    const ROOT: &str = "/repo";

    #[test]
    fn an_already_active_venv_is_left_alone() {
        let fs = FakeFs::new(&["/repo/.venv/bin/python"], &["/repo/.venv/bin/activate"]);
        let venv = plan(Path::new(ROOT), OsStr::new("/repo/.venv/bin:/usr/bin"), &fs).unwrap();

        assert!(!venv.auto_activated, "PATH was already right, so there is nothing to announce");
        assert_eq!(venv.env.path, OsString::from("/repo/.venv/bin:/usr/bin"), "PATH is untouched");
        assert_eq!(venv.env.virtual_env, None, "we did not activate, so we set nothing");
    }

    #[test]
    fn an_inactive_venv_is_activated_and_announced() {
        let fs = FakeFs::new(
            &["/repo/.venv/bin/python", "/usr/bin/python"],
            &["/repo/.venv/bin/activate"],
        );
        let venv = plan(Path::new(ROOT), OsStr::new("/usr/bin"), &fs).unwrap();

        assert!(venv.auto_activated, "the system python was first, so we had to intervene");
        assert_eq!(
            venv.env.path,
            OsString::from("/repo/.venv/bin:/usr/bin"),
            ".venv/bin goes in front, and the rest of PATH is kept"
        );
        assert_eq!(
            venv.env.virtual_env.as_deref(),
            Some(Path::new("/repo/.venv")),
            "children see VIRTUAL_ENV, as they would after sourcing activate"
        );
    }

    #[test]
    fn a_missing_venv_fails_with_instructions() {
        let fs = FakeFs::new(&["/usr/bin/python"], &[]);
        let err = plan(Path::new(ROOT), OsStr::new("/usr/bin"), &fs).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("/repo/.venv"), "the message must name the path, got: {message}");
        assert!(
            message.contains("uv venv"),
            "a guard that only says no is a worse guard, got: {message}"
        );
    }

    #[test]
    fn a_venv_without_a_python_fails_even_though_activate_exists() {
        let fs = FakeFs::new(&["/usr/bin/python"], &["/repo/.venv/bin/activate"]);
        let err = plan(Path::new(ROOT), OsStr::new("/usr/bin"), &fs).unwrap_err();
        let message = err.to_string();

        assert!(
            message.contains("/usr/bin/python"),
            "the message must name the python it found instead, got: {message}"
        );
        assert!(
            message.contains("/repo/.venv/bin/python"),
            "and the one it wanted, got: {message}"
        );
    }

    #[test]
    fn an_empty_path_with_a_venv_present_still_activates() {
        let fs = FakeFs::new(&["/repo/.venv/bin/python"], &["/repo/.venv/bin/activate"]);
        let venv = plan(Path::new(ROOT), OsStr::new(""), &fs).unwrap();

        assert!(venv.auto_activated);
        assert_eq!(
            venv.env.path,
            OsString::from("/repo/.venv/bin:"),
            "an empty PATH entry is preserved rather than silently rewritten"
        );
    }

    #[test]
    fn a_python_reached_through_a_symlinked_root_counts_as_active() {
        struct Linked;
        impl Fs for Linked {
            fn is_executable(&self, path: &Path) -> bool {
                path == Path::new("/link/.venv/bin/python")
            }
            fn exists(&self, _path: &Path) -> bool {
                true
            }
            fn canonical(&self, path: &Path) -> PathBuf {
                PathBuf::from(path.to_string_lossy().replace("/link/", "/repo/"))
            }
        }

        let venv = plan(Path::new(ROOT), OsStr::new("/link/.venv/bin"), &Linked).unwrap();
        assert!(
            !venv.auto_activated,
            "the same file reached by another name is still the right python"
        );
    }
}
