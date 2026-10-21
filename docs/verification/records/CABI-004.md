# Record: CABI-004

| | |
|---|---|
| Requirements | CABI-004 |
| Method | A |
| Check | Verify checks `no-std-<target>`: `aldis-ffi` |

Verify checks `no-std-<target>`: `aldis-ffi` builds with `--no-default-features --features panic-handler` for thumbv7em, armv7r, and riscv32imac, without `std` or `alloc`, and `cargo xtask ffi-imports <target>` (`xtask/src/binaries.rs`) lists with `llvm-nm` every symbol the static library leaves undefined and fails on any beyond `memcpy`, `memmove`, `memset`, `memcmp`, and `bcmp`: no allocator, thread, or C runtime symbol. First run 2026-10-21: none on all three targets
