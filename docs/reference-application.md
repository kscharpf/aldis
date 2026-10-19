# Reference application — scenario

**Area:** APP · **Track:** A ([roadmap](08-roadmap.md) §3) · **Plan:** [A0 Scenario](milestones/a0-scenario.md) (decisions D1–D16) · **Code:** `refapp/` · **Scenario version:** 1 (draft for maintainer review)

## Summary

| Item | Value | Section |
|---|---|---|
| Story | A small Earth-orbiting spacecraft and its control centre: a **flight node** and a **ground node**, both sans-I/O, connected by a harness that owns time, the link, faults, and the trace | §2 |
| Flight applications | On-board time, housekeeping (1 Hz), event reporter, payload (0.5 Hz, packets longer than a frame), memory dump (segmented packets), command handler (immediate and time-tagged), table store | §2.1 |
| Ground applications | Telemetry reception and decoding, packet-sequence tracking, time-stamp decoding, dump reassembly, scripted operations (the ops script); FOP-1 from A3 | §2.2 |
| APIDs | `0x010` HK, `0x011` events, `0x020` payload, `0x021` dump, `0x080` commands, `0x081` table upload, `2047` idle | §3.1 |
| Spacecraft ID | `0x155` (341), **proposed, fictional**, awaiting OI-26 | §3.4 |
| Downlink | TM, 223-octet frames, FECF, MC_OCF with the CLCW, VC 0 (HK, events), VC 1 (payload, dump), VC 7 (OID frames only), fixed-priority mux, 8 frames/s | §4.1 |
| Uplink | TC, frames ≤ 128 octets, FECF, one VC with Segment Header, MAP 0 commands (blocking), MAP 1 table upload (segmentation); Type-BD in A2; COP-1 from A3 (K = 4, W = 8, T1_Initial = 1 s, Transmission_Limit = 3) | §4.2 |
| Time | CUC Level 1, 4 coarse + 2 fractional octets, T0 = 2 100 000 000 s, no drift | §5 |
| Link model | Simulated µs clock from 0; one-way delay 10 ms; downlink slot 125 ms; uplink slot 250 ms; scripted faults by (direction, unit index) | §6 |
| Golden traces | Canonical JSON Lines in `refapp/golden/<case>.jsonl`; header line + one record per observable event; byte comparison | §7 |
| Node interface | `step(now, inputs) -> outputs + next deadline`; in-process for Rust, a lockstep stdin/stdout protocol for the A5 C++ and A6 QEMU flight nodes (OI-24) | §8 |
| Code rules | `aldis` facade only; flight node `no_std`, no heap, flight lint set; `forbid(unsafe_code)` everywhere | §9 |

**Open items that affect this document:** OI-26 (spacecraft ID), OI-24 (out-of-process nodes, §8.2), OI-23 (golden traces and vectors, §7.5), OI-25 (framework trace, §7.5), OI-11 (OID frames, §4.1), OI-06 (SANA registries, §3.1, §4.3). Every protocol value below cites the Blue Book clause it was read from.

**How to read the requirements.** Requirement IDs follow [00 §5](00-introduction.md#5-requirement-conventions), grouped in decades per section. **T** means a golden-trace reproduction test or a harness test in `refapp/` annotated `// req:`, and the header of each golden trace lists the IDs its case verifies (§7.2). **I** means review of this document or of `refapp/`. **A** means a CI check. Requirements name the library requirements they exercise.

## 1. Purpose and scope

The reference application shows an integrator how the libraries fit together end to end, gives the library tracks API feedback before the v1.0 freeze, and shows that the Rust-backed and native libraries are interchangeable: every implementation must reproduce the same golden traces byte for byte (GEN-030). It is built once, in A1, and grown by A2–A6 ([plan](milestones/a0-scenario.md) §2.1):

| Milestone | Link level | Adds |
|---|---|---|
| A1 | Packets: one Space Packet per link unit | All applications, the nominal timeline, the first golden traces |
| A2 | Frames: TM down, TC Type-BD up, hand-written loops | Downlink faults |
| A3 | COP-1: FOP-1 on the ground, FARM-1 on board | Uplink faults; the link-level trace is frozen from here on |
| A4 | Engines replace the loops; AOS and USLP variants | Nothing on the TM/TC link |
| A5 | C++ flight node over `aldis.hpp` (Rust-backed and native) | Nothing |
| A6 | `aldis-framework` stacks; flight node bare-metal under QEMU | Nothing |

Not covered: synchronization and channel coding, SDLS, CFDP, and time correlation (00 §2.1).

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-001 | The reference application SHALL consist of a flight node and a ground node that exchange link units only through the harness (§6), and SHALL use only the public Aldis APIs (APP-100). | — | I |
| APP-002 | The **timeline** (what each application does, and when, §2.4) SHALL be fixed in A1 and only extended. A later scenario version MAY add applications, commands, ops-script entries, record kinds, event codes, or cases, and SHALL NOT change or remove an existing one. | — | I |
| APP-003 | For the nominal case, the **packet projection** SHALL be identical at every link level (A1, A2, A3, the A4 variants) and in every implementation. The packet projection is, for each node, record kind (`packet_out`, `packet_in`), and APID, the ordered sequence of packet octets (§7.3). Order across APIDs and instants are not part of it. | — | T |
| APP-004 | From A3 on, the link-level traces SHALL be frozen: the A4, A5, and A6 implementations SHALL reproduce the golden traces of every `a3-*` case byte for byte. | GEN-030 | T |
| APP-005 | A later milestone SHALL add cases and SHALL NOT redefine an existing case. A released golden trace SHALL change only through a new trace version (APP-087). | — | I |
| APP-006 | Link units SHALL be whole Space Packets (A1) or whole Transfer Frames (A2 on), without synchronization or channel coding. Every channel SHALL use a zero-length security layout (`NoSecurity`, API-073). | — | I |

## 2. Roles and applications

### 2.1 Flight node

| Application | Behaviour | APID |
|---|---|---|
| On-board time | On-board time = T0 + `now` (§5) | — |
| Housekeeping | One HK packet every 1 s at t = k s while t < 30 s (k = 0…29), layout §3.3.1 | `0x010` |
| Event reporter | One event packet per on-board event (§3.3.2), generated at the instant of the event | `0x011` |
| Payload | One science packet every 2 s at t = 0.5 + 2k s while t < 30 s and the payload mode is *on*; the mode is *on* at t = 0 | `0x020` |
| Memory dump | On command, a dump SDU (§3.3.4) segmented into packets (SPP-013), one segment every 125 ms from the execution instant | `0x021` |
| Command handler | Executes commands (§3.3.5), immediately or at their time tag | `0x080` (in) |
| Table store | Stores an uploaded table (§3.3.6); a command activates it | `0x081` (in) |

Within one instant the flight node works in this order: (1) handle delivered link units and the packets they carry, in delivery order (immediate commands execute here); (2) execute the time-tagged commands due, in queue order; (3) emit a due dump segment; (4) housekeeping; (5) payload. This order is part of the timeline (APP-002).

### 2.2 Ground node

| Application | Behaviour |
|---|---|
| Telemetry reception | Decodes link units with the strict codecs (A1: Space Packets; A2 on: TM frames, packet extraction) and delivers every telemetry packet (`packet_in`) |
| Sequence tracking | Classifies the sequence count of every telemetry packet per APID (SPP-011); a gap, repeat, or reset becomes an application event (§7.4) |
| Time-stamp decoding | Decodes the CUC time code of every packet that carries one (implicit or explicit P-field, SPP-008) |
| Dump reassembly | Rejoins the dump segments (SPP-013) and reports the reassembled dump as an application event |
| Ops script | Releases commands and the table upload at the instants of §2.4 (`packet_out`); from A3 also issues FOP-1 directives |
| Commanding (A3 on) | FOP-1 sending end; transfer and directive notifications become application events |

### 2.3 Commands and time tags

| Rule | Value |
|---|---|
| Immediate command | Executes in the step in which it is delivered (§2.1 order, step 1) |
| Time-tagged command | Queued at delivery; executes in the first step at or after its execution time; equal execution times execute in delivery order |
| Time-tag queue capacity | 8 commands |
| Verification report | `CMD_EXECUTED` or `CMD_REJECTED` (§3.3.2) at the execution instant; checks of function code and arguments happen at execution |
| Rejected at delivery | A time tag earlier than the delivery instant (reason 2), or a full queue (reason 4) |
| Nominal case | Uses only time-tagged commands, each released at least 5 s before its execution time, so that no on-board packet depends on uplink latency (APP-003) |

### 2.4 Nominal timeline (case `nominal`)

Applications generate packets while t < 30 s; the run ends at t = 35 s, so every packet is delivered at every link level. Execution times are on-board times T0 + the instant shown. Command sequence counts (APID `0x080`) follow release order from 0.

| Release (s) | Entry | From | Execution (s) | Expected on-board result |
|---|---|---|---|---|
| 0.0 | FOP-1 directive *Initiate AD Service (without CLCW Check)* | A3 | — | FOP-1 Active (232.1 §4.1) |
| 1.0 | Command `NOOP`, seq 0 | A1 | 6.0 | `CMD_EXECUTED` |
| 2.0 | Table upload, table 1, 300 octets (APID `0x081`, seq 0) | A1 | — | Stored; no report |
| 3.0 | Command `ACTIVATE_TABLE` (table 1, CRC `0x0C4F`), seq 1 | A1 | 10.0 | `CMD_EXECUTED`, `TABLE_ACTIVATED` |
| 4.0 | Command `DUMP_MEMORY` (address `0x00010000`, 600 octets), seq 2 | A1 | 12.0 | `CMD_EXECUTED`; five dump packets at 12.000…12.500 s; `DUMP_COMPLETED` at 12.5 s |
| 5.0 | Command `SET_PAYLOAD_MODE` (off), seq 3 | A1 | 20.0 | `CMD_EXECUTED`, `PAYLOAD_MODE` (0) |
| 5.0 | Command `NOOP`, seq 4 (same instant: blocked with seq 3 from A2, LNK-071) | A1 | 20.0 | `CMD_EXECUTED` |
| 6.0 | Command `SET_PAYLOAD_MODE` (on), seq 5 | A1 | 24.0 | `CMD_EXECUTED`, `PAYLOAD_MODE` (1) |

The resulting telemetry: 30 HK packets, 13 payload packets (t = 0.5…18.5 s and 24.5…28.5 s), 5 dump packets, and 10 event packets. Derived values, to be cross-checked when A1 implements them: CRC-16 (CMN-001) of the table is `0x0C4F`, and of the 600 dumped memory octets `0x80A8`.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-010 | The flight node SHALL implement the applications of §2.1, in the per-instant order of §2.1. | — | T |
| APP-011 | The ground node SHALL implement the applications of §2.2. It SHALL decode every received link unit and packet with the strict codecs (API-030), track sequence counts per APID (SPP-011), decode time codes (SPP-008, TIME-010), and reassemble dumps (SPP-013). | 133.0 §4.1.3.4.3.3–4 | T |
| APP-012 | The command handler SHALL follow §2.3: immediate commands execute when delivered; time-tagged commands execute in the first step at or after their execution time, in delivery order for equal times; the queue holds 8 commands. | — | T |
| APP-013 | Every command SHALL produce exactly one verification report, `CMD_EXECUTED` or `CMD_REJECTED`, at its execution instant, except the rejections at delivery of §2.3 (reasons 2 and 4), which are reported at the delivery instant. | — | T |
| APP-014 | The nominal case SHALL use only time-tagged commands, each released at least 5 s before its execution time. | — | I |
| APP-015 | The payload application SHALL generate one packet at t = 0.5 + 2k s (k ≥ 0, t < 30 s) while its mode is *on*, and none while it is *off*. `SET_PAYLOAD_MODE` SHALL change the mode at its execution instant. | — | T |
| APP-016 | The memory dump SHALL build the dump SDU of §3.3.4 and send it as Space Packets with Sequence Flags `01`, `00`, `10` (or `11` for a single packet), at most 128 user-data octets each, one packet every 125 000 µs from the execution instant, followed by `DUMP_COMPLETED` in the step of the last packet. A `DUMP_MEMORY` command executed while a dump is in progress SHALL be rejected with reason 5. | 133.0 §4.1.3.4.2.2 | T |
| APP-017 | The table store SHALL keep the most recently uploaded table (one table of up to 512 octets). `ACTIVATE_TABLE` SHALL report reason 1 if the stored table has another ID or none was uploaded, and otherwise compare the CRC-16 (CMN-001) of the stored table with the command's argument and report `TABLE_ACTIVATED` or `TABLE_REJECTED`. | — | T |
| APP-018 | The ground node SHALL report each sequence gap, repeat, and reset (SPP-011), each reassembled or discarded dump, and each packet rejected by a strict decoder as the application events of §7.4. | — | T |
| APP-019 | The nominal case SHALL follow the timeline of §2.4. Ops-script entries marked with a later milestone SHALL apply only from that link level on. | — | T |

## 3. Identifiers and packet formats

### 3.1 APIDs

APIDs are chosen freely: 133.0 §4.1.3.3.4.4 note 1 restricts only the Idle Packet APID, and 133.0 Annex B2.1 records the SANA APID policy as 0–2046 unmanaged and 2047 for the Idle Packet (to be confirmed against the live registry, OI-06). One APID names one managed data path (133.0 §4.1.3.3.4.2).

| APID | Direction | Application | Packet Type | Secondary header (§3.2) | Sequence Flags |
|---|---|---|---|---|---|
| `0x010` (16) | down | Housekeeping | 0 | CUC, implicit P-field | `11` |
| `0x011` (17) | down | Event reports | 0 | CUC, explicit P-field | `11` |
| `0x020` (32) | down | Payload science | 0 | CUC, implicit P-field | `11` |
| `0x021` (33) | down | Memory dump | 0 | CUC, implicit P-field | `01` / `00` / `10`, or `11` |
| `0x080` (128) | up | Commands | 1 | none | `11` |
| `0x081` (129) | up | Table upload | 1 | none | `11` |
| `0x7FF` (2047) | down (A2 on) | Idle Packets, inserted by the link procedures only (LNK-013) | 0 | none (SPP-010) | `11` |

Every APID has its own Packet Sequence Count, starting at 0 when the run starts and continuing modulo 16384 (133.0 §4.1.3.4.3.3–4). Telecommand packets carry a Packet Sequence Count, not a Packet Name (133.0 §4.1.3.4.3.2).

### 3.2 Secondary headers

| Form | Octets | Content |
|---|---|---|
| CUC, implicit P-field | 6 | T-field only: 4 coarse ‖ 2 fractional octets (§5) |
| CUC, explicit P-field | 7 | P-field `0x1E` (extension 0, time code ID `001`, 4 − 1 = `11` coarse, `10` = 2 fractional; 301.0 §3.2.2) ‖ the 6-octet T-field |

The secondary header holds a Time Code Field only (133.0 §4.1.4.2.1.5 a). The P-field choice is fixed per APID (133.0 §4.1.4.2.2.4–5), so both forms exercise SPP-008.

### 3.3 Packet user data

All integers are unsigned and big-endian (GEN-050). Octet offsets count from the first octet of the User Data Field.

#### 3.3.1 Housekeeping (`0x010`, 32 octets; packet length 44)

| Octets | Field |
|---|---|
| 0–3 | HK packet number k (0, 1, …) |
| 4–5 | Commands executed (wrapping modulo 2¹⁶) |
| 6–7 | Commands rejected (wrapping modulo 2¹⁶) |
| 8 | Time-tag queue occupancy |
| 9 | Payload mode (0 off, 1 on) |
| 10 | Last activated table ID (0 = none) |
| 11 | Spare, `0x00` |
| 12–13 | CRC-16 of the last activated table (0 = none) |
| 14–15 | Dumps completed |
| 16–31 | Sensor channels j = 0…7, u16 each: (1000 · (j + 1) + 3 · k) mod 2¹⁶ |

#### 3.3.2 Event reports (`0x011`, 8 octets; packet length 21)

Layout: event code (u16) ‖ p1 (u16) ‖ p2 (u16) ‖ p3 (u16). Unused parameters are 0. Several events at one instant are emitted in the order the flight node raises them (§2.1).

| Code | Event | p1 | p2 | p3 |
|---|---|---|---|---|
| `0x0001` | `CMD_EXECUTED` | Function code | Command sequence count | 0 |
| `0x0002` | `CMD_REJECTED` | Function code (0 if unreadable) | Command sequence count | Reason: 1 unknown function, 2 time tag in the past, 3 malformed arguments, 4 time-tag queue full, 5 dump in progress |
| `0x0010` | `TABLE_ACTIVATED` | Table ID | Length | CRC-16 |
| `0x0011` | `TABLE_REJECTED` | Table ID | Reason: 1 no table, 2 CRC mismatch | 0 |
| `0x0020` | `DUMP_COMPLETED` | Address bits 31–16 | Address bits 15–0 | Length |
| `0x0030` | `PAYLOAD_MODE` | New mode | 0 | 0 |

On execution a command raises `CMD_EXECUTED` first and its effect events after it.

#### 3.3.3 Payload science (`0x020`, 400 octets; packet length 412)

Octets 0–3: payload packet number n (0, 1, … over all payload packets of the run). Octets 4–399: octet i = (n + i) mod 256. A payload packet is longer than one TM data field (211 octets, §4.1), so it spans frames (LNK-011).

#### 3.3.4 Memory dump (`0x021`)

The dump SDU is address (u32) ‖ length L (u16, 1…1024) ‖ L memory octets. Simulated memory: the octet at address a is (a XOR (a >> 8)) mod 256. The SDU is cut into chunks of 128 octets (the last one shorter); each chunk is the user data of one dump packet. The nominal dump (606-octet SDU) gives chunks of 128, 128, 128, 128, and 94 octets.

#### 3.3.5 Commands (`0x080`)

| Octets | Field |
|---|---|
| 0 | Function code |
| 1 | Execution mode: `0x00` immediate, `0x01` time-tagged |
| 2–7 | Execution time: CUC T-field, 4 + 2 octets (§5); zero for immediate commands |
| 8… | Arguments |

| Code | Function | Arguments | Packet length |
|---|---|---|---|
| `0x01` | `NOOP` | none | 14 |
| `0x02` | `DUMP_MEMORY` | address (u32) ‖ length (u16, 1…1024) | 20 |
| `0x03` | `ACTIVATE_TABLE` | table ID (u8) ‖ CRC-16 (u16) | 17 |
| `0x04` | `SET_PAYLOAD_MODE` | mode (u8: 0 off, 1 on) | 15 |

#### 3.3.6 Table upload (`0x081`)

Table ID (u8, 1…255) ‖ length L (u16, 1…512) ‖ L table octets. The nominal table (ID 1, L = 300) has octet i = (3 · i + 1) mod 256. Its packet (309 octets) is longer than one TC frame data field, so it is segmented on MAP 1 (§4.2).

### 3.4 Spacecraft ID

TM, TC, AOS, and USLP frames carry a spacecraft ID assigned through CCSDS 320.0 and SANA (00 §6). The public SANA spacecraft ID registry was checked on 2026-10-19 and showed no value set aside for testing or documentation. **Proposed:** SCID `0x155` (341), whose alternating bit pattern (`01 0101 0101`) makes a misplaced or shifted SCID field visible in a hex dump. It fits the 10-bit TM and TC fields (132.0 §4.1.2.2.3; 232.0 §4.1.2.5) and the AOS and USLP fields. The value is fictional: the maintainer confirms it against the full registry or picks another (OI-26), and `refapp/README.md` and every golden-trace case description label it as fictional.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-020 | The scenario SHALL use the APIDs, Packet Types, secondary headers, and Sequence Flags of §3.1. No APID other than 2047 SHALL be used for Idle Packets. | 133.0 §4.1.3.3.4.2, §4.1.3.3.4.4 note 1, Annex B2.1 | I |
| APP-021 | Every APID SHALL have its own Packet Sequence Count, starting at 0 and continuing modulo 16384; telecommand packets SHALL carry a Packet Sequence Count (exercises SPP-011, SPP-012). | 133.0 §4.1.3.4.3.2–4 | T |
| APP-022 | Secondary headers SHALL hold a Time Code Field only, with the P-field form of §3.2 fixed per APID: implicit for `0x010`, `0x020`, and `0x021`, explicit (`0x1E`) for `0x011` (exercises SPP-007, SPP-008). | 133.0 §4.1.4.2.1.5, §4.1.4.2.2.4–5; 301.0 §3.2.2 | T |
| APP-023 | Housekeeping, event, payload, and dump packets SHALL have the user data of §3.3.1–3.3.4. | — | T |
| APP-024 | Command and table-upload packets SHALL have the user data of §3.3.5 and §3.3.6. A command whose length does not match its function code SHALL be rejected with reason 3. | — | T |
| APP-025 | Event codes (§3.3.2) and command function codes (§3.3.5) SHALL never be renumbered or reused; new ones take the next free value. | — | I |
| APP-026 | Every frame of the scenario SHALL carry the spacecraft ID of §3.4, the same for downlink and uplink. Until OI-26 is resolved the value is provisional and SHALL be labelled fictional. | 132.0 §4.1.2.2.3; 232.0 §4.1.2.5 | I |

## 4. Link plan

### 4.1 TM downlink (A2 on)

| Level | Managed parameter | Value | Source |
|---|---|---|---|
| Physical channel | Transfer Frame Version Number | 1 (binary `00`) | 132.0 Table 5-1 |
| | Transfer Frame Length | 223 octets | 132.0 Table 5-1, §4.1.1.2 |
| | Presence of Frame Error Control | Present | 132.0 Table 5-1, §4.1.6 |
| | Valid Spacecraft IDs | {SCID of §3.4} | 132.0 Table 5-1 |
| | MC Multiplexing Scheme | One master channel | 132.0 Table 5-1 |
| Master channel | Valid VCIDs | {0, 1, 7} | 132.0 Table 5-2 |
| | VC Multiplexing Scheme | `FixedPriority` (LNK-031): VC 0, then VC 1; an OID frame on VC 7 when neither has a released data field | 132.0 Table 5-2, §4.2.4.3 |
| | Presence of MC_FSH | Absent | 132.0 Table 5-2 |
| | Presence of MC_OCF | Present: the CLCW of the uplink VC, in every frame including OID frames | 132.0 Table 5-2, §4.1.5.3 |
| Virtual channels 0, 1, 7 | Data Field Content | Packets | 132.0 Table 5-3, §4.1.4.6.3 |
| | Presence of VC_FSH, VC_OCF | Absent | 132.0 Table 5-3 |
| Packet transfer (VC 0, 1) | Valid Packet Version Numbers | {`000`} (Space Packet) | 132.0 Table 5-4; 133.0 §4.1.3.2.2 |
| | Maximum Packet Length | 512 octets | 132.0 Table 5-4 |
| | Incomplete packets delivered | Not required | 132.0 Table 5-4 |

Frame layout: primary header 6 ‖ data field **211** ‖ OCF 4 ‖ FECF 2 (CMN-040). 223 octets is valid uncoded (at most 2048 octets for TM, 131.0-B-6 §11.3), equals the Reed-Solomon length (255 − 2E − q) · I with E = 16, q = 0, I = 1 (131.0 §11.6.3), and is a Turbo information block length (131.0 §11.8.2), so the scenario stays valid when coding is added (roadmap §7).

| Policy | VC 0 | VC 1 | VC 7 |
|---|---|---|---|
| Carries | HK, events | Payload, dump | OID frames only (OI-11 default; 132.0 §4.1.4.6.3 note 2) |
| Mux priority | 1 (highest) | 2 | used only when VC 0 and VC 1 have nothing released (LNK-030) |
| Release deadline after the first octet is written (LNK-012) | 250 000 µs | 1 000 000 µs | — |
| Idle fill on deadline release (LNK-013) | Space Packet Idle Packet (APID 2047), idle data octets `0x00` (SPP-010), continuing into the next data field if fewer than 7 octets remain | same | — |

If OI-11 is resolved differently, the scenario follows it with a new trace version (APP-087).

### 4.2 TC uplink (A2 on) and COP-1 (A3 on)

| Level | Managed parameter | Value | Source |
|---|---|---|---|
| Physical channel | Maximum Transfer Frame Length | 128 octets | 232.0 Table 5-1 |
| | Presence of Frame Error Control | Present | 232.0 Table 5-1, §4.1.4 |
| | Maximum Number of Transfer Frames Given to the Coding Sublayer as a Single Data Unit | 1 | 232.0 Table 5-1 |
| | Maximum Length of Data Unit Given to the Coding Sublayer | 128 octets | 232.0 Table 5-1 |
| | Maximum Repetitions | 1 (no systematic retransmission) | 232.0 Table 5-1, §2.4.2 |
| Master channel | Maximum Transfer Frame Length; Valid VCIDs | 128; {0} | 232.0 Table 5-2 |
| Virtual channel 0 | COP in Effect; CLCW Version Number | 1; 1 | 232.0 Table 5-3 |
| | CLCW Reporting Rate | 8 per second (every TM frame, §4.1) | 232.0 Table 5-3, §4.2.1.1.1 note 3 |
| | Presence of Segment Header; Valid MAP IDs | Present; {0, 1} | 232.0 Table 5-3, §4.1.3.2.1.4 |
| | Repetitions for Type-A frames; for control commands | 1; 1 | 232.0 Table 5-3 |
| MAP 0: commands | Data Field Content; Blocking; Segmentation | Packets; Permitted; Prohibited | 232.0 Table 5-4 |
| MAP 1: table upload | Data Field Content; Blocking; Segmentation | Packets; Prohibited; Permitted | 232.0 Table 5-4 |
| MAP 0 and 1 | Maximum Frame Data Unit Length | 121 octets (Segment Header 1 + 120) | 232.0 Table 5-4, §4.1.3.2.1.1 |
| Packet transfer | Valid PVNs; Maximum Packet Length; incomplete packets | {`000`}; 512; Not required | 232.0 Table 5-5 |

Frame layout: primary header 5 ‖ Segment Header 1 ‖ user data ≤ 120 ‖ FECF 2. The table-upload packet (309 octets) becomes three segments (120, 120, 69). Commands released at one instant are blocked into one FDU on MAP 0 in release order while they fit (232.0 §4.3.1.8; LNK-040, LNK-071). Once a MAP 1 packet has started, no MAP 0 FDU is interleaved until it ends (LNK-041); otherwise MAP 0 goes first.

| COP-1 parameter | Value | Source |
|---|---|---|
| Service (A2) | Type-BD frames, N(S) = 0 (COP-010, TC-008); the flight node's CLCW reports the FARM-B Counter (COP-034), Lockout = Wait = Retransmit = 0, Report Value 0 | 232.0 §4.2.1.9 |
| Service (A3 on) | Type-AD for every FDU; Type-BC for Unlock and Set V(R) | 232.1 §5.2 |
| CLCW Status Field; No RF Available; No Bit Lock | `000`; 0; 0 | 232.0 §4.2.1.4 |
| FOP_Sliding_Window_Width K | 4 (≤ PW; the 3-segment table upload and blocked commands exercise the window) | 232.1 §5.1.12, Table 7-1 |
| FARM_Sliding_Window_Width W | 8, retransmission allowed: PW = NW = 4 | 232.1 §6.1.8.2, §6.1.8.3.1, Table 7-2 |
| Transmission_Limit; Timeout_Type | 3; 0 (Alert when the limit is reached) | 232.1 §5.1.10.2–3, Table 7-1 |
| T1_Initial | 1 000 000 µs (≥ 395 000 µs, the sum below) | 232.1 §5.1.9.2 |
| FARM-1 buffer model | Internal, depth 2 (COP-032) | 232.1 §6.3.2.3 |

| 232.1 §5.1.9.2 delay | Value in the link model (§6) |
|---|---|
| a) processing below FOP-1 at the sending end | ≤ 250 000 µs: wait for the next uplink slot |
| b) transmitting a maximum-length frame | 0: contained in the slot |
| c) propagation, sending to receiving end | 10 000 µs |
| d) processing below FARM-1 | 0 |
| e) sampling FARM-1 status into a CLCW | ≤ 125 000 µs: next downlink slot |
| f) sending the CLCW out | 0: sent in that slot |
| g) propagation, opposite direction | 10 000 µs |
| h) extracting the CLCW and delivering it to FOP-1 | 0 |
| **Sum** | **395 000 µs** |

### 4.3 AOS and USLP variants (A4)

| Variant | Downlink | Uplink | Notes |
|---|---|---|---|
| `aos` | AOS, same frame length, VC and mux plan, and OCF use as §4.1 | TC as §4.2 | Parameter table added in A4 |
| `uslp` | USLP fixed-length frames | USLP variable-length frames, COP-1 over USLP ([cop1.md](standards/cop1.md) §2.1) | No Encapsulation Packet user data until an EPI is available from SANA (OI-06): EPP appears only as Encapsulation Idle Packets (LNK-013) |

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-030 | The TM physical channel SHALL use version 1 frames of 223 octets with the FECF present, one master channel with the SCID of §3.4. | 132.0 Table 5-1; 131.0 §11.3, §11.6.3, §11.8.2 | T |
| APP-031 | The master channel SHALL have valid VCIDs {0, 1, 7}, no MC_FSH, and the MC_OCF present in every frame, OID frames included, carrying the latest CLCW of the uplink VC (LNK-033). | 132.0 Table 5-2, §4.1.5.3; 232.0 §4.2.1.1.1 note 3 | T |
| APP-032 | VCs 0, 1, and 7 SHALL carry Packets, without VC_FSH or VC_OCF, with the packet-transfer parameters of §4.1. | 132.0 Tables 5-3, 5-4 | T |
| APP-033 | The downlink SHALL multiplex with `FixedPriority`, VC 0 before VC 1 (LNK-031), release partly filled data fields after the deadlines of §4.1 (LNK-012), and complete them with Idle Packets (LNK-013, SPP-010). | 132.0 §4.2.2.5, §4.2.4.3 | T |
| APP-034 | When no data field is released at a frame slot, the flight node SHALL send an OID frame on VC 7 (LNK-030, LNK-014), with one OID generator for the run (CMN-021, LNK-036). | 132.0 §4.1.4.6, §4.2.4.4 | T |
| APP-035 | MC and VC frame counts SHALL start at 0 when the run starts (LNK-032). | 132.0 §4.1.2.5–6 | T |
| APP-036 | The ground node SHALL validate every TM frame (TM-001…TM-011), track frame counts and report gaps (LNK-051), extract packets (LNK-060…064), and feed the CLCW from the OCF to its commanding (LNK-053). | 132.0 §4.3 | T |
| APP-040 | The TC physical channel SHALL use frames of at most 128 octets with the FECF present, one frame per data unit, and no systematic retransmission. | 232.0 Table 5-1, §2.4.2 | T |
| APP-041 | The uplink SHALL use VC 0 with a Segment Header, MAPs 0 and 1 configured as §4.2, and a maximum FDU length of 121 octets. | 232.0 Tables 5-2…5-5 | T |
| APP-042 | Command packets released at one instant SHALL be blocked into one FDU on MAP 0 in release order while they fit (LNK-040, LNK-071); table-upload packets SHALL be segmented on MAP 1 without interleaving (LNK-041, LNK-070). | 232.0 §4.3.1 | T |
| APP-043 | In A2 every FDU SHALL be sent in a Type-BD frame with N(S) = 0, and the flight node's CLCW SHALL report the FARM-B Counter of the accepted Type-B frames (COP-034, COP-035). | 232.0 §4.2.1.9; 232.1 §6.3 | T |
| APP-044 | From A3 every FDU SHALL be sent on the AD service through FOP-1, with K = 4, T1_Initial = 1 000 000 µs, Transmission_Limit = 3, and Timeout_Type = 0, and FARM-1 SHALL use W = 8 with retransmission allowed (PW = NW = 4). | 232.1 §5.1.9.2, §5.1.10, §5.1.12, §6.1.8.2–3, Tables 7-1, 7-2 | T |
| APP-045 | The AOS and USLP variants of §4.3 SHALL each have their own golden traces, the same packet projection as the TM/TC nominal case (APP-003), and SHALL exist before A6 exits. Their parameter tables SHALL be added as new requirements in A4. | — | T |

## 5. Time

| Item | Value | Source |
|---|---|---|
| Code | CUC Level 1: time code ID `001`, epoch 1958 January 1 TAI, no leap seconds | 301.0 §3.2.1–3.2.2; TIME-014 |
| Format | 4 coarse octets, 2 fractional octets (resolution 2⁻¹⁶ s) | 301.0 §3.2.2; TIME-010 |
| Explicit P-field | `0x1E` | 301.0 §3.2.2 |
| T0 | Coarse 2 100 000 000 s (`0x7D2B7500`), fraction 0 | — |
| On-board time | T0 + `now`, no drift, no correction | — |
| Ground time | The `Instant` at which a unit is delivered | — |

Every on-board time stamp and every execution time in the scenario falls on a multiple of 125 000 µs, so the 2-octet fraction is exact and no rounding rule is involved.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-060 | On-board time SHALL be CUC Level 1 with 4 coarse and 2 fractional octets, equal to T0 + `now`, with T0 = 2 100 000 000 s, and SHALL be converted with `aldis-time` (TIME-011). | 301.0 §3.2.1–3.2.2 | T |
| APP-061 | Every on-board generation instant and every execution time SHALL be a multiple of 125 000 µs. | — | I |
| APP-062 | The ground node SHALL take the delivery `Instant` as its time and SHALL NOT correct on-board time (no correlation, offset, or drift in scenario version 1). | — | I |

## 6. Link model and faults

### 6.1 Clock, slots, and delay

| Item | A1 (packets) | A2 on (frames) |
|---|---|---|
| Clock | Simulated `Instant` from 0 µs (API-003), owned by the harness | same |
| Downlink unit | Each Space Packet the flight node outputs, sent at its output instant | One TM frame per **frame slot**, every 125 000 µs from t = 0; the flight node must supply one (LNK-030) |
| Uplink unit | Each Space Packet the ground node outputs, sent at its output instant | At most one TC frame per **uplink slot**, every 250 000 µs from t = 0 |
| One-way delay | 10 000 µs, both directions | same |
| Run | From t = 0 to the case's end instant, exclusive | same |

### 6.2 Scheduler and tie order

The harness advances to the earliest of: frame and uplink slots, link arrivals, ops-script instants, and each node's reported deadline (API-053). At each instant t it does, in this order:

| Step | Action |
|---|---|
| 1 | Deliver every unit arriving at t, ordered by (send instant, downlink before uplink, unit index, copy); record `unit_delivered` |
| 2 | Step the ground node if it has deliveries, ops-script entries, an uplink slot, or a deadline at or before t; record its outputs in output order; send its unit, if any |
| 3 | Step the flight node likewise, with the downlink slot |

Step instants are not recorded: an implementation that asks for extra wake-ups produces the same trace, so a node SHALL behave the same whether or not it is stepped at an instant where nothing is due.

### 6.3 Faults

Faults are **scripted** per case, not seeded: each entry addresses one link unit by (direction, unit index), where the index counts the units sent in that direction from 0.

| Fault | Arguments | Effect |
|---|---|---|
| `drop` | — | The unit is not delivered |
| `duplicate` | — | The unit is delivered twice at the same instant, copies 0 and 1 |
| `delay` | d (µs) | The unit arrives d µs later than its normal arrival (which can reorder units) |
| `flip` | octet n, bit b (bit 0 = most significant, GEN-050) | One bit is inverted; the FECF catches it |
| `truncate` | n | Only the first n octets are delivered |

### 6.4 Cases

| Case | Level | Faults | Purpose |
|---|---|---|---|
| `a0-empty` | — | none | Harness self-test: no applications, end instant 0, header-only trace |
| `a1-nominal` | A1 | none | The timeline of §2.4 (end 35 s) |
| `a1-immediate` | A1 | none | `a1-nominal` plus immediate commands (outside APP-003); defined in A1 |
| `a2-nominal` | A2 | none | Same timeline over TM/TC frames |
| `a2-downlink-*` | A2 | Frame loss and corruption | Frame-gap and packet-sequence detection (LNK-051, SPP-011); defined in A2 |
| `a3-nominal` | A3 | none | Same timeline over COP-1; frozen from A3 (APP-004) |
| `a3-uplink-*` | A3 | Loss, duplication, reordering, a lockout and its recovery with Unlock | Retransmission and COP-1 alerts; defined in A3 |
| `a4-aos-nominal`, `a4-uslp-nominal` | A4 | none | Variants (§4.3) |

The fault scripts of the A2 and A3 cases are fixed by the milestone that adds them (APP-005).

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-070 | The harness SHALL own simulated time, starting at 0 µs, and the link; nodes SHALL see time only as the `now` of each step and SHALL NOT see each other. | API-003 | I |
| APP-071 | The link SHALL follow §6.1: one unit per output at A1; from A2 one downlink frame per 125 000 µs slot, supplied by the flight node at every slot (LNK-030), and at most one uplink frame per 250 000 µs slot. | — | T |
| APP-072 | Every unit SHALL arrive 10 000 µs after it is sent, plus the delay of a `delay` fault. | — | T |
| APP-073 | The harness SHALL advance and break ties as §6.2 defines. | — | T |
| APP-074 | A node's outputs SHALL NOT depend on whether it is stepped at instants where nothing is due for it (API-053). | API-053 | T |
| APP-075 | Faults SHALL be scripted per case as §6.3 defines, addressed by (direction, unit index), and applied by the harness only. | — | T |
| APP-076 | The cases SHALL be those of §6.4, each with its end instant, ops script, and fault script in the scenario file (APP-107). | — | I |
| APP-077 | The harness and the nodes SHALL NOT read a wall clock, use threads for anything that affects outputs, iterate hash-based containers, or use randomness (GEN-031 applied to the application). | GEN-031 | I |

## 7. Golden traces

### 7.1 Encoding

A golden trace is a **JSON Lines** file: UTF-8, one JSON object per line, each line ending in LF (also the last), no other whitespace, keys in the fixed order of §7.3, integers in decimal without sign or leading zeros, strings from the fixed vocabulary of §7.3–7.4 (no escapes), and octets as **uppercase hexadecimal** strings, as in the [test vectors](07-test-vectors.md#2-codec-vectors).

### 7.2 Header

Line 1 is the header:

`{"kind":"header","scenario":"aldis-refapp","scenario_version":1,"case":"a1-nominal","trace_version":1,"requirements":["APP-003","APP-010"]}`

| Key | Meaning |
|---|---|
| `scenario` | Always `aldis-refapp` |
| `scenario_version` | Version of this document's scenario (1); a new version only extends (APP-002) |
| `case` | Case ID of §6.4 |
| `trace_version` | Version of this golden trace, from 1, bumped by every change (§7.5) |
| `requirements` | The requirement IDs the case verifies, sorted; read by `cargo xtask trace` (VER-020) |

The implementation that produced a run (language, library build, versions) goes in a separate run manifest written next to the run's output, never in the trace.

### 7.3 Records

Every line after the header records one observable event, in the order of §6.2. Keys appear in exactly this order:

| `kind` | Keys | Recorded when |
|---|---|---|
| `packet_out` | `t_us`, `kind`, `node`, `apid`, `hex` | An application of `node` (`flight` or `ground`) generates a packet |
| `fault` | `t_us`, `kind`, `dir`, `index`, `fault`, `args` | A fault of §6.3 is applied to unit `index` in direction `dir` (`down` or `up`); recorded just before that unit's `unit_sent`; `args` is the list of §6.3 arguments, `[]` if none |
| `unit_sent` | `t_us`, `kind`, `dir`, `index`, `hex` | A node outputs a link unit (the octets as sent, before faults) |
| `unit_delivered` | `t_us`, `kind`, `dir`, `index`, `copy` | A unit arrives at the other node; `copy` is 0, or 1 for the second copy of a duplicate. The delivered octets are those of `unit_sent` after the faults recorded for the unit |
| `packet_in` | `t_us`, `kind`, `node`, `apid`, `hex` | A packet is delivered to an application of `node` |
| `app_event` | `t_us`, `kind`, `node`, `code`, `name`, `args` | A node raises an application event (§7.4); `args` is a list of unsigned integers |

Library-internal events and step instants are **not** recorded, so the hand-written loops (A2, A3) and the engines (A4) can produce the same trace.

### 7.4 Application events

A closed vocabulary with stable codes, mapped by each node from library events and its own state, in the spirit of API-080. Codes are never renumbered or reused. A2 adds frame-level codes in 100–199, A3 COP-1 codes (FOP-1 notifications and alerts, FARM-1 state) in 200–299.

| Code | `name` | Node | `args` |
|---|---|---|---|
| 1 | `seq_gap` | ground | APID, missing count |
| 2 | `seq_repeat` | ground | APID, sequence count |
| 3 | `seq_reset` | ground | APID, sequence count |
| 4 | `dump_reassembled` | ground | address, length, CRC-16 (CMN-001) of the memory octets |
| 5 | `dump_discarded` | ground | APID, octets discarded |
| 6 | `packet_rejected` | either | error code of the catalogue in [03 §9](03-api-model.md#9-error-catalogue) |
| 20 | `cmd_received` | flight | function code, command sequence count |
| 21 | `table_received` | flight | table ID, length |

### 7.5 Comparison, location, and versioning

| Item | Rule |
|---|---|
| Comparison | Byte equality of the whole file. On a mismatch the harness prints the first differing record, decoded with the strict codecs |
| Strict decoding | CI decodes every `unit_sent` and every packet of every golden trace with the strict codecs (from A1) |
| Location | `refapp/golden/<case>.jsonl`, owned by track A |
| Versioning | A change to a released golden trace bumps its `trace_version` and records the reason in `refapp/golden/CHANGES.md`, as TV-021 does for vectors |
| Vectors | Golden traces are whole-system regression baselines, **not** conformance vectors: a frame or packet from a golden trace becomes a vector only with `hand-derived` or `cross-checked` provenance (TV-020; OI-23) |
| Framework trace | Each node's step inputs (`now`, delivered units, slot) can be extracted from a golden trace, so that M6 can define the FW-013 trace to match and A6 can replay both nodes (OI-25) |

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-080 | Golden traces SHALL be encoded as §7.1 defines, and the harness SHALL reject a trace that is not in that canonical form. | — | T |
| APP-081 | Line 1 of every golden trace SHALL be the header of §7.2, listing the requirement IDs the case verifies. | VER-020 | T |
| APP-082 | Every further line SHALL be one record of §7.3, with the keys in the order given there, in the processing order of §6.2. | — | T |
| APP-083 | Application events SHALL use the codes and names of §7.4. | API-080 | T |
| APP-084 | Golden traces SHALL NOT contain library-internal events, step instants, or the identity of the implementation; the implementation SHALL be recorded in a separate run manifest. | — | I |
| APP-085 | Traces SHALL be compared by byte equality, and a mismatch SHALL be reported with the first differing record. | — | T |
| APP-086 | From A1, CI SHALL decode every link unit and packet of every golden trace with the strict codecs. | API-030 | T |
| APP-087 | Golden traces SHALL be kept in `refapp/golden/<case>.jsonl`; every change to a released trace SHALL bump its `trace_version` and add the reason to `refapp/golden/CHANGES.md`. | — | I |
| APP-088 | A frame or packet taken from a golden trace SHALL become a conformance vector only with `hand-derived` or `cross-checked` provenance (TV-020). | — | I |

## 8. Node interface

### 8.1 Step contract

A node is a sans-I/O state machine with one operation:

```
step(now, input) -> outputs, next_deadline
  input   = { delivered: [unit], ops: [ops-script entry index], tx_slot: bool }
  outputs = ordered list of: unit(octets) | packet_out(apid, octets) | packet_in(apid, octets) | app_event(code, args)
```

At A1, `tx_slot` is true in every step and a node may output any number of units. From A2, the flight node outputs exactly one unit when `tx_slot` is true and none otherwise; the ground node outputs at most one unit when `tx_slot` is true and none otherwise. `next_deadline` is the earliest instant at which the node needs a step, or none. Rust nodes (A1–A4) implement this as a trait in-process.

### 8.2 Node protocol (A5, A6; OI-24)

The A5 C++ flight node and the A6 bare-metal flight node under QEMU run as child processes and speak this protocol in lockstep over stdin/stdout, so the link, clock, faults, and trace writer stay in the harness. Each message is a 4-octet big-endian body length followed by the body; all integers are big-endian.

| Type | Direction | Body after the type octet |
|---|---|---|
| `0x01` HELLO | harness → node | protocol version (u16, 1), scenario version (u16), role (u8: 0 flight, 1 ground), case ID length (u8) ‖ ASCII case ID |
| `0x02` STEP | harness → node | `now` (u64 µs), `tx_slot` (u8), unit count (u16) ‖ each: length (u16) ‖ octets, ops count (u16) ‖ each: entry index (u16) |
| `0x03` END | harness → node | — |
| `0x81` HELLO_ACK | node → harness | protocol version (u16) |
| `0x82` STEP_RESULT | node → harness | output count (u16) ‖ each: kind (u8: 1 unit, 2 packet_out, 3 packet_in, 4 app_event) ‖ unit: length (u16) ‖ octets; packet: APID (u16) ‖ length (u16) ‖ octets; event: code (u16) ‖ count (u8) ‖ u64 each; then `next_deadline` (u64, all ones = none) |
| `0x83` END_ACK | node → harness | — |

The harness sends one message and waits for its answer; it never reads the wall clock (CI job timeouts bound a hung node). A malformed message, an unexpected type, or a closed stream ends the run with an error and no trace. The protocol is implemented and may be refined in A5, before any trace depends on it.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-090 | Every node SHALL implement the step contract of §8.1, including the `tx_slot` rules per link level. | LNK-030 | T |
| APP-091 | Nodes SHALL be sans-I/O: they SHALL NOT read clocks, perform I/O, or hold global state (GEN-030…032 applied to the application). | GEN-031 | I |
| APP-092 | A node running out of process SHALL use the node protocol of §8.2, and the harness SHALL produce the same trace for an in-process and an out-of-process node with the same behaviour. | — | T |

## 9. Implementation rules

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-100 | `refapp/` crates SHALL depend on Aldis only through the `aldis` facade, by path, with default features off and only the features each crate uses. They SHALL NOT depend on any other `aldis-*` crate, on `aldis-testkit`, or on a `test-hooks` feature, also not as dev-dependencies, and SHALL NOT use items hidden from the documentation. From A5 the C++ node SHALL use only `aldis.h` and `aldis.hpp` (CABI-072). | — | A/I |
| APP-101 | Every API pain point found in `refapp/` SHALL be filed as a GitHub issue labelled `api-feedback` for track R; the workaround SHALL cite the issue, and each A-milestone plan SHALL list its findings. `refapp/` SHALL NOT ask for an application-specific API. | — | I |
| APP-102 | `refapp-flight` SHALL be `no_std` without a heap and SHALL pass the flight lint set (VER-001); it SHALL build for `thumbv7em-none-eabihf`. From A6 it SHALL also pass the panic-freedom link check (VER-010 method). | GEN-010, GEN-020 | A |
| APP-103 | `refapp-ground` SHALL pass the flight lint set's restriction lints, because it parses untrusted downlink; it MAY use `std`. | GEN-020 | A |
| APP-104 | `refapp-scenario` and `refapp-harness` SHALL use the tooling lint profile (RS-017). | — | A |
| APP-105 | Every crate in `refapp/` SHALL declare `#![forbid(unsafe_code)]`. | GEN-070 | A |
| APP-106 | `refapp/` SHALL be a separate Cargo workspace on the pinned toolchain (RS-001). CI SHALL check its formatting (VER-004), Clippy, tests, the `thumbv7em-none-eabihf` build of `refapp-flight`, and its licences and advisories (VER-003); from A1 CI SHALL reproduce every golden trace. | VER-001 | A |
| APP-107 | All scenario values SHALL come from one language-neutral file in `refapp/scenario/`, validated against a published JSON Schema; the Rust constants SHALL be generated from it (a C++ header from A5), and CI SHALL check that the generated files are current. | — | A |
| APP-108 | Tests in `refapp/` SHALL carry `// req:` annotations (VER-020), and `cargo xtask trace` SHALL read them and the `requirements` of golden-trace headers. | VER-020 | A |
