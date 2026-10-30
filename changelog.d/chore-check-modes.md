### Added
- `cargo xtask check` has three tiers (VER-068). `--quick` formats and runs `cargo check` in the
  four workspaces and lists every error. The default tier is as before, without coverage.
  `--full` adds coverage, `stack`, and `big-endian`. The steps beyond the base ones run when the
  verification run would run the check they mirror for the change (the rule of VER-056, from the
  checks' declared inputs, not a list of directories). The last line names the tier and the gate
  checks it did not run: `check: passed (full tier; …)` only for `--full`, `check: quick tier ok
  (not a gate pass; …)` and `check: default tier ok (…)` otherwise.
- `cargo xtask coverage -p <crate>` measures only the crates named, and a coverage failure lists
  the missed lines and branches as `path:line` ranges (VER-070).

### Changed
- `cargo xtask check` runs every step, and the tools the xtask steps spawn, under the gate's
  `RUSTFLAGS` and `RUSTDOCFLAGS` (`-D warnings`), taken from the gate's `CHECK_ENV`, so a warning
  the gate rejects fails locally. The first run after this change rebuilds, as cargo's cache key
  includes the flags.
- A test fails when a check of `cargo xtask verify list` is neither covered by a step of
  `check --full` nor named in `GATE_ONLY` with a reason, when a command line of a covered check
  has no step, or when a variable of the gate's `CHECK_ENV` is neither set by `check` nor excused.
- `AGENTS.md` and `docs/process/agent-brief.md`: `--quick` while editing, the default tier before
  a commit, `--full` once before the final commit and after merging `main`.
