//! The CLI surface: help, exit codes, `install`, and `stats`.

mod common;

use common::Repo;
use predicates::prelude::*;

#[test]
fn help_lists_the_commands() {
    let dir = tempfile::tempdir().unwrap();
    let assert = common::madoqua(dir.path()).arg("--help").assert().success();
    let stdout = common::stdout(&assert);

    for command in ["run", "install", "stats", "guide"] {
        assert!(stdout.contains(command), "`{command}` is missing from --help:\n{stdout}");
    }
}

#[test]
fn the_run_help_documents_the_overlay_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let assert = common::madoqua(dir.path()).args(["run", "--help"]).assert().success();
    let stdout = common::stdout(&assert);

    for phrase in ["hooks.local.toml", "extend_check", "MADOQUA_SKIP"] {
        assert!(stdout.contains(phrase), "`{phrase}` belongs in --help:\n{stdout}");
    }
}

#[test]
fn an_unknown_command_fails_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    common::madoqua(dir.path())
        .arg("no-such-command")
        .assert()
        // A usage error is madoqua unable to do its job, not a finding about
        // the code — exit 1 here would be a contract regression.
        .code(2)
        .stderr(predicate::str::contains("panicked").not());
}

#[test]
fn running_outside_a_repository_could_not_complete_rather_than_finding_something() {
    let dir = tempfile::tempdir().unwrap();
    common::madoqua(dir.path())
        .arg("run")
        .env("GIT_CEILING_DIRECTORIES", dir.path())
        .assert()
        .code(2);
}

#[test]
fn root_runs_against_another_repository_and_walks_up_to_its_root() {
    let repo = Repo::new();
    repo.tool("fakecheck", "exit 0");
    repo.write("pyproject.toml", "[tool.madoqua]\nfix = []\ncheck = [\"fakecheck\"]\n");
    repo.stage("src/a.py", "x = 1\n");

    // Standing somewhere else entirely, and pointing at a subdirectory rather
    // than the root: finding the root is `--root`'s whole job.
    let elsewhere = tempfile::tempdir().unwrap();
    let assert = common::madoqua(elsewhere.path())
        .args(["--root".as_ref(), repo.path().join("src").as_os_str()])
        .arg("run")
        .assert()
        .success();

    assert!(
        common::only_line(&assert.get_output().stdout).starts_with("pre-commit ok (1 py files"),
        "got: {}",
        common::stdout(&assert)
    );
}

#[test]
fn root_pointing_outside_a_repository_could_not_complete() {
    let here = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();

    common::madoqua(here.path())
        .args(["--root".as_ref(), elsewhere.path().as_os_str()])
        .arg("run")
        .env("GIT_CEILING_DIRECTORIES", elsewhere.path())
        .assert()
        .code(2);
}

#[test]
fn install_writes_an_executable_shim_and_points_git_at_it() {
    let repo = Repo::new();

    let assert = repo.madoqua().arg("install").assert().success();
    assert!(common::stdout(&assert).contains("hooks/pre-commit"), "install says what it did");

    let shim = repo.path().join("hooks/pre-commit");
    let contents = std::fs::read_to_string(&shim).unwrap();
    assert!(contents.starts_with("#!/bin/sh\n"), "sh, not bash: {contents}");
    assert!(
        contents.contains(".venv/bin/madoqua"),
        "the shim must try the repo's own venv first, since git runs it with the login \
         PATH and not the shell's: {contents}"
    );
    assert!(
        contents.trim_end().ends_with("exec madoqua run"),
        "and fall back to whatever `madoqua` is on PATH: {contents}"
    );
    assert_eq!(
        repo.git(&["config", "core.hooksPath"]).trim(),
        "hooks",
        "the shim is in the working tree so it can be committed and shared"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&shim).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0, "git will not run a shim it cannot execute; mode {mode:o}");
    }
}

#[test]
fn install_is_idempotent() {
    let repo = Repo::new();
    repo.madoqua().arg("install").assert().success();
    let first = std::fs::read_to_string(repo.path().join("hooks/pre-commit")).unwrap();
    repo.madoqua().arg("install").assert().success();

    assert_eq!(std::fs::read_to_string(repo.path().join("hooks/pre-commit")).unwrap(), first);
    assert_eq!(repo.git(&["config", "core.hooksPath"]).trim(), "hooks");
}

#[test]
fn an_installed_hook_runs_on_a_real_commit_and_logs_it() {
    let repo = Repo::new();
    repo.tool("fakefix", r##"for f in "$@"; do echo "# fixed" >> "$f"; done"##);
    repo.tool("fakecheck", "exit 0");
    repo.write("pyproject.toml", "[tool.madoqua]\nfix = [\"fakefix\"]\ncheck = [\"fakecheck\"]\n");
    repo.madoqua().arg("install").assert().success();
    repo.stage("a.py", "x = 1\n");

    let output = repo.commit("add a.py");

    assert!(
        output.status.success(),
        "the commit was blocked: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // git decides which of its own streams a hook's stdout lands on, so the
    // assertion is that the developer sees the verdict, not which pipe it is on.
    let seen = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(seen.contains("pre-commit ok (1 py files"), "the commit said nothing: {seen}");
    assert_eq!(
        repo.git(&["show", "HEAD:a.py"]),
        "x = 1\n# fixed\n",
        "the fixer's edits made it into the commit, not just into the working tree"
    );
    assert_eq!(repo.log_records().len(), 1, "the commit left a timing record behind");
}

/// The hook is run by git with the login `PATH`, not the shell's, so `madoqua`
/// as a dev dependency is invisible to it unless the shim looks in `.venv/bin`
/// itself. The decoy on `PATH` fails loudly: a commit that goes through proves
/// the shim never reached it.
#[test]
fn the_shim_runs_the_venv_binary_before_the_one_on_path() {
    let repo = Repo::new();
    repo.tool("fakecheck", "exit 0");
    repo.write("pyproject.toml", "[tool.madoqua]\nfix = []\ncheck = [\"fakecheck\"]\n");
    repo.madoqua().arg("install").assert().success();
    std::fs::copy(assert_cmd::cargo::cargo_bin("madoqua"), repo.path().join(".venv/bin/madoqua"))
        .unwrap();
    let decoy = tempfile::tempdir().unwrap();
    common::write_executable(
        &decoy.path().join("madoqua"),
        "#!/bin/sh\necho 'decoy madoqua on PATH was run' >&2\nexit 99\n",
    );
    repo.stage("a.py", "x = 1\n");

    let path = format!("{}:{}", decoy.path().display(), std::env::var("PATH").unwrap_or_default());
    let output = repo.commit_with_path("add a.py", &path);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "the commit was blocked: {stderr}");
    assert!(!stderr.contains("decoy"), "the shim fell through to PATH: {stderr}");
    assert_eq!(repo.log_records().len(), 1, "the venv binary ran the hook and logged it");
}

/// Without a binary in `.venv/bin` the shim falls back to `PATH`, which is
/// where a `uv tool install` puts it.
#[test]
fn the_shim_falls_back_to_path_when_the_venv_has_no_madoqua() {
    let repo = Repo::new();
    repo.tool("fakecheck", "exit 0");
    repo.write("pyproject.toml", "[tool.madoqua]\nfix = []\ncheck = [\"fakecheck\"]\n");
    repo.madoqua().arg("install").assert().success();
    repo.stage("a.py", "x = 1\n");
    assert!(!repo.path().join(".venv/bin/madoqua").exists(), "fixture has no venv binary");

    let output = repo.commit("add a.py");

    assert!(
        output.status.success(),
        "the commit was blocked: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(repo.log_records().len(), 1);
}

#[test]
fn a_blocked_commit_does_not_become_a_commit() {
    let repo = Repo::new();
    repo.tool("fakefail", "echo 'a.py:1: nope' >&2; exit 1");
    repo.write("pyproject.toml", "[tool.madoqua]\nfix = []\ncheck = [\"fakefail\"]\n");
    repo.madoqua().arg("install").assert().success();
    repo.stage("a.py", "x = 1\n");

    let output = repo.commit("add a.py");

    assert!(!output.status.success(), "exit 1 from the hook has to stop the commit");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("== fakefail failed =="),
        "and the developer has to be told which tool said no"
    );
    assert_eq!(
        repo.git(&["rev-list", "--count", "HEAD"]).trim(),
        "1",
        "still just the base commit"
    );
}

#[test]
fn stats_renders_a_table_of_what_the_log_holds() {
    let repo = Repo::new();
    repo.tool("fakecheck", "exit 0");
    repo.write("pyproject.toml", "[tool.madoqua]\nfix = []\ncheck = [\"fakecheck\"]\n");

    repo.stage("a.py", "x = 1\n");
    repo.madoqua().arg("run").assert().success();
    repo.stage("b.py", "y = 2\n");
    repo.madoqua().arg("run").assert().success();

    let assert = repo.madoqua().arg("stats").assert().success();
    let stdout = common::stdout(&assert);
    let lines: Vec<&str> = stdout.lines().collect();

    assert!(lines[0].starts_with("step"), "the table has a header: {stdout}");
    assert!(lines[0].contains("p95"), "and p95 is the column worth sorting by: {stdout}");
    assert!(lines[1].starts_with("fakecheck  "), "one row per step, left-aligned: {stdout}");
    assert!(
        lines.last().unwrap().starts_with("total run  "),
        "and the whole-run summary last: {stdout}"
    );
}

#[test]
fn stats_json_carries_the_same_numbers_and_stays_clean() {
    let repo = Repo::new();
    repo.tool("fakecheck", "exit 0");
    repo.write("pyproject.toml", "[tool.madoqua]\nfix = []\ncheck = [\"fakecheck\"]\n");
    repo.stage("a.py", "x = 1\n");
    repo.madoqua().arg("run").assert().success();

    let assert = repo.madoqua().args(["stats", "--json", "--days", "7"]).assert().success();
    let stdout = common::stdout(&assert);
    let report: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|err| panic!("not JSON: {err}\n{stdout}"));

    assert_eq!(report["days"], 7, "the window is echoed back so a saved report is self-describing");
    assert_eq!(report["runs"], 1);
    assert_eq!(report["steps"][0]["name"], "fakecheck");
    assert_eq!(report["steps"][0]["n"], 1);
    assert!(report["total"]["p95"].is_u64(), "got: {report}");
}

#[test]
fn stats_with_no_log_says_so_rather_than_failing() {
    let repo = Repo::new();
    repo.madoqua()
        .arg("stats")
        .assert()
        .success()
        .stdout(predicate::str::contains("no runs in the last 30 days"));
}

#[test]
fn stats_can_filter_a_shared_log_by_repository() {
    let repo = Repo::new();
    let log = repo.path().join("shared.jsonl");
    let line = |repo_name: &str, ms: u64| {
        format!(
            r#"{{"ts":"2026-08-31T12:03:22Z","repo":"{repo_name}","head":"abc1234","files":1,"total_ms":{ms},"venv_auto_activated":false,"steps":[{{"name":"ty","phase":"check","ms":{ms},"exit":0}}]}}"#
        )
    };
    std::fs::write(&log, format!("{}\n{}\n", line("mine", 100), line("theirs", 900))).unwrap();
    repo.write("pyproject.toml", &format!("[tool.madoqua]\nlog = \"{}\"\n", log.display()));

    // A fixed window, because the fixture's timestamps do not move with today.
    let assert = repo
        .madoqua()
        .args(["stats", "--json", "--repo", "mine", "--days", "36500"])
        .assert()
        .success();
    let report: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();

    assert_eq!(report["runs"], 1, "the other repo's runs are not mine to worry about");
    assert_eq!(report["total"]["max"], 100);
    assert_eq!(report["repo"], "mine");
}

#[test]
fn guide_prints_the_named_topic_on_stdout_and_exits_clean() {
    let dir = tempfile::tempdir().unwrap();
    let assert = common::madoqua(dir.path())
        .args(["guide", "tune"])
        .assert()
        // A guide is an inventory, not a verdict: it never returns 1, and it
        // never needs a repository either.
        .success();
    let stdout = common::stdout(&assert);

    assert!(stdout.starts_with("# madoqua guide: tune\n"), "got: {stdout}");
    assert!(stdout.contains("MADOQUA_SKIP"), "the tune guide is the reference: {stdout}");
    assert!(stdout.trim_end().ends_with("next: run `madoqua stats`"), "got: {stdout}");
    assert_eq!(common::stderr(&assert), "", "the guide is output, not logging");
}

#[test]
fn guide_with_no_topic_picks_setup_outside_a_configured_repo() {
    let dir = tempfile::tempdir().unwrap();
    let assert = common::madoqua(dir.path()).arg("guide").assert().success();
    let stdout = common::stdout(&assert);

    assert!(
        stdout.starts_with("# madoqua guide: not configured here -> setup"),
        "an agent needs to see why it got this topic: {stdout}"
    );
}

#[test]
fn guide_with_no_topic_picks_triage_once_the_hook_is_installed() {
    let repo = Repo::new();
    repo.madoqua().arg("install").assert().success();

    let assert = repo.madoqua().arg("guide").assert().success();
    let stdout = common::stdout(&assert);

    assert!(
        stdout.starts_with("# madoqua guide: configured via hooks/pre-commit -> triage"),
        "the installed shim is the strongest evidence madoqua runs here: {stdout}"
    );
}

#[test]
fn an_unknown_guide_topic_could_not_complete() {
    let dir = tempfile::tempdir().unwrap();
    common::madoqua(dir.path())
        .args(["guide", "how"])
        .assert()
        // A usage error is madoqua unable to do its job, not a finding.
        .code(2)
        .stderr(predicate::str::contains("how"));
}
