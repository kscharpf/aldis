# 03 — Language-neutral API model

This document defines the **concepts** that every profile exposes. Profiles choose idiomatic names and signatures, such as Rust lifetimes or C pointer/length pairs, but SHALL keep the semantics defined here. Code sketches use neutral pseudo-notation. Concrete signatures are in [profiles/rust.md](profiles/rust.md) and [profiles/c-abi.md](profiles/c-abi.md).

## 1. Identifier and value types

Wire identifiers are distinct, range-checked types, never bare integers:

| Type | Range | Used by |
|---|---|---|
| `Apid` | 0 – 2047 (2047 = idle) | SPP |
| `SpacecraftId` | 0 – 1023 (TM, TC, AOS Issue 5); 0 – 255 (AOS Issue ≤4); 0 – 65535 (USLP) | frames |
| `VcId` | 0 – 7 (TM); 0 – 63 (TC, AOS, USLP; 63 = OID for AOS/USLP) | frames |
| `MapId` | 0 – 63 (TC Segment Header); 0 – 15 (USLP) | TC, USLP |
| `PacketVersion` | 0 (Space Packet), 7 (Encapsulation Packet), others per SANA | SPP, EPP, extraction |
| `SeqCount` | 0 – 16383, wrapping | SPP |
| `FrameSeqNum` | 0 – 255, wrapping | TC N(S), V(S), V(R) |
| `Instant` | u64 microseconds, monotonic, arbitrary epoch | engines |
| `Duration` | u64 microseconds | engines, configs |

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-001 | Constructing an identifier from an out-of-range integer SHALL fail with `OutOfRange`. Encoders SHALL accept only valid identifiers, so no encoder can emit an out-of-range field. | — | T |
| API-002 | `SpacecraftId` validity SHALL depend on the protocol and edition that uses it. Each frame codec SHALL define the identifier width it accepts. | 132.0 §4.1.2.2.3; 232.0 §4.1.2.5; 732.0 §4.1.2.2.3; 732.1 §4.1.2.2.3 | T |
| API-003 | `Instant` arithmetic SHALL saturate rather than wrap. A caller supplying a non-monotonic `Instant` SHALL NOT cause undefined behavior; engines SHALL treat time going backwards as zero elapsed time. | — | T |

## 2. Codecs: views and writers

### 2.1 Views (decoding)

```
decode(bytes, [params], mode) -> Result<View, Error>
View.field_x() -> T                  // typed accessors; never fail
View.payload() -> borrowed slice     // zero-copy
View.anomalies() -> AnomalySet       // lenient mode only; empty in strict mode
View.len() -> usize                  // octets consumed from `bytes`
```

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-010 | A successful `decode` SHALL guarantee that every accessor on the view returns a value without further checks, and that every sub-slice lies within the input. | — | T/F |
| API-011 | `decode` SHALL accept an input longer than the data unit wherever the data unit is self-delimiting (Space Packet, Encapsulation Packet, TC frame, variable-length USLP frame). It SHALL report the consumed length so callers can parse streams. | 133.0 §4.1.3.5; 133.1 §4.1.2.8; 232.0 §4.1.2.7; 732.1 §4.1.2.7 | T |
| API-012 | For fixed-length frames (TM, AOS, fixed-length USLP), `decode` SHALL require the configured frame length and SHALL reject inputs of any other length with `LengthMismatch`. | 132.0 §4.1.1.2; 732.0 §4.1.1.2; 732.1 §4.1.2.7 | T |
| API-013 | Every codec SHALL provide a `peek_length(prefix)` that returns the total length of the data unit from the minimum header prefix, or `Incomplete{needed}` if the prefix is too short. A codec whose length depends on managed parameters (TM, AOS, and USLP for fixed-length and truncated frames) SHALL also take the channel configuration that sets it. | — | T |
| API-014 | Views SHALL offer a conversion to an owned **header** value type (plain fields, no borrow) for storage and comparison. | — | T |

### 2.2 Writers (encoding)

Two encoding styles are required:

```
// one-shot: header fields + payload slice → bytes
encode(header, payload, out_buf) -> Result<usize, Error>

// two-phase, zero-copy: reserve, fill in place, finalize (computes FECF, length fields, FHEC …)
w = Writer::begin(out_buf, header, config) -> Result<Writer, Error>
w.payload_mut() -> mutable slice          // caller fills in place
w.finish(payload_len) -> Result<usize, Error>
```

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-020 | Encoders SHALL produce exactly the octets the Blue Book specifies for the given fields. The round trip `decode(encode(x)) == x` SHALL hold for every valid `x`. | — | V/F |
| API-021 | Encoders SHALL compute derived fields themselves: length fields, FECF, FHEC, Secondary Header Length, Length-of-Length/header size. The caller SHALL NOT be able to supply an inconsistent derived value. | — | T |
| API-022 | When `out_buf` is too small, encoders SHALL return `BufferTooSmall{needed}` and SHALL NOT write a partial data unit that could be mistaken for a valid one. | — | T |
| API-023 | Two-phase writers SHALL leave the buffer in a defined state if `finish` is never called. They SHALL NOT have side effects beyond the supplied buffer. | — | T |

## 3. Strict and lenient decoding

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-030 | Every decoder SHALL support `Strict` (default) and `Lenient` modes. | — | T |
| API-031 | In `Strict` mode, any violation of a SHALL in the Blue Book that is checkable from the octets and the supplied configuration SHALL be an error. | — | V |
| API-032 | In `Lenient` mode, deviations limited to **reserved/spare bits, fields CCSDS declares "undefined", conventional-zero fields, fixed-value fields of idle data units, and the TM Segment Length Identifier**, a fixed-value field that 132.0 keeps only as a legacy marker (e.g. EPP CCSDS Defined Field, TM Packet Order Flag when Sync Flag = 1, Secondary Header Flag of an Idle Packet, TM Segment Length Identifier ≠ `11` when Sync Flag = 0), plus a reserved P-field time code ID decoded as TIME-002 specifies, SHALL be accepted. They SHALL be reported as anomaly flags. Version numbers, lengths, pointers outside the field they index, and FECF/FHEC failures SHALL remain errors in both modes, and a value a decoder reports in both modes, such as the FHEC's corrected symbols, SHALL NOT be an anomaly flag (OI-15, OI-16, OI-33). | e.g. 133.1 §4.1.2.7.2; 132.0 §4.1.2.7.4, §4.1.2.7.5.2 notes 1–2; 133.0 §4.1.3.3.3.4 | V |
| API-033 | Each standard document SHALL list the anomaly flags its decoder can raise. | — | I |

## 4. Configuration objects

Configurations carry managed parameters (see GEN-060). They are passed **by reference** to decoders, writers, and engines.

```
PhysicalChannelConfig  { frame_length | max_frame_length, fecf_present, edition, … }
MasterChannelConfig    { scid, valid_vcids, fsh, ocf, mux policy … }
VirtualChannelConfig   { vcid, data_field_content, fsh, ocf, security: SecurityLayout, … }
MapConfig              { map_id, content, segmentation, blocking, max_sdu_len, … }
PacketTransferConfig   { valid_pvns, max_packet_len, deliver_incomplete }
```

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-040 | Configuration constructors SHALL reject combinations the Blue Book forbids. Examples: TM VC_FSH length outside 2–64; a USLP Insert Zone with variable-length frames; AOS VCID 63 used for a data VC; K > PW for FOP-1. | 132.0 Table 5-2; 732.1 Table 5-1 note 5; 732.0 Table 5-3; 232.1 §5.1.12 | T |
| API-041 | Configurations SHALL be plain data (no references to external state), so they can be serialized, compared, and placed in read-only memory. | — | I |

## 5. Engines: the sans-I/O contract

Every Layer 2 engine follows one contract, so that the framework (Layer 3) and every binding can drive any engine the same way.

```
Engine::new(config, [storage]) -> Result<Engine, Error>

// inputs (each returns Result<(), Error>, plus input-specific handles)
engine.handle_<input>(now: Instant, …)          // e.g. handle_frame, handle_clcw, submit_packet, directive
engine.handle_timeout(now: Instant)

// outputs (pull-based; caller drains until None)
engine.poll_transmit(out_buf) -> Option<Transmit{len, meta}>   // octets to send downward
engine.poll_deliver(out_buf)  -> Option<Deliver{len, meta}>    // SDUs to deliver upward
engine.poll_event()           -> Option<Event>                 // notifications, alerts, statistics
engine.poll_timeout()         -> Option<Instant>               // earliest deadline needing handle_timeout
```

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-050 | Engines SHALL NOT invoke caller code (no callbacks), except through the security hooks of §7. | — | I |
| API-051 | All outputs produced by an input SHALL be queued inside the engine's fixed-capacity output queues. The caller SHALL be able to drain them in any interleaving of `poll_*` calls without changing their content or relative order within each output kind. | — | T/V |
| API-052 | If an input would overflow an output queue, the engine SHALL reject the input with `WouldBlock` and SHALL NOT change state (GEN-042). The exception is an input defined by CCSDS to cause a protocol-level reaction to resource exhaustion, which SHALL produce that reaction instead (e.g. FARM-1 Wait state, [COP-1](standards/cop1.md) §4). | 232.1 §6.1.2 | T |
| API-053 | `poll_timeout` SHALL return the earliest instant at which `handle_timeout` must be called for correct behavior, or `None`. Calling `handle_timeout` early or repeatedly SHALL be harmless. | — | T |
| API-054 | Every engine output that crosses the engine boundary SHALL carry enough metadata (VC/MAP, sequence numbers, request handle, QoS) for the caller to correlate it with inputs, without inspecting octets. | — | T |
| API-055 | A `Transmit` that represents a frame requiring Lower-Procedure acknowledgement (the COP-1 AD/BC/BD Out flags) SHALL carry a **transmit handle**. The caller SHALL report completion with `transmit_done(handle, accepted\|rejected)`. | 232.1 §5.1.6 | T/V |
| API-056 | Engines SHALL expose read-only **status snapshots** (state, counters, queue occupancy) for housekeeping telemetry, without side effects. | — | T |

### 5.1 Why sans-I/O

Engines are fully deterministic (GEN-030). The same vectors therefore test them on a desktop and on the flight target. They don't care whether bytes travel over a UART, SpaceWire, a modem FPGA, UDP, or an SLE link. And the C ABI never has to marshal callbacks or threads across the language boundary.

## 6. Ownership of buffers

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-060 | Input buffers passed to engines SHALL be copied into engine-owned fixed storage if the engine needs them after the call returns, e.g. packets queued for a later frame. Engines SHALL NOT retain borrowed references across calls. | — | I |
| API-061 | Output buffers SHALL be caller-supplied. A `poll_*` call with a buffer too small for the next output SHALL return `BufferTooSmall{needed}` and leave that output queued. | — | T |

## 7. Security hooks (SDLS reservation)

v1 does not implement SDLS, but every frame codec and engine reserves its fields so that SDLS (355.0) can be added later without breaking APIs. In TM, AOS, and USLP, the Security Header follows the Primary Header/FSH/Insert Zone and precedes the data field, and the Security Trailer follows the data field and precedes the OCF and FECF (132.0 §6.3; 732.0 §6.3; 732.1 §6.3). In TC, the Security Header follows the Segment Header if one is present, otherwise the Primary Header. It is never present in Type-C (control command) frames (232.0 §6.3.4).

```
SecurityLayout { header_len: 0..=64, trailer_len: 0..=N }      // per VC (TM/AOS) or per VC/MAP (TC/USLP)

trait SecurityHook {
    fn protect(&mut self, ctx: FrameCtx, frame: &mut [u8], header: Range, data: Range, trailer: Range) -> Result<(), Error>;
    fn verify (&mut self, ctx: FrameCtx, frame: &mut [u8], header: Range, data: Range, trailer: Range) -> Result<Range /*plain data*/, Error>;
}
```

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-070 | Every frame codec SHALL accept a `SecurityLayout` per VC (and per MAP where SDLS may be applied per MAP). Data field lengths SHALL be computed accounting for the Security Header and Trailer. | 132.0 §6.3.5.2; 232.0 §6.3; 732.0 §6.3; 732.1 §6.3; 355.0 §4.1.1.1.1 | T/V |
| API-071 | A Security Header length SHALL be limited to ≤ 64 octets. Header and trailer lengths SHALL be fixed for the configuration's lifetime. | 355.0 §4.1.1.1.4, §4.1.1.3.3 | T |
| API-072 | Views SHALL expose the Security Header and Trailer as opaque slices. Writers SHALL expose them as mutable slices for the hook to fill. | — | T |
| API-073 | v1 SHALL ship a `NoSecurity` hook (layout 0/0, identity), plus a test-only `FixedPatternSecurity` hook that writes known patterns into non-zero-length header and trailer. The second one exercises length accounting and field placement. | — | T/V |
| API-074 | Engines SHALL call `protect` after all other frame fields except the FECF are final, and SHALL compute the FECF after `protect` returns. On reception, the FECF SHALL be checked before `verify` is called. These rules follow the SDLS processing order. | 355.0 §2.3; 232.0 Cor. 1 §6.5.2 | T |
| API-075 | On USLP truncated frames with SDLS, the layout SHALL be the fixed 6-octet header / 16-octet trailer, and the truncated frame length SHALL be ≥ 28 octets. | 732.1 Annex D1.2.3–D1.2.4 | T |

## 8. Events

Engines report notable occurrences as typed events. These include frame-count gaps, discarded frames with reasons, idle frames seen, OCF/FSH received, COP-1 notifications and alerts, and SDU delivery failures. Event types are defined per engine in [04](04-engines.md) and [COP-1](standards/cop1.md).

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-080 | Each event SHALL have a stable numeric kind code shared by all profiles, and a fixed-size payload. | — | V |
| API-081 | When the event queue is full, an engine SHALL drop the *oldest* informational event and increment an `events_dropped` counter. Events that the CCSDS procedure requires to reach the user (COP-1 notifications, alerts, and data-loss indications) SHALL NOT be dropped. Instead, the input that would generate them SHALL return `WouldBlock` (API-052). | — | T |

## 9. Error catalogue

Numeric codes are stable across profiles. In the C ABI, `0` is success and errors are negative (see [C ABI profile](profiles/c-abi.md)).

| Code | Kind | Meaning |
|---|---|---|
| 1 | `BufferTooSmall` | Output buffer too small; carries `needed` |
| 2 | `Incomplete` | Input shorter than required; carries `needed` |
| 3 | `LengthMismatch` | Length field / configured length / buffer length disagree |
| 4 | `UnsupportedVersion` | Version number field not the one this codec handles |
| 5 | `OutOfRange` | A value exceeds its field range or a configured limit |
| 6 | `InvalidField` | A field holds a value the Blue Book forbids (carries field id) |
| 7 | `ReservedValue` | A field holds a value CCSDS reserves (strict mode) |
| 8 | `CrcMismatch` | FECF check failed |
| 9 | `HeaderUncorrectable` | AOS FHEC detected an uncorrectable error |
| 10 | `UnknownChannel` | SCID/VCID/MAP ID/APID not in the configuration |
| 11 | `InvalidConfig` | Configuration violates a Blue Book constraint (carries parameter id) |
| 12 | `WouldBlock` | Engine queue full; input rejected without state change |
| 13 | `InvalidState` | Operation not permitted in the engine's current state (non-CCSDS misuse) |
| 14 | `Security` | Security hook reported failure |
| 15 | `CapacityExceeded` | Configuration exceeds compile-time capacity |
| 16 | `InvalidHandle` | Unknown or stale transmit/request handle |
| 17 | `Malformed` | Structurally impossible input (e.g. pointer beyond data field) |

### 9.1 Field codes

An error that names a field (GEN-041), such as `InvalidField`, carries a **field code**: a stable number shared by all profiles, so that the C ABI can carry it (CABI-010, CABI-021) and the differential tests can compare it (VER-053). Profiles may also expose the field's name for display. The catalogue table is created in M2 WP-0 ([roadmap](08-roadmap.md) §1, OI-27), listing every field identifier the codecs use, and grows with each codec.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-082 | Every field identifier SHALL have a numeric code (16 bits) listed in the field catalogue of this section and shared by all profiles. Codes SHALL never be renumbered or reused; a new field takes the next free code. The C ABI SHALL carry the code. Profiles MAY also expose the field name, which is for display only and SHALL NOT be compared by tests or tools. | GEN-041 | T |
