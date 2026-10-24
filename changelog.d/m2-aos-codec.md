### Added
- `aldis-aos`: the AOS Transfer Frame codec of 732.0-B-5 EC1, in Issue 5 and in the wire format of
  732.0-B-4 and earlier (M2 WP-4, modules 2–7; AOS-001…AOS-003, AOS-010…AOS-019, GEN-062): the
  edition `AosEdition` (default `Issue5`); the managed parameters `AosPhysicalChannelConfig` and
  `AosVirtualChannelConfig` behind the `VcLookup` trait and its fixed-capacity `VcTable`, rejecting
  VCID 63 for a data VC, an SCID above 255 in Issue 4, and the B_PDU limits of AOS-018;
  `AosPrimaryHeader::decode`, which corrects the header with the FHEC before any field is read; the
  zero-copy `AosFrame` decoder, with the FECF checked over the corrected header, the Insert Zone,
  the SDLS slices, the OCF, and the data field as a `DataUnit`; `Mpdu` and `Bpdu` with their
  pointers classified (`PacketPointer`, `BitstreamPointer`); the one-shot `encode`, the two-phase
  `AosFrameWriter`, and OID frames on VCID 63 (`begin_oid`). Lenient-mode flags `AosAnomaly` with
  bit indices.
- Field codes 89–97 for the AOS fields and parameters (`docs/03-api-model.md` §9.1); the Spacecraft
  ID, VCID, frame data field, security lengths, Transfer Frame Version Number, frame length, and
  First Header Pointer codes are shared with `aldis-aos`.
- Vectors `test-vectors/aos/` (11 frame files) from the independent model
  `test-vectors/models/aos_frame.py`, which uses the FHEC model `aos_fhec.py`, with a worked
  derivation in `docs/verification/derivations/aos-frame.md`.
- `aldis-testkit`: AOS strategies (feature `strategies-aos`); `aldis-aos` property tests.
- Fuzz targets `aos_decode` and `aos_roundtrip`; the `aldis-aos` exerciser group covers the frame
  codec and every `aos/` vector.
- OI-43 (open): whether OID frames on VCID 63 may carry an OCF or SDLS fields; the codec gives them
  neither, as recommended.

### Changed
- AOS-002, AOS-003, AOS-012, AOS-017: the error kinds and fields of the strict checks are stated,
  and a Bitstream Data Pointer naming the zone's last bit is `InvalidField` in both modes
  (732.0 §4.1.4.3.3.3).
