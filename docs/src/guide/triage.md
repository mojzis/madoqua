# Triage a blocked commit

madoqua blocked a commit, or `madoqua run` exited non-zero. Work it before you
try to commit again.

**Read what it printed.** A failing run prints the failing tools' output on
stderr and nothing else; stdout stays empty. Exit `1` means a check failed or
the virtualenv guard refused - your commit is not ready. Exit `2` means
madoqua could not run at all, and the stderr message names what it could not
do; that is a configuration bug, not a finding about your code.

**Apply the remedy ladder.** Take the first rule that applies.

1. `no virtualenv at .../.venv`: create it and install the tools the checks
   call - `uv venv && uv sync`. Nothing about your code is wrong.
2. `exists but its python is not the one that would run`: something on PATH
   shadows `.venv/bin/python`, usually a version-manager shim.
   `command -v python` at the repository root names it.
3. A fixer could not start: that tool is not installed in `.venv`. Install it.
   madoqua refuses the commit rather than claim it applied a formatter that
   never ran.
4. A check failed: fix what its output names, stage the fix, re-run. The
   fixers already rewrote and re-staged what they could, so what is left is
   what no fixer can do for you.
5. The check is wrong for this repository - wrong tool, or wrong arguments -
   and only then: change it in `[tool.madoqua]` and say why in the commit
   message. See `madoqua guide tune`.

**Around every edit.** Fix the code, stage it, and run `madoqua run` again.
Done means one `pre-commit ok` line on stdout and nothing on stderr.

**Do not:**

- Do not commit with `--no-verify`. That is not passing the hook, that is not
  running it.
- Do not use `MADOQUA_SKIP` to get past a check that is telling the truth. It
  is for a check that is broken or slow right now.
- Do not delete a check from `[tool.madoqua]` to make one commit go through.
  That is repo-wide policy, and it hides the same failure for everyone else.
- Do not raise `timeout_s` or `max_output_lines` to change a verdict. Neither
  decides whether a check passes.

next: run `madoqua run`
