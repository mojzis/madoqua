//! Agent-facing instructions for the three moments someone meets madoqua.
//!
//! The prose lives in `docs/src/guide/*.md` and is pulled in with
//! [`include_str!`], so the documentation site and the CLI serve the same
//! bytes. There is no guide text in this file, and there must never be: a
//! second copy is a copy that drifts.

use std::path::Path;

/// Instructions for a repository madoqua is not wired into yet.
const SETUP: &str = include_str!("../docs/src/guide/setup.md");
/// Instructions for a run that blocked a commit.
const TRIAGE: &str = include_str!("../docs/src/guide/triage.md");
/// Reference for the registry, the layers and the log.
const TUNE: &str = include_str!("../docs/src/guide/tune.md");

/// Which set of instructions to print.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Topic {
    /// madoqua is not wired into this repository yet.
    Setup,
    /// A run blocked a commit; what to do with it.
    Triage,
    /// Config keys, phases and the timing log.
    Tune,
}

impl Topic {
    /// The topic's name as written on the command line and in the header.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Triage => "triage",
            Self::Tune => "tune",
        }
    }

    /// The guide text, byte-identical to the docs page it is included from.
    #[must_use]
    pub fn text(self) -> &'static str {
        match self {
            Self::Setup => SETUP,
            Self::Triage => TRIAGE,
            Self::Tune => TUNE,
        }
    }

    /// Every topic, for tests and for exhaustive rendering.
    pub const ALL: [Self; 3] = [Self::Setup, Self::Triage, Self::Tune];
}

/// What made a repository count as configured.
///
/// Ordered by how strong the evidence is that madoqua actually runs here: a
/// shim git will execute beats a table someone wrote and never wired up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    /// The committed shim `madoqua install` writes.
    Shim,
    /// A hand-wired hook in `.git/hooks`, which is not committed but still runs.
    GitHookShim,
    /// A `pyproject.toml` carrying a `[tool.madoqua]` table.
    PyProject,
    /// The personal overlay at `.git/hooks.local.toml`.
    Overlay,
}

impl ConfigSource {
    /// How the header line names this source.
    fn label(self) -> &'static str {
        match self {
            Self::Shim => "hooks/pre-commit",
            Self::GitHookShim => ".git/hooks/pre-commit",
            Self::PyProject => "pyproject.toml [tool.madoqua]",
            Self::Overlay => ".git/hooks.local.toml",
        }
    }
}

/// How the printed topic was chosen.
///
/// Carried into the header so a reader — usually an agent that did not pass a
/// topic — can see why it got the text it got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    /// The user named the topic on the command line.
    Explicit,
    /// The topic was derived from what was found in the working directory.
    Auto(Option<ConfigSource>),
}

/// Report whether madoqua is wired into `dir`, and by what.
///
/// Looks at `dir` only. Walking up to find a repository root would make the
/// answer depend on where the caller happened to stand, and the answer is meant
/// to be about *this* repository — so the guide tells its reader to run at the
/// repository root instead.
///
/// Unreadable or unparseable files are treated as absent rather than as errors:
/// a broken `pyproject.toml` is a reason to say "not configured", not a reason
/// to refuse to print instructions.
#[must_use]
pub fn detect(dir: &Path) -> Option<ConfigSource> {
    if shim_invokes_madoqua(&dir.join(crate::install::HOOKS_DIR).join("pre-commit")) {
        return Some(ConfigSource::Shim);
    }
    if shim_invokes_madoqua(&dir.join(".git").join("hooks").join("pre-commit")) {
        return Some(ConfigSource::GitHookShim);
    }
    if pyproject_has_madoqua_table(&dir.join("pyproject.toml")) {
        return Some(ConfigSource::PyProject);
    }
    if dir.join(".git").join("hooks.local.toml").is_file() {
        return Some(ConfigSource::Overlay);
    }
    None
}

/// Report whether a `pre-commit` hook script actually runs madoqua.
///
/// Comments are stripped first: a hook that says `# TODO: switch to madoqua`
/// is exactly the repository the setup guide is written for.
fn shim_invokes_madoqua(path: &Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    contents.lines().any(|line| line.split('#').next().unwrap_or("").contains("madoqua"))
}

/// Parse `pyproject.toml` and report whether it carries a `[tool.madoqua]`
/// table.
///
/// Parsed rather than grepped: `tool.madoqua` appearing in a comment, a string,
/// or another tool's table is not configuration.
fn pyproject_has_madoqua_table(path: &Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    // `toml::from_str` for a document, not `str::parse` — the latter parses a
    // bare TOML *value* and rejects every real pyproject.toml.
    let Ok(document) = toml::from_str::<toml::Table>(&contents) else {
        return false;
    };
    document.get("tool").and_then(|tool| tool.get("madoqua")).is_some_and(toml::Value::is_table)
}

/// The topic to print when the user named none.
///
/// `tune` is never auto-selected: it is a reference, and nothing about a
/// repository's state says "you need the reference right now".
#[must_use]
pub fn auto_topic(source: Option<ConfigSource>) -> Topic {
    if source.is_some() { Topic::Triage } else { Topic::Setup }
}

/// The first line of every guide, naming the topic and how it was chosen.
fn header(topic: Topic, selection: Selection) -> String {
    match selection {
        Selection::Explicit => format!("# madoqua guide: {}", topic.name()),
        Selection::Auto(None) => {
            format!("# madoqua guide: not configured here -> {}", topic.name())
        }
        Selection::Auto(Some(source)) => {
            format!("# madoqua guide: configured via {} -> {}", source.label(), topic.name())
        }
    }
}

/// The complete guide output: header line, blank line, then the docs page
/// verbatim.
#[must_use]
pub fn render(topic: Topic, selection: Selection) -> String {
    format!("{}\n\n{}", header(topic, selection), topic.text())
}

/// Every madoqua invocation the guides show, as argv vectors ready for clap.
///
/// Public because the check that matters — feeding each one through the real
/// `Command` — can only run where the `Cli` type lives. A guide that shows a
/// command the CLI would reject is worse than no guide.
///
/// Placeholders like `<name>` are dropped: they are holes for the reader to
/// fill, not arguments.
#[must_use]
#[doc(hidden)]
pub fn embedded_invocations(topic: Topic) -> Vec<Vec<String>> {
    command_lines(topic.text()).iter().flat_map(|line| madoqua_invocations(line)).collect()
}

/// Command strings written in a guide: inline backtick spans that invoke
/// madoqua, plus every non-blank line of a fenced `bash` block.
fn command_lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> =
        inline_code_spans(text).into_iter().filter(|span| invokes(span)).collect();
    out.extend(
        lines_with_fence(text)
            .filter(|&(line, fence)| fence == Some("bash") && !line.trim().is_empty())
            .map(|(line, _)| line.to_owned()),
    );
    out
}

/// Split a command line on pipes and keep the segments that invoke madoqua.
fn madoqua_invocations(line: &str) -> Vec<Vec<String>> {
    line.split('|')
        .map(str::trim)
        .filter(|segment| invokes(segment))
        .map(|segment| {
            segment
                .split_whitespace()
                // `<name>` and `--repo=<name>` are holes for the reader; a
                // token carrying either bracket is not an argument.
                .filter(|token| !token.contains('<') && !token.contains('>'))
                .map(str::to_owned)
                .collect()
        })
        .collect()
}

/// Whether a command string is a madoqua invocation rather than prose or
/// another tool.
fn invokes(command: &str) -> bool {
    command == "madoqua" || command.starts_with("madoqua ")
}

/// Inline `code` spans, in source order. Fenced blocks are skipped: they hold
/// TOML, which a config-key check would misread as keys.
fn inline_code_spans(text: &str) -> Vec<String> {
    let mut spans = Vec::new();
    for (line, fence) in lines_with_fence(text) {
        if fence.is_some() {
            continue;
        }
        let mut rest = line;
        while let Some(open) = rest.find('`') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('`') else { break };
            spans.push(after[..close].to_owned());
            rest = &after[close + 1..];
        }
    }
    spans
}

/// Each line of `text` paired with the info string of the fence it sits in, or
/// `None` when it sits outside one. Fence markers themselves are not yielded.
///
/// One walker for both extractors: they would disagree about what opened a
/// fence for exactly as long as there were two of them.
fn lines_with_fence(text: &str) -> impl Iterator<Item = (&str, Option<&str>)> {
    let mut fence: Option<&str> = None;
    text.lines().filter_map(move |line| {
        if let Some(info) = line.strip_prefix("```") {
            fence = if fence.is_some() { None } else { Some(info.trim()) };
            return None;
        }
        Some((line, fence))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No topic may exceed this many lines. Failing this test is the point of
    /// it: a guide that grows past a screenful stops being read.
    const LINE_CAP: usize = 60;

    fn write(dir: &Path, name: &str, contents: &str) {
        if let Some(parent) = dir.join(name).parent() {
            std::fs::create_dir_all(parent).expect("fixture parent should be creatable");
        }
        std::fs::write(dir.join(name), contents).expect("fixture write should succeed");
    }

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir should be creatable")
    }

    // --- Content rules ---

    #[test]
    fn every_topic_fits_the_line_cap() {
        for topic in Topic::ALL {
            let lines = topic.text().trim_end().lines().count();
            assert!(
                lines <= LINE_CAP,
                "guide `{}` is {lines} lines, cap is {LINE_CAP}; cut it rather than raising \
                 the cap",
                topic.name(),
            );
            assert!(lines > 10, "guide `{}` is {lines} lines, which is not a guide", topic.name());
        }
    }

    #[test]
    fn every_topic_is_plain_ascii() {
        for topic in Topic::ALL {
            let offender = topic.text().chars().find(|c| !c.is_ascii());
            assert!(
                offender.is_none(),
                "guide `{}` contains the non-ASCII character {offender:?}; guides are piped \
                 and captured, so they stay ASCII",
                topic.name(),
            );
        }
    }

    #[test]
    fn every_topic_ends_with_a_single_next_line() {
        for topic in Topic::ALL {
            let trimmed = topic.text().trim_end();
            let last = trimmed.lines().next_back().expect("guide should not be empty");
            assert!(
                last.starts_with("next: run "),
                "guide `{}` must end with a `next: run` line, ends with {last:?}",
                topic.name(),
            );
            let count = trimmed.lines().filter(|l| l.starts_with("next: run ")).count();
            assert_eq!(count, 1, "guide `{}` should have exactly one next line", topic.name());
        }
    }

    #[test]
    fn triage_states_the_prohibitions_verbatim() {
        let text = Topic::Triage.text();
        for phrase in [
            "Do not commit with `--no-verify`",
            "Do not use `MADOQUA_SKIP` to get past a check",
            "Do not delete a check from `[tool.madoqua]`",
        ] {
            assert!(text.contains(phrase), "triage guide should contain {phrase:?}");
        }
    }

    #[test]
    fn triage_states_the_exit_code_contract() {
        let text = Topic::Triage.text();
        assert!(text.contains("Exit `1`"), "triage should say what 1 means");
        assert!(text.contains("Exit `2`"), "triage should say what 2 means");
    }

    #[test]
    fn setup_puts_the_venv_before_the_install() {
        // The order is the whole point of the setup guide: `madoqua install`
        // into a repo with no `.venv` produces a hook that blocks every commit.
        let text = Topic::Setup.text();
        let venv = text.find("uv venv").expect("setup should create the venv");
        let install = text.find("madoqua install").expect("setup should install the hook");
        assert!(venv < install, "setup must create the virtualenv before installing the hook");
    }

    #[test]
    fn tune_documents_the_layer_precedence_and_the_phases() {
        let text = Topic::Tune.text();
        assert!(text.contains("Later layers win"), "tune should state the precedence");
        assert!(text.contains("replace that phase's list entirely"), "tune should state replace");
        assert!(text.contains("append to it"), "tune should state extend");
        assert!(text.contains("run in parallel"), "tune should state what the check phase is");
    }

    // --- Every command in the guides must be a real invocation ---
    //
    // The commands are fed through the real clap `Command` in `cli.rs`, where
    // the `Cli` type lives. What is checked here is that the extraction those
    // tests rely on actually finds the commands, so an empty result can never
    // pass as "all valid".

    #[test]
    fn extraction_finds_every_command_the_guides_show() {
        let setup = embedded_invocations(Topic::Setup);
        assert!(
            setup.contains(&vec!["madoqua".to_owned(), "install".to_owned()]),
            "setup shows `madoqua install` in a bash fence, extraction returned {setup:?}",
        );
        assert!(
            setup.contains(&vec!["madoqua".to_owned(), "guide".to_owned(), "triage".to_owned()]),
            "setup points at the triage guide, extraction returned {setup:?}",
        );
        let tune = embedded_invocations(Topic::Tune);
        assert!(
            tune.contains(&vec!["madoqua".to_owned(), "stats".to_owned()]),
            "the `--repo=<name>` placeholder should be stripped, leaving a real argv; got {tune:?}",
        );
        for topic in Topic::ALL {
            let found = embedded_invocations(topic);
            assert!(!found.is_empty(), "guide `{}` shows no commands at all", topic.name());
            for argv in &found {
                assert_eq!(argv.first().map(String::as_str), Some("madoqua"), "argv is {argv:?}");
            }
        }
    }

    #[test]
    fn extraction_skips_commands_that_are_not_madoqua() {
        let setup = embedded_invocations(Topic::Setup);
        assert!(
            !setup.iter().any(|argv| argv.iter().any(|word| word == "uv")),
            "`uv venv && uv sync` sits in a bash fence and is not ours to parse: {setup:?}",
        );
    }

    // --- Every config key in the guides must exist ---

    #[test]
    fn every_config_key_in_the_guides_exists() {
        let accepted = crate::config::keys::accepted_keys();
        let mut checked = 0_usize;

        for topic in Topic::ALL {
            for span in inline_code_spans(topic.text()) {
                let Some(key) = config_key_candidate(&span) else { continue };
                checked += 1;
                assert!(
                    accepted.contains(key.as_str()),
                    "guide `{}` names config key `{key}`, which the deserializer does not \
                     accept; accepted: {accepted:?}",
                    topic.name(),
                );
            }
        }
        assert!(checked >= 6, "expected the guides to name several config keys, found {checked}");
    }

    /// Recognise a span as a config key reference: `snake_case` with an
    /// underscore, which is what distinguishes a key from an ordinary
    /// backticked word like `check` or `log`. Anything with a `.` or a `[` is
    /// a path or a TOML table header, not a key.
    fn config_key_candidate(span: &str) -> Option<String> {
        if span.contains('.') || span.contains('[') {
            return None;
        }
        if is_snake_case(span) && span.contains('_') {
            return Some(span.to_owned());
        }
        None
    }

    fn is_snake_case(s: &str) -> bool {
        !s.is_empty()
            && s.starts_with(|c: char| c.is_ascii_lowercase())
            && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    }

    #[test]
    fn a_key_that_does_not_exist_would_be_caught() {
        assert_eq!(config_key_candidate("extend_check").as_deref(), Some("extend_check"));
        assert!(
            !crate::config::keys::accepted_keys().contains("extend_lint"),
            "the key check would pass anything if every snake_case word were accepted",
        );
        assert_eq!(config_key_candidate("MADOQUA_SKIP"), None, "an env var is not a config key");
        assert_eq!(config_key_candidate("tool.madoqua"), None, "a dotted path is not a key");
        assert_eq!(config_key_candidate("check"), None, "a word without an underscore is prose");
    }

    // --- Detection ---

    #[test]
    fn empty_directory_is_not_configured() {
        let dir = tempdir();
        assert_eq!(detect(dir.path()), None);
        assert_eq!(auto_topic(detect(dir.path())), Topic::Setup);
    }

    #[test]
    fn the_installed_shim_is_configured() {
        let dir = tempdir();
        write(dir.path(), "hooks/pre-commit", crate::install::SHIM);
        assert_eq!(detect(dir.path()), Some(ConfigSource::Shim));
        assert_eq!(auto_topic(detect(dir.path())), Topic::Triage);
    }

    #[test]
    fn a_shim_under_dot_git_is_configured_too() {
        let dir = tempdir();
        write(dir.path(), ".git/hooks/pre-commit", crate::install::SHIM);
        assert_eq!(
            detect(dir.path()),
            Some(ConfigSource::GitHookShim),
            "a hand-wired hook is still a repository that runs madoqua",
        );
    }

    #[test]
    fn a_pre_commit_hook_for_something_else_is_not_configured() {
        let dir = tempdir();
        write(dir.path(), "hooks/pre-commit", "#!/bin/sh\nexec pre-commit run\n");
        assert_eq!(detect(dir.path()), None);
    }

    #[test]
    fn a_shim_that_only_mentions_madoqua_in_a_comment_is_not_configured() {
        let dir = tempdir();
        write(dir.path(), "hooks/pre-commit", "#!/bin/sh\n# TODO: switch to madoqua\nexit 0\n");
        assert_eq!(detect(dir.path()), None);
    }

    #[test]
    fn pyproject_with_tool_madoqua_is_configured() {
        let dir = tempdir();
        write(dir.path(), "pyproject.toml", "[project]\nname = \"x\"\n\n[tool.madoqua]\n");
        assert_eq!(detect(dir.path()), Some(ConfigSource::PyProject));
    }

    #[test]
    fn pyproject_without_tool_madoqua_is_not_configured() {
        let dir = tempdir();
        write(dir.path(), "pyproject.toml", "[project]\nname = \"x\"\n\n[tool.ruff]\n");
        assert_eq!(detect(dir.path()), None);
    }

    #[test]
    fn pyproject_mentioning_madoqua_in_a_string_is_not_configured() {
        let dir = tempdir();
        write(dir.path(), "pyproject.toml", "[project]\ndependencies = [\"tool.madoqua\"]\n");
        assert_eq!(
            detect(dir.path()),
            None,
            "detection parses the TOML, so a mention in a string is not a config table",
        );
    }

    #[test]
    fn unparseable_pyproject_is_not_configured() {
        let dir = tempdir();
        write(dir.path(), "pyproject.toml", "[project\nname = \n");
        assert_eq!(
            detect(dir.path()),
            None,
            "a broken file is a reason to say 'not configured', not to refuse to print",
        );
    }

    #[test]
    fn unreadable_pyproject_is_not_configured() {
        let dir = tempdir();
        std::fs::create_dir(dir.path().join("pyproject.toml")).expect("create dir");
        assert_eq!(detect(dir.path()), None);
    }

    #[test]
    fn the_overlay_alone_is_configured() {
        let dir = tempdir();
        write(dir.path(), ".git/hooks.local.toml", "extend_check = [\"bandit\"]\n");
        assert_eq!(detect(dir.path()), Some(ConfigSource::Overlay));
    }

    #[test]
    fn the_shim_wins_over_pyproject_and_the_overlay() {
        let dir = tempdir();
        write(dir.path(), "hooks/pre-commit", crate::install::SHIM);
        write(dir.path(), "pyproject.toml", "[tool.madoqua]\n");
        write(dir.path(), ".git/hooks.local.toml", "log = \"x.jsonl\"\n");
        assert_eq!(
            detect(dir.path()),
            Some(ConfigSource::Shim),
            "the strongest evidence madoqua runs here is that git will run it",
        );
    }

    #[test]
    fn pyproject_wins_over_the_overlay() {
        let dir = tempdir();
        write(dir.path(), "pyproject.toml", "[tool.madoqua]\n");
        write(dir.path(), ".git/hooks.local.toml", "log = \"x.jsonl\"\n");
        assert_eq!(detect(dir.path()), Some(ConfigSource::PyProject));
    }

    #[test]
    fn tune_is_never_auto_selected() {
        for source in [
            None,
            Some(ConfigSource::Shim),
            Some(ConfigSource::PyProject),
            Some(ConfigSource::Overlay),
        ] {
            assert_ne!(
                auto_topic(source),
                Topic::Tune,
                "nothing about a repository's state says 'you need the reference right now'",
            );
        }
    }

    // --- Rendering ---

    #[test]
    fn explicit_header_omits_the_arrow() {
        assert_eq!(header(Topic::Tune, Selection::Explicit), "# madoqua guide: tune");
        assert_eq!(header(Topic::Setup, Selection::Explicit), "# madoqua guide: setup");
    }

    #[test]
    fn auto_header_names_the_reason() {
        assert_eq!(
            header(Topic::Setup, Selection::Auto(None)),
            "# madoqua guide: not configured here -> setup"
        );
        assert_eq!(
            header(Topic::Triage, Selection::Auto(Some(ConfigSource::Shim))),
            "# madoqua guide: configured via hooks/pre-commit -> triage"
        );
        assert_eq!(
            header(Topic::Triage, Selection::Auto(Some(ConfigSource::GitHookShim))),
            "# madoqua guide: configured via .git/hooks/pre-commit -> triage"
        );
        assert_eq!(
            header(Topic::Triage, Selection::Auto(Some(ConfigSource::PyProject))),
            "# madoqua guide: configured via pyproject.toml [tool.madoqua] -> triage"
        );
        assert_eq!(
            header(Topic::Triage, Selection::Auto(Some(ConfigSource::Overlay))),
            "# madoqua guide: configured via .git/hooks.local.toml -> triage"
        );
    }

    #[test]
    fn render_is_the_header_then_the_docs_page_verbatim() {
        let rendered = render(Topic::Triage, Selection::Explicit);
        assert_eq!(rendered, format!("# madoqua guide: triage\n\n{}", Topic::Triage.text()));
        assert!(
            rendered.ends_with(Topic::Triage.text()),
            "the CLI must emit the docs page byte for byte",
        );
    }

    #[test]
    fn every_topic_names_itself() {
        assert_eq!(Topic::Setup.name(), "setup");
        assert_eq!(Topic::Triage.name(), "triage");
        assert_eq!(Topic::Tune.name(), "tune");
    }
}
