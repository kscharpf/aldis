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
| 48–63 | Frame Header Error Control (optional) | 16 | RS(10,6) over GF(16) |

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
| AOS-013 | The FHEC codec SHALL implement the shortened Reed-Solomon (10,6) code over GF(2⁴): field polynomial x⁴+x+1; generator g(x) = (x+α⁶)(x+α⁷)(x+α⁸)(x+α⁹); 5 virtual zero fill symbols at the start; systematic. Symbols 6–11 are header bits 0–15 and 40–47 (4 bits each); symbols 12–15 are parity. | §4.1.2.6.5 | V |
| AOS-014 | On decode with FHEC configured, the codec SHALL correct up to two symbol errors in the protected fields and report `corrected_symbols`. An uncorrectable header SHALL fail with `HeaderUncorrectable`. The VC Frame Count (bits 16–39) is not protected and SHALL NOT be altered. | §4.1.2.6 notes | V |
| AOS-015 | The Insert Zone SHALL be exposed as a fixed-length slice of the configured length. It SHALL be present in every frame, including OID frames, when configured. | §4.1.3 | V |
| AOS-016 | An OID frame SHALL have VCID = 63 and a data field filled from the `OidGenerator`. | §4.1.4.1.5 | V |
| AOS-017 | M_PDU and B_PDU views SHALL classify their pointers (`PacketStartsAt`/`NoPacketStart`/`OnlyIdleData`; `ValidBits(n)`/`AllValid`/`OnlyIdle`). An out-of-range pointer SHALL be `Malformed` in strict mode. | §4.1.4.2.2, §4.1.4.3.3 | V |
| AOS-018 | A B_PDU zone larger than 16 384 bits (2 048 octets) SHALL be rejected for configurations using the Bitstream service. | §4.1.4.3.3 note | T |
| AOS-019 | The writer SHALL compute the FHEC (when configured) and the FECF. It SHALL accept the VC frame count, cycle, replay flag, and data field contents from the caller or engine. | §4.1.2, §4.1.6 | V |

## 5. Anomaly flags (lenient mode)

`AosAnomaly::{CycleNonZeroWhileUnused, BpduSpareSet, PointerBeyondZone, Issue4SpareSet, FhecCorrected}`

## 6. Test-vector categories

`aos/minimal`, `aos/fhec_encode`, `aos/fhec_correct_1_symbol`, `aos/fhec_correct_2_symbols`, `aos/fhec_uncorrectable`, `aos/insert_zone`, `aos/mpdu_pointers`, `aos/bpdu_pointers`, `aos/oid`, `aos/issue4_compat`, `aos/scid_10bit`.
