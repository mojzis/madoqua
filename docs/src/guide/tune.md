# Tune madoqua

Reference for what madoqua runs and what it reports. Everything here is
repo-wide policy unless it is in the personal overlay.

**Layers.** Built-in defaults, then `[tool.madoqua]` in the repo's
`pyproject.toml`, then `<repo>/.git/hooks.local.toml` - the personal overlay,
which is not committed - then `MADOQUA_SKIP`. Later layers win.

- `fix` and `check` replace that phase's list entirely.
- `extend_fix` and `extend_check` append to it. A layer may do both; the
  replacement happens first and the extension appends to the result.
- Scalar keys such as `log`: the later layer wins.
- A layer that says nothing about a key leaves it alone.

**The registry.** An entry is a command line, or a table:

```toml
[tool.madoqua]
fix = ["ruff check --fix --quiet", "ruff format --quiet"]
check = [
  "ruff check --quiet",
  { name = "ty", cmd = "ty check", timeout_s = 120, max_output_lines = 200 },
]
```

| Key | Default | Meaning |
|---|---|---|
| `cmd` | - | The command line. Required in table form. |
| `name` | leading non-flag words of `cmd` | What the verdict, the log and `MADOQUA_SKIP` call this step. |
| `pass_files` | true | Append the staged file list. Set it false for repo-wide tools. |
| `timeout_s` | none | Kill the step after this many seconds. A killed step fails the commit. |
| `max_output_lines` | none | Keep this many lines of a failure and truncate the rest. This is the knob for fitting a failure into an agent's context window. |

Commands are split on whitespace with quotes honoured, then run directly.
There is no shell, so a pipe, a semicolon or `&&` is refused rather than
passed on as a literal argument. Put the pipeline in a script.

**Phases.** Fix steps run sequentially and their output is re-staged; a
non-zero exit from one is not fatal, because the check that follows reports
it. A fixer that could not be started is fatal. Check steps run in parallel,
are read-only, and a non-zero exit blocks the commit.

**Skipping.** `MADOQUA_SKIP="ty check"` drops checks by exact name for one
run, and never touches the fix phase.

**Timings.** `log` defaults to `.git/hook-timings.jsonl`. Point it under `~`
to pool every repo's runs; records written outside the repository carry the
repo name, which `madoqua stats --repo=<name>` filters on. Rows are sorted by
p95 descending, so the first is the one worth fixing. Writing the log is
best-effort and never blocks a commit.

next: run `madoqua stats`
