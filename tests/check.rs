//! End-to-end behaviour of `madoqua check`, the whole-repo run, against real
//! git repositories. The tools are stubs in the repo's `.venv/bin`.

mod common;

use common::Repo;

/// A repo with a fixer that would leave a mark, a check that records the
/// files it was handed, and some history to check.
fn wired() -> Repo {
    let repo = Repo::new();
    repo.tool("fakefix", r##"for f in "$@"; do echo "# fixed" >> "$f"; done"##);
    repo.tool("fakecheck", r#"printf '%s\n' "$@" > .seen-$(basename "$0"); exit 0"#);
    repo.write(".gitignore", ".venv/\n.seen-*\nignored.py\n");
    repo.write("pyproject.toml", "[tool.madoqua]\nfix = [\"fakefix\"]\ncheck = [\"fakecheck\"]\n");
    repo.stage("a.py", "x = 1\n").stage("pkg/b.py", "y = 2\n").stage("README.md", "hi\n");
    repo.git(&["add", ".gitignore", "pyproject.toml"]);
    repo.git(&["commit", "-qm", "code"]);
    repo
}

fn seen(repo: &Repo, tool: &str) -> Vec<String> {
    repo.read(&format!(".seen-{tool}")).lines().map(ToOwned::to_owned).collect()
}

#[test]
fn check_runs_every_check_on_every_matching_file_and_fixes_nothing() {
    let repo = wired();

    let assert = repo.madoqua().arg("check").assert().success();
    let line = common::only_line(&assert.get_output().stdout);

    assert!(line.starts_with("check ok (2 files, "), "got: {line}");
    assert!(line.ends_with(": fakecheck passed"), "no fixer is named, got: {line}");
    assert_eq!(seen(&repo, "fakecheck"), ["a.py", "pkg/b.py"], "every Python file, nothing staged");
    assert_eq!(repo.read("a.py"), "x = 1\n", "a whole-repo run never rewrites a file");
    assert!(repo.git(&["status", "--porcelain"]).trim().is_empty(), "and never stages anything");
    assert!(!repo.log_exists(), "a full run would swamp the hook's percentiles");
}

#[test]
fn check_sees_untracked_files_but_not_ignored_or_deleted_ones() {
    let repo = wired();
    repo.write("new.py", "z = 3\n").write("ignored.py", "nope\n");
    std::fs::remove_file(repo.path().join("pkg/b.py")).unwrap();

    repo.madoqua().arg("check").assert().success();
    assert_eq!(
        seen(&repo, "fakecheck"),
        ["a.py", "new.py"],
        "what a commit could contain: a new file is in, an ignored or deleted one is not"
    );
}

#[test]
fn a_failing_check_fails_the_full_run_with_its_output() {
    let repo = wired();
    repo.tool("fakefail", "echo 'a.py:1: nope' >&2; exit 1");
    repo.write("pyproject.toml", "[tool.madoqua]\ncheck = [\"fakecheck\", \"fakefail\"]\n");

    let assert = repo.madoqua().arg("check").assert().code(1);
    assert_eq!(common::stderr(&assert), "== fakefail failed ==\na.py:1: nope\n");
    assert!(assert.get_output().stdout.is_empty(), "no verdict line when the run fails");
}

#[test]
fn full_cmd_is_what_check_runs_and_cmd_is_what_the_hook_runs() {
    let repo = wired();
    repo.tool("scan", r#"printf '%s\n' "$@" > .seen-scan"#);
    repo.write(
        "pyproject.toml",
        "[tool.madoqua]\nfix = []\n\
         check = [{ name = \"scan\", cmd = \"scan --focus-args\", full_cmd = \"scan .\" }]\n",
    );

    let assert = repo.madoqua().arg("check").assert().success();
    assert!(common::only_line(&assert.get_output().stdout).ends_with(": scan passed"));
    assert_eq!(seen(&repo, "scan"), ["."], "`full_cmd` as written, with no file list");

    repo.stage("a.py", "x = 2\n");
    repo.madoqua().arg("run").assert().success();
    assert_eq!(
        seen(&repo, "scan"),
        ["--focus-args", "a.py"],
        "the hook keeps `cmd` and the staged files"
    );
}

#[test]
fn when_keeps_a_check_to_one_of_the_two_runs() {
    let repo = wired();
    repo.write(
        "pyproject.toml",
        "[tool.madoqua]\nfix = []\ncheck = [\n\
         { name = \"hook only\", cmd = \"fakecheck\", when = \"commit\" },\n\
         { name = \"full only\", cmd = \"fakecheck\", when = \"full\" },\n]\n",
    );

    let assert = repo.madoqua().arg("check").assert().success();
    let line = common::only_line(&assert.get_output().stdout);
    assert!(line.ends_with(": full only passed"), "got: {line}");

    repo.stage("a.py", "x = 2\n");
    let assert = repo.madoqua().arg("run").assert().success();
    let line = common::only_line(&assert.get_output().stdout);
    assert!(line.ends_with(": hook only passed"), "got: {line}");
}

#[test]
fn an_advisory_failure_is_shown_but_does_not_fail_either_run() {
    let repo = wired();
    repo.tool("audit", "echo 'CVE-0000 in somelib'; exit 1");
    repo.write(
        "pyproject.toml",
        "[tool.madoqua]\nfix = []\ncheck = [\"fakecheck\", \
         { name = \"audit\", cmd = \"audit\", pass_files = false, blocking = false }]\n",
    );

    let assert = repo.madoqua().arg("check").assert().success();
    let line = common::only_line(&assert.get_output().stdout);
    assert!(
        line.ends_with(": fakecheck passed; audit failed (advisory)"),
        "the verdict must not say the advisory step passed, got: {line}"
    );
    assert_eq!(
        common::stderr(&assert),
        "== audit failed (advisory) ==\nCVE-0000 in somelib\n",
        "its output is still there to read"
    );

    repo.stage("a.py", "x = 2\n");
    let assert = repo.madoqua().arg("run").assert().success();
    let line = common::only_line(&assert.get_output().stdout);
    assert!(
        line.ends_with("audit failed (advisory)"),
        "the hook lets the commit through, got: {line}"
    );
    assert_eq!(repo.log_records()[0]["steps"][1]["exit"], 1, "and logs the real exit code");
}

#[test]
fn an_advisory_failure_is_reported_alongside_a_blocking_one() {
    let repo = wired();
    repo.tool("audit", "echo advisory; exit 1");
    repo.tool("fakefail", "echo blocking; exit 1");
    repo.write(
        "pyproject.toml",
        "[tool.madoqua]\ncheck = [\"fakefail\", \
         { name = \"audit\", cmd = \"audit\", blocking = false }]\n",
    );

    let assert = repo.madoqua().arg("check").assert().code(1);
    assert_eq!(
        common::stderr(&assert),
        "== fakefail failed ==\nblocking\n== audit failed (advisory) ==\nadvisory\n"
    );
}

#[test]
fn madoqua_skip_drops_a_check_from_the_full_run_too() {
    let repo = wired();
    repo.tool("fakefail", "exit 1");
    repo.write("pyproject.toml", "[tool.madoqua]\ncheck = [\"fakecheck\", \"fakefail\"]\n");

    let assert = repo.madoqua().arg("check").env("MADOQUA_SKIP", "fakefail").assert().success();
    assert!(common::only_line(&assert.get_output().stdout).ends_with(": fakecheck passed"));
}

#[test]
fn a_repo_with_nothing_to_check_still_answers() {
    let repo = Repo::new();
    repo.tool("fakecheck", "exit 0");
    repo.write("pyproject.toml", "[tool.madoqua]\ncheck = [\"fakecheck\"]\n");

    let assert = repo.madoqua().arg("check").assert().success();
    let line = common::only_line(&assert.get_output().stdout);
    assert!(
        line.starts_with("check ok (0 files, ") && line.ends_with(": no checks run"),
        "a command someone typed says what it did, even when that was nothing; got: {line}"
    );
}

#[test]
fn a_missing_venv_fails_the_full_run_as_a_finding() {
    let repo = Repo::without_venv();
    let assert = repo.madoqua().arg("check").assert().code(1);
    assert!(common::stderr(&assert).contains(".venv"), "got: {}", common::stderr(&assert));
}

#[test]
fn a_bad_when_could_not_complete() {
    let repo = wired();
    repo.write(
        "pyproject.toml",
        "[tool.madoqua]\ncheck = [{ cmd = \"fakecheck\", when = \"ci\" }]\n",
    );

    let assert = repo.madoqua().arg("check").assert().code(2);
    let stderr = common::stderr(&assert);
    assert!(stderr.contains("\"ci\""), "the error names the value, got: {stderr}");
    assert!(stderr.contains("pyproject.toml"), "and the file, got: {stderr}");
}

#[test]
fn check_in_a_linked_worktree_checks_the_worktree() {
    let repo = wired();
    let worktree = repo.worktree("task");
    worktree.write("only-here.py", "w = 1\n");

    worktree.madoqua().arg("check").assert().success();
    assert_eq!(seen(&worktree, "fakecheck"), ["a.py", "only-here.py", "pkg/b.py"]);
}

#[test]
fn a_file_outside_a_sparse_checkout_is_not_handed_to_a_tool() {
    let repo = wired();
    // What `git sparse-checkout` does to a path outside the cone: marked
    // skip-worktree, and absent from disk without git calling it deleted.
    repo.git(&["update-index", "--skip-worktree", "pkg/b.py"]);
    std::fs::remove_file(repo.path().join("pkg/b.py")).unwrap();

    repo.madoqua().arg("check").assert().success();
    assert_eq!(
        seen(&repo, "fakecheck"),
        ["a.py"],
        "a path that is not on disk is not a file to check"
    );
}

#[test]
fn a_conflicted_file_is_checked_once() {
    let repo = wired();
    repo.git(&["checkout", "-qb", "other"]);
    repo.stage("a.py", "x = 'other'\n");
    repo.git(&["commit", "-qm", "other"]);
    repo.git(&["checkout", "-q", "main"]);
    repo.stage("a.py", "x = 'main'\n");
    repo.git(&["commit", "-qm", "main"]);
    let merge = repo.git_command().args(["merge", "-q", "other"]).output().unwrap();
    assert!(!merge.status.success(), "the fixture needs a conflict on a.py");

    repo.madoqua().arg("check").assert().success();
    assert_eq!(
        seen(&repo, "fakecheck"),
        ["a.py", "pkg/b.py"],
        "git lists a conflicted path once per stage; a tool should see it once"
    );
}
