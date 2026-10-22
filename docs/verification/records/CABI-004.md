# Record: CABI-004

| | |
|---|---|
| Requirements | CABI-004 |
| Method | A |
| Check | Verify checks `no-std-<target>`: `aldis-ffi` |

Verify checks `no-std-<target>`: `aldis-ffi` builds with `--no-default-features --features panic-handler` for thumbv7em, armv7r, and riscv32imac, without `std` or `alloc`, and `cargo xtask ffi-imports <target>` (`xtask/src/binaries.rs`) lists with `llvm-nm` every symbol the static library leaves undefined and fails on any beyond `memcpy`, `memmove`, `memset`, `memcmp`, and `bcmp`: no allocator, thread, or C runtime symbol. First run 2026-10-21: none on all three targets

**Deviation on `sparc-unknown-none-elf`** (maintainer decision 2026-10-21, #81, OI-08). The non-gating check `rust-sparc-leon3` runs `cargo xtask ffi-imports` report-only: the `compiler_builtins` that `-Z build-std` puts into `libaldis_ffi.a` refers to the SPARC ABI quad-float routines `_Q_add`, `_Q_sub`, `_Q_mul`, `_Q_div`, `_Q_cmp`, `_Q_feq`, `_Q_fgt`, and `_Q_flt`, which nothing in the archive defines. No Aldis code calls them, and a link with `--gc-sections` drops them, as the exerciser's link shows. So on this target the library needs, beyond the `mem*` functions, either a link with `--gc-sections` or the `_Q_*` routines from the integrator; the [aldis-ffi README](../../../crates/aldis-ffi/README.md) says so. Rust on LEON is best-effort in v1 (OI-08), and the deviation stands until the target or `compiler_builtins` changes.
