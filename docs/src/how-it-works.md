# How it works

## The pipeline

```mermaid
flowchart TD
    guard[venv guard<br/>PATH must reach .venv/bin/python] -->|no venv| blocked[exit 1<br/>with instructions]
    guard --> staged[git diff --cached<br/>*.py *.pyi]
    staged -->|nothing| quiet[exit 0<br/>no output]
    staged --> fix[fix phase<br/>sequential, writes]
    fix -->|a fixer that never ran| failures
    fix --> add[git add<br/>the same file list]
    add --> check[check phase<br/>parallel, read-only]
    check --> log[append one JSON line<br/>to the timing log]
    log -->|all pass| verdict[one line on stdout]
    log -->|any fails| failures[failing tools' output<br/>on stderr, exit 1]
```

The log is written before the verdict is printed, so a run is timed whether or
not it was allowed through. A fixer that exits non-zero is not a reason to
refuse the commit — `ruff check --fix` exits non-zero for what it could not
fix, and the check that follows is about to say so — but a fixer that *never
ran* is: no check reports a missing `ruff format`, and the commit would record
unformatted code under a verdict claiming it was formatted.

Fix steps run one at a time and in configuration order, because they rewrite
the same files and the formatter has to see the linter's output. Checks are
read-only, so they all run at once, each on its own thread with its own capture
of stdout and stderr. Each step times itself inside its thread, so what the log
records is how long the tool took and not how long it waited to be joined.

## Shape of the crate

```mermaid
flowchart LR
    main[main.rs<br/>parse, log, exit code] --> cli[cli.rs<br/>command bodies]
    cli --> hook[hook.rs<br/>the pipeline, the verdict]
    cli --> stats[stats.rs<br/>percentiles, table]
    cli --> install[install.rs<br/>the pre-commit shim]
    hook --> config[config.rs<br/>what to run]
    hook --> venv[venv.rs<br/>PATH]
    hook --> git[git.rs<br/>every git call]
    hook --> runner[runner.rs<br/>every other process]
    hook --> timelog[timelog.rs<br/>the log file]
    stats --> report[report.rs<br/>wire format]
    timelog --> report
```

`main.rs` stays thin: argument parsing, tracing setup, exit-code mapping. Every
command body lives in `cli.rs` or in the module it delegates to.

Anything that spawns a process or touches the filesystem lives behind a named
seam, and everything downstream of a seam takes already-parsed data. `venv.rs`
is the clearest case: the guard's decision is a pure function of the current
`PATH` and a filesystem oracle, so all four of its branches are unit-tested on
a machine with no virtualenv at all. See
[`docs/adr/`](https://github.com/mojzis/madoqua/tree/main/docs/adr) for the
decisions and what they cost.
