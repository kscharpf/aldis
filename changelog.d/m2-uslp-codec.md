### Added
- `aldis-uslp`: the USLP Transfer Frame codec of 732.1-B-3 (M2 WP-5, modules 1–8; USLP-001…USLP-010,
  USLP-012…USLP-016): `Upid`, typed by a table generated from the SANA registry `uslp_protocol_id`;
  the managed parameters the codec reads (`PhysicalChannelConfig`, `VcConfig` with `VcParams`,
  `MapConfig`) behind the `VcLookup` trait and its fixed-capacity `VcTable`; the zero-copy
  `UslpFrame` decoder, full or truncated (`UslpFullFrame`, `UslpTruncatedFrame`), and
  `peek_length`; the TFDF Header (`TfdfHeader`, `TfdzRule` with its First Header or Last Valid
  Octet Pointer); the one-shot `encode`, the two-phase `UslpFrameWriter`, `encode_truncated`, and
  `encode_oid`; Frame Delimiting and Fill Data Removal (`next_frame`, `FrameDelimiter`); and the
  USLP Frame Validation Checks (`validate_frame`) with stable reason codes
  (`docs/standards/uslp-frame.md` §3.1). Lenient-mode flags `UslpAnomaly` with bit indices.
- `cargo xtask sana`: SANA registry snapshots in `specs/sana/` with URL, retrieval date, and
  SHA-256 (`import`), the offline check of the snapshot and its generated table (part of
  `cargo xtask ci`), and the nightly comparison with the live registry (`check-live`, job
  `sana-registries`; M2 plan D13).
- Field codes 89–99 for the USLP fields (`docs/03-api-model.md` §9.1); `spacecraft_id`, `vc_id`,
  `map_id`, `frame_data_field`, `user_len`, `frame_version`, `frame_len`, `ocf_flag`, and
  `max_frame_len` are shared with USLP.
- Vectors `test-vectors/uslp/` (13 files, 408 cases) from the independent model
  `test-vectors/models/uslp_frame.py`, with worked derivations in
  `docs/verification/derivations/uslp-frame.md`.
- `aldis-testkit`: USLP strategies (feature `strategies-uslp`); `aldis-uslp` property tests and a
  USLP-013 test that decodes COP-1 control commands with `aldis-tc` (a dev-dependency).
- Fuzz targets `uslp_decode`, `uslp_truncated`, and `uslp_roundtrip`; the `aldis-uslp` exerciser
  group; `aldis-uslp` in the coverage gate, the `features` check, and the nightly mutants job.
- OI-44 (open, recommendations implemented): the OID header checked on reception with the new flag
  `OidHeaderMismatch`, at least one TFDZ octet, and the delimiter's valid header.

### Fixed
- `cargo xtask fields` writes the `aldis-ffi` field assertions as rustfmt lays out a comparison
  too long for one line.
