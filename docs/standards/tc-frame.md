# TC Transfer Frame and CLCW — CCSDS 232.0-B-4 (+ Cor. 1)

**Crate:** `aldis-tc` · **Reference:** `specs/pdf/232x0b4e1c1.pdf` ([how to download](../../specs/README.md))

## 1. Wire format (§4.1)

```
Primary Header (5) ‖ [Segment Header (1)] ‖ [Security Header] ‖ Data ‖ [Security Trailer] ‖ [FECF (2)]
Total length 6 … 1024 octets (Frame Length field = total − 1)
```

| Bits | Field | Size |
|---|---|---|
| 0–1 | Transfer Frame Version Number `00` | 2 |
| 2 | Bypass Flag (0 = Type-A, 1 = Type-B) | 1 |
| 3 | Control Command Flag (0 = Type-D data, 1 = Type-C control) | 1 |
| 4–5 | Reserved Spare `00` | 2 |
| 6–15 | Spacecraft ID | 10 |
| 16–21 | Virtual Channel ID | 6 |
| 22–31 | Frame Length (total − 1) | 10 |
| 32–39 | Frame Sequence Number N(S) | 8 |

Flag combinations (Table 4-1): `00` AD, `01` **reserved**, `10` BD, `11` BC.
Segment Header (optional per VC, never in Type-C frames): Sequence Flags (2 b: `01` first, `00` continuing, `10` last, `11` unsegmented) ‖ MAP ID (6 b).
Control commands (Type-C data field): **Unlock** = `00`; **Set V(R)** = `82 00 vv`.

## 2. Configuration (managed parameters, §5)

| Level | Parameters |
|---|---|
| Physical channel | max frame length (≤ 1024), FECF present, valid SCIDs, max frames per CLTU, max repetitions |
| Master channel | SCID, valid VCIDs (0–63), max frame length |
| Virtual channel | VCID, Segment Header present, data field content (Packets/VCA_SDU, if no Segment Header), valid MAP IDs, blocking permitted, repetitions (Type-A, Type-BC), `SecurityLayout` (per VC, or per MAP) |
| MAP | MAP ID, content (Packets/MAP_SDU), blocking, segmentation, max MAP_SDU length, max FDU length (≤ 1019) |

## 3. Frame requirements

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| TC-001 | The decoder SHALL derive the frame length from the Frame Length field and consume exactly that many octets (API-011). | §4.1.2.7 | V |
| TC-002 | The decoder SHALL reject TFVN ≠ `00` (`UnsupportedVersion`) and the reserved flag combination Bypass = 0 / Control Command = 1 (`ReservedValue`, both modes). | §4.1.2.2, Table 4-1 | V |
| TC-003 | Non-zero Reserved Spare bits SHALL be `ReservedValue` in strict mode and an anomaly in lenient mode. | §4.1.2.4.2 | V |
| TC-004 | The Segment Header SHALL be parsed only when the VC configuration says it is present and the frame is Type-D. In Type-C frames it SHALL be absent. | §4.1.3.2.1.4, §4.1.3.2.2.1.3 | V |
| TC-005 | A Type-C data field SHALL be decoded as `Unlock`, `SetVr(v)`, or, for any other pattern, `ReservedValue` in strict mode and `UnknownControlCommand(raw)` in lenient mode. | §4.1.3.3 | V |
| TC-006 | The FECF SHALL be verified when it is configured (CMN-001). | §4.1.4 | V |
| TC-007 | The writer SHALL compute Frame Length and the FECF. It SHALL reject total length > configured maximum or > 1024, and a data field larger than 1019 (1017 with FECF). | §4.1.2.7, §4.1.3.1.2 | V |
| TC-008 | In Type-B frames the writer SHALL accept the caller's N(S) value. COP-1 conventionally sets it to 0 for Type-B frames, but the codec SHALL NOT enforce this. | §4.1.2.8 note 3 | T |

## 4. Receiving-end procedures provided by the codec

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| TC-020 | `FrameDelimiter` SHALL implement the **Frame Delimiting and Fill Removal Procedure**. Given the octets of one decoded data unit (e.g. one CLTU's decoded codeblocks), it SHALL yield successive frames by Frame Length. If the remaining octets are fewer than a frame's declared length, they SHALL be discarded with an event (`TruncatedFrame`). Trailing octets that cannot form a valid header SHALL be treated as fill and discarded. | §4.4.9.2.4–6 | V |
| TC-021 | `validate_frame(&config)` SHALL implement the **Frame Validation Checks**: (a) expected TFVN; (b) expected MCID (TFVN + SCID); (c) header values consistent with implemented features (VCID valid, flags not reserved, Segment Header consistent); (d) Frame Length consistent with the octets present; (e) FECF matches if present. Each failure SHALL be reported with a distinct reason. | §4.4.9.3.3 | V |
| TC-022 | Frames with an invalid VCID SHALL be discarded with reason `InvalidVcid`. FDUs with an invalid MAP ID SHALL be discarded with reason `InvalidMapId`. | §4.4.7.3, §4.4.3.3 | V |
| TC-023 | Validation SHALL be applied identically to Type-A and Type-B frames, before any FARM-1 processing. | §4.4.9.3.1 | T |

## 5. CLCW

32-bit word carried in the OCF (§4.2):

| Bits | Field | Size | Value / meaning |
|---|---|---|---|
| 0 | Control Word Type | 1 | `0` |
| 1–2 | CLCW Version Number | 2 | `00` |
| 3–5 | Status Field | 3 | mission-specific |
| 6–7 | COP in Effect | 2 | `01` = COP-1 |
| 8–13 | Virtual Channel ID | 6 | |
| 14–15 | Reserved Spare | 2 | `00` |
| 16 | No RF Available | 1 | |
| 17 | No Bit Lock | 1 | `0` if unused |
| 18 | Lockout | 1 | |
| 19 | Wait | 1 | |
| 20 | Retransmit | 1 | |
| 21–22 | FARM-B Counter | 2 | |
| 23 | Reserved Spare | 1 | `0` |
| 24–31 | Report Value N(R) | 8 | = V(R) |

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| TC-030 | `Clcw` SHALL encode and decode all fields above. Strict decoding SHALL require Control Word Type `0`, version `00`, and zero spares. Strict decoding SHALL accept any COP in Effect value, exposing it. FOP-1 rejects COP ≠ `01` itself ([cop1.md](cop1.md)). | §4.2.1 | V |
| TC-031 | The Status Field SHALL be passed through unchanged (mission-specific). | §4.2.1.4 | V |

## 6. Anomaly flags (lenient mode)

`TcAnomaly::{ReservedSpareSet, UnknownControlCommand, ClcwSpareSet}`

## 7. Test-vector categories

`tc/ad`, `tc/bd`, `tc/bc_unlock`, `tc/bc_setvr`, `tc/segment_header`, `tc/max_length`, `tc/delimit_multiple_frames_with_fill`, `tc/validation_failures` (one per check a–e), `tc/clcw`.
