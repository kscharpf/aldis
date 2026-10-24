### Added
- `aldis-ffi`: the C ABI of the TC Space Data Link Protocol codec `aldis-tc` and the CLCW (M2
  plan WP-3 module 8, D4, D14; CABI-010…012, CABI-033): `aldis_tc_config_check` and
  `aldis_tc_mc_config_check`; `aldis_tc_peek_length` and `aldis_tc_header_decode`; the decoder
  `aldis_tc_decode`; the Frame Validation Checks `aldis_tc_validate`, which returns the reason
  of a rejection as an `ALDIS_TC_REASON_*` code; the one-shot `aldis_tc_encode`, which takes
  the User Data or Control Command in an `aldis_tc_content_t`; the two-phase writer
  `aldis_tc_writer_begin` and `aldis_tc_writer_finish`; the Frame Delimiting and Fill Removal
  Procedure as the stateless `aldis_tc_next_frame`, which takes the data unit and an offset;
  the Control Commands `aldis_tc_control_decode` and `aldis_tc_control_encode`; and the CLCW as
  a 32-bit word, `aldis_clcw_decode` and `aldis_clcw_encode`, with the POD `aldis_clcw_t`. A
  master channel is an `aldis_tc_mc_config_t`, holding its physical channel and Spacecraft ID,
  with an array of `aldis_tc_vc_config_t` and an array of `aldis_tc_map_config_t`, each with
  its count and its own stride; every call that takes it uses the security layouts of the
  frame's VC or MAP. Views and writer layouts give every field as an offset and a length
  (`aldis_tc_layout_t`). The header `include/aldis/aldis.h` is regenerated.
- `aldis-ffi`: the build-time capacity `[tc] max_maps` of `capacities.toml`, published as
  `ALDIS_TC_MAX_MAPS` and reported by `aldis_limits` as `tc_max_maps` (CABI-030).
- `aldis-tc`: `TcFrame::peek_header`, the Primary Header of a frame from its first five octets
  without a channel lookup, for example to find the Spacecraft ID of a frame on a physical
  channel that carries several master channels (API-014); and `TcFrameWriter::resume`, which
  takes up a frame begun with the same arguments without writing to it, for writers that keep
  no state between calls, such as the C ABI's `aldis_tc_writer_finish` (API-023).
- Every live case of `test-vectors/tc/` runs through the C ABI (`tc` joins `FFI_AREAS` of
  `cargo xtask consistency`); `crates/aldis-ffi/tests/tc.rs` checks the channel and MAP
  arrays, the security layouts of a VC or MAP, the stateless writers and delimiter, the
  validation reasons, the Control Commands, and the CLCW; the exerciser's `aldis-ffi` group,
  the C smoke test, and the fuzz target `ffi_decode` call the new functions.
