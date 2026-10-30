### Changed
- `aldis-ffi`: a fallible C function returns a `uint32_t`: `ALDIS_OK` (0) or the catalogue code of the error, the value of `ErrorKind::code()` (OI-57 convention 3; CABI-020). The `ALDIS_ERR_*` constants keep their names and are positive (`ALDIS_ERR_BUFFER_TOO_SMALL` is 1, not −1); they are the enumerators of the mirror enum `aldis_status`, generated from the error catalogue of `docs/03-api-model.md` §9 by the new `cargo xtask status [--write]` (run by `cargo xtask fix` and `ci`), with a compile-time assertion that each equals `ErrorKind::<Kind>.code()`; a test checks that every `ErrorKind::ALL` entry has its constant. `aldis_error_detail_t::kind` equals the returned status. `aldis_tc_stream_item_t::error_code` and `aldis_uslp_stream_item_t::error_code` are `uint32_t`. C ABI version 0.5.0 (CABI-060).
- `test-vectors/core/errors.json`: the `c_abi` expectation recorded the negated code; each case is superseded by an `-002` case with `status` (TV-021).
- `docs/`: `docs/profiles/c-abi.md` (CABI-010, CABI-020, §10, §12) describes the status codes.

### Added
- `xtask`: `cargo xtask status [--write]`, which checks the error catalogue and generates `crates/aldis-ffi/src/status.rs` from it.

### Removed
- `xtask`: the post-processing that put negative `#define` values of the header in parentheses; the header has none.
