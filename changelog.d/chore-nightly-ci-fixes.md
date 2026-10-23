### Fixed
- `cargo xtask header c-smoke` reads the static library's native dependencies with cargo's colour
  off, so `CARGO_TERM_COLOR=always` no longer leaves an escape sequence in the link line (#86).
- The nightly big-endian job no longer runs the `xtask` tests in the `cross` image, whose git is
  older than `git merge-tree --write-tree` needs; the verify check `big-endian` still runs them (#86).
