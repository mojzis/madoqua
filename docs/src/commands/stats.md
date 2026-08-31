# `stats`

```sh
madoqua stats
madoqua stats --days 7
madoqua stats --json
madoqua stats --repo myproject   # for a log shared between repos
```

Reads the timing log and prints one row per step:

```
step         n   p50   p95   max
ty check    84  1620  2340  4100
ruff check  84    41    58    93

total run   84  1712  2455  4180
```

Sorted by p95 descending, because p95 is the number worth tuning against: the
mean of a check that is usually instant and occasionally twenty seconds
describes neither case. All figures are milliseconds, and percentiles are
nearest-rank, so every one of them is a duration that actually happened.

The whole-run total is set apart because it is *not* the sum of the rows above
it — checks run in parallel.

## Flags

| Flag | Default | Meaning |
|---|---|---|
| `--days <N>` | `30` | How far back to look. |
| `--json` | off | Emit the report as JSON instead of a table. |
| `--repo <NAME>` | none | Only count runs from this repository. Only meaningful when `log` points outside the repo. |

## JSON shape

```json
{
  "days": 30,
  "runs": 84,
  "total": { "n": 84, "p50": 1712, "p95": 2455, "max": 4180 },
  "steps": [
    { "name": "ty check", "n": 84, "p50": 1620, "p95": 2340, "max": 4100 }
  ]
}
```

`repo` appears at the top level only when `--repo` was given. Every field name
is part of the contract — renaming one is a breaking change.

## The log itself

One JSON object per run, appended to
[`log`](../configuration.md#where-timings-go):

```json
{"ts":"2026-08-31T12:03:22Z","head":"a1b2c3d","files":3,"total_ms":1834,
 "venv_auto_activated":false,
 "steps":[{"name":"ruff fix","phase":"fix","ms":41,"exit":0},
          {"name":"ty check","phase":"check","ms":1620,"exit":0}]}
```

- `ts` is UTC. Local offsets would need a timezone database; `Z` is
  unambiguous and sorts, which matters for a log several machines may share.
- `head` is the short sha of the *parent* — the commit being made does not
  exist yet.
- `exit` is the tool's own code, or `-1` when madoqua killed it for outliving
  its timeout, or `127` when the tool never ran at all. A tool killed by
  something else — a segfault, an external `kill` — is recorded as
  `128 + signal`, as a shell would report it, so `-1` keeps meaning exactly one
  thing.
- `timed_out: true` appears only on a step madoqua killed, which also carries
  `"exit": -1`.
- `repo` appears only when the log lives outside the repository.
- Runs with no staged Python files are not logged. They would otherwise
  dominate the percentiles of any repo that also commits prose.

## Exit codes

`stats` is an inventory, not a verdict: `0`, or `2` if the run could not
complete. Never `1` — "your checks are slow" is not a finding.
