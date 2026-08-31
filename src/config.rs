//! What to run, read from `[tool.madoqua]` in the repo's `pyproject.toml`,
//! from the personal overlay at `<repo_root>/.git/hooks.local.toml`, and from
//! `MADOQUA_SKIP`.
//!
//! Both files deserialize into the same `Layer`, and both are applied by the
//! same merge, so the semantics are written down once. This module owns
//! *everywhere the configuration is written down*, which is why the two file
//! reads and the one environment read all live here; everything that decides
//! anything takes already-parsed data.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

/// Which half of the run a step belongs to.
///
/// The distinction is not cosmetic: fix steps write to the working tree and
/// run sequentially, check steps are read-only and run in parallel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Sequential, writes to the working tree, a non-zero exit is not fatal.
    Fix,
    /// Parallel, read-only, a non-zero exit fails the commit.
    Check,
}

impl Phase {
    /// The name this phase is logged under. Part of the log's contract.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fix => "fix",
            Self::Check => "check",
        }
    }
}

/// One command madoqua runs, with its argv already split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// What this step is called in the verdict, in `MADOQUA_SKIP` and in the log.
    pub name: String,
    /// The command and its arguments. Never empty.
    pub argv: Vec<String>,
    /// Whether the staged file list is appended to `argv`.
    pub pass_files: bool,
    /// Kill the step after this long. `None` means wait forever.
    pub timeout: Option<Duration>,
    /// Keep at most this many lines of captured output. `None` means all of it.
    pub max_output_lines: Option<usize>,
}

/// The resolved answer to "what runs, and where do timings go".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Sequential writers, in the order they are declared.
    pub fix: Vec<Step>,
    /// Parallel read-only checks.
    pub check: Vec<Step>,
    /// Where to append the timing log. `None` means the built-in default path.
    pub log: Option<String>,
}

/// The built-in registry, identical to the bash hook this replaces.
const DEFAULT_FIX: [(&str, &str); 2] =
    [("ruff fix", "ruff check --fix --quiet --"), ("ruff format", "ruff format --quiet --")];
const DEFAULT_CHECK: [(&str, &str); 2] =
    [("ruff check", "ruff check --quiet --"), ("ty check", "ty check --")];

impl Default for Config {
    /// The zero-config behaviour: the bash hook's registry.
    fn default() -> Self {
        Self { fix: builtin(&DEFAULT_FIX), check: builtin(&DEFAULT_CHECK), log: None }
    }
}

/// Build steps from a built-in table. The table is a literal we control, so a
/// split failure here is a bug in this file rather than in a user's config.
fn builtin(table: &[(&str, &str)]) -> Vec<Step> {
    table
        .iter()
        .map(|(name, cmd)| Step {
            name: (*name).to_owned(),
            argv: split_command(cmd).unwrap_or_else(|_| vec![(*cmd).to_owned()]),
            pass_files: true,
            timeout: None,
            max_output_lines: None,
        })
        .collect()
}

impl Config {
    /// Resolve the configuration for `root` against the real environment:
    /// built-in defaults, then `pyproject.toml`, then the personal overlay,
    /// then `MADOQUA_SKIP`.
    ///
    /// The environment read is the one impure line; [`Config::resolved_from`]
    /// is the same decision over data a test can hand it.
    pub fn resolve(root: &Path) -> Result<Self> {
        Self::resolved_from(root, &skip_var())
    }

    /// [`Config::resolve`] with the skip list supplied rather than read.
    pub fn resolved_from(root: &Path, skip: &str) -> Result<Self> {
        let mut config = Self::default();

        if let Some(layer) = read_pyproject(root)? {
            config.apply(layer).context("[tool.madoqua] in pyproject.toml")?;
        }
        if let Some(layer) = read_overlay(&overlay_path(root))? {
            config.apply(layer).context(".git/hooks.local.toml")?;
        }
        config.check = filter_checks(&config.check, skip);
        Ok(config)
    }

    /// Merge one layer on top of this configuration.
    ///
    /// `fix` / `check` replace the list entirely; `extend_fix` / `extend_check`
    /// append to it; scalar keys overwrite. A layer may do both, in which case
    /// the replacement happens first and the extension appends to it.
    fn apply(&mut self, layer: Layer) -> Result<()> {
        if let Some(specs) = layer.fix {
            self.fix = parse_steps(&specs)?;
        }
        if let Some(specs) = layer.check {
            self.check = parse_steps(&specs)?;
        }
        if let Some(specs) = layer.extend_fix {
            self.fix.extend(parse_steps(&specs)?);
        }
        if let Some(specs) = layer.extend_check {
            self.check.extend(parse_steps(&specs)?);
        }
        if let Some(log) = layer.log {
            self.log = Some(log);
        }
        Ok(())
    }
}

/// Where the personal overlay lives.
fn overlay_path(root: &Path) -> PathBuf {
    root.join(".git").join("hooks.local.toml")
}

/// One file's worth of settings, before merging.
///
/// Every key is `Option` on purpose: "absent" has to stay distinguishable from
/// "set to the same value the default has", or replace-versus-extend cannot be
/// expressed.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Layer {
    fix: Option<Vec<StepSpec>>,
    check: Option<Vec<StepSpec>>,
    extend_fix: Option<Vec<StepSpec>>,
    extend_check: Option<Vec<StepSpec>>,
    log: Option<String>,
}

/// A step as written in TOML: either a bare command line or a table.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StepSpec {
    /// `"ruff check --quiet"` — defaults for everything else.
    Cmd(String),
    /// `{ name = "ty", cmd = "ty check", timeout_s = 120 }`.
    Table(StepTable),
}

#[derive(Debug, Deserialize)]
struct StepTable {
    name: Option<String>,
    cmd: String,
    pass_files: Option<bool>,
    timeout_s: Option<u64>,
    max_output_lines: Option<usize>,
}

/// `pyproject.toml`'s nesting, so serde can reach `[tool.madoqua]`.
#[derive(Debug, Default, Deserialize)]
struct PyProject {
    #[serde(default)]
    tool: Tool,
}

#[derive(Debug, Default, Deserialize)]
struct Tool {
    madoqua: Option<Layer>,
}

/// Read `<root>/pyproject.toml`. A missing file, or a file without our table,
/// is not an error — it means "use the defaults". Malformed TOML is, and the
/// error names the file.
fn read_pyproject(root: &Path) -> Result<Option<Layer>> {
    let path = root.join("pyproject.toml");
    let Some(text) = read_optional(&path)? else { return Ok(None) };
    let parsed: PyProject =
        toml::from_str(&text).with_context(|| format!("cannot parse {}", path.display()))?;
    Ok(parsed.tool.madoqua)
}

/// Read the personal overlay, which is the same schema at the top level.
fn read_overlay(path: &Path) -> Result<Option<Layer>> {
    let Some(text) = read_optional(path)? else { return Ok(None) };
    let layer =
        toml::from_str(&text).with_context(|| format!("cannot parse {}", path.display()))?;
    Ok(Some(layer))
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("cannot read {}", path.display())),
    }
}

fn parse_steps(specs: &[StepSpec]) -> Result<Vec<Step>> {
    specs.iter().map(parse_step).collect()
}

fn parse_step(spec: &StepSpec) -> Result<Step> {
    let (cmd, table) = match spec {
        StepSpec::Cmd(cmd) => (cmd.as_str(), None),
        StepSpec::Table(table) => (table.cmd.as_str(), Some(table)),
    };

    let argv = split_command(cmd).with_context(|| format!("cannot use command `{cmd}`"))?;
    let name = table.and_then(|t| t.name.clone()).unwrap_or_else(|| derive_name(&argv));

    Ok(Step {
        name,
        argv,
        pass_files: table.and_then(|t| t.pass_files).unwrap_or(true),
        timeout: table.and_then(|t| t.timeout_s).map(Duration::from_secs),
        max_output_lines: table.and_then(|t| t.max_output_lines),
    })
}

/// The name of a step written in string form: the leading words that are not
/// flags, so `ruff check --quiet --` is called "ruff check".
fn derive_name(argv: &[String]) -> String {
    let words: Vec<&str> =
        argv.iter().take_while(|word| !word.starts_with('-')).map(String::as_str).collect();
    if words.is_empty() { argv.first().cloned().unwrap_or_default() } else { words.join(" ") }
}

/// Split a command line on whitespace, honouring single and double quotes.
///
/// This is deliberately not a shell: there are no pipes, no redirections and
/// no expansions, and a command asking for them is rejected rather than
/// silently misread as a literal argument.
fn split_command(cmd: &str) -> Result<Vec<String>> {
    for forbidden in ["|", ";", "&&"] {
        if cmd.contains(forbidden) {
            bail!(
                "`{forbidden}` needs a shell, and madoqua runs commands directly; \
                 split it into separate entries or wrap it in a script"
            );
        }
    }

    let mut words = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;

    for ch in cmd.chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => current.push(ch),
            None if ch == '\'' || ch == '"' => {
                started = true;
                quote = Some(ch);
            }
            None if ch.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            None => {
                started = true;
                current.push(ch);
            }
        }
    }

    if quote.is_some() {
        return Err(anyhow!("unbalanced quote"));
    }
    if started {
        words.push(current);
    }
    if words.is_empty() {
        return Err(anyhow!("the command is empty"));
    }
    Ok(words)
}

/// Drop the checks named in `MADOQUA_SKIP`, matching by name exactly.
///
/// Skips apply to the check phase only — a skip that silently stopped the
/// formatter would leave the working tree in a state the next run reformats.
fn filter_checks(checks: &[Step], skip: &str) -> Vec<Step> {
    let skipped: Vec<&str> = skip.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    checks.iter().filter(|step| !skipped.contains(&step.name.as_str())).cloned().collect()
}

/// The environment variable that drops checks from a single run.
pub const SKIP_VAR: &str = "MADOQUA_SKIP";

/// Read [`SKIP_VAR`]. Absent and empty mean the same thing: skip nothing.
fn skip_var() -> String {
    std::env::var(SKIP_VAR).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(steps: &[Step]) -> Vec<&str> {
        steps.iter().map(|s| s.name.as_str()).collect()
    }

    fn layer(toml_text: &str) -> Layer {
        toml::from_str(toml_text).unwrap()
    }

    #[test]
    fn the_defaults_are_the_bash_registry() {
        let config = Config::default();
        assert_eq!(names(&config.fix), ["ruff fix", "ruff format"], "fix order matters");
        assert_eq!(names(&config.check), ["ruff check", "ty check"], "the bash check registry");
        assert_eq!(
            config.check[1].argv,
            ["ty", "check", "--"],
            "the `--` separator is part of the ported invocation"
        );
    }

    #[test]
    fn a_string_step_takes_files_and_derives_its_name_from_the_leading_words() {
        let step = parse_step(&StepSpec::Cmd("ruff check --quiet".to_owned())).unwrap();
        assert_eq!(step.name, "ruff check", "flags are not part of the name");
        assert_eq!(step.argv, ["ruff", "check", "--quiet"]);
        assert!(step.pass_files, "string form defaults to passing the file list");
        assert_eq!(step.timeout, None, "string form has no timeout");
    }

    #[test]
    fn a_table_step_carries_its_own_name_and_limits() {
        let step = parse_step(&StepSpec::Table(StepTable {
            name: Some("ty".to_owned()),
            cmd: "ty check".to_owned(),
            pass_files: Some(false),
            timeout_s: Some(120),
            max_output_lines: Some(200),
        }))
        .unwrap();
        assert_eq!(step.name, "ty", "an explicit name wins over the derived one");
        assert!(!step.pass_files, "a repo-wide tool gets no file list");
        assert_eq!(step.timeout, Some(Duration::from_secs(120)));
        assert_eq!(step.max_output_lines, Some(200));
    }

    #[test]
    fn quoted_arguments_survive_splitting() {
        assert_eq!(
            split_command("ty check --config 'line length = 100'").unwrap(),
            ["ty", "check", "--config", "line length = 100"],
            "quotes group one argument; the quotes themselves are not passed on"
        );
    }

    #[test]
    fn an_empty_quoted_argument_is_kept() {
        assert_eq!(
            split_command("tool --arg ''").unwrap(),
            ["tool", "--arg", ""],
            "an explicitly empty argument is not the same as no argument"
        );
    }

    #[test]
    fn shell_operators_are_rejected_by_name() {
        for cmd in ["ruff check | tee log", "a; b", "a && b"] {
            let err = split_command(cmd).unwrap_err();
            assert!(
                format!("{err:#}").contains("needs a shell"),
                "`{cmd}` must be rejected with an explanation, got: {err:#}"
            );
        }
    }

    #[test]
    fn an_unbalanced_quote_is_an_error() {
        let err = split_command("tool 'oops").unwrap_err();
        assert!(
            format!("{err:#}").contains("unbalanced"),
            "the error must say what is wrong, got: {err:#}"
        );
    }

    #[test]
    fn a_list_key_replaces_and_extend_appends() {
        let mut config = Config::default();
        config.apply(layer("check = [\"mypy\"]\nextend_check = [\"bandit\"]\n")).unwrap();
        assert_eq!(
            names(&config.check),
            ["mypy", "bandit"],
            "replacement happens first, then the extension appends to the result"
        );
        assert_eq!(
            names(&config.fix),
            ["ruff fix", "ruff format"],
            "a layer that says nothing about fix leaves it alone"
        );
    }

    #[test]
    fn extend_alone_keeps_the_defaults() {
        let mut config = Config::default();
        config.apply(layer("extend_check = [\"bandit -q\"]\n")).unwrap();
        assert_eq!(names(&config.check), ["ruff check", "ty check", "bandit"]);
    }

    #[test]
    fn a_scalar_key_is_overwritten_by_the_later_layer() {
        let mut config = Config::default();
        config.apply(layer("log = \"a.jsonl\"\n")).unwrap();
        config.apply(layer("log = \"b.jsonl\"\n")).unwrap();
        assert_eq!(config.log.as_deref(), Some("b.jsonl"), "the overlay wins");
    }

    #[test]
    fn the_overlay_is_layered_on_top_of_pyproject() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[tool.madoqua]\ncheck = [\"mypy\"]\nlog = \"repo.jsonl\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(overlay_path(dir.path()), "extend_check = [\"bandit\"]\n").unwrap();

        let config = Config::resolve(dir.path()).unwrap();
        assert_eq!(names(&config.check), ["mypy", "bandit"]);
        assert_eq!(config.log.as_deref(), Some("repo.jsonl"), "the overlay said nothing about log");
    }

    #[test]
    fn no_config_at_all_yields_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            Config::resolve(dir.path()).unwrap(),
            Config::default(),
            "the tool has to work in a repo that has never heard of it"
        );
    }

    #[test]
    fn a_pyproject_without_our_table_yields_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[project]\nname = \"x\"\n").unwrap();
        assert_eq!(Config::resolve(dir.path()).unwrap(), Config::default());
    }

    #[test]
    fn malformed_toml_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "this is not toml {{").unwrap();
        let err = Config::resolve(dir.path()).unwrap_err();
        assert!(
            format!("{err:#}").contains("pyproject.toml"),
            "the error must name the file it could not parse, got: {err:#}"
        );
    }

    #[test]
    fn a_bad_command_names_the_file_and_the_command() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[tool.madoqua]\ncheck = [\"ruff | tee\"]\n",
        )
        .unwrap();
        let err = format!("{:#}", Config::resolve(dir.path()).unwrap_err());
        assert!(err.contains("pyproject.toml"), "must name the layer, got: {err}");
        assert!(err.contains("ruff | tee"), "must name the command, got: {err}");
    }

    #[test]
    fn skip_filters_checks_by_exact_name() {
        let checks = Config::default().check;
        assert_eq!(
            names(&filter_checks(&checks, "ty check")),
            ["ruff check"],
            "the named check is dropped and the rest survive"
        );
        assert_eq!(
            names(&filter_checks(&checks, " ty check , ruff check ")),
            [] as [&str; 0],
            "the list is comma-separated and whitespace around a name is ignored"
        );
        assert_eq!(
            names(&filter_checks(&checks, "ty")),
            ["ruff check", "ty check"],
            "matching is exact, not a prefix — `ty` is not `ty check`"
        );
        assert_eq!(names(&filter_checks(&checks, "")), ["ruff check", "ty check"]);
    }

    #[test]
    fn resolving_applies_the_skip_list_as_the_last_layer() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[tool.madoqua]\ncheck = [\"ruff check --quiet\", \"ty check\"]\n",
        )
        .unwrap();

        let config = Config::resolved_from(dir.path(), "ty check").unwrap();
        assert_eq!(
            names(&config.check),
            ["ruff check"],
            "MADOQUA_SKIP is the last word on what runs, after both files"
        );
        assert_eq!(
            names(&config.fix),
            ["ruff fix", "ruff format"],
            "and it never touches the fix phase: half-formatted files are worse than none"
        );
    }

    #[test]
    fn a_command_that_is_all_flags_still_gets_a_name() {
        let argv = ["--fix".to_owned(), "x".to_owned()];
        assert_eq!(
            derive_name(&argv),
            "--fix",
            "an unnameable step still needs something for MADOQUA_SKIP and the verdict to say"
        );
        assert_eq!(derive_name(&[]), "", "and an empty argv cannot name anything");
    }
}
