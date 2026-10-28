# Tune madoqua

Reference for what madoqua runs and what it reports. Everything here is
repo-wide policy unless it is in the personal overlay.

**Layers.** Built-in defaults, `[tool.madoqua]` in `pyproject.toml`, the
uncommitted overlay `hooks.local.toml` in the git directory (`<repo>/.git`, or
the clone's in a linked worktree), then `MADOQUA_SKIP`. Later layers win.

- `fix` and `check` replace that phase's list entirely; `extend_fix` and
  `extend_check` append to it, after the replacement. Scalar keys such as
  `log`: the later layer wins. A layer silent about a key leaves it alone.

**The registry.** An entry is a command line, or a table with these keys:

| Key | Default | Meaning |
|---|---|---|
| `cmd` | - | The command line. Required in table form. |
| `name` | leading non-flag words of `cmd` | What the verdict, the log and `MADOQUA_SKIP` call this step. |
| `on` | `["*.py", "*.pyi"]` | Globs for the staged files that wake this step. Replaces the default. |
| `pass_files` | true | Append the files `on` matched. Set it false for tools that scope themselves. |
| `timeout_s` | none | Kill the step after this many seconds. Killed is failed. |
| `max_output_lines` | none | Keep this many lines of a failure and truncate the rest. This is the knob for fitting a failure into an agent's context window. |
| `when` | `always` | Checks only. `commit`: only in the hook; `full`: only in `madoqua check`. |
| `full_cmd` | `cmd` | Checks only. What `madoqua check` runs instead, with no file list. |
| `blocking` | true | Checks only. false: a failure is reported but never fails the run. |

**`on` and the file list.** A step runs only if a staged file matches its
`on`, and gets those files after its arguments: `ruff check --quiet a.py`.
Globs as in .gitignore: no `/` matches the name at any depth, `*` stays in one
directory, `**` spans many. With no match anywhere there is no run at all - no
output, no log row, exit `0`. No shell: `|`, `;` and `&&` are refused.

**The whole repo.** `madoqua check` runs every check as if every file were
staged (tracked, or untracked and not ignored), and fixes, stages and logs
nothing. The siblings, set up for both runs:

```toml
[tool.madoqua]
extend_check = [
  { name = "biston", cmd = "biston scan --focus-args", full_cmd = "biston scan ." },
  { name = "zorilla", cmd = "zorilla check" },
  { name = "gerenuk", cmd = "gerenuk run -- -q", pass_files = false, when = "commit",
    on = ["*.py", "uv.lock", "pyproject.toml"], timeout_s = 120 },
  { name = "pytest", cmd = "pytest -q", pass_files = false, when = "full" },
  { name = "pysentry", cmd = "pysentry-rs .", pass_files = false, when = "full",
    blocking = false },
]
```

**Phases.** Fixers run in order, only their files are re-staged, and only a
fixer that could not start blocks. Checks run in parallel; non-zero blocks.

**Skipping.** `MADOQUA_SKIP="ty check"` drops checks by exact name for one
run, and never touches the fix phase. `log` defaults to `hook-timings.jsonl` in
the git directory; point it under `~` to pool every repo's runs, which
`madoqua stats --repo=<name>` filters. Rows are sorted by p95 descending.

next: run `madoqua stats`
