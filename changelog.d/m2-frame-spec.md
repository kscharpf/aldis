### Added
- Specification: USLP-016, the TFDZ Construction Rule must match the VC's frame type
  (`000`–`010` fixed-length, `011`–`111` variable-length), `InvalidField` in both modes
  (732.1 §4.1.4.2.2.1.1, Table 4-3; OI-35 item 3).
- Specification: bit indices for the `TmAnomaly`, `AosAnomaly`, and `UslpAnomaly` flags
  (`docs/standards/tm-frame.md`, `aos-frame.md`, `uslp-frame.md`).
- `specs/`: CCSDS 700.1-G-1, the USLP Green Book, as a `ref` document (OI-05).

### Changed
- Specification: the frame anomaly lists and API-032 follow M2 D9 (OI-33). `FhpBeyondDataField`,
  `PointerBeyondZone`, `FhecCorrected`, `VcfLengthMismatchForQos`, and `UnknownUpid` are removed:
  out-of-range pointers are `Malformed` and a VCF Count Length mismatch is `LengthMismatch` in both
  modes (TM-008, AOS-017, USLP-004), the FHEC reports `corrected_symbols()` (AOS-014), and unknown
  UPIDs decode as `Upid::Other(n)` (USLP-007). API-032 names the TM Segment Length Identifier.
- Specification: AOS-013 and AOS-014 follow the wording of 732.0-B-5 §4.1.2.6.5: a (15,11) code
  shortened to a binary (40,24) code. AOS-014 states that three or more symbol errors can be
  miscorrected, and that the FECF is checked over the corrected header. AOS-018 bounds the B_PDU zone
  at 2 047 octets, and in Issue 5 the frame at 2 048 octets (OI-35 items 5–7).
- Specification: USLP-002 and the USLP configuration allow truncated frames only on
  variable-length VCs (Annex D1.2.1; OI-35 item 2). USLP-005 is no longer an interpretation: OI-05 is
  resolved from 732.1 §4.1.4.2 and confirmed by 700.1-G-1 and Yamcs 5.13.7.
- Open issues: OI-35 items 1 and 8 (the truncated frame's rule and UPID, and how truncated frames
  are delimited) have proposed answers for the maintainer.
