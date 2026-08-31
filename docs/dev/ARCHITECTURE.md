# Architecture

How the crate is put together. For *why*, and what each choice cost, see
[`../adr/`](../adr/README.md).

## Module map

| Module | Responsibility |
|---|---|
| `main.rs` | Parse arguments, set up tracing, map `Outcome` to an exit code. Nothing else. |
| `cli.rs` | The `clap` definitions and the body of every command. |
| `config.rs` | Reads `[tool.madoqua]` from the project's `pyproject.toml`. |
| `report.rs` | The JSON wire format. Field names here are public contract. |

## The rule the layout exists to enforce

Anything impure — spawning a process, reading the filesystem, reading the
environment — lives in a named seam module, and everything downstream of a seam
takes already-parsed data. The rules modules never reach the world directly.

That is what makes the interesting logic unit-testable with nothing installed:
a test constructs the parsed input by hand and asserts on the parsed output. A
new seam is a real decision and needs an ADR, not a new `Command::new` call
somewhere convenient.

## Tests

- **Unit tests** (`#[cfg(test)] mod tests` inside each module) cover internal
  logic, using hand-built inputs.
- **Integration tests** (`tests/*.rs`) drive the built binary through
  `assert_cmd`, so they cover argument parsing, stdout and exit codes — the
  parts a unit test cannot see. Shared helpers live in `tests/common/mod.rs`.

Every test asserts on a value, and every integration test that asserts on
stdout also asserts on the exit code.
