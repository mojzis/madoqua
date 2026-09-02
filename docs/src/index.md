# madoqua

A pre-commit hook for Python repos: format and lint-fix what you staged,
re-stage the result, run the read-only checks in parallel, and say one line
about it. Small, fast, silent until there's something to say.

A clean commit prints exactly this and nothing else:

```
pre-commit ok (3 py files, 1.8s, slowest: ty check 1.6s): ruff fix, ruff format applied & staged; ruff check, ty check passed
```

A commit with a problem prints only the tool that had one, and stops the
commit.

## At a glance

```sh
madoqua install   # write hooks/pre-commit and point git at it
madoqua run       # what the hook does; also what a bare `madoqua` does
madoqua stats     # what the hook has been costing you
madoqua guide     # what to do here, right now
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Clean — the commit may proceed |
| `1` | A check failed, or the virtualenv guard refused — the commit is blocked |
| `2` | The run could not complete — madoqua itself is misconfigured or broken |

The three are part of the contract: `1` and `2` are deliberately distinct, so
"your code is not ready" never reads as "madoqua is not working".

Start at [Setup](setup.md), then [Configuration](configuration.md).
