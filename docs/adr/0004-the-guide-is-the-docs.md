# 0004 — The guide ships the docs, and it is a seam

Status: accepted

## Decision

`madoqua guide [setup|triage|tune]` prints agent-facing instructions for the
three moments someone meets the tool: it is not wired up here, a run blocked a
commit, or the reference is needed.

The prose is not in `guide.rs`. Each topic is a page under `docs/src/guide/`,
`include_str!`d into the binary, so the published site and the CLI serve the
same bytes and there is nowhere for a second copy to drift.

`guide.rs` is an eighth seam under ADR 0002. It reads `hooks/pre-commit`,
`.git/hooks/pre-commit`, `pyproject.toml` and `.git/hooks.local.toml` to answer
one question — is madoqua wired into *this* directory — and answers it
tolerantly: unreadable, malformed and absent are all "not configured". It looks
at one directory and never walks up, because the answer is supposed to be about
the repository the reader is standing in, and the guide tells them to stand at
the root.

With no topic, a configured directory gets `triage` and an unconfigured one
gets `setup`. `tune` is never auto-selected. The first line of the output names
the topic and why it was chosen, so a reader that passed no topic can tell
whether to trust what follows.

Four tests hold the pages to their job, and they are the point of the design:

- Every madoqua invocation a guide shows is extracted and fed through the real
  `clap::Command`. A guide that shows a command the CLI rejects is worse than
  no guide.
- Every config key a guide names is checked against the field list serde
  actually accepts, asked of the deserializer rather than written down twice
  (`config::keys`).
- No page exceeds 60 lines.
- Every page ends with exactly one `next: run` line.

Like `stats`, `guide` is an inventory rather than a verdict: `0` or `2`, never
`1` (ADR 0001). It needs no repository at all.

## Why

The audience is an agent, and an agent reads what the tool hands it, not the
website. A hook that can say "here is what to do about me, given where you are
standing" turns three round trips — read the README, guess the config, guess
again — into one command.

Keeping the prose in the docs tree rather than in a string constant is what
makes that affordable to maintain: there is one place to edit, the site gets it
for free, and the compiler notices if a page is deleted.

The 60-line cap and the single `next:` line are not style. They are what stops
the guides from growing into a second manual, which is the failure mode of
every in-tool help text that nobody caps.

## What it costs

An eighth seam, in a crate whose ADR 0002 already said seven was about as many
as the rule survives. `guide.rs` earns it the same way `config.rs` does — it
exists in order to answer one question about the filesystem — but it is the
third exception, and a fourth should be an argument for a `fs.rs`, not another
bullet in that table.

Detection is heuristic. A repository configured entirely through something we
do not look for reads as unconfigured and gets setup instructions, which is
wrong but harmless: the instructions are idempotent.

The extraction the parse check relies on is not a shell. It understands a `|`
pipeline and `<placeholder>` holes and nothing else, so a guide author who
writes `&&`, `;` or a redirect inside a command span gets a test failure that
blames the CLI for rejecting a command the guide never really showed. That is
the right way round — loud rather than silent — but it is a constraint on
writing the pages, and it is written down in the `guide.rs` module doc because
that is where someone hitting it will look.

The guides duplicate, in a compressed form, what `docs/src/configuration.md`
and `docs/src/troubleshooting.md` say at length. That is deliberate — the
audiences differ — but it is two places to update when the behaviour changes,
and only the machine-checkable half (commands, config keys) is guarded by
tests.

## What would make us revisit it

A fourth topic. Three is a set someone can hold in their head and a menu they
will read; four is a manual with a table of contents, and at that point the
right move is to point at the site rather than to grow the binary.

Also: if the extraction tests start needing exceptions — a guide that has to
show a command clap cannot parse, or a key that is not a key — then the guides
have outgrown being checkable, and an unchecked guide is one that will be wrong
within two releases.
