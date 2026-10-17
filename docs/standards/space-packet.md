# Space Packet — CCSDS 133.0-B-2

**Crate:** `aldis-spp` · **Reference:** `specs/pdf/133x0b2e2.pdf` ([how to download](../../specs/README.md))

## 1. Wire format (§4.1)

| Bits | Field | Size | Values |
|---|---|---|---|
| 0–2 | Packet Version Number | 3 | `000` |
| 3 | Packet Type | 1 | 0 = telemetry/reporting, 1 = telecommand/requesting |
| 4 | Secondary Header Flag | 1 | 1 = secondary header present |
| 5–15 | APID | 11 | 0–2047; 2047 (all ones) = Idle Packet |
| 16–17 | Sequence Flags | 2 | `00` continuation, `01` first, `10` last, `11` unsegmented |
| 18–31 | Packet Sequence Count or Packet Name | 14 | 0–16383 |
| 32–47 | Packet Data Length | 16 | (octets in Packet Data Field) − 1 |
| 48… | Packet Data Field | 1–65 536 octets | [Secondary Header] ‖ [User Data] |

Total packet length is 7 to 65 542 octets (§4.1.2.2).

## 2. Requirements

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| SPP-001 | The decoder SHALL reject a version number ≠ `000` with `UnsupportedVersion`. | §4.1.3.2.2 | V |
| SPP-002 | The decoder SHALL compute total length = 7 + Packet Data Length. It SHALL return `Incomplete{needed}` if the buffer is shorter, and SHALL consume exactly that length if the buffer is longer (API-011). | §4.1.3.5 | V |
| SPP-003 | `peek_length` SHALL need only the first 6 octets. | §4.1.3.5 | T |
| SPP-004 | For an Idle Packet (APID = 2047), strict mode SHALL reject Secondary Header Flag = 1. | §4.1.3.3.3.4 | V |
| SPP-005 | The encoder SHALL reject an empty Packet Data Field. It SHALL reject Secondary Header Flag = 0 with an empty User Data Field, and Secondary Header Flag = 1 with a zero-length secondary header. | §4.1.4.1.2, §4.1.4.2.1.2, §4.1.4.3.2 | V |
| SPP-006 | The encoder SHALL reject a total packet length above the configured `max_packet_len` (managed parameter, ≤ 65 542) with `OutOfRange`. | §5.2 Table 5-1 | T |
| SPP-007 | Secondary header length is a managed parameter per APID. When it is supplied, the view SHALL split the Packet Data Field into `secondary_header()` and `user_data()`; otherwise it SHALL expose only `data_field()`. | §4.1.4.2.1.4 | T |
| SPP-008 | When a secondary header layout of `TimeCode(Format) [+ ancillary len]` is configured, the view SHALL decode the time code via `aldis-time` (feature `time`). A P-field SHALL be expected exactly when the configuration says so. | §4.1.4.2.1.5, §4.1.4.2.2.4–5 | T |
| SPP-009 | The Packet Type SHALL be exposed. The 14-bit field SHALL be exposed as `sequence_count()` for both types, with an alias `packet_name()` for telecommand packets, without reinterpreting the bits. | §4.1.3.4.3.2 | T |
| SPP-010 | An `IdlePacket` builder SHALL produce an Idle Packet of any requested total length ≥ 7, with APID 2047, Secondary Header Flag 0, Sequence Flags `11`, and a caller-supplied idle pattern (default `0x00`). | §4.1.3.3.4.4, §4.1.4.3.4 | V |
| SPP-011 | A `SequenceCounter` helper SHALL generate per-APID counts modulo 16384. A `SequenceTracker` SHALL classify received counts per APID as in-sequence, gap (with count of missing packets), repeat, or reset (CMN-030 semantics). Capacity (number of APIDs tracked) is compile-time. | §4.1.3.4.3.3–4, §4.3.2.2 | T/V |
| SPP-012 | The Octet String service SHALL be provided as an encoder that builds the primary header from (APID, packet type, secondary-header indicator, octet string), using a `SequenceCounter`. It SHALL always set Sequence Flags = `11`. | §3.4, §4.1.3.4.2.3, §4.2.2 | T |
| SPP-013 | A segmentation helper SHALL split an SDU across consecutive packets of one APID with flags `01`/`00`/`10`, and a reassembly helper SHALL rejoin them, detecting sequence gaps. Both are utilities: the protocol does not mandate segmentation. | §4.1.3.4.2 note | T |

## 3. Anomaly flags (lenient mode)

`SppAnomaly` flags. The bit index is the flag's position in `AnomalySet::bits()` and is shared with the C ABI. It never changes. Version ≠ 0 remains an error in both modes.

| Bit | Flag | Deviation | Source |
|---|---|---|---|
| 0 | `IdleWithSecondaryHeader` | Idle Packet (APID 2047) with Secondary Header Flag = 1 (SPP-004) | 133.0 §4.1.3.3.3.4 |

## 4. Test-vector categories

`spp/decode_valid`, `spp/decode_invalid` (each error), `spp/encode` (round trip), `spp/idle`, `spp/sequence_tracker`, `spp/secondary_header_time` (with `aldis-time`), `spp/stream` (several concatenated packets in one buffer).
