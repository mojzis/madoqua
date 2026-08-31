# Setup

## Install

madoqua is a Rust binary shipped as a Python wheel, so any Python installer
works:

```sh
uv tool install madoqua
# or
pipx install madoqua
```

## Verify

```sh
madoqua doctor
```

## Using it from Claude Code

Paste this into your project's `CLAUDE.md`:

<!-- BEGIN SHARED:claude-snippet -->
```markdown
## madoqua

TODO: describe what madoqua does, in the voice you would use to tell an agent
when to reach for it.

```sh
madoqua doctor          # report the resolved config and version
madoqua doctor --json   # the same, machine-readable
```

Exit codes: `0` clean, `1` findings, `2` the run could not complete.
```
<!-- END SHARED:claude-snippet -->
