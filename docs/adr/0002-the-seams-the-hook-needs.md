# 0002 — The seams the hook needs

Status: accepted

## Decision

Everything impure in madoqua lives in one of seven modules, and nothing else
may reach the world:

| Module | Owns |
|---|---|
| `git.rs` | Every `git` invocation. |
| `runner.rs` | Every other process spawn, plus its timing, capture and timeout. |
| `venv.rs` | Reading `PATH`, probing for executables, and the `PATH` children get. |
| `timelog.rs` | Reading `HOME`, and reading and appending the timing log. |
| `clock.rs` | The wall clock. |
| `config.rs` | Everywhere the configuration is written down: `pyproject.toml`, the overlay, and `MADOQUA_SKIP`. |
| `install.rs` | Writing the `hooks/pre-commit` shim, and only that. |

The last two are the deliberate exceptions to "a seam owns one kind of
impurity", and both are exceptions for the same reason. `config.rs` reads two
files and one environment variable because it is where "what is configured" is
decided, and splitting the read from the parse would have produced a module
whose only job was `read_to_string`. `install.rs` writes three things — a
directory, a file, a mode — once, on an explicit `madoqua install`, and is the
only module that writes anything outside the log; a `fs.rs` it were the sole
caller of would be a module that earned nothing.

Neither is a licence to reach the world from anywhere else: the test is whether
the module *exists in order to* touch what it touches.

Each seam exposes a pure core and a thin impure wrapper wherever the decision
is interesting. `venv::plan(root, path_value, fs)` is the reference shape: the
guard's whole decision is a function of a `PATH` string and a filesystem trait,
and `venv::guard(root)` is the three lines that supply the real ones.

## Why

The hook's interesting logic is all in the awkward cases — a venv that exists
but is shadowed, a check that names a step that no longer exists, a log path
that points outside the repo. Those are the cases nobody sets up by hand in an
integration test. Behind a seam they are unit tests that run in microseconds on
a machine with no Python, no `ruff` and no `.venv`.

## What it costs

Seven modules for a program that is conceptually one script, and a trait
(`venv::Fs`) that exists only so tests can lie about the filesystem. Adding a
new kind of impurity means adding a module rather than a line — and two of the
seven are already exceptions, which is about as many as the rule survives.

The integration tests still need a real git repository, because `git.rs` is a
seam we did not put a fake behind — git's behaviour is the thing being relied
on, and a fake git would only test our idea of it.

## What would make us revisit it

A seam whose pure core never grows a second branch is a module that earned
nothing. `clock.rs` is the candidate: if it stays two functions forever, it
could fold into `timelog.rs`.
