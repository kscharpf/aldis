# Common link features

**Implemented in:** `aldis-core` · **Used by:** TM (132.0), TC (232.0), AOS (732.0), USLP (732.1) · **Acronyms:** [glossary](../glossary.md)

This document specifies algorithms and fields shared by several data link protocols. Each is implemented once in `aldis-core`.

## 1. Frame Error Control Field (CRC-16)

All four protocols define the FECF identically: 132.0 §4.1.6, 232.0 §4.1.4, 732.0 §4.1.6, and 732.1 Annex B.

- Generator G(X) = X¹⁶ + X¹² + X⁵ + 1 (0x1021)
- Register preset to all ones (L(X) term)
- MSB-first processing, no reflection, no final XOR
- FECF = the final 16 bits of the frame, MSB first

This is the CRC known in catalogues as **CRC-16/IBM-3740** (also called "CRC-16/CCITT-FALSE").

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CMN-001 | `crc16(data)` SHALL compute the FECF exactly as specified above. | 132.0 §4.1.6.2 | V |
| CMN-002 | `crc16(b"123456789")` SHALL equal `0x29B1`. This vector was verified against the generator definition during spec preparation. | — | V |
| CMN-003 | FECF verification SHALL be performed either by recomputing the CRC over the frame minus the FECF and comparing, or by computing the syndrome over the whole frame and checking for zero. The two methods are equivalent; both SHALL be tested. | 132.0 §4.1.6.3 | T |
| CMN-004 | An implementation SHALL provide a table-driven variant (256-entry table in read-only memory) and a bitwise variant selectable at build time. Both SHALL produce identical results. | — | T/V |

## 2. Operational Control Field

The OCF is 4 octets, present per the TM/USLP OCF flag or the AOS per-VC managed parameter (132.0 §4.1.5, 732.0 §4.1.5, 732.1 §4.1.5).

| Bit 0 | Bit 1 | Content |
|---|---|---|
| 0 | — | **Type-1 Report:** CLCW (COP-1) or PLCW (COP-P, USLP only) |
| 1 | 0 | Type-2 Report, project-specific |
| 1 | 1 | Type-2 Report, SDLS Frame Security Report (FSR) |

The table is the same in 132.0-B-3 §4.1.5.4–5, 732.0-B-4 and 732.0-B-5 §4.1.5.4–5, and 732.1-B-3 §4.1.5.2.1–2. None of the four values of bits 0–1 is reserved, so every four octets are an OCF and classification cannot fail. In a Type-1 Report bit 1 belongs to the report (it is the first bit of the CLCW Version Number, 232.0 §4.2.1.3). The FSR is defined in SDLS Extended Procedures (355.1-B-1), which `specs/` does not hold; `aldis-core` decodes neither the CLCW nor the FSR. `aldis_core::ocf` holds each report as its four octets, with the flag bits checked by the report's constructor (RS-020); a constructor given octets of another report fails with `InvalidField` naming `ocf_type_flag` or `ocf_report_use` ([03 §9.1](../03-api-model.md#91-field-codes)).

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CMN-010 | `Ocf::decode([u8;4])` SHALL classify the report as `Clcw(raw)`, `ProjectSpecific(raw)`, or `SdlsFsr(raw)` using bits 0–1. On USLP a Type-1 Report (`Clcw`) may also be a PLCW (732.1 §4.1.5.2.1 a); the variant keeps its name, and the caller that knows the channel decodes it. The enum is closed: the Blue Books assign all four values of the two bits (GEN-081 applies to enums that may grow). | 132.0 §4.1.5.4–5; 732.0 §4.1.5.4–5; 732.1 §4.1.5.2.1–2 | V |
| CMN-011 | CLCW field decoding SHALL be delegated to the TC codec ([tc-frame.md §5](tc-frame.md#5-clcw)). `aldis-core` treats the CLCW as opaque except for bit 0. | 232.0 §4.2 | I |

## 3. Only-Idle-Data (OID) pattern

TM (132.0 §4.1.4.6.2), AOS (732.0 §4.1.4.1.5.2), and USLP (732.1 §4.1.4.1.10) require the data field of an OID Transfer Frame to be filled with a pseudo-noise sequence. The sequence comes from a 32-cell LFSR with polynomial D⁰ + D¹ + D² + D²² + D³². It is initialized **once at device start-up** and **never restarted**: the sequence continues across successive OID frames.

Reference formulation (Fibonacci form, all-ones seed). Output bit xₙ, with x₀ … x₃₁ = 1:

```
x[n] = x[n-1] XOR x[n-2] XOR x[n-22] XOR x[n-32]      for n ≥ 32
```

The first 20 output octets are `FF FF FF FF 6D B6 D8 61 45 1F 11 F1 97 16 72 3C BE 7E 00 B1`, as printed in 132.0 Annex D and reproduced by the formulation above during spec preparation.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CMN-020 | `OidGenerator` SHALL produce the sequence above, MSB first within each octet, starting at device start-up state. | 132.0 §4.1.4.6.2 & Annex D; 732.0 §4.1.4.1.5.2 & Annex D; 732.1 §4.1.4.1.10 & Annex H | V |
| CMN-021 | The generator state SHALL persist across frames. A generator SHALL be owned by the physical channel multiplexer, one per physical channel, and SHALL NOT be reset by engine reconfiguration. A full engine re-initialization, representing a "device start-up", MAY reset it. A decoder is stateless and so does not check the data of a received OID frame against the sequence (TM-010, AOS-016, USLP-009); receivers discard it. | 132.0 §4.1.4.6.2.1; 732.0 §4.1.4.1.5.2; 732.1 §4.1.4.1.10 | T |
| CMN-022 | A Galois-form implementation, if provided, SHALL use seed `0b00000000001111111111111111111101` and SHALL produce identical output. | 132.0 §4.1.4.6.2.2; 732.1 Annex H | V |
| CMN-023 | `OidGenerator::fill(buf)` SHALL write the next `buf.len()` octets of the sequence and advance by exactly that amount. | — | T |
| CMN-024 | Receivers SHALL NOT be required to verify OID content. A `verify_oid` utility MAY be provided for test equipment. | — | — |

## 4. Frame counters

| Protocol | Counter | Width | Modulus |
|---|---|---|---|
| TM | Master Channel Frame Count | 8 bits | 256 |
| TM | Virtual Channel Frame Count | 8 bits | 256 |
| AOS | VC Frame Count (+ optional VC Frame Count Cycle) | 24 bits (+4) | 2²⁴ (2²⁸ with cycle) |
| USLP | VCF Count | 0–56 bits (per header field) | 2^(8·n) |
| TC | Frame Sequence Number N(S) | 8 bits | 256 (COP-1 manages it, see [cop1.md](cop1.md)) |

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CMN-030 | `aldis-core` SHALL provide a modular counter type parameterized by width (1–64 bits). It SHALL support increment, `distance(prev, next)`, and gap classification: `InSequence`, `Gap{missing}`, `Repeat`, `Backwards`. | — | T/V |
| CMN-031 | Gap classification SHALL treat a forward distance of 1 as in-sequence, 0 as repeat, a distance in `2..=modulus/2` as a gap, and anything else as backwards or reset. | — | T |

## 5. Field placement with SDLS

All frame codecs use one layout calculator (`FrameLayout`) that computes the octet ranges of every field. Its inputs are the protocol, the configuration, the header flags, and the `SecurityLayout` (see [03 §7](../03-api-model.md#7-security-hooks-sdls-reservation)).

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CMN-040 | `FrameLayout` SHALL return non-overlapping, contiguous ranges, in order, for: primary header, secondary header / insert zone, security header, data field, security trailer, OCF, FECF. The ranges SHALL cover the frame exactly. | 132.0 §6.3; 732.0 §6.3; 732.1 §6.3 | T/F |
| CMN-041 | If the layout leaves a data field of zero or negative length, the configuration SHALL be rejected with `InvalidConfig`. A project rule: the Blue Books state no minimum data field length. | — | T |
