# Task: port `pre-commit` bash hook to a Rust CLI

Build a small, opinionated Rust CLI that replaces the bash pre-commit hook below.
It is a personal/team tool for a handful of Python repos — bias every decision
toward simplicity and exact preservation of the current behavior. Do not add
features beyond this spec.

Name: `madoqua` (single binary; the dik-dik genus — silent until there's
something to say). Use it for the crate, binary, `[tool.madoqua]` table, env
vars, and log dir. README tagline: "small, fast, silent until there's something
to say."

## Reference implementation (current behavior, port faithfully)

```bash
#!/usr/bin/env bash
# venv guard -> fix (sequential, writes) -> add -> checks (parallel, read-only) -> one-line verdict
set -uo pipefail

repo_root=$(git rev-parse --show-toplevel)

declare -a names cmds
check() { names+=("$1"); cmds+=("$2"); }
check "ruff check" "ruff check --quiet --"
check "ty check"   "ty check --"

local_cfg="$repo_root/.git/hooks.local.sh"
[[ -f $local_cfg ]] && source "$local_cfg"

if [[ -n "${PRECOMMIT_SKIP:-}" ]]; then
  # filter names/cmds by comma-separated names in PRECOMMIT_SKIP
  ...
fi

# venv guard: `command -v python` must equal "$repo_root/.venv/bin/python";
# if not and .venv/bin/activate exists -> source it, note "(auto-activated .venv)";
# re-verify after activation; hard-fail with instructive message otherwise.

# staged files: git diff --cached --name-only -z --diff-filter=ACMR -- '*.py' '*.pyi'
# empty -> exit 0 silently

# fix phase (sequential, order matters), then: git add -- "${files[@]}"
ruff check --fix --quiet -- "${files[@]}" || true
ruff format --quiet -- "${files[@]}"

# check phase: all checks in parallel, stdout+stderr captured per check
# success -> exactly ONE line:
#   pre-commit ok (3 py files) (auto-activated .venv): ruff fix+format applied & staged; ruff check, ty check passed
# failure -> for each failed check only:
#   == <name> failed ==
#   <captured output>
# exit 1
```

## Architecture

- Rust, edition 2024. **No async runtime.** `std::process::Command` + `std::thread`
  for the parallel check phase (spawn all, join all).
- Dependencies, exhaustive: `clap` (derive), `serde`, `toml`, `serde_json`, `anyhow`.
  If you're tempted to add another, don't.
- Subcommands:
  - `madoqua run` — the hook itself (also the default when invoked with no args,
    so a hooks shim can just `exec madoqua`).
  - `madoqua install` — writes `hooks/pre-commit` shim (`#!/bin/sh\nexec madoqua run`,
    chmod +x) and runs `git config core.hooksPath hooks`. Idempotent.
  - `madoqua stats` — see Logging.

## Configuration

Read `[tool.madoqua]` from `<repo_root>/pyproject.toml`:

```toml
[tool.madoqua]
fix = [
  "ruff check --fix --quiet",   # string form: files appended, name derived from first word(s)
  "ruff format --quiet",
]
check = [
  "ruff check --quiet",
  { name = "ty", cmd = "ty check", pass_files = true, timeout_s = 120, max_output_lines = 200 },
]
# log = ".git/hook-timings.jsonl"   # optional override, see Logging
```

- Entries are either a plain string or an inline table (serde untagged enum).
  Defaults: `pass_files = true`, no timeout, no output cap.
- `pass_files = false` runs the command without the file list (repo-wide tools).
- `max_output_lines` truncates captured output tail-first with a
  `... (N lines truncated)` marker — this is the token-budget knob.
- Commands are split with basic shell-words rules (whitespace, quotes); no pipes,
  no expansions. Reject anything containing `|`, `;`, `&&` with a clear error.
- If `[tool.madoqua]` is absent, use built-in defaults identical to the bash
  registry above (so the tool works with zero config).

### Personal overlay

If `<repo_root>/.git/hooks.local.toml` exists, deserialize the same schema and merge:
- `check` / `fix` present in overlay → **replaces** that list entirely.
- `extend_check` / `extend_fix` in overlay → **appends** to the repo list.
- Scalar keys (`log`, etc.) → overlay wins.
Document these four lines of semantics in `--help`.

### One-off skips

`MADOQUA_SKIP="ty,ruff check"` — comma-separated check *names*, filters the check
phase only. Keep the exact matching-by-name behavior of the bash version.

## Behavior details to preserve exactly

1. Venv guard semantics as in the reference, including both error messages and the
   `(auto-activated .venv)` note in the verdict. "Activate" in Rust = prepend
   `.venv/bin` to PATH and set VIRTUAL_ENV for child processes (no sourcing).
2. Staged file selection: same git command, same filters, NUL-safe.
3. No staged py files → exit 0 with **no output at all**.
4. Fix phase sequential in config order; non-zero exit from a fixer does NOT
   abort (checks will report); `git add` the file list after fixers.
5. Check phase fully parallel; capture combined stdout+stderr per check.
6. Verdict line format (success):
   `pre-commit ok (<N> py files, <total>s, slowest: <name> <t>s)[ (auto-activated .venv)]: <fix summary>; <check names> passed`
   — this extends the bash version with the two timing fields. Keep it ONE line.
7. Failure output: only failing checks, `== <name> failed ==` header + output
   (post-truncation). Exit 1.
8. All informational/error messages to stderr; verdict line to stdout.

## Logging (new)

Append one JSON line per run to the log file. Default path
`<repo_root>/.git/hook-timings.jsonl`; overridable via `log` config key.
Support `~` expansion so a user CAN centralize under home
(e.g. `log = "~/.local/state/madoqua/timings.jsonl"`); if the path is outside the
repo, add a `"repo"` field (repo_root basename) to each record. Create parent
dirs as needed. Logging failures must never fail the hook — best-effort, warn once
to stderr.

Record schema:

```json
{"ts":"2026-08-31T14:03:22+02:00","head":"a1b2c3d","files":3,"total_ms":1834,
 "venv_auto_activated":false,
 "steps":[{"name":"ruff fix","phase":"fix","ms":41,"exit":0},
          {"name":"ty","phase":"check","ms":1620,"exit":0}]}
```

- `head` = short sha of HEAD (the parent — the commit doesn't exist yet).
- Per-step timing measured inside each thread (`Instant`), not at join time.
- Timeouts log the step with `"exit":-1,"timed_out":true` and count as failure.

### `madoqua stats`

Reads the log, prints per-step `n / p50 / p95 / max` (ms), sorted by p95 desc,
plus total-run p50/p95. Flags: `--days N` (default 30), `--json`. Compute
percentiles in Rust — no external deps. If the log records include `repo`,
add a `--repo <name>` filter. Keep the output a plain aligned table, no colors.

## Packaging

- Phase 1 (this task): plain cargo project, binary installable via
  `cargo install --path .`. Structure the crate so main.rs is thin over a lib.
- Phase 2 (stub only): add maturin scaffolding (`pyproject.toml` with
  `[build-system] requires=["maturin"]`, `[tool.maturin] bindings="bin"`) so the
  binary ships as a Python wheel and repos can `uv add --dev madoqua`. Add a
  `justfile` or short README section for `maturin build`. Do not set up CI.

## Tests

Integration tests against real temp git repos (init, commit base, stage files):
- clean run → single verdict line, exit 0, log line written, files restaged after fix
- check failure → only failing tool's output, exit 1
- no py files staged → no output, exit 0, still logs? NO — no-op runs are not logged
- `pass_files = false` check receives no file args
- overlay replace vs extend semantics
- MADOQUA_SKIP filtering
- truncation with max_output_lines
- venv guard: missing .venv → exit 1 with the instructive message
Use `assert_cmd`-style testing only if you can do it without adding it as a
dependency — otherwise plain `std::process` in tests is fine.

## Non-goals — do not implement

Other hook types (pre-push etc.), non-Python file filters, config in any file
other than pyproject.toml + the overlay, colors/spinners/progress output, daemon
mode, watching, git stash handling for partially staged files (documented
limitation: agents and `git add <file>` workflows stage whole files).

## Definition of done

`madoqua install && git commit` in a test repo produces the one-line verdict with
timing, `.git/hook-timings.jsonl` grows by one valid record, `madoqua stats`
renders the table, and all integration tests pass.
