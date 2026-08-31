# madoqua

TODO: one-paragraph description of what madoqua does and who it is for.

## At a glance

```sh
madoqua doctor
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Clean — nothing to report |
| `1` | The command found something worth reporting |
| `2` | The run could not complete |

The three are part of the contract: `1` and `2` are deliberately distinct, so
"we looked and found something" never reads as "we could not look".

Start at [Setup](setup.md), then [Commands](commands/overview.md).
