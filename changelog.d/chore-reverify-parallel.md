### Changed
- `scripts/reverify`: prepares and dispatches every pull request first, then waits for all verify runs together and prints one summary line per PR; a branch already up to date with `main` reuses a successful or in-progress run of its head instead of dispatching another.
