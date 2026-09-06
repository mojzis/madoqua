# `install`

```sh
madoqua install
```

Writes `hooks/pre-commit` in the working tree:

```sh
#!/bin/sh
root=$(git rev-parse --show-toplevel)
[ -x "$root/.venv/bin/madoqua" ] && exec "$root/.venv/bin/madoqua" run
exec madoqua run
```

makes it executable, and runs `git config core.hooksPath hooks`.

Running it twice changes nothing.

## Why the working tree and not `.git/hooks`

`hooks/pre-commit` can be committed, so everyone who clones the repo gets the
same hook without a setup step. The shim decides where the binary is and
nothing else, for the same reason the bash script it replaces is being
retired: everything past the `exec` is fixed by upgrading the binary.

## Why `.venv/bin` before `PATH`

git runs hooks with your login `PATH`, not your shell's. madoqua installed as
a dev dependency is on `PATH` only while the venv is active, and a fish login
shell, an IDE's git integration or CI never activates it - so a shim that
only said `exec madoqua run` failed those commits with `madoqua: not found`.
The [virtualenv guard](run.md) cannot help there: it runs inside a madoqua
that was never found. The shim tries the repo's own venv first and falls back
to `PATH`, which is where `uv tool install` puts it.

Note that `core.hooksPath` is per-clone git config, so each clone still runs
`madoqua install` once — or you can set it in your own `~/.gitconfig`.

## Exit codes

`0`, unless the run could not complete (`2`).
