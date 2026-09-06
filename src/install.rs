//! `madoqua install`: put a four-line shim in `hooks/pre-commit` and point git
//! at it.
//!
//! The shim lives in the working tree rather than in `.git/hooks` so it can be
//! committed and everyone on the repo gets the same hook. Its only logic is
//! where to find the binary, for the same reason the bash version is being
//! retired: everything else is fixed by upgrading the binary.
//!
//! It looks in `<repo>/.venv/bin` before `PATH` because git runs hooks with
//! the login `PATH`, not the shell's. madoqua installed as a dev dependency is
//! on `PATH` only while the venv is active, and a fish login shell, an IDE's
//! git or CI never activates it; the guard in `hook.rs` cannot help with that
//! because it runs inside a madoqua that was never found.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The directory `core.hooksPath` is pointed at.
pub const HOOKS_DIR: &str = "hooks";

/// What the shim contains.
///
/// `sh`, not `bash`, because there is nothing here that needs more than `sh`.
/// `git rev-parse` rather than `.venv` relative to the cwd, so the shim does
/// not depend on where git chose to run it from.
pub const SHIM: &str = "#!/bin/sh\n\
root=$(git rev-parse --show-toplevel)\n\
[ -x \"$root/.venv/bin/madoqua\" ] && exec \"$root/.venv/bin/madoqua\" run\n\
exec madoqua run\n";

/// Write the shim and configure git. Running it twice changes nothing.
pub fn install(root: &Path) -> Result<PathBuf> {
    let dir = root.join(HOOKS_DIR);
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;

    let shim = dir.join("pre-commit");
    std::fs::write(&shim, SHIM).with_context(|| format!("cannot write {}", shim.display()))?;
    make_executable(&shim)?;

    crate::git::set_hooks_path(root, HOOKS_DIR)?;
    Ok(shim)
}

fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .with_context(|| format!("cannot make {} executable", path.display()))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}
