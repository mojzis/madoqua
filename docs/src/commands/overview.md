# Commands

| Command | What it does |
|---|---|
| [`run`](run.md) | The hook: guard, fix, stage, check, one line. Also what a bare `madoqua` does. |
| [`install`](install.md) | Write `hooks/pre-commit` and point git at it. |
| [`stats`](stats.md) | Summarise the timing log. |
| [`guide`](guide.md) | Print setup, triage or tune instructions for this repository. |

## Global flags

| Flag | Meaning |
|---|---|
| `--root <PATH>` | Where to start looking for the repository. Defaults to the current directory. |
| `--verbose`, `-v` | Raise the default log level to `debug`. `RUST_LOG` still wins. |
