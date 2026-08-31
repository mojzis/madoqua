# `doctor`

Reports the resolved configuration and the version of the binary you are
running. Use it to confirm an install, and to confirm that madoqua is reading
the `pyproject.toml` you think it is.

```sh
madoqua doctor
madoqua doctor --json
```

## Flags

| Flag | Meaning |
|---|---|
| `--json` | Emit the report as JSON instead of human-readable text. |

## JSON shape

```json
{
  "version": "0.1.0"
}
```

Every field name is part of the contract — renaming one is a breaking change.

## Exit codes

`0` always, unless the run could not complete (`2`).
