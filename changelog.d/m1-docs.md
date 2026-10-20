### Added
- `aldis`: feature `spp-time`, which enables `spp`, `time`, and the `time` feature of
  `aldis-spp`, so the Space Packet time secondary header (SPP-008) is reachable through the
  facade without `full` (maintainer decision 2026-10-19). `full` now includes it.
- `cargo xtask trace --scope m1` runs, without `--strict` until the M1 exit, in the `spec`
  check of `cargo xtask verify`, in `cargo xtask ci`, and in the nightly CI (M1 D8, VER-020).

### Changed
- `aldis-time`, `aldis-spp`, `aldis-epp`, `aldis`: every public item's docs cite its CCSDS
  clause and requirement IDs (RS-015); the crate docs list the clauses of each module and
  document the features. `docs/09-open-issues.md` records M1 D4 against OI-06, which stays open.
