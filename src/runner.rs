//! The process seam: everything that spawns a tool and times it.
//!
//! Fix steps run one at a time and in declared order, because they rewrite the
//! same files and the second formatter has to see the first one's output.
//! Checks are read-only, so they all run at once and are joined afterwards.
//! Each step times itself inside its own thread, so a check's recorded
//! duration is how long the tool took and not how long it waited to be joined.

use std::any::Any;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::{Phase, Step};
use crate::venv::ChildEnv;

/// How often a timed step asks whether its child has finished.
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// The exit code recorded for a step madoqua killed for running too long.
///
/// It has to keep meaning exactly that, so a child that died of a signal is
/// recorded as `128 + signal` rather than sharing this code.
pub const EXIT_TIMED_OUT: i32 = -1;

/// The exit code recorded for a step that never ran at all.
///
/// An uninstalled tool, an empty command, or a child madoqua lost track of.
pub const EXIT_NOT_RUN: i32 = 127;

/// The shell's convention for "died of signal N".
#[cfg(unix)]
const SIGNAL_BASE: i32 = 128;

/// What one step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepResult {
    /// The step's configured name.
    pub name: String,
    /// Which half of the run it belonged to.
    pub phase: Phase,
    /// Wall-clock time for the tool itself.
    pub duration: Duration,
    /// The process's exit code, [`EXIT_TIMED_OUT`] when madoqua killed it,
    /// `128 + signal` when something else did, or [`EXIT_NOT_RUN`] when it
    /// never started.
    pub exit: i32,
    /// Whether the step ran out of time.
    pub timed_out: bool,
    /// Combined stdout and stderr, already truncated to `max_output_lines`.
    pub output: String,
}

impl StepResult {
    /// Whether this step should block the commit. Only meaningful for checks —
    /// a failing fixer is reported by the check that follows it, not by itself.
    pub fn failed(&self) -> bool {
        self.timed_out || self.exit != 0
    }

    /// Whether the tool never ran — it is not installed, or madoqua lost the
    /// child. Distinct from "it ran and complained", because a fixer that was
    /// never there must not be reported as having been applied.
    pub fn could_not_start(&self) -> bool {
        self.exit == EXIT_NOT_RUN
    }
}

/// Run the fix phase in order.
///
/// A fixer's non-zero exit is deliberately not fatal: `ruff check --fix` exits
/// non-zero for the violations it could not fix, and those are exactly what
/// the check phase is about to report.
pub fn run_fixes(root: &Path, steps: &[Step], files: &[String], env: &ChildEnv) -> Vec<StepResult> {
    steps.iter().map(|step| run_step(root, step, files, env, Phase::Fix)).collect()
}

/// Run every check at once and return the results in configuration order, so
/// the verdict does not reorder itself run to run.
pub fn run_checks(
    root: &Path,
    steps: &[Step],
    files: &[String],
    env: &ChildEnv,
) -> Vec<StepResult> {
    thread::scope(|scope| {
        #[expect(
            clippy::needless_collect,
            reason = "every check has to be spawned before any of them is joined, or \
                      the phase runs sequentially"
        )]
        let handles: Vec<_> = steps
            .iter()
            .map(|step| scope.spawn(move || run_step(root, step, files, env, Phase::Check)))
            .collect();

        handles
            .into_iter()
            .zip(steps)
            .map(|(handle, step)| {
                handle.join().unwrap_or_else(|payload| panicked(step, payload.as_ref()))
            })
            .collect()
    })
}

/// A step whose thread panicked is reported as a failure rather than taking
/// the whole run down — the developer still gets the other checks' output.
fn panicked(step: &Step, payload: &(dyn Any + Send)) -> StepResult {
    // The panic message is the whole diagnostic value of this path, and it
    // only exists inside the payload.
    let message = payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "no message".to_owned());

    StepResult {
        name: step.name.clone(),
        phase: Phase::Check,
        duration: Duration::ZERO,
        exit: EXIT_NOT_RUN,
        timed_out: false,
        output: format!("madoqua: the thread running `{}` panicked: {message}\n", step.name),
    }
}

/// Run one step and capture what it said.
fn run_step(
    root: &Path,
    step: &Step,
    files: &[String],
    env: &ChildEnv,
    phase: Phase,
) -> StepResult {
    let started = Instant::now();
    let outcome = spawn_and_wait(root, step, files, env);
    let duration = started.elapsed();

    let (exit, timed_out, output) = match outcome {
        Spawned::Finished(status, output) => (exit_code(status), false, output),
        Spawned::Killed(output) => (
            EXIT_TIMED_OUT,
            true,
            format!(
                "madoqua: `{}` was killed after {}s\n{output}",
                step.name,
                step.timeout.unwrap_or_default().as_secs()
            ),
        ),
        Spawned::NeverStarted(message) => (EXIT_NOT_RUN, false, message),
    };

    StepResult {
        name: step.name.clone(),
        phase,
        duration,
        exit,
        timed_out,
        output: truncate(&output, step.max_output_lines),
    }
}

/// The exit code to record for a finished child.
///
/// A child killed by a signal has no exit code of its own, so it gets the
/// shell's `128 + signal`: [`EXIT_TIMED_OUT`] has to keep meaning exactly
/// "madoqua killed this one", or the log cannot tell a timeout from a segfault.
fn exit_code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return SIGNAL_BASE + signal;
        }
    }
    EXIT_NOT_RUN
}

/// How a spawn ended. The three cases are genuinely different verdicts, and
/// collapsing them is how a missing tool ends up reported as a timeout: an
/// uninstalled tool is a normal thing for a hook to report, but it is not the
/// same as one madoqua had to kill.
enum Spawned {
    /// The tool ran to completion. Carries its status and its output.
    Finished(ExitStatus, String),
    /// madoqua killed it for outliving its timeout. Carries what it had said.
    Killed(String),
    /// It never ran, or madoqua lost track of the child. Carries the whole
    /// message to show the developer.
    NeverStarted(String),
}

fn spawn_and_wait(root: &Path, step: &Step, files: &[String], env: &ChildEnv) -> Spawned {
    let Some((program, args)) = step.argv.split_first() else {
        return Spawned::NeverStarted("madoqua: empty command\n".to_owned());
    };

    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(root)
        .env("PATH", &env.path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(virtual_env) = &env.virtual_env {
        command.env("VIRTUAL_ENV", virtual_env);
    }
    if step.pass_files {
        command.args(files);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return Spawned::NeverStarted(format!("madoqua: cannot run `{program}`: {err}\n"));
        }
    };

    // Drain both pipes on their own threads. A tool that writes more than a
    // pipe buffer would otherwise block forever waiting for us to read, and we
    // would be waiting for it to exit.
    let stdout = child.stdout.take().map(Reader::draining);
    let stderr = child.stderr.take().map(Reader::draining);

    let waited = match step.timeout {
        None => child.wait().map(Some),
        Some(limit) => wait_with_timeout(&mut child, limit),
    };

    // On a clean exit, wait for the readers so the last chunk is not lost. On
    // a timeout, take whatever they have: the killed process may have left a
    // descendant holding the write end of the pipe, and waiting for that would
    // undo the timeout we just enforced.
    let collected = matches!(waited, Ok(Some(_)));
    let mut output = stdout.map_or_else(String::new, |reader| reader.finish(collected));
    output.push_str(&stderr.map_or_else(String::new, |reader| reader.finish(collected)));

    match waited {
        Ok(Some(status)) => Spawned::Finished(status, output),
        Ok(None) => Spawned::Killed(output),
        Err(err) => Spawned::NeverStarted(format!(
            "madoqua: lost track of `{}`: {err}\n{output}",
            step.name
        )),
    }
}

/// A pipe being drained on its own thread, whose buffer can be read before the
/// thread has finished.
struct Reader {
    handle: thread::JoinHandle<()>,
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl Reader {
    fn draining<R: Read + Send + 'static>(mut source: R) -> Self {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&buffer);

        let handle = thread::spawn(move || {
            let mut chunk = [0_u8; 8192];
            loop {
                let read = match source.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => read,
                };
                let Ok(mut sink) = sink.lock() else { break };
                sink.extend_from_slice(&chunk[..read]);
            }
        });

        Self { handle, buffer }
    }

    /// The captured bytes, waiting for the thread first when `join` is set.
    fn finish(self, join: bool) -> String {
        if join {
            let _ = self.handle.join();
        }
        self.buffer
            .lock()
            .map_or_else(|_| String::new(), |bytes| String::from_utf8_lossy(&bytes).into_owned())
    }
}

/// Wait for `child`, killing it once `limit` has passed.
///
/// `Ok(None)` means madoqua killed it. `Err` means madoqua could not tell what
/// the child was doing, which is a different thing and must not be reported as
/// a timeout — `was killed after 0s` about a step nobody killed is worse than
/// no message.
fn wait_with_timeout(child: &mut Child, limit: Duration) -> std::io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Keep the first `max` lines and say how many were dropped.
///
/// The head is what survives: a linter's first complaints are the ones worth
/// reading, and this cap exists so a thousand-line traceback does not fill an
/// agent's context window.
fn truncate(output: &str, max: Option<usize>) -> String {
    let Some(max) = max else { return output.to_owned() };

    let total = output.lines().count();
    if total <= max {
        return output.to_owned();
    }

    let head: String = output.lines().take(max).flat_map(|line| [line, "\n"]).collect();
    format!("{head}... ({} lines truncated)\n", total - max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> ChildEnv {
        ChildEnv { path: std::env::var_os("PATH").unwrap_or_default(), virtual_env: None }
    }

    fn step(name: &str, argv: &[&str]) -> Step {
        Step {
            name: name.to_owned(),
            argv: argv.iter().map(|a| (*a).to_owned()).collect(),
            pass_files: true,
            timeout: None,
            max_output_lines: None,
        }
    }

    #[test]
    fn output_under_the_cap_is_untouched() {
        assert_eq!(truncate("a\nb\n", Some(5)), "a\nb\n", "nothing to say, nothing to add");
        assert_eq!(truncate("a\nb\n", None), "a\nb\n", "no cap means no marker, ever");
    }

    #[test]
    fn output_over_the_cap_keeps_the_head_and_counts_the_rest() {
        assert_eq!(
            truncate("1\n2\n3\n4\n5\n", Some(2)),
            "1\n2\n... (3 lines truncated)\n",
            "the first complaints are the ones worth reading"
        );
    }

    #[test]
    fn a_step_reports_its_exit_code_and_combined_output() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_step(
            dir.path(),
            &step("noisy", &["sh", "-c", "echo out; echo err >&2; exit 3"]),
            &[],
            &env(),
            Phase::Check,
        );

        assert_eq!(result.exit, 3, "the tool's exit code is reported as it is");
        assert!(result.failed(), "a non-zero check blocks the commit");
        assert!(result.output.contains("out"), "stdout is captured, got: {:?}", result.output);
        assert!(result.output.contains("err"), "stderr is captured too, got: {:?}", result.output);
    }

    #[test]
    fn pass_files_false_hands_the_tool_no_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let mut repo_wide = step("count", &["sh", "-c", r#"echo "argc=$#""#, "sh"]);
        repo_wide.pass_files = false;

        let result = run_step(dir.path(), &repo_wide, &["a.py".to_owned()], &env(), Phase::Check);
        assert_eq!(result.output.trim(), "argc=0", "a repo-wide tool must not see the file list");
    }

    #[test]
    fn pass_files_true_appends_the_file_list() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_step(
            dir.path(),
            &step("echo", &["sh", "-c", r#"echo "$@""#, "sh"]),
            &["a.py".to_owned(), "b b.py".to_owned()],
            &env(),
            Phase::Check,
        );
        assert_eq!(
            result.output.trim(),
            "a.py b b.py",
            "the staged files are appended, spaces and all"
        );
    }

    #[test]
    fn a_missing_tool_is_a_finding_rather_than_a_crash() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_step(
            dir.path(),
            &step("ghost", &["madoqua-no-such-tool"]),
            &[],
            &env(),
            Phase::Check,
        );

        assert_eq!(result.exit, EXIT_NOT_RUN, "an uninstalled tool fails its own check");
        assert!(
            result.output.contains("madoqua-no-such-tool"),
            "the output must name the tool, got: {:?}",
            result.output
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_step_killed_by_a_signal_is_not_confused_with_one_madoqua_killed() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_step(
            dir.path(),
            &step("suicidal", &["sh", "-c", "kill -9 $$"]),
            &[],
            &env(),
            Phase::Check,
        );

        assert_eq!(result.exit, SIGNAL_BASE + 9, "a signal death is `128 + signal`, as in a shell");
        assert!(!result.timed_out, "nothing timed it out");
        assert!(result.failed(), "and it still blocks the commit");
    }

    #[test]
    fn a_step_that_could_not_start_says_so_and_a_step_that_merely_failed_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let missing = run_step(
            dir.path(),
            &step("ghost", &["madoqua-no-such-tool"]),
            &[],
            &env(),
            Phase::Check,
        );
        let complained = run_step(
            dir.path(),
            &step("grumpy", &["sh", "-c", "exit 1"]),
            &[],
            &env(),
            Phase::Check,
        );

        assert!(missing.could_not_start(), "an uninstalled tool never ran");
        assert!(!complained.could_not_start(), "a tool that exited 1 did run and had an opinion");
    }

    #[test]
    fn a_step_that_overruns_is_killed_and_marked() {
        let dir = tempfile::tempdir().unwrap();
        let mut slow = step("slow", &["sleep", "30"]);
        slow.timeout = Some(Duration::from_millis(50));

        let result = run_step(dir.path(), &slow, &[], &env(), Phase::Check);
        assert!(result.timed_out, "the step must be marked as timed out");
        assert_eq!(result.exit, EXIT_TIMED_OUT, "a kill is logged as -1");
        assert!(result.failed(), "a timeout blocks the commit");
        assert!(result.duration < Duration::from_secs(5), "it must not have waited for sleep(30)");
    }

    #[test]
    fn checks_run_in_parallel_but_are_reported_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let steps = [step("slow", &["sh", "-c", "sleep 0.3"]), step("fast", &["sh", "-c", "true"])];

        let started = Instant::now();
        let results = run_checks(dir.path(), &steps, &[], &env());
        let elapsed = started.elapsed();

        assert_eq!(
            results.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["slow", "fast"],
            "results follow configuration order, not finishing order"
        );
        assert!(
            elapsed < Duration::from_millis(900),
            "three sequential 0.3s sleeps would be slower than this; got {elapsed:?}"
        );
    }

    #[test]
    fn fixes_run_in_order_and_a_failing_fixer_does_not_stop_the_next() {
        let dir = tempfile::tempdir().unwrap();
        let steps = [
            step("first", &["sh", "-c", "echo one >> log.txt; exit 1"]),
            step("second", &["sh", "-c", "echo two >> log.txt"]),
        ];

        let results = run_fixes(dir.path(), &steps, &[], &env());
        assert_eq!(results[0].exit, 1, "the first fixer failed");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("log.txt")).unwrap(),
            "one\ntwo\n",
            "the second fixer still ran, and ran after the first"
        );
    }
}
