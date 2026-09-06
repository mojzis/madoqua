# Troubleshooting

## `madoqua: command not found`

The wheel installs a binary onto your tool path. With `uv tool install`, check
that `~/.local/bin` is on `PATH`.

If the hook itself fails this way, remember that `git` runs `hooks/pre-commit`
with your login `PATH`, not your shell's. The shim `madoqua install` writes
now looks in `<repo>/.venv/bin` first, so a dev-dependency install
works without the venv active; a shim written by an older version only says
`exec madoqua run`. Run `madoqua install` again to replace it, or add
`madoqua` to the venv with `uv add --dev madoqua`.

## `madoqua: no virtualenv at …/.venv`

madoqua only runs tools out of the repo's own virtualenv. Create one and
install what the checks call:

```sh
uv venv && uv sync
```

## The verdict says `(auto-activated .venv)` every time

That is madoqua telling you your shell has not activated the venv — it worked
around it for the hook's children, but everything else you type is using a
different `python`. Activate it, or use `uv run`.

## `…/.venv exists but its python is not the one that would run`

Something on `PATH` shadows `.venv/bin/python` even after madoqua puts it
first — usually a shim from a version manager. `command -v python` from the
repo root will name it.

## A check is slow and I do not know which

```sh
madoqua stats
```

Sorted by p95 descending, so the first row is the one worth fixing. Give that
step a `timeout_s` while you work on it, or drop it for a single commit with
`MADOQUA_SKIP="<name>"`.

## A failing check floods my terminal (or my agent's context)

Cap it:

```toml
check = [{ name = "ty", cmd = "ty check", max_output_lines = 200 }]
```

The first 200 lines are kept and the rest becomes
`... (N lines truncated)`.

## `` `|` needs a shell, and madoqua runs commands directly ``

Commands are split with quote-aware whitespace rules and executed directly —
there is no shell, so pipes, redirections and expansions are refused rather
than passed along as literal arguments. Put the pipeline in a script and call
the script.

## The output is not what I expect

Run with `--verbose` (or `RUST_LOG=debug`) — logs go to stderr, so they never
contaminate stdout:

```sh
madoqua --verbose run
```

## `cannot parse .../pyproject.toml`

madoqua reads `[tool.madoqua]` out of the project's `pyproject.toml`. A missing
file, or a file with no `[tool.madoqua]` table, is fine — both mean "defaults".
Malformed TOML is not, and the error names the file. The same goes for
`.git/hooks.local.toml`.

## A commit went through with unformatted code

madoqua only sees `git diff --cached`. If a file was not staged, it was not
checked. Note also that the fixers run on whole files: a file staged with
`git add -p` is committed in full.
