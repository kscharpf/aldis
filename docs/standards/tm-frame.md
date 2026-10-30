# TM Transfer Frame — CCSDS 132.0-B-3

**Crate:** `aldis-tm` · **Reference:** `specs/pdf/132x0b3.pdf` ([how to download](../../specs/README.md)) · **Acronyms:** [glossary](../glossary.md)

## 1. Wire format (§4.1)

The field layout is that of the Blue Book clause cited in the heading, and of the rustdoc of the view and writer types; the vectors pin the octets.

Secondary Header: ID octet = version `00` (2 bits) ‖ length − 1 (6 bits), followed by 1–63 data octets (total 2–64, fixed per MC/VC).

## 2. Configuration (managed parameters, §5)

The managed parameters are those of the Blue Book clause cited in the heading; the configuration types carry them, with their ranges, in their rustdoc.

## 3. Requirements

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| TM-001 | The decoder SHALL require the configured fixed frame length and the configured FECF presence (API-012). A physical channel SHALL be configured with a frame length of at most 4 095 octets, else `InvalidConfig` (`frame_len`) with that maximum (OI-51: above it the CRC-16 misses some double-bit errors, VER-064; `hz:HZ-TM-04`). | §4.1.1.2, §4.1.6.1.2 | V |
| TM-002 | The decoder SHALL reject TFVN ≠ `00` with `UnsupportedVersion`. | §4.1.2.2.2.2 | V |
| TM-003 | If the Secondary Header Flag is set, the decoder SHALL read the FSH length from its ID octet. It SHALL reject FSH version ≠ `00`, and an FSH that would overrun the frame. It SHALL reject a Secondary Header Length field of 0 (a Secondary Header with no data octet; the field must be 1 to 63) with `InvalidField` (`fsh_len`). | §4.1.3.1.3 b, §4.1.3.2 | V |
| TM-004 | If the OCF Flag is set, the decoder SHALL expose the 4 OCF octets ([common §2](common-link-features.md#2-operational-control-field)). | §4.1.5 | V |
| TM-005 | If the FECF is configured, the decoder SHALL verify it before returning a view and SHALL fail with `CrcMismatch` on error. A separate `decode_unchecked` SHALL exist for diagnostics, and it SHALL return a distinct `UncheckedTmFrame` that has the read accessors of the frame and no other operation that takes a checked frame; it becomes a `TmFrame` only through `check_fecf`, which fails with `CrcMismatch` on a wrong FECF (API-015). | §4.1.6 | V |
| TM-006 | The data field length SHALL be computed as frame length − 6 − FSH − security header − security trailer − OCF − FECF (CMN-040). | §4.1.4.2, §6.3.5.2 | V |
| TM-007 | With Sync Flag = 0, strict mode SHALL require Packet Order Flag = 0 and Segment Length ID = `11`. Lenient mode SHALL flag deviations. With Sync Flag = 1, these fields SHALL be exposed raw and not validated. | §4.1.2.7.4–5 | V |
| TM-008 | With Sync Flag = 0, the view SHALL classify the FHP as `PacketStartsAt(n)` (n < data field length), `NoPacketStart` (`0x7FF`), or `OnlyIdleData` (`0x7FE`). Any other n ≥ data field length SHALL be `Malformed` in both modes: it points outside the field it indexes (API-032; [open issues](../09-open-issues.md) OI-33). With Sync Flag = 1 the FHP is undefined and SHALL be exposed raw. | §4.1.2.7.6, §4.1.2.7.6.2 note | V |
| TM-009 | The writer SHALL compute the FSH ID octet, the OCF flag (from configuration), and the FECF. It SHALL accept MC/VC frame counts, the sync flag, and the FHP as inputs (counts are owned by engines, [04](../04-engines.md)). | §4.1.2–6 | V |
| TM-010 | An OID writer SHALL set FHP = `0x7FE` and fill the data field from a supplied `OidGenerator` (CMN-020). It SHALL use a VCID that is configured for packets, else `InvalidField` (`sync_flag`); and a VC that does not use SDLS, that is with no Security Header or Trailer, else `InvalidField` (`vc_id`), checked in that order; it writes nothing on error (maintainer decision of 2026-10-25). Every other writer (`encode`, `begin`, and the C ABI's `aldis_tm_encode`) SHALL apply the SDLS check to a frame with FHP = `0x7FE`, since that pointer makes it an OID frame. FSH and OCF MAY carry valid data. The decoder SHALL NOT check the OID data: it depends on the generator's position across frames (CMN-021), which a stateless decoder does not know, and receivers discard it. The decoder SHALL check the SDLS rule (OI-54): with Sync Flag = 0 and FHP = `0x7FE`, on a VC whose security layout has a Security Header or Trailer, `TmFrame::decode_with` and `decode_unchecked` (and the C ABI `_with` decoders) return `InvalidField` naming `vc_id` in strict mode, and in lenient mode accept the frame and set the anomaly `OnlyIdleDataOnSdlsVc` (§4, bit 2). The check is the last of the decoder's, after the FHP classification (`Malformed`), the Packet Order Flag (`ReservedValue`, `packet_order_flag`) and the Segment Length ID (`InvalidField`, `segment_length_id`) checks, so a frame with an earlier fault reports that fault in strict mode and, in lenient mode, carries its flags together. With Sync Flag = 1 the FHP is undefined (TM-008) and nothing is checked. `TmFrame::decode` and `aldis_tm_decode`, which take no channel table and read every VC as one without SDLS (HZ-TM-07), cannot check it and never report it. | §4.1.4.6; §6.4.6.2 | V |
| TM-011 | `TmFrame::validate(self, &MasterChannelConfig)` SHALL return a `ValidatedTmFrame` (API-016) after it has checked SCID and VCID against the configuration. It SHALL check that the OCF Flag and FSH presence and length match the configured MC/VC parameters, and that the Sync Flag matches the VC data field content. It SHALL, as its last check, return `InvalidField` naming `vc_id` for an Only Idle Data frame (FHP `0x7FE`) on a VC that uses SDLS, whatever the mode the frame was decoded in (TM-010, OI-54). | §5; §4.1.2.4.3; §4.1.2.7.3.3 | V |
| TM-012 | A one-shot Only Idle Data encoder (`encode_oid`, with the Galois form `encode_oid_galois`; C ABI `aldis_tm_encode_oid`) SHALL write the frame of TM-010 in one call: FHP = `0x7FE`, the data field filled from the supplied generator, the Secondary Header data field and the OCF given by the caller, the FECF computed. It SHALL make the checks of TM-010 and check the Secondary Header data and OCF as `encode` does (`LengthMismatch` `fsh_len`; `InvalidField` `ocf_flag`). On error it SHALL write nothing and leave the generator unchanged (GEN-042). The two generator forms SHALL give the same frame (CMN-022). | §4.1.4.6 | V |

## 4. Anomaly flags (lenient mode)

`TmAnomaly` flags. The bit index is the flag's position in `AnomalySet::bits()` and is shared with the C ABI. It never changes once `aldis-tm` is released. Before OI-33 the flags were listed without bit indices, so withdrawing `FhpBeyondDataField` (an FHP beyond the data field is `Malformed` in both modes, TM-008) reserves no bit. Version, length, and FECF errors remain errors in both modes (API-032).

| Bit | Flag | Deviation | Source |
|---|---|---|---|
| 0 | `PacketOrderFlagSet` | Packet Order Flag = 1 with Sync Flag = 0 (TM-007) | 132.0 §4.1.2.7.4 note |
| 1 | `SegmentLengthIdNot11` | Segment Length ID ≠ `11` with Sync Flag = 0, a fixed-value legacy field (TM-007, API-032) | 132.0 §4.1.2.7.5.2, notes 1–2 |
| 2 | `OnlyIdleDataOnSdlsVc` | Sync Flag = 0 and FHP = `0x7FE` (Only Idle Data) on a VC that uses SDLS, set only by a decode that takes the VC's security layout (TM-010, OI-54); the C ABI constant is `ALDIS_TM_ANOMALY_ONLY_IDLE_DATA_ON_SDLS_VC` | 132.0 §6.4.6.2 |

## 5. Test-vector categories

The case groups are the directories of `test-vectors/` for this standard; see [test vectors](../07-test-vectors.md).

## 6. Project rules and choices the vectors pin

The Blue Book does not decide these; they are the project's, and the second review ([m2-vectors.md](../verification/reviews/m2-vectors.md)) lists them:

- A frame with no data octet is rejected (CMN-041, whose source is none). The error names `frame_data_field` (7) when a configured field (FSH, OCF, Security fields) leaves none, and `frame_len` (68) for a physical channel too short for its own header and FECF when no VC is configured (`tm-inv-cfg-001`).
- The FECF is checked before the version (TM-005), so a frame with both faults is `CrcMismatch` (`tm-inv-crc-002`); `tm-inv-ver-001` and `-002` carry valid FECFs.
- Sync = 0 with Packet Order Flag = 1 is `ReservedValue` in strict mode (TM-007): the Blue Book gives the rule in a NOTE (§4.1.2.7.4). The error kinds and fields for a VC whose content does not fit the writer's request (TM-009, TM-010) are `InvalidField` naming `sync_flag`.
- Frames longer than 2048 octets are accepted up to 4 095 octets: 132.0-B-3 states no maximum (131.0-B-6 §11 gives 2048 because of the 11-bit FHP), so that the FHP offsets 2045 and 2046 are testable (`tm-fhp-enc-005`, `tm-fhp-enc-err-002`). Above 4 095 octets the channel is refused (TM-001, OI-51), since the CRC-16 no longer detects every double-bit error there (VER-064).
- An Only Idle Data frame on a VC that uses SDLS (OI-54, TM-010, TM-011). The test is on the decoded frame and the VC's security layout, in this order of the decoder: Sync Flag = 0 and FHP = `0x7FE` (Only Idle Data), and the VC's Security Header length or Security Trailer length is not 0. It is the last check of `decode_with` and `decode_unchecked`, after the length, FECF, version, channel lookup, Secondary Header, data field, FHP classification (`Malformed`, `first_header_pointer`), Packet Order Flag, and Segment Length ID checks; a frame with an earlier fault reports it. Strict mode: `InvalidField`, field `vc_id`, with no `expected` or `found` value. Lenient mode: the frame is accepted and the anomaly `OnlyIdleDataOnSdlsVc` (bit 2, `1 << 2` in the C ABI mask) is in the frame's anomalies, together with any other anomaly the same frame raises. `validate` returns `InvalidField` (`vc_id`) for the same frame after all its other checks, whatever the mode it was decoded in. With Sync Flag = 1 the FHP is undefined and the rule does not apply; with FHP `0x7FF` or an offset it does not apply either. `decode` (no channel table) and `aldis_tm_decode` read every VC as one without SDLS, so they accept the frame and report no anomaly. Vector operations that take a channel table (`decode_with`) exercise the rule; the plain `decode` operation does not.
