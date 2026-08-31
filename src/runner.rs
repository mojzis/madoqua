//! The process seam: everything that spawns a tool and times it.
//!
//! Fix steps run one at a time and in declared order, because they rewrite the
//! same files and the second formatter has to see the first one's output.
//! Checks are read-only, so they all run at once and are joined afterwards.
//! Each step times itself inside its own thread, so a check's recorded
//! duration is how long the tool took and not how long it waited to be joined.

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

/// The exit code recorded for a step we killed, and for one we could not start.
const EXIT_TIMED_OUT: i32 = -1;
const EXIT_NOT_RUN: i32 = 127;

/// What one step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepResult {
    /// The step's configured name.
    pub name: String,
    /// Which half of the run it belonged to.
    pub phase: Phase,
    /// Wall-clock time for the tool itself.
    pub duration: Duration,
    /// The process's exit code, or [`EXIT_TIMED_OUT`] when we killed it.
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
            .map(|(handle, step)| handle.join().unwrap_or_else(|_| panicked(step)))
            .collect()
    })
}

/// A step whose thread panicked is reported as a failure rather than taking
/// the whole run down — the developer still gets the other checks' output.
fn panicked(step: &Step) -> StepResult {
    StepResult {
        name: step.name.clone(),
        phase: Phase::Check,
        duration: Duration::ZERO,
        exit: EXIT_NOT_RUN,
        timed_out: false,
        output: format!("madoqua: the thread running `{}` panicked\n", step.name),
    }
}

/// Run one step and capture what it said.
pub fn run_step(
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
        Ok((Some(status), output)) => (status.code().unwrap_or(EXIT_TIMED_OUT), false, output),
        Ok((None, output)) => (
            EXIT_TIMED_OUT,
            true,
            format!(
                "madoqua: `{}` was killed after {}s\n{output}",
                step.name,
                step.timeout.unwrap_or_default().as_secs()
            ),
        ),
        Err(message) => (EXIT_NOT_RUN, false, message),
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

/// `Ok((None, _))` means the child was killed for running too long. `Err` means
/// it never started, which is a normal thing for a hook to report: an
/// uninstalled tool is a finding, not a crash.
fn spawn_and_wait(
    root: &Path,
    step: &Step,
    files: &[String],
    env: &ChildEnv,
) -> Result<(Option<ExitStatus>, String), String> {
    let (program, args) =
        step.argv.split_first().ok_or_else(|| "madoqua: empty command\n".to_owned())?;

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

    let mut child =
        command.spawn().map_err(|err| format!("madoqua: cannot run `{program}`: {err}\n"))?;

    // Drain both pipes on their own threads. A tool that writes more than a
    // pipe buffer would otherwise block forever waiting for us to read, and we
    // would be waiting for it to exit.
    let stdout = child.stdout.take().map(Reader::draining);
    let stderr = child.stderr.take().map(Reader::draining);

    let status = match step.timeout {
        None => child.wait().ok(),
        Some(limit) => wait_with_timeout(&mut child, limit),
    };

    // On a clean exit, wait for the readers so the last chunk is not lost. On
    // a timeout, take whatever they have: the killed process may have left a
    // descendant holding the write end of the pipe, and waiting for that would
    // undo the timeout we just enforced.
    let collected = status.is_some();
    let mut output = stdout.map_or_else(String::new, |reader| reader.finish(collected));
    output.push_str(&stderr.map_or_else(String::new, |reader| reader.finish(collected)));
    Ok((status, output))
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

/// Wait for `child`, killing it once `limit` has passed. `None` means killed.
fn wait_with_timeout(child: &mut Child, limit: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Err(_) => return None,
            Ok(None) => {}
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Keep the first `max` lines and say how many were dropped.
///
/// The head is what survives: a linter's first complaints are the ones worth
/// reading, and this cap exists so a thousand-line traceback does not fill an
/// agent's context window.
pub fn truncate(output: &str, max: Option<usize>) -> String {
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
