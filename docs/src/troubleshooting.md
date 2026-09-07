# Troubleshooting

## `madoqua: command not found`

The wheel installs a binary onto your tool path. With `uv tool install`, check
that `~/.local/bin` is on `PATH`.

If the hook itself fails this way, remember that `git` runs `hooks/pre-commit`
with your login `PATH`, not your shell's. The shim `madoqua install` writes
now looks in `<repo>/.venv/bin` first, so a dev-dependency install
works without the venv active; a shim written by an older version only says
`exec madoqua run`; run `madoqua install` again to replace it. Either way the
binary has to be in the venv for that lookup to find it: `uv add --dev madoqua`.

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
`hooks.local.toml`.

## Does it work in a `git worktree`?

Yes, and it uses the same overlay and the same timing log as the clone the
worktree belongs to: a linked worktree's `.git` is a *file* pointing at the
clone's metadata directory, and madoqua asks git where that is rather than
joining `.git/` onto the checkout. A `log` you configure yourself stays
relative to the working tree you are committing in.

madoqua 0.2.2 and earlier joined the path literally, so every run in a linked
worktree failed with `cannot read …/.git/hooks.local.toml: Not a directory
(os error 20)` and exit `2`, whether or not an overlay existed. Upgrade rather
than working around it: the tools madoqua runs are the ones guarding the
commit.

## A commit went through with unformatted code

madoqua only sees `git diff --cached`. If a file was not staged, it was not
checked. Note also that the fixers run on whole files: a file staged with
`git add -p` is committed in full.
