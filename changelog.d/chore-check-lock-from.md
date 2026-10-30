### Added
- `cargo xtask check` takes a lock per worktree and refuses a second run while one runs, naming
  the holder; a lock left by a killed run is taken over. `cargo xtask check --from <step>` skips
  the steps before the named one and ends `partial, not a pass`.

### Changed
- `docs/process/agent-brief.md` item 2: long commands start with `run_in_background` and are
  awaited by their completion notification, not by hand-written poll loops.
