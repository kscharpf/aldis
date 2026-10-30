# Encapsulation Packet — CCSDS 133.1-B-3

**Crate:** `aldis-epp` · **Reference:** `specs/pdf/133x1b3e1.pdf` ([how to download](../../specs/README.md)) · **Acronyms:** [glossary](../glossary.md)

## 1. Wire format (§4.1)

The field layout is that of the Blue Book clause cited in the heading, and of the rustdoc of the view and writer types; the vectors pin the octets.

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
| EPP-006 | If EPI ≠ `110`, a non-zero Protocol ID Extension SHALL be `ReservedValue` in strict mode and an anomaly in lenient mode. | §4.1.2.6.3 | V |
| EPP-007 | A non-zero CCSDS Defined Field SHALL be an anomaly (lenient) or `ReservedValue` (strict). The value SHALL be exposed. | §4.1.2.7.2 | V |
| EPP-008 | The encoder SHALL support a header-size policy: automatic, or fixed at 1, 2, 4, or 8 octets. Automatic SHALL choose the smallest header that can represent the total length (with the 1-octet header reserved for idle). A fixed size SHALL fail with `OutOfRange` if the length does not fit. The User Defined and Protocol ID Extension fields exist only in 4- and 8-octet headers, so EPI `110` (whose extension identifies the protocol) or non-zero user-defined bits SHALL make the automatic policy choose at least 4 octets, and a fixed size of 1 or 2 octets SHALL then fail with `InvalidField`. A non-zero extension with EPI ≠ `110` SHALL be unrepresentable in the encoder's fields (building the header from its parts rejects it with `InvalidField`), and the encoder SHALL reject an empty data field when EPI ≠ `000` with `InvalidField` (OI-17, RS-020). | §4.1.2.1.2 note; §4.1.2.1.1 d–e; §4.1.2.6.2–3; §4.1.3.1.5 | V |
| EPP-009 | An idle-packet builder SHALL produce an Encapsulation Idle Packet of any total length ≥ 1. A length of 1 uses the 1-octet header; longer packets use the smallest header that fits, with EPI `000` and a caller-supplied idle pattern. | §4.1.2.3 note 1, §4.1.3.1.3 | V |
| EPP-010 | The configured minimum and maximum encapsulated data unit lengths and the valid EPI and extended-EPI sets (managed parameters) SHALL be enforced by a validation step, which consumes the decoded packet and returns a validated packet (API-016). At the sending end the check SHALL be mandatory: the encoders SHALL accept only a data unit checked against the configuration, or the configuration itself (two-phase writer), so an unchecked user data unit cannot be encoded (maintainer decision 2026-10-19, issue #22). An Idle Packet carries no data unit and SHALL always pass. For EPI `110`, both `110` and the extension SHALL be in their sets. An EPI or extension outside its set SHALL be `UnknownChannel`; a data unit length outside the limits SHALL be `OutOfRange` (OI-17). | §5 Table 5-1; §4.2.1.1.2; §4.1.3.1.3; §4.3.1.1.3 | T |
| EPP-011 | The view SHALL expose EPI, extension, user-defined bits, CCSDS-defined field (if present), header length, total length, and the data slice. | §4.1.2 | T |
| EPP-012 | A 2-octet header with EPI `110` SHALL be `InvalidField` in both modes: the header has no Protocol ID Extension to identify the protocol. (A 1-octet header already requires EPI `000`, EPP-004.) (OI-17) | §4.1.2.1.1 e; §4.1.2.6.2 | V |

## 3. Anomaly flags (lenient mode)

`EppAnomaly` flags. The bit index is the flag's position in `AnomalySet::bits()` and is shared with the C ABI. It never changes.

| Bit | Flag | Deviation | Source |
|---|---|---|---|
| 0 | `NonZeroEpiExtension` | Non-zero Protocol ID Extension with EPI ≠ `110` (EPP-006) | 133.1 §4.1.2.6.3 |
| 1 | `NonZeroCcsdsDefined` | Non-zero CCSDS Defined Field (EPP-007) | 133.1 §4.1.2.7.2 |
| 2 | — | Reserved, never raised. Was `EmptyNonIdle`, withdrawn by OI-16 before release: an absent Encapsulated Data Field with EPI ≠ `000` is an error in both modes (EPP-005) | — |

## 4. Test-vector categories

The case groups are the directories of `test-vectors/` for this standard; see [test vectors](../07-test-vectors.md).
