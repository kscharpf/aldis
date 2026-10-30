### Added
- `cargo xtask check` takes a lock per worktree and refuses a second run while one runs, naming
  the holder; a lock left by a killed run is taken over. `cargo xtask check --from <step>` skips
  the steps before the named one and ends `partial, not a pass`.

### Changed
- `docs/process/agent-brief.md` item 2: long commands start with `run_in_background` and are
  awaited by their completion notification, not by hand-written poll loops.
- `scripts/reverify --cancel <n>` cancels the pull request's own in-flight hosted verify run of a
  head that is behind `main` and verifies the up-to-date head (VER-060); a cancelled run is
  reported `cancelled`, neither FAILED nor STALE. Nothing cancels without the flag.
