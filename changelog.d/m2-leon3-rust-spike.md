### Added
- Verification: non-gating check `rust-sparc-leon3` (OI-08). It builds the facade and the C ABI
  library for `sparc-unknown-none-elf` with `-C target-cpu=leon3` on the tooling nightly
  (`-Z build-std=core`), runs the exerciser's panic-freedom link check and canary (VER-010), and
  runs every vector case under QEMU `leon3_generic`. The exerciser gains LEON3 start-up code, a
  trap table with window overflow and underflow handlers, an APBUART console, and an error-mode
  exit (`exerciser/src/target.rs`, `exerciser/link-leon3.x`).
- Verification image: Clippy on the tooling nightly, and Ubuntu's `binutils-sparc64-linux-gnu`
  (GNU ld 2.42) and `qemu-system-sparc` (QEMU 8.2.2), with their upstream versions asserted.

### Changed
- `docs/06-verification.md` §4: the LEON3 row now runs the vectors under QEMU, non-gating.
  `docs/09-open-issues.md` OI-08 records the spike's findings, among them that the static
  library does not meet CABI-004 on SPARC (`compiler_builtins` refers to the `_Q_*` quad-float
  routines). OI-08 is resolved (maintainer, 2026-10-21): Rust on LEON stays best-effort in v1, with
  the native C++ implementation as the primary LEON path, and CABI-004 on SPARC is a recorded
  deviation (`docs/verification/records/CABI-004.md`) with an integrator note in
  `crates/aldis-ffi/README.md`: link with `--gc-sections`, or supply the `_Q_*` routines.
