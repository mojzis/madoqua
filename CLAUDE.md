# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with
code in this repository.

## Project Overview

`madoqua` is a git pre-commit hook for Python repos, ported from a bash script.
It guards that the repo's `.venv` is what tools will run from, runs the fix
commands over the staged `*.py`/`*.pyi` files sequentially, re-stages them, runs
the check commands in parallel, and prints one line on success or the failing
tools' output on failure. Every run appends a timing record to a JSONL log that
`madoqua stats` summarises.

It is a hybrid Rust/Python project: a Rust binary packaged as a Python wheel via
maturin and published to PyPI.

Architecture details: `docs/dev/ARCHITECTURE.md`. Decisions and their costs:
`docs/adr/`.

## Prerequisites

- A Rust toolchain matching `rust-toolchain.toml`.
- `mdbook` and `mdbook-mermaid` for `make docs` — the pinned pair is in
  `docs/toolchain.sh`.
- The commands madoqua runs are configuration, not code: the built-in defaults
  are `ruff` and `ty`, resolved out of `<repo_root>/.venv/bin`. Tests never
  need them installed — the fixture repo in `tests/common/mod.rs` writes shell
  stubs into a stub `.venv/bin`, which the venv guard then puts on `PATH`.

## Common Commands

```sh
# Pre-commit checks (always run before committing)
cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-features

make review        # the above, plus audit and deny
make review-quick  # skip the network checks
make docs          # build the mdBook site + llms.txt
```

If formatting fails, fix it with `cargo fmt --all` and re-run.

## Development Workflow

All features and bug fixes follow TDD (red-green-refactor). No implementation
code without a failing test first. Bug fixes must include a regression test that
fails without the fix.

## Test Changes Require Deliberation

When a test fails during implementation:

1. **Stop and diagnose.** Understand WHY it fails before changing anything. Is
   the test wrong, or is the implementation wrong?
2. **Default assumption: the test is right.** Fix the implementation first.
3. **If the test genuinely needs updating** (requirements changed, API evolved),
   explain what changed and why the old assertion is no longer correct before
   modifying it.
4. **Never weaken an assertion just to make it pass.** Making a test more
   permissive without understanding the failure is not a fix.
5. **If uncertain, ask.** A 2-line question is cheaper than a silent wrong
   decision.

## Key Invariants

These are the ones that hold today. Add to this list as the design settles — an
invariant the code no longer upholds is a bug in one of the two.

- **Exit codes are part of the contract.** `0` clean, `1` findings, `2` the run
  could not complete. Do not collapse `1` and `2` (ADR 0001). A command that is
  an inventory rather than a verdict returns `0` or `2` and never `1`, and its
  docs must say so — that is why `stats` never returns `1`.
- **A blocked commit is `1`, not `2`.** A failed check and a refused virtualenv
  guard both mean "your commit is not ready", which is a finding. `2` is
  reserved for madoqua being unable to do its job at all: not a repository,
  unreadable config, a command it cannot honour.
- **The verdict is one line, on success only.** A hook that prints on every
  clean run trains people to stop reading it. Failures print only the tools
  that failed. Anything else you are tempted to say goes on stderr, or nowhere.
- **The fix phase is sequential and the check phase is parallel.** Fixers
  rewrite the same files in a meaningful order; checks are read-only. A fixer's
  non-zero exit is never fatal — the check that follows reports it. A fixer
  that could not be *started* is fatal, because no check follows a tool that
  isn't installed, and a verdict saying `applied & staged` about it would be
  the exact opposite of what happened.
- **Logging is best-effort.** A failure to write the timing log warns on stderr
  and the commit proceeds. Nothing about timings may ever block a commit.
- **A no-op run is not logged.** Runs with no staged Python files write
  nothing, or they dominate the percentiles of any repo that also commits
  prose.
- **`main.rs` stays thin.** Argument parsing, tracing setup, exit-code mapping.
  Command bodies belong in `cli.rs`.
- **Impurity lives in named seams.** Every process spawn, filesystem read and
  environment lookup belongs in a module that exists to do it, and everything
  downstream of a seam takes already-parsed data. That is what keeps the rules
  unit-testable with nothing installed. A new seam needs an ADR, not a
  conveniently placed `Command::new` — and `hook.rs` is not a seam, so an
  `std::env::var` in it is a bug in the layout (ADR 0002).
- **Logs go to stderr, output to stdout.** `init_tracing` writes to stderr on
  purpose: a `--json` run must be pipeable into `jq` with `--verbose` on.
- **`clippy::unwrap_used` / `expect_used` warn outside tests, and the documented
  `cargo clippy … -D warnings` makes them fatal.** Use `anyhow::Context` on
  every `?` that crosses an I/O or parsing boundary — a bare `?` loses which
  operation failed.
- **Serde field names in `report.rs` are public contract.** Renaming one is a
  breaking change, and `docs/src/commands/` must be updated in the same commit.
- **Config keys that a flag can override are `Option`, not defaulted.** "Absent"
  has to stay distinguishable from "set to the default value", or the
  flag-beats-file-beats-built-in precedence cannot be expressed.
- **Guide prose lives in `docs/src/guide/`, never in `guide.rs`.** The CLI
  `include_str!`s the same bytes the site publishes; a second copy is a copy
  that drifts. The tests are the contract: every madoqua command a guide shows
  is fed through the real `clap::Command`, every config key it names is checked
  against what the deserializer accepts (`config::keys`), each page is capped
  at 60 lines and ends with exactly one `next: run` line. Cut a guide rather
  than raise the cap — one past a screenful stops being read.
- **`guide` is an inventory, so it returns `0` or `2` and never `1`**, like
  `stats`. It also never needs a git repository: detection looks at one
  directory and treats an unreadable or malformed file as "not configured"
  rather than as a reason to refuse to print.
- **Every test asserts on a value.** Running without panicking is not a test,
  and an integration test that asserts on stdout also asserts on the exit code.

## Docs

The mdBook site under `docs/` deploys to GitHub Pages on push to `main`
(`.github/workflows/docs.yml`), along with `llms.txt` and `llms-full.txt`.

- Shared prose lives in `docs/shared/` and is injected into `README.md` and
  `docs/src/setup.md` by `docs/inject-shared.sh`. Edit the shared file, then run
  the script — never edit between the `<!-- BEGIN SHARED:... -->` markers.
- `docs/gen-version.sh` syncs the docs version badge with `Cargo.toml`.
- `docs/toolchain.sh` pins the `mdbook` / `mdbook-mermaid` pair and is the one
  source of truth for both `make docs` and the workflow. The two share a
  preprocessor protocol that changed between mdbook 0.4 and 0.5, and a
  mismatched pair fails with `Unable to parse the input`, naming neither tool —
  `make docs` runs the check first so it names both.
- `docs/src/guide/*.md` is served twice: by the site, and by `madoqua guide`,
  which `include_str!`s it. Editing one of those pages changes the binary's
  output, so `cargo test` is part of editing them (ADR 0004).
- Adding a page means adding it to `docs/src/SUMMARY.md`; the `llms.txt`
  generator reads that file.
