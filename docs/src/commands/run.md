# `run`

The hook. `madoqua` with no subcommand does the same thing, so the installed
shim can be a bare `exec madoqua`.

```sh
madoqua run
```

## What it does, in order

1. **Virtualenv guard.** Every child process gets `<repo_root>/.venv/bin` at
   the front of its `PATH` and `VIRTUAL_ENV` pointing at that venv — the
   observable half of `source .venv/bin/activate`. That happens on every run,
   not only when the venv is missing from `PATH`: where `python` resolves says
   nothing about where `pytest` does, and a directory in front of the venv that
   holds one but not the other would otherwise decide what a check, or a
   process a check spawns by name, actually runs. The rest of `PATH` is kept,
   in order, behind it. `python` then has to resolve to
   `<repo_root>/.venv/bin/python`: if there is no venv, or something still
   shadows it, the commit is blocked with instructions. The guard runs before
   anything else, including the file list: a repo without a venv is
   misconfigured whether or not this commit touches Python.
2. **Staged files.** `git diff --cached --name-only -z --diff-filter=ACMR -- '*.py' '*.pyi'`.
   Nothing staged in Python means exit `0` with no output and no log entry.
3. **Fix phase.** Sequentially, in configuration order. A fixer's non-zero exit
   does not stop the run: `ruff check --fix` exits non-zero for what it could
   *not* fix, and the check phase is about to report exactly that. A fixer
   madoqua cannot *find* is different — nothing downstream reports a missing
   `ruff format` — so that blocks the commit.
4. **`git add`** the staged file list, so what the fixers wrote is what gets
   committed.
5. **Check phase.** Every check at once, on its own thread, with stdout and
   stderr captured per check.
6. **One line, or the failures.**

## Output

A clean run prints one line to **stdout**:

```
pre-commit ok (3 py files, 1.8s, slowest: ty check 1.6s): ruff fix, ruff format applied & staged; ruff check, ty check passed
```

with ` (auto-activated .venv)` after the parenthesis when `.venv/bin` was not
already the front of your `PATH` and the guard put it there — which means your
shell is not set up the way you think it is.

A failing run prints nothing to stdout, and to **stderr** only the tools that
failed:

```
== ty check failed ==
src/a.py:12: error: Argument 1 has incompatible type "int"
```

Everything informational goes to stderr, so `madoqua run` stays pipeable.

## Configuration

See [Configuration](../configuration.md), and `madoqua run --help` for the
overlay semantics in short form.

## Exit codes

| Code | When |
|---|---|
| `0` | Every check passed, or there was nothing staged to check |
| `1` | A check failed or timed out, a fixer could not be run, or the virtualenv guard refused |
| `2` | madoqua could not run: not a repository, unreadable config, a command it cannot honour |
