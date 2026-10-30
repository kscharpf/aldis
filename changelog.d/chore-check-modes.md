### Added
- `cargo xtask check` has three tiers (VER-068). `--quick` formats and runs `cargo check` in the
  four workspaces and lists every error. The default tier is as before, without coverage.
  `--full` adds coverage of the touched crates, `stack`, and `big-endian`. The last line of a
  passing run names the tier and the gate checks it did not run.
- `cargo xtask coverage -p <crate>` measures only the crates named, and a coverage failure lists
  the missed lines and branches as `path:line` ranges (VER-070).

### Changed
- `cargo xtask check` runs its steps under the gate's `RUSTFLAGS` and `RUSTDOCFLAGS`
  (`-D warnings`), so a warning the gate rejects fails locally. The first run after this change
  rebuilds, as cargo's cache key includes the flags.
- A test fails when a check of `cargo xtask verify list` is neither covered by a step of
  `check --full` nor named in `GATE_ONLY` with a reason.
- `AGENTS.md` and `docs/process/agent-brief.md`: `--quick` while editing, the default tier before
  a commit, `--full` once before the final commit.
