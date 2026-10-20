# USLP Transfer Frame — CCSDS 732.1-B-3

**Crate:** `aldis-uslp` · **Reference:** `specs/pdf/732x1b3e1.pdf` ([how to download](../../specs/README.md))

## 1. Wire format (§4.1, Annex D)

```
Non-truncated:  Primary Header (7–14) ‖ [Insert Zone] ‖ [Security Header] ‖ TFDF ‖ [Security Trailer] ‖ [OCF (4)] ‖ [FECF (2)]
Truncated:      Truncated Primary Header (4) ‖ [Security Header (6)] ‖ TFDF ‖ [Security Trailer (16)]
TFDF          = TFDF Header (1 or 3) ‖ TFDZ
```

**Primary Header (non-truncated)**

| Bits | Field | Size | Notes |
|---|---|---|---|
| 0–3 | TFVN | 4 | `1100` |
| 4–19 | Spacecraft ID | 16 | |
| 20 | Source-or-Destination ID | 1 | 0 = SCID is source, 1 = destination |
| 21–26 | VCID | 6 | 63 = OID |
| 27–30 | MAP ID | 4 | 0–15; OID frames use 0 |
| 31 | End of Frame Primary Header Flag | 1 | 1 = truncated header (Annex D) |
| 32–47 | Frame Length (total − 1) | 16 | up to 65 536 octets |
| 48 | Bypass/Sequence Control Flag | 1 | 0 = sequence-controlled, 1 = expedited |
| 49 | Protocol Control Command Flag | 1 | 1 = TFDF carries protocol control (COP / SDLS) |
| 50–51 | Reserve Spares | 2 | `00` |
| 52 | OCF Flag | 1 | |
| 53–55 | VCF Count Length | 3 | 0–7 octets |
| 56… | VCF Count | 0–56 | mod 2^(8·len) |

Flag combinations (Table 4-1): `00` sequence-controlled user data, `01` **reserved**, `10` expedited user data, `11` protocol control commands (sequence-controlled service).

**TFDF Header:** TFDZ Construction Rule (3 bits) ‖ UPID (5 bits) ‖ [First Header / Last Valid Octet Pointer (16 bits)].

| Rule | TFDZ | Meaning | Pointer |
|---|---|---|---|
| `000` | fixed | Packets spanning frames | FHP (`0xFFFF` = no packet starts or ends) |
| `001` | fixed | Start of MAPA_SDU / VCA_SDU | LVO (`0xFFFF` = SDU continues) |
| `010` | fixed | Continuing portion of MAPA_SDU / VCA_SDU | LVO (`0xFFFF` = SDU continues) |
| `011` | variable | Octet stream | — |
| `100` | variable | Starting segment | — |
| `101` | variable | Continuing segment | — |
| `110` | variable | Last segment | — |
| `111` | variable | No segmentation (one MAPA/VCA SDU, complete packets, control commands, or Prox-1 SPDUs) | — |

The TFDZ is fixed-length on a VC whose VC Transfer Frame Type is fixed-length, and variable-length otherwise (§4.1.4.2.2.1.1), so a VC's frame type fixes which rules it may carry (USLP-016) and whether the pointer is present (USLP-005).

## 2. Configuration (managed parameters, §5)

| Level | Parameters |
|---|---|
| Physical channel | frame type (fixed/variable), frame length (fixed) or max length (variable), Insert Zone present/length (fixed only), FECF present, max frames per C&S data unit, max repetitions |
| Master channel | frame type, SCID (0–65535), valid VCIDs (0–62, +63), OCF required (fixed only) |
| Virtual channel | frame type (fixed when the physical or master channel is fixed, Table 5-3 note 2), VCID, VCF count length for sequence-controlled and expedited QoS, COP in effect (COP-1/COP-P/none), MAP IDs, truncated frame length (6–32; only on a variable-length VC, Annex D1.2.1; at least 28 with SDLS, Annex D1.2.4), SDU type, OCF allowed (variable) / required (fixed), repetitions, max TFDF completion delay, max inter-frame delay, `SecurityLayout` (per VC/MAP) |
| MAP | MAP ID, SDU type (Packet / MAPA_SDU / Octet stream), UPID |

## 3. Requirements

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| USLP-001 | The decoder SHALL reject TFVN ≠ `1100` and the reserved flag combination `01`. | §4.1.2.2.2, Table 4-1 | V |
| USLP-002 | If the End of Frame Primary Header Flag = 1, the decoder SHALL decode a truncated frame. Its total length SHALL come from the VC configuration (`truncated_frame_length`, 6–32 octets). Truncated frames are allowed only on a VC whose VC Transfer Frame Type is variable-length: the VC configuration SHALL reject a truncated frame length on a fixed-length VC with `InvalidConfig`, and a truncated frame on a configured VC that has no truncated frame length SHALL be `InvalidField` (End of Frame Primary Header Flag) in both modes (OI-35 item 2). The decoder SHALL treat the frame as expedited user data with a 1-octet TFDF header and no Insert Zone, OCF, or FECF. Its construction rule is one of `011`–`111` (USLP-016). Annex D1.4.1.2 gives rule `111` and the UPID "Mission Specific Information-1" only in notes; whether the decoder checks them is open ([open issues](../09-open-issues.md) OI-35 item 1), and until it is answered both are exposed and not checked. | Annex D1.2.1, D1.2.2, D1.3.1 note 3, D1.3.2, D1.4.1.2 | V |
| USLP-003 | For fixed-length physical channels, the frame length SHALL equal the configured length. For variable-length channels, the Frame Length field SHALL govern and the decoder SHALL consume exactly that many octets. | §4.1.2.7; Table 5-1 | V |
| USLP-004 | The VCF Count SHALL be decoded with the length in the header. A VCF Count Length that differs from the VC's configured count length for the frame's QoS (sequence-controlled or expedited, by the Bypass/Sequence Control Flag) SHALL be `LengthMismatch` in both modes, because the field sets where the header ends (API-032; [open issues](../09-open-issues.md) OI-33). | §4.1.2.11–12, §4.1.2.11.4; Table 5-3 | V |
| USLP-005 | The TFDF header SHALL be 3 octets, with the First Header/Last Valid Octet Pointer, for construction rules `000`, `001`, and `010`, and 1 octet, without it, for `011`–`111`. With USLP-016 the pointer is present exactly when the VC's TFDZ is fixed-length ([open issues](../09-open-issues.md) OI-05, resolved). | §4.1.4.2.1.2, §4.1.4.2.2.2.1–3, §4.1.4.2.4.1, §4.1.4.2.4.3, §4.1.4.2.4.5; Annex C3.1; Annex D1.4.1.2 | V |
| USLP-006 | For rule `000`, the FHP SHALL be classified as `PacketStartsAt(n)` or `NoPacketStartOrEnd` (`0xFFFF`). For rules `001`/`010`, the LVO SHALL be classified as `LastValidOctet(n)` or `SduContinues` (`0xFFFF`). n beyond the TFDZ SHALL be `Malformed`. | §4.1.4.2.4 | V |
| USLP-007 | The UPID SHALL be exposed as a raw 5-bit value plus a typed interpretation from a table generated from the **SANA UPID registry**. Values SHALL NOT be hard-coded from memory (OI-06). Unknown UPIDs SHALL decode successfully as `Upid::Other(n)` in both modes and raise no anomaly ([open issues](../09-open-issues.md) OI-33). | §4.1.4.2.3 | V |
| USLP-008 | The Insert Zone SHALL be permitted only on fixed-length physical channels (configuration error otherwise). | Table 5-1 note 5 | T |
| USLP-009 | OID frames SHALL have VCID 63, MAP ID 0, rule `001`, UPID = "Only Idle Data", LVO = last octet of the TFDZ, and a TFDZ filled from the `OidGenerator`. OID frames SHALL be generated only on fixed-length physical channels. | §4.1.4.1.5–12; Table 5-1 note 4 | V |
| USLP-010 | The writer SHALL compute Frame Length, VCF Count Length (from configuration and QoS), the TFDF header, and the FECF. It SHALL take the TFDZ as given: completing a partly filled fixed-length TFDZ with an Encapsulation Idle Packet is the packet generator's (LNK-013; [open issues](../09-open-issues.md) OI-34). | §4.1.4.3.4 | V |
| USLP-011 | Truncated and non-truncated frames SHALL NOT be mixed in one C&S data unit (enforced by the TC/USLP multiplexing engine, [04](../04-engines.md)). | Annex D1.2.5 | T |
| USLP-012 | With SDLS on truncated frames, the layout SHALL be 6/16 octets and the truncated frame length SHALL be ≥ 28 (API-075). | Annex D1.2.3–4 | T |
| USLP-013 | When the TFDZ carries COP-1 control commands (protocol control flag = 1), the TFDZ SHALL be decodable with the TC control-command decoder (TC-005). | Table 4-1 | V |
| USLP-014 | When the Synchronization and Channel Coding Sublayer below is TC Synchronization and Channel Coding, `validate_frame(&config)` SHALL implement the **USLP Frame Validation Checks**: (a) expected TFVN; (b) one of the expected MCIDs (TFVN + SCID); (c) header values consistent with the features configured for the spacecraft; (d) the octets present consistent with the Frame Length field or, for a truncated frame, with the VC's truncated frame length; (e) FECF matches if present. Each failure SHALL be reported with a distinct reason. | §4.3.11.3 | V |
| USLP-015 | When the Synchronization and Channel Coding Sublayer below is TC Synchronization and Channel Coding, `FrameDelimiter` SHALL implement the **Frame Delimiting and Fill Data Removal Procedure**. Given the octets of one decoded data unit, it SHALL yield successive frames by Frame Length. If the remaining octets are fewer than a frame's declared length, they SHALL be discarded with an event (`IncompleteFrame`). Trailing octets that cannot form a valid header SHALL be treated as fill and discarded. How a data unit that holds a truncated frame is delimited is open: [open issues](../09-open-issues.md) OI-35 item 8 proposes an answer. | §4.3.11.1.6, §4.3.11.2 | V |
| USLP-016 | The TFDZ Construction Rule SHALL match the TFDZ type that the VC Transfer Frame Type sets: `000`–`010` on a fixed-length VC, and `011`–`111` on a variable-length VC, which includes every truncated frame. VCID 63 counts as fixed-length, because OID frames exist only on fixed-length channels (USLP-009). The decoder SHALL reject any other rule with `InvalidField` (TFDZ Construction Rule) in both modes, before it reads a pointer, and the writer SHALL reject it with `InvalidField` ([open issues](../09-open-issues.md) OI-35 item 3). | §4.1.4.2.2.1.1, §4.1.4.2.2.2.1–8, Table 4-3; Annex D1.2.1 | V |

## 4. Anomaly flags (lenient mode)

`UslpAnomaly` flags. The bit index is the flag's position in `AnomalySet::bits()` and is shared with the C ABI. It never changes once `aldis-uslp` is released. Before OI-33 the flags were listed without bit indices, so withdrawing `VcfLengthMismatchForQos` (an error in both modes, USLP-004) and `UnknownUpid` (reported by `Upid::Other(n)`, USLP-007) reserves no bit. Version, length, and FECF errors remain errors in both modes (API-032).

| Bit | Flag | Deviation | Source |
|---|---|---|---|
| 0 | `ReserveSparesSet` | Reserve Spares (bits 50–51) ≠ `00` | §4.1.2.9.2 |

## 5. Test-vector categories

`uslp/fixed_packets_rule000`, `uslp/fixed_mapa_rules001_010`, `uslp/variable_rules011_111`, `uslp/vcf_count_lengths_0_to_7`, `uslp/truncated`, `uslp/truncated_sdls_layout`, `uslp/oid`, `uslp/insert_zone`, `uslp/protocol_control_cop1`, `uslp/invalid`.
