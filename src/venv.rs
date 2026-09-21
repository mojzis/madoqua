//! The virtualenv guard, and the environment child processes inherit.
//!
//! The hook refuses to run tools from outside the repo's `.venv`: a `ruff`
//! picked up from the system would lint with a different version than CI, and
//! the failure would look like the code's fault. "Activating" here is not
//! sourcing anything — it is prepending `.venv/bin` to the `PATH` the child
//! processes get, and setting `VIRTUAL_ENV`, which is all `activate` does that
//! a child can observe.
//!
//! Both happen on every run, not only when the guard finds the venv missing
//! from `PATH`. Where `python` resolves says nothing about where `pytest`
//! does: a half-activated shell can have the venv's interpreter in front and a
//! version manager's directory in front of *that*, so a check — or a process a
//! check spawns by name — picks up the wrong tool.
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
    /// The `PATH` to run tools with, `.venv/bin` first.
    pub path: OsString,
    /// `VIRTUAL_ENV`, always the repo's venv: the guard only passes when that
    /// is the venv the tools will run from, so there is nothing else to say.
    pub virtual_env: PathBuf,
}

/// A passing guard: which environment to use, and whether we had to fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Venv {
    /// True when `.venv/bin` was not already the front of `PATH` and we put it
    /// there. The verdict says so, because it means the developer's shell is
    /// not set up the way they probably think it is.
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
/// `<root>/.venv/bin` goes to the front of `PATH` unless it is there already,
/// and `VIRTUAL_ENV` is always set: a `python` that resolves into the venv is
/// not evidence that anything else does. The venv has to exist, and it has to
/// win once it is in front — an activation that does not actually win is a
/// failure, not a shrug.
pub fn plan(root: &Path, path_value: &OsStr, fs: &impl Fs) -> Result<Venv, VenvError> {
    let venv = root.join(".venv");
    let bin = venv.join("bin");
    let python = bin.join("python");

    // Whether a venv is there at all is only worth asking when `PATH` does not
    // already reach its python: a venv whose interpreter is running is a venv,
    // whatever `activate` scripts it was or was not built with.
    if !resolves_to(path_value, &python, fs) && !fs.exists(&bin.join("activate")) {
        return Err(VenvError(format!(
            "madoqua: no virtualenv at {}\n  \
             create one and install the tools the hook runs:\n      \
             uv venv && uv sync\n  \
             (or: python -m venv .venv && .venv/bin/pip install -e '.[dev]')",
            venv.display()
        )));
    }

    // The rest of `PATH` is kept, in order, behind the venv. An entry that is
    // now a duplicate of `.venv/bin` costs a stat and changes no lookup;
    // dropping entries would change what tools a check can still reach.
    let leads = leads_with(path_value, &bin, fs);
    let path = if leads { path_value.to_os_string() } else { prepend(&bin, path_value) };

    if !resolves_to(&path, &python, fs) {
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

    Ok(Venv { auto_activated: !leads, env: ChildEnv { path, virtual_env: venv } })
}

/// The guard against the real filesystem and the real `PATH`.
pub fn guard(root: &Path) -> Result<Venv, VenvError> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    plan(root, &path, &RealFs)
}

/// Is the `python` on `path_value` the one at `expected`?
///
/// Compared by the directory it sits in, not by the file it links to: `uv venv`
/// symlinks `.venv/bin/python` to the interpreter it built from, so following
/// the link would make `/usr/bin/python` and the venv's python the same file —
/// and only one of them sits next to `ruff`. The directory is canonicalised so
/// a repo reached through a symlinked root still counts as active.
fn resolves_to(path_value: &OsStr, expected: &Path, fs: &impl Fs) -> bool {
    let Some(found) = which(path_value, "python", fs) else {
        return false;
    };
    if found == expected {
        return true;
    }
    match (found.parent(), expected.parent()) {
        (Some(found_dir), Some(expected_dir)) => {
            fs.canonical(found_dir) == fs.canonical(expected_dir)
        }
        _ => false,
    }
}

/// The first executable named `program` on `path_value`, as `command -v` would
/// report it.
fn which(path_value: &OsStr, program: &str, fs: &impl Fs) -> Option<PathBuf> {
    std::env::split_paths(path_value)
        .map(|dir| dir.join(program))
        .find(|candidate| fs.is_executable(candidate))
}

/// Is `dir` already the first entry on `path_value`?
///
/// Canonicalised on both sides, so a repo reached through a symlinked root
/// counts as leading rather than being prepended a second time under its real
/// name — which, for a `PATH` that is already right, would be noise.
fn leads_with(path_value: &OsStr, dir: &Path, fs: &impl Fs) -> bool {
    std::env::split_paths(path_value)
        .next()
        .is_some_and(|first| first == dir || fs.canonical(&first) == fs.canonical(dir))
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
    fn an_already_leading_venv_is_left_alone_but_still_exports_virtual_env() {
        let fs = FakeFs::new(&["/repo/.venv/bin/python"], &["/repo/.venv/bin/activate"]);
        let venv = plan(Path::new(ROOT), OsStr::new("/repo/.venv/bin:/usr/bin"), &fs).unwrap();

        assert!(!venv.auto_activated, "PATH was already right, so there is nothing to announce");
        assert_eq!(venv.env.path, OsString::from("/repo/.venv/bin:/usr/bin"), "PATH is untouched");
        assert_eq!(
            venv.env.virtual_env,
            PathBuf::from("/repo/.venv"),
            "children are told which venv they are in, whatever the shell claims"
        );
    }

    #[test]
    fn a_tool_shadowed_by_an_earlier_directory_is_still_taken_from_the_venv() {
        // The bug: `python` resolving to the venv says nothing about `pytest`.
        // A directory earlier on PATH that holds one but not the other used to
        // win, including for a tool a check spawns by name.
        let fs = FakeFs::new(
            &["/repo/.venv/bin/python", "/repo/.venv/bin/pytest", "/shadow/pytest"],
            &["/repo/.venv/bin/activate"],
        );
        let venv =
            plan(Path::new(ROOT), OsStr::new("/shadow:/repo/.venv/bin:/usr/bin"), &fs).unwrap();

        assert_eq!(
            which(&venv.env.path, "pytest", &fs),
            Some(PathBuf::from("/repo/.venv/bin/pytest")),
            "the venv's pytest is the one a child would find"
        );
        assert!(venv.auto_activated, "madoqua reordered PATH, and the verdict says so");
        assert_eq!(
            venv.env.path,
            OsString::from("/repo/.venv/bin:/shadow:/repo/.venv/bin:/usr/bin"),
            "the rest of PATH is preserved, in order, behind the venv"
        );
        assert_eq!(venv.env.virtual_env, PathBuf::from("/repo/.venv"));
    }

    #[test]
    fn a_venv_further_back_on_path_is_pulled_to_the_front() {
        let fs = FakeFs::new(
            &["/repo/.venv/bin/python", "/usr/bin/ruff"],
            &["/repo/.venv/bin/activate"],
        );
        let venv = plan(Path::new(ROOT), OsStr::new("/usr/bin:/repo/.venv/bin"), &fs).unwrap();

        assert_eq!(
            std::env::split_paths(&venv.env.path).next(),
            Some(PathBuf::from("/repo/.venv/bin")),
            "a venv that merely appears on PATH does not decide what runs"
        );
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
            venv.env.virtual_env,
            PathBuf::from("/repo/.venv"),
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
    fn a_venv_built_from_the_system_python_is_still_activated() {
        // `uv venv` symlinks `.venv/bin/python` to the interpreter it found,
        // often `/usr/bin/python3.X`. Following that link makes the system
        // python and the venv python the same file, but only one of them
        // sits next to `ruff`.
        struct Shared;
        impl Fs for Shared {
            fn is_executable(&self, path: &Path) -> bool {
                path == Path::new("/usr/bin/python") || path == Path::new("/repo/.venv/bin/python")
            }
            fn exists(&self, _path: &Path) -> bool {
                true
            }
            fn canonical(&self, path: &Path) -> PathBuf {
                if path.file_name().is_some_and(|n| n == "python") {
                    PathBuf::from("/usr/bin/python3.14")
                } else {
                    path.to_path_buf()
                }
            }
        }

        let venv = plan(Path::new(ROOT), OsStr::new("/usr/bin"), &Shared).unwrap();
        assert!(
            venv.auto_activated,
            "the system python is the venv's interpreter, but it is not the venv"
        );
        assert_eq!(venv.env.path, OsString::from("/repo/.venv/bin:/usr/bin"));
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
