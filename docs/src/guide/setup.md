# Setup madoqua in this repository

madoqua is not wired into this repository yet. Run everything below at the
repository root, in this order.

**1. Install.** madoqua runs tools only out of `<repo>/.venv`, so the hook and
CI lint with the same versions, and the built-in registry runs `ruff` and `ty`.
Add all three as dev dependencies:

```bash
uv add --dev madoqua ruff ty
```

Or `uv tool install madoqua` for madoqua alone; prefer the dev dependency,
since a tool install pins no version per project.

**2. Create the virtualenv**, if `uv add` did not already: `uv venv && uv sync`.
A missing venv blocks every commit, whether or not it touches Python.

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

`madoqua guide tune` is the full key reference, including how a sibling tool
such as biston, zorilla or gerenuk is wired in as one more check.

**6. Verify that it runs.** madoqua acts only on staged `.py` and `.pyi`
files. With none staged it prints nothing, exits `0` and writes no log row, so
silence is not a verdict. Make a one-line edit to any `.py` file, `git add` it,
and run `madoqua run`: a run that happened ends with `pre-commit ok` on stdout
or the failing tools' output on stderr, and `madoqua stats` shows its row.

**7. Exit codes.** `0` clean, `1` the commit is blocked, `2` madoqua could not
run. Keep them distinct in anything that wraps madoqua: `1` is your code's
problem, `2` is madoqua's.

next: run `madoqua run` with a `.py` change staged
