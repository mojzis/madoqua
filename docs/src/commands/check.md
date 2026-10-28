# `check`

The whole repository, checked the way the hook checks a commit.

```sh
madoqua check
```

This is the command for "is the repo clean" — before a push, in CI, or in place
of a hand-written task that runs the same tools. It runs from the same
`[tool.madoqua]` list as the hook, so the two cannot drift apart.

## What it does, in order

1. **Virtualenv guard**, exactly as [`run`](run.md) does it.
2. **Every file a commit could contain.** Tracked files, plus untracked ones
   git does not ignore, minus files deleted from the working tree, sorted by
   path. Each check keeps the files its `on` globs match, as it does with the
   staged files in the hook.
3. **Check phase.** Every check whose `when` includes the full run, at once,
   in parallel. A check with a `full_cmd` runs that instead of its `cmd`, with
   no file list.
4. **One line, or the failures.**

There is no fix phase and nothing is staged: this run reports, it never
rewrites. Nothing is written to the timing log either — the log is for the
hook's latency, and a full test run in it would swamp the percentiles.

## One list, two runs

```toml
[tool.madoqua]
check = [
  "ruff check --quiet",
  "ty check",
  { name = "biston", cmd = "biston scan --focus-args", full_cmd = "biston scan ." },
  { name = "gerenuk", cmd = "gerenuk run -- -q", pass_files = false, when = "commit",
    on = ["*.py", "uv.lock", "pyproject.toml"] },
  { name = "pytest", cmd = "pytest -q", pass_files = false, when = "full" },
  { name = "vulture", cmd = "vulture src", pass_files = false, when = "full" },
  { name = "pysentry", cmd = "pysentry-rs .", pass_files = false, when = "full",
    blocking = false },
]
```

- `ruff check` and `ty check` run in both: at commit on the staged Python
  files, here on every Python file in the repository.
- `biston` scans what is staged at commit and the whole tree here.
- `gerenuk` works out its own diff, so it only makes sense at commit; `pytest`
  runs the whole suite instead, and only here.
- `pysentry` is advisory: its findings are printed, and the run still passes.

See [Configuration](../configuration.md) for every key.

## Output

A clean run prints one line to **stdout**:

```
check ok (148 files, 6.2s, slowest: pytest 5.9s): ruff check, ty check, biston, pytest, vulture passed; pysentry failed (advisory)
```

with ` (auto-activated .venv)` after the parenthesis when the guard put
`.venv/bin` on `PATH`. A run in which no check matched a file still says so:
`check ok (0 files, 0.0s): no checks run`.

A failing run prints nothing to stdout, and to **stderr** the checks that
failed — every one of them, since all checks run to the end. A failed advisory
check is marked as one:

```
== ty check failed ==
src/a.py:12: error: Argument 1 has incompatible type "int"
== pysentry failed (advisory) ==
CVE-2026-0001 in somelib 1.2.3
```

A failed advisory check prints to stderr on a clean run too.

## Exit codes

| Code | When |
|---|---|
| `0` | Every blocking check passed, or no check matched a file |
| `1` | A blocking check failed, timed out or could not be run, or the virtualenv guard refused |
| `2` | madoqua could not run: not a repository, unreadable config, a command it cannot honour |

## Large repositories

Without a `full_cmd`, a check that takes files is handed every matching file
in the repository on its command line. On a repository big enough to exceed
the system's argument limit, the tool cannot be started and the check fails
with exit `127`, naming it. Give that check a `full_cmd` that lets the tool
find the files itself — `ruff check .` instead of a file list.
