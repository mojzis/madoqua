# 0007 — A whole-repo run, from the same list of checks

Status: accepted

## Decision

`madoqua check` runs every check as if every file a commit could contain were
staged: tracked files, and untracked ones git does not ignore, minus those
deleted from the working tree. It fixes nothing, stages nothing and logs
nothing. Its verdict, its report of failures and its exit codes are the
hook's, and so are the venv guard, the overlay and `MADOQUA_SKIP`.

What differs between the two runs is said on the step, in the one list of
checks, with three keys:

- `when = "always" | "commit" | "full"` — which runs the check is part of.
  `always` is the default, so a configuration that never mentions it runs
  in both, exactly as written.
- `full_cmd` — what `madoqua check` runs instead of `cmd`, handed no file list:
  `biston scan --focus-args` at commit, `biston scan .` across the repo.
  Without it the full run runs `cmd`, with every matching file appended when
  `pass_files` says so. It is refused next to `when = "commit"` or `"full"`,
  where one of the two commands would never run.
- `blocking = false` — the check is reported, on stderr and in the verdict
  (`audit failed (advisory)`), but never fails the run. In both runs.

The three are check keys. On a fixer they are refused with an error rather
than ignored: fixers run only in the hook, and a fixer's failure already
never blocks.

`on` keeps its meaning — the files that wake a step — and is matched against
the whole-repo list instead of the staged one.

## Why

The repos this hook lives in also carry a `poe check` that runs the same
tools across the repository, with its own list, its own arguments and a
hand-written parallel shell. The two lists drift, and the shell had a bug no
one saw: its parallel half ended in a bare `wait`, which exits `0`, so only
the test suite's failure ever failed it. madoqua already runs tools in
parallel and treats the exit code as a contract (ADR 0001).

One list with per-step keys rather than a second `[tool.madoqua.full]` list,
because most checks are the same command in both runs, and a second list
would be a copy of the first that drifts — the problem this replaces. A step
that differs says how, next to the command it differs from. Per-step keys
also leave the overlay rules as they are: there is still one `check` and one
`extend_check`.

`full_cmd` takes no file list because the reason to write one is that the
tool knows how to find the whole repository itself, and a long argument list
is the thing that runs into `ARG_MAX` on a big repository.

No fix phase, because "is the repository clean" is a question, and a command
that answers it by rewriting files has answered a different one. Untracked
files are in because `ruff check .` sees them and a new module is the file
nobody has staged yet.

Not logged, because the log answers "is my hook getting slow", and one run
of the whole test suite would swamp the percentiles of a hook that takes a
second.

`blocking = false` exists because some checks are worth seeing and not worth
stopping for — a vulnerability audit of dependencies nobody in the commit
touched. Before this the only way to say that was `|| true` in a shell, which
madoqua does not have.

## What it costs

A check's two runs are configured in one table, which gets wide: a step with
`when`, `full_cmd`, `on` and `timeout_s` does not fit on a line.

Without `full_cmd`, a `pass_files` step is handed every matching file in the
repository on its command line. That is right for `ruff` and `ty`, and fails
on a repository large enough to exceed `ARG_MAX` — as a check that could not
start, exit `127`, naming the tool. `full_cmd` is the way out.

The full run has no timing history: `madoqua stats` cannot say whether it is
getting slower.

There is no whole-repo fix. `ruff format .` is still a command you run
yourself.

## What would make us revisit it

Wanting `madoqua check` timed — which would add a field to the log record
saying which run wrote it, and a filter to `stats`. Or a third run (CI,
pre-push) with its own differences, at which point `when` wants to be a list
and `full_cmd` a table keyed by run.
