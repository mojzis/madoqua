# Configuration

madoqua works with no configuration at all. The built-in registry is the one
the bash hook it replaces used:

| Phase | Commands |
|---|---|
| fix | `ruff check --fix --quiet --`, `ruff format --quiet --` |
| check | `ruff check --quiet --`, `ty check --` |

## `[tool.madoqua]` in `pyproject.toml`

```toml
[tool.madoqua]
fix = [
  "ruff check --fix --quiet",
  "ruff format --quiet",
]
check = [
  "ruff check --quiet",
  { name = "ty", cmd = "ty check", pass_files = true, timeout_s = 120, max_output_lines = 200 },
]
# log = ".git/hook-timings.jsonl"
```

An entry is either a command line or a table:

| Key | Default | Meaning |
|---|---|---|
| `cmd` | — | The command line. Required in table form. |
| `name` | the leading non-flag words of `cmd` | What the verdict, the log and `MADOQUA_SKIP` call this step. |
| `pass_files` | `true` | Append the staged file list to the command. `false` for repo-wide tools. |
| `timeout_s` | none | Kill the step after this long. A killed step fails the commit and is logged with `"exit": -1, "timed_out": true`. |
| `max_output_lines` | none | Keep this many lines of output and replace the rest with `... (N lines truncated)`. This is the knob for keeping a failure inside an agent's context window. |

Command lines are split on whitespace with single and double quotes honoured.
There is no shell: `|`, `;` and `&&` are rejected with an error rather than
being passed along as literal arguments. Put a pipeline in a script and call
the script.

## The personal overlay

`<repo_root>/.git/hooks.local.toml` is the same schema at the top level, and is
not committed — it is where your own preferences go without imposing them on
everyone else:

```toml
extend_check = ["bandit -q -r src"]
log = "~/.local/state/madoqua/timings.jsonl"
```

Four lines of semantics, also in `madoqua run --help`:

- `check` / `fix` in the overlay **replace** that list entirely.
- `extend_check` / `extend_fix` **append** to the repo's list.
- Scalar keys (`log`, …) — the overlay wins.
- A layer that says nothing about a key leaves it alone.

## Skipping a check once

```sh
MADOQUA_SKIP="ty check" git commit -m "wip"
```

Comma-separated step *names*, matched exactly — the built-in registry's names
are `ruff check` and `ty check`, and a table entry's `name` overrides that, so
`MADOQUA_SKIP="ty"` against the defaults silently skips nothing. It filters the
check phase only: skipping the formatter would leave the working tree in a
state the next run reformats anyway.

`fix` is not skippable, and a fixer madoqua cannot find blocks the commit
rather than being reported as applied — no check reports a missing formatter.

## Where timings go

`log` defaults to `.git/hook-timings.jsonl`, which is per-repo and disappears
with the clone. Point it at `~/…` to collect every repo's runs in one place —
`~` expands, parent directories are created, and records written outside the
repository carry a `repo` field so [`stats --repo`](commands/stats.md) can tell
them apart.

Writing the log is best-effort. A failure warns on stderr and the commit
proceeds.

## Limitation: partially staged files

madoqua runs the fixers on the whole file and then `git add`s it, so a file you
staged in part is committed in full. This is deliberate — the workflows madoqua
is built for (agents, and `git add <file>`) stage whole files. If you rely on
`git add -p`, this hook is not for you.
