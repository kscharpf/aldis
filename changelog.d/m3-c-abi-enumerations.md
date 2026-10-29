### Changed
- `aldis-ffi`: the C ABI's enumerations are mirror enums (`src/mirror.rs`): one fieldless `#[repr(u8)]` or `#[repr(u32)]` enum per closed set of `ALDIS_*` values, parsed by one generic `parse` into a Rust enum, then converted to the core type by an exhaustive `match` (OI-57 convention 2; CABI-010, CABI-022). The hand-written constant and `match` pairs of the AOS, OCF, OID, SPP, TC, time, TM, and USLP modules and of the decode mode are gone.
- `include/aldis/aldis.h`: the `ALDIS_*` enumeration constants keep their names and values, but are enumerators of an `enum` that no field uses and no longer `#define`s, so `#if ALDIS_X` on one no longer works. C ABI version 0.4.0 (CABI-060).
- `docs/`: `docs/profiles/c-abi.md` (CABI-010, CABI-022, §10, §12) describes the mirror enums.
