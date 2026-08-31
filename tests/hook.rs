//! End-to-end behaviour of `madoqua run` against real git repositories.
//!
//! Every test here fixes a behaviour the bash hook had, or one the port added
//! on purpose. The tools are stubs in the repo's `.venv/bin`, so nothing needs
//! ruff or ty installed to run these.

mod common;

use common::Repo;

/// A repo wired with a fixer that appends a marker and a check that passes.
fn wired() -> Repo {
    let repo = Repo::new();
    repo.tool("fakefix", r##"for f in "$@"; do echo "# fixed" >> "$f"; done"##);
    repo.tool("fakecheck", "exit 0");
    repo.write("pyproject.toml", "[tool.madoqua]\nfix = [\"fakefix\"]\ncheck = [\"fakecheck\"]\n");
    repo
}

#[test]
fn a_clean_run_says_one_line_restages_the_fixes_and_logs_the_timings() {
    let repo = wired();
    repo.stage("a.py", "x = 1\n");

    let assert = repo.madoqua().arg("run").assert().success();
    let line = common::only_line(&assert.get_output().stdout);

    assert!(line.starts_with("pre-commit ok (1 py files, "), "got: {line}");
    assert!(line.contains("slowest: "), "the verdict carries timings, got: {line}");
    assert!(
        line.contains("(auto-activated .venv)"),
        "the stub venv was not on PATH, so the hook had to activate it; got: {line}"
    );
    assert!(
        line.ends_with(": fakefix applied & staged; fakecheck passed"),
        "the summary names the fixers and the checks, got: {line}"
    );

    assert_eq!(
        repo.staged_content("a.py"),
        "x = 1\n# fixed\n",
        "what the fixer wrote has to be staged, or the commit records the unfixed file"
    );

    let records = repo.log_records();
    assert_eq!(records.len(), 1, "one run, one log line");
    assert_eq!(records[0]["files"], 1);
    assert_eq!(records[0]["venv_auto_activated"], true);
    assert_eq!(records[0]["head"].as_str().unwrap().len(), 7, "the short sha of the parent commit");
    assert!(records[0]["total_ms"].is_u64(), "got: {}", records[0]);
    assert_eq!(
        records[0]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|step| (step["name"].as_str().unwrap(), step["phase"].as_str().unwrap()))
            .collect::<Vec<_>>(),
        [("fakefix", "fix"), ("fakecheck", "check")],
        "fixes are logged before checks, each with its phase"
    );
}

#[test]
fn a_failing_check_reports_itself_and_nothing_else() {
    let repo = wired();
    repo.tool("fakefail", "echo 'a.py:1: nope' >&2; exit 1");
    repo.write(
        "pyproject.toml",
        "[tool.madoqua]\nfix = []\ncheck = [\"fakecheck\", \"fakefail\"]\n",
    );
    repo.stage("a.py", "x = 1\n");

    let assert = repo.madoqua().arg("run").assert().code(1);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    assert_eq!(
        stderr, "== fakefail failed ==\na.py:1: nope\n",
        "only the failing tool gets to speak, and it speaks on stderr"
    );
    assert!(
        assert.get_output().stdout.is_empty(),
        "there is no verdict line when the commit is blocked"
    );
    assert_eq!(repo.log_records().len(), 1, "a failed run is still worth timing");
}

#[test]
fn nothing_staged_in_python_means_nothing_at_all() {
    let repo = wired();
    repo.stage("notes.txt", "hello\n");

    let assert = repo.madoqua().arg("run").assert().success();
    assert!(assert.get_output().stdout.is_empty(), "a hook with nothing to do stays quiet");
    assert!(assert.get_output().stderr.is_empty(), "on both streams");
    assert!(
        !repo.log_exists(),
        "a no-op run would otherwise dominate the percentiles of a repo that also commits prose"
    );
}

#[test]
fn a_check_with_pass_files_false_receives_no_file_arguments() {
    let repo = wired();
    repo.tool("argc", r#"echo "argc=$#"; exit 1"#);
    repo.write(
        "pyproject.toml",
        "[tool.madoqua]\nfix = []\ncheck = [{ name = \"argc\", cmd = \"argc\", pass_files = false }]\n",
    );
    repo.stage("a.py", "x = 1\n");
    repo.stage("b.py", "y = 2\n");

    let assert = repo.madoqua().arg("run").assert().code(1);
    assert_eq!(
        String::from_utf8_lossy(&assert.get_output().stderr),
        "== argc failed ==\nargc=0\n",
        "a repo-wide tool must not be handed the staged file list"
    );
}

#[test]
fn a_check_with_pass_files_receives_every_staged_file() {
    let repo = wired();
    repo.tool("argc", r#"echo "argc=$#"; exit 1"#);
    repo.write("pyproject.toml", "[tool.madoqua]\nfix = []\ncheck = [\"argc\"]\n");
    repo.stage("a.py", "x = 1\n");
    repo.stage("b.pyi", "y: int\n");
    repo.stage("c.txt", "not python\n");

    let assert = repo.madoqua().arg("run").assert().code(1);
    assert_eq!(
        String::from_utf8_lossy(&assert.get_output().stderr),
        "== argc failed ==\nargc=2\n",
        ".py and .pyi are the hook's business; .txt is not"
    );
}

#[test]
fn the_overlay_replaces_the_repos_list() {
    let repo = wired();
    repo.tool("fakefail", "echo boom >&2; exit 1");
    repo.write("pyproject.toml", "[tool.madoqua]\nfix = []\ncheck = [\"fakefail\"]\n");
    repo.write(".git/hooks.local.toml", "check = [\"fakecheck\"]\n");
    repo.stage("a.py", "x = 1\n");

    let assert = repo.madoqua().arg("run").assert().success();
    assert!(
        common::only_line(&assert.get_output().stdout).ends_with(": fakecheck passed"),
        "`check` in the overlay replaces the repo's list rather than adding to it"
    );
}

#[test]
fn the_overlay_can_append_instead() {
    let repo = wired();
    repo.tool("fakefail", "echo boom >&2; exit 1");
    repo.write("pyproject.toml", "[tool.madoqua]\nfix = []\ncheck = [\"fakecheck\"]\n");
    repo.write(".git/hooks.local.toml", "extend_check = [\"fakefail\"]\n");
    repo.stage("a.py", "x = 1\n");

    let assert = repo.madoqua().arg("run").assert().code(1);
    assert_eq!(
        String::from_utf8_lossy(&assert.get_output().stderr),
        "== fakefail failed ==\nboom\n",
        "`extend_check` keeps the repo's checks and adds the personal one"
    );
}

#[test]
fn madoqua_skip_drops_a_check_by_name_for_one_run() {
    let repo = wired();
    repo.tool("fakefail", "echo boom >&2; exit 1");
    repo.write(
        "pyproject.toml",
        "[tool.madoqua]\nfix = []\ncheck = [\"fakecheck\", { name = \"slow\", cmd = \"fakefail\" }]\n",
    );
    repo.stage("a.py", "x = 1\n");

    repo.madoqua().arg("run").assert().code(1);

    let assert = repo.madoqua().env("MADOQUA_SKIP", "slow").arg("run").assert().success();
    let line = common::only_line(&assert.get_output().stdout);
    assert!(
        line.ends_with(": fakecheck passed"),
        "the skipped check is not claimed to have passed"
    );
}

#[test]
fn max_output_lines_keeps_the_head_and_counts_the_rest() {
    let repo = wired();
    repo.tool("noisy", "seq 1 5; exit 1");
    repo.write(
        "pyproject.toml",
        "[tool.madoqua]\nfix = []\ncheck = [{ name = \"noisy\", cmd = \"noisy\", max_output_lines = 2 }]\n",
    );
    repo.stage("a.py", "x = 1\n");

    let assert = repo.madoqua().arg("run").assert().code(1);
    assert_eq!(
        String::from_utf8_lossy(&assert.get_output().stderr),
        "== noisy failed ==\n1\n2\n... (3 lines truncated)\n",
        "the cap is what stops a thousand-line traceback filling an agent's context"
    );
}

#[test]
fn a_check_that_overruns_its_timeout_is_killed_and_logged_as_one() {
    let repo = wired();
    repo.tool("hang", "sleep 30");
    repo.write(
        "pyproject.toml",
        "[tool.madoqua]\nfix = []\ncheck = [{ name = \"hang\", cmd = \"hang\", timeout_s = 1 }]\n",
    );
    repo.stage("a.py", "x = 1\n");

    let started = std::time::Instant::now();
    let assert = repo.madoqua().arg("run").assert().code(1);
    let elapsed = started.elapsed();

    assert!(
        String::from_utf8_lossy(&assert.get_output().stderr).contains("was killed after 1s"),
        "the report must say why there is no tool output to show"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "a timeout that still waits for the tool's descendants is not a timeout; took {elapsed:?}"
    );

    let step = &repo.log_records()[0]["steps"][0];
    assert_eq!(step["exit"], -1, "a kill is logged as -1");
    assert_eq!(step["timed_out"], true, "and marked, so stats can tell it from a real failure");
}

#[test]
fn a_missing_venv_blocks_the_commit_with_instructions() {
    let repo = Repo::without_venv();
    repo.stage("a.py", "x = 1\n");

    let assert = repo.madoqua().arg("run").assert().code(1);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    assert!(stderr.contains("no virtualenv at"), "got: {stderr}");
    assert!(stderr.contains(".venv"), "the message names the path it wanted, got: {stderr}");
    assert!(stderr.contains("uv venv"), "and how to make one, got: {stderr}");
    assert!(assert.get_output().stdout.is_empty(), "no verdict when the guard refuses");
    assert!(!repo.log_exists(), "a run that never started has no timings to record");
}

#[test]
fn a_fixers_failure_does_not_stop_the_run() {
    let repo = wired();
    repo.tool("grumpy", r##"for f in "$@"; do echo "# fixed" >> "$f"; done; exit 1"##);
    repo.write("pyproject.toml", "[tool.madoqua]\nfix = [\"grumpy\"]\ncheck = [\"fakecheck\"]\n");
    repo.stage("a.py", "x = 1\n");

    let assert = repo.madoqua().arg("run").assert().success();
    assert!(
        common::only_line(&assert.get_output().stdout).contains("grumpy applied & staged"),
        "`ruff check --fix` exits non-zero for what it could not fix; that is the checks' news"
    );
    assert_eq!(repo.staged_content("a.py"), "x = 1\n# fixed\n", "and its edits are still staged");
    assert_eq!(repo.log_records()[0]["steps"][0]["exit"], 1, "the log still records what happened");
}

#[test]
fn a_shell_operator_in_a_command_is_refused_before_anything_runs() {
    let repo = wired();
    repo.write("pyproject.toml", "[tool.madoqua]\ncheck = [\"fakecheck | tee out\"]\n");
    repo.stage("a.py", "x = 1\n");

    let assert = repo.madoqua().arg("run").assert().code(2);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        stderr.contains("needs a shell"),
        "a config madoqua cannot honour is a broken run, not a finding; got: {stderr}"
    );
}

#[test]
fn the_log_can_be_pointed_outside_the_repo_and_then_names_the_repo() {
    let repo = wired();
    let elsewhere = tempfile::tempdir().unwrap();
    let log = elsewhere.path().join("nested/timings.jsonl");
    repo.write(
        "pyproject.toml",
        &format!(
            "[tool.madoqua]\nfix = []\ncheck = [\"fakecheck\"]\nlog = \"{}\"\n",
            log.display()
        ),
    );
    repo.stage("a.py", "x = 1\n");

    repo.madoqua().arg("run").assert().success();

    let text = std::fs::read_to_string(&log).expect("madoqua creates the parent directories");
    let record: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert!(
        record["repo"].is_string(),
        "a shared log mixes repos, so each line has to name its own; got: {record}"
    );
    assert!(!repo.log_exists(), "and nothing is written to the default path");
}

#[test]
fn an_unwritable_log_warns_but_lets_the_commit_through() {
    let repo = wired();
    repo.write(
        "pyproject.toml",
        "[tool.madoqua]\nfix = []\ncheck = [\"fakecheck\"]\nlog = \"blocked/timings.jsonl\"\n",
    );
    // A file where madoqua wants a directory: `create_dir_all` cannot win.
    repo.write("blocked", "in the way\n");
    repo.stage("a.py", "x = 1\n");

    let assert = repo.madoqua().arg("run").assert().success();
    assert!(
        common::only_line(&assert.get_output().stdout).starts_with("pre-commit ok"),
        "timings are a nice-to-have; blocking a commit over one would not be"
    );
    assert!(
        String::from_utf8_lossy(&assert.get_output().stderr)
            .contains("could not write the timing log"),
        "but it says so, once, on stderr"
    );
}
