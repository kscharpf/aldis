### Added
- `cargo xtask stack` and the gating verify check `stack` (VER-011, GEN-022): the worst-case stack of
  every entry point of the library, Rust and C ABI, on thumbv7em, armv7r, and riscv32imac, from the
  frames and relocations of the `aldis-ffi` static library and checked complete against its LLVM IR
  (no indirect call or recursion that an exemption does not bound), within the budgets of
  `xtask/stack-budgets.toml`; the report `target/spec/stack-<target>.md` is the VER-011 table.
- `cargo xtask loops`, a step of `cargo xtask ci` and the verify check `spec` (GEN-022): every loop
  of the library code is named, with its bound, in its crate's GEN-022/023 inspection record, and
  every loop a record names exists; `cargo xtask loops list` prints the loops found.
- `aldis-core`: the Kani harness `error_kind_lookup_is_bounded`, the GEN-022 bound of
  `ErrorKind::from_code` for every code.

### Changed
- The Loops tables of `docs/verification/inspections/` gain an `ID` column that names each loop;
  the `aldis-ffi` table gains the loops of `lib.rs`, `spp_state.rs`, and `time.rs`.
- The nightly job `stack-sizes` runs `cargo xtask stack <target>`; `cargo xtask stack-sizes`, which
  reported the exerciser's frames, is removed.
- `cargo mutants` leaves out the Kani proof modules (`.cargo/mutants.toml`), and the nightly job
  `mutants` logs disk and memory before, during, and after the run and keeps the report on failure
  (issue #96).
