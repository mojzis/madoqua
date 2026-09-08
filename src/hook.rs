//! The hook itself: guard, fix, stage, check, and say one thing about it.
//!
//! The shape is inherited from the bash script this replaces, and the reason
//! for it is that a hook which prints on success trains people to stop reading
//! it. A clean run gets exactly one line; a failing run gets the failing
//! tools' output and nothing else.

use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::cli::Outcome;
use crate::config::{Config, Phase};
use crate::report::{RunRecord, StepRecord};
use crate::runner::StepResult;
use crate::{clock, git, runner, timelog, venv};

/// Run the hook for the repository containing `start`.
///
/// The verdict goes to `out`; everything else — guard failures, check output,
/// warnings — goes to stderr, so `madoqua run` stays pipeable.
pub fn run(start: &Path, out: &mut impl Write) -> Result<Outcome> {
    let started = Instant::now();
    let root = git::repo_root(start)?;
    // Where git keeps this repository's metadata, which is not `<root>/.git`
    // when the run is happening in a linked worktree.
    let git_dir = git::common_dir(&root)?;
    let config = Config::resolve(&root, &git_dir)?;

    // The guard comes before the file list, as it does in the bash version: a
    // repo whose venv is missing is misconfigured whether or not this
    // particular commit happens to touch Python.
    let venv = match venv::guard(&root) {
        Ok(venv) => venv,
        Err(err) => {
            eprintln!("{err}");
            return Ok(Outcome::FindingsReported);
        }
    };

    let files = git::staged_python_files(&root)?;
    if files.is_empty() {
        // Nothing to say, and nothing worth logging: an empty run would
        // dominate the percentiles of every repo that also commits prose.
        return Ok(Outcome::Clean);
    }

    let mut steps = runner::run_fixes(&root, &config.fix, &files, &venv.env);
    if !config.fix.is_empty() {
        git::add(&root, &files).context("cannot re-stage the files the fixers rewrote")?;
    }

    steps.extend(runner::run_checks(&root, &config.check, &files, &venv.env));

    let total = started.elapsed();
    write_log(&root, &git_dir, &config, &files, &steps, venv.auto_activated, total);

    let failures: Vec<&StepResult> = steps.iter().filter(|step| blocks(step)).collect();

    if failures.is_empty() {
        writeln!(out, "{}", success_line(files.len(), total, &steps, venv.auto_activated))?;
        Ok(Outcome::Clean)
    } else {
        eprint!("{}", failure_report(&failures));
        Ok(Outcome::FindingsReported)
    }
}

/// Whether a step is a reason to refuse the commit.
///
/// A fixer that exits non-zero is not: `ruff check --fix` exits non-zero for
/// what it could not fix, and the check that follows is about to say so. A
/// fixer that *never ran* is, because no check is going to report it — a
/// missing `ruff format` leaves `ruff check` perfectly happy, and the commit
/// would record unformatted code under a verdict claiming it was formatted.
fn blocks(step: &StepResult) -> bool {
    match step.phase {
        Phase::Check => step.failed(),
        Phase::Fix => step.could_not_start(),
    }
}

/// Append the run to the timing log, and never let that stop a commit.
fn write_log(
    root: &Path,
    git_dir: &Path,
    config: &Config,
    files: &[String],
    steps: &[StepResult],
    venv_auto_activated: bool,
    total: Duration,
) {
    let target = timelog::resolve(root, git_dir, config.log.as_deref());
    let record = RunRecord {
        ts: clock::format_rfc3339_utc(clock::now_unix()),
        repo: target.repo.clone(),
        head: git::head_short_sha(root).unwrap_or_default(),
        files: files.len(),
        total_ms: millis(total),
        venv_auto_activated,
        steps: steps.iter().map(step_record).collect(),
    };

    if let Err(err) = timelog::append(&target, &record) {
        eprintln!("madoqua: could not write the timing log: {err:#}");
    }
}

fn step_record(step: &StepResult) -> StepRecord {
    StepRecord {
        name: step.name.clone(),
        phase: step.phase.as_str().to_owned(),
        ms: millis(step.duration),
        exit: step.exit,
        timed_out: step.timed_out,
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// The one line a clean run prints.
///
/// The timings are in it because they are the cheapest possible nudge: a hook
/// that quietly grew to four seconds is one nobody notices until they start
/// committing less often.
fn success_line(
    files: usize,
    total: Duration,
    steps: &[StepResult],
    venv_auto_activated: bool,
) -> String {
    let names = |phase: Phase| -> Vec<&str> {
        steps.iter().filter(|s| s.phase == phase).map(|s| s.name.as_str()).collect()
    };

    let slowest = steps.iter().max_by_key(|step| step.duration).map_or_else(String::new, |step| {
        format!(", slowest: {} {}", step.name, seconds(step.duration))
    });
    let head = format!("{files} py files, {}{slowest}", seconds(total));

    let note = if venv_auto_activated { " (auto-activated .venv)" } else { "" };

    let mut clauses = Vec::new();
    let applied = names(Phase::Fix);
    if !applied.is_empty() {
        clauses.push(format!("{} applied & staged", applied.join(", ")));
    }
    let checked = names(Phase::Check);
    clauses.push(if checked.is_empty() {
        "no checks run".to_owned()
    } else {
        format!("{} passed", checked.join(", "))
    });

    format!("pre-commit ok ({head}){note}: {}", clauses.join("; "))
}

/// What a failing run prints: the failing tools, and only those.
fn failure_report(failures: &[&StepResult]) -> String {
    let mut out = String::new();
    for step in failures {
        let _ = writeln!(out, "== {} failed ==", step.name);
        out.push_str(&step.output);
        // A tool that ends without a newline would otherwise glue its last
        // line to the next tool's header.
        if !step.output.is_empty() && !step.output.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

fn seconds(duration: Duration) -> String {
    format!("{:.1}s", duration.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(name: &str, phase: Phase, ms: u64, exit: i32, output: &str) -> StepResult {
        StepResult {
            name: name.to_owned(),
            phase,
            duration: Duration::from_millis(ms),
            exit,
            timed_out: false,
            output: output.to_owned(),
        }
    }

    fn default_steps() -> Vec<StepResult> {
        vec![
            step("ruff fix", Phase::Fix, 41, 0, ""),
            step("ruff format", Phase::Fix, 30, 0, ""),
            step("ruff check", Phase::Check, 55, 0, ""),
            step("ty check", Phase::Check, 1620, 0, ""),
        ]
    }

    #[test]
    fn a_fixer_that_complained_does_not_block_but_one_that_never_ran_does() {
        let complained = step("ruff fix", Phase::Fix, 41, 1, "");
        let missing = step("ruff format", Phase::Fix, 0, 127, "madoqua: cannot run `ruff`\n");

        assert!(
            !blocks(&complained),
            "`ruff check --fix` exits non-zero for what it could not fix; the check says so"
        );
        assert!(
            blocks(&missing),
            "no check reports a missing formatter, so the commit would record unformatted code"
        );
    }

    #[test]
    fn a_check_that_passed_does_not_block_and_one_that_failed_does() {
        assert!(!blocks(&step("ty check", Phase::Check, 1, 0, "")));
        assert!(blocks(&step("ty check", Phase::Check, 1, 1, "nope\n")));
    }

    #[test]
    fn the_verdict_is_one_line_naming_the_count_the_time_and_the_slowest_step() {
        let line = success_line(3, Duration::from_millis(1834), &default_steps(), false);
        assert_eq!(
            line,
            "pre-commit ok (3 py files, 1.8s, slowest: ty check 1.6s): \
             ruff fix, ruff format applied & staged; ruff check, ty check passed"
        );
        assert!(!line.contains('\n'), "a hook that prints a paragraph on success gets ignored");
    }

    #[test]
    fn an_auto_activated_venv_is_announced_in_the_verdict() {
        let line = success_line(1, Duration::from_millis(100), &default_steps(), true);
        assert!(
            line.contains(") (auto-activated .venv): "),
            "the note sits between the counts and the summary, got: {line}"
        );
    }

    #[test]
    fn a_run_with_no_fixers_says_nothing_about_fixing() {
        let checks = vec![step("ruff check", Phase::Check, 55, 0, "")];
        let line = success_line(1, Duration::from_millis(60), &checks, false);
        assert_eq!(
            line, "pre-commit ok (1 py files, 0.1s, slowest: ruff check 0.1s): ruff check passed",
            "an empty fix list means no `applied & staged` clause at all"
        );
    }

    #[test]
    fn a_run_with_every_check_skipped_still_says_so() {
        let fixes = vec![step("ruff fix", Phase::Fix, 41, 0, "")];
        let line = success_line(1, Duration::from_millis(50), &fixes, false);
        assert!(
            line.ends_with("ruff fix applied & staged; no checks run"),
            "silence about checks would read as `checks passed`, got: {line}"
        );
    }

    #[test]
    fn failures_report_only_the_tools_that_failed() {
        let failed = step("ty check", Phase::Check, 100, 1, "a.py:1: error: nope\n");
        let report = failure_report(&[&failed]);
        assert_eq!(report, "== ty check failed ==\na.py:1: error: nope\n");
        assert!(!report.contains("ruff"), "a passing tool has nothing to contribute");
    }

    #[test]
    fn output_without_a_trailing_newline_does_not_run_into_the_next_header() {
        let first = step("a", Phase::Check, 1, 1, "no newline here");
        let second = step("b", Phase::Check, 1, 1, "");
        assert_eq!(
            failure_report(&[&first, &second]),
            "== a failed ==\nno newline here\n== b failed ==\n"
        );
    }
}
