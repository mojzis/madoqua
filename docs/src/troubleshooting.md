# Troubleshooting

## `madoqua: command not found`

The wheel installs a binary onto your tool path. With `uv tool install`, check
that `~/.local/bin` is on `PATH`.

## The output is not what I expect

Run with `--verbose` (or `RUST_LOG=debug`) — logs go to stderr, so they never
contaminate the JSON on stdout:

```sh
madoqua --verbose doctor
```

## `cannot parse .../pyproject.toml`

madoqua reads `[tool.madoqua]` out of the project's `pyproject.toml`. A missing
file, or a file with no `[tool.madoqua]` table, is fine — both mean "defaults".
Malformed TOML is not, and the error names the file.
