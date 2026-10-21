### Changed
- xtask is a workspace of crates: the `cargo xtask` dispatcher (`xtask/cli`), shared helpers
  (`xtask/lib`), the verification harness (`xtask/verify`), and one crate per group of tools
  (`xtask/spec`, `changelog`, `layers`, `binaries`, `features`, `coverage`, `cpp`, `ci`), each
  claiming its commands with a `Tool`. Every `cargo xtask` command and its output are unchanged.
- `cargo xtask verify` no longer runs every check on any change to `xtask/` (VER-056): the xtask
  inputs of each check are derived from the crate graph (`cargo metadata`), so that a change to a
  tool crate runs the checks that build, lint, or test the whole workspace, `spec`, and the checks
  whose scripts run one of its commands (for example `features` for `xtask/features`, and
  `cpp-layers` for `xtask/cpp`), while a change to the harness, the helpers, the dispatcher, or a
  manifest still runs every check. `aldis-testkit` is no longer an input of the C++ checks. Every
  xtask path is an input of every check, as before, when the crate graph cannot be read, or an
  xtask crate has a build script, a procedural macro, a file included at build time, or allows
  `unsafe` code (docs/06-verification.md §7). The record format is unchanged.
