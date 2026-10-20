### Added
- Exerciser (M1 WP-5, D6, VER-010): runs under QEMU with semihosting on all three bare-metal
  targets, not only Cortex-M: Cortex-R5F on QEMU's `none` machine (`-cpu cortex-r5f`; the
  Cortex-R52 board `mps3-an536` needs QEMU 9.0, newer than Ubuntu 24.04's 8.2.2) and RV32 on
  `virt`. Start-up code, fault vectors that exit with failure, and a one-line report per run
  (`exerciser/src/target.rs`, `link-cortex-r.x`, `link-riscv.x`). The verify checks
  `exerciser-<target>` and the nightly `exerciser` job run all three, with one retry.
- Exerciser (M1 WP-5, D7): `build.rs` embeds every live case of `test-vectors/{time,spp,epp}/`,
  and a handler per crate compares every output exactly, as the host suites do (TV-012).

### Fixed
- Exerciser: public items of `aldis-time` and `aldis-epp` that were never called are now
  called (the `From` and `Default` impls of the bounded time fields, `Picos::get`, the
  per-format `pfield()`, `From<ReservedPField> for PField`, `EpiSet::from_bits`,
  `ExtendedEpiSet::from_bits`, `From<UserDefined> for u8`), so VER-010's link-time proof covers
  them.
