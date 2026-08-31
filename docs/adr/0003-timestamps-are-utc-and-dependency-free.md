# 0003 — Timestamps are UTC, and we compute them ourselves

Status: accepted

## Decision

The timing log's `ts` field is RFC 3339 in UTC with a `Z` suffix, produced by
about thirty lines of civil-date arithmetic in `clock.rs` rather than by a date
crate. `stats` parses it back, and tolerates a numeric offset on read so a log
written by something else still loads.

## Why

The alternative was a dependency. `chrono` or `time` with local-offset support
pulls in a timezone database and, on Unix, the local-offset call is either
unsound in a threaded process or requires care we would rather not owe. madoqua
spawns threads for every check.

UTC also survives the case the log exists for: a `log` path under `$HOME`
collecting runs from several repositories, potentially from more than one
machine. Timestamps that sort are worth more there than timestamps that match
the clock on the wall.

The arithmetic itself is Howard Hinnant's `days_from_civil` / `civil_from_days`,
which is exact, table-free, and short enough to read in one sitting. Its unit
tests assert round-tripping across leap days and a century boundary.

## What it costs

A developer reading `.git/hook-timings.jsonl` by hand sees UTC, not their own
clock, and has to do the offset in their head. `madoqua stats` hides this —
it reports durations and windows, never absolute times — so the cost lands only
on someone reading the raw file.

We also own a date implementation, which is a category of code with a long
history of subtle bugs. The mitigation is that it does exactly two things and
has tests for both.

## What would make us revisit it

Wanting to show absolute times in `stats` output. That is the point at which
"the user must read UTC" stops being invisible, and a real date crate starts
paying for itself.
