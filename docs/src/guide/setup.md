# Setup madoqua in this repository

madoqua is not wired into this repository yet. Run everything below at the
repository root, in this order.

**1. Install.** Add it as a dev dependency: `uv add --dev madoqua`.
Alternatives: `uv tool install madoqua`, or `pipx install madoqua`. Prefer the
dev dependency; a tool install pins no version per project.

**2. Create the virtualenv.** madoqua runs tools only out of `<repo>/.venv`,
so the hook and CI lint with the same versions:

```bash
uv venv && uv sync
```

With the built-in registry that means `ruff` and `ty` have to be installed
there. A missing venv blocks every commit, whether or not it touches Python.

**3. Baseline before you gate.** Stage your files, run `madoqua run`, and
resolve everything it reports before installing the hook; run
`madoqua guide triage` for how. A hook installed into a repo that cannot pass
it gets bypassed with `--no-verify`, not fixed.

**4. Install the hook.**

```bash
madoqua install
```

That writes `hooks/pre-commit` and sets `core.hooksPath` to `hooks`. Commit the
shim and every clone gets the same hook - each clone still runs
`madoqua install` once, because `core.hooksPath` is local config.

**5. Configure, only if the defaults are wrong.** The built-in registry fixes
with `ruff check --fix` and `ruff format`, then checks with `ruff check` and
`ty check`. Add to it in `pyproject.toml`:

```toml
[tool.madoqua]
extend_check = ["bandit -q -r src"]
```

`madoqua guide tune` is the full key reference.

**6. Exit codes.** `0` clean, `1` the commit is blocked, `2` madoqua could not
run. Keep them distinct in anything that wraps madoqua: `1` is your code's
problem, `2` is madoqua's.

next: run `madoqua run`
