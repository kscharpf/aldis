### Added
- `aldis-tm`: the TM Transfer Frame codec (132.0-B-3; TM-001…TM-011; M2 WP-2). Managed
  parameters in `TmPhysicalChannelConfig`, `TmMasterChannelConfig`, and `TmVirtualChannelConfig`,
  whose constructors reject what 132.0 §5 forbids (an MC_FSH with a VC_FSH, an MC_OCF with a
  VC_OCF, a frame without a data octet, a VCID twice). `TmFrame::decode`, `decode_with` (the
  Security Header and Trailer of each VC from a `VcLookup`), `decode_unchecked`, and
  `peek_length`: the FECF checked before any field, the Secondary Header version and length, the
  OCF as an `Ocf`, the First Header Pointer classified as `PacketPointer` (an offset outside the
  data field is `Malformed` in both modes), and the Packet Order Flag and Segment Length
  Identifier checked in strict mode and reported as `TmAnomaly` flags in lenient mode.
  `TmFrame::validate` checks a frame against its master channel. The one-shot `encode`, the
  two-phase `TmFrameWriter`, and `TmFrameWriter::begin_oid` for Only Idle Data frames from an
  `OidGenerator` compute the Secondary Header Identification Field, the OCF Flag, and the FECF.
- Field codes 67–76 for the TM frame fields (`frame_version`, `frame_len`, `ocf_flag`,
  `fsh_flag`, `sync_flag`, `packet_order_flag`, `segment_length_id`, `first_header_pointer`,
  `fsh_version`, `fsh_len`), in `docs/03-api-model.md` §9.1 and the generated files.
- Vectors `test-vectors/tm/{minimal,fsh_ocf_fecf,fhp_values,oid_sequence,security_layout,invalid}.json`,
  computed by the independent model `test-vectors/models/tm_frame.py`, with one decode and one
  encode worked by hand in `docs/verification/derivations/tm-frame.md`; the exerciser runs them
  on the bare-metal targets.
- `aldis-testkit`: `strategies::tm` (feature `strategies-tm`), and the property tests
  `crates/aldis-tm/tests/properties.rs`. Fuzz targets `tm_decode` and `tm_roundtrip` join the
  nightly matrix; `aldis-tm` joins the coverage gate, the features check, and the nightly
  mutation testing, and has a GEN-022/023 inspection record.
