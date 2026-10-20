### Changed
- `cargo xtask verify` runs only the checks a change needs (VER-056): each check declares its
  inputs in `xtask/src/verify/checks.rs` and is skipped, as `skipped` with the base in the
  reason, when none of them changed against the merge base with `origin/main` and a published
  passing record covers that base. A documentation-only change (`docs/`, `changelog.d/`, the root
  `*.md`, `specs/README.md`, `specs/manifest.tsv`) runs only `spec`. Every check runs when the
  change is not known, a changed path is an input of no check, no record covers the base, or with
  the new `--all` flag, which the nightly run uses. This replaces the C++-only path rule
  (`CPP_PATHS`); `--all-cpp` still forces the C++ checks.
- Records are now format `aldis-verify-record/2`, with a `scope` object (base, covering record,
  changed paths, each check's inputs, and whether each check ran and why) in place of `cpp`.
  `cargo xtask verify check` and `--publish` re-derive every skip from git and refuse a record
  whose skips do not hold (VER-057, VER-058); format 1 records are still read. `cargo xtask
  verify list` and the new `verify scope [<rev>] [--main <rev>]` show which checks a commit needs.

### Fixed
- `cargo xtask verify` no longer skips the C++ checks when `git diff` fails, and now runs them
  when a file they read outside `cpp/` changes: the image inputs (`rust-toolchain.toml`,
  `tooling-nightly`) and what building xtask reads (`Cargo.toml`, `Cargo.lock`, the crate
  manifests, `aldis-testkit`).
