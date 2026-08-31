//! End-to-end checks on the CLI surface: exit codes, `--help`, and the JSON
//! wire format. These run the real binary.

mod common;

use common::madoqua;
use predicates::prelude::*;

#[test]
fn help_lists_the_commands() {
    let dir = tempfile::tempdir().unwrap();
    madoqua(dir.path()).arg("--help").assert().success().stdout(predicate::str::contains("doctor"));
}

#[test]
fn an_unknown_command_fails_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    madoqua(dir.path()).arg("no-such-command").assert().failure();
}

#[test]
fn doctor_exits_zero_and_names_the_version() {
    let dir = tempfile::tempdir().unwrap();
    madoqua(dir.path())
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("madoqua "));
}

#[test]
fn doctor_json_is_parseable_and_carries_the_version() {
    let dir = tempfile::tempdir().unwrap();
    let out = madoqua(dir.path()).args(["doctor", "--json"]).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();

    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|err| panic!("not JSON: {err}\n{stdout}"));
    assert_eq!(
        parsed["version"],
        serde_json::Value::from(env!("CARGO_PKG_VERSION")),
        "the JSON form carries the same version the text form prints"
    );
}
