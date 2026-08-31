# `madoqua guide`

Print the instructions for one of the three moments someone — usually an agent
— meets madoqua.

```sh
madoqua guide          # let madoqua choose
madoqua guide setup    # not wired into this repo yet
madoqua guide triage   # a run blocked a commit
madoqua guide tune     # the key reference
```

| Topic | For |
|---|---|
| [`setup`](../guide/setup.md) | A repository madoqua is not wired into yet. |
| [`triage`](../guide/triage.md) | A run that exited non-zero. |
| [`tune`](../guide/tune.md) | The registry, the layers, skipping and the log. |

## Choosing a topic

With no topic, madoqua looks at one directory — `--root` when you pass it, the
current one otherwise — and prints `setup` when it finds nothing there and
`triage` when it does. Detection uses that directory as given and never walks
up, so point it at the repository root. What it looks for, in order:

1. `hooks/pre-commit` invoking madoqua — the shim [`install`](install.md)
   writes.
2. `.git/hooks/pre-commit` invoking madoqua — a hand-wired hook.
3. `[tool.madoqua]` in `pyproject.toml`, parsed rather than grepped.
4. `.git/hooks.local.toml`, the personal overlay.

`tune` is never auto-selected: it is a reference, and nothing about a
repository's state says "you need the reference right now".

The first line of the output names the topic and why it was chosen, so a reader
that passed no topic can tell whether to trust what follows:

```
# madoqua guide: configured via hooks/pre-commit -> triage
```

An explicit topic reads nothing from disk and prints
`# madoqua guide: tune` instead.

## Exit codes

`guide` is an inventory, not a verdict: it returns `0`, or `2` if it could not
write its output. It never returns `1` and never needs a git repository.

## Where the text comes from

The three pages under [Agent guide](../guide/setup.md) are `include_str!`d into
the binary, so the site and the CLI serve the same bytes. Tests hold them to it:
every madoqua command a guide shows is fed through the real argument parser,
every config key it names is checked against what the deserializer accepts, and
no guide may exceed 60 lines — one that grows past a screenful stops being read.
