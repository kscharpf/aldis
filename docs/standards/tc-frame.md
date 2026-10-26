# TC Transfer Frame and CLCW — CCSDS 232.0-B-4 (+ Cor. 1)

**Crate:** `aldis-tc` · **Reference:** `specs/pdf/232x0b4e1c1.pdf` ([how to download](../../specs/README.md)) · **Acronyms:** [glossary](../glossary.md)

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

The codec keeps the parameters it reads, in `aldis_tc::config`; the others belong to the engines that use them (M4: data field content, blocking, segmentation, the MAP_SDU and FDU limits, repetitions, frames per data unit, the multiplexing schemes, the CLCW reporting rate):

| Type | Parameters | Checked by its constructor |
|---|---|---|
| `PhysicalChannelConfig` | Maximum Transfer Frame Length, FECF present (Table 5-1) | 6 (8 with FECF) to 1024 octets |
| `VcConfig` | SCID, VCID, Maximum Transfer Frame Length, `SegmentHeaderUse::{Absent(SecurityLayout), Present}` (Tables 5-3, 6-2) | 6 to 1024 octets |
| `MapConfig` | SCID, VCID, MAP ID, `SecurityLayout` (Tables 5-4, 6-3) | — |
| `VcTable<V, M>` | the channels of one Physical Channel; implements the `VcLookup` trait the decoder, writers, and validator take (M2 D14). The valid SCIDs (Table 5-1) and each Master Channel's valid VCIDs (Table 5-2) are those of its Virtual Channels | no VC (`vc_id`; TC-024); VCs and MAPs unique; a MAP only on a VC with Segment Headers; every VC and MAP leaves at least one data octet in its longest frame (CMN-041); at most `V` VCs and `M` MAPs |

A frame's limit is the smaller of its Virtual and Physical Channel's maximum. The Master Channel's own maximum (Table 5-2) is not kept separately: each of its Virtual Channels carries one.

## 3. Frame requirements

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| TC-001 | The decoder SHALL derive the frame length from the Frame Length field and consume exactly that many octets (API-011). | §4.1.2.7 | V |
| TC-002 | The decoder SHALL reject TFVN ≠ `00` (`UnsupportedVersion`) and the reserved flag combination Bypass = 0 / Control Command = 1 (`ReservedValue`, both modes). `peek_length` applies the same two checks to the header it reads (API-013), with the same errors. | §4.1.2.2, Table 4-1 | V |
| TC-003 | Non-zero Reserved Spare bits SHALL be `ReservedValue` in strict mode and an anomaly in lenient mode. | §4.1.2.4.2 | V |
| TC-004 | The Segment Header SHALL be parsed only when the VC configuration says it is present and the frame is Type-D. In Type-C frames it SHALL be absent. | §4.1.3.2.1.4, §4.1.3.2.2.1.3 | V |
| TC-005 | A Type-C data field SHALL be decoded as `Unlock`, `SetVr(v)`, or, for any other pattern, `ReservedValue` in strict mode and `UnknownControlCommand(raw)` in lenient mode. | §4.1.3.3 | V |
| TC-006 | The FECF SHALL be verified when it is configured (CMN-001). | §4.1.4 | V |
| TC-007 | The writer SHALL compute Frame Length and the FECF. It SHALL reject total length > configured maximum or > 1024, and a data field larger than 1019 (1017 with FECF). The data-field limit is the 1024 limit expressed for the data field, so it is the same error: the one-shot writer reports a frame above its limit as `OutOfRange` naming `frame_len`; the two-phase writer's `finish` reports User Data above its reservation as `OutOfRange` naming `user_len`. | §4.1.2.7, §4.1.3.1.2 | V |
| TC-008 | In Type-B frames the writer SHALL accept the caller's N(S) value. COP-1 conventionally sets it to 0 for Type-B frames, but the codec SHALL NOT enforce this. | §4.1.2.8 note 3 | T |
| TC-009 | The writer SHALL build a Type-C frame (Control Command Flag `1`: BC) from a Control Command only, and a Type-D frame (Control Command Flag `0`: AD or BD) from a Frame Data Unit only. Any other pairing SHALL be rejected with `InvalidField` naming `control_command_flag`. | §4.1.2.3.2.2, Table 4-1 | V |

## 4. Receiving-end procedures provided by the codec

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| TC-020 | `FrameDelimiter` SHALL implement the **Frame Delimiting and Fill Removal Procedure**. Given the octets of one decoded data unit (e.g. one CLTU's decoded codeblocks), it SHALL yield successive frames by Frame Length. If the remaining octets are fewer than a frame's declared length, they SHALL be discarded with an event (`TruncatedFrame`). Trailing octets that cannot form a valid header SHALL be treated as fill and discarded. | §4.4.9.2.4–6 | V |
| TC-021 | `validate_frame(&config)` SHALL implement the **Frame Validation Checks**: (a) expected TFVN; (b) expected MCID (TFVN + SCID); (c) header values consistent with implemented features (VCID valid, flags not reserved, Segment Header consistent); (d) Frame Length consistent with the octets present; (e) FECF matches if present. Each failure SHALL be reported with a distinct reason. | §4.4.9.3.3 | V |
| TC-022 | Frames with an invalid VCID SHALL be discarded with reason `InvalidVcid`. FDUs with an invalid MAP ID SHALL be discarded with reason `InvalidMapId`. | §4.4.7.3, §4.4.3.3 | V |
| TC-023 | Validation SHALL be applied identically to Type-A and Type-B frames, before any FARM-1 processing. | §4.4.9.3.1 | T |
| TC-024 | A channel table (`VcTable::new`) SHALL be rejected with `InvalidConfig` naming `vc_id` when it holds no Virtual Channel. A Physical Channel with none cannot receive a frame (every frame names a VCID, TC-022), and the error a frame then got named `spacecraft_id`, not `vc_id` (OI-55). | 232.0 §5, Table 5-2 | T |

### 4.1 Check order and validation reasons

`TcFrame::decode` and `validate_frame` run one sequence of checks (`aldis_tc::view`), in the order of the Frame Validation Checks of §4.4.9.3.3, and stop at the first failure. `validate_frame` takes the octets of exactly one frame, as the delimiter yields them, and reports a `Rejection`: the reason below and the decoder's error. `decode` accepts octets after the frame (API-011) and reports only the error. Each reason has a stable numeric code, shared with the C ABI (M2 D14). Reasons 4 and 11 occur only in strict mode; in lenient mode the deviation is an anomaly flag (§6).

| Code | Reason | Check | Condition | Decoder error (field) |
|---|---|---|---|---|
| 1 | `UnexpectedVersion` | (a) | TFVN ≠ `00` (§4.1.2.2.2) | `UnsupportedVersion` (`frame_version`) |
| 2 | `UnexpectedSpacecraft` | (b) | SCID not among the valid SCIDs of the Physical Channel | `UnknownChannel` (`spacecraft_id`) |
| 3 | `ReservedFlags` | (c) | Bypass Flag `0`, Control Command Flag `1` (Table 4-1) | `ReservedValue` (`control_command_flag`) |
| 4 | `ReservedSpare` | (c) | Reserved Spare ≠ `00`, strict mode (§4.1.2.4.2) | `ReservedValue` (`tc_header_spare`) |
| 5 | `InvalidVcid` | (c), TC-022 | VCID not valid on the Master Channel (§4.4.7.3) | `UnknownChannel` (`vc_id`) |
| 6 | `FrameTooLong` | (c) | Frame Length above the VC's or the Physical Channel's maximum (Tables 5-1, 5-3) | `OutOfRange` (`frame_len`) |
| 7 | `FrameTooShort` | (c) | no data octet after the Primary Header, a Segment Header the frame must have, and the FECF; checked again with the Security Header and Trailer once the MAP is known ([OI-41](../09-open-issues.md) item 1) | `InvalidField` (`frame_data_field`) |
| 8 | `LengthMismatch` | (d) | octets present ≠ Frame Length, or fewer than 5 | `LengthMismatch` (`frame_len`) when validating; `Incomplete` when decoding fewer octets than the frame |
| 9 | `FecfMismatch` | (e) | FECF configured and wrong (§4.1.4.3) | `CrcMismatch` |
| 10 | `InvalidMapId` | TC-022 | MAP ID of the Segment Header not valid on the VC (§4.4.3.3) | `UnknownChannel` (`map_id`) |
| 11 | `ReservedControlCommand` | TC-005 | Type-C data field neither Unlock nor Set V(R), strict mode (§4.1.3.3.4) | `ReservedValue` (`control_command`) |

The FECF is checked before the Segment Header is read, so a bit error in the MAP ID is reported as reason 9, not 10.

**Limit of check (d).** A frame the delimiter yields (§4.2) was bounded by its own Frame Length field, and check (d) reads the same field, so (d) cannot see a Frame Length error in such a frame, in either direction: the frame is lengthened or shortened, and (d) agrees with it. The FECF (TC-006, reason 9) is then the only control, and on a channel with no FECF the frame is accepted. Check (d) rejects a Frame Length error only for octets bounded by something other than that field (assurance case ASM-2, `hz:HZ-TC-01`).

### 4.2 Delimiting

`next_frame(rest)` is stateless: it returns the next item of the octets not yet delimited, and the caller skips its length (M2 D14). `FrameDelimiter` iterates with it. An item is a `Frame`, a `TruncatedFrame` (the rest, discarded; §4.4.9.2.5), or `Fill` (the rest, discarded; §4.4.9.2.6). The next octets form a **valid header** when there are at least five of them, the TFVN is `00`, and the Frame Length declares at least six octets (§4.1.1, §4.1.2.7); the Fill Data pattern `01010101` of §4.4.9.2.6 note 2 has TFVN `01` and is never a valid header ([OI-41](../09-open-issues.md) item 2). A frame the delimiter yields still goes through `validate_frame`, which cannot see a Frame Length error in it (the limit of check (d), §4.1). The delimiter takes every octet after the last frame as fill whatever their number, although at most six occur with the BCH codeblock (§4.4.9.2.6 note 2); a bound set by the caller is [OI-47](../09-open-issues.md), implemented in M3.

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
| TC-030 | `Clcw` SHALL encode and decode all fields above. Strict decoding SHALL require Control Word Type `0`, version `00`, and zero spares. Strict decoding SHALL accept any COP in Effect value, exposing it. FOP-1 rejects COP ≠ `01` itself ([cop1.md](cop1.md)). §4.2.1.5.2 says the field "shall be set to `01`" for COP-1 and API-031 would make a checkable SHALL violation a strict error; this requirement is a listed exception to API-031 (maintainer decision of 2026-10-25). | §4.2.1 | V |
| TC-031 | The Status Field SHALL be passed through unchanged (mission-specific). | §4.2.1.4 | V |

## 6. Anomaly flags (lenient mode)

`TcAnomaly`, with the bit indices shared with the C ABI (API-033):

| Bit | Flag | Raised when (lenient mode only) |
|---|---|---|
| 0 | `ReservedSpareSet` | Reserved Spare of the Primary Header ≠ `00` (§4.1.2.4.2; TC-003) |
| 1 | `UnknownControlCommand` | a Type-C data field that is neither Unlock nor Set V(R); the view keeps its octets in `data_field()` (§4.1.3.3.4; TC-005) |
| 2 | `ClcwSpareSet` | a Reserved Spare of the CLCW, bits 14–15 or bit 23, not zero (§4.2.1.7.2, §4.2.1.10.2; TC-030) |

The Control Word Type and the CLCW Version Number are errors in both modes (`InvalidField` naming `control_word_type`, `UnsupportedVersion` naming `clcw_version`; API-032).

## 7. Test-vector categories

`tc/ad`, `tc/bd`, `tc/bc_unlock`, `tc/bc_setvr`, `tc/segment_header`, `tc/max_length`, `tc/delimit_multiple_frames_with_fill`, `tc/validation_failures` (one per check a–e, and every reason of §4.1), `tc/clcw`; and `tc/decode_errors` (the decoder's errors and lenient anomalies) and `tc/config` (the managed parameters of §2).
