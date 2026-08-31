# 0002 — The five seams the hook needs

Status: accepted

## Decision

Everything impure in madoqua lives in one of five modules, and nothing else may
reach the world:

| Module | Owns |
|---|---|
| `git.rs` | Every `git` invocation. |
| `runner.rs` | Every other process spawn, plus its timing, capture and timeout. |
| `venv.rs` | Reading `PATH`, probing for executables, and the `PATH` children get. |
| `timelog.rs` | Reading `HOME`, and reading and appending the timing log. |
| `clock.rs` | The wall clock. |

`config.rs` reads two known files and is the one deliberate exception: it is
where "what is configured" is decided, and splitting the read from the parse
would have produced a module whose only job was `read_to_string`.

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

Five modules for a program that is conceptually one script, and a trait
(`venv::Fs`) that exists only so tests can lie about the filesystem. Adding a
sixth kind of impurity means adding a module rather than a line.

The integration tests still need a real git repository, because `git.rs` is a
seam we did not put a fake behind — git's behaviour is the thing being relied
on, and a fake git would only test our idea of it.

## What would make us revisit it

A seam whose pure core never grows a second branch is a module that earned
nothing. `clock.rs` is the candidate: if it stays two functions forever, it
could fold into `timelog.rs`.
