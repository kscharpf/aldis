### Added
- `aldis-ffi`: the C ABI of the TM Transfer Frame codec `aldis-tm` (M2 plan WP-2 module 7, D4,
  D14; CABI-010…012, CABI-033): `aldis_tm_config_check` and `aldis_tm_mc_config_check`;
  `aldis_tm_peek_length` and `aldis_tm_header_decode`; the decoders `aldis_tm_decode`,
  `aldis_tm_decode_with`, `aldis_tm_decode_unchecked`, and `aldis_tm_decode_unchecked_with`;
  `aldis_tm_validate`; the one-shot `aldis_tm_encode`, which takes the frame's variable
  fields in an `aldis_tm_content_t`; the
  two-phase writer `aldis_tm_writer_begin` and `aldis_tm_writer_finish`; and the OID frame
  writer `aldis_tm_oid_writer_begin` and `aldis_tm_oid_writer_finish`, which take an
  `aldis_oid_generator_t`. A physical channel is an `aldis_tm_config_t`; a master channel an
  `aldis_tm_mc_config_t`, holding its physical channel, with an array of
  `aldis_tm_vc_config_t` and its count, whose stride is the structs' `struct_size`. The `_with`
  decoders take each virtual channel's security layout from the master channel, and so does
  `aldis_tm_validate`; the plain decoders take the physical channel alone. Views and
  writer layouts give every field as an offset and a length (`aldis_tm_layout_t`). The header
  `include/aldis/aldis.h` is regenerated.
- `aldis-tm`: `TmFrameWriter::resume`, which takes up a frame begun with the same arguments
  without zero-filling it, for writers that keep no state between calls, such as the C ABI's
  `aldis_tm_writer_finish` (API-023).
- Every live case of `test-vectors/tm/` runs through the C ABI (`tm` joins `FFI_AREAS` of
  `cargo xtask consistency`); `crates/aldis-ffi/tests/tm.rs` checks the channel arrays, the
  security layout variants, and the stateless writers; the exerciser's `aldis-ffi` group, the
  C smoke test, and the fuzz target `ffi_decode` call the new functions.

### Changed
- `test-vectors/tm/security_layout.json`: the `validate` operation decodes the frame with the
  master channel's security layouts before checking it (c-abi.md §11), in the model and in the
  Rust, exerciser, and C ABI runners; `tm-sec-val-001`, which validated a frame decoded
  without SDLS, is superseded by `tm-sec-val-002`.
