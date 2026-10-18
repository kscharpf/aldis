# Encapsulation Packet — CCSDS 133.1-B-3

**Crate:** `aldis-epp` · **Reference:** `specs/pdf/133x1b3e1.pdf` ([how to download](../../specs/README.md))

## 1. Wire format (§4.1)

| Length of Length (bits 6–7) | Header length | Fields after octet 0 | Packet length range |
|---|---|---|---|
| `00` | 1 octet | none (Idle Packet only) | exactly 1 |
| `01` | 2 octets | Packet Length (1 octet) | 2 – 255 |
| `10` | 4 octets | User Defined (4 b) ‖ Protocol ID Extension (4 b) ‖ Packet Length (2 octets) | 4 – 65 535 |
| `11` | 8 octets | User Defined (4 b) ‖ Protocol ID Extension (4 b) ‖ CCSDS Defined (2 octets) ‖ Packet Length (4 octets) | 8 – 4 294 967 295 |

Octet 0 holds the Packet Version Number `111` (bits 0–2), the Encapsulation Protocol ID (EPI, bits 3–5), and the Length of Length (bits 6–7).
EPI `000` is the Idle Packet, `110` means "use the Protocol ID Extension", and `111` means mission-specific. The other values are assigned by SANA.
**Unlike the Space Packet, Packet Length is the *total* packet length including the header** (§4.1.2.8.2).

## 2. Requirements

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| EPP-001 | The decoder SHALL reject a version ≠ `111` with `UnsupportedVersion`. | §4.1.2.2.2 | V |
| EPP-002 | The decoder SHALL accept all four header lengths, whether a stream uses one fixed size or a mix (variable). | §4.1.2.1.2 | V |
| EPP-003 | The decoder SHALL reject Packet Length < header length, or Packet Length > available octets (`Incomplete`). | §4.1.2.8.2, Table 4-2 | V |
| EPP-004 | If Length of Length = `00`, the decoder SHALL require EPI = `000` and SHALL report a 1-octet Idle Packet. | §4.1.2.4.4 | V |
| EPP-005 | If the Encapsulated Data Field is absent (Packet Length = header length), the decoder SHALL require EPI = `000` in both modes (OI-16). | §4.1.3.1.4–5 | V |
| EPP-006 | If EPI ≠ `110`, a non-zero Protocol ID Extension SHALL be an error in strict mode and an anomaly in lenient mode. | §4.1.2.6.3 | V |
| EPP-007 | A non-zero CCSDS Defined Field SHALL be an anomaly (lenient) or `ReservedValue` (strict). The value SHALL be exposed. | §4.1.2.7.2 | V |
| EPP-008 | The encoder SHALL support a `HeaderSize::{Auto, Fixed(1\|2\|4\|8)}` policy. `Auto` SHALL choose the smallest header that can represent the total length (with the 1-octet header reserved for idle). `Fixed` SHALL fail with `OutOfRange` if the length does not fit. | §4.1.2.1.2 note | V |
| EPP-009 | An `idle(total_len)` builder SHALL produce an Encapsulation Idle Packet of any total length ≥ 1. A length of 1 uses the 1-octet header; longer packets use the smallest header that fits, with EPI `000` and a caller-supplied idle pattern. | §4.1.2.3 note 1, §4.1.3.1.3 | V |
| EPP-010 | The configured minimum and maximum encapsulated data unit lengths and the valid EPI and extended-EPI sets (managed parameters) SHALL be enforced by a `validate(&EppConfig)` step. | §5 Table 5-1 | T |
| EPP-011 | The view SHALL expose EPI, extension, user-defined bits, CCSDS-defined field (if present), header length, total length, and the data slice. | §4.1.2 | T |

## 3. Anomaly flags (lenient mode)

`EppAnomaly` flags. The bit index is the flag's position in `AnomalySet::bits()` and is shared with the C ABI. It never changes.

| Bit | Flag | Deviation | Source |
|---|---|---|---|
| 0 | `NonZeroEpiExtension` | Non-zero Protocol ID Extension with EPI ≠ `110` (EPP-006) | 133.1 §4.1.2.6.3 |
| 1 | `NonZeroCcsdsDefined` | Non-zero CCSDS Defined Field (EPP-007) | 133.1 §4.1.2.7.2 |
| 2 | — | Reserved, never raised. Was `EmptyNonIdle`, withdrawn by OI-16 before release: an absent Encapsulated Data Field with EPI ≠ `000` is an error in both modes (EPP-005) | — |

## 4. Test-vector categories

`epp/header_1`, `epp/header_2`, `epp/header_4`, `epp/header_8`, `epp/idle`, `epp/auto_size_boundaries` (255/256, 65 535/65 536 total length), `epp/invalid`.
