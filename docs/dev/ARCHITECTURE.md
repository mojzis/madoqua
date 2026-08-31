# Architecture

How the crate is put together. For *why*, and what each choice cost, see
[`../adr/`](../adr/README.md).

## Module map

| Module | Responsibility | Seam? |
|---|---|---|
| `main.rs` | Parse arguments, set up tracing, map `Outcome` to an exit code. Nothing else. | |
| `cli.rs` | The `clap` definitions and the body of every command. | |
| `hook.rs` | The pipeline, and the wording of the verdict. | |
| `config.rs` | What to run: `[tool.madoqua]`, the `.git/hooks.local.toml` overlay, `MADOQUA_SKIP`, the merge rules, command splitting. | reads two files and one variable |
| `stats.rs` | Percentiles and the table. Pure. | |
| `report.rs` | The JSON wire formats — the log record and `stats --json`. Field names here are public contract. | |
| `git.rs` | **Every** `git` invocation in the crate. | yes |
| `runner.rs` | **Every other** process spawn: fix sequentially, checks in parallel, timeouts, output capture and truncation. | yes |
| `venv.rs` | The virtualenv guard and the `PATH` children inherit. `plan` is pure; `guard` is the one-line impure wrapper. | yes |
| `timelog.rs` | Resolving the log path (`~`, in-repo or not) and appending to it. | yes |
| `clock.rs` | The wall clock, and RFC 3339 without a date crate. | yes |
| `install.rs` | Write the `hooks/pre-commit` shim and point git at it. | writes three things, once |

## Dependencies

`clap`, `serde`, `serde_json`, `toml`, `anyhow`, `tracing`. Everything else is
`std`: the parallel check phase is `std::thread::scope`, the timeout is
`try_wait` on a poll loop, percentiles are ten lines of arithmetic, and the
timestamp is Howard Hinnant's civil-date algorithm. A hook that has to install
half of crates.io before it can lint a file is a hook people uninstall.

## The rule the layout exists to enforce

Anything impure — spawning a process, reading the filesystem, reading the
environment — lives in a named seam module, and everything downstream of a seam
takes already-parsed data. The rules modules never reach the world directly.

That is what makes the interesting logic unit-testable with nothing installed:
a test constructs the parsed input by hand and asserts on the parsed output. A
new seam is a real decision and needs an ADR, not a new `Command::new` call
somewhere convenient.

`venv.rs` is the pattern in miniature. `plan(root, path_value, fs)` takes the
current `PATH` as an `OsStr` and the filesystem as a trait object, so all four
of its outcomes — already active, activated, no venv, shadowed — are unit tests
that run on a machine with no Python at all. `guard(root)` is the three lines
that read the real `PATH` and hand it a `RealFs`.

## Tests

- **Unit tests** (`#[cfg(test)] mod tests` inside each module) cover internal
  logic, using hand-built inputs.
- **Integration tests** (`tests/*.rs`) drive the built binary against a real
  temporary git repository, so they cover argument parsing, the git calls, the
  log and exit codes — the parts a unit test cannot see. Shared helpers live in
  `tests/common/mod.rs`.

  The fixture repo gets a stub `.venv` whose `bin` holds the fake tools. That
  is not a convenience: it means the guard's activation path is what puts those
  tools on `PATH`, so every test that runs a tool also proves the guard works,
  and none of them need `ruff` or `ty` installed.

Every test asserts on a value, and every integration test that asserts on
stdout also asserts on the exit code.
