# Setup

## Install

madoqua is a Rust binary shipped as a Python wheel, so any Python installer
works:

```sh
uv tool install madoqua
# or
pipx install madoqua
# or, per project
uv add --dev madoqua
```

## Wire it into a repository

```sh
cd your-python-project
madoqua install
```

That writes `hooks/pre-commit` and sets `core.hooksPath` to `hooks`. Commit the
shim and everyone who clones the repo gets the same hook — each clone still
runs `madoqua install` once, since `core.hooksPath` is local config.

## Prerequisites

- A virtualenv at `<repo_root>/.venv` with the tools the checks call installed.
  madoqua refuses to run tools from outside it: a system `ruff` would lint with
  a different version than CI, and the failure would look like your code's
  fault.
- With the defaults, that means `ruff` and `ty`:

  ```sh
  uv venv && uv sync
  ```

## Verify

```sh
madoqua run     # with something staged; prints one line if all is well
madoqua stats   # after a few commits
```

## Using it from Claude Code

Paste this into your project's `CLAUDE.md`:

<!-- BEGIN SHARED:claude-snippet -->
```markdown
## madoqua

`madoqua` is this repo's pre-commit hook. It runs the fixers on the staged
Python files, re-stages them, runs the checks in parallel, and prints one line.
It runs itself on `git commit` — invoke it directly to see what a commit would
say before making one.

```sh
madoqua guide                     # what to do here, right now
madoqua run                       # what `git commit` will do; one line if clean
MADOQUA_SKIP="ty check" madoqua run   # drop a check by name, this run only
madoqua stats                     # which check is costing the most time
```

Start with `madoqua guide`: it prints setup instructions in a repo that is not
wired up yet, and triage instructions in one that is. `madoqua guide tune` is
the configuration reference.

Exit codes: `0` clean, `1` a check failed (the commit would be blocked), `2`
madoqua could not run. Failing checks print only the failing tool's output, on
stderr.

Configuration is `[tool.madoqua]` in `pyproject.toml`. Set `max_output_lines`
on a noisy check to keep its failures inside a context window.
```
<!-- END SHARED:claude-snippet -->
