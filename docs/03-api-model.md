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
| API-002 | Spacecraft ID validity SHALL depend on the protocol and edition that uses it. Each frame codec SHALL define the identifier width it accepts. | 132.0 §4.1.2.2.3; 232.0 §4.1.2.5; 732.0 §4.1.2.2.3; 732.1 §4.1.2.2.3 | T |
| API-003 | Instant arithmetic SHALL saturate rather than wrap. A caller supplying a non-monotonic instant SHALL NOT cause undefined behavior; engines SHALL treat time going backwards as zero elapsed time. | — | T |

## 2. Codecs: views and writers

### 2.1 Views (decoding)

```
decode(bytes, [params]) -> Result<View, Error>                       // strict
decode_lenient(bytes, [params]) -> Result<Lenient<View>, Error>      // lenient
Lenient.anomalies() -> AnomalySet    // the deviations the lenient decode accepted
View.field_x() -> T                  // typed accessors; never fail
View.payload() -> borrowed slice     // zero-copy
View.len() -> usize                  // octets consumed from `bytes`
```

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-010 | A successful `decode` SHALL guarantee that every accessor on the view returns a value without further checks, and that every sub-slice lies within the input. | — | T/F |
| API-011 | `decode` SHALL accept an input longer than the data unit wherever the data unit is self-delimiting (Space Packet, Encapsulation Packet, TC frame, variable-length USLP frame). It SHALL report the consumed length so callers can parse streams. | 133.0 §4.1.3.5; 133.1 §4.1.2.8; 232.0 §4.1.2.7; 732.1 §4.1.2.7 | T |
| API-012 | For fixed-length frames (TM, AOS, fixed-length USLP), `decode` SHALL require the configured frame length and SHALL reject inputs of any other length with `LengthMismatch`. | 132.0 §4.1.1.2; 732.0 §4.1.1.2; 732.1 §4.1.2.7 | T |
| API-013 | Every codec SHALL provide a length peek that, from the minimum header prefix, returns the total length of the data unit, or `Incomplete` with the octets needed, if the prefix is too short. A codec whose length depends on managed parameters (TM, AOS, and USLP for fixed-length and truncated frames) SHALL also take the channel configuration that sets it. | — | T |
| API-014 | Views SHALL offer a conversion to an owned **header** value type (plain fields, no borrow) for storage and comparison. | — | T |
| API-015 | What a decoder checked SHALL be part of the type it returns (RS-020, RS-023). A decode that omits a check that the default decode makes (the unchecked TM decode omits the FECF check, TM-005) SHALL return a distinct view type that offers the read accessors of the checked view and is accepted by no operation that needs a checked frame. It SHALL become the checked view only through the check itself, which fails with `CrcMismatch`. The checked view MAY convert into the unchecked one, which forgets the check. | — | T |
| API-016 | A function that checks a decoded view against managed parameters (the TM, TC, USLP, and Encapsulation Packet validations) SHALL return a refined validated type that dereferences to the view; it SHALL NOT return a bare success (RS-020). Whether the function takes the decoded view (TM, Encapsulation Packet) or the octets (TC, USLP) is not specified. The type proves that the checks passed against some configuration, not which one. An operation that needs a validated frame SHALL take the validated type (the FARM-1 frame takes a validated TC frame, TC-023). | — | T |

### 2.2 Writers (encoding)

Two encoding styles are required:

```
// one-shot: header fields + payload slice → bytes
encode(header, payload, out_buf) -> Result<usize, Error>

// two-phase, zero-copy: reserve, fill in place, protect, finalize (computes FECF, FHEC …)
// The writer is a typestate (RS-023): each step consumes it and returns the next state.
w = Writer::begin(out_buf, header, config) -> Result<Writer<Unprotected>, Error>
w.payload_mut() -> mutable slice          // caller fills in place
w = w.protect(hook, ctx)  -> Result<Writer<Protected>, Error>   // SDLS (API-074); TC and USLP
                                                               // take the payload length first
w = w.unsecured()         -> Result<Writer<Protected>, Error>   // no SDLS: the NoSecurity path
w = w.finalize()          -> Writer<Finalized>                  // FECF (and FHEC) last
w.octets() / w.into_octets() / w.len()                          // only a Finalized frame
```

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-020 | Encoders SHALL produce exactly the octets the Blue Book specifies for the given fields. Decoding what was encoded SHALL give back the input for every valid `x`. | — | V/F |
| API-021 | Encoders SHALL compute derived fields themselves: length fields, FECF, FHEC, Secondary Header Length, Length-of-Length/header size. The caller SHALL NOT be able to supply an inconsistent derived value. | — | T |
| API-022 | When `out_buf` is too small, encoders SHALL return `BufferTooSmall{needed}` and SHALL NOT write a partial data unit that could be mistaken for a valid one. | — | T |
| API-023 | Two-phase writers SHALL leave the buffer in a defined state if `finalize` is never called. They SHALL NOT have side effects beyond the supplied buffer. The two-phase writers are Rust typestate writers (`begin`, `protect` or `unsecured`, `finalize`; RS-021, RS-023); the C ABI has none and builds a frame or packet with a one-shot encoder, which keeps no state between calls (OI-57; OI-50 is superseded). | — | T |

## 3. Strict and lenient decoding

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-030 | Every decoder SHALL support strict and lenient modes as two entry points of different types, not as a runtime argument: `decode` is strict and returns the view alone, and the lenient entry point (with the same suffix on every variant) returns the view in a lenient wrapper, which holds the anomaly set. A view SHALL NOT carry an anomaly set. The C ABI keeps its `mode` argument, which selects between the two entry points (CABI-010), and its view structs keep the `anomalies` field, which is `0` for a strict decode. | — | T |
| API-031 | In strict mode (`decode`), any violation of a SHALL in the Blue Book that is checkable from the octets and the supplied configuration SHALL be an error. Listed exceptions: a CLCW whose COP in Effect is not `01` is decoded and the value exposed, since FOP-1 rejects it (TC-030; 232.0 §4.2.1.5.2, maintainer decision of 2026-10-25); the data of an OID frame is not checked against the pseudo-noise sequence, which a stateless decoder cannot know (CMN-021, TM-010, AOS-016, USLP-009); the TM decoder without a channel table (`decode`) cannot tell that an Only Idle Data frame is on a VC that uses SDLS, which `decode_with` rejects (TM-010, OI-54); a USLP UPID that the SANA registry does not assign, decoded as an other UPID (USLP-007, OI-33 item 4); and truncated and non-truncated USLP frames mixed in one data unit (USLP-011, OI-35 item 8), on different VCs only: a VC with a truncated frame length carries only truncated frames (OI-48, USLP-015). | — | V |
| API-032 | In lenient mode (`decode_lenient`), deviations limited to **reserved/spare bits, fields CCSDS declares "undefined", conventional-zero fields, fixed-value fields of idle data units, and the TM Segment Length Identifier**, a fixed-value field that 132.0 keeps only as a legacy marker (e.g. EPP CCSDS Defined Field, TM Packet Order Flag when Sync Flag = 0, Secondary Header Flag of an Idle Packet, TM Segment Length Identifier ≠ `11` when Sync Flag = 0), a TM Only Idle Data frame on a VC that uses SDLS (OI-54), plus a reserved P-field time code ID decoded as TIME-002 specifies, SHALL be accepted. They SHALL be reported as anomaly flags in the lenient wrapper's anomaly set. Version numbers, lengths, pointers outside the field they index, and FECF/FHEC failures SHALL remain errors in both modes, and a value a decoder reports in both modes, such as the FHEC's corrected symbols, SHALL NOT be an anomaly flag (OI-15, OI-16, OI-33). | e.g. 133.1 §4.1.2.7.2; 132.0 §4.1.2.7.4, §4.1.2.7.5.2 notes 1–2; 133.0 §4.1.3.3.3.4 | V |
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

## 5. Engines: the sans-I/O step contract

Every Layer 2 engine follows one contract, so that the framework (Layer 3) and every binding can drive any engine the same way. An engine is a **pure transition function** over plain-data state. It has no output queues, no timers of its own, and no callbacks. Everything it wants done is returned to the caller as a bounded list of **actions**, and the caller decides what to do with them and when.

```
// The engine's behaviour (neutral notation; the profiles choose the exact form, see API-058)
step(state, input, now: Instant, out: &mut [u8]) -> Result<(state', Actions<N>), Error>

// inputs: one variant per event the engine reacts to
Input = Frame(..) | Clcw(..) | Packet(..) | Directive(..) | TransmitDone(handle, response)
      | Timeout                                    // the deadline has passed
      | TransmitOpportunity | BufferReleased | …   // per engine, see 04 and the standards

// outputs: a fixed-capacity list, in the order the protocol procedure produces them
Action = Transmit{ range-in-out, meta, handle? }   // octets to send downward
       | Deliver{ range-in-out, meta }             // an SDU to deliver upward
       | Notify(Event)                             // notification, alert, indication, statistic

// queries: pure functions of the state
next_deadline(state) -> Option<Instant>    // earliest instant at which `Timeout` must be fed
status(state) -> Status                    // state, counters, occupancy
```

`state` holds everything the Blue Book defines as protocol state (V(S), V(R), the Sent_Queue and Wait_Queue, the state of the timers, the FARM-1 flags and counters) plus the data an engine must retain between inputs (API-060). It holds **no output**. A timer is state, a deadline and an armed flag, so "start timer" and "cancel timer" in a state table are state updates that `next_deadline` reports. They are not actions.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-050 | Engines SHALL NOT invoke caller code: no callbacks, with no exception. Anything an engine needs from the caller, including SDLS processing (§7), is an input, and anything it wants done is an action it returns. | — | I |
| API-051 | Withdrawn: the output-queue contract is replaced by the step contract (API-057, API-059; OI-56). | — | — |
| API-052 | If an input would overflow the engine's **input-side storage** (the data it must retain under API-060: the packet queue, the Wait_Queue, and similar), the engine SHALL reject the input with `WouldBlock` and SHALL NOT change state (API-058, GEN-042). An engine has no output storage to overflow (API-059). The exception is an input defined by CCSDS to cause a protocol-level reaction to resource exhaustion, which SHALL produce that reaction instead: for FARM-1, the Wait state and the buffer-availability inputs (COP-033, [COP-1](standards/cop1.md) §3). | 232.1 §3.3.2, §6.3.2.3, Table 6-1 E1, E2, E10 | T |
| API-053 | `next_deadline` SHALL return the earliest instant at which the timeout input must be fed for correct behavior, or none, and SHALL be a pure function of the state. Timer expiry SHALL be an input. A timeout fed early, or repeatedly, SHALL be harmless: it SHALL change nothing while no deadline has passed (API-003 applies). | — | T |
| API-054 | Every action that crosses the engine boundary SHALL carry enough metadata (VC/MAP, sequence numbers, request handle, QoS) for the caller to correlate it with inputs, without inspecting octets. | — | T |
| API-055 | A transmit action that represents a frame requiring Lower-Procedure acknowledgement (the COP-1 AD/BC/BD Out flags) SHALL carry a **transmit handle**. The caller SHALL report completion with the transmit-done input, naming the handle and whether the frame was accepted or rejected. | 232.1 §5.1.6 | T/V |
| API-056 | Engines SHALL expose read-only **status snapshots** (state, counters, occupancy of the retained input data) for housekeeping telemetry, as pure functions of the state, without side effects. | — | T |
| API-057 | The behaviour of an engine for one input SHALL be a pure, deterministic function of (state, input, `now`) that returns the next state and a list of actions. Its capacity N, the largest number of actions one input can produce, SHALL be a compile-time bound that the engine documents, derived from its capacities and, for COP-1, from the actions listed in each cell of Table 5-1 and Table 6-1. The actions SHALL appear in the order in which the procedure produces them (for COP-1, the order of the cell). No action SHALL be dropped, coalesced, or deferred to a later input. | 232.1 Tables 5-1, 6-1 (COP-1) | T/V |
| API-058 | An input that returns an error SHALL leave the state unchanged and SHALL produce no action, except where the CCSDS procedure itself defines a state change for it (GEN-042). The pure function of API-057 is the unit that tests, vectors, the state-table check, and formal proofs address. A profile MAY realise it as one function that returns the next state, or, where copying the state is too costly, as a pure `decide` that returns a plan and an infallible `commit` that applies the plan; both SHALL be observably identical to the function of API-057. | GEN-042 | T/V |
| API-059 | Engines SHALL hold no output queue. Queuing of actions, and the choice of when to feed the next input, belong to the caller or the framework ([05](05-framework.md)). The caller SHALL be able to consume the actions of one input in any way and at any time before it feeds the next, without changing what the engine does. | — | I |

### 5.1 Why sans-I/O, and why a step function

Engines are fully deterministic (GEN-030). The same vectors therefore test them on a desktop and on the flight target. They don't care whether bytes travel over a UART, SpaceWire, a modem FPGA, UDP, or an SLE link. And the C ABI never has to marshal callbacks or threads across the language boundary.

A step function over plain state, in place of an object that queues and is drained, gives each engine one place where its behaviour is defined and checked: the state table maps to the function, the function maps to a vector, and the C ABI, which loads state by value and writes it back per call, maps onto it without change. The resource limits are explicit in the types: `N` bounds the actions, the capacities bound the state, and the output area (§6) bounds the octets. See OI-56.

## 6. Ownership of buffers

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-060 | Input buffers passed to engines SHALL be copied into engine-owned fixed storage if the engine needs them after the call returns, e.g. packets queued for a later frame. Engines SHALL NOT retain borrowed references across calls. | — | I |
| API-061 | The octets of an action (a frame to transmit, an SDU to deliver) SHALL be written into a **caller-supplied output area** passed with the input, and the action SHALL hold the range of the area and the metadata, not a copy and not a pointer into engine state. The ranges of one input's actions SHALL NOT overlap. An area too small for them SHALL make the input fail with `BufferTooSmall{needed}` and leave the state unchanged (API-058). The ranges stay valid until the caller reuses the area. | — | T |
| API-062 | Every engine SHALL document, for each input, the largest number of octets it can write to the output area (a function of its capacities), so that a caller can size the area once. | — | I |

## 7. Security: SDLS applied by the caller

v1 does not implement SDLS, but every frame codec and engine reserves its fields so that SDLS (355.0) can be added later without breaking APIs. In TM, AOS, and USLP, the Security Header follows the Primary Header/FSH/Insert Zone and precedes the data field, and the Security Trailer follows the data field and precedes the OCF and FECF (132.0 §6.3; 732.0 §6.3; 732.1 §6.3). In TC, the Security Header follows the Segment Header if one is present, otherwise the Primary Header. It is never present in Type-C (control command) frames (232.0 §6.3.4).

**SDLS is not inside an engine** (OI-56). An engine never calls security code. On the sending side it returns a `Transmit` action that carries the frame as the engine built it, with the Security Header and Trailer reserved at the configured lengths and the FHEC and FECF not yet computed, together with the ranges of the header, the data, and the trailer (`FrameLayout`, CMN-040). The caller applies SDLS to those ranges, then finishes the frame with the codec's writer, which computes the FHEC and the FECF last. On the receiving side the caller decodes the frame, checks the FECF (and corrects the AOS FHEC), applies SDLS verification, and only then gives the plain frame to the engine. The engine sees the frame as it was before protection.

```
SecurityLayout { header_len: 0..=64, trailer_len: 0..=N }      // per VC (TM/AOS) or per VC/MAP (TC/USLP)

// A codec-level helper in aldis-core, applied by the caller; engines do not take it.
trait SecurityHook {
    fn layout(&self) -> HookLayout;   // mandatory: Exact(SecurityLayout) or Any (API-073)
    fn protect(&mut self, ctx: FrameCtx, frame: &mut [u8], header: Range, data: Range, trailer: Range) -> Result<(), Error>;
    fn verify (&mut self, ctx: FrameCtx, frame: &mut [u8], header: Range, data: Range, trailer: Range) -> Result<Range /*plain data*/, Error>;
}
```

`SecurityHook` stays, in `aldis-core`, as the shape of the call that applies SDLS to the ranges of a frame. It is a helper for the code that surrounds an engine: the framework's stages, an application, and the tests. It is no longer a type parameter of an engine and no engine holds one. It stays, rather than being withdrawn, because the contract between a real SDLS implementation and the frame (which ranges it gets, in which order it runs, which error it returns) must be written once, and the two reference hooks below need a trait to implement.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-070 | Every frame codec SHALL accept a security layout per VC (and per MAP where SDLS may be applied per MAP). Data field lengths SHALL be computed accounting for the Security Header and Trailer. A codec writer's protect step SHALL compare the layout of its hook (API-073) with the channel's lengths and reject a mismatch with `InvalidField`; its unsecured form SHALL reject a channel that has Security Header or Trailer octets. | 132.0 §6.3.5.2; 232.0 §6.3; 732.0 §6.3; 732.1 §6.3; 355.0 §4.1.1.1.1 | T/V |
| API-071 | A Security Header length SHALL be limited to ≤ 64 octets. A Security Trailer length SHALL be limited to ≤ 65 535 octets, the capacity of a security layout (a project limit; no Blue Book states one). Header and trailer lengths SHALL be fixed for the configuration's lifetime. | 355.0 §4.1.1.1.4, §4.1.1.3.3 | T |
| API-072 | Views SHALL expose the Security Header and Trailer as opaque slices. Writers SHALL expose them as mutable slices for the caller's SDLS processing to fill. | — | T |
| API-073 | Every security hook SHALL state the Security Header and Trailer lengths it is for (an exact layout, or any layout for a hook that serves any); a codec writer's protect step SHALL reject a hook whose layout differs from the channel's. v1 SHALL ship a no-security hook (layout 0/0, identity), plus two test-only hooks (feature `test-hooks`), both for any layout: a fixed-pattern hook, which writes known patterns into non-zero-length header and trailer, and a pre-filled hook, which changes nothing, for tests, vector runners, and fuzz targets that fill the Security fields by hand before the protect step. The fixed-pattern hook exercises length accounting and field placement and, applied by a test around an engine's transmit action, the order of API-074. | — | T/V |
| API-074 | A sending-side engine SHALL return the frame with every field final except the Security Header and Trailer contents, the FHEC, and the FECF, and SHALL return the ranges of the Security Header, the data, and the Security Trailer. The caller SHALL apply `protect` (the writer's `protect`, which checks the hook's layout against the channel's, API-070) after all other frame fields except the FECF are final, and SHALL compute the FHEC (AOS) and the FECF after `protect` returns, with the codec writer. On reception, the caller SHALL check the FECF before `verify` is called, and SHALL give the engine only a frame that has passed both. These rules follow the SDLS processing order. | 355.0 §2.3; 232.0 Cor. 1 §6.5.2 | T |
| API-075 | On USLP truncated frames with SDLS, the layout SHALL be the fixed 6-octet header / 16-octet trailer, and the truncated frame length SHALL be ≥ 28 octets. | 732.1 Annex D1.2.3–D1.2.4 | T |

## 8. Events

Engines report notable occurrences as typed events, returned among the actions of the input that caused them. These include frame-count gaps, discarded frames with reasons, idle frames seen, OCF/FSH received, COP-1 notifications and alerts, and SDU delivery failures. Event types are defined per engine in [04](04-engines.md) and [COP-1](standards/cop1.md).

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-080 | Each event SHALL have a stable numeric kind code shared by all profiles, and a fixed-size payload. | — | V |
| API-081 | Withdrawn: the drop-oldest rule and the `events_dropped` counter belong to the event queue, which no longer exists (API-059; OI-56). | — | — |
| API-083 | An event SHALL be returned as an action of the input that causes it, and SHALL NOT be dropped or coalesced by the engine (API-057). In particular the events that the CCSDS procedure requires to reach the user (COP-1 notifications, alerts, and data-loss indications) cannot be lost in the engine. Counters of occurrences that are not events (discards by reason, gaps) are state (API-056). | 232.1 §4.2–§4.4 | T |

## 9. Error catalogue

Numeric codes are stable across profiles. In the C ABI, `0` is success and an error is its code, as an unsigned status (see [C ABI profile](profiles/c-abi.md)). `cargo xtask status --write` generates from this table the `ALDIS_ERR_*` constants of the C ABI (`crates/aldis-ffi/src/status.rs`, which the header publishes), each asserted at compile time to equal `ErrorKind::<Kind>.code()`; `cargo xtask status` checks that the table runs from code 1 without a gap and that the file matches it. The file is not edited by hand.

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
| 12 | `WouldBlock` | Engine input storage full; input rejected without state change |
| 13 | `InvalidState` | Operation not permitted in the engine's current state (non-CCSDS misuse) |
| 14 | `Security` | SDLS processing (a security hook, applied by the caller) reported failure |
| 15 | `CapacityExceeded` | Configuration exceeds compile-time capacity |
| 16 | `InvalidHandle` | Unknown or stale transmit/request handle |
| 17 | `Malformed` | Structurally impossible input (e.g. pointer beyond data field) |
| 18 | `InternalError` | An internal invariant or consistency check failed |

### 9.1 Field codes

An error that names a field (GEN-041), such as `InvalidField`, carries a **field code**: a stable number shared by all profiles, so that the C ABI can carry it (CABI-010, CABI-021) and the differential tests can compare it (VER-053). Profiles may also expose the field's name for display. The catalogue below was created in M2 WP-0 ([M2 plan](milestones/m2-frame-codecs.md) D2, OI-27), with every field identifier that `aldis-core`, `aldis-time`, `aldis-spp`, and `aldis-epp` name, and grows with each crate. Since 2026-10-24 its codes are divided into blocks (D2 as amended that day).

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| API-082 | Every field identifier SHALL have a numeric code (16 bits) listed in the field catalogue of this section and shared by all profiles. Codes SHALL never be renumbered or reused; a new field takes the next free code of its block (below). The C ABI SHALL carry the code. Profiles MAY also expose the field name, which is for display only and SHALL NOT be compared by tests or tools. | GEN-041 | T |

**Rules of the catalogue.**

- Rows are in code order. Code 0 is never assigned: in the C ABI it means "no field" (CABI-021).
- One code per field. A Blue Book field that several codecs carry has one row, listing every codec that names it; for example the Spacecraft Identifier of TM, TC, AOS, and USLP, or the Packet Version Number of Space and Encapsulation Packets.
- **Kind** says what the code names. `field` is a field of a data unit defined by a Blue Book: a header field, a P-field or T-field subfield, or a data field. `parameter` is anything else an error can name: a managed parameter, a configuration value, an argument of a call, or a length the caller passes, such as a buffer length. A parameter row cites the clause that gives its limits where there is one, and otherwise the requirement it implements.
- **Name** is the display name: lower case, words joined by `_`, unique in the table. It is what an error's `Display` prints, and it is never compared (API-082).
- **Used by** lists the crates that name the field, by the part of the crate name after `aldis-`: the codec crates `core`, `time`, `spp`, `epp`, `tm`, `tc`, `aos`, `uslp`, and the crates that have a block (below). `cargo xtask fields` rejects any other entry.
- **Blocks.** Codes 1–106, assigned before the blocks, keep their numbers: they are the legacy range. Each new crate gets a **block of 50 codes**, reserved when its milestone plan is accepted, with a row in the table of blocks below; a field that only that crate names takes the next free code of its block. A field that several crates name, or that a crate without a block names (the crates of M1 and M2), takes the next free code of the **common block**, 107–149. Within a block the codes run from its first code with no gap. A code never moves (API-082): when a second crate starts to name a field of a crate's block, the code stays in that block, and "Used by" gains the second crate.
- Since each new crate draws on its own block, pull requests for different crates never take the same code, and nothing is renumbered at merge. Two open pull requests that take the same code of one block, the common block or one crate's, are settled as roadmap §6 rule 7 settles requirement IDs: the one that merges second renumbers its rows.

| Codes | Owner | Reserved by |
|---|---|---|
| 1–106 | legacy | M1 and M2: the codes assigned before the blocks ([M2 plan](milestones/m2-frame-codecs.md) D2) |
| 107–149 | common | M2 plan D2 as amended 2026-10-24: fields that several crates name, or that a crate without a block names |
| 150–199 | `cop1` | Pre-assigned 2026-10-24 to `aldis-cop1`, for M3 ([roadmap](08-roadmap.md) §1) |
| 200–249 | `link` | Pre-assigned 2026-10-24 to `aldis-link`, for M4 ([roadmap](08-roadmap.md) §1) |

The next crate's block is 250–299. `cargo xtask fields` checks that the table of blocks has this shape (the legacy range, the common block, then one block of 50 per crate, each following the one before) and that every code lies in its owner's block: a code of 1–106 is grandfathered; above them, a code that one crate with a block names alone lies in that crate's block, a code of a crate's block is named by that crate, and every other code lies in the common block.

The table is the registry. `cargo xtask fields --write` generates from it the Rust module `aldis_core::field`, one `pub const` per row, [`test-vectors/core/fields.json`](../test-vectors/core/fields.json) for the C++ and binding runners, and the `ALDIS_FIELD_*` constants of the C ABI (`crates/aldis-ffi/src/field.rs`, which the header publishes); none of them is edited by hand. `cargo xtask fields`, part of `cargo xtask ci` and of the verification run, fails when a file differs from the table, when a code or name is used twice, the rows are not in code order, a code is not the next free code of its block or lies outside its owner's block, or when "Used by" names something other than a codec crate or the owner of a block. A vector case that expects an error may give the code it names (`expect.field`, [07](07-test-vectors.md) §2).

| Code | Name | Kind | Meaning | Used by | Defined by |
|---|---|---|---|---|---|
| 1 | `spacecraft_id` | field | Spacecraft Identifier | core, tm, tc, uslp, aos | 132.0 §4.1.2.2.3; 232.0 §4.1.2.5; 732.0 §4.1.2.2.3; 732.1 §4.1.2.2.3 |
| 2 | `vc_id` | field | Virtual Channel Identifier | core, tm, tc, uslp, aos, cop1, link | 132.0 §4.1.2.3; 232.0 §4.1.2.6; 732.0 §4.1.2.3; 732.1 §4.1.2.4 |
| 3 | `map_id` | field | Multiplexer Access Point Identifier | core, tc, uslp, link | 232.0 §4.1.3.2.2.3; 732.1 §4.1.2.5 |
| 4 | `packet_version` | field | Packet Version Number | core, spp, epp, link | 133.0 §4.1.3.2; 133.1 §4.1.2.2 |
| 5 | `apid` | field | Application Process Identifier | core, spp | 133.0 §4.1.3.3.4 |
| 6 | `sequence_count` | field | Packet Sequence Count or Packet Name | core | 133.0 §4.1.3.4.3 |
| 7 | `frame_data_field` | field | Transfer Frame Data Field, as the frame layout places it | core, tm, tc, uslp, aos | 132.0 §4.1.4; 232.0 §4.1.3; 732.0 §4.1.4; 732.1 §4.1.4; CMN-041 |
| 8 | `security_header_len` | parameter | Length of the Security Header on a Virtual Channel or MAP | core, tm, tc, aos, uslp, link | 355.0 §4.1.1.1.4; API-071 |
| 9 | `security_trailer_len` | parameter | Length of the Security Trailer on a Virtual Channel or MAP | core, tm, tc, aos, uslp, link | 355.0 §4.1.2; API-071 |
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
| 52 | `packet_len` | field | Total length of a packet in octets: the Encapsulation Packet's Packet Length field, or a Space Packet's primary header and Packet Data Field | spp, epp, link | 133.0 §4.1.2.2, §4.1.3.5; 133.1 §4.1.2.8 |
| 53 | `max_packet_len` | parameter | Maximum Packet Length, a managed parameter | spp, link | 133.0 §5.2 Table 5-1; 132.0 §5.4 Table 5-4; 732.0 §5.5 Table 5-4; 732.1 §5.6 Table 5-5 |
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
| 76 | `fsh_len` | field | Total length of the Transfer Frame Secondary Header in octets: its Length field plus one, and the `MC_FSH` or `VC_FSH` Length managed parameter | tm, link | 132.0 §4.1.3.2.3, §5.2 Table 5-2, §5.3 Table 5-3 |
| 77 | `control_command_flag` | field | Control Command Flag, read with the Bypass Flag as the frame type of Table 4-1 | tc | 232.0 §4.1.2.3, Table 4-1 |
| 78 | `tc_header_spare` | field | Reserved Spare of the TC Transfer Frame Primary Header, bits 4–5 | tc | 232.0 §4.1.2.4 |
| 79 | `max_frame_len` | parameter | Maximum Transfer Frame Length, a managed parameter of a Physical or Virtual Channel | tc, uslp, link | 232.0 §5 Tables 5-1, 5-3; 732.1 §5.2 Table 5-1 note 2 |
| 80 | `segment_header` | field | Segment Header of a TC Frame Data Unit | tc | 232.0 §4.1.3.2.2 |
| 81 | `control_command` | field | Control Command in the data field of a Type-C TC frame | tc | 232.0 §4.1.3.3 |
| 82 | `control_word_type` | field | Control Word Type of the CLCW, bit 0 | tc | 232.0 §4.2.1.2 |
| 83 | `clcw_version` | field | CLCW Version Number, bits 1–2 | tc | 232.0 §4.2.1.3 |
| 84 | `clcw_status` | field | Status Field of the CLCW, bits 3–5 | tc | 232.0 §4.2.1.4 |
| 85 | `cop_in_effect` | field | COP in Effect of the CLCW, bits 6–7 | tc | 232.0 §4.2.1.5 |
| 86 | `clcw_spare_14_15` | field | Reserved Spare of the CLCW, bits 14–15 | tc | 232.0 §4.2.1.7 |
| 87 | `clcw_spare_23` | field | Reserved Spare of the CLCW, bit 23 | tc | 232.0 §4.2.1.10 |
| 88 | `farm_b_counter` | field | FARM-B Counter of the CLCW, bits 21–22 | tc, cop1 | 232.0 §4.2.1.9 |
| 89 | `protocol_control_command_flag` | field | Protocol Control Command Flag of the USLP Primary Header, read with the Bypass/Sequence Control Flag as Table 4-1 | uslp | 732.1 §4.1.2.8.2, Table 4-1 |
| 90 | `uslp_header_spare` | field | Reserve Spares of the USLP Primary Header, bits 50–51 | uslp | 732.1 §4.1.2.9 |
| 91 | `end_of_frame_primary_header_flag` | field | End of Frame Primary Header Flag: a truncated Primary Header | uslp | 732.1 §4.1.2.6; Annex D1.3 |
| 92 | `vcf_count_len` | field | Virtual Channel Frame Count Length, and the VC Count Length managed parameters for each quality of service | uslp, link | 732.1 §4.1.2.11; §5.4 Table 5-3 |
| 93 | `vcf_count` | field | Virtual Channel Frame Count: of a USLP frame, of the length its VCF Count Length gives; of an AOS frame, 24 bits | uslp, aos | 732.1 §4.1.2.12; 732.0 §4.1.2.4 |
| 94 | `tfdz_construction_rule` | field | TFDZ Construction Rule of the TFDF Header | uslp | 732.1 §4.1.4.2.2 |
| 95 | `upid` | field | USLP Protocol Identifier of the TFDF Header | uslp | 732.1 §4.1.4.2.3 |
| 96 | `fhp_lvo_pointer` | field | First Header/Last Valid Octet Pointer of the TFDF Header | uslp | 732.1 §4.1.4.2.4 |
| 97 | `insert_zone_len` | parameter | Insert Zone Length, a managed parameter of a USLP or AOS Physical Channel, and the Insert Zone a writer is given | uslp, aos, link | 732.1 §4.1.3; §5.2 Table 5-1 note 5; 732.0 §4.1.3.4.1, §5.2 Table 5-1 |
| 98 | `truncated_frame_len` | parameter | Truncated Transfer Frame Length, a managed parameter of a Virtual Channel | uslp | 732.1 §5.4 Table 5-3 note 5; Annex D1.2.1, D1.2.4, D1.3.2 |
| 99 | `transfer_frame_type` | parameter | Transfer Frame Type of a Physical or Virtual Channel: fixed or variable length | uslp | 732.1 §5.2 Table 5-1, §5.4 Table 5-3 note 2; §4.1.4.2.2.1.1 |
| 100 | `aos_header_spare` | field | Reserved Spare of the AOS Transfer Frame Primary Header, bits 42–43, in Issue 4 and earlier (the two most significant SCID bits in Issue 5) | aos | 732.0-B-4 §4.1.2.5.4; 732.0 §4.1.2.5.4 |
| 101 | `vc_frame_count_cycle` | field | Virtual Channel Frame Count Cycle of an AOS Transfer Frame, bits 44–47 | aos | 732.0 §4.1.2.5.5 |
| 102 | `data_field_content` | parameter | Data Field Content of an AOS virtual channel: `M_PDU`, `B_PDU`, `VCA_SDU`, or Idle Data | aos | 732.0 §4.1.4.1.3–4, §5.4 Table 5-3 |
| 103 | `ocf` | field | Operational Control Field of an AOS Transfer Frame, present by the `VC_OCF` managed parameter of its virtual channel | aos | 732.0 §4.1.5.2–3, §5.4 Table 5-3 |
| 104 | `mpdu_spare` | field | Reserved Spare of the `M_PDU` Header, bits 0–4, in Issue 4 and earlier | aos | 732.0-B-4 §4.1.4.2.2 |
| 105 | `bpdu_spare` | field | Reserved Spare of the `B_PDU` Header, bits 0–1 | aos | 732.0 §4.1.4.3.2 |
| 106 | `bitstream_data_pointer` | field | Bitstream Data Pointer of the `B_PDU` Header | aos | 732.0 §4.1.4.3.3 |
| 150 | `v_s` | parameter | Transmitter Frame Sequence Number V(S) | cop1 | 232.1 §5.1.8 |
| 151 | `v_r` | parameter | Receiver Frame Sequence Number V(R) | cop1 | 232.1 §6.1.8.1 |
| 152 | `nn_r` | parameter | Expected Acknowledgement Frame Sequence Number NN(R) | cop1 | 232.1 §5.1.8 |
| 153 | `sliding_window_k` | parameter | FOP-1 Sliding Window Width K | cop1 | 232.1 §5.1.8, §7.2 Table 7-1 |
| 154 | `farm_sliding_window_w` | parameter | FARM-1 Sliding Window Width W | cop1 | 232.1 §6.1.8.2, §7.3 Table 7-2 |
| 155 | `farm_positive_window_pw` | parameter | FARM-1 Positive Window Width PW | cop1 | 232.1 §6.1.8.3, §7.3 Table 7-2 |
| 156 | `farm_negative_window_nw` | parameter | FARM-1 Negative Window Width NW | cop1 | 232.1 §6.1.8.3, §7.3 Table 7-2 |
| 157 | `t1_initial` | parameter | FOP-1 Initial Timer T1 Duration | cop1 | 232.1 §5.1.9.1, §7.2 Table 7-1 |
| 158 | `transmission_limit` | parameter | FOP-1 Transmission Limit | cop1 | 232.1 §5.1.9.2, §7.2 Table 7-1 |
| 159 | `timeout_type` | parameter | FOP-1 Timeout Type (TT0 or TT1) | cop1 | 232.1 §5.1.9.1, §7.2 Table 7-1 |
| 160 | `suspend_state` | parameter | FOP-1 Suspend State (SS = 0..4) | cop1 | 232.1 §5.1.11 |
| 161 | `ad_repetitions` | parameter | FOP-1 AD Repetitions | cop1 | 232.1 §5.1.9.3, §7.2 Table 7-1 |
| 162 | `bc_repetitions` | parameter | FOP-1 BC Repetitions | cop1 | 232.1 §5.1.9.3, §7.2 Table 7-1 |
| 163 | `directive_id` | parameter | Identifier of a FOP-1 directive | cop1 | 232.1 §4.2 |
| 164 | `request_id` | parameter | Identifier of a FOP-1 transfer request | cop1 | 232.1 §4.4 |
| 200 | `release_deadline` | parameter | Maximum delay between the first octet written to a data field and its release | link | 732.1 §5.4 Table 5-3 and note 4; LNK-012 |
| 201 | `idle_policy` | parameter | Idle policy of a Virtual Channel that carries Packets: `Oid` or `IdleField` | link | 132.0 §4.1.4.6.3 note 2; 732.0 §4.1.4.2.3.4; 732.1 §4.1.4.1.5–7, §4.2.2.1.5; LNK-014, OI-11 |
| 202 | `idle_packet_kind` | parameter | Idle packet that completes a data field: Space Packet Idle Packet or Encapsulation Idle Packet | link | 132.0 §4.2.2.5; 732.0 §4.2.2.5; 732.1 §4.2.2.1.5; LNK-013 |
| 203 | `idle_pattern` | parameter | Idle data pattern of the octets of idle packets and idle data, and of an Insert Zone fill | link | 732.0 §4.1.4.2.3.2, §4.1.4.3.4.2; 732.1 §4.1.4.3.3 note 1; 133.1 §4.1.3.1.3 note 3; LNK-013, LNK-034 |
| 204 | `valid_pvns` | parameter | Valid Packet Version Numbers of a Virtual Channel or MAP used for Packet Transfer | link | 132.0 §5.4 Table 5-4; 732.0 §5.5 Table 5-4; 732.1 §5.6 Table 5-5; LNK-010 |
| 205 | `deliver_incomplete` | parameter | Whether incomplete Packets are required to be delivered at the receiving end | link | 132.0 §5.4 Table 5-4; 732.0 §5.5 Table 5-4; 732.1 §5.6 Table 5-5; LNK-061 |
| 206 | `mux_policy` | parameter | VC or MC Multiplexing Scheme: fixed priority, round robin, or weighted round robin | link | 132.0 §5.1–5.2 Tables 5-1, 5-2; 732.0 §5.2–5.3 Tables 5-1, 5-2; 732.1 §5.2–5.3 Tables 5-1, 5-2; LNK-031 |
| 207 | `vc_priority` | parameter | Priority of a VC under the fixed-priority multiplexing policy | link | LNK-031 |
| 208 | `vc_weight` | parameter | Weight of a VC under the weighted round-robin multiplexing policy | link | LNK-031 |
| 209 | `blocking` | parameter | Whether several complete Packets may share one Frame Data Unit or TFDF | link | 232.0 §5.3–5.4 Tables 5-3, 5-4; 732.1 §4.2.2.2.8; LNK-040, LNK-042 |
| 210 | `segmentation` | parameter | Whether a MAP permits segmentation of its SDUs | link | 232.0 §5.4 Table 5-4; LNK-040 |
| 211 | `max_fdu_len` | parameter | Maximum Frame Data Unit Length of a MAP (TC), or the maximum TFDF length of a MAP or VC (USLP) | link | 232.0 §4.1.3.1.2, §5.4 Table 5-4; 732.1 §4.1.4.1.4; LNK-040, LNK-042 |
| 212 | `max_sdu_len` | parameter | Maximum `MAP_SDU` Length of a MAP that permits segmentation | link | 232.0 §5.4 Table 5-4; LNK-073 |
| 213 | `max_frames_per_unit` | parameter | Maximum Number of Transfer Frames given to the coding sublayer as a single data unit | link | 232.0 §5.1 Table 5-1; 732.1 §5.2 Table 5-1; LNK-044 |
| 214 | `max_unit_len` | parameter | Maximum Length of Data Unit given to the coding sublayer | link | 232.0 §5.1 Table 5-1; LNK-044 |
| 215 | `qos` | parameter | Quality of service chosen for a transfer request: sequence-controlled or expedited | link | 232.0 §4.1.2.3.1; 732.1 §4.1.2.8; LNK-043 |
| 216 | `link_protocol` | parameter | Frame protocol (TM, TC, AOS, USLP) a link engine is configured for | link | LNK-001 |
