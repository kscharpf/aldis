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

An error that names a field (GEN-041), such as `InvalidField`, carries a **field code**: a stable number shared by all profiles, so that the C ABI can carry it (CABI-010, CABI-021) and the differential tests can compare it (VER-053). Profiles may also expose the field's name for display. The catalogue below was created in M2 WP-0 ([M2 plan](milestones/m2-frame-codecs.md) D2, OI-27), with every field identifier that `aldis-core`, `aldis-time`, `aldis-spp`, and `aldis-epp` name, and grows with each codec.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-082 | Every field identifier SHALL have a numeric code (16 bits) listed in the field catalogue of this section and shared by all profiles. Codes SHALL never be renumbered or reused; a new field takes the next free code. The C ABI SHALL carry the code. Profiles MAY also expose the field name, which is for display only and SHALL NOT be compared by tests or tools. | GEN-041 | T |

**Rules of the catalogue.**

- Codes run from 1 in table order, with no gap. Code 0 is never assigned: in the C ABI it means "no field" (CABI-021).
- One code per field. A Blue Book field that several codecs carry has one row, listing every codec that names it; for example the Spacecraft Identifier of TM, TC, AOS, and USLP, or the Packet Version Number of Space and Encapsulation Packets.
- **Kind** says what the code names. `field` is a field of a data unit defined by a Blue Book: a header field, a P-field or T-field subfield, or a data field. `parameter` is anything else an error can name: a managed parameter, a configuration value, an argument of a call, or a length the caller passes, such as a buffer length. A parameter row cites the clause that gives its limits where there is one, and otherwise the requirement it implements.
- **Name** is the display name: lower case, words joined by `_`, unique in the table. It is what an error's `Display` prints, and it is never compared (API-082).
- **Used by** lists the codec crates that name the field, by the part of the crate name after `aldis-`: `core`, `time`, `spp`, `epp`, `tm`, `tc`, `aos`, `uslp`. `cargo xtask fields` rejects any other entry.
- A new row takes the next code. Two open pull requests that take the same code are settled as roadmap §6 rule 7 settles requirement IDs: the one that merges second renumbers its rows.

The table is the registry. `cargo xtask fields --write` generates from it the Rust module `aldis_core::field`, one `pub const` per row, [`test-vectors/core/fields.json`](../test-vectors/core/fields.json) for the C++ and binding runners, and the `ALDIS_FIELD_*` constants of the C ABI (`crates/aldis-ffi/src/field.rs`, which the header publishes); none of them is edited by hand. `cargo xtask fields`, part of `cargo xtask ci` and of the verification run, fails when a file differs from the table, when a code or name is used twice or the codes are not 1 to n in order, or when "Used by" names something other than a codec crate. A vector case that expects an error may give the code it names (`expect.field`, [07](07-test-vectors.md) §2).

| Code | Name | Kind | Meaning | Used by | Defined by |
|---|---|---|---|---|---|
| 1 | `spacecraft_id` | field | Spacecraft Identifier | core, tm, tc, uslp, aos | 132.0 §4.1.2.2.3; 232.0 §4.1.2.5; 732.0 §4.1.2.2.3; 732.1 §4.1.2.2.3 |
| 2 | `vc_id` | field | Virtual Channel Identifier | core, tm, tc, uslp, aos | 132.0 §4.1.2.3; 232.0 §4.1.2.6; 732.0 §4.1.2.3; 732.1 §4.1.2.4 |
| 3 | `map_id` | field | Multiplexer Access Point Identifier | core, tc, uslp | 232.0 §4.1.3.2.2.3; 732.1 §4.1.2.5 |
| 4 | `packet_version` | field | Packet Version Number | core, spp, epp | 133.0 §4.1.3.2; 133.1 §4.1.2.2 |
| 5 | `apid` | field | Application Process Identifier | core, spp | 133.0 §4.1.3.3.4 |
| 6 | `sequence_count` | field | Packet Sequence Count or Packet Name | core | 133.0 §4.1.3.4.3 |
| 7 | `frame_data_field` | field | Transfer Frame Data Field, as the frame layout places it | core, tm, tc, uslp, aos | 132.0 §4.1.4; 232.0 §4.1.3; 732.0 §4.1.4; 732.1 §4.1.4; CMN-041 |
| 8 | `security_header_len` | parameter | Length of the Security Header on a Virtual Channel or MAP | core, tm, aos | 355.0 §4.1.1.1.4; API-071 |
| 9 | `security_trailer_len` | parameter | Length of the Security Trailer on a Virtual Channel or MAP | core, tm, aos | 355.0 §4.1.2; API-071 |
| 10 | `secured_fields` | parameter | Octet ranges of the Security Header, the data field, and the Security Trailer | core | 355.0 §4.1; API-070 |
| 11 | `width` | parameter | Width of a bit field, 1 to 64 bits | core | GEN-050 |
| 12 | `bit_offset` | parameter | Bit offset of a bit field in its buffer | core | GEN-050 |
| 13 | `buf_bits` | parameter | Length of the buffer that holds a bit field, in bits | core | GEN-050 |
| 14 | `offset` | parameter | Octet offset of a big-endian integer in its buffer | core | GEN-050 |
| 15 | `buf_len` | parameter | Length of the buffer that holds a big-endian integer, in octets | core | GEN-050 |
| 16 | `value` | parameter | Value written to a bit field, or held by a modular counter | core | GEN-050; CMN-030 |
| 17 | `bits` | parameter | Width of a modular counter, 1 to 64 bits | core | CMN-030 |
| 18 | `record_len` | parameter | Length of a record pushed to a fixed-capacity byte queue | core | API-061 |
| 19 | `capacity` | parameter | Capacity of a fixed-capacity byte queue, in octets | core | GEN-011 |
| 20 | `pfield` | field | Preamble Field (P-field) of a time code | time | 301.0 §3.1.1 |
| 21 | `pfield_tail` | field | P-field extension octets that the time code does not define | time | 301.0 §3.1.1; TIME-005 |
| 22 | `time_code_id` | field | Time code identification, P-field octet 1 bits 1–3 | time, spp | 301.0 §3.1.1; 133.0 §4.1.4.2.2 |
| 23 | `tfield` | field | Time Field (T-field) of a time code | time | 301.0 §3.1.2 |
| 24 | `t_len` | field | T-field length of an agency-defined code, 1 to 16 octets | time | 301.0 §3.6.2 |
| 25 | `coarse_octets` | field | Number of octets of the CUC basic time unit | time | 301.0 §3.2.2 |
| 26 | `fine_octets` | field | Number of octets of the CUC fractional time unit | time | 301.0 §3.2.2 |
| 27 | `mission_bits` | field | CUC P-field octet 2 bits 6–7, reserved for mission definition | time | 301.0 §3.2.2 |
| 28 | `coarse` | field | CUC count of basic time units (coarse time) | time | 301.0 §3.2.1 |
| 29 | `fine` | field | CUC fraction of the basic time unit (fine time) | time | 301.0 §3.2.1 |
| 30 | `nanoseconds` | parameter | Nanoseconds converted to CUC fine time | time | TIME-011 |
| 31 | `cds_day` | field | CDS day segment | time | 301.0 §3.3.1, §3.3.2 |
| 32 | `ms_of_day` | field | CDS ms of day segment | time | 301.0 §3.3.1, Annex A |
| 33 | `subms` | field | CDS submillisecond segment, and its length in P-field bits 6–7 | time | 301.0 §3.3.1, §3.3.2 |
| 34 | `agency_epoch` | parameter | Calendar date of the agency-defined epoch of a CDS code | time | 301.0 §3.3.2; TIME-033 |
| 35 | `year` | field | Year: the CCS year segment, the ASCII year subfield | time | 301.0 §3.4.1, §3.5.1, Annex A |
| 36 | `month` | field | Month: the CCS month segment, the ASCII month subfield | time | 301.0 §3.4.1.1, §3.5.1.1, Annex A |
| 37 | `day` | field | Day of month: the CCS day of month segment, the ASCII day subfield | time | 301.0 §3.4.1.1, §3.5.1.1, Annex A |
| 38 | `day_of_year` | field | Day of year: the CCS day of year segment, the ASCII day of year subfield | time | 301.0 §3.4.1.2, §3.5.1.2, Annex A |
| 39 | `hour` | field | Hour: the CCS hour segment, the ASCII hour subfield | time | 301.0 §3.4.1, §3.5.1, Annex A |
| 40 | `minute` | field | Minute: the CCS minute segment, the ASCII minute subfield | time | 301.0 §3.4.1, §3.5.1, Annex A |
| 41 | `second` | field | Second: the CCS second segment, the ASCII second subfield | time | 301.0 §3.4.1, §3.5.1, Annex A |
| 42 | `subsecond` | field | CCS subsecond segments, 10⁻² to 10⁻¹² s | time | 301.0 §3.4.1 |
| 43 | `subsecond_octets` | field | CCS resolution: the number of subsecond segments, P-field bits 5–7 | time | 301.0 §3.4.2 |
| 44 | `date` | field | CCS date segments, whose calendar variation P-field bit 4 selects | time | 301.0 §3.4.1, §3.4.2 |
| 45 | `fraction` | field | ASCII decimal fraction of the second | time | 301.0 §3.5.1.1, §3.5.1.2 |
| 46 | `fraction_digits` | parameter | Number of fraction digits an ASCII format writes | time | 301.0 §3.5.1.1; TIME-031 |
| 47 | `max_fraction_digits` | parameter | Largest number of fraction digits the ASCII parser accepts | time | 301.0 §3.5.1.1; TIME-031 |
| 48 | `secondary_header_flag` | field | Secondary Header Flag | spp | 133.0 §4.1.3.3.3 |
| 49 | `data_field` | field | Packet Data Field | spp | 133.0 §4.1.4.1 |
| 50 | `secondary_header` | field | Packet Secondary Header | spp | 133.0 §4.1.4.2 |
| 51 | `time_code` | parameter | Time code of a time secondary header layout | spp | 133.0 §4.1.4.2.2; SPP-008 |
| 52 | `packet_len` | field | Total length of a packet in octets: the Encapsulation Packet's Packet Length field, or a Space Packet's primary header and Packet Data Field | spp, epp | 133.0 §4.1.2.2, §4.1.3.5; 133.1 §4.1.2.8 |
| 53 | `max_packet_len` | parameter | Maximum Packet Length, a managed parameter | spp | 133.0 §5.2 Table 5-1 |
| 54 | `user_len` | parameter | Length of the user data a two-phase writer is finished with: the User Data Field of a Space Packet, the User Data of a TC Frame Data Unit, or the Transfer Frame Data Zone of a USLP frame | spp, tc, uslp | 133.0 §4.1.4.3; 232.0 §4.1.3.2.1; 732.1 §4.1.4.3; API-023 |
| 55 | `sdu` | parameter | SDU that the segmenter splits across packets | spp | 133.0 §4.1.3.4.2; SPP-013 |
| 56 | `max_user_len` | parameter | Largest User Data Field of one segment | spp | 133.0 §4.1.3.4.2; SPP-013 |
| 57 | `epi` | field | Encapsulation Protocol ID | epp | 133.1 §4.1.2.3 |
| 58 | `epi_extension` | field | Encapsulation Protocol ID Extension | epp | 133.1 §4.1.2.6 |
| 59 | `user_defined` | field | User Defined Field | epp | 133.1 §4.1.2.5 |
| 60 | `ccsds_defined` | field | CCSDS Defined Field | epp | 133.1 §4.1.2.7 |
| 61 | `header_size` | parameter | Header size policy of the encoder, which sets the Length of Length | epp | 133.1 §4.1.2.1.1, §4.1.2.4; EPP-008 |
| 62 | `data_len` | parameter | Length of the Encapsulated Data Field | epp | 133.1 §4.1.3, Table 4-2 |
| 63 | `min_data_len` | parameter | Minimum Data Unit Length, a managed parameter | epp | 133.1 §5 Table 5-1 |
| 64 | `max_data_len` | parameter | Maximum Data Unit Length, a managed parameter | epp | 133.1 §5 Table 5-1 |
| 65 | `ocf_type_flag` | field | Type Flag of the Operational Control Field, bit 0: Type-1 or Type-2 Report | core | 132.0 §4.1.5.4; 732.0 §4.1.5.4; 732.1 §4.1.5.2.1 |
| 66 | `ocf_report_use` | field | Bit 1 of a Type-2 Report in the Operational Control Field: project-specific or SDLS Frame Security Report | core | 132.0 §4.1.5.5; 732.0 §4.1.5.5; 732.1 §4.1.5.2.2 |
| 67 | `frame_version` | field | Transfer Frame Version Number | tm, tc, uslp, aos | 132.0 §4.1.2.2.2; 232.0 §4.1.2.2; 732.0 §4.1.2.2.2; 732.1 §4.1.2.2.2 |
| 68 | `frame_len` | field | Length of a Transfer Frame in octets: the Frame Length field of a TC or USLP frame, or the Transfer Frame Length managed parameter of a TM, AOS, or USLP physical channel with fixed-length frames | tm, tc, uslp, aos | 132.0 §4.1.1.2, §5.1 Table 5-1; 232.0 §4.1.2.7; 732.1 §4.1.2.7, Table 5-1; 732.0 §4.1.1.2, §5.2 Table 5-1 |
| 69 | `ocf_flag` | field | Operational Control Field Flag | tm, uslp | 132.0 §4.1.2.4; 732.1 §4.1.2.10 |
| 70 | `fsh_flag` | field | Transfer Frame Secondary Header Flag | tm | 132.0 §4.1.2.7.2 |
| 71 | `sync_flag` | field | Synchronization Flag of the TM Transfer Frame Data Field Status | tm | 132.0 §4.1.2.7.3 |
| 72 | `packet_order_flag` | field | Packet Order Flag | tm | 132.0 §4.1.2.7.4 |
| 73 | `segment_length_id` | field | Segment Length Identifier | tm | 132.0 §4.1.2.7.5 |
| 74 | `first_header_pointer` | field | First Header Pointer: of the TM Transfer Frame Data Field Status, or of the AOS `M_PDU` Header | tm, aos | 132.0 §4.1.2.7.6; 732.0 §4.1.4.2.2; 732.0-B-4 §4.1.4.2.3 |
| 75 | `fsh_version` | field | Transfer Frame Secondary Header Version Number | tm | 132.0 §4.1.3.2.2 |
| 76 | `fsh_len` | field | Total length of the Transfer Frame Secondary Header in octets: its Length field plus one, and the `MC_FSH` or `VC_FSH` Length managed parameter | tm | 132.0 §4.1.3.2.3, §5.2 Table 5-2, §5.3 Table 5-3 |
| 77 | `control_command_flag` | field | Control Command Flag, read with the Bypass Flag as the frame type of Table 4-1 | tc | 232.0 §4.1.2.3, Table 4-1 |
| 78 | `tc_header_spare` | field | Reserved Spare of the TC Transfer Frame Primary Header, bits 4–5 | tc | 232.0 §4.1.2.4 |
| 79 | `max_frame_len` | parameter | Maximum Transfer Frame Length, a managed parameter of a Physical or Virtual Channel | tc, uslp | 232.0 §5 Tables 5-1, 5-3; 732.1 §5.2 Table 5-1 note 2 |
| 80 | `segment_header` | field | Segment Header of a TC Frame Data Unit | tc | 232.0 §4.1.3.2.2 |
| 81 | `control_command` | field | Control Command in the data field of a Type-C TC frame | tc | 232.0 §4.1.3.3 |
| 82 | `control_word_type` | field | Control Word Type of the CLCW, bit 0 | tc | 232.0 §4.2.1.2 |
| 83 | `clcw_version` | field | CLCW Version Number, bits 1–2 | tc | 232.0 §4.2.1.3 |
| 84 | `clcw_status` | field | Status Field of the CLCW, bits 3–5 | tc | 232.0 §4.2.1.4 |
| 85 | `cop_in_effect` | field | COP in Effect of the CLCW, bits 6–7 | tc | 232.0 §4.2.1.5 |
| 86 | `clcw_spare_14_15` | field | Reserved Spare of the CLCW, bits 14–15 | tc | 232.0 §4.2.1.7 |
| 87 | `clcw_spare_23` | field | Reserved Spare of the CLCW, bit 23 | tc | 232.0 §4.2.1.10 |
| 88 | `farm_b_counter` | field | FARM-B Counter of the CLCW, bits 21–22 | tc | 232.0 §4.2.1.9 |
| 89 | `protocol_control_command_flag` | field | Protocol Control Command Flag of the USLP Primary Header, read with the Bypass/Sequence Control Flag as Table 4-1 | uslp | 732.1 §4.1.2.8.2, Table 4-1 |
| 90 | `uslp_header_spare` | field | Reserve Spares of the USLP Primary Header, bits 50–51 | uslp | 732.1 §4.1.2.9 |
| 91 | `end_of_frame_primary_header_flag` | field | End of Frame Primary Header Flag: a truncated Primary Header | uslp | 732.1 §4.1.2.6; Annex D1.3 |
| 92 | `vcf_count_len` | field | Virtual Channel Frame Count Length, and the VC Count Length managed parameters for each quality of service | uslp | 732.1 §4.1.2.11; §5.4 Table 5-3 |
| 93 | `vcf_count` | field | Virtual Channel Frame Count: of a USLP frame, of the length its VCF Count Length gives; of an AOS frame, 24 bits | uslp, aos | 732.1 §4.1.2.12; 732.0 §4.1.2.4 |
| 94 | `tfdz_construction_rule` | field | TFDZ Construction Rule of the TFDF Header | uslp | 732.1 §4.1.4.2.2 |
| 95 | `upid` | field | USLP Protocol Identifier of the TFDF Header | uslp | 732.1 §4.1.4.2.3 |
| 96 | `fhp_lvo_pointer` | field | First Header/Last Valid Octet Pointer of the TFDF Header | uslp | 732.1 §4.1.4.2.4 |
| 97 | `insert_zone_len` | parameter | Insert Zone Length, a managed parameter of a USLP or AOS Physical Channel, and the Insert Zone a writer is given | uslp, aos | 732.1 §4.1.3; §5.2 Table 5-1 note 5; 732.0 §4.1.3.4.1, §5.2 Table 5-1 |
| 98 | `truncated_frame_len` | parameter | Truncated Transfer Frame Length, a managed parameter of a Virtual Channel | uslp | 732.1 §5.4 Table 5-3 note 5; Annex D1.2.1, D1.2.4, D1.3.2 |
| 99 | `transfer_frame_type` | parameter | Transfer Frame Type of a Physical or Virtual Channel: fixed or variable length | uslp | 732.1 §5.2 Table 5-1, §5.4 Table 5-3 note 2; §4.1.4.2.2.1.1 |
| 100 | `aos_header_spare` | field | Reserved Spare of the AOS Transfer Frame Primary Header, bits 42–43, in Issue 4 and earlier (the two most significant SCID bits in Issue 5) | aos | 732.0-B-4 §4.1.2.5.4; 732.0 §4.1.2.5.4 |
| 101 | `vc_frame_count_cycle` | field | Virtual Channel Frame Count Cycle of an AOS Transfer Frame, bits 44–47 | aos | 732.0 §4.1.2.5.5 |
| 102 | `data_field_content` | parameter | Data Field Content of an AOS virtual channel: `M_PDU`, `B_PDU`, `VCA_SDU`, or Idle Data | aos | 732.0 §4.1.4.1.3–4, §5.4 Table 5-3 |
| 103 | `ocf` | field | Operational Control Field of an AOS Transfer Frame, present by the `VC_OCF` managed parameter of its virtual channel | aos | 732.0 §4.1.5.2–3, §5.4 Table 5-3 |
| 104 | `mpdu_spare` | field | Reserved Spare of the `M_PDU` Header, bits 0–4, in Issue 4 and earlier | aos | 732.0-B-4 §4.1.4.2.2 |
| 105 | `bpdu_spare` | field | Reserved Spare of the `B_PDU` Header, bits 0–1 | aos | 732.0 §4.1.4.3.2 |
| 106 | `bitstream_data_pointer` | field | Bitstream Data Pointer of the `B_PDU` Header | aos | 732.0 §4.1.4.3.3 |
