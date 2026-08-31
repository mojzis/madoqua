## madoqua

`madoqua` is this repo's pre-commit hook. It runs the fixers on the staged
Python files, re-stages them, runs the checks in parallel, and prints one line.
It runs itself on `git commit` — invoke it directly to see what a commit would
say before making one.

```sh
madoqua guide                     # what to do here, right now
madoqua run                       # what `git commit` will do; one line if clean
MADOQUA_SKIP="ty check" madoqua run   # drop a check by name, this run only
madoqua stats                     # which check is costing the most time
```

Start with `madoqua guide`: it prints setup instructions in a repo that is not
wired up yet, and triage instructions in one that is. `madoqua guide tune` is
the configuration reference.

Exit codes: `0` clean, `1` a check failed (the commit would be blocked), `2`
madoqua could not run. Failing checks print only the failing tool's output, on
stderr.

Configuration is `[tool.madoqua]` in `pyproject.toml`. Set `max_output_lines`
on a noisy check to keep its failures inside a context window.
