### Added
- `aldis-ffi`: the C ABI of the AOS Transfer Frame codec `aldis-aos` (M2 plan WP-4 module 8,
  D4, D14; CABI-010…012, CABI-033), following the frame codec conventions of
  `docs/profiles/c-abi.md` §11: `aldis_aos_fhec_encode` and `aldis_aos_fhec_decode`;
  `aldis_aos_config_check` and `aldis_aos_mc_config_check`; `aldis_aos_peek_length` and
  `aldis_aos_header_decode`; the frame decoder `aldis_aos_decode`; the data unit decoders
  `aldis_aos_mpdu_decode` and `aldis_aos_bpdu_decode`; the one-shot `aldis_aos_encode`, which
  takes the frame's variable fields in an `aldis_aos_content_t`; the two-phase writer
  `aldis_aos_writer_begin` and `aldis_aos_writer_finish`; and the OID frame writer
  `aldis_aos_oid_writer_begin` on VCID 63, which takes an `aldis_oid_generator_t` and which
  `aldis_aos_writer_finish` finishes. A physical channel is an `aldis_aos_config_t` with its
  edition; a master channel an `aldis_aos_mc_config_t`, one SCID holding its physical
  channel, with an array of `aldis_aos_vc_config_t` and its count. The packet offset of an
  `M_PDU` crosses as a `uint16_t` checked as a `PacketOffset`, and the count of valid bits of
  a `B_PDU` crosses as a `uint16_t` checked as a `ValidBitCount`. The header
  `include/aldis/aldis.h` is regenerated.
- `aldis-aos`: `AosFrameWriter::resume`, which takes up a frame `begin` or `begin_oid` started
  with the same arguments without zero-filling it, for writers that keep no state between
  calls, such as the C ABI's `aldis_aos_writer_finish` (API-023).
- Every live case of `test-vectors/aos/` runs through the C ABI (`aos` joins `FFI_AREAS` of
  `cargo xtask consistency`); `crates/aldis-ffi/tests/aos.rs` checks the channel arrays, the
  layouts, the FHEC and data unit decoders, and the stateless writers; the exerciser's
  `aldis-ffi` group, the C smoke test, and the fuzz target `ffi_decode` call the new
  functions.
