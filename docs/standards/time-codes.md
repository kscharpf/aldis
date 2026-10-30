# Time codes — CCSDS 301.0-B-4

**Crate:** `aldis-time` · **Reference:** `specs/pdf/301x0b4e1.pdf` ([how to download](../../specs/README.md)) · **Acronyms:** [glossary](../glossary.md)

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
| TIME-002 | Time code IDs `000`, `011`, and `111` SHALL be rejected as `ReservedValue` in strict mode. In lenient mode the decoder SHALL return the P-field as an unknown time code, raise the anomaly ReservedTimeCodeId, and report only the P-field octets as consumed, because the T-field length is not defined; the caller parses the T-field (OI-15). | 301.0 §3.1.1 | V |
| TIME-003 | Decoders SHALL report the total consumed length (P-field + T-field; P-field only for an unknown time code, TIME-002) so a time code can be followed by other data, e.g. in a Space Packet secondary header. | 133.0 §4.1.4.2.2 | T |
| TIME-004 | No time code operation SHALL depend on the host clock or time zone (GEN-031). | — | I |
| TIME-005 | P-field extension octets that 301.0 does not define (CUC octet 3 and later; octet 2 and later of CDS, CCS, and agency-defined codes) SHALL be `ReservedValue` in strict mode. In lenient mode the decoder SHALL keep them raw for round trip, count them in the consumed length, and raise the anomaly UndefinedPFieldExtension. A P-field SHALL be at most 8 octets; a longer chain SHALL be `Malformed` in both modes (OI-18). | 301.0 §3.1.1 | V |
| TIME-006 | An agency-defined code (time code ID `110`) SHALL be decoded as its raw P-field and a raw T-field of (P-field bits 4–7) + 1 octets, 1 to 16. | 301.0 §3.6 | V |

## 3. CUC — CCSDS Unsegmented Code (§3.2)

P-field octet 1: ID `001` (Level 1, epoch 1958-01-01 TAI) or `010` (Level 2, agency-defined epoch). Bits 4–5 hold the number of basic (coarse) time octets minus 1. Bits 6–7 hold the number of fractional octets.
P-field octet 2 (optional): bits 1–2 add more coarse octets; bits 3–5 add more fractional octets; bits 6–7 are mission-defined.
The encoder puts as many octets as it can in octet 1 (up to 4 coarse and 3 fractional) and the rest in octet 2. A received P-field that splits the counts differently decodes to the same format; the decoder also returns the P-field exactly as received.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| TIME-010 | The CUC format SHALL support 1–7 coarse octets and 0–10 fractional octets. Values above 4 coarse or 3 fractional octets require P-field octet 2. | 301.0 §3.2 | V |
| TIME-011 | A CUC value SHALL hold coarse time as a 64-bit unsigned integer and fine time as a left-aligned binary fraction (up to 80 bits). Conversion **to** `(seconds, nanoseconds)` SHALL truncate toward zero. Conversion **from** nanoseconds SHALL also truncate, rounding the fraction down to a value of the format (OI-40); the fraction then stays below one second, so nothing carries into the coarse time. With these rules a round trip never gives a later time, and the following SHALL be documented. The conversion from a CUC value to nanoseconds and back is exact if and only if the value is a multiple of 2⁻⁹ s: always for 0 and 1 fractional octets (2⁻⁸ s is exactly 3 906 250 ns), and otherwise one unit of the last fractional octet earlier for 2 or 3 octets. With 4 or more fractional octets, finer than a nanosecond, the conversion from nanoseconds to a CUC value and back gives the nanoseconds back if they are a multiple of 1 953 125 (2⁻⁹ s) and one fewer otherwise. The conversions SHALL take the basic time unit to be the second, the unit of the CCSDS codes (301.0 §1.2) and the one §3.2.1 recommends, and SHALL document that for a Level 2 code they give the time in seconds only if the agency's unit is the second (#82). | 301.0 §1.2, §3.2.1 | T/V |
| TIME-012 | Encoding a value that does not fit the configured coarse width SHALL fail with `OutOfRange`. It SHALL NOT wrap. | — | T |
| TIME-013 | The mission-defined bits (octet 2 bits 6–7) SHALL be preserved on round trip. | 301.0 §3.2 | V |
| TIME-014 | Level 1 CUC SHALL be documented as TAI-based with no leap seconds. The library SHALL NOT apply leap-second corrections to CUC. | 301.0 §3.2 | I |

## 4. CDS — CCSDS Day Segmented Code (§3.3)

P-field: ID `100`. Bit 4 is the epoch (0 = 1958-01-01, 1 = agency). Bit 5 is the day segment length (0 = 16-bit, 1 = 24-bit). Bits 6–7 give the sub-millisecond segment: `00` none, `01` 16-bit µs, `10` 32-bit ps, `11` reserved.
T-field: day (16/24 bits) ‖ milliseconds of day (32 bits) ‖ sub-milliseconds (0/16/32 bits).

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| TIME-020 | The CDS codec SHALL implement all valid combinations of day length (16/24) and sub-millisecond resolution (none/µs/ps). Sub-ms code `11` SHALL be `ReservedValue` in both modes: it sets the segment length, which is then unknown (OI-15). | 301.0 §3.3.2 | V |
| TIME-021 | Decoding SHALL reject, in both modes, milliseconds-of-day ≥ 86 401 000 (allowing one positive leap second), µs sub-segment ≥ 1000, and ps sub-segment ≥ 10⁹; encoding SHALL reject the same values (OI-18). | 301.0 §3.3, Annex A | V |
| TIME-022 | CDS SHALL be documented as UTC-based (leap seconds apply). Leap-second handling is the caller's responsibility in v1 (see §6). | 301.0 §3.3 | I |

## 5. CCS — Calendar Segmented Code (§3.4) and ASCII (§3.5)

CCS P-field: ID `101`. Bit 4 is the calendar variation (0 = month/day, 1 = day-of-year). Bits 5–7 give the number of sub-second BCD octets, 0–6 (`111` not used). T-field: BCD year (16 bits) ‖ (month, day-of-month | day-of-year 16 bits, top 4 bits zero) ‖ hour ‖ minute ‖ second ‖ 0–6 octets of 10⁻² … 10⁻¹² s.

ASCII Time Code A `YYYY-MM-DDThh:mm:ss.d…dZ` and B `YYYY-DDDThh:mm:ss.d…dZ`. The fraction has one or more digits, and the trailing `Z` is optional.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| TIME-030 | The CCS codec SHALL validate every BCD nibble (0–9) and every calendar range: year 1–9999, month 1–12, day valid for the month and year (leap years), DOY 1–366 (366 only in leap years), hour 0–23, minute 0–59, second 0–60. Resolution code `111` ("not used") SHALL be `ReservedValue` in both modes, because it sets the T-field length (OI-15). | 301.0 §3.4, Annex A | V |
| TIME-031 | The ASCII codec SHALL parse and format both variations with 0 to 12 fractional digits (configurable maximum). It SHALL accept the optional `Z` and preserve its presence on round trip. Seconds value 60 SHALL be accepted (leap second). | 301.0 §3.5.1.1–2 | V |
| TIME-032 | ASCII subsets (calendar-only, time-only, truncated subfields, §3.5.1.3) SHALL be supported for **parsing** behind an explicit subset option. Formatting subsets is optional in v1. | 301.0 §3.5.1.3 | V |
| TIME-033 | The library SHALL provide conversions between CDS, CCS, and ASCII representations of the same UTC instant, with no time-scale change. When the target format has coarser sub-second resolution than the source, the conversion SHALL truncate toward zero. A value the target cannot represent (before the CDS epoch, beyond the CDS day count, or outside years 1–9999) SHALL be `OutOfRange` (OI-18). | — | V |
| TIME-034 | In the CCS day-of-year variation, a non-zero value in the four most significant bits of the 16-bit DOY segment SHALL be `ReservedValue` in strict mode and SHALL raise the anomaly NonZeroDoyHighNibble in lenient mode. The encoder SHALL write them as zero (OI-15). | 301.0 §3.4.1.2 | V |

### 5.1 ASCII grammar (TIME-031, TIME-032; OI-29)

**Complete codes** (TIME-031): A is `YYYY-MM-DDThh:mm:ss[.d…][Z]` and B is `YYYY-DDDThh:mm:ss[.d…][Z]`. Every subfield has exactly its digit count, with leading zeros. With zero fractional digits the period is absent; a period SHALL be followed by at least one digit. The fraction has at most a configurable maximum of 0–12 digits (default 12); more is `OutOfRange` (`fraction`). `T` and `Z` are upper case. The input SHALL be exactly the code, with no leading or trailing octets. Errors: a separator, length, or trailing-octet problem is `Malformed`; a non-digit in a digit subfield is `InvalidField` naming the subfield; a value out of range (year 1–9999, day valid for the month or year, hour, minute, second 0–60) is `OutOfRange`. Formatting writes the same grammar into a caller buffer without `core::fmt`; the longest code is 33 octets.

**Subsets** (TIME-032, parsing only, through an explicit subset parser) follow 301.0 §3.5.1.3:
- The calendar subfields are `YYYY`, `MM`, `DD` (A) or `YYYY`, `DDD` (B), separated by `-`; the time subfields are `hh`, `mm`, `ss`, and the fraction, separated by `:`, `:`, `.`.
- A subset keeps a contiguous run of subfields. A subfield deleted on the left keeps the separator that followed it (`-MM-DD`, `--DD`, `-MM`, `:mm:ss`, `::ss.d`); one deleted on the right drops its separator (`YYYY-MM`, `hh:mm`).
- A calendar subset alone or a time subset alone has no `T`. Joined with `T`, the calendar part SHALL end with its last subfield (`DD` or `DDD`) and the time part SHALL start with `hh`.
- A after `YYYY-`, two digits are a month and three a day of year; a left-truncated `-DDD` and `-MM` are told apart the same way.
- `Z` is allowed only when a time part is present (OI-29).
- Ranges that need missing context use the widest case: a day of month without a month is 1–31; with a month but no year it may be the 29th of February; a day of year without a year is 1–366.

### 5.2 Conversions (TIME-033; D3, OI-18, OI-29)

Conversions go through an exact UTC instant with picosecond resolution, the finest of the three codes, so only the target's resolution can lose precision; it truncates toward zero. The CDS day count is days since 1958 January 1 (Level 1). A CDS code with an agency-defined epoch converts only when the caller supplies the epoch date (D3); otherwise `InvalidConfig`. Milliseconds of day 86 400 000–86 400 999 convert to and from 23:59:60; second 60 at any other minute is `OutOfRange` (`second`) for a CDS target, because CDS cannot represent it (OI-29), while CCS and ASCII carry it unchanged. A date before the CDS epoch, a day count beyond the target's day segment, or a year above 9999 is `OutOfRange` (TIME-033). Month/day and day-of-year forms convert exactly into each other.

## 6. Time scales (deferred)

TAI↔UTC conversion requires a leap-second table that changes over a mission's lifetime. v1 SHALL NOT embed such a table. A `LeapSecondTable` trait, with caller-supplied data, is reserved for v1.x ([open issues](../09-open-issues.md)).

## 7. Anomaly flags (lenient mode)

`TimeAnomaly` flags. The bit index is the flag's position in `AnomalySet::bits()` and is shared with the C ABI. It never changes.

| Bit | Flag | Deviation | Source |
|---|---|---|---|
| 0 | `ReservedTimeCodeId` | P-field time code ID `000`, `011`, or `111`; only the P-field is consumed (TIME-002) | 301.0 §3.1.1 |
| 1 | — | Reserved, never raised. Was `ReservedSubmsCode`, withdrawn by OI-15 before release: CDS sub-ms code `11` is an error in both modes (TIME-020) | — |
| 2 | `NonZeroDoyHighNibble` | CCS day-of-year segment with a non-zero high nibble (TIME-034) | 301.0 §3.4.1.2 |
| 3 | `UndefinedPFieldExtension` | P-field extension octet that 301.0 does not define for the code; kept raw (TIME-005) | 301.0 §3.1.1 |

## 8. Test-vector categories

The case groups are the directories of `test-vectors/` for this standard; see [test vectors](../07-test-vectors.md).
