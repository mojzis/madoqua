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
# log = "build/timings.jsonl"   # default: hook-timings.jsonl in the git dir
```

An entry is either a command line or a table:

| Key | Default | Meaning |
|---|---|---|
| `cmd` | — | The command line. Required in table form. |
| `name` | the leading non-flag words of `cmd` | What the verdict, the log and `MADOQUA_SKIP` call this step. |
| `on` | `["*.py", "*.pyi"]` | Globs for the staged files that wake this step. A list replaces the default rather than adding to it. |
| `pass_files` | `true` | Append the staged files `on` matched to the command. `false` for repo-wide tools. |
| `timeout_s` | none | Kill the step after this long. A killed step fails the run, unless it is `blocking = false`, and is logged with `"exit": -1, "timed_out": true`. |
| `max_output_lines` | none | Keep this many lines of output and replace the rest with `... (N lines truncated)`. This is the knob for keeping a failure inside an agent's context window. |
| `when` | `"always"` | Checks only. `"commit"` runs it only in the hook, `"full"` only in [`madoqua check`](commands/check.md). |
| `full_cmd` | `cmd` | Checks only. What `madoqua check` runs instead of `cmd`, handed no file list. |
| `blocking` | `true` | Checks only. `false` reports a failure — on stderr, and as `failed (advisory)` in the verdict — but never fails the run. |

`when`, `full_cmd` and `blocking` on a fixer are refused with an error: fixers
run only in the hook, and a fixer's failure never blocks anyway. So is
`full_cmd` on a step with `when = "commit"` or `"full"`: one of its two
commands would never run.

## Which files wake a step: `on`

A step runs only when at least one staged file matches its `on` list, and with
`pass_files` it is handed exactly those files. A step that matches nothing is
not run, not logged and not named in the verdict; a commit in which no step
matches anything prints nothing and writes no log row.

The globs follow `.gitignore`, minus everything but the wildcards:

| Glob | Matches |
|---|---|
| `*.py` | No `/`: a file name at any depth — `a.py`, `pkg/sub/a.py`. |
| `uv.lock` | Likewise any `uv.lock`; write `/uv.lock` for the root one only. |
| `tests/*.py` | Has a `/`: anchored at the root, and `*` stays in one directory. |
| `tests/**` | `**` as a whole segment spans any depth: everything under `tests`. |

`?` is one character. Nothing else is special. A trailing `/` and an empty
list are refused rather than read as "never".

This is how a test runner that works out its own diff is woken by more than
Python, without being handed files it would not know what to do with:

```toml
[tool.madoqua]
extend_check = [
  { name = "gerenuk", cmd = "gerenuk run -- -q", pass_files = false,
    on = ["*.py", "uv.lock", "pyproject.toml", "tests/**"], timeout_s = 120 },
]
```

In [`madoqua check`](commands/check.md) the same globs are matched against
every file in the repository instead of the staged ones.

Only the files a *fixer* ran on are re-staged. A file that only a check looked
at keeps whatever you staged of it.

Command lines are split on whitespace with single and double quotes honoured.
There is no shell: `|`, `;` and `&&` are rejected with an error rather than
being passed along as literal arguments. Put a pipeline in a script and call
the script.

## The hook and the whole-repo run

`madoqua run` (the hook) and `madoqua check` (the whole repository) run from
the same list. A check runs the same command in both unless it says otherwise:

```toml
[tool.madoqua]
extend_check = [
  # The staged files at commit, the whole tree in `madoqua check`.
  { name = "biston", cmd = "biston scan --focus-args", full_cmd = "biston scan ." },
  # Too slow for every commit.
  { name = "pytest", cmd = "pytest -q", pass_files = false, when = "full" },
  # Worth reading, not worth failing over.
  { name = "pysentry", cmd = "pysentry-rs .", pass_files = false, when = "full",
    blocking = false },
]
```

## The personal overlay

`hooks.local.toml` in the repository's git directory is the same schema at the
top level, and is not committed — it is where your own preferences go without
imposing them on everyone else:

```toml
extend_check = ["bandit -q -r src"]
log = "~/.local/state/madoqua/timings.jsonl"
```

Four lines of semantics, also in `madoqua run --help`:

- `check` / `fix` in the overlay **replace** that list entirely.
- `extend_check` / `extend_fix` **append** to the repo's list.
- Scalar keys (`log`, …) — the overlay wins.
- A layer that says nothing about a key leaves it alone.

## Worktrees share one git directory

The overlay and the default timing log live in the repository's git directory:
`<repo_root>/.git` in an ordinary clone, and the clone's `.git` in a linked
worktree, where `<worktree>/.git` is a file pointing at it. madoqua asks git
(`git rev-parse --git-common-dir`) instead of joining `.git/` onto the working
tree, so one overlay configures every worktree of a repository and one log
holds every worktree's runs.

That is deliberate, and it is what makes an overlay useful in the
worktree-per-task workflow: a per-checkout overlay would have to be written
again for every task branch and would vanish with `git worktree remove`.

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

`log` defaults to `hook-timings.jsonl` in the git directory, which is per-repo
and disappears with the clone. A configured path is relative to the working
tree instead — that is where `build/timings.jsonl` belongs. Point it at `~/…`
to collect every repo's runs in one place — `~` expands, parent directories are
created, and records written outside the repository carry a `repo` field so
[`stats --repo`](commands/stats.md) can tell them apart.

Writing the log is best-effort. A failure warns on stderr and the commit
proceeds.

## Limitation: partially staged files

madoqua runs the fixers on the whole file and then `git add`s it, so a file you
staged in part is committed in full. This is deliberate — the workflows madoqua
is built for (agents, and `git add <file>`) stage whole files. If you rely on
`git add -p`, this hook is not for you.
