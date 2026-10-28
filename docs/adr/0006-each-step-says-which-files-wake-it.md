# 0006 — Each step says which staged files wake it

Status: accepted

Amended by [0007](0007-a-whole-repo-run-from-the-same-list.md): `madoqua check`
matches `on` against every file in the repository instead of the staged ones.

## Decision

A step's table form takes an `on` list of globs. A step runs only if a staged
file matches one of them, and with `pass_files` it is handed exactly the files
that matched. The default is `["*.py", "*.pyi"]`, the bash hook's pathspec, so
a configuration that never mentions `on` runs exactly as it did.

`git.rs` now reports every staged file (`ACMR`, as before) and decides nothing.
The selection is `Step::select` over that list, matched by `pattern.rs`: pure,
and written in the `.gitignore` subset everyone already reads — a glob with no
`/` matches a file name at any depth, one with a `/` is anchored at the root,
`*` and `?` stay in one segment, `**` spans any number. Nothing else is
special.

What "a run" means follows from that:

- A step that matched nothing is not run, not logged and not named in the
  verdict. A run in which no step matched anything is the no-op run: silent,
  exit `0`, no log row.
- Only the files a fixer ran on are re-staged.
- The verdict says `N files`, not `N py files`, and the log's `files` is the
  number of staged files at least one step ran on. The field keeps its name;
  under the defaults its value is what it always was.

## Why

The gate was one pathspec for every step, so a commit that changed only
`uv.lock` never reached a test runner, whatever the test runner was configured
to care about. The sibling that computes its own diff had grown an ignore list
to stop running the whole suite too often, while the commits that most needed
it were the ones madoqua never woke it for.

Two decisions had been fused: *when to call a tool* and *what it runs once
called*. The second belongs to the tool. The first is madoqua's, and the
honest place for it is next to the command it gates rather than in a constant
in the git seam. Running every `pass_files = false` step on every commit would
have handed the decision to the tool instead, at the price of a test runner's
start-up on every prose commit and a log full of no-op rows.

Per-step file lists, rather than one wider list for everyone, because a
formatter handed `uv.lock` fails for an uninteresting reason. Re-staging only
the fixers' files because `git add` on a file only a check read stages
whatever the developer left unstaged in it — harmless while every step ran on
Python and fixers ran on all of it, a silent widening of the commit once they
do not.

The matcher is ours rather than `globset` for the reason the date code is
(ADR 0003): it is well under a hundred lines with its tests beside it, and
every dependency is one more thing a hook makes people install.

## What it costs

The ignore list in the tool and the `on` list in madoqua overlap, and nothing
keeps them consistent. `on` is meant to be broad — "could this change test
results" — and the tool's list precise; a repo that makes `on` precise has two
lists to maintain.

`MADOQUA_SKIP` still filters checks only, and `on` cannot be set from the
command line. A one-off "run gerenuk regardless" is not expressible except by
staging a file it matches.

The verdict's wording changed. Anything that parsed `py files` out of it — it
was never documented as a format — breaks.

## What would make us revisit it

A tool that needs to know *which* of its trigger files changed while still
not wanting them as arguments, which would argue for passing the matched list
some other way (an environment variable, a file). Or a second kind of trigger
that is not a staged path at all — a branch, a time since the last full run —
which would make `on` the wrong name for the knob.
