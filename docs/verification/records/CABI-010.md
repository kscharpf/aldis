# Record: CABI-010, CABI-011

| | |
|---|---|
| Requirements | CABI-010, CABI-011 |
| Method | I |
| Check | Review of `crates/aldis-ffi/src/` and the generated header |

Review of `crates/aldis-ffi/src/` and the generated header in M2 WP-0c (2026-10-21): only fixed-width integers, `size_t`, `bool`, and `#[repr(C)]` structs of them cross the ABI, enumerations are integer fields or arguments with `ALDIS_*` constants, and buffers are `(const uint8_t *, size_t)` or `(uint8_t *, size_t, size_t *)`; the handles are opaque struct pointers. Reviewed in each PR that changes `aldis-ffi`
