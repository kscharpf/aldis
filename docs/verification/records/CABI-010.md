# Record: CABI-010, CABI-011

| | |
|---|---|
| Requirements | CABI-010, CABI-011 |
| Method | I |
| Check | Review of `crates/aldis-ffi/src/` and the generated header |

Review of `crates/aldis-ffi/src/` and the generated header in M2 WP-0c (2026-10-21): only fixed-width integers, `size_t`, `bool`, and `#[repr(C)]` structs of them cross the ABI, enumerations are integer fields or arguments with `ALDIS_*` constants, and buffers are `(const uint8_t *, size_t)` or `(uint8_t *, size_t, size_t *)`; the handles are opaque struct pointers. Reviewed again for `src/tm.rs` (M2 WP-2 module 7, branch `m2/tm-c-abi`, 2026-10-23): the TM configuration, fields, header, layout, and view structs hold only `uint8_t`, `uint16_t`, `uint32_t`, `size_t`, and `bool` (in output structs only), the data field content and the First Header Pointer class are `uint8_t` members with `ALDIS_TM_*` constants, and every field buffer is a `(const uint8_t *, size_t)` pair. Since the review of #94 (maintainer decision 2026-10-24, c-abi.md §11) the five frame-content buffers of `aldis_tm_encode` are such pairs as members of the input struct `aldis_tm_content_t`, the one struct of the ABI that holds pointers. Reviewed in each PR that changes `aldis-ffi`

TC (M2 WP-3 module 8, branch `m2/tc-c-abi`, 2026-10-24): `src/tc.rs` reviewed. The TC configuration, fields, header, layout, view, delimiter item, Control Command, and CLCW structs hold only `uint8_t`, `uint16_t`, `uint32_t`, `size_t`, and `bool` (in output structs only); the frame type, Sequence Flags, Control Command, and delimiter item kind are `uint8_t` members with `ALDIS_TC_*` constants, and the validation reason a `uint32_t` with `ALDIS_TC_REASON_*`. `aldis_clcw_t` is an input of `aldis_clcw_encode` and an output of `aldis_clcw_decode`, so its flags are `uint8_t` 0 or 1, as an input struct's are. The User Data of `aldis_tc_encode` is a `(const uint8_t *, size_t)` pair in the input struct `aldis_tc_content_t`, the TC content struct of c-abi.md §11; no configuration struct holds a pointer.
