# Record: CABI-010, CABI-011

| | |
|---|---|
| Requirements | CABI-010, CABI-011 |
| Method | I |
| Check | Review of `crates/aldis-ffi/src/` and the generated header |

Review of `crates/aldis-ffi/src/` and the generated header in M2 WP-0c (2026-10-21): only fixed-width integers, `size_t`, `bool`, and `#[repr(C)]` structs of them cross the ABI, enumerations are integer fields or arguments with `ALDIS_*` constants, and buffers are `(const uint8_t *, size_t)` or `(uint8_t *, size_t, size_t *)`; the handles are opaque struct pointers. Reviewed again for `src/tm.rs` (M2 WP-2 module 7, branch `m2/tm-c-abi`, 2026-10-23): the TM configuration, fields, header, layout, and view structs hold only `uint8_t`, `uint16_t`, `uint32_t`, `size_t`, and `bool` (in output structs only), the data field content and the First Header Pointer class are `uint8_t` members with `ALDIS_TM_*` constants, and every field buffer is a `(const uint8_t *, size_t)` pair. Reviewed in each PR that changes `aldis-ffi`
