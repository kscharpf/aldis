# Space Packet — CCSDS 133.0-B-2

**Crate:** `aldis-spp` · **Reference:** `specs/pdf/133x0b2e2.pdf` ([how to download](../../specs/README.md)) · **Acronyms:** [glossary](../glossary.md)

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
| SPP-004 | For an Idle Packet (APID = 2047), strict mode SHALL reject Secondary Header Flag = 1; lenient mode SHALL accept it and raise `IdleWithSecondaryHeader` (API-032, OI-16). | §4.1.3.3.3.4 | V |
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

`spp/decode_valid`, `spp/decode_invalid` (each error), `spp/encode` (round trip), `spp/idle`, `spp/sequence_tracker`, `spp/secondary_header_time` (with `aldis-time`), `spp/stream` (several concatenated packets in one buffer), `spp/octet_string` (SPP-012), `spp/split` (opaque secondary header, SPP-007).

## 5. Behavior details (OI-30)

These rules fill in what the requirements leave to the implementation. 133.0 is silent on them, or they are decisions recorded in OI-30.

### 5.1 Decoding (SPP-001…004, SPP-009)

- **Order of checks:** fewer than 6 octets → `Incomplete` (`needed` = 6); version ≠ `000` → `UnsupportedVersion`; fewer than 7 + Packet Data Length octets → `Incomplete` (`needed` = that total); then the idle check of SPP-004.
- **SPP-004 in strict mode:** `InvalidField` (`secondary_header_flag`).
- **`peek_length`:** applies every check that needs only the 6-octet primary header and holds in both modes, so it never disagrees with `decode` on a header.
- **`packet_name()`:** returns the 14-bit field for telecommand packets and nothing for telemetry packets; `sequence_count()` returns it for both (SPP-009).

### 5.2 Encoding (SPP-005, SPP-006, SPP-010, SPP-012)

- **Secondary Header Flag:** the encoder takes the secondary header and the user data separately and sets the flag from whether the secondary header is present. A flag of 1 with a zero-length secondary header cannot be expressed (SPP-005, API-021). An empty Packet Data Field is `InvalidField` (`data_field`).
- **Idle Packets:** a packet with APID 2047 and a secondary header is `InvalidField` (`secondary_header_flag`) in every encoder (§4.1.3.3.3.4).
- **`max_packet_len`:** a managed parameter of 7–65 542 octets (default 65 542); a larger packet is `OutOfRange` (`packet_len`, SPP-006).
- **Two-phase writer, before `finish`:** the version field holds `111` and the Packet Data Length is 0, so every decoder rejects the buffer with `UnsupportedVersion` (API-023).
- **Idle builder:** sets APID 2047, Secondary Header Flag 0, Sequence Flags `11`, a caller-supplied sequence count (default 0), and fills the User Data Field by repeating a caller-supplied pattern (default the single octet `0x00`; SPP-010).
- **Octet String encoder:** sets the Secondary Header Flag from the Secondary Header Indicator (§3.4.2.3) and the Sequence Flags to `11` (§4.1.3.4.2.3). It takes the count from its `SequenceCounter`, which advances only when a packet is written (SPP-012). APID 2047 is `InvalidField` (`apid`): Idle Packets come from the idle builder.

### 5.3 Sequence counts (SPP-011; OI-30 point 3)

- **`SequenceCounter`:** yields the current count and advances modulo 16 384 (GEN-024). Use one counter per APID.
- **`SequenceTracker<N>`:** tracks up to N APIDs.
  - The first packet of an APID returns `First`.
  - Later packets return the `aldis-core` `Continuity` of CMN-031; `Backwards` is the "reset" of SPP-011.
  - A new APID while N APIDs are tracked is `CapacityExceeded` and changes nothing; `forget(apid)` frees a slot.

### 5.4 Segmentation and reassembly (SPP-013; OI-30 point 4)

- **Segmenter:** splits a non-empty SDU into User Data Fields of at most a given length. A single segment is sent as `11`; otherwise the flags are `01`, `00`…, `10`. Each packet takes the next count of the caller's `SequenceCounter`, which advances only when a packet is written, and may carry a secondary header the caller supplies per packet.
  - An empty SDU is `InvalidField` (`sdu`); a maximum of 0 user data octets is `OutOfRange` (`max_user_len`).
  - APID 2047 is `InvalidField` (`apid`): an Idle Packet carries Idle Data, not user data (§4.1.4.3.4).
  - Each packet is checked like any other (SPP-005, SPP-006).
- **Reassembler:** handles one APID and assembles into the caller's buffer. A packet of another APID is `InvalidField` (`apid`). Otherwise the reassembler never fails or gets stuck: the partial SDU is discarded and the outcome says why, then the current packet is handled as if nothing were in progress:

| Situation | Discard reason | Current packet |
|---|---|---|
| Sequence gap (any continuity but `InSequence`) while assembling | `Gap` | `01` starts a new SDU; `11` is delivered; `00`/`10` is dropped |
| `01` while assembling | `Restarted` | starts a new SDU |
| `11` while assembling | `Interrupted` | delivered whole |
| `00` or `10` with nothing in progress | `Orphan` | dropped |
| SDU would exceed the buffer | `Overflow` | dropped, with the partial SDU |

The outcome of each packet has two parts:
- **Discarded:** why a partial SDU was thrown away, if one was: `Gap`, `Restarted`, `Interrupted`, or `Overflow`.
- **Status of the current packet:** in progress, complete (the SDU length; the SDU stays readable until the next packet), or dropped with its reason. The reason is `Orphan` for a `00` or `10` with nothing in progress, which includes one whose SDU a gap has just discarded, or `Overflow`.

Both helpers borrow a buffer: the segmenter its SDU, the reassembler its reassembly buffer. A caller that cannot keep the borrow from one call to the next, such as the C ABI, suspends a helper into a state without the buffer (`SegmenterState`, `ReassemblerState`) and resumes it with the same buffer on every call; nothing keeps a reference between calls (API-060; [M2 plan](../milestones/m2-frame-codecs.md) D5). Resuming a segmenter with an SDU of another length is `LengthMismatch` (`sdu`); resuming a reassembler with a buffer shorter than the SDU in progress is `BufferTooSmall`, with `needed`.

### 5.5 Secondary header layouts (SPP-007, SPP-008; OI-30 points 1 and 2)

- **No configuration in the decoder:** `decode` takes no configuration and exposes the whole Packet Data Field. The caller looks up the APID's layout and splits the packet with it.
- **Layouts:** none, opaque (a fixed length ≥ 1), or a time code (feature `time`) with an optional P-field and an ancillary length.
- **Flag mismatch:** a flag that disagrees with the layout (flag 1 with no layout, flag 0 with a layout) is `InvalidField` (`secondary_header_flag`) in both modes, since §4.1.3.3.3.3 fixes the flag per APID.
- **Too short:** a data field shorter than the secondary header is `LengthMismatch` (`secondary_header`). This includes a data field that ends inside the time code's P-field or T-field. The `expected` length includes the Ancillary Data Field; when the data field ends inside a P-field extension, the time code's length is not yet known and `expected` is a lower bound.
- **Time codes:** only CUC and CDS are allowed (133.0 §4.1.4.2.2.2: the "segmented binary or unsegmented binary" codes of 301.0; CCS is BCD). Configuring another code is `InvalidConfig` (`time_code`).
- **With a P-field:** the code's characteristics come from the P-field (§4.1.4.2.2.4), but its time code ID must equal the configured code's (§4.1.4.2.2.3); otherwise `InvalidField` (`time_code_id`).
- **Without a P-field:** the configured format is used.
- **Ancillary Data Field:** the configured ancillary length follows the time code. With an opaque layout, the whole secondary header counts as ancillary data (§4.1.4.2.1.5 b).
- **Time code errors:** the other errors of the time code decoder, such as `OutOfRange` for a CDS millisecond of day above 86 400 999, are returned as they are, under the packet's decode mode.
