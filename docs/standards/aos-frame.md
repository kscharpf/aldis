# AOS Transfer Frame — CCSDS 732.0-B-5

**Crate:** `aldis-aos` · **Reference:** `specs/pdf/732x0b5ec1.pdf` ([how to download](../../specs/README.md))

## 1. Wire format (§4.1)

```
Primary Header (6, or 8 with FHEC) ‖ [Insert Zone] ‖ [Security Header] ‖ Data Field ‖ [Security Trailer] ‖ [OCF (4)] ‖ [FECF (2)]
```

| Bits | Field | Size | Notes |
|---|---|---|---|
| 0–1 | Transfer Frame Version Number | 2 | `01` |
| 2–9 | Spacecraft ID (8 LSBs) | 8 | |
| 10–15 | Virtual Channel ID | 6 | 63 = OID frames |
| 16–39 | VC Frame Count | 24 | mod 2²⁴ |
| 40 | Replay Flag | 1 | 0 realtime / 1 replay |
| 41 | VC Frame Count Cycle Use Flag | 1 | |
| 42–43 | Spacecraft ID (2 MSBs) | 2 | **Issue 5**; reserved `00` in Issue ≤4 |
| 44–47 | VC Frame Count Cycle | 4 | `0000` if unused |
| 48–63 | Frame Header Error Control (optional) | 16 | Reed-Solomon (15,11) over GF(2⁴), shortened by 5 symbols to a binary (40,24) code (AOS-013) |

The data field holds exactly one of: an **M_PDU** (packets), a **B_PDU** (bitstream), a **VCA_SDU**, or Idle Data (OID frame), fixed per VC.

- **M_PDU (Issue 5):** First Header Pointer (16 bits) ‖ Packet Zone. FHP `0xFFFF` = no packet starts; `0xFFFE` = only idle data.
- **B_PDU:** Reserved Spare (2 bits, `00`) ‖ Bitstream Data Pointer (14 bits, index of the last valid *bit*) ‖ Data Zone. Pointer `0x3FFF` = all valid; `0x3FFE` = only idle.

## 2. Editions

Issue 5 extended the SCID to 10 bits using former spare bits 42–43, and the M_PDU First Header Pointer to 16 bits (to allow frames up to 65 536 octets). Missions on Issue ≤4 are not wire-compatible.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| AOS-001 | The physical-channel configuration SHALL carry `AosEdition::{Issue5, Issue4Compatible}`, defaulting to `Issue5` (GEN-062). | §4.1.2.2.3 notes | T |
| AOS-002 | In `Issue4Compatible`, the SCID SHALL be 8 bits. Bits 42–43 SHALL be written as `00` and checked as reserved spare on decode. | §4.1.2.2.3 note 1 | V |
| AOS-003 | In `Issue4Compatible`, the M_PDU header SHALL be 5 spare bits followed by an 11-bit FHP (`0x7FF` none, `0x7FE` idle). Confirmed against 732.0-B-4 ([open issues](../09-open-issues.md) OI-04). | 732.0-B-4 §4.1.4.2.2, §4.1.4.2.3 (superseded) | V |

## 3. Configuration (managed parameters, §5)

| Level | Parameters |
|---|---|
| Physical channel | frame length, FHEC present, Insert Zone present/length, FECF present, edition, valid SCIDs, MC mux policy |
| Master channel | SCID, valid VCIDs (0–62; 63 always valid for OID) |
| Virtual channel | VCID (0–62), content (M_PDU / B_PDU / VCA_SDU / idle), VC_OCF present, `SecurityLayout` |
| Packet transfer | valid PVNs, max packet length, deliver incomplete packets |

There is **no OCF flag in the AOS header**. OCF presence is a per-VC managed parameter, so decoding requires the VC configuration.

## 4. Requirements

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| AOS-010 | Decoding SHALL be two-step: `AosPrimaryHeader::decode(bytes, &PhysicalChannelConfig)`, then `AosFrame::decode(bytes, &PhysicalChannelConfig, &VcLookup)`. `VcLookup` maps a VCID to its VC configuration (needed for OCF presence and data field content). | §4.1.5.2 | T |
| AOS-011 | The decoder SHALL reject TFVN ≠ `01`. | §4.1.2.2.2.2 | V |
| AOS-012 | The decoder SHALL ignore the VC Frame Count Cycle when its Use Flag = 0. Strict mode SHALL require the cycle field to be `0000` in that case. | §4.1.2.5.3, §4.1.2.5.5.3 | V |
| AOS-013 | The FHEC codec SHALL implement the Reed-Solomon (15,11) code over GF(2⁴), shortened by 5 symbols and converted to a binary (40,24) code: 4-bit symbols (J = 4), correction of up to two symbol errors (E = 2), field generator polynomial F(x) = x⁴+x+1, code generator polynomial g(x) = (x+α⁶)(x+α⁷)(x+α⁸)(x+α⁹), systematic. Symbols 1–5 are virtual fill: all zeros, at the start of the codeword only, never transmitted. Symbols 6–11 are header bits 0–15 and 40–47 (4 bits each, the leftmost bit transmitted first); symbols 12–15 are the parity symbols in bits 48–63. Counted in transmitted symbols the code is (10,6), the name 732.0-B-4 §4.1.2.6.5 gives the same code; one implementation serves both editions (OI-04, OI-35 item 6). | §4.1.2.6.5 a–h; 732.0-B-4 §4.1.2.6.5 | V |
| AOS-014 | On decode with FHEC configured, the codec SHALL correct up to two symbol errors in the ten transmitted symbols (header bits 0–15 and 40–63) before any header field is read, and the view SHALL report `corrected_symbols()` (0, 1, or 2) in both modes; a correction is not an anomaly (OI-33). When no pattern of at most two symbol errors in the transmitted symbols accounts for the syndromes, decoding SHALL fail with `HeaderUncorrectable` in both modes. With three or more symbol errors the decoder can return a wrong header as corrected, and no decoder can prevent it: the code's minimum distance is 5 (n − k + 1 for the (15,11) code, unchanged by shortening), so it guarantees two corrections and nothing beyond, and some patterns of three or more errors lie within two symbols of another codeword. `HeaderUncorrectable` is therefore reported only for the patterns the decoder detects (OI-35 item 5). When an FECF is configured, it SHALL be checked over the frame with the corrected header: the sender computes it over the transmitted header, and the receiving end corrects the header before it checks the FECF. A miscorrected header then fails with `CrcMismatch`, unless the CRC also misses the difference. The VC Frame Count (bits 16–39) is not protected and SHALL NOT be altered. | §4.1.2.6.1 notes 1–2; §4.1.2.6.5 b, note 3; §4.1.6.2; §4.3.7.2–3 | V |
| AOS-015 | The Insert Zone SHALL be exposed as a fixed-length slice of the configured length. It SHALL be present in every frame, including OID frames, when configured. | §4.1.3 | V |
| AOS-016 | An OID frame SHALL have VCID = 63 and a data field filled from the `OidGenerator`. | §4.1.4.1.5 | V |
| AOS-017 | M_PDU and B_PDU views SHALL classify their pointers (`PacketStartsAt`/`NoPacketStart`/`OnlyIdleData`; `ValidBits(n)`/`AllValid`/`OnlyIdle`). A pointer that is neither one of its two reserved values nor inside its zone (a First Header Pointer ≥ the Packet Zone length in octets; a Bitstream Data Pointer ≥ the Bitstream Data Zone length in bits) SHALL be `Malformed` in both modes: it points outside the field it indexes (API-032; [open issues](../09-open-issues.md) OI-33). | §4.1.4.2.2, §4.1.4.3.3 | V |
| AOS-018 | A configuration in which a VC carries B_PDUs SHALL be rejected with `InvalidConfig` if its Bitstream Data Zone is longer than 2 047 octets, in both editions. The 14-bit Bitstream Data Pointer holds the index of the last valid bit and reserves all ones (`0x3FFF`, no idle data) and all ones minus one (`0x3FFE`, only idle data), so it can name every bit of a zone of at most 16 382 bits; the longest zone of whole octets within that is 2 047 octets (16 376 bits, last index `0x3FF7`). In a zone of 2 048 octets the index 16 382 would read as "only idle data". In `Issue5` the configuration SHALL also be rejected if the frame length exceeds 2 048 octets (16 384 bits) while any VC carries B_PDUs, the limit 732.0-B-5 states for the Bitstream Service; the zone is then at most 2 040 octets (frame − 6-octet Primary Header − 2-octet B_PDU Header). 732.0-B-4 has the same pointer and reserved values but states no frame limit (OI-35 item 7). | §4.1.4.3.1.3–4, §4.1.4.3.3.2–4 and note; 732.0-B-4 §4.1.4.3.3 | T |
| AOS-019 | The writer SHALL compute the FHEC (when configured) and the FECF. It SHALL accept the VC frame count, cycle, replay flag, and data field contents from the caller or engine. | §4.1.2, §4.1.6 | V |

## 5. Anomaly flags (lenient mode)

`AosAnomaly` flags. The bit index is the flag's position in `AnomalySet::bits()` and is shared with the C ABI. It never changes once `aldis-aos` is released. Before OI-33 the flags were listed without bit indices, so withdrawing `PointerBeyondZone` (an out-of-range pointer is `Malformed` in both modes, AOS-017) and `FhecCorrected` (the view reports `corrected_symbols()` in both modes, AOS-014) reserves no bit. Version, length, FHEC, and FECF errors remain errors in both modes (API-032).

| Bit | Flag | Deviation | Source |
|---|---|---|---|
| 0 | `CycleNonZeroWhileUnused` | VC Frame Count Cycle ≠ `0000` with its Use Flag = 0 (AOS-012) | §4.1.2.5.5.3 |
| 1 | `BpduSpareSet` | B_PDU Reserved Spare ≠ `00` | §4.1.4.3.2.2 |
| 2 | `Issue4SpareSet` | In `Issue4Compatible`, Primary Header bits 42–43 ≠ `00` (AOS-002) or M_PDU Reserved Spare ≠ `00000` (AOS-003) | 732.0-B-4 §4.1.2.5.4, §4.1.4.2.2 |

## 6. Test-vector categories

`aos/minimal`, `aos/fhec_encode`, `aos/fhec_correct_1_symbol`, `aos/fhec_correct_2_symbols`, `aos/fhec_uncorrectable`, `aos/insert_zone`, `aos/mpdu_pointers`, `aos/bpdu_pointers`, `aos/oid`, `aos/issue4_compat`, `aos/scid_10bit`.
