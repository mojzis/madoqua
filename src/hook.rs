//! The hook itself, and the whole-repo run built from the same parts: guard,
//! fix, stage, check, and say one thing about it.
//!
//! The shape is inherited from the bash script this replaces, and the reason
//! for it is that a hook which prints on success trains people to stop reading
//! it. A clean run gets exactly one line; a failing run gets the failing
//! tools' output and nothing else.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::cli::Outcome;
use crate::config::{Config, Mode, Phase, Step};
use crate::report::{RunRecord, StepRecord};
use crate::runner::{Job, StepResult};
use crate::venv::Venv;
use crate::{clock, git, runner, timelog, venv};

/// What both runs need before they look at any file: where the repository
/// is, what to run in it, and a venv that passed the guard.
struct Prepared {
    root: PathBuf,
    git_dir: PathBuf,
    config: Config,
    venv: Venv,
}

/// Find the repository, resolve the configuration for `mode`, and guard the
/// venv. `None` means the guard refused and has already said why on stderr.
fn prepare(start: &Path, mode: Mode) -> Result<Option<Prepared>> {
    let root = git::repo_root(start)?;
    // Where git keeps this repository's metadata, which is not `<root>/.git`
    // when the run is happening in a linked worktree.
    let git_dir = git::common_dir(&root)?;
    let config = Config::resolve(&root, &git_dir)?.for_mode(mode);

    // The guard comes before the file list, as it does in the bash version: a
    // repo whose venv is missing is misconfigured whether or not this
    // particular commit happens to touch Python.
    match venv::guard(&root) {
        Ok(venv) => Ok(Some(Prepared { root, git_dir, config, venv })),
        Err(err) => {
            eprintln!("{err}");
            Ok(None)
        }
    }
}

/// Run the hook for the repository containing `start`.
///
/// The verdict goes to `out`; everything else — guard failures, check output,
/// warnings — goes to stderr, so `madoqua run` stays pipeable.
pub fn run(start: &Path, out: &mut impl Write) -> Result<Outcome> {
    let started = Instant::now();
    let Some(Prepared { root, git_dir, config, venv }) = prepare(start, Mode::Commit)? else {
        return Ok(Outcome::FindingsReported);
    };

    let staged = git::staged_files(&root)?;
    let fixes = jobs(&config.fix, &staged);
    let checks = jobs(&config.check, &staged);
    if fixes.is_empty() && checks.is_empty() {
        // Nothing to say, and nothing worth logging: an empty run would
        // dominate the percentiles of every repo that also commits prose.
        return Ok(Outcome::Clean);
    }
    let file_count = covered(&staged, fixes.iter().chain(&checks)).len();

    // Only what a fixer ran on: re-adding a file nothing rewrote would stage
    // edits the developer deliberately left out of this commit.
    let rewritten = covered(&staged, &fixes);
    let mut steps = runner::run_fixes(&root, &fixes, &venv.env);
    git::add(&root, &rewritten).context("cannot re-stage the files the fixers rewrote")?;

    steps.extend(runner::run_checks(&root, &checks, &venv.env));

    let total = started.elapsed();
    write_log(&root, &git_dir, &config, file_count, &steps, venv.auto_activated, total);

    verdict(out, Mode::Commit, file_count, total, &steps, venv.auto_activated)
}

/// Check the whole repository containing `start`: every check, as if every
/// file in it were staged, and nothing fixed, staged or logged.
///
/// "Every file" is what a commit could contain — tracked files and untracked
/// ones git does not ignore, minus those deleted from the working tree — so a
/// new file is checked before anyone remembers to `git add` it.
///
/// Not logged: the timing log answers "is my hook getting slow", and a run of
/// the whole test suite in it would swamp the percentiles that question is
/// asked of.
pub fn check(start: &Path, out: &mut impl Write) -> Result<Outcome> {
    let started = Instant::now();
    let Some(Prepared { root, config, venv, .. }) = prepare(start, Mode::Full)? else {
        return Ok(Outcome::FindingsReported);
    };

    let files = git::repo_files(&root)?;
    let checks = jobs(&config.check, &files);
    let file_count = covered(&files, &checks).len();
    let steps = runner::run_checks(&root, &checks, &venv.env);

    verdict(out, Mode::Full, file_count, started.elapsed(), &steps, venv.auto_activated)
}

/// Say how the run went: the one line on stdout when nothing blocks, the
/// failing tools on stderr when something does.
///
/// A failed advisory step is on stderr either way — it is information, not
/// the verdict — and is named in the one line, so a clean run cannot read as
/// though it passed.
fn verdict(
    out: &mut impl Write,
    mode: Mode,
    files: usize,
    total: Duration,
    steps: &[StepResult],
    venv_auto_activated: bool,
) -> Result<Outcome> {
    let reported: Vec<&StepResult> =
        steps.iter().filter(|step| blocks(step) || is_advisory_failure(step)).collect();
    eprint!("{}", failure_report(&reported));

    if steps.iter().any(blocks) {
        return Ok(Outcome::FindingsReported);
    }
    writeln!(out, "{}", success_line(mode, files, total, steps, venv_auto_activated))
        .context("cannot write to stdout")?;
    Ok(Outcome::Clean)
}

/// The steps that have something to run on, each with its share of `files`
/// — the staged files in the hook, every file in the full run. A step whose
/// `on` matched nothing is left out entirely: it is not run, not logged and
/// not named in the verdict.
fn jobs<'a>(steps: &'a [Step], files: &[String]) -> Vec<Job<'a>> {
    steps
        .iter()
        .map(|step| Job { step, files: step.select(files) })
        .filter(|job| !job.files.is_empty())
        .collect()
}

/// The files at least one of `jobs` runs on, in the order of `files`.
fn covered<'a>(files: &[String], jobs: impl IntoIterator<Item = &'a Job<'a>>) -> Vec<String> {
    // A set rather than a scan per job: the full run asks this of every file
    // in the repository.
    let used: HashSet<&str> =
        jobs.into_iter().flat_map(|job| job.files.iter().map(String::as_str)).collect();
    files.iter().filter(|path| used.contains(path.as_str())).cloned().collect()
}

/// Whether a step is a reason to refuse the commit.
///
/// A fixer that exits non-zero is not: `ruff check --fix` exits non-zero for
/// what it could not fix, and the check that follows is about to say so. A
/// fixer that *never ran* is, because no check is going to report it — a
/// missing `ruff format` leaves `ruff check` perfectly happy, and the commit
/// would record unformatted code under a verdict claiming it was formatted.
///
/// A check that is not `blocking` never is: it is reported and the run passes.
fn blocks(step: &StepResult) -> bool {
    match step.phase {
        Phase::Check => step.blocking && step.failed(),
        Phase::Fix => step.could_not_start(),
    }
}

/// A check that failed but was configured not to block.
fn is_advisory_failure(step: &StepResult) -> bool {
    step.phase == Phase::Check && !step.blocking && step.failed()
}

/// Append the run to the timing log, and never let that stop a commit.
fn write_log(
    root: &Path,
    git_dir: &Path,
    config: &Config,
    files: usize,
    steps: &[StepResult],
    venv_auto_activated: bool,
    total: Duration,
) {
    let target = timelog::resolve(root, git_dir, config.log.as_deref());
    let record = RunRecord {
        ts: clock::format_rfc3339_utc(clock::now_unix()),
        repo: target.repo.clone(),
        head: git::head_short_sha(root).unwrap_or_default(),
        files,
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
    mode: Mode,
    files: usize,
    total: Duration,
    steps: &[StepResult],
    venv_auto_activated: bool,
) -> String {
    let slowest = steps.iter().max_by_key(|step| step.duration).map_or_else(String::new, |step| {
        format!(", slowest: {} {}", step.name, seconds(step.duration))
    });
    let head = format!("{files} files, {}{slowest}", seconds(total));

    let note = if venv_auto_activated { " (auto-activated .venv)" } else { "" };

    let mut clauses = Vec::new();
    let applied: Vec<&str> =
        steps.iter().filter(|s| s.phase == Phase::Fix).map(|s| s.name.as_str()).collect();
    if !applied.is_empty() {
        clauses.push(format!("{} applied & staged", applied.join(", ")));
    }
    let checks: Vec<&StepResult> = steps.iter().filter(|s| s.phase == Phase::Check).collect();
    let passed: Vec<&str> =
        checks.iter().filter(|s| !s.failed()).map(|s| s.name.as_str()).collect();
    let advised: Vec<&str> =
        checks.iter().filter(|s| is_advisory_failure(s)).map(|s| s.name.as_str()).collect();
    if checks.is_empty() {
        clauses.push("no checks run".to_owned());
    }
    if !passed.is_empty() {
        clauses.push(format!("{} passed", passed.join(", ")));
    }
    if !advised.is_empty() {
        clauses.push(format!("{} failed (advisory)", advised.join(", ")));
    }

    let label = match mode {
        Mode::Commit => "pre-commit ok",
        Mode::Full => "check ok",
    };
    format!("{label} ({head}){note}: {}", clauses.join("; "))
}

/// What a failing run prints: the failing tools, and only those. An advisory
/// failure says so in its header, so the reader can tell it from the failure
/// that stopped the run.
fn failure_report(failures: &[&StepResult]) -> String {
    let mut out = String::new();
    for step in failures {
        let note = if step.blocking { "" } else { " (advisory)" };
        let _ = writeln!(out, "== {} failed{note} ==", step.name);
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
    use crate::runner::EXIT_NOT_RUN;

    fn step(name: &str, phase: Phase, ms: u64, exit: i32, output: &str) -> StepResult {
        StepResult {
            name: name.to_owned(),
            phase,
            duration: Duration::from_millis(ms),
            exit,
            timed_out: false,
            output: output.to_owned(),
            blocking: true,
        }
    }

    fn advisory(name: &str, exit: i32, output: &str) -> StepResult {
        StepResult { blocking: false, ..step(name, Phase::Check, 10, exit, output) }
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
    fn a_failing_advisory_check_does_not_block() {
        assert!(
            !blocks(&advisory("pysentry", 1, "CVE-0000\n")),
            "advisory means reported, not fatal"
        );
        assert!(!blocks(&advisory("pysentry", EXIT_NOT_RUN, "")), "even when it never started");
    }

    #[test]
    fn a_failed_advisory_check_is_named_in_the_verdict_and_not_as_passed() {
        let steps =
            vec![step("ruff check", Phase::Check, 55, 0, ""), advisory("pysentry", 1, "x\n")];
        let line = success_line(Mode::Commit, 2, Duration::from_millis(60), &steps, false);
        assert!(
            line.ends_with("): ruff check passed; pysentry failed (advisory)"),
            "a clean verdict must not claim an advisory failure passed, got: {line}"
        );
    }

    #[test]
    fn a_passing_advisory_check_passed_like_any_other() {
        let steps = vec![advisory("pysentry", 0, "")];
        let line = success_line(Mode::Commit, 1, Duration::from_millis(10), &steps, false);
        assert!(line.ends_with("): pysentry passed"), "got: {line}");
    }

    #[test]
    fn the_full_run_says_check_ok_rather_than_pre_commit_ok() {
        let steps = vec![step("ruff check", Phase::Check, 55, 0, "")];
        let line = success_line(Mode::Full, 12, Duration::from_millis(60), &steps, false);
        assert_eq!(line, "check ok (12 files, 0.1s, slowest: ruff check 0.1s): ruff check passed");
    }

    #[test]
    fn an_advisory_failure_is_marked_as_one_in_the_report() {
        let blocking = step("ty check", Phase::Check, 1, 1, "nope\n");
        let advised = advisory("pysentry", 1, "CVE-0000\n");
        assert_eq!(
            failure_report(&[&blocking, &advised]),
            "== ty check failed ==\nnope\n== pysentry failed (advisory) ==\nCVE-0000\n",
            "the reader has to be able to tell which failure is the reason the run failed"
        );
    }

    #[test]
    fn the_verdict_is_one_line_naming_the_count_the_time_and_the_slowest_step() {
        let line =
            success_line(Mode::Commit, 3, Duration::from_millis(1834), &default_steps(), false);
        assert_eq!(
            line,
            "pre-commit ok (3 files, 1.8s, slowest: ty check 1.6s): \
             ruff fix, ruff format applied & staged; ruff check, ty check passed"
        );
        assert!(!line.contains('\n'), "a hook that prints a paragraph on success gets ignored");
    }

    #[test]
    fn an_auto_activated_venv_is_announced_in_the_verdict() {
        let line =
            success_line(Mode::Commit, 1, Duration::from_millis(100), &default_steps(), true);
        assert!(
            line.contains(") (auto-activated .venv): "),
            "the note sits between the counts and the summary, got: {line}"
        );
    }

    #[test]
    fn a_run_with_no_fixers_says_nothing_about_fixing() {
        let checks = vec![step("ruff check", Phase::Check, 55, 0, "")];
        let line = success_line(Mode::Commit, 1, Duration::from_millis(60), &checks, false);
        assert_eq!(
            line, "pre-commit ok (1 files, 0.1s, slowest: ruff check 0.1s): ruff check passed",
            "an empty fix list means no `applied & staged` clause at all"
        );
    }

    #[test]
    fn a_run_with_every_check_skipped_still_says_so() {
        let fixes = vec![step("ruff fix", Phase::Fix, 41, 0, "")];
        let line = success_line(Mode::Commit, 1, Duration::from_millis(50), &fixes, false);
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
