### Added
- `aldis-ffi`: the C ABI of the parts of `aldis-core` the frame codecs share (M2 plan WP-1,
  D14): `aldis_crc16`, the CRC-16 of a buffer (CMN-001); `aldis_ocf_classify`, the report four
  Operational Control Field octets hold (`ALDIS_OCF_CLCW`, `_PROJECT_SPECIFIC`, `_SDLS_FSR`),
  and `aldis_ocf_check`, which checks them against a report and names the Type Flag or bit 1
  when they differ (CMN-010, CMN-011); and the OID generator `aldis_oid_generator_t` in
  caller-owned storage, in the Fibonacci or the Galois form (`ALDIS_OID_FIBONACCI`,
  `ALDIS_OID_GALOIS`), with `_storage_size`, `_storage_align`, `_init`, `_fill`, and `_deinit`
  and the storage checks and handle tags of the SPP helpers (CMN-020…023; D5, CABI-031,
  CABI-032). The header `include/aldis/aldis.h` is regenerated.
- The C ABI vector runner runs `test-vectors/core/{crc16,ocf,oid}.json`; the exerciser's
  `aldis-ffi` group, the C smoke test, and the fuzz target `ffi_decode` call the new functions.
- `docs/verification/inspections/gen-022-023-aldis-ffi.md`: the GEN-022/023 inspection of the
  new `aldis-ffi` modules and the shared checks they call, pending exit review (VER-061).

### Changed
- `cargo xtask consistency`: the C ABI runner must run the `core/` files of the `aldis-core`
  modules that have a C ABI (`FFI_FILES`), and no longer requires the rest of `core/`, whose
  modules have none.
