# 0005 — The paths under `.git` come from git

Status: accepted

## Decision

The two files madoqua keeps "in `.git`" — the personal overlay
`hooks.local.toml` and the default timing log `hook-timings.jsonl` — are
resolved by asking git where the repository's metadata is, not by joining
`.git/` onto the working tree:

```
git rev-parse --git-common-dir
```

The *common* directory, deliberately: the one every worktree of a clone
shares. One overlay configures every checkout of a repository, and one log
holds every checkout's runs, so `madoqua stats` in a two-day-old task worktree
reports the repository's history rather than that worktree's.

Everything else stays anchored to the working tree. The staged file list, the
directory the tools run in, the `.venv` the guard insists on and a `log` path
the user configured themselves are all about the files being committed, and in
a linked worktree those are the worktree's, not the clone's.

A log that lands inside either directory is the repository's own log and its
records carry no `repo` field. Worktrees of one clone are one repository; there
is nothing for `stats --repo` to tell apart.

`git.rs` grew one function for this, `common_dir`, because it is a `git`
invocation and ADR 0002 says those live in exactly one place. `config.rs` and
`timelog.rs` take the directory as an argument and remain testable against a
path that no filesystem has to contain.

## Why

In a linked worktree `<root>/.git` is a *file* that names the metadata
directory. Joining a name onto it produces a path through a file, and reading
that fails with `Not a directory` — not with `NotFound`, which is the one error
the config reader forgives. So every run in a linked worktree aborted with
exit `2` before a single check had started, whether or not an overlay existed:

```
madoqua: cannot read /…/wt/.git/hooks.local.toml: Not a directory (os error 20)
```

A guard that a worktree turns off is worse than no guard, because the answer in
the moment is always to disable the hook and commit anyway — which is what
happened, in a repository whose commits are supposed to be gated. Forgiving
`NotADirectory` alongside `NotFound` would have unblocked the commit and
silently dropped the overlay: the checks the developer thought were running
would not have run, and nothing would have said so.

Choosing the common directory over the per-worktree one (`--git-dir`) is the
same argument. An overlay is one developer's settings for one repository. A
copy per worktree would have to be written again for every task branch and
would vanish with `git worktree remove` — the friction the overlay exists to
remove, reintroduced exactly where worktrees are used most.

## What it costs

One more `git` spawn per run, and two more arguments threaded through
`Config::resolve` and `timelog::target`.

Runs from different worktrees of one clone are indistinguishable in the log.
That is the deliberate reading of "one repository", but it does mean `stats`
cannot answer "is this branch's test suite slower than main's".

`guide.rs` still joins `.git/` onto the directory it is pointed at. It is not
allowed to need a repository, or a `git` binary, so it cannot ask; the cost is
that a worktree of a clone configured *only* by a hand-wired `.git/hooks/`
script or an overlay reads as unconfigured and gets setup instructions. Those
instructions are idempotent, which is why this was left alone rather than
turned into a fourth exception to ADR 0002.

## What would make us revisit it

Someone wanting per-worktree settings — a slow check dropped for one task
branch. The answer would be another layer between the overlay and
`MADOQUA_SKIP`, read from `--git-dir`, not a move of this one: the layering
already exists, and the clone-wide overlay is the layer that should not be
per-checkout.

Also: a second consumer of `common_dir` that wants the *worktree's* directory
instead. Two callers wanting two different answers from one function is the
signal that the pair belongs in a `Layout` the seam returns whole.
