### Added
- `aldis-tc`: the TC Transfer Frame codec of 232.0-B-4 Cor. 1 (M2 WP-3, modules 1–7; TC-001…TC-008,
  TC-020…TC-023, TC-030, TC-031): the managed parameters the codec reads (`PhysicalChannelConfig`,
  `VcConfig`, `MapConfig`) behind the `VcLookup` trait and its fixed-capacity `VcTable`; the
  zero-copy `TcFrame` decoder and `peek_length`; the one-shot `encode` and the two-phase
  `TcFrameWriter`, with SDLS fields per VC or per MAP; the Control Commands Unlock and Set V(R);
  the CLCW (`Clcw`); Frame Delimiting and Fill Removal as the stateless `next_frame` and the
  `FrameDelimiter` iterator; and the Frame Validation Checks (`validate_frame`) with stable reason
  codes (`docs/standards/tc-frame.md` §4.1). Lenient-mode flags `TcAnomaly` with bit indices.
- Field codes 67–80 for the TC frame and CLCW fields (`docs/03-api-model.md` §9.1); `user_len`
  (54) is shared with the TC two-phase writer.
- Vectors `test-vectors/tc/` (11 files) from the independent model `test-vectors/models/tc_frame.py`,
  with a worked derivation in `docs/verification/derivations/tc-frame.md`.
- `aldis-testkit`: TC strategies (feature `strategies-tc`); `aldis-tc` property tests.
- Fuzz targets `tc_decode`, `tc_roundtrip`, `tc_delimit`, and `tc_clcw`; the `aldis-tc` exerciser
  group; `aldis-tc` in the coverage gate, the `features` check, and the nightly mutants job.
- OI-41 (resolved): empty User Data in a TC frame is rejected, and the delimiter takes as a valid header at least five octets with TFVN `00` and a Frame Length of at least six.
