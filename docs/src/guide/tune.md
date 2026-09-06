# Tune madoqua

Reference for what madoqua runs and what it reports. Everything here is
repo-wide policy unless it is in the personal overlay.

**Layers.** Built-in defaults, then `[tool.madoqua]` in the repo's
`pyproject.toml`, then `<repo>/.git/hooks.local.toml` - the personal overlay,
which is not committed - then `MADOQUA_SKIP`. Later layers win.

- `fix` and `check` replace that phase's list entirely; `extend_fix` and
  `extend_check` append to it, after the replacement. Scalar keys such as
  `log`: the later layer wins. A layer silent about a key leaves it alone.

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
| `pass_files` | true | Append the staged file list. Set it false for tools that scope themselves. |
| `timeout_s` | none | Kill the step after this many seconds. A killed step fails the commit. |
| `max_output_lines` | none | Keep this many lines of a failure and truncate the rest. This is the knob for fitting a failure into an agent's context window. |

**The file list** is the staged `.py` and `.pyi` files, relative to the
repository root, appended after the command's own arguments, so the string
form `ruff check --quiet` runs as `ruff check --quiet a.py pkg/b.py`. With no
staged Python file there is no run at all - no step, no log row, exit `0`.
Commands are split on whitespace with quotes honoured and run without a
shell, so a pipe, a semicolon or `&&` is refused; put the pipeline in a script.

**Siblings**, in the same table form; gerenuk computes its own diff:

```toml
[tool.madoqua]
extend_check = [
  { name = "biston", cmd = "biston scan --focus-args" },
  { name = "zorilla", cmd = "zorilla check" },
  { name = "gerenuk", cmd = "gerenuk run -- -q", pass_files = false, timeout_s = 120 },
]
```

**Phases.** Fix steps run sequentially and are re-staged; a non-zero exit is
not fatal, the check that follows reports it, but a fixer that could not start
is. Check steps run in parallel, are read-only, and a non-zero exit blocks.

**Skipping.** `MADOQUA_SKIP="ty check"` drops checks by exact name for one
run, and never touches the fix phase. `log` defaults to
`.git/hook-timings.jsonl`; point it under `~` to pool every repo's runs, which
`madoqua stats --repo=<name>` filters. Rows are sorted by p95 descending.

next: run `madoqua stats`
