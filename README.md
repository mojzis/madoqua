# madoqua

A pre-commit hook for Python repos: fix what you staged, re-stage it, run the
checks in parallel, say one line. Small, fast, silent until there's something
to say.

```
pre-commit ok (3 py files, 1.8s, slowest: ty check 1.6s): ruff fix, ruff format applied & staged; ruff check, ty check passed
```

That is the entire output of a clean commit. A commit with a problem prints
only the tool that had one.

[Documentation](https://mojzis.github.io/madoqua) ·
[Configuration](https://mojzis.github.io/madoqua/configuration.html) ·
[Commands](https://mojzis.github.io/madoqua/commands/overview.html)

## Install

```sh
uv tool install madoqua
# or
pipx install madoqua
# or, per project
uv add --dev madoqua
```

## Use

```sh
madoqua install   # write hooks/pre-commit and point git at it
madoqua run       # what the hook does; also what a bare `madoqua` does
madoqua stats     # what the hook has been costing you
```

madoqua needs a virtualenv at `<repo_root>/.venv` with the tools it calls
installed — it refuses to run a `ruff` from outside it, because a version that
disagrees with CI produces failures that look like your code's fault. With the
defaults that means `uv venv && uv sync`.

## Configure

Zero configuration gets you `ruff check --fix`, `ruff format`, `ruff check` and
`ty check`. To change that, add `[tool.madoqua]` to `pyproject.toml`:

```toml
[tool.madoqua]
fix = ["ruff check --fix --quiet", "ruff format --quiet"]
check = [
  "ruff check --quiet",
  { name = "ty", cmd = "ty check", timeout_s = 120, max_output_lines = 200 },
]
```

`hooks.local.toml` in the repo's git directory — `.git/hooks.local.toml` in an
ordinary clone — is a personal, uncommitted overlay on the same schema:
`check`/`fix` replace a list, `extend_check`/`extend_fix` append to it, scalars
win. `MADOQUA_SKIP="ty,ruff check"` drops checks by name for one run.

See [Configuration](https://mojzis.github.io/madoqua/configuration.html).

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Clean — the commit may proceed |
| `1` | A check failed, or the virtualenv guard refused — the commit is blocked |
| `2` | The run could not complete — madoqua itself is misconfigured or broken |

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

## Development

```sh
make review-quick   # fmt, clippy, tests
make review         # the above plus cargo-audit and cargo-deny
make docs           # build the mdBook site + llms.txt
make wheel          # build the Python wheel with maturin
```

Installing from a checkout:

```sh
cargo install --path .
```

See [`docs/dev/ARCHITECTURE.md`](docs/dev/ARCHITECTURE.md) and
[`docs/adr/`](docs/adr/README.md).

## License

MIT — see [LICENSE](LICENSE).
