# Architecture decision records

One file per decision, numbered, past tense, short. A record says what was
decided, what it costs, and what would make us revisit it — not how the code
works. That belongs in [`../dev/ARCHITECTURE.md`](../dev/ARCHITECTURE.md).

Records are immutable once merged. Changing your mind means a new record that
supersedes the old one, and a `Superseded by` line added to it.

| # | Decision | Status |
|---|---|---|
| [0001](0001-exit-codes-are-a-contract.md) | `0` / `1` / `2` are three distinct answers | accepted |
