//! `madoqua install`: put a two-line shim in `hooks/pre-commit` and point git
//! at it.
//!
//! The shim lives in the working tree rather than in `.git/hooks` so it can be
//! committed and everyone on the repo gets the same hook. It contains no logic
//! for the same reason the bash version is being retired: a shim that only
//! `exec`s can be updated by upgrading the binary.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The directory `core.hooksPath` is pointed at.
pub const HOOKS_DIR: &str = "hooks";

/// What the shim contains. `sh`, not `bash`, because there is nothing here
/// that needs more than `sh`.
pub const SHIM: &str = "#!/bin/sh\nexec madoqua run\n";

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
