# TM Transfer Frame — CCSDS 132.0-B-3

**Crate:** `aldis-tm` · **Reference:** `specs/pdf/132x0b3.pdf` ([how to download](../../specs/README.md)) · **Acronyms:** [glossary](../glossary.md)

## 1. Wire format (§4.1)

```
Primary Header (6) ‖ [Secondary Header (2–64)] ‖ [Security Header] ‖ Data Field ‖ [Security Trailer] ‖ [OCF (4)] ‖ [FECF (2)]
```

| Bits | Field | Size | Notes |
|---|---|---|---|
| 0–1 | Transfer Frame Version Number | 2 | `00` |
| 2–11 | Spacecraft ID | 10 | |
| 12–14 | Virtual Channel ID | 3 | 0–7 |
| 15 | OCF Flag | 1 | static per MC/VC |
| 16–23 | Master Channel Frame Count | 8 | mod 256 |
| 24–31 | Virtual Channel Frame Count | 8 | mod 256 |
| 32 | Secondary Header Flag | 1 | static per MC/VC |
| 33 | Synchronization Flag | 1 | 0 = packets/idle (octet-synchronized, forward-ordered), 1 = VCA_SDU |
| 34 | Packet Order Flag | 1 | `0` (reserved) when Sync = 0; undefined when Sync = 1 |
| 35–36 | Segment Length ID | 2 | `11` when Sync = 0; undefined when Sync = 1 |
| 37–47 | First Header Pointer | 11 | offset of first packet header; `0x7FF` no packet starts; `0x7FE` OID frame |

Secondary Header: ID octet = version `00` (2 bits) ‖ length − 1 (6 bits), followed by 1–63 data octets (total 2–64, fixed per MC/VC).

## 2. Configuration (managed parameters, §5)

| Level | Parameters |
|---|---|
| Physical channel | frame length (fixed), FECF present, valid SCIDs, MC multiplexing policy |
| Master channel | SCID, valid VCIDs (0–7), MC_FSH present/length (2–64), MC_OCF present |
| Virtual channel | VCID, data field content (Packets / VCA_SDU), VC_FSH present/length, VC_OCF present, `SecurityLayout` |
| Packet transfer | valid PVNs, max packet length, deliver incomplete packets |

## 3. Requirements

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| TM-001 | The decoder SHALL require the configured fixed frame length and the configured FECF presence (API-012). | §4.1.1.2, §4.1.6.1.2 | V |
| TM-002 | The decoder SHALL reject TFVN ≠ `00` with `UnsupportedVersion`. | §4.1.2.2.2.2 | V |
| TM-003 | If the Secondary Header Flag is set, the decoder SHALL read the FSH length from its ID octet. It SHALL reject FSH version ≠ `00`, and an FSH that would overrun the frame. | §4.1.3.2 | V |
| TM-004 | If the OCF Flag is set, the decoder SHALL expose the 4 OCF octets ([common §2](common-link-features.md#2-operational-control-field)). | §4.1.5 | V |
| TM-005 | If the FECF is configured, the decoder SHALL verify it before returning a view and SHALL fail with `CrcMismatch` on error. A separate `decode_unchecked` SHALL exist for diagnostics. | §4.1.6 | V |
| TM-006 | The data field length SHALL be computed as frame length − 6 − FSH − security header − security trailer − OCF − FECF (CMN-040). | §4.1.4.2, §6.3.5.2 | V |
| TM-007 | With Sync Flag = 0, strict mode SHALL require Packet Order Flag = 0 and Segment Length ID = `11`. Lenient mode SHALL flag deviations. With Sync Flag = 1, these fields SHALL be exposed raw and not validated. | §4.1.2.7.4–5 | V |
| TM-008 | With Sync Flag = 0, the view SHALL classify the FHP as `PacketStartsAt(n)` (n < data field length), `NoPacketStart` (`0x7FF`), or `OnlyIdleData` (`0x7FE`). Any other n ≥ data field length SHALL be `Malformed` in both modes: it points outside the field it indexes (API-032; [open issues](../09-open-issues.md) OI-33). With Sync Flag = 1 the FHP is undefined and SHALL be exposed raw. | §4.1.2.7.6, §4.1.2.7.6.2 note | V |
| TM-009 | The writer SHALL compute the FSH ID octet, the OCF flag (from configuration), and the FECF. It SHALL accept MC/VC frame counts, the sync flag, and the FHP as inputs (counts are owned by engines, [04](../04-engines.md)). | §4.1.2–6 | V |
| TM-010 | An OID writer SHALL set FHP = `0x7FE` and fill the data field from a supplied `OidGenerator` (CMN-020). It SHALL use a VCID that is configured for packets, else `InvalidField` (`sync_flag`); and a VC that does not use SDLS, that is with no Security Header or Trailer, else `InvalidField` (`vc_id`), checked in that order; it writes nothing on error (maintainer decision of 2026-10-25). Every other writer (`encode`, `begin`, `resume`, and their C ABI calls) SHALL apply the SDLS check to a frame with FHP = `0x7FE`, since that pointer makes it an OID frame. FSH and OCF MAY carry valid data. The decoder SHALL NOT check the OID data: it depends on the generator's position across frames (CMN-021), which a stateless decoder does not know, and receivers discard it. | §4.1.4.6; §6.4.6.2 | V |
| TM-011 | `validate(&MasterChannelConfig)` SHALL check SCID and VCID against the configuration. It SHALL check that the OCF Flag and FSH presence and length match the configured MC/VC parameters, and that the Sync Flag matches the VC data field content. | §5; §4.1.2.4.3; §4.1.2.7.3.3 | V |

## 4. Anomaly flags (lenient mode)

`TmAnomaly` flags. The bit index is the flag's position in `AnomalySet::bits()` and is shared with the C ABI. It never changes once `aldis-tm` is released. Before OI-33 the flags were listed without bit indices, so withdrawing `FhpBeyondDataField` (an FHP beyond the data field is `Malformed` in both modes, TM-008) reserves no bit. Version, length, and FECF errors remain errors in both modes (API-032).

| Bit | Flag | Deviation | Source |
|---|---|---|---|
| 0 | `PacketOrderFlagSet` | Packet Order Flag = 1 with Sync Flag = 0 (TM-007) | 132.0 §4.1.2.7.4 note |
| 1 | `SegmentLengthIdNot11` | Segment Length ID ≠ `11` with Sync Flag = 0, a fixed-value legacy field (TM-007, API-032) | 132.0 §4.1.2.7.5.2, notes 1–2 |

## 5. Test-vector categories

`tm/minimal` (no FSH/OCF/FECF), `tm/fsh_ocf_fecf`, `tm/fhp_values`, `tm/oid_sequence` (consecutive OID frames continue the PN sequence; the OID frame of a VC with SDLS is rejected), `tm/security_layout` (FixedPatternSecurity), `tm/invalid`.

## 6. Project rules and choices the vectors pin

The Blue Book does not decide these; they are the project's, and the second review ([m2-vectors.md](../verification/reviews/m2-vectors.md)) lists them:

- A frame with no data octet is rejected (CMN-041, whose source is none). The error names `frame_data_field` (7) when a configured field (FSH, OCF, Security fields) leaves none, and `frame_len` (68) for a physical channel too short for its own header and FECF when no VC is configured (`tm-inv-cfg-001`).
- The FECF is checked before the version (TM-005), so a frame with both faults is `CrcMismatch` (`tm-inv-crc-002`); `tm-inv-ver-001` and `-002` carry valid FECFs.
- Sync = 0 with Packet Order Flag = 1 is `ReservedValue` in strict mode (TM-007): the Blue Book gives the rule in a NOTE (§4.1.2.7.4). The error kinds and fields for a VC whose content does not fit the writer's request (TM-009, TM-010) are `InvalidField` naming `sync_flag`.
- Frames longer than 2048 octets are accepted: 132.0-B-3 states no maximum (131.0-B-6 §11 gives 2048 because of the 11-bit FHP), so that the FHP offsets 2045 and 2046 are testable (`tm-fhp-enc-005`, `tm-fhp-enc-err-002`).
