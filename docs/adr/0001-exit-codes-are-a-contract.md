# 0001 — `0` / `1` / `2` are three distinct answers

Status: accepted

## Decision

madoqua's exit codes mean exactly three things, and never collapse:

- `0` — the command ran and found nothing to report.
- `1` — the command ran and found something to report.
- `2` — the command could not complete.

`main.rs` maps `Outcome::Clean` to `0`, `Outcome::FindingsReported` to `1`, and
every `Err` to `2`. No command may return `1` for a failure, and none may
return `2` for a finding.

## Why

A CLI that is called from a hook, a Makefile or an agent is read by its exit
code before anything else. If "we looked and found a problem" and "we could not
look" share a code, the caller cannot tell a real finding from a broken
install, and the only safe reading of a non-zero code is to ignore it.

## What it costs

Every new command has to decide, up front, which of its failure modes are
findings and which are breakages — including the awkward middle cases, where a
partial answer is available. A command that is an inventory rather than a
verdict has no `1` at all, and must say so in its docs.

## What would make us revisit it

A caller that genuinely cannot distinguish codes (some CI shells only see
zero/non-zero) and that we care about supporting. That is an argument for a
`--format` flag, not for collapsing the codes.
