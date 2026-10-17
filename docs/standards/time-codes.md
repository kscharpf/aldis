# Time codes — CCSDS 301.0-B-4

**Crate:** `aldis-time` · **Reference:** `specs/pdf/301x0b4e1.pdf` ([how to download](../../specs/README.md))

## 1. Scope

The four standard time codes: CUC, CDS, CCS, and ASCII (A and B). Each binary code supports an explicit **P-field** (preamble) or an implicit one supplied by configuration. Agency-defined codes (§3.6) are representable only as raw P-field/T-field octets.

## 2. Common model

```
TimeCode = Cuc(CucFormat, CucValue) | Cds(CdsFormat, CdsValue) | Ccs(CcsFormat, CcsValue) | Ascii(AsciiFormat, AsciiValue)
decode_with_pfield(bytes)            -> (TimeCode, consumed)     // P-field present in data
decode_implicit(bytes, &Format)      -> (TimeCode, consumed)     // P-field known from management
encode(&TimeCode, include_pfield, out) -> len
```

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| TIME-001 | P-field octet 1 SHALL be decoded as: bit 0 extension flag; bits 1–3 time code ID; bits 4–7 code-specific detail. If the extension flag is set, the following octet SHALL be part of the P-field, which repeats while each octet's bit 0 is set. | 301.0 §3.1 | V |
| TIME-002 | Time code IDs `000`, `011`, and `111` SHALL be rejected as `ReservedValue` in strict mode, and decoded as `Agency/Unknown` raw in lenient mode. | 301.0 §3.1 | V |
| TIME-003 | Decoders SHALL report the total consumed length (P-field + T-field) so a time code can be followed by other data, e.g. in a Space Packet secondary header. | 133.0 §4.1.4.2.2 | T |
| TIME-004 | No time code operation SHALL depend on the host clock or time zone (GEN-031). | — | I |

## 3. CUC — CCSDS Unsegmented Code (§3.2)

P-field octet 1: ID `001` (Level 1, epoch 1958-01-01 TAI) or `010` (Level 2, agency-defined epoch). Bits 4–5 hold the number of basic (coarse) time octets minus 1. Bits 6–7 hold the number of fractional octets.
P-field octet 2 (optional): bits 1–2 add more coarse octets; bits 3–5 add more fractional octets; bits 6–7 are mission-defined.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| TIME-010 | `CucFormat` SHALL support 1–7 coarse octets and 0–10 fractional octets. Values above 4 coarse or 3 fractional octets require P-field octet 2. | 301.0 §3.2 | V |
| TIME-011 | `CucValue` SHALL hold coarse time as u64 and fine time as a left-aligned binary fraction (up to 80 bits). Conversion **to** `(seconds, nanoseconds)` SHALL truncate toward zero. Conversion **from** nanoseconds SHALL round the fraction up (ceiling). With these rules the round trip `CucValue → ns → CucValue` is exact for 0–3 fractional octets (≤ 29 fractional bits). For 4 or more fractional octets it is exact only to the nanosecond, and this SHALL be documented. | — | T/V |
| TIME-012 | Encoding a value that does not fit the configured coarse width SHALL fail with `OutOfRange`. It SHALL NOT wrap. | — | T |
| TIME-013 | The mission-defined bits (octet 2 bits 6–7) SHALL be preserved on round trip. | 301.0 §3.2 | V |
| TIME-014 | Level 1 CUC SHALL be documented as TAI-based with no leap seconds. The library SHALL NOT apply leap-second corrections to CUC. | 301.0 §3.2 | I |

## 4. CDS — CCSDS Day Segmented Code (§3.3)

P-field: ID `100`. Bit 4 is the epoch (0 = 1958-01-01, 1 = agency). Bit 5 is the day segment length (0 = 16-bit, 1 = 24-bit). Bits 6–7 give the sub-millisecond segment: `00` none, `01` 16-bit µs, `10` 32-bit ps, `11` reserved.
T-field: day (16/24 bits) ‖ milliseconds of day (32 bits) ‖ sub-milliseconds (0/16/32 bits).

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| TIME-020 | The CDS codec SHALL implement all valid combinations of day length (16/24) and sub-millisecond resolution (none/µs/ps). Sub-ms code `11` SHALL be `ReservedValue`. | 301.0 §3.3 | V |
| TIME-021 | Strict decoding SHALL reject milliseconds-of-day ≥ 86 401 000 (allowing one positive leap second), µs sub-segment ≥ 1000, and ps sub-segment ≥ 10⁹. | 301.0 §3.3 (UTC-based) | V |
| TIME-022 | CDS SHALL be documented as UTC-based (leap seconds apply). Leap-second handling is the caller's responsibility in v1 (see §6). | 301.0 §3.3 | I |

## 5. CCS — Calendar Segmented Code (§3.4) and ASCII (§3.5)

CCS P-field: ID `101`. Bit 4 is the calendar variation (0 = month/day, 1 = day-of-year). Bits 5–7 give the number of sub-second BCD octets, 0–6 (`111` not used). T-field: BCD year (16 bits) ‖ (month, day-of-month | day-of-year 16 bits, top 4 bits zero) ‖ hour ‖ minute ‖ second ‖ 0–6 octets of 10⁻² … 10⁻¹² s.

ASCII Time Code A `YYYY-MM-DDThh:mm:ss.d…dZ` and B `YYYY-DDDThh:mm:ss.d…dZ`. The fraction has one or more digits, and the trailing `Z` is optional.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| TIME-030 | The CCS codec SHALL validate every BCD nibble (0–9) and every calendar range: month 1–12, day valid for the month and year (leap years), DOY 1–366, hour 0–23, minute 0–59, second 0–60. | 301.0 §3.4 | V |
| TIME-031 | The ASCII codec SHALL parse and format both variations with 0 to 12 fractional digits (configurable maximum). It SHALL accept the optional `Z` and preserve its presence on round trip. Seconds value 60 SHALL be accepted (leap second). | 301.0 §3.5.1.1–2 | V |
| TIME-032 | ASCII subsets (calendar-only, time-only, truncated subfields, §3.5.1.3) SHALL be supported for **parsing** behind an explicit `AsciiSubset` option. Formatting subsets is optional in v1. | 301.0 §3.5.1.3 | V |
| TIME-033 | The library SHALL provide conversions between CDS, CCS, and ASCII representations of the same UTC instant, with no time-scale change. | — | V |

## 6. Time scales (deferred)

TAI↔UTC conversion requires a leap-second table that changes over a mission's lifetime. v1 SHALL NOT embed such a table. A `LeapSecondTable` trait, with caller-supplied data, is reserved for v1.x ([open issues](../09-open-issues.md)).

## 7. Anomaly flags (lenient mode)

`TimeAnomaly::{ReservedTimeCodeId, ReservedSubmsCode, NonZeroDoyHighNibble}`
