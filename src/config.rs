//! What to run, read from `[tool.madoqua]` in the repo's `pyproject.toml`,
//! from the personal overlay at `hooks.local.toml` in the repository's git
//! directory, and from `MADOQUA_SKIP`.
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

use crate::pattern::Pattern;

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

/// Which command is running: the commit hook, or the whole-repo check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `madoqua run`: fix and check what is staged.
    Commit,
    /// `madoqua check`: check every file in the repository, fix nothing.
    Full,
}

/// Which modes a check runs in, as written in its `when` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// Both the hook and the full run. The default.
    Always,
    /// Only the hook — a tool that works out its own diff, say.
    Commit,
    /// Only the full run — too slow, or too broad, for every commit.
    Full,
}

impl When {
    /// Parsed by hand rather than by serde: a step is an untagged enum, and
    /// serde reports any bad field in one as "did not match any variant",
    /// which names neither the key nor the values it would have taken.
    fn parse(value: &str) -> Result<Self> {
        match value {
            "always" => Ok(Self::Always),
            "commit" => Ok(Self::Commit),
            "full" => Ok(Self::Full),
            other => bail!("`when = \"{other}\"` is not one of \"always\", \"commit\", \"full\""),
        }
    }

    fn includes(self, mode: Mode) -> bool {
        match self {
            Self::Always => true,
            Self::Commit => mode == Mode::Commit,
            Self::Full => mode == Mode::Full,
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
    /// The staged files that wake this step. Never empty.
    pub on: Vec<Pattern>,
    /// Which modes the step runs in. Always [`When::Always`] for a fixer.
    pub(crate) when: When,
    /// What the full run runs instead of `argv`, handed no file list. `None`
    /// means `argv`, with the file list when `pass_files` says so.
    pub(crate) full_argv: Option<Vec<String>>,
    /// Whether a failure fails the run. `false` reports it and lets the run
    /// pass. Always `true` for a fixer.
    pub blocking: bool,
}

impl Step {
    /// The staged files this step runs on, in the order git listed them. An
    /// empty answer means the step does not run at all.
    pub fn select(&self, staged: &[String]) -> Vec<String> {
        staged
            .iter()
            .filter(|path| self.on.iter().any(|glob| glob.matches(path)))
            .cloned()
            .collect()
    }

    /// This step as `mode` runs it, or `None` when it does not run in `mode`.
    fn for_mode(&self, mode: Mode) -> Option<Self> {
        if !self.when.includes(mode) {
            return None;
        }
        let mut step = self.clone();
        if let (Mode::Full, Some(full)) = (mode, &self.full_argv) {
            step.argv.clone_from(full);
            step.pass_files = false;
        }
        Some(step)
    }
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

/// What a step runs on when it does not say: the bash hook's pathspec.
const DEFAULT_ON: [&str; 2] = ["*.py", "*.pyi"];

/// [`DEFAULT_ON`], parsed. The globs are literals we control, so a parse
/// failure here is a bug in this file, and dropping the glob is the answer
/// that keeps `clippy::expect_used` quiet without hiding it from the tests.
fn default_on() -> Vec<Pattern> {
    DEFAULT_ON.iter().filter_map(|glob| Pattern::parse(glob).ok()).collect()
}

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
            on: default_on(),
            when: When::Always,
            full_argv: None,
            blocking: true,
        })
        .collect()
}

impl Config {
    /// Resolve the configuration for the working tree at `root`, whose git
    /// directory is `git_dir`, against the real environment: built-in
    /// defaults, then `pyproject.toml`, then the personal overlay, then
    /// `MADOQUA_SKIP`.
    ///
    /// The two directories are the same one for an ordinary checkout and
    /// different ones in a linked worktree; [`crate::git::common_dir`] is what
    /// tells them apart.
    ///
    /// The environment read is the one impure line; [`Config::resolved_from`]
    /// is the same decision over data a test can hand it.
    pub fn resolve(root: &Path, git_dir: &Path) -> Result<Self> {
        Self::resolved_from(root, git_dir, &skip_var())
    }

    /// [`Config::resolve`] with the skip list supplied rather than read.
    pub fn resolved_from(root: &Path, git_dir: &Path, skip: &str) -> Result<Self> {
        let mut config = Self::default();

        if let Some(layer) = read_pyproject(root)? {
            config.apply(layer).context("[tool.madoqua] in pyproject.toml")?;
        }
        if let Some(layer) = read_overlay(&overlay_path(git_dir))? {
            config.apply(layer).context("hooks.local.toml")?;
        }
        config.check = filter_checks(&config.check, skip);
        Ok(config)
    }

    /// The steps `mode` runs, each as that mode runs it.
    ///
    /// The full run fixes nothing: it answers "is the repository clean", and
    /// rewriting files is a different question with a different command. The
    /// rest is each check's `when` and `full_cmd`, decided here so the
    /// pipeline and the runner never need to know there are two modes.
    #[must_use]
    pub fn for_mode(&self, mode: Mode) -> Self {
        let steps = |steps: &[Step]| steps.iter().filter_map(|step| step.for_mode(mode)).collect();
        Self {
            fix: if mode == Mode::Full { Vec::new() } else { steps(&self.fix) },
            check: steps(&self.check),
            log: self.log.clone(),
        }
    }

    /// Merge one layer on top of this configuration.
    ///
    /// `fix` / `check` replace the list entirely; `extend_fix` / `extend_check`
    /// append to it; scalar keys overwrite. A layer may do both, in which case
    /// the replacement happens first and the extension appends to it.
    fn apply(&mut self, layer: Layer) -> Result<()> {
        if let Some(specs) = layer.fix {
            self.fix = parse_steps(&specs, Phase::Fix)?;
        }
        if let Some(specs) = layer.check {
            self.check = parse_steps(&specs, Phase::Check)?;
        }
        if let Some(specs) = layer.extend_fix {
            self.fix.extend(parse_steps(&specs, Phase::Fix)?);
        }
        if let Some(specs) = layer.extend_check {
            self.check.extend(parse_steps(&specs, Phase::Check)?);
        }
        if let Some(log) = layer.log {
            self.log = Some(log);
        }
        Ok(())
    }
}

/// The overlay's name inside the git directory.
const OVERLAY_NAME: &str = "hooks.local.toml";

/// Where the personal overlay lives: in the git directory, not under the
/// working tree.
///
/// Every worktree of a clone shares that directory, which is the semantics the
/// overlay wants — it is one developer's settings for one repository, and a
/// copy per worktree would have to be written again for every task branch,
/// then vanish with `git worktree remove`.
fn overlay_path(git_dir: &Path) -> PathBuf {
    git_dir.join(OVERLAY_NAME)
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
    on: Option<Vec<String>>,
    when: Option<String>,
    full_cmd: Option<String>,
    blocking: Option<bool>,
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

fn parse_steps(specs: &[StepSpec], phase: Phase) -> Result<Vec<Step>> {
    specs.iter().map(|spec| parse_step(spec, phase)).collect()
}

fn parse_step(spec: &StepSpec, phase: Phase) -> Result<Step> {
    let (cmd, table) = match spec {
        StepSpec::Cmd(cmd) => (cmd.as_str(), None),
        StepSpec::Table(table) => (table.cmd.as_str(), Some(table)),
    };

    let argv = split_command(cmd).with_context(|| format!("cannot use command `{cmd}`"))?;
    let name = table.and_then(|t| t.name.clone()).unwrap_or_else(|| derive_name(&argv));

    if let (Phase::Fix, Some(table)) = (phase, table) {
        refuse_check_keys(table).with_context(|| format!("cannot use fixer `{cmd}`"))?;
    }
    let when = match table.and_then(|t| t.when.as_deref()) {
        None => When::Always,
        Some(value) => When::parse(value).with_context(|| format!("cannot use `{cmd}`"))?,
    };
    let full_argv = match table.and_then(|t| t.full_cmd.as_deref()) {
        None => None,
        Some(_) if when == When::Commit => bail!(
            "`full_cmd` on `{cmd}` would never be used: it has `when = \"commit\"`, \
             so the full run skips it"
        ),
        Some(_) if when == When::Full => bail!(
            "`full_cmd` on `{cmd}` means its `cmd` would never run: it has \
             `when = \"full\"`, so put the command in `cmd`"
        ),
        Some(full) => {
            Some(split_command(full).with_context(|| format!("cannot use full_cmd `{full}`"))?)
        }
    };

    Ok(Step {
        name,
        argv,
        pass_files: table.and_then(|t| t.pass_files).unwrap_or(true),
        timeout: table.and_then(|t| t.timeout_s).map(Duration::from_secs),
        max_output_lines: table.and_then(|t| t.max_output_lines),
        on: match table.and_then(|t| t.on.as_deref()) {
            None => default_on(),
            Some(globs) => {
                parse_on(globs).with_context(|| format!("cannot use `on` for `{cmd}`"))?
            }
        },
        when,
        full_argv,
        blocking: table.and_then(|t| t.blocking).unwrap_or(true),
    })
}

/// Refuse the keys that only mean something for a check.
///
/// Fixers run only in the hook, so `when` and `full_cmd` could never take
/// effect, and a fixer's failure already never blocks — the check after it
/// reports what it left behind. A key that silently does nothing is a key
/// someone will spend an afternoon on.
fn refuse_check_keys(table: &StepTable) -> Result<()> {
    let present = [
        ("when", table.when.is_some()),
        ("full_cmd", table.full_cmd.is_some()),
        ("blocking", table.blocking.is_some()),
    ];
    match present.iter().find(|(_, set)| *set) {
        Some((key, _)) => bail!(
            "`{key}` applies to checks only: fixers run only in the commit hook, \
             and a fixer's failure is reported by the check after it"
        ),
        None => Ok(()),
    }
}

/// A step's `on` list. An empty one is refused rather than read as "never":
/// a step that cannot run is a step someone meant to remove, and the overlay
/// or `MADOQUA_SKIP` are the ways to stop one running.
fn parse_on(globs: &[String]) -> Result<Vec<Pattern>> {
    if globs.is_empty() {
        bail!("an empty list would never run it; remove the step, or use MADOQUA_SKIP for one run");
    }
    globs
        .iter()
        .map(|glob| Pattern::parse(glob).with_context(|| format!("bad glob `{glob}`")))
        .collect()
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

/// The field names the configuration deserializer actually accepts.
///
/// Asked of serde rather than written down, so a renamed field cannot leave a
/// stale list behind: [`crate::guide`] checks every config key its pages name
/// against this, and a hand-maintained list would drift the moment a key moved.
///
/// Test-only, and gated as such: this answers a question about the schema that
/// only the guide tests ask, and none of it belongs in a shipped binary.
#[cfg(test)]
pub(crate) mod keys {
    use std::collections::BTreeSet;
    use std::fmt;

    use serde::de::{self, Deserializer, Visitor};
    use serde::forward_to_deserialize_any;

    use super::{Layer, StepTable};

    /// Carries the captured field list out through serde's error channel,
    /// which is the only way out of a `Deserializer` that refuses to produce a
    /// value.
    #[derive(Debug)]
    struct Captured(Vec<&'static str>);

    impl fmt::Display for Captured {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "captured fields: {:?}", self.0)
        }
    }

    impl std::error::Error for Captured {}

    impl de::Error for Captured {
        fn custom<T: fmt::Display>(_msg: T) -> Self {
            Self(Vec::new())
        }
    }

    /// A `Deserializer` that answers "what fields does this struct have?" and
    /// nothing else.
    struct FieldCapture;

    impl<'de> Deserializer<'de> for FieldCapture {
        type Error = Captured;

        fn deserialize_struct<V: Visitor<'de>>(
            self,
            _name: &'static str,
            fields: &'static [&'static str],
            _visitor: V,
        ) -> Result<V::Value, Captured> {
            Err(Captured(fields.to_vec()))
        }

        fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Captured> {
            Err(Captured(Vec::new()))
        }

        forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
            bytes byte_buf option unit unit_struct newtype_struct seq tuple
            tuple_struct map enum identifier ignored_any
        }
    }

    /// The field names `T`'s derived `Deserialize` will accept.
    fn field_names<T: serde::de::DeserializeOwned>() -> Vec<&'static str> {
        match T::deserialize(FieldCapture) {
            Err(Captured(fields)) => fields,
            // Unreachable for a derived struct impl, which always reaches
            // `deserialize_struct` and always returns `Err`. Left as an empty
            // list rather than a panic (`clippy::panic` is warned crate-wide):
            // an empty list fails `the_layer_and_step_keys_are_all_there`
            // loudly instead of passing vacuously.
            Ok(_) => Vec::new(),
        }
    }

    /// Every key a configuration file may set: the layer keys and the keys of
    /// a step written in table form.
    #[must_use]
    pub fn accepted_keys() -> BTreeSet<&'static str> {
        field_names::<Layer>().into_iter().chain(field_names::<StepTable>()).collect()
    }

    #[cfg(test)]
    mod tests {
        use super::accepted_keys;

        #[test]
        fn the_layer_and_step_keys_are_all_there() {
            let keys = accepted_keys();
            for expected in [
                "fix",
                "check",
                "extend_fix",
                "extend_check",
                "log",
                "cmd",
                "name",
                "pass_files",
                "timeout_s",
                "max_output_lines",
                "on",
                "when",
                "full_cmd",
                "blocking",
            ] {
                assert!(
                    keys.contains(expected),
                    "`{expected}` should be an accepted key: {keys:?}"
                );
            }
        }

        #[test]
        fn a_key_that_was_never_declared_is_not_accepted() {
            assert!(
                !accepted_keys().contains("extend_lint"),
                "the list comes from serde, so an invented key must be absent",
            );
        }
    }
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

    /// [`Config::resolve`] for an ordinary checkout, where the git directory
    /// is the `.git` under the working tree.
    fn resolve_in_clone(root: &Path) -> Result<Config> {
        Config::resolve(root, &root.join(".git"))
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
        let step =
            parse_step(&StepSpec::Cmd("ruff check --quiet".to_owned()), Phase::Check).unwrap();
        assert_eq!(step.name, "ruff check", "flags are not part of the name");
        assert_eq!(step.argv, ["ruff", "check", "--quiet"]);
        assert!(step.pass_files, "string form defaults to passing the file list");
        assert_eq!(step.timeout, None, "string form has no timeout");
    }

    #[test]
    fn a_table_step_carries_its_own_name_and_limits() {
        let step = parse_step(
            &StepSpec::Table(StepTable {
                name: Some("ty".to_owned()),
                cmd: "ty check".to_owned(),
                pass_files: Some(false),
                timeout_s: Some(120),
                max_output_lines: Some(200),
                on: None,
                when: None,
                full_cmd: None,
                blocking: None,
            }),
            Phase::Check,
        )
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
        std::fs::write(overlay_path(&dir.path().join(".git")), "extend_check = [\"bandit\"]\n")
            .unwrap();

        let config = resolve_in_clone(dir.path()).unwrap();
        assert_eq!(names(&config.check), ["mypy", "bandit"]);
        assert_eq!(config.log.as_deref(), Some("repo.jsonl"), "the overlay said nothing about log");
    }

    #[test]
    fn no_config_at_all_yields_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_in_clone(dir.path()).unwrap(),
            Config::default(),
            "the tool has to work in a repo that has never heard of it"
        );
    }

    #[test]
    fn a_pyproject_without_our_table_yields_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[project]\nname = \"x\"\n").unwrap();
        assert_eq!(resolve_in_clone(dir.path()).unwrap(), Config::default());
    }

    #[test]
    fn malformed_toml_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "this is not toml {{").unwrap();
        let err = resolve_in_clone(dir.path()).unwrap_err();
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
        let err = format!("{:#}", resolve_in_clone(dir.path()).unwrap_err());
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

        let config =
            Config::resolved_from(dir.path(), &dir.path().join(".git"), "ty check").unwrap();
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

    /// The shape of a linked worktree: `<root>/.git` is a file naming the
    /// clone's metadata directory, not a directory of its own.
    ///
    /// Joining the overlay's name onto it aims a read at a path *through* a
    /// file, which fails with `Not a directory` — not with `NotFound` — and so
    /// aborted the whole run before a single check had been started.
    #[test]
    fn the_overlay_is_read_from_the_git_directory_a_worktree_points_at() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("task-42");
        let git_dir = dir.path().join("clone/.git");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(git_dir.join("worktrees/task-42")).unwrap();
        std::fs::write(root.join(".git"), "gitdir: ../clone/.git/worktrees/task-42\n").unwrap();
        std::fs::write(overlay_path(&git_dir), "extend_check = [\"bandit\"]\n").unwrap();

        let config = Config::resolve(&root, &git_dir).unwrap();
        assert_eq!(
            names(&config.check),
            ["ruff check", "ty check", "bandit"],
            "the overlay belongs to the clone, and every worktree of it runs with it"
        );
    }

    #[test]
    fn a_missing_overlay_in_a_worktree_is_still_just_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("task-42");
        let git_dir = dir.path().join("clone/.git");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::write(root.join(".git"), "gitdir: ../clone/.git/worktrees/task-42\n").unwrap();

        assert_eq!(
            Config::resolve(&root, &git_dir).unwrap(),
            Config::default(),
            "no overlay is no overlay; the `.git` file next to the checkout is not one"
        );
    }

    fn staged(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| (*p).to_owned()).collect()
    }

    fn step_from(toml_text: &str) -> Result<Step> {
        let spec: StepSpec = toml::from_str::<Layer>(&format!("check = [{toml_text}]\n"))
            .map(|layer| layer.check.unwrap_or_default().into_iter().next())?
            .ok_or_else(|| anyhow!("no step in `{toml_text}`"))?;
        parse_step(&spec, Phase::Check)
    }

    #[test]
    fn every_step_runs_on_staged_python_unless_it_says_otherwise() {
        let all = staged(&["README.md", "a.py", "uv.lock", "pkg/b.pyi"]);
        for step in Config::default().fix.iter().chain(&Config::default().check) {
            assert_eq!(
                step.select(&all),
                ["a.py", "pkg/b.pyi"],
                "`{}` keeps the bash hook's `*.py` / `*.pyi`",
                step.name
            );
        }
        let string_form = step_from("\"bandit -q\"").unwrap();
        assert_eq!(string_form.select(&all), ["a.py", "pkg/b.pyi"], "and so does a configured one");
    }

    #[test]
    fn a_step_with_on_runs_on_what_its_globs_match_in_staged_order() {
        let step =
            step_from(r#"{ cmd = "gerenuk run", on = ["uv.lock", "*.py", "tests/**"] }"#).unwrap();
        assert_eq!(
            step.select(&staged(&["a.py", "README.md", "tests/data/x.json", "uv.lock"])),
            ["a.py", "tests/data/x.json", "uv.lock"],
            "any glob matching is enough, and git's order is kept"
        );
        assert_eq!(
            step.select(&staged(&["README.md", "docs/x.md"])),
            [] as [&str; 0],
            "nothing matching means the step has nothing to do"
        );
    }

    #[test]
    fn on_replaces_the_default_rather_than_adding_to_it() {
        let step = step_from(r#"{ cmd = "lockcheck", on = ["uv.lock"] }"#).unwrap();
        assert_eq!(
            step.select(&staged(&["a.py", "uv.lock"])),
            ["uv.lock"],
            "a step that lists its files gets those files and not Python as well"
        );
    }

    #[test]
    fn an_empty_on_list_is_refused() {
        let err = format!("{:#}", step_from(r#"{ cmd = "never", on = [] }"#).unwrap_err());
        assert!(err.contains("never"), "the error names the command, got: {err}");
        assert!(err.contains("MADOQUA_SKIP"), "and says what to use instead, got: {err}");
    }

    #[test]
    fn a_bad_glob_names_the_command_and_the_glob() {
        let err = format!("{:#}", step_from(r#"{ cmd = "lint", on = ["tests/"] }"#).unwrap_err());
        assert!(err.contains("lint"), "must name the command, got: {err}");
        assert!(err.contains("tests/"), "must name the glob, got: {err}");
    }

    fn config_from(toml_text: &str) -> Result<Config> {
        let mut config = Config::default();
        config.apply(toml::from_str(toml_text)?)?;
        Ok(config)
    }

    #[test]
    fn a_check_runs_in_both_modes_and_blocks_unless_it_says_otherwise() {
        let step = step_from("\"bandit -q\"").unwrap();
        assert_eq!(step.when, When::Always, "an existing config keeps running at commit time");
        assert!(step.blocking, "and a failing check keeps blocking");
        assert_eq!(step.full_argv, None, "and runs the same command in both modes");
    }

    #[test]
    fn when_full_keeps_a_check_out_of_the_hook() {
        let config = config_from(
            r#"check = ["ruff check", { cmd = "vulture src", when = "full", pass_files = false }]"#,
        )
        .unwrap();
        assert_eq!(names(&config.for_mode(Mode::Commit).check), ["ruff check"]);
        assert_eq!(names(&config.for_mode(Mode::Full).check), ["ruff check", "vulture src"]);
    }

    #[test]
    fn when_commit_keeps_a_check_out_of_the_full_run() {
        let config =
            config_from(r#"check = ["ruff check", { cmd = "gerenuk run", when = "commit" }]"#)
                .unwrap();
        assert_eq!(names(&config.for_mode(Mode::Commit).check), ["ruff check", "gerenuk run"]);
        assert_eq!(names(&config.for_mode(Mode::Full).check), ["ruff check"]);
    }

    #[test]
    fn an_unknown_when_names_the_values_it_accepts() {
        let err =
            format!("{:#}", config_from(r#"check = [{ cmd = "x", when = "ci" }]"#).unwrap_err());
        for value in ["always", "commit", "full"] {
            assert!(err.contains(value), "the error lists `{value}`, got: {err}");
        }
    }

    #[test]
    fn the_full_run_has_no_fixers() {
        let full = Config::default().for_mode(Mode::Full);
        assert_eq!(
            names(&full.fix),
            [] as [&str; 0],
            "a whole-repo run reports, it never rewrites"
        );
        assert_eq!(names(&full.check), ["ruff check", "ty check"]);
        assert_eq!(
            Config::default().for_mode(Mode::Commit),
            Config::default(),
            "the hook is as it was"
        );
    }

    #[test]
    fn full_cmd_replaces_the_command_in_the_full_run_and_takes_no_files() {
        let config = config_from(
            r#"check = [{ name = "biston", cmd = "biston scan --focus-args", full_cmd = "biston scan ." }]"#,
        )
        .unwrap();

        let commit = &config.for_mode(Mode::Commit).check[0];
        assert_eq!(commit.argv, ["biston", "scan", "--focus-args"], "the hook runs `cmd`");
        assert!(commit.pass_files, "with the staged files, as before");

        let full = &config.for_mode(Mode::Full).check[0];
        assert_eq!(full.name, "biston", "the name is the step's, whichever command runs");
        assert_eq!(full.argv, ["biston", "scan", "."], "the full run runs `full_cmd`");
        assert!(!full.pass_files, "a command written for the whole repo is handed no file list");
    }

    #[test]
    fn without_full_cmd_the_full_run_keeps_the_command_and_its_file_list() {
        let full = Config::default().for_mode(Mode::Full);
        assert_eq!(full.check[0].argv, ["ruff", "check", "--quiet", "--"]);
        assert!(full.check[0].pass_files, "every matching file in the repo is appended");
    }

    #[test]
    fn a_bad_full_cmd_names_itself() {
        let err = format!(
            "{:#}",
            config_from(r#"check = [{ cmd = "pytest", full_cmd = "pytest | tee" }]"#).unwrap_err()
        );
        assert!(err.contains("full_cmd"), "must name the key, got: {err}");
        assert!(err.contains("pytest | tee"), "and the command, got: {err}");
    }

    #[test]
    fn full_cmd_on_a_commit_only_step_is_refused() {
        let err = format!(
            "{:#}",
            config_from(
                r#"check = [{ cmd = "gerenuk run", when = "commit", full_cmd = "pytest" }]"#
            )
            .unwrap_err()
        );
        assert!(err.contains("full_cmd"), "a key that can never be used is a mistake, got: {err}");
        assert!(err.contains("gerenuk run"), "and the error names the step, got: {err}");
    }

    #[test]
    fn full_cmd_on_a_full_only_step_is_refused() {
        let err = format!(
            "{:#}",
            config_from(r#"check = [{ cmd = "pytest", when = "full", full_cmd = "pytest -x" }]"#)
                .unwrap_err()
        );
        assert!(err.contains("full_cmd"), "`cmd` would never run, got: {err}");
        assert!(err.contains("`cmd`"), "so the error says where the command goes, got: {err}");
    }

    #[test]
    fn blocking_false_is_carried_into_both_modes() {
        let config = config_from(
            r#"check = [{ cmd = "pysentry-rs .", pass_files = false, blocking = false }]"#,
        )
        .unwrap();
        assert!(!config.for_mode(Mode::Commit).check[0].blocking);
        assert!(!config.for_mode(Mode::Full).check[0].blocking);
    }

    #[test]
    fn the_check_only_keys_are_refused_on_a_fixer() {
        for key in [r#"when = "full""#, r#"full_cmd = "ruff format .""#, "blocking = false"] {
            let text = format!(r#"extend_fix = [{{ cmd = "ruff format", {key} }}]"#);
            let err = format!("{:#}", config_from(&text).unwrap_err());
            let name = key.split(' ').next().unwrap_or_default();
            assert!(err.contains(name), "`{key}` on a fixer is refused by name, got: {err}");
            assert!(err.contains("ruff format"), "and the error names the step, got: {err}");
        }
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
