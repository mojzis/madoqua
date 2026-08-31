//! Project configuration, read from `[tool.madoqua]` in the repo's
//! `pyproject.toml`.
//!
//! Unknown keys are accepted rather than rejected, so a newer madoqua's config
//! file does not break an older binary.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Everything madoqua reads out of `pyproject.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Config {}

/// Wrapper types mirroring `pyproject.toml`'s nesting: `[tool.madoqua]`.
#[derive(Debug, Default, Deserialize)]
struct PyProject {
    #[serde(default)]
    tool: Tool,
}

#[derive(Debug, Default, Deserialize)]
struct Tool {
    #[serde(default)]
    madoqua: Config,
}

impl Config {
    /// Load `<root>/pyproject.toml`.
    ///
    /// A missing file or a missing `[tool.madoqua]` table is not an error —
    /// both yield defaults. Malformed TOML is, and the error names the file.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("pyproject.toml");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(err).with_context(|| format!("cannot read {}", path.display())),
        };

        let parsed: PyProject =
            toml::from_str(&text).with_context(|| format!("cannot parse {}", path.display()))?;
        Ok(parsed.tool.madoqua)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_pyproject_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            Config::load(dir.path()).unwrap(),
            Config::default(),
            "no pyproject.toml is a valid project, not an error"
        );
    }

    #[test]
    fn a_pyproject_without_our_table_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[project]\nname = \"x\"\n").unwrap();
        assert_eq!(
            Config::load(dir.path()).unwrap(),
            Config::default(),
            "[tool.madoqua] is optional"
        );
    }

    #[test]
    fn malformed_toml_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "this is not toml {{").unwrap();
        let err = Config::load(dir.path()).unwrap_err();
        assert!(
            format!("{err:#}").contains("pyproject.toml"),
            "the error must name the file it could not parse, got: {err:#}"
        );
    }
}
