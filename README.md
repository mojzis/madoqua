# madoqua

TODO: one-line description of madoqua.

[Documentation](https://mojzis.github.io/madoqua) ·
[Commands](https://mojzis.github.io/madoqua/commands/overview.html)

## Install

```sh
uv tool install madoqua
# or
pipx install madoqua
```

## Use

```sh
madoqua doctor          # report the resolved config and version
madoqua doctor --json   # the same, machine-readable
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Clean — nothing to report |
| `1` | The command found something worth reporting |
| `2` | The run could not complete |

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

## Development

```sh
make review-quick   # fmt, clippy, tests
make review         # the above plus cargo-audit and cargo-deny
make docs           # build the mdBook site + llms.txt
```

See [`docs/dev/ARCHITECTURE.md`](docs/dev/ARCHITECTURE.md) and
[`docs/adr/`](docs/adr/README.md).

## License

MIT — see [LICENSE](LICENSE).
