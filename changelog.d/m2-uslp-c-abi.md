### Added
- `aldis-ffi`: the C ABI of the USLP Transfer Frame codec `aldis-uslp` (M2 plan WP-5 module 9,
  D4, D12, D14; CABI-010…012, CABI-033): `aldis_uslp_config_check` and
  `aldis_uslp_vc_table_check`; `aldis_uslp_peek_length`; the decoder `aldis_uslp_decode`, whose
  `aldis_uslp_frame_t` carries a full or a truncated frame with a `truncated` tag;
  `aldis_uslp_validate`, which writes a `uint32_t` reason code `ALDIS_USLP_REASON_*`; the one-shot
  `aldis_uslp_encode`, which takes the TFDF Header, the Insert Zone, the TFDZ, and the OCF in an
  `aldis_uslp_content_t`; the two-phase writer `aldis_uslp_writer_begin` and
  `aldis_uslp_writer_finish`; `aldis_uslp_encode_truncated`; `aldis_uslp_encode_oid`, which takes
  an `aldis_oid_generator_t` of either form; and the stateless delimiter
  `aldis_uslp_next_frame`, which takes the whole data unit and an offset. A channel table is an
  `aldis_uslp_config_t` with arrays of `aldis_uslp_vc_config_t` and `aldis_uslp_map_config_t`,
  each naming its SCID, built on every call into a table of the new `capacities.toml` section
  `[uslp]` (`ALDIS_USLP_MAX_VCS`, `ALDIS_USLP_MAX_MAPS`), which `aldis_limits_t` reports. The UPID
  crosses as a 5-bit `uint8_t`, assigned or not, with an `ALDIS_USLP_UPID_*` constant for each
  value of the SANA snapshot. The header `include/aldis/aldis.h` is regenerated.
- `aldis-uslp`: `UslpFrameWriter::resume`, which takes up a frame begun with the same arguments
  without zero-filling its Security Header and Trailer, for writers that keep no state between
  calls, such as `aldis_uslp_writer_finish` (API-023); and `encode_oid_galois`, an OID frame from
  the Galois form of the generator (CMN-022).
- Every live case of `test-vectors/uslp/` runs through the C ABI (`uslp` joins `FFI_AREAS` of
  `cargo xtask consistency`); `crates/aldis-ffi/tests/uslp.rs` checks the channel arrays and
  capacities, the tagged view, the stateless writers, the validation reasons, the delimiter, and
  the OID generator handle; the exerciser's `aldis-ffi` group, the fuzz target `ffi_decode`, and
  the C smoke test cover the new functions.
