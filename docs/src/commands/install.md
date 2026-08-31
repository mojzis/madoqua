# `install`

```sh
madoqua install
```

Writes `hooks/pre-commit` in the working tree:

```sh
#!/bin/sh
exec madoqua run
```

makes it executable, and runs `git config core.hooksPath hooks`.

Running it twice changes nothing.

## Why the working tree and not `.git/hooks`

`hooks/pre-commit` can be committed, so everyone who clones the repo gets the
same hook without a setup step. The shim holds no logic for the same reason the
bash script it replaces is being retired: a two-line `exec` is fixed by
upgrading the binary.

Note that `core.hooksPath` is per-clone git config, so each clone still runs
`madoqua install` once — or you can set it in your own `~/.gitconfig`.

## Exit codes

`0`, unless the run could not complete (`2`).
