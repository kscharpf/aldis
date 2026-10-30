### Removed
- The decision log, `docs/process/decisions.md`, and `cargo xtask decisions` (VER-062 keeps the decision rights and no longer asks for a log line). The history stays in git: `git log -- docs/process/decisions.md`. `cargo xtask metrics` no longer counts the log's entries or treats it as bookkeeping.
