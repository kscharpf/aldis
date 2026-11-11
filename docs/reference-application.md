# Reference application — scenario

**Area:** APP · **Track:** A ([roadmap](08-roadmap.md) §3) · **Plan:** [A0 Scenario](milestones/a0-scenario.md) (decisions D1–D16); §10 [ACFS cFS flight side](milestones/acfs-flight-side.md) (decisions D1–D19) · **Code:** `refapp/` · **Scenario version:** 1 (draft for maintainer review)

## Summary

| Item | Value | Section |
|---|---|---|
| Story | A small Earth-orbiting spacecraft and its control centre: a **flight node** and a **ground node**, both sans-I/O, connected by a harness that owns time, the link, faults, and the trace | §2 |
| Flight applications | On-board time, housekeeping (1 Hz), event reporter, payload (0.5 Hz, packets longer than a frame), memory dump (segmented packets), command handler (immediate and time-tagged), table store | §2.1 |
| Ground applications | Telemetry reception and decoding, packet-sequence tracking, time-stamp decoding, dump reassembly, scripted operations (the ops script); FOP-1 from A3 | §2.2 |
| APIDs | `0x010` HK, `0x011` events, `0x020` payload, `0x021` dump, `0x080` commands, `0x081` table upload, `2047` idle | §3.1 |
| Spacecraft ID | `0x155` (341), **fictional**, confirmed by the maintainer (OI-26) | §3.4 |
| Downlink | TM, 223-octet frames, FECF, MC_OCF with the CLCW, VC 0 (HK, events), VC 1 (payload, dump), VC 7 (OID frames only), fixed-priority mux, 8 frames/s | §4.1 |
| Uplink | TC, frames ≤ 128 octets, FECF, one VC with Segment Header, MAP 0 commands (blocking), MAP 1 table upload (segmentation); Type-BD in A2; COP-1 from A3 (K = 4, W = 8, T1_Initial = 1 s, Transmission_Limit = 3) | §4.2 |
| Time | CUC Level 1, 4 coarse + 2 fractional octets, T0 = 2 100 000 000 s, no drift | §5 |
| Link model | Simulated µs clock from 0; one-way delay 10 ms; downlink slot 125 ms; uplink slot 250 ms; scripted faults by (direction, unit index) | §6 |
| Golden traces | Canonical JSON Lines in `refapp/golden/<case>.jsonl`; header line + one record per observable event; byte comparison | §7 |
| Node interface | `step(now, inputs) -> outputs + next deadline`; in-process for Rust, a lockstep protocol for the A5 C++ and A6 QEMU flight nodes over stdin/stdout (OI-24) and for the cFS node over loopback TCP (§10.6) | §8 |
| Code rules | `aldis` facade only; flight node `no_std`, no heap, flight lint set; `forbid(unsafe_code)` everywhere | §9 |
| Interoperability | The A3 nodes against YAMCS 5.14.0 as a black box over a UDP transport driver: managed-parameter table, driver rules, pass criteria | §11 |
| cFS flight side | A second flight node: cFS apps on cFE, OSAL, and the pc-linux PSP calling Aldis through `aldis.h`, in lockstep with the harness, reproducing the A1–A3 golden traces in three phases; plan [ACFS](milestones/acfs-flight-side.md), decisions accepted 2026-10-21 | §10 |

**Open items that affect this document:** none. Resolved: OI-86 (order of deliveries and ops entries, §2.2), OI-87 (queue-full check, §2.3), OI-88 and OI-89 (`packet_rejected` of the ground node, APP-130), OI-90 (count of the Idle Packets, §4.4.4), all 2026-11-08; OI-11 (OID frames, §4.1; 2026-11-01), OI-23 (golden traces and vectors, §7.5), OI-24 (out-of-process nodes, §8.2), OI-25 (framework trace, §7.5), OI-26 (spacecraft ID, §3.4), OI-06 (SANA registries, §3.1, §4.3). Every protocol value below cites the Blue Book clause it was read from.

**How to read the requirements.** Requirement IDs follow [00 §5](00-introduction.md#5-requirement-conventions), grouped in decades per section. **T** means a golden-trace reproduction test or a harness test in `refapp/` annotated `// req:`, and the header of each golden trace lists the IDs its case verifies (§7.2). **I** means review of this document or of `refapp/`. **A** means a CI check. Requirements name the library requirements they exercise.

## 1. Purpose and scope

The reference application shows an integrator how the libraries fit together end to end, gives the library tracks API feedback before the v1.0 freeze, and shows that the Rust-backed and native libraries are interchangeable: every implementation must reproduce the same golden traces byte for byte (GEN-030). It is built once, in A1, and grown by A2–A6 ([plan](milestones/a0-scenario.md) §2.1):

| Milestone | Link level | Adds |
|---|---|---|
| A1 | Packets: one Space Packet per link unit | All applications, the nominal timeline, the first golden traces |
| A2 | Frames: TM down, TC Type-BD up, through the `aldis-link` engines | Downlink and uplink fault cases |
| A3 | COP-1: FOP-1 on the ground, FARM-1 on board, through the COP-1 engines | Uplink faults; the link-level trace is frozen from here on |
| A4 | AOS and USLP variants, on the same engines | Nothing on the TM/TC link |
| A5 | C++ flight node over `aldis.hpp` (engine handles) and `aldis.h` (codecs), Rust-backed and native | Nothing |
| A6 | Flight node bare-metal under QEMU, with the application's own run loop (the `aldis-framework` stacks come after SDLS and TM coding, roadmap §7) | Nothing |

Not covered: synchronization and channel coding, SDLS, CFDP, and time correlation (00 §2.1).

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-001 | The reference application SHALL consist of a flight node and a ground node that exchange link units only through the harness (§6), and SHALL use only the public Aldis APIs (APP-100). | — | I |
| APP-002 | The **timeline** (what each application does, and when, §2.4) SHALL be fixed in A1 and only extended. A later scenario version MAY add applications, commands, ops-script entries, record kinds, event codes, or cases, and SHALL NOT change or remove an existing one. | — | I |
| APP-003 | For the nominal case, the **packet projection** SHALL be identical at every link level (A1, A2, A3, the A4 variants) and in every implementation. The packet projection is, for each node, record kind (`packet_out`, `packet_in`), and APID, the ordered sequence of packet octets (§7.3). Order across APIDs and instants are not part of it. | — | T |
| APP-004 | From A3 on, the link-level traces, made by the `aldis-link` engines, SHALL be frozen: the A5 and A6 implementations and the cFS flight node SHALL reproduce the golden traces of every `a3-*` case byte for byte. | GEN-030 | T |
| APP-005 | A later milestone SHALL add cases and SHALL NOT redefine an existing case. A released golden trace SHALL change only through a new trace version (APP-087). | — | I |
| APP-006 | Link units SHALL be whole Space Packets (A1) or whole Transfer Frames (A2 on), without synchronization or channel coding. Every channel SHALL use a zero-length security layout (the no-security hook, API-073). | — | I |

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

Within one instant the flight node works in this order: (1) handle delivered link units and the packets they carry, in delivery order (immediate commands execute here); (2) execute the time-tagged commands due, in queue order; (3) emit a due dump segment and, after the last segment of a dump, `DUMP_COMPLETED` (so the event follows the last dump packet and precedes the housekeeping and payload packets of the same instant); (4) housekeeping; (5) payload. This order is part of the timeline (APP-002).

**Frame level (A2 on).** Steps 1 to 5 are unchanged and generate the packets as at A1. Each packet is submitted to the `PacketGenerator` of its VC when it is generated (VC 0: APIDs `0x010` and `0x011`; VC 1: `0x020` and `0x021`), so a field is started at the instant of the packet that opens it (LNK-012). In a step with `tx_slot` the node then does (6): for VC 0, then VC 1, feed `Timeout` to the generator if its `next_deadline` is not after `now`, and, if the multiplexer holds no field of that VC, feed `Release` and `Offer` the field it returns; set the OCF slot (§4.4.1); feed `Opportunity` and output the frame, which is the last output of the step. A packet generated at a slot instant is in the field of that instant's frame only if the field is releasable then (full, or past its deadline); otherwise it waits in the field. Step 6 runs only in a step with `tx_slot` (§4.4.2).

**COP-1 level (A3 on).** Steps 1 to 6 are unchanged, and FARM-1 now also sees the Type-AD and Type-BC frames: step 1 handles a delivered TC frame as §4.5.3 and APP-146 say.

### 2.2 Ground node

| Application | Behaviour |
|---|---|
| Telemetry reception | Decodes link units with the strict codecs (A1: Space Packets; A2 on: TM frames, packet extraction) and delivers every telemetry packet (`packet_in`) |
| Sequence tracking | Classifies the sequence count of every telemetry packet per APID (SPP-011); a gap, repeat, or reset becomes an application event (§7.4) |
| Time-stamp decoding | Decodes the CUC time code of every packet that carries one (implicit or explicit P-field, SPP-008) |
| Dump reassembly | Rejoins the dump segments (SPP-013) and reports the reassembled dump as an application event |
| Ops script | Releases commands and the table upload at the instants of §2.4 (`packet_out`); from A3 also issues FOP-1 directives |
| Commanding (A3 on) | FOP-1 sending end; transfer and directive notifications become application events |

Within one instant the ground node works in this order: (1) handle the delivered link units, in delivery order (at A2 on, the frame-level events of §7.4 come first, then the packets and events as at A1); (2) release the ops-script entries due, in script order (`packet_out`; at A2 on the packet goes to the `MapSegmenter` and not out); (3) A2 on, in a step with `tx_slot`: ask the segmenter for one FDU and, if it hands one out, output the frame as the last output of the step. The deliveries come before the ops entries, as the flight node's step 1 comes before its other steps (OI-86, resolved 2026-11-08; no case of A1 or A2 has a delivery and an ops entry at one instant). The FDU is taken after the entries, so that the two commands released at 5.0 s in §2.4 share one FDU (APP-042).

**COP-1 level (A3 on).** The ground node works in the order of §4.5.1 (APP-144): the OCF of each delivered frame goes to FOP-1 within step (1), `Timeout` follows the deliveries, the ops entries (directives included) come next, and in a step with `tx_slot` the segmenter's FDU is requested of FOP-1 and the frame FOP-1 holds is output as the last output of the step.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-132 | From A2 on, every packet of the flight node SHALL be submitted to the generator of its VC when it is generated, and in a step with `tx_slot` the node SHALL do, after steps 1 to 5 of §2.1 and in this order: signal the deadline to each generator whose deadline has passed, take the next field from the generator of VC 0 and then of VC 1 and offer it to the multiplexer where the multiplexer holds no field of the VC, set the OCF slot (APP-134), ask the multiplexer for the frame, and output the frame as the last output of the step. In a step without `tx_slot` it SHALL do none of this. | 132.0 §4.2.2.2, §4.2.2.5, §4.2.4.3–4; LNK-012, LNK-076 | T |
| APP-133 | From A2 on, the ground node SHALL handle in each step, in this order, the delivered units, the ops-script entries due, and, in a step with `tx_slot`, one FDU of the segmenter as a frame (the last output of the step). | LNK-043, LNK-071 | T |

### 2.3 Commands and time tags

| Rule | Value |
|---|---|
| Immediate command | Executes in the step in which it is delivered (§2.1 order, step 1) |
| Time-tagged command | Queued at delivery; executes in the first step at or after its execution time; equal execution times execute in delivery order |
| Time-tag queue capacity | 8 commands |
| Verification report | `CMD_EXECUTED` or `CMD_REJECTED` (§3.3.2) at the execution instant; checks of function code and arguments happen at execution |
| Rejected at delivery | A time tag earlier than the delivery instant (reason 2), or a full queue (reason 4); the full-queue check is made at delivery, before the commands due at that instant leave the queue (OI-87). A time tag equal to the delivery instant is not earlier: the command is queued and executes in that step (APP-012) |
| Nominal case | Uses only time-tagged commands, each released at least 5 s before its execution time, so that no on-board packet depends on uplink latency (APP-003) |

### 2.4 Nominal timeline (case `nominal`)

Applications generate packets while t < 30 s; the run ends at t = 35 s, so every packet is delivered at every link level. Execution times are on-board times T0 + the instant shown. Command sequence counts (APID `0x080`) follow release order from 0.

| Release (s) | Entry | From | Execution (s) | Expected on-board result |
|---|---|---|---|---|
| 0.0 | FOP-1 directive *Initiate AD Service (without CLCW Check)* | A3 | — | FOP-1 Active (232.1 §4.1; §4.5.2, §4.5.5) |
| 1.0 | Command `NOOP`, seq 0 | A1 | 6.0 | `CMD_EXECUTED` |
| 2.0 | Table upload, table 1, 300 octets (APID `0x081`, seq 0) | A1 | — | Stored; no report |
| 3.0 | Command `ACTIVATE_TABLE` (table 1, CRC `0x0C4F`), seq 1 | A1 | 10.0 | `CMD_EXECUTED`, `TABLE_ACTIVATED` |
| 4.0 | Command `DUMP_MEMORY` (address `0x00010000`, 600 octets), seq 2 | A1 | 12.0 | `CMD_EXECUTED`; five dump packets at 12.000…12.500 s; `DUMP_COMPLETED` at 12.5 s |
| 5.0 | Command `SET_PAYLOAD_MODE` (off), seq 3 | A1 | 20.0 | `CMD_EXECUTED`, `PAYLOAD_MODE` (0) |
| 5.0 | Command `NOOP`, seq 4 (same instant: blocked with seq 3 from A2, LNK-071) | A1 | 20.0 | `CMD_EXECUTED` |
| 6.0 | Command `SET_PAYLOAD_MODE` (on), seq 5 | A1 | 24.0 | `CMD_EXECUTED`, `PAYLOAD_MODE` (1) |

The resulting telemetry: 30 HK packets, 13 payload packets (t = 0.5…18.5 s and 24.5…28.5 s), 5 dump packets, and 10 event packets. Derived values, cross-checked in A1 WP-1: the CRC-16 (CMN-001) of the 300 table octets is `0x0C4F`, and of the 600 dumped memory octets `0x80A8`. Both were recomputed from the defining expression of 132.0 §4.1.6.2.2 (the message polynomial with the preset term, reduced modulo the generator by long division over GF(2)), not from the shift-register figure or from the library, and the same evaluation gives `0x29B1` for the ASCII octets "123456789" (CMN-002). The ground node's `dump_reassembled` event of this case therefore has the arguments 65 536, 600, 32 936 (§7.4).

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-010 | The flight node SHALL implement the applications of §2.1, in the per-instant order of §2.1; a housekeeping packet SHALL report the state after steps 1–3 of its instant (§3.3.1). | — | T |
| APP-011 | The ground node SHALL implement the applications of §2.2. It SHALL decode every received link unit and packet with the strict codecs (API-030), track sequence counts per APID (SPP-011), decode time codes (SPP-008, TIME-010), and reassemble dumps (SPP-013). | 133.0 §4.1.3.4.3.3–4 | T |
| APP-012 | The command handler SHALL follow §2.3: immediate commands execute when delivered; time-tagged commands execute in the first step at or after their execution time, in delivery order for equal times; the queue holds 8 commands, and a command delivered to a full queue is rejected with reason 4 even if a command due at that instant would leave it (OI-87). | — | T |
| APP-013 | Every command SHALL produce exactly one verification report, `CMD_EXECUTED` or `CMD_REJECTED`, at its execution instant, except the rejections at delivery of §2.3 (reasons 2 and 4), which are reported at the delivery instant, and a rejection with reason 3 of a command that has no valid execution instant (an execution mode other than `0x00` and `0x01`, a non-zero execution time in an immediate command, or an undecodable time tag), which is reported at the delivery instant too. | — | T |
| APP-014 | The nominal case SHALL use only time-tagged commands, each released at least 5 s before its execution time. | — | I |
| APP-015 | The payload application SHALL generate one packet at t = 0.5 + 2k s (k ≥ 0, t < 30 s) while its mode is *on*, and none while it is *off*; the payload packet number counts the packets generated, so a skipped slot uses none (§3.3.3). `SET_PAYLOAD_MODE` SHALL change the mode at its execution instant. | — | T |
| APP-016 | The memory dump SHALL build the dump SDU of §3.3.4 and send it as Space Packets with Sequence Flags `01`, `00`, `10` (or `11` for a single packet), at most 128 user-data octets each, one packet every 125 000 µs from the execution instant, followed by `DUMP_COMPLETED` in the step of the last packet, after that packet and before the housekeeping and payload packets of the same instant (§2.1). A `DUMP_MEMORY` command executed while a dump is in progress SHALL be rejected with reason 5. | 133.0 §4.1.3.4.2.2 | T |
| APP-017 | The table store SHALL keep the most recently uploaded table (one table of up to 503 octets, so that its packet, 9 octets longer (§3.3.6), fits the 512-octet Maximum Packet Length of §4.2, 232.0 Table 5-5). `ACTIVATE_TABLE` SHALL report reason 1 if the stored table has another ID or none was uploaded, and otherwise compare the CRC-16 (CMN-001) of the stored table with the command's argument and report `TABLE_ACTIVATED` or `TABLE_REJECTED`. | — | T |
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

An `ACTIVATE_TABLE` that raises `TABLE_REJECTED` is an executed command (APP-129) and counts in "Commands executed", not in "Commands rejected", which count the `CMD_REJECTED` reports (APP-013).

A housekeeping packet reports the state after steps 1–3 of its instant (§2.1). A command executed at that instant is counted (executed, rejected, payload mode, table, dumps completed). The queue occupancy is the number of time-tagged commands then queued, so a command delivered after the HK instant is not counted until the next HK packet. A delivery at the HK instant itself is handled in step 1 and counted. Deliveries arrive 10 000 µs after their release instant (§6.1).

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

Octets 0–3: payload packet number n (0, 1, … over all payload packets of the run). n counts only the payload packets generated, so a slot skipped while the payload mode is *off* uses no number. Octets 4–399: octet i = (n + i) mod 256. A payload packet is longer than one TM data field (211 octets, §4.1), so it spans frames (LNK-011).

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

Table ID (u8, 1…255) ‖ length L (u16, 1…503) ‖ L table octets. Every table of the scenario has octet i = (3 · i + 1) mod 256, whatever its ID and length; the nominal table is ID 1, L = 300. Its packet (309 octets) is longer than one TC frame data field, so it is segmented on MAP 1 (§4.2).

### 3.4 Spacecraft ID

TM, TC, AOS, and USLP frames carry a spacecraft ID assigned through CCSDS 320.0 and SANA (00 §6). The public SANA spacecraft ID registry was checked on 2026-10-19 and showed no value set aside for testing or documentation. **Chosen:** SCID `0x155` (341), whose alternating bit pattern (`01 0101 0101`) makes a misplaced or shifted SCID field visible in a hex dump. It fits the 10-bit TM and TC fields (132.0 §4.1.2.2.3; 232.0 §4.1.2.5) and the AOS and USLP fields. The value is fictional, confirmed by the maintainer on 2026-10-19 (OI-26); `refapp/README.md` and every golden-trace case description label it as fictional.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-020 | The scenario SHALL use the APIDs, Packet Types, secondary headers, and Sequence Flags of §3.1. No APID other than 2047 SHALL be used for Idle Packets. | 133.0 §4.1.3.3.4.2, §4.1.3.3.4.4 note 1, Annex B2.1 | I |
| APP-021 | Every APID SHALL have its own Packet Sequence Count, starting at 0 and continuing modulo 16384; telecommand packets SHALL carry a Packet Sequence Count (exercises SPP-011, SPP-012). | 133.0 §4.1.3.4.3.2–4 | T |
| APP-022 | Secondary headers SHALL hold a Time Code Field only, with the P-field form of §3.2 fixed per APID: implicit for `0x010`, `0x020`, and `0x021`, explicit (`0x1E`) for `0x011` (exercises SPP-007, SPP-008). | 133.0 §4.1.4.2.1.5, §4.1.4.2.2.4–5; 301.0 §3.2.2 | T |
| APP-023 | Housekeeping, event, payload, and dump packets SHALL have the user data of §3.3.1–3.3.4. | — | T |
| APP-024 | Command and table-upload packets SHALL have the user data of §3.3.5 and §3.3.6. A command whose length does not match its function code SHALL be rejected with reason 3. | — | T |
| APP-025 | Event codes (§3.3.2) and command function codes (§3.3.5) SHALL never be renumbered or reused; new ones take the next free value. | — | I |
| APP-026 | Every frame of the scenario SHALL carry the spacecraft ID of §3.4, the same for downlink and uplink. The value is fictional and SHALL be labelled so wherever the scenario is published (OI-26). | 132.0 §4.1.2.2.3; 232.0 §4.1.2.5 | I |

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

OI-11 is resolved as this default (2026-11-01); a later change of it would be a new trace version (APP-087).

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
| MAP 0 and 1 | Maximum Frame Data Unit Length | 121 octets, the Segment Header included (Segment Header 1 + 120) | 232.0 Table 5-4, §4.1.3.2.1.1 |
| Packet transfer | Valid PVNs; Maximum Packet Length; incomplete packets | {`000`}; 512; Not required | 232.0 Table 5-5 |

Frame layout: primary header 5 ‖ Segment Header 1 ‖ user data ≤ 120 ‖ FECF 2. The 121-octet maximum of the Frame Data Unit includes the Segment Header. The table-upload packet (309 octets) becomes three segments (120, 120, 69). Commands released at one instant are blocked into one FDU on MAP 0 in release order while they fit (232.0 §4.3.1.8; LNK-040, LNK-071). Once a MAP 1 packet has started, no MAP 0 FDU is interleaved until it ends (LNK-041); otherwise MAP 0 goes first.

| COP-1 parameter | Value | Source |
|---|---|---|
| Service (A2) | Type-BD frames, N(S) = 0 (COP-010, TC-008); the flight node's CLCW reports the FARM-B Counter (COP-034), Lockout = Wait = Retransmit = 0, Report Value 0 | 232.0 §4.2.1.9 |
| Service (A3 on) | Type-AD for every FDU; Type-BC for Unlock and Set V(R) | 232.1 §5.2 |
| CLCW Status Field; No RF Available; No Bit Lock | `000`; 0; 0 | 232.0 §4.2.1.4 |
| FOP_Sliding_Window_Width K | 4 (≤ PW; the 3-segment table upload and blocked commands exercise the window) | 232.1 §5.1.12, Table 7-1 |
| FARM_Sliding_Window_Width W | 8, retransmission allowed: PW = NW = 4 | 232.1 §6.1.8.2, §6.1.8.3.1, Table 7-2 |
| Transmission_Limit; Timeout_Type | 3; 0 (Alert when the limit is reached) | 232.1 §5.1.10.2–3, Table 7-1 |
| T1_Initial | 1 000 000 µs (≥ 395 000 µs, the sum below) | 232.1 §5.1.9.2 |
| FARM-1 delivery buffer | Caller-owned (the harness), depth 2; `buffer_available` per frame (COP-033) | 232.1 §3.3.2, §6.3.2.3 |

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
| APP-033 | The downlink SHALL multiplex with a fixed priority, VC 0 before VC 1 (LNK-031), release partly filled data fields after the deadlines of §4.1 (LNK-012), and complete them with Idle Packets (LNK-013, SPP-010). | 132.0 §4.2.2.5, §4.2.4.3 | T |
| APP-034 | When no data field is released at a frame slot, the flight node SHALL send an OID frame on VC 7 (LNK-030, LNK-014), with one OID generator for the run (CMN-021, LNK-036). | 132.0 §4.1.4.6, §4.2.4.4 | T |
| APP-035 | MC and VC frame counts SHALL start at 0 when the run starts (LNK-032). | 132.0 §4.1.2.5–6 | T |
| APP-036 | The ground node SHALL validate every TM frame (TM-001…TM-011), track frame counts and report gaps (LNK-051), extract packets (LNK-060…064), and decode the OCF as a CLCW (LNK-053): in A2 it reports a change as `clcw_changed` (§7.4) and gives nothing to FOP-1 (D2 of the A2 plan), from A3 it feeds the CLCW to FOP-1. | 132.0 §4.3 | T |
| APP-040 | The TC physical channel SHALL use frames of at most 128 octets with the FECF present, one frame per data unit, and no systematic retransmission. | 232.0 Table 5-1, §2.4.2 | T |
| APP-041 | The uplink SHALL use VC 0 with a Segment Header, MAPs 0 and 1 configured as §4.2, and a maximum FDU length of 121 octets. | 232.0 Tables 5-2…5-5 | T |
| APP-042 | Command packets released at one instant SHALL be blocked into one FDU on MAP 0 in release order while they fit (LNK-040, LNK-071); table-upload packets SHALL be segmented on MAP 1 without interleaving (LNK-041, LNK-070). | 232.0 §4.3.1 | T |
| APP-043 | In A2 every FDU SHALL be sent in a Type-BD frame with N(S) = 0, and the flight node's CLCW SHALL report the FARM-B Counter of the accepted Type-B frames (COP-034, COP-035). | 232.0 §4.2.1.9; 232.1 §6.3 | T |
| APP-044 | From A3 every FDU SHALL be sent on the AD service through FOP-1, with K = 4, T1_Initial = 1 000 000 µs, Transmission_Limit = 3, and Timeout_Type = 0, and FARM-1 SHALL use W = 8 with retransmission allowed (PW = NW = 4). | 232.1 §5.1.9.2, §5.1.10, §5.1.12, §6.1.8.2–3, Tables 7-1, 7-2 | T |
| APP-045 | The AOS and USLP variants of §4.3 SHALL each have their own golden traces, the same packet projection as the TM/TC nominal case (APP-003), and SHALL exist before A6 exits. Their parameter tables SHALL be added as new requirements in A4. | — | T |

### 4.4 Frame-level behaviour (A2 on)

What the nodes do with the plan of §4.1 and §4.2, fixed before any node exists. The order of the steps is in §2.1 and §2.2; the events are in §7.4.

#### 4.4.1 CLCW sampling

The OCF of every downlink frame, OID frames included (APP-031), is the CLCW that FARM-1 reports at the slot instant after the deliveries of that instant (step 1 of §2.1): `Farm1::clcw` converted by `Clcw::to_ocf`, set into the multiplexer's OCF slot in step 6 before the frame is taken. It therefore shows every uplink frame delivered at or before the slot instant and none sent later. An uplink frame sent at the uplink slot *u* arrives at *u* + 10 000 µs and first shows in the frame of the slot *u* + 125 000 µs (*u* is a multiple of 250 000 µs, so that is a frame slot, and the first one after the arrival).

Before the first uplink frame arrives, the CLCW is that of FARM-1 in its initial state: Control Word Type 0, version `00`, Status Field `000`, COP in Effect `01`, VCID 0, all flags 0, FARM-B Counter 0, Report Value 0, so the OCF is `01 00 00 00` (232.0 §4.2.1.2–4.2.1.11, Figure 4-6). In A2 only the FARM-B Counter changes: it is bits 21 and 22 of the CLCW and counts the Type-B frames accepted in Bypass mode, modulo 4 (232.0 §4.2.1.9; 232.1 Table 6-1, E6), so the OCF word is `0x01000000` + 512 × (counter mod 4); a counter that wraps from 3 to 0 returns to the initial word. FARM-1 accepts every Type-BD frame that passes the validation checks, also one whose FDU the reassembler later refuses, and a frame that fails them (the FECF) does not count. The Blue Books give no sampling instant (232.1 §6.2.4 and §6.3.2.4 say that FARM-1 reports its current status; 232.0 §4.2.1.1.1 note 3 asks only for some minimum rate): the instant is this scenario's rule, and the rate is every frame (§4.2).

#### 4.4.2 Release deadlines and wake-ups

A field is *started* when its first octet is written (LNK-012): at the generation instant of the packet that opens it, or, for the residue that the previous field could not hold (the tail of a packet, or of an idle packet), at the slot step in which the previous field was `Release`d. The deadlines of §4.1 (VC 0: 250 000 µs; VC 1: 1 000 000 µs) run from that instant, not from the transmission of the frame: a full field that waits in the multiplexer behind VC 0 (fixed priority) has already started the next one (the slot of 12.75 s in §4.4.4 is the case). The generator reports the deadline as `next_deadline`; the flight node reports the earliest of the two generators', the next housekeeping and payload instants, the next time-tagged command, and the next dump segment (API-053). Every deadline of the scenario falls on a frame slot, because fields start at slots and the deadlines are multiples of 125 000 µs, so none asks for a step between slots; the node reports them all the same, and since step 6 runs only with `tx_slot` (APP-132), an extra wake-up changes no output (APP-074). The ground node has no deadline in A2: FOP-1 with `RequestBd` alone keeps no timer (232.1 §5.2.6), and it reports `next_deadline` as none. At the slot step the packets of the instant are submitted first and `Timeout` is fed after them, so that a packet generated at the instant of a deadline is in the field that the deadline releases.

#### 4.4.3 Link-engine parameters

The engines of `aldis-link` and `aldis-cop1` take the values below. The file `refapp/scenario/scenario.json` and the generated constants (`refapp-scenario/src/generated.rs`) already hold the frame lengths, the VCIDs, the two release deadlines, the idle pattern, the maximum packet lengths, the MAP IDs, the maximum FDU length, the FECF flags, and the COP-1 values of §4.2. **Added by A2 WP-2** to the file, its schema, and the constants where the value is a number or a flag (APP-136, APP-107): `packet_version`, `priority_realtime`, `priority_payload`, `ocf_initial`, `deliver_incomplete_packets`, and `deliver_idle_packets` in `tm`; `packet_version`, `segment_header`, and the four blocking and segmentation flags of the two MAPs in `tc`; `farm_status_field`, `farm_initial_vr`, `fop_repetitions_type_a`, and `fop_repetitions_control` in `cop1`. The rows that name an engine policy (idle policy, multiplexing policy, quality of service, VC roles) are choices of the engine's API, fixed by the nodes, and have no value in the file:

| Engine | Parameter | Value | Source |
|---|---|---|---|
| `PacketGenerator`, `PacketExtractor`, `MapReassembler` | Valid Packet Version Numbers | {`000`} on VC 0, 1, and 7 and on the TC VC | 132.0 Table 5-4; 232.0 Table 5-5; 133.0 §4.1.3.2.2 |
| `PacketGenerator` | Idle policy; idle packet kind | `Oid` (OI-11); Space Packet Idle Packet | LNK-014, LNK-013 |
| `ChannelMux` | VC roles | VC 0 and VC 1 carry Packets; VC 7 is the VC of the OID frames (also configured for Packets) | 132.0 §4.1.4.6.3 |
| `ChannelMux` | Multiplexing policy; priorities | `FixedPriority`; VC 0 rank 1, VC 1 rank 2 (unique; rank 1 is served first); VC 7 none. The engine's `vc_priority` serves the *highest* value (LNK-031), so a node passes the rank inverted (the flight node: 255 less the rank) | LNK-031, §4.1 |
| `ChannelMux`, `FrameReceiver` (TM) | OCF and Secondary Header | MC_OCF present, MC_FSH absent, VC_OCF and VC_FSH absent; so the data field is 211 octets (223 − 6 − 4 − 2) | 132.0 Tables 5-2, 5-3 |
| `ChannelMux` | Initial value of the OCF slot | `01 00 00 00` (§4.4.1) | LNK-033 |
| `PacketExtractor` | Incomplete packets delivered; Idle Packets delivered | no; no | 132.0 Table 5-4; LNK-063 |
| `MapSegmenter`, `MapReassembler` | Segment Header on the TC VC | present | 232.0 §4.1.3.2.1.4 |
| `MapSegmenter`, `MapReassembler` | MAP 0: service, blocking, segmentation; MAP 1: the same | Packets, permitted, prohibited; Packets, prohibited, permitted | 232.0 Table 5-4 |
| `MapSegmenter` | Quality of service of a request | expedited (`RequestBd`, LNK-043) | 232.0 §4.1.2.3 |
| `Farm1` | Status Field; initial V(R); window | `000`; 0; W = 8, retransmission allowed (PW = NW = 4) | 232.0 §4.2.1.4; 232.1 §6.1.8 |
| `Fop1` | Repetitions of Type-A frames and of control commands | 1; 1 | 232.0 Table 5-3 |

The capacities of the engines (const generics) are the nodes' choice and not scenario values.

#### 4.4.4 Expected frame schedule of `a2-nominal`

Derived by hand from §4.1, §4.2, §2.4, and the rules of §2.1, §2.2, §4.4.1, and §4.4.2, in the manner of §2.4's "expected on-board result" (an independent regeneration is D8(c) of the A2 plan); `a2-nominal` reproduces it exactly (APP-137). The packets generated are those of §2.4: 30 HK packets of 44 octets, 10 event packets of 21 octets, 13 payload packets of 412 octets, and 5 dump packets of 140, 140, 140, 140, and 106 octets, in the order of §2.1. Times are slot instants; frame *i* is the unit of index *i* (`down`), sent at *i* × 125 000 µs, for *i* = 0…279 (end 35 s).

**Frame header fields.** MC Frame Count = *i* mod 256 (it wraps at *i* = 256, t = 32 s); VC Frame Count = the number of earlier frames of the VC, modulo 256; version `00`, SCID `0x155`, OCF Flag 1, no Secondary Header, Synchronization Flag 0, Packet Order Flag 0, Segment Length Identifier `11`. The first two octets are `15 51` on VC 0, `15 53` on VC 1, and `15 5F` on VC 7; the Data Field Status and First Header Pointer (FHP) are `18 00` for FHP 0, `18 C9` for 201, and `1F FE` for an OID frame (FHP `0x7FE`; 132.0 §4.1.2.7.6). Every frame ends with the OCF (§4.4.1) and the FECF.

Frame 0 begins `15 5F 00 00 1F FE`, frame 2 `15 51 02 00 18 00`, and frame 4 `15 53 04 00 18 00`.

**Totals.** 61 data frames and 219 OID frames: 31 on VC 0, 30 on VC 1, and 219 on VC 7. At the end MC count 23 (after the wrap), VC 0 count 30, VC 1 count 29, VC 7 count 218.

**VC 0.** HK packet *k* is generated at *k* s and starts a field then (or joins one the events of that instant started), so frame 8*k* + 2 (t = *k* + 0.25 s) is released by the deadline, for *k* = 0…29. Its data field is the event packets of the instant *k* s, in generation order, then HK *k*, then one Space Packet Idle Packet that fills the field to 211 octets; FHP 0. The idle packet is APID 2047, Sequence Flags `11`, count 0 (OI-90, resolved), idle data `0x00`: for 167 octets (HK alone) its header is `07 FF C0 00 00 A0` followed by 161 octets `00`. The events are: 1 at 6 s (`CMD_EXECUTED`), 2 at 10 s, 1 at 12 s, 3 at 20 s, 2 at 24 s, so the idle packet is 146, 125, 146, 104, and 125 octets in the frames of those seconds, and 167 in the other 25. One more frame: `DUMP_COMPLETED` (12.5 s) starts a field alone, so frame 102 (t = 12.75 s) holds that event packet and an idle packet of 190 octets, FHP 0.

**VC 1, payload.** Payload packet *n* is generated at T(n) = 0.5 + 2*n* s for *n* = 0…9 and at 24.5, 26.5, and 28.5 s for *n* = 10, 11, 12. Except for *n* = 6 (below), it fills field 1 with its first 211 octets at once, so frame T(n) / 0.125 s carries them, FHP 0 (no VC 0 field is ever released at a half second). Releasing field 1 starts field 2 with the other 201 octets at T(n), whose deadline is T(n) + 1 s: frame (T(n) + 1 s) / 0.125 s carries them and an idle packet of 10 octets (`07 FF C0 00 00 03` and four `00`), FHP 201. That makes 24 frames for 12 packets.

**VC 1, dump and payload packet 6.** The dump packets (12.0 to 12.5 s) and payload packet 6 (12.5 s) are one stream of 1 078 octets on VC 1; six frames carry it, and the multiplexer sends VC 0 first at the slot where both have a field:

| Frame | t (s) | VC count | FHP | Data field |
|---|---|---|---|---|
| 97 | 12.125 | 12 | 0 | dump 0 (140), first 71 octets of dump 1 |
| 99 | 12.375 | 13 | 69 | last 69 of dump 1, dump 2 (140), first 2 of dump 3 |
| 100 | 12.500 | 14 | 138 | last 138 of dump 3, first 73 of dump 4 |
| 101 | 12.625 | 15 | 33 | last 33 of dump 4, first 178 of payload 6 |
| 103 | 12.875 | 16 | `0x7FF` (no packet starts) | octets 178…388 of payload 6 |
| 110 | 13.750 | 17 | 23 | last 23 of payload 6, an idle packet of 188 octets |

Frame 98 (12.25 s) is VC 0 (the events of 12.0 s and HK 12) and frame 102 (12.75 s) is VC 0 (`DUMP_COMPLETED`). At slot 102 both VCs have a field: VC 0 goes first (LNK-031), the full VC 1 field waits in the multiplexer and goes at slot 103, and the 23 octets left over start field 2 at slot 102, so its deadline is 13.75 s (slot 110). The dump packets arrive complete at 12.135 s (dump 0), 12.385 s (dumps 1 and 2), 12.510 s (dump 3), and 12.635 s (dump 4), and the ground node reports `dump_reassembled` (65 536, 600, 32 936) at 12.635 s.

**OID frames.** Every other slot, 219 in all, is an OID frame on VC 7 with FHP `0x7FE`: frames 0 and 1, then every slot not listed above. Its data field is the next 211 octets of the OID generator, which is never restarted (CMN-021, LNK-036): the *m*-th OID frame (VC count *m*) carries octets 211*m* to 211*m* + 210 of the sequence, so the data field of frame 0 begins `FF FF FF FF 6D B6 D8 61 45 1F` (132.0 §4.1.4.6.2 note).

**OCF.** The word is `0x01000000` until the first uplink frame arrives and then changes in exactly these frames, at the first slot after an arrival (§4.4.1). The ground node reports each change as `clcw_changed` at the arrival instant (slot time + 10 000 µs):

| Frame | t (s) | FARM-B Counter | OCF word | Argument (decimal) |
|---|---|---|---|---|
| 9 | 1.125 | 1 | `0x01000200` | 16 777 728 |
| 17 | 2.125 | 2 | `0x01000400` | 16 778 240 |
| 19 | 2.375 | 3 | `0x01000600` | 16 778 752 |
| 21 | 2.625 | 0 | `0x01000000` | 16 777 216 |
| 25 | 3.125 | 1 | `0x01000200` | 16 777 728 |
| 33 | 4.125 | 2 | `0x01000400` | 16 778 240 |
| 41 | 5.125 | 3 | `0x01000600` | 16 778 752 |
| 49 | 6.125 | 0 | `0x01000000` | 16 777 216 |

**Uplink frames.** The ground node sends one TC frame at each of eight uplink slots (`up` index 0…7), each a Type-BD frame (`21 55`: version `00`, Bypass 1, Control Command 0, SCID `0x155`), VC 0, N(S) = 0, with the Frame Length field (the frame length minus one) in octets 2 and 3 and the Segment Header after the fifth octet (232.0 §4.1.2, §4.1.3.2.2). The table upload (309 octets) takes three slots without interleaving (LNK-041) and the two commands of 5.0 s share one FDU (232.0 §4.3.1.8; APP-042). Each frame arrives 10 000 µs after it is sent, is accepted by FARM-1, and counts once in the FARM-B Counter.

| Index | t (s) | Content | Segment Header (flags, MAP) | FDU / frame (octets) | First five octets and Segment Header |
|---|---|---|---|---|---|
| 0 | 1.0 | `NOOP`, seq 0 (14) | `11`, 0 | 15 / 22 | `21 55 00 15 00` `C0` |
| 1 | 2.0 | table upload, octets 0…119 of 309 | `01`, 1 | 121 / 128 | `21 55 00 7F 00` `41` |
| 2 | 2.25 | octets 120…239 | `00`, 1 | 121 / 128 | `21 55 00 7F 00` `01` |
| 3 | 2.5 | octets 240…308 | `10`, 1 | 70 / 77 | `21 55 00 4C 00` `81` |
| 4 | 3.0 | `ACTIVATE_TABLE`, seq 1 (17) | `11`, 0 | 18 / 25 | `21 55 00 18 00` `C0` |
| 5 | 4.0 | `DUMP_MEMORY`, seq 2 (20) | `11`, 0 | 21 / 28 | `21 55 00 1B 00` `C0` |
| 6 | 5.0 | `SET_PAYLOAD_MODE` (off), seq 3 (15), and `NOOP`, seq 4 (14) | `11`, 0 | 30 / 37 | `21 55 00 24 00` `C0` |
| 7 | 6.0 | `SET_PAYLOAD_MODE` (on), seq 5 (15) | `11`, 0 | 16 / 23 | `21 55 00 16 00` `C0` |

The flight node raises no frame-level event in `a2-nominal`. Its commands arrive 10 000 µs after their release, as at A1, so the packet projection and the instants of the packets the nodes output are those of `a1-nominal` (APP-003; D7 of the A2 plan).

#### 4.4.5 Blue Book readings the nodes rely on

Checked clause by clause against the Blue Books before any node code (A2 WP-1); the Blue Books settle each, and the three points they leave to the project are named.

- **Type-BD and N(S).** The Bypass Flag 1 and the Control Command Flag 0 make a Type-BD frame (232.0 §4.1.2.3.3, Table 4-1). 232.0 §4.1.2.8 note 3 says the COP does not use the Frame Sequence Number of a Type-B frame and sets it to zero; a note is not a requirement, and the decoder accepts any value, but the scenario sends zero (COP-010, TC-008), as the COP does.
- **FARM-B Counter.** It is bits 21 and 22 of the CLCW, one counter per VC, increments once for each Type-B frame accepted in Bypass mode (232.0 §4.2.1.9), and FARM-1 accepts every valid Type-BD frame in each of its three states (232.1 Table 6-1, E6). A frame that fails the validation checks (232.0 §4.4.9.3) is not accepted.
- **OCF in every frame.** If the OCF is present it occurs in every frame of its channel (132.0 §4.1.5.3), and an OID frame may carry valid data in it (132.0 §4.1.4.6.3 note 1), so the OID frames carry the CLCW (APP-031).
- **OID frames.** FHP `0x7FE`, the data field from the LFSR that is never restarted, a VCID that carries Packets (132.0 §4.1.4.6.1–3), and an OID frame may be sent in the middle of a split packet (note 3), which the VC 1 schedule of §4.4.4 relies on.
- **Idle Packets and the FHP.** The shortest Space Packet Idle Packet is 7 octets, and a remainder shorter than that spills into the next field (132.0 §4.2.2.5 and its note; no remainder below 7 arises in A2); an Idle Packet is a Packet (133.0 §4.1.2.2 note 2), so the FHP of a field that holds the residue of a packet and then an Idle Packet points at the Idle Packet (132.0 §4.1.2.7.6.2–3). The Idle Data pattern is the mission's (133.0 §4.1.4.3.4 note). The Blue Book does not say whether the Idle Packets form one counted sequence; the maintainer decided on 2026-11-08 that they keep the count 0 (OI-90).
- **Blocking and segmentation.** Several complete Packets may share one FDU under one Segment Header if they fit the maximum FDU (232.0 §4.3.1.8); the portions of a Packet go in consecutive frames of the MAP without interlacing (§4.3.1.6); the Sequence Flags are `01`, `00`, `10`, `11` (Table 4-2); the receiver rebuilds each from the flags and the length field of each Packet (§4.4.1.3).
- **Not in the Blue Books.** The instant at which the CLCW is sampled (§4.4.1), the release deadlines (§4.4.2), and the count of the Idle Packets (OI-90, above) are the scenario's rules (LNK-012 names the deadline a project parameter for TM).

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-134 | The OCF of every downlink frame, OID frames included, SHALL be the CLCW of FARM-1 as it is after the uplink frames delivered at or before the slot instant (§4.4.1), starting from the initial CLCW `01 00 00 00`. | 132.0 §4.1.5.3; 232.0 §4.2.1.9; 232.1 §6.2.4, §6.3.2.4 | T |
| APP-135 | The release deadline of a field SHALL run from the instant its first octet is written (§4.4.2); the flight node SHALL report the earliest deadline of its generators, its periodic applications, its command queue, and its dump as `next_deadline`, the ground node none, and the output of a node SHALL NOT change if it is stepped at other instants (APP-074). | LNK-012; API-053 | T |
| APP-136 | The link-engine parameters of §4.4.3 SHALL be in the scenario file and the generated constants, and the nodes of A2 SHALL take them from there. | — | A/I |
| APP-137 | The golden trace of `a2-nominal` SHALL have the downlink and uplink schedule of §4.4.4: the frame kinds, counts, First Header Pointers, fields, and OCF words, and the uplink frames and their Segment Headers. | 132.0 §4.1.2, §4.1.4.6, §4.2.2; 232.0 §4.1.2, §4.1.3.2, §4.3.1; LNK-011…LNK-014, LNK-030…LNK-033 | T |

### 4.5 COP-1 behaviour (A3 on)

What the nodes do at the link level `cop1` (D2 of the A3 plan), fixed before any node exists: the order of work with FOP-1 and its timer, the directives, the use of FARM-1, and the hand-derived schedule of `a3-nominal`. Below that level (`frames`, the `a2-*` cases) none of it applies: Type-BD frames and the nodes of §4.4 stay as they are (APP-005, APP-043). The fault cases are in §6.8 and the events in §7.4. Every Blue Book value is read from 232.1-B-2 with Cor. 1 and 232.0-B-4; the readings are in §4.5.4.

#### 4.5.1 Order of work

The ground node, in a step at the `cop1` level, does this in order:

1. For each TM frame delivered, in delivery order: the outputs of APP-138 up to and including `clcw_changed`; then the OCF is given to FOP-1 as `Clcw` (as `InvalidClcw` if `Clcw::decode` refuses the word), and the events of FOP-1's actions follow in the order of the actions (§7.4), the `fop1_state` of a change of state last; then the rest of APP-138 (the extractor's events and the packets').
2. `Timeout` is given to FOP-1, once, if its `next_deadline` is not after `now`; the node reports that deadline as its own (APP-135).
3. Each ops entry due, in script order: a `fop1_directive` entry gives its directive to FOP-1 (§4.5.2), a command or a table upload goes to the segmenter.
4. In a step with `tx_slot`: (a) if no `RequestAd` of the node is unanswered (the last one got its Accept or its Reject), the node asks the segmenter for one FDU and gives it to FOP-1 as `RequestAd`; (b) if FOP-1 holds a transmit request, the node outputs its frame as the last output of the step and then gives `TransmitDone` (accepted) for its kind.

The request identifier of the *n*-th FDU given to FOP-1 is *n* − 1 (from 0), whatever the answer. A **held** transmit request is one that FOP-1 returned and that has not yet left in a slot: FOP-1 sets the Out flag of its kind to Not_Ready (232.1 §5.1.6), so there is at most one of each kind. A Type-BC request goes before a Type-AD request, and the other stays held for the next slot (one frame leaves per slot, APP-071). A request that FOP-1 returns in steps 1 to 3 of a slot instant, or in 4(a), leaves in that slot, since 4(b) comes after them; a request that it returns at any other instant, or in answer to `TransmitDone`, leaves in the next slot, up to 250 000 µs later (232.1 §5.1.9.2 a). The Abort Request that begins a retransmission (232.1 §5.2.7 a) is not acted on: a held frame leaves in its slot and gets its `TransmitDone` (the link model has no Lower Procedure to cancel, and the engine changes no Out flag at an Abort).

The order of steps 2 and 3 matters only when a deadline and a directive fall at one instant, and no case has that; a deadline and an ops entry of a command do fall together in `a3-uplink-loss` (5.0 s) and the Timeout then goes first. A CLCW and a deadline never share an instant in the cases, since a CLCW is delivered 10 000 µs after a multiple of 125 000 µs.

The flight node's order at the `cop1` level is that of A2 (§2.1, APP-132, APP-139) with FARM-1 now also seeing the Type-AD and Type-BC frames; §4.5.3 gives what it does with them.

#### 4.5.2 The directives

The `directive` parameter of a `fop1_directive` entry (§6.5) takes the values of the table. The Directive Types and Qualifiers are those of 232.1 Table 4-1, which names the types and gives no number: the **code** is this scenario's, the row number of the table, and is what the events of §7.4 carry. The harness rejects a value it does not know, as it does for a `call` (§6.5); the rows marked "later" are not in scenario version 1 and a later version may add them (APP-002). The state column is Table 5-1.

| Code | Directive Type (Table 4-1) | Qualifier | `directive` value | Table 5-1: event, accepted in, goes to | Used by |
|---|---|---|---|---|---|
| 1 | Initiate AD Service (without CLCW check) | none | `"initiate_ad_without_clcw"` | E23: S6 only (Reject in S1–S5); Accept, Initialize, Confirm; S1 | all cases |
| 2 | Initiate AD Service (with CLCW check) | none | `"initiate_ad_with_clcw"` | E24: S6 only; Accept, Initialize, Start Timer; S4; Confirm on the first clean CLCW (E1 in S4, S1) | `a3-uplink-limit` |
| 3 | Initiate AD Service (with Unlock) | none | `"initiate_ad_with_unlock"` | E25: S6 only, `BC_Out_Flag` Ready (else E26, Reject); Accept, Initialize, Transmit Unlock Type-BC Frame; S5; Confirm on the first clean CLCW (E1 in S5, S1) | `a3-uplink-lockout` |
| 4 | Initiate AD Service (with Set V(R)) | V\*(R) | later | E27, E28 | none |
| 5 | Terminate AD Service | none | `"terminate_ad"` | E29: every state; S1–S5: Accept, Alert[term], Confirm; S6: Accept, Confirm (no Alert); S6 | `a3-uplink-lockout` |
| 6 | Resume AD Service | none | later | E30–E34 | none |
| 7 | Set V(S) to V\*(S) | V\*(S), 0 to 255 | `{"set_vs":n}` | E35: S6 only and Suspend_State 0 (else Reject); Accept, V(S) := V\*(S), NN(R) := V\*(S), Confirm; stays S6 | `a3-uplink-lockout`, `a3-uplink-limit` |
| 8 | Set FOP_Sliding_Window_Width (K) | K | later | E36 | none |
| 9 | Set T1_Initial | T1_Initial | later | E37 | none |
| 10 | Set Transmission_Limit | limit | later | E38 | none |
| 11 | Set Timeout_Type | TT | later | E39 | none |

The *Initiate AD Service* directives and *Set V(S)* are accepted in S6 only (232.1 §4.1 note 3 for Set V(S)), so a case that sets V(S) and starts the service again after the service ran uses *Terminate AD Service* first, or follows an Alert, which also ends in S6. After an Alert no further Alert occurs until FOP-1 receives one of the Initiate AD Service directives (232.1 §4.3.2), and the directive of the same case that follows *Set V(S)* is one of them.

The notifications of a directive are Accept, then Positive Confirm or Negative Confirm (232.1 §4.2, Table 4-2); a Reject ends it. Negative Confirm comes from the Alert action when an Initiate directive is still waiting for its clean CLCW (232.1 §5.2.15 d).

#### 4.5.3 FARM-1 on board

At the `cop1` level the flight node gives FARM-1 every TC frame of the uplink VC that passed the strict decode and the validation checks (TC-021, TC-023), of type AD, BD, and BC, and applies all its outputs:

- **Delivery buffer.** `buffer_available` is true while the node's delivery buffer, of depth 2, has room (§4.2, COP-033). The FDU of an accepted Type-AD or Type-BD frame goes to the `MapReassembler` in the same step, so the buffer is free again at the next frame and no case reaches Wait (E2, E10: the cells have the M3 vectors, COP-005). FARM-1 delivers the data field after the Segment Header, and the reassembler takes the Segment Header octet in front of it (D10 of the A3 plan, #394).
- **Discards.** A frame that FARM-1 discards (E1 in Lockout, E2 to E5) raises `frame_discarded` with layer 2 and the number of the event (APP-139, §7.4); a frame the strict decode rejects is `frame_rejected` and FARM-1 is given the invalid-frame input (E9), also for a Type-BC frame whose data field is neither Unlock nor Set V(R) (strict decode: `ReservedValue`, 7, TC-005, COP-031; no case sends one).
- **State.** A change of FARM-1's state raises `farm1_state` (§7.4) after the other outputs of the cell, which is when 232.1 §6.3.1 updates the state.
- **Unlock.** An accepted *Unlock* (E7) resets only FARM-1 (232.1 §6.3.2.2 note, COP-036); the node raises `farm1_unlock`, then `farm1_state` if the state changed, and resets its `MapReassembler` and its delivery buffer, so that a half-received packet is not completed by a segment sent after the Unlock.
- **CLCW.** The OCF of every downlink frame is `Farm1::clcw` at the slot instant after the deliveries (§4.4.1, APP-134), now with N(R) = V(R), the Lockout and Retransmit flags, and the FARM-B Counter of the Type-BD and Type-BC frames accepted (232.1 §6.1.6, Table 6-1 E6 to E8). The word is `0x01000000` + `0x2000` × Lockout + `0x1000` × Wait + `0x0800` × Retransmit + `0x0200` × (FARM-B Counter mod 4) + N(R): Lockout, Wait, Retransmit, the counter, and the Report Value are bits 18, 19, 20, 21–22, and 24–31 of the CLCW, with bit 0 the most significant (232.0 §4.2.1.8.4 to §4.2.1.11), and the COP in Effect `01` in bits 6–7 gives `0x01000000` (§4.4.1). In the scenario Wait is always 0.

FARM-1's windows are PW = NW = 4 (W = 8, §4.2): with V(R) = *r*, N(S) = *r* is accepted (E1), *r* + 1 to *r* + 3 is discarded and sets Retransmit (E3), *r* − 4 to *r* − 1 is discarded (E4), and N(S) from *r* + 4 to *r* − 5 (mod 256) is discarded and puts FARM-1 into Lockout (E5; 232.1 §6.1.8.3.1).

#### 4.5.4 Blue Book readings the A3 nodes rely on

Checked clause by clause against 232.1-B-2 (with Cor. 1) and 232.0-B-4 before any node code (A3 WP-1). The Blue Books settle each; where the repository's rule goes beyond them, it says so.

- **Directives (232.1 §4.1, Table 4-1, Table 5-1).** Table 4-1 lists eleven Directive Types (four Initiate, one Terminate, one Resume, five Set); the sentence before it says "twelve" and the four categories above it add to eleven, so the table is what the scenario reads. Which state accepts which directive is Table 5-1 E23 to E39, as the table of §4.5.2 gives it.
- **Order of the cell's actions and the Alert (232.1 §5.2.15, §4.3.2, Table 4-4).** An Alert cancels the timer, purges the Sent_Queue (a Negative Confirm for each frame, in queue order), purges the Wait_Queue (a Reject), gives a Negative Confirm to a waiting Initiate directive, and then notifies; the state is S6. The reason codes are Limit, T1, Lockout, Synch, NN(R), CLCW, LLIF, and Term; the scenario numbers them 1 to 8 in that order (§7.4), which the Blue Book does not. In S6 every CLCW event is Ignore (Table 5-1 E1 to E15), so after an Alert the CLCWs change nothing until an Initiate directive.
- **Transfer notifications (232.1 §4.4.2, Table 4-6).** Accept, then Positive Confirm when a CLCW acknowledges the FDU or Negative Confirm when the Sent_Queue is purged; a request refused (in S4, S5, S6, or with a request already waiting, E19, E20) is Rejected and no other notification follows. An FDU is acknowledged by a CLCW whose N(R) passes its N(S) (§5.2.8).
- **Timer and transmission count (232.1 §5.1.9.1, §5.1.10, §5.2.4 d, §5.2.7 c, §5.2.8, Table 5-1 E2, E6, E10, E16, E17).** The timer is started, or restarted, by each *new* Type-AD or Type-BC frame FOP-1 transmits and by each Initiate AD (or BC) Retransmission, not by each frame of a retransmitted series (§5.2.10 b): it is the single T1 of the Sent_Queue, which runs from the last such event. It is cancelled when a CLCW acknowledges every frame (E2) and left running when it acknowledges some (E6). Transmission_Count is 1 for the first transmission, is incremented by each retransmission initiated (E8, E10, E16), and reset to 1 when frames are acknowledged; Transmission_Limit = 3 therefore allows the first transmission and two retransmissions (§5.1.10.2), and the third expiry with the count at the limit raises Alert[T1] (E17) under Timeout_Type 0.
- **CLCW classification (232.1 §5.3.2.1).** The table checks the Lockout Flag, then N(R), then the Retransmit and Wait Flags. N(R) is valid when NN(R) ≤ N(R) ≤ V(S) (mod 256); a CLCW with an invalid N(R) in S1 to S4 is E13, Alert[NN(R)]. This is why a Set V(S) that puts V(S) away from FARM-1's V(R) cannot make a lockout alone (§6.8, `a3-uplink-lockout`): the first CLCW after the frame left shows the old N(R) and FOP-1 alerts on N(R) before FARM-1's Lockout reaches it.
- **FARM-1 (232.1 §6.1.8.3.1, §6.3.2.2, Table 6-1; 232.0 §4.2.1).** The windows, E1 to E8, and the CLCW fields are as §4.5.3 gives them. A Type-BC *Unlock* in any state sets Retransmit, Wait, and Lockout to 0, counts in the FARM-B Counter, and goes to S1 (E7); it does not change V(R).
- **Frames (232.0 §4.1.2.3.3 Table 4-1, §4.1.2.8 note 3, §4.1.3.1.3, §4.1.3.3).** Type-AD has Bypass 0 and Control Command 0, Type-BD 1 and 0, Type-BC 1 and 1, so the first octet of a frame of the scenario's spacecraft ID is `01` (AD), `21` (BD), or `31` (BC). The Type-C data field holds the Control Command alone, with no Segment Header: Unlock is the octet `00`, and the frame has 5 + 1 + 2 = 8 octets. N(S) of a Type-BC frame is 0 as that of a Type-BD frame is (note 3; COP-010).
- **Not in the Blue Books:** the instant of the CLCW sample (§4.4.1), when a held frame leaves and when `Timeout` is given (§4.5.1), the codes of §4.5.2 and §7.4, and what the node does with the Abort Request (§4.5.1). They are this scenario's rules.

**The timer against T1_Initial = 1 000 000 µs.** A frame leaves at a slot *u*, arrives at *u* + 10 000 µs, is first reported in the downlink frame of *u* + 125 000 µs, and reaches FOP-1 at *u* + 135 000 µs: every acknowledgement of §4.5.5 and §6.8 comes 135 000 µs after the frame, which is 865 000 µs inside T1_Initial and 260 000 µs inside the 395 000 µs of §4.2. A retransmission series of *n* frames that FOP-1 starts at a CLCW at *t* leaves in the slots after *t* (the first at most 250 000 µs later, then one every 250 000 µs), and its last frame is acknowledged at most 250 000 + 250 000 (*n* − 1) + 135 000 µs after *t*, while the timer started at *t* expires 1 000 000 µs after it: 635 000 µs for *n* = 2 and 885 000 µs for *n* = 3 are inside T1_Initial; four frames (the whole window K = 4) can reach 1 135 000 µs, which is not. No case of A3 queues more than two frames behind a loss, and the schedules below give the margin of each.

#### 4.5.5 Expected schedule of `a3-nominal`

Derived by hand from §4.2, §2.4, §4.4.1, §4.4.4, and the rules of §4.5.1 to §4.5.4, before any node exists (an independent regeneration is D8(c) of the A3 plan); `a3-nominal` reproduces it exactly (APP-150). The downlink is that of `a2-nominal` (§4.4.4) except for the OCF words below, the packet projection is that of `a1-nominal` (APP-003), and the uplink frames leave at the instants of §4.4.4, since every FDU is requested at its slot, accepted at once, and sent in it. The window never fills: at most one frame is unacknowledged at any instant, and the timer of each frame is cancelled 135 000 µs after it starts.

**Uplink frames** (`up` index 0 to 7). Each is a Type-AD frame (first octets `01 55`: version `00`, Bypass 0, Control Command 0, SCID `0x155`), VC 0, with N(S) = V(S) of FOP-1, and has the length and the Segment Header of the frame of the same index in §4.4.4; only the first octet and N(S) differ. FARM-1 accepts every frame with E1.

| Index | t (s) | N(S) | FDU | First five octets and Segment Header |
|---|---|---|---|---|
| 0 | 1.0 | 0 | `NOOP`, seq 0 | `01 55 00 15 00` `C0` |
| 1 | 2.0 | 1 | table upload, octets 0…119 | `01 55 00 7F 01` `41` |
| 2 | 2.25 | 2 | octets 120…239 | `01 55 00 7F 02` `01` |
| 3 | 2.5 | 3 | octets 240…308 | `01 55 00 4C 03` `81` |
| 4 | 3.0 | 4 | `ACTIVATE_TABLE`, seq 1 | `01 55 00 18 04` `C0` |
| 5 | 4.0 | 5 | `DUMP_MEMORY`, seq 2 | `01 55 00 1B 05` `C0` |
| 6 | 5.0 | 6 | `SET_PAYLOAD_MODE` (off), seq 3, and `NOOP`, seq 4 | `01 55 00 24 06` `C0` |
| 7 | 6.0 | 7 | `SET_PAYLOAD_MODE` (on), seq 5 | `01 55 00 16 07` `C0` |

**CLCW.** The OCF is `0x01000000` until the frame of index 0 arrives. FARM-B Counter 0 and the flags 0 throughout; N(R) = V(R) = the number of frames accepted. The word changes in exactly these downlink frames, the first slot after each arrival, and the ground node reports each change as `clcw_changed` at the arrival instant of the downlink frame; FOP-1 receives each of these CLCWs and acknowledges the frame (E2: every outstanding frame acknowledged, the timer cancelled) at the same instant. The CLCWs of the slots between them repeat the last word and give FOP-1 E1 (nothing outstanding) or E5 (a frame outstanding, not yet reported), both Ignore.

| Downlink frame | t (s) | N(R) | OCF word | `clcw_changed` (decimal) | At | Acknowledges | Margin to the timer |
|---|---|---|---|---|---|---|---|
| 9 | 1.125 | 1 | `0x01000001` | 16 777 217 | 1.135 | FDU 0 (index 0) | 865 000 µs |
| 17 | 2.125 | 2 | `0x01000002` | 16 777 218 | 2.135 | FDU 1 (index 1) | 865 000 µs |
| 19 | 2.375 | 3 | `0x01000003` | 16 777 219 | 2.385 | FDU 2 (index 2) | 865 000 µs |
| 21 | 2.625 | 4 | `0x01000004` | 16 777 220 | 2.635 | FDU 3 (index 3) | 865 000 µs |
| 25 | 3.125 | 5 | `0x01000005` | 16 777 221 | 3.135 | FDU 4 (index 4) | 865 000 µs |
| 33 | 4.125 | 6 | `0x01000006` | 16 777 222 | 4.135 | FDU 5 (index 5) | 865 000 µs |
| 41 | 5.125 | 7 | `0x01000007` | 16 777 223 | 5.135 | FDU 6 (index 6) | 865 000 µs |
| 49 | 6.125 | 8 | `0x01000008` | 16 777 224 | 6.135 | FDU 7 (index 7) | 865 000 µs |

**Application events of the ground node** (the flight node raises none: no discard, no change of state, no Unlock). At 0.0 s, in this order: `fop1_directive` (1, 1, 0) (Accept) and `fop1_directive` (3, 1, 0) (Positive Confirm) of *Initiate AD Service (without CLCW check)*, which leaves S6 for S1 in one cell (E23), then `fop1_state` (1). At each instant of the table above, after the frame's `clcw_changed`: `fop1_transfer` (3, *n*) for the FDU *n* in the row (Positive Confirm). FOP-1 sends no retransmission, raises no Alert, and the timer is never within 865 000 µs of expiring, so no `Timeout` is given.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-144 | At the `cop1` level the ground node SHALL do in each step, in this order: for each TM frame delivered, the outputs of APP-138 up to `clcw_changed`, the OCF given to FOP-1 as Clcw (InvalidClcw if it is not a CLCW), FOP-1's events, and the remaining outputs of APP-138; Timeout to FOP-1, once, if its `next_deadline` is not after `now`; each ops entry due, in script order (a directive to FOP-1, a command or table upload to the segmenter); and, in a step with `tx_slot`, a RequestAd of one FDU if none is unanswered, then the frame of the held transmit request (Type-BC before Type-AD) as the last output of the step, followed by TransmitDone for its kind. A transmit request that FOP-1 returns at another time SHALL be held for the next slot, and the Abort Request SHALL NOT be acted on (§4.5.1). | 232.1 §3.2.3, §5.1.6, §5.1.9.1, §5.2.4 to §5.2.10, §5.3.2.1, Table 5-1 E41 to E46; COP-006, COP-013 | T |
| APP-145 | The ground node SHALL give FOP-1 an FDU only with RequestAd, and only when no earlier RequestAd is unanswered (232.1 §4.4.2); it SHALL number the FDUs from 0 in the order requested; and after an Alert it SHALL do nothing of its own: it SHALL NOT resubmit a negatively confirmed or rejected FDU, and it SHALL report a Reject Response to a request as `fop1_transfer` (§7.4), not as `uplink_refused`, which stays the report of an input that FOP-1 returns an error for (APP-143). Recovery is the directives of the ops script. | 232.1 §4.3.2, §4.4.2; COP-004 | T |
| APP-146 | At the `cop1` level the flight node SHALL give FARM-1 every TC frame of the uplink VC that passed the strict decode and the validation checks, of type AD, BD, and BC, with `buffer_available` from its delivery buffer of depth 2, and SHALL output, in this order and in addition to APP-139: for a frame that FARM-1 discards, `frame_discarded` (2, event number), and `farm1_state` if the state changed; for an accepted Unlock, `farm1_unlock`, then `farm1_state` if the state changed, and the node SHALL reset its MapReassembler and delivery buffer; for an accepted Type-AD frame, the reassembly and packet outputs of APP-139 in the same step, with the Segment Header octet put in front of the data FARM-1 delivers (D10 of the A3 plan). A Type-BC frame whose data field is neither Unlock nor Set V(R) SHALL be `frame_rejected` (7) with FARM-1 given the invalid-frame input (E9). | 232.1 §3.3.2, §6.1.8.3.1, §6.3.2.1, §6.3.2.2, §6.3.2.3, Table 6-1; 232.0 §4.1.3.3; COP-030, COP-031, COP-033, COP-036; TC-005, TC-021, TC-023 | T |
| APP-147 | The OCF of every downlink frame at the `cop1` level SHALL be the word `0x01000000` + `0x2000` × Lockout + `0x1000` × Wait + `0x0800` × Retransmit + `0x0200` × (FARM-B Counter mod 4) + N(R), of FARM-1's state after the uplink frames delivered at or before the slot instant (APP-134), with N(R) = V(R). | 232.0 §4.2.1.5 to §4.2.1.11; 232.1 §6.1.3 to §6.1.7, §6.2.4, Table 6-1; COP-034, COP-035 | T |
| APP-148 | The `directive` parameter of a `fop1_directive` entry SHALL take the values of §4.5.2 (`"initiate_ad_without_clcw"`, `"initiate_ad_with_clcw"`, `"initiate_ad_with_unlock"`, `"terminate_ad"`, and `{"set_vs":n}` with n from 0 to 255), the ground node SHALL give each to FOP-1 as its Directive Type and Qualifier of 232.1 Table 4-1, and the harness SHALL reject any other value when it parses the file. | 232.1 §4.1, Table 4-1, Table 5-1 E23 to E40 | T |
| APP-149 | The COP-1 events SHALL be those of §7.4 (200 to 203 of the ground node, 210 and 211 of the flight node), raised on the notifications, Alerts, and changes of state of the engines, in the order of the engine's actions and the positions that APP-144 and APP-146 give them. | 232.1 §4.2 to §4.4, Tables 4-2, 4-4, 4-6, §6.3.1; COP-004; API-080 | T |
| APP-150 | The golden trace of `a3-nominal` SHALL have the uplink schedule, the CLCW words, the acknowledgements, and the events of §4.5.5, and the packet projection of `a1-nominal` (APP-003). | 232.1 §5.1.9.1, Table 5-1 E2, E5; 232.0 §4.1.2.3.3, §4.2.1; COP-010, COP-011 | T |


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
| APP-062 | The ground node SHALL take the delivery instant as its time and SHALL NOT correct on-board time (no correlation, offset, or drift in scenario version 1). | — | I |

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
| 2 | Step the ground node at t = 0 (the start of the run), and whenever it has deliveries, ops-script entries, an uplink slot, or a deadline at or before t; record its outputs in output order; send its unit, if any |
| 3 | Step the flight node likewise, with the downlink slot |

Step instants are not recorded: an implementation that asks for extra wake-ups produces the same trace, so a node SHALL behave the same whether or not it is stepped at an instant where nothing is due. Within a step the nodes work in the order of §2.1 and §2.2.

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
| `a0-empty` | A1 rules (packets) | none | Harness self-test: no applications, end instant 0, header-only trace |
| `a1-nominal` | A1 | none | The timeline of §2.4 (end 35 s) |
| `a1-immediate` | A1 | none | `a1-nominal` plus immediate commands (outside APP-003); §6.6 |
| `a1-rejects` | A1 | none | Every rejection reason of §3.3.2, both `TABLE_REJECTED` reasons, and immediate execution (APP-013, APP-017, APP-024); §6.6 |
| `a1-downlink-faults` | A1 | `drop`, `duplicate`, `delay`, `truncate` of downlink packets | The ground node's sequence, dump, and rejected-packet events (APP-018) before frames hide them; §6.6 |
| `a2-nominal` | A2 | none | The timeline of §2.4 over TM/TC frames (end 35 s); the schedule of §4.4.4 |
| `a2-downlink-loss` | A2 | `drop` and `duplicate` of downlink frames | Frame-gap and discontinuity detection, extractor resynchronisation, packet-sequence and dump events (LNK-051, LNK-061, SPP-011); §6.7 |
| `a2-downlink-corrupt` | A2 | `flip`, `truncate`, and `delay` of downlink frames | The strict codec error path, the `CallerDiscard` path, and frame reordering (LNK-051, LNK-080); §6.7 |
| `a2-uplink-faults` | A2 | `drop`, `flip`, and `duplicate` of uplink frames | The TC receive and reassembly error paths and the lack of recovery on the Bypass service (APP-043); §6.7 |
| `a3-nominal` | A3 (`cop1`) | none | The timeline of §2.4 over COP-1 (end 35 s); the schedule of §4.5.5; frozen from A3 (APP-004) |
| `a3-uplink-loss` | A3 | `drop` of uplink and of downlink frames | The headline case: a lost segment with frames behind it (E3, Retransmit flag, retransmission), a lone frame lost (only T1 retransmits), and lost acknowledgements (T1 expiry, E4); §6.8 |
| `a3-uplink-disorder` | A3 | `duplicate`, `delay`, and `flip` of uplink frames | Reordering through the FARM-1 window (E3, E4) and the FECF path (E9) under COP-1; §6.8 |
| `a3-uplink-lockout` | A3 | `drop` of one downlink frame; directives *Terminate AD Service*, *Set V(S)*, *Initiate AD Service* | A lockout (E5), Alert[Lockout], and its recovery with Unlock; §6.8 |
| `a3-uplink-limit` | A3 | `drop` of one frame and its two retransmissions; directives *Set V(S)*, *Initiate AD Service (with CLCW check)* | Transmission_Limit reached, Alert[T1], the negative confirms, and a recovery; §6.8 |
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

### 6.5 Script entries in the scenario file

A case's **ops script** (`ops`) and **fault script** (`faults`) are arrays in the scenario file; both are optional and empty by default. The JSON Schema generated from the harness's scenario model (`refapp/scenario/scenario.schema.json`) describes the field names and ranges, and the harness enforces the ranges of the script entries, with the rules below, when it parses the file; this section says what the entries mean. An entry holds **parameters, not octets**: the ground node builds the packet from them (§3.3.5, §3.3.6), so the A5 C++ node reads the same file and the script cannot disagree with the packet layouts.

| Ops entry `kind` | Parameters | Meaning |
|---|---|---|
| `command` | `at_us`; `execution`: `{"mode":"immediate"}` or `{"mode":"time_tagged","exec_us":…}`; `call`: `function` (`noop`, `dump_memory`, `activate_table`, `set_payload_mode`, `unknown`) with its arguments of §3.3.5 (`address` and `length`; `table_id` and `crc`; `mode`; `code`); optional `extra_octets` | A command packet on APID `0x080`, output by the ground node at `at_us` (§6.2). `exec_us` is the execution time as µs after T0. `unknown` carries a function code that no function has. `extra_octets` appends that many `0x00` octets after the arguments, so that the packet length does not match the function code (APP-024) |
| `table_upload` | `at_us`, `table_id`, `length` | A table-upload packet on APID `0x081` with the table octets of §3.3.6 |
| `fop1_directive` | `at_us`, `directive`: `"initiate_ad_without_clcw"`, `"initiate_ad_with_clcw"`, `"initiate_ad_with_unlock"`, `"terminate_ad"`, or `{"set_vs":n}` (n from 0 to 255) | A FOP-1 directive (§2.4, §4.5.2): the Directive Type and Qualifier of 232.1 Table 4-1. Reserved from A1: issued from A3 on and ignored below that link level (APP-019). The harness rejects a value it does not know (APP-148) |

| Fault entry | Parameters | Meaning |
|---|---|---|
| (one entry per fault) | `dir` (`down` or `up`), `index`, `fault`: `{"kind":"drop"}`, `{"kind":"duplicate"}`, `{"kind":"delay","delay_us":d}`, `{"kind":"flip","octet":n,"bit":b}`, or `{"kind":"truncate","octets":n}` | The fault of §6.3 on the unit with that index among those sent in that direction. `flip` is valid at frame levels only |

Rules:
- An entry's index is its position in the array. It is the ops-script entry index of the node interface (§8.1, §8.2).
- Ops entries are in release order, so release instants never decrease, and entries with one instant are released in array order. Command sequence counts (APID `0x080`) follow the release order of the `command` entries; table uploads have their own count (APID `0x081`, §3.1).
- A fault script addresses units by their index in the direction as sent, before any fault; the index of a downlink unit therefore follows from the per-instant order of §2.1.
- Every entry is released before the case's end instant. Every `exec_us` is a multiple of the frame slot (APP-061). The harness rejects a file that breaks these rules.

### 6.6 Cases of A1

`a1-nominal` has the ops script of §2.4 (its entries marked A1, in release order) and no faults. The three cases below are added by APP-002: each is a new case, and none changes another. Each case's ops script and fault script are written out in `refapp/scenario/scenario.json` together with its golden trace, and a harness test checks them of all four cases (`a1-nominal` too) against the Rust model of the scenario file and the rules of §6.5; `refapp/refapp-harness/tests/a1_golden.rs` checks the reports and ground events of §2.4 and this section on the golden traces.

Every command of these cases that has an effect at its delivery instant (§2.3: immediate execution, rejection at delivery) is released at an instant that is 115 000 µs after a multiple of 125 000 µs, so that it arrives on the 125 000 µs grid and every on-board time stamp stays exact (APP-061, APP-128). Commands that only queue need no such care. Events listed below are the event packets of §3.3.2 on APID `0x011` unless called an application event; the flight node also raises `cmd_received` for each command packet delivered and `table_received` for each table stored (APP-129), which the tables leave out.

**`a1-immediate`** (end 35 s): the ops script of `a1-nominal`, then two immediate commands. Everything else is as in `a1-nominal`, and the packet projection differs from it (not part of APP-003).

| Release (s) | Entry | Seq | Report |
|---|---|---|---|
| 14.115 | `NOOP`, immediate | 6 | `CMD_EXECUTED` (1, 6) at 14.125 s |
| 21.115 | `SET_PAYLOAD_MODE` (on), immediate | 7 | `CMD_EXECUTED` (4, 7), `PAYLOAD_MODE` (1) at 21.125 s; the payload packet at 22.5 s is generated, which the nominal timeline does not do |

**`a1-rejects`** (end 21 s): one command for each rejection reason of §3.3.2 and both `TABLE_REJECTED` reasons of APP-017. Seqs follow release order. Each report carries the arguments of §3.3.2, written (p1, p2, p3).

| Release (s) | Entry | Seq | Report |
|---|---|---|---|
| 0.115 | `NOOP`, immediate | 0 | `CMD_EXECUTED` (1, 0, 0) at 0.125 s |
| 0.615 | Function code 127 (`unknown`), immediate | 1 | `CMD_REJECTED` (127, 1, 1), unknown function, at 0.625 s |
| 1.115 | `NOOP` with one extra octet, time tag 3.0 s | 2 | `CMD_REJECTED` (1, 2, 3), malformed arguments, at 3.0 s |
| 3.615 | `NOOP`, time tag 3.0 s (before the arrival at 3.625 s) | 3 | `CMD_REJECTED` (1, 3, 2), time tag in the past, at 3.625 s |
| 4.115 | `ACTIVATE_TABLE` (table 1, CRC `0x0C4F`), immediate, no table yet | 4 | `CMD_EXECUTED` (3, 4, 0), `TABLE_REJECTED` (1, 1, 0): no table, at 4.125 s |
| 5.115 | Table upload, table 1, 300 octets (APID `0x081`, seq 0) | — | Stored at 5.125 s; no report |
| 6.115 | `ACTIVATE_TABLE` (table 1, CRC `0x0C4E`), immediate | 5 | `CMD_EXECUTED` (3, 5, 0), `TABLE_REJECTED` (1, 2, 0): CRC mismatch, at 6.125 s (`0x0C4F` is the CRC of the table, §2.4) |
| 8.115 | `DUMP_MEMORY` (address `0x00010000`, 300 octets), immediate | 6 | `CMD_EXECUTED` (2, 6, 0) at 8.125 s; three dump packets (128, 128, 50 SDU octets) at 8.125, 8.250, and 8.375 s; `DUMP_COMPLETED` (1, 0, 300) at 8.375 s |
| 8.240 | `DUMP_MEMORY`, same arguments, immediate, while the dump runs | 7 | `CMD_REJECTED` (2, 7, 5), dump in progress, at 8.250 s, before that step's dump packet (§2.1) |
| 10.115 | Nine `NOOP`, time tag 20.0 s, released at one instant | 8…16 | Seqs 8…15 queue (the queue holds 8, §2.3); seq 16 gives `CMD_REJECTED` (1, 16, 4), queue full, at 10.125 s; at 20.0 s `CMD_EXECUTED` (1, n, 0) for n = 8…15 in queue order |

The ground node raises one application event: `dump_reassembled` with the arguments 65 536, 300, 32 736 at 8.385 s (the CRC-16 of the 300 memory octets from `0x00010000`, computed as in §2.4). It raises no sequence or rejected-packet event.

**`a1-downlink-faults`** (end 35 s): the ops script of `a1-nominal` and five scripted faults on downlink packets, `drop`, `duplicate`, `delay`, and `truncate` (no `flip`: there is no FECF at packet level). The flight node's output is that of `a1-nominal`; the faults change only what the ground node receives. Packets are given by their downlink unit index and by their APID and Packet Sequence Count. Ground events are (APID, …) as in §7.4, and times are arrival instants (send instant plus 10 000 µs, plus the delay).

| Unit | Packet | Fault | Ground node |
|---|---|---|---|
| 2 | HK `0x010`, count 1 (t = 1 s) | `drop` | At 2.010 s count 2 arrives after count 0: `seq_gap` (16, 1) |
| 4 | Payload `0x020`, count 1 (t = 2.5 s) | `duplicate` | `packet_in` twice at 2.510 s; the second raises `seq_repeat` (32, 1) |
| 5 | HK `0x010`, count 3 (t = 3 s) | `delay` 1 500 000 µs, so it arrives at 4.510 s | Count 4 arrives first at 4.010 s: `seq_gap` (16, 1). Count 3 arrives at 4.510 s: `seq_reset` (16, 3). Count 5 arrives at 5.010 s after count 3, the tracker's last count: `seq_gap` (16, 1) |
| 11 | Payload `0x020`, count 3 (t = 6.5 s) | `truncate` to 20 octets | At 6.510 s the strict decoder rejects the unit: `packet_rejected` (2), `Incomplete` (03 §9), and no `packet_in`. Count 4 arrives at 8.510 s after count 2: `seq_gap` (32, 1) |
| 25 | Dump `0x021`, count 2 (the third packet, t = 12.25 s) | `drop` | Count 3 arrives at 12.385 s after count 1: `seq_gap` (33, 1), then `dump_discarded` (33, 256), the 256 SDU octets of the two packets held. Count 3 (`00`) and count 4 (`10`) are dropped as orphans without an event, and no `dump_reassembled` follows |

The classification is Aldis's own (SPP-011, CMN-030, CMN-031): a forward distance of 1 is in sequence, 0 a repeat, 2 to 8 192 a gap with distance minus 1 missing, anything else a reset, and the tracker takes every received count as the new last count. The reassembler's reasons are those of `docs/standards/space-packet.md` §5.4.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-126 | The ops script and the fault script of a case SHALL be the optional arrays `ops` and `faults` of the scenario file, with the entries of §6.5: parameters, not octets, in release order, each released before the case's end instant, and each execution time a multiple of the frame slot. The harness SHALL reject a file that breaks these rules, and the schema SHALL describe the entries. | — | T |
| APP-127 | The cases `a1-immediate`, `a1-rejects`, and `a1-downlink-faults` SHALL be those of §6.6, each in the scenario file with its golden trace, and their reports and ground events SHALL be those the tables of §6.6 give. | — | T |
| APP-128 | In a packet-level case (A1), an immediate command SHALL be released at an instant that, with the one-way delay, is a multiple of the frame slot; the harness SHALL reject a file in which one is not. The same SHALL hold for a time-tagged command that is rejected at delivery (§2.3: a time tag before the arrival, or a full queue), which the harness finds by following the queue of the script. The rule does not apply to the frame-level cases (A2 on): a frame arrives 10 000 µs after a multiple of the uplink slot, off the 125 000 µs grid (D4 of the A2 plan); APP-141 holds there. | — | T |
| APP-129 | For every command packet delivered the flight node SHALL raise `cmd_received` (function code, command sequence count) before any other event of that packet, and for every table stored `table_received`. A command whose function code is unknown or whose length does not match its function SHALL be rejected with reason 1 or 3 at its execution instant; an `ACTIVATE_TABLE` that finds no table, another table ID, or a CRC mismatch is an executed command and raises `CMD_EXECUTED` before its `TABLE_REJECTED`. | — | T |
| APP-130 | For every link unit delivered the ground node SHALL output, in this order: `packet_rejected` if the strict decoder fails, and nothing further (a unit with octets after its one packet fails with `LengthMismatch`, the exact decode of API-019, OI-89); otherwise `packet_in`, then the sequence event of the packet (none for the first packet of an APID or an in-sequence one), then for the dump APID `dump_discarded` if a partial SDU was thrown away (arguments: APID, SDU octets held) and `dump_reassembled` if the SDU is complete. A packet that the reassembler drops without discarding an SDU (an orphan, or a segment too long for the buffer) raises no event. A reassembled SDU that does not parse as §3.3.4 raises `packet_rejected` after the events above, since the failure is found only then, with `Incomplete` for an SDU shorter than its 6-octet header, `OutOfRange` for a length field of 0 or above 1 024, and `LengthMismatch` when the memory octets are not as many as the length field says (OI-88). | 133.0 §4.1.3.4.3.3–4, §4.3.2.2 | T |
| APP-131 | When the flight node cannot build a downlink packet (the time conversion or the packet writer fails, so that no packet is output and the count of its APID does not advance), it SHALL count the failure and raise the application event `packet_rejected` (§7.4) with the error code of the failure, in the step in which it happened; a dump whose packet cannot be built is abandoned, without `DUMP_COMPLETED`. For an uplink unit that is not one unsegmented telecommand packet of the commands or table-upload APID, or a table upload that the table store refuses (APP-017), the flight node SHALL raise the same event in the step that delivers it, with no `cmd_received`, `table_received`, or verification report. From A2 on the unit is a TC frame: one that fails the strict decode or the validation raises `frame_rejected` instead (APP-139), and the packet rule applies to each packet that the MAP reassembler delivers, and to an FDU it refuses, in the step that delivers the frame. | — | T |

### 6.7 Cases of A2

The four cases are added by APP-002. Each has the end instant 35 s and the ops script of `a1-nominal` (the entries of §2.4 marked A1; those marked A3 are ignored, APP-019), so each uplink run has the eight frames of §4.4.4, and none uses an immediate command (D4 of the A2 plan). Faults address units by index as sent (§6.5): a downlink index is the slot number, because the flight node sends one frame at every slot, and an uplink index is that of the table in §4.4.4. The frames are named by index, the arrival instants are the send instants plus 10 000 µs, and the events are (`args`) as in §7.4, in the order of APP-138 and APP-139. `clcw_changed` events that §4.4.4 lists and that a fault does not change are left out. The packet-level consequences follow the classification of §6.6 (SPP-011) and the reassembler's reasons of `docs/standards/space-packet.md` §5.4. The fault scripts are fixed here (APP-005); A3 adds its own `a3-*` cases.

**`a2-nominal`** has no faults. The ground node raises the eight `clcw_changed` events of §4.4.4 and `dump_reassembled` (65 536, 600, 32 936) at 12.635 s; the flight node raises no frame-level event.

**`a2-downlink-loss`**: `drop` of three frames and `duplicate` of one, chosen so that the lost frames start a packet, continue one, and carry dump segments.

| Index | Frame | Fault |
|---|---|---|
| 18 | VC 0, HK 2 (2.25 s) | `duplicate` |
| 36 | VC 1, first field of payload 2 (4.5 s) | `drop` |
| 92 | VC 1, second field of payload 5 (11.5 s) | `drop` |
| 99 | VC 1, tail of dump 1, dump 2, head of dump 3 (12.375 s) | `drop` |

The ground node raises, besides the `clcw_changed` events and the `packet_in` records of the packets that arrive:

| Arrival (s) | Frame | Events |
|---|---|---|
| 2.260 | 18, copy 1 | `frame_discontinuity` (0, 0), `frame_discontinuity` (1, 0), then the HK packet again and `seq_repeat` (16, 2) |
| 4.635 | 37 | `frame_gap` (0, 0, 1) |
| 5.510 | 44 | `frame_gap` (1, 1, 1); the field is the tail of a packet the extractor never saw and gives no event; payload 2 is lost |
| 7.510 | 60 | payload 3 completes: `seq_gap` (32, 1) |
| 11.635 | 93 | `frame_gap` (0, 0, 1) |
| 12.135 | 97 | `frame_gap` (1, 1, 1), `partial_discarded` (1, 211) (the first field of payload 5, lost for good), then dump 0 (no sequence event; the reassembler holds 128 octets) |
| 12.510 | 100 | `frame_gap` (0, 0, 1), `frame_gap` (1, 1, 1), `partial_discarded` (1, 71) (the head of dump 1) |
| 12.635 | 101 | dump 4 completes: `seq_gap` (33, 3), `dump_discarded` (33, 128); dump 4 then is an orphan and gives no event; there is no `dump_reassembled` |
| 13.760 | 110 | payload 6 completes: `seq_gap` (32, 1) |

**`a2-downlink-corrupt`**: `flip`, `truncate`, and `delay` of downlink frames.

| Index | Frame | Fault |
|---|---|---|
| 5 | OID frame (0.625 s) | `flip` octet 1, bit 6 (the VCID 7 becomes 6) |
| 20 | VC 1, first field of payload 1 (2.5 s) | `delay` 1 200 000 µs |
| 66 | VC 0, HK 8 (8.25 s) | `flip` octet 100, bit 3 (the data field) |
| 148 | VC 1, first field of payload 9 (18.5 s) | `truncate` 100 octets |

The strict decoder checks the FECF before it reads any field, so the first flip is `CrcMismatch` (8) and not `UnknownChannel`; the node gives the receiver `CallerDiscard` for it and for the second flip (LNK-080), which raises no event, and a lost frame shows as a gap at the next frame. The truncated frame fails the length check, `LengthMismatch` (3). The delay is more than one slot, so frame 20 arrives after frames 21 to 29 (3.71 s) and before frame 30. The ground node raises the events below. The table leaves out the events no fault changes, such as `dump_reassembled` and the `clcw_changed` events of the nominal run, which the trace still holds:

| Arrival (s) | Frame | Events |
|---|---|---|
| 0.635 | 5 | `frame_rejected` (8) |
| 0.760 | 6 | `frame_gap` (0, 0, 1), `frame_gap` (1, 7, 1) |
| 2.635 | 21 | `frame_gap` (0, 0, 1), then `clcw_changed` 16 777 216 |
| 3.510 | 28 | `frame_gap` (1, 1, 1) (the second field of payload 1, with no packet in progress) |
| 3.710 | 20 | `frame_discontinuity` (0, 0), `frame_discontinuity` (1, 1), `clcw_changed` 16 778 752 (the CLCW of the slot of frame 20) |
| 3.760 | 30 | `frame_gap` (0, 0, 9) (the expectation was reset to 21 by frame 20), `clcw_changed` 16 777 728 |
| 4.510 | 36 | `frame_gap` (1, 1, 1), `partial_discarded` (1, 211) (the late first field of payload 1) |
| 5.510 | 44 | payload 2 completes: `seq_gap` (32, 1) |
| 8.260 | 66 | `frame_rejected` (8) |
| 8.385 | 67 | `frame_gap` (0, 0, 1) |
| 9.260 | 74 | `frame_gap` (1, 0, 1), HK 9 arrives: `seq_gap` (16, 1) |
| 18.510 | 148 | `frame_rejected` (3) |
| 18.635 | 149 | `frame_gap` (0, 0, 1) |
| 19.510 | 156 | `frame_gap` (1, 1, 1) |
| 25.510 | 204 | payload 10 completes: `seq_gap` (32, 1) |

**`a2-uplink-faults`**: `drop`, `flip`, and `duplicate` of uplink frames on the Bypass service (APP-043), with no recovery.

| Index | Frame | Fault |
|---|---|---|
| 0 | `NOOP`, seq 0 (1.0 s) | `flip` octet 12, bit 7 (the function code, so the FECF fails) |
| 2 | table upload, second segment (2.25 s) | `drop` |
| 6 | `SET_PAYLOAD_MODE` (off) and `NOOP`, one FDU (5.0 s) | `duplicate` |

The flight node raises, in the order of APP-139:

| Arrival (s) | Frame | Events |
|---|---|---|
| 1.010 | 0 | `frame_rejected` (8); FARM-1 sees an invalid frame (E9), the counter does not move; the `NOOP` never executes |
| 2.510 | 3 | the first segment is held and the last one arrives: the `MapReassembler` refuses the FDU, the packet of 120 + 69 octets disagreeing with its own length field, `packet_rejected` (3) (`LengthMismatch`); no `table_received` |
| 3.010 | 4 | `cmd_received` (3, 1) |
| 4.010 | 5 | `cmd_received` (2, 2) |
| 5.010 | 6, copies 0 and 1 | `cmd_received` (4, 3), `cmd_received` (1, 4), and the same two again; FARM-1 accepts both copies |
| 6.010 | 7 | `cmd_received` (4, 5) |

The consequences are on board: no `CMD_EXECUTED` at 6.0 s; at 10.0 s `CMD_EXECUTED` (3, 1, 0) and `TABLE_REJECTED` (1, 1, 0) (no table); the dump at 12.0 s as in `a2-nominal`; at 20.0 s the two commands run twice (`CMD_EXECUTED` (4, 3, 0), `PAYLOAD_MODE` (0), `CMD_EXECUTED` (1, 4, 0), and the same again). The time-tag queue holds at most 7 commands. The downlink has the same frame kinds as in `a2-nominal` (the fields differ in their idle fill). The ground node raises `clcw_changed` at 2.135 s (16 777 728), 2.635 s (16 778 240), 3.135 s (16 778 752), 4.135 s (16 777 216), 5.135 s (16 778 240: two frames accepted), and 6.135 s (16 778 752), `dump_reassembled` as in `a2-nominal`, and no other frame-level event.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-140 | The cases `a2-nominal`, `a2-downlink-loss`, `a2-downlink-corrupt`, and `a2-uplink-faults` SHALL be those of §6.7, each in the scenario file with its golden trace, and their frame-level and packet-level events SHALL be those the tables of §6.7 give. | — | T |
| APP-141 | In a frame-level case (A2 on) the harness SHALL reject a scenario file that contains an `immediate` command, and a time-tagged command that the flight node rejects at delivery (§2.3: a time tag before the arrival, reason 2, or a full queue, reason 4), which it finds by following the queue of the script as for APP-128. It finds the arrival instant of each command by serving the ops entries in release order, one frame per uplink slot (§4.2): the commands released at one instant share a frame data unit while they fit, a table upload takes the slots of its segments, and a frame arrives 10 000 µs after its slot. Reason: a frame arrives off the 125 000 µs grid (APP-061), so an effect at delivery would be off-grid. | APP-061, APP-128 | T |
| APP-143 | In frame mode the ground node SHALL count every input that the segmenter (a submit or the request for the next FDU) or FOP-1 refuses, and every packet of the script whose APID has no MAP (error `UnknownChannel`, stage 1, MAP ID 0, since there is none), and raise `uplink_refused` (§7.4) at once with the stage, the error code, and the MAP ID; a refused request or FOP-1 input leaves no frame for that slot, and the node SHALL NOT retry the input. | LNK-040, LNK-043; COP-010 | T |

### 6.8 Cases of A3

The five cases are added by APP-002 (a case, no change of an existing one), with the schedules of §4.5.5 and the rules of §4.5.1 to §4.5.4. Each has the end instant 35 s and the ops script of `a1-nominal` (the entries of §2.4 marked A1) with the entry marked A3 (*Initiate AD Service (without CLCW check)* at 0.0 s) and, where the case says so, further `fop1_directive` entries; none uses an immediate command (APP-141). The cases run at the link level `cop1`. Faults address units by index as sent (§6.5): a downlink index is the slot number, and an uplink index counts every unit the ground node sends, the retransmissions and the Type-BC frame included, in the order sent, so after a retransmission it differs from the indexes of §4.4.4. FDU *n* is the *n*-th frame data unit the ground node requests, from 0: FDU 0 is `NOOP` (seq 0), FDUs 1 to 3 are the three segments of the table upload, FDU 4 is `ACTIVATE_TABLE`, FDU 5 `DUMP_MEMORY`, FDU 6 the two commands of 5.0 s, and FDU 7 `SET_PAYLOAD_MODE` (on); it is the request identifier of §7.4. Arrival instants are the send instants plus 10 000 µs. The fault scripts, the directive scripts, and every table below are fixed here (APP-005) and were derived by hand from Tables 5-1 and 6-1 of 232.1 and the slot model, before any node exists; the blessed traces are compared with them (D8 of the A3 plan). A retransmitted frame has the octets of its first transmission (COP-011), so only the instants and the index change.

In each table, "E*n*" names the cell of Table 5-1 (FOP-1, ground) or Table 6-1 (FARM-1, flight) that the event is, as §4.5.4 reads them, and `clcw_changed` is listed only when the OCF word changes, with the word as decimal (§4.5.3). The downlink of each case has the frame kinds of `a2-nominal` (§4.4.4) in the slots, except where a case drops frames. Events of one instant are in the order of APP-144, APP-146, and APP-149.

**`a3-nominal`** has no fault and no directive entry beyond that of 0.0 s: the schedule of §4.5.5.

#### `a3-uplink-loss`: the headline case

| Fault | Index | Unit | Purpose |
|---|---|---|---|
| `drop` | up 2 | the frame of N(S) = 2, the second segment of the table (2.25 s) | a loss with later frames behind it: E3 on board, Retransmit flag in the CLCW, E10 on the ground, a retransmission of the lost frame and of the one behind it |
| `drop` | down 33 to 39 | the seven frames of 4.125 s to 4.875 s, which carry the acknowledgements of N(S) = 5 | an acknowledged frame whose acknowledgement is lost: T1 expires, the frame is sent again and discarded as a duplicate (E4) |
| `drop` | up 10 | the frame of N(S) = 7, the last command (6.0 s) | a lone command frame with no later frame behind it: no negative acknowledgement is possible, and only T1 retransmits it |

The directive script is that of 0.0 s. Uplink units (all Type-AD, first octets `01 55`):

| Index | t (s) | N(S) | Content | Arrival | FARM-1 |
|---|---|---|---|---|---|
| 0 | 1.0 | 0 | FDU 0 | 1.010 | E1, accepted |
| 1 | 2.0 | 1 | FDU 1 | 2.010 | E1 |
| 2 | 2.25 | 2 | FDU 2 | dropped | |
| 3 | 2.5 | 3 | FDU 3 | 2.510 | E3: discarded, Retransmit flag set |
| 4 | 2.75 | 2 | FDU 2 again | 2.760 | E1, Retransmit flag reset |
| 5 | 3.0 | 3 | FDU 3 again | 3.010 | E1; the table is complete |
| 6 | 3.25 | 4 | FDU 4 (ordered for 3.0 s, behind the two retransmissions) | 3.260 | E1 |
| 7 | 4.0 | 5 | FDU 5 | 4.010 | E1; the CLCWs of slots 4.125 to 4.875 are dropped |
| 8 | 5.0 | 5 | FDU 5 again, at the expiry of T1 | 5.010 | E4: discarded as a duplicate |
| 9 | 5.25 | 6 | FDU 6 (ordered for 5.0 s, behind the retransmission) | 5.260 | E1 |
| 10 | 6.0 | 7 | FDU 7 | dropped | |
| 11 | 7.0 | 7 | FDU 7 again, at the expiry of T1 | 7.010 | E1 |

**The retransmissions.** The second segment is lost at 2.25 s, FARM-1 discards the third at 2.51 s with E3 and sets Retransmit, and the first CLCW that shows it (the slot 2.625 s, N(R) = 2) reaches FOP-1 at 2.635 s: some frames outstanding, N(R) = NN(R), Retransmit 1, Transmission_Count 1 below the limit 3, so E10: Initiate AD Retransmission (count 2, timer 2.635 s to 3.635 s, both frames marked), S1 to S2. The frame of N(S) = 2 leaves in the slot of 2.75 s and the one of N(S) = 3 in the slot of 3.0 s; the CLCW of the slot 2.75 s still shows Retransmit (E10 in S2: Ignore), that of 2.875 s shows N(R) = 3 (E6: the first frame is acknowledged, S2 to S1), and that of 3.125 s shows N(R) = 4 (E6). FDU 4, requested at 3.0 s, waits in the Wait_Queue behind the retransmissions and is Accepted when the slot of 3.0 s is answered, with its transmit decision held for 3.25 s, which restarts the timer to 4.0 s; the last retransmission is acknowledged at 3.135 s, 500 000 µs after the order and 500 000 µs before the timer that the order started would have expired (§4.5.4). FDU 6 waits for the same reason behind the retransmission at 5.0 s.

**Ground events** (the events of `a3-nominal` that are not listed are the same: `clcw_changed` and `fop1_transfer` Positive Confirm for each acknowledgement):

| t (s) | Events |
|---|---|
| 0.0 | `fop1_directive` (1, 1, 0), `fop1_directive` (3, 1, 0), `fop1_state` (1) |
| 1.135 | `clcw_changed` 16 777 217, `fop1_transfer` (3, 0) |
| 2.135 | `clcw_changed` 16 777 218, `fop1_transfer` (3, 1) |
| 2.635 | `clcw_changed` 16 779 266 (N(R) = 2, Retransmit), `fop1_state` (2) (E10) |
| 2.885 | `clcw_changed` 16 777 219 (N(R) = 3), `fop1_transfer` (3, 2), `fop1_state` (1) (E6) |
| 3.135 | `clcw_changed` 16 777 220, `fop1_transfer` (3, 3) |
| 3.385 | `clcw_changed` 16 777 221, `fop1_transfer` (3, 4) |
| 5.0 | the timer of FDU 5 (started 4.0 s) expires: `Timeout`, E16, the frame is sent again; no event |
| 5.010 | `frame_gap` (0, 0, 7), `frame_gap` (1, 7, 5) (of the seven dropped frames, five are OID frames of VC 7, reported here as the gap of 5 on VC 7; the other two are frames of VC 0 whose gaps are reported by the rows of 5.260 s and 5.510 s below, when the next VC 0 frame arrives), `clcw_changed` 16 777 222 (N(R) = 6), `fop1_transfer` (3, 5) (E6: FDU 5 is acknowledged by the first CLCW the ground node sees after 4.01 s) |
| 5.260 | `frame_gap` (1, 0, 1) (the housekeeping frame of 4.25 s), then HK 5 arrives and `seq_gap` (16, 1) |
| 5.385 | `clcw_changed` 16 777 223 (N(R) = 7), `fop1_transfer` (3, 6) |
| 5.510 | `frame_gap` (1, 1, 1): the second field of payload 2, whose first field (4.5 s) was dropped, is the tail of a packet the extractor never saw and gives no event, as in `a2-downlink-loss`; payload 2 is lost |
| 7.0 | the timer of FDU 7 (started 6.0 s) expires: `Timeout`, E16, the frame is sent again; no event |
| 7.135 | `clcw_changed` 16 777 224 (N(R) = 8), `fop1_transfer` (3, 7) |
| 7.510 | payload 3 completes: `seq_gap` (32, 1) |

**Flight events:** 1.010 `cmd_received` (1, 0); 2.510 `frame_discarded` (2, 3); 3.010 `table_received` (1, 300); 3.260 `cmd_received` (3, 1); 4.010 `cmd_received` (2, 2); 5.010 `frame_discarded` (2, 4); 5.260 `cmd_received` (4, 3) and `cmd_received` (1, 4); 7.010 `cmd_received` (4, 5). FARM-1 changes no state.

**Timers.** The timer of N(S) = 5 started at 4.0 s and the CLCWs that could acknowledge it were dropped, so it expires at 5.0 s with the count at 1 (E16): this is the only reason, 1 000 000 µs after the frame left. The timer of N(S) = 7 started at 6.0 s and nothing could acknowledge a frame that never arrived, and expires at 7.0 s (E16). No other timer expires: each of the others is cancelled by E2 135 000 µs after it starts or restarted by a new frame. The `Timeout` of 5.0 s falls at the instant of the ops entries of that slot and goes before them (§4.5.1).

**On board.** Every command executes as in `a3-nominal`: the latest arrival is that of `SET_PAYLOAD_MODE` (on) at 7.010 s, before its execution time 24.0 s. The ground node loses housekeeping packet 4 and payload packet 2 to the dropped frames (their `packet_in` is absent) and everything else of the downlink arrives.

#### `a3-uplink-disorder`

| Fault | Index | Unit | Purpose |
|---|---|---|---|
| `duplicate` | up 0 | the frame of N(S) = 0 (1.0 s) | E4 on board: the second copy is in the negative window |
| `delay` 400 000 µs | up 2 | the frame of N(S) = 2 (2.25 s), so it arrives at 2.660 s, after N(S) = 3 | reordering through the window: E3, a retransmission of both frames, and the delayed original accepted before its retransmission (E4) |
| `flip` octet 12, bit 7 | up 9 | the frame of N(S) = 7 (6.0 s), in the function code of its command | the FECF catches it (E9); only T1 retransmits it |

The directive script is that of 0.0 s. Uplink units:

| Index | t (s) | N(S) | Content | Arrival | FARM-1 |
|---|---|---|---|---|---|
| 0 | 1.0 | 0 | FDU 0 | 1.010, copies 0 and 1 | E1, then E4 |
| 1 | 2.0 | 1 | FDU 1 | 2.010 | E1 |
| 2 | 2.25 | 2 | FDU 2 | 2.660 (delayed) | E1, Retransmit flag reset |
| 3 | 2.5 | 3 | FDU 3 | 2.510 | E3: discarded, Retransmit flag set |
| 4 | 2.75 | 2 | FDU 2 again | 2.760 | E4: discarded |
| 5 | 3.0 | 3 | FDU 3 again | 3.010 | E1; the table is complete |
| 6 | 3.25 | 4 | FDU 4 (as in `a3-uplink-loss`) | 3.260 | E1 |
| 7 | 4.0 | 5 | FDU 5 | 4.010 | E1 |
| 8 | 5.0 | 6 | FDU 6 | 5.010 | E1 |
| 9 | 6.0 | 7 | FDU 7 | 6.010 | FECF fails: not given to the reassembler |
| 10 | 7.0 | 7 | FDU 7 again, at the expiry of T1 | 7.010 | E1 |

The CLCW of the slot 2.625 s (N(R) = 2, Retransmit 1: after 2.510 s and before 2.660 s) gives E10 at 2.635 s as in `a3-uplink-loss`, and the delayed original arriving at 2.660 s makes the CLCW of the slot 2.75 s show N(R) = 3 and Retransmit 0, which reaches FOP-1 at 2.760 s as E6 (S2 to S1): FDU 2 is acknowledged by its original, and the retransmission that left at 2.75 s is the duplicate. The retransmission of N(S) = 3 (3.0 s) is the frame FARM-1 accepts, with the margins of `a3-uplink-loss` (the series of two frames is acknowledged 500 000 µs after the order).

**Ground events** (as `a3-nominal` for the rest):

| t (s) | Events |
|---|---|
| 2.635 | `clcw_changed` 16 779 266, `fop1_state` (2) |
| 2.760 | `clcw_changed` 16 777 219 (N(R) = 3), `fop1_transfer` (3, 2), `fop1_state` (1) |
| 3.135 | `clcw_changed` 16 777 220, `fop1_transfer` (3, 3) |
| 3.385 | `clcw_changed` 16 777 221, `fop1_transfer` (3, 4) |
| 4.135, 5.135 | `clcw_changed` 16 777 222, 16 777 223, with `fop1_transfer` (3, 5), (3, 6) |
| 7.0 | the timer of FDU 7 expires: `Timeout`, E16; no event |
| 7.135 | `clcw_changed` 16 777 224, `fop1_transfer` (3, 7) |

The events of 1.135 s and 2.135 s are those of `a3-nominal`; the frame of 2.0 s is acknowledged at 2.135 s. **Flight events:** 1.010 `cmd_received` (1, 0), then `frame_discarded` (2, 4) (copy 1); 2.510 `frame_discarded` (2, 3); 2.760 `frame_discarded` (2, 4); 3.010 `table_received` (1, 300); 3.260 `cmd_received` (3, 1); 4.010 `cmd_received` (2, 2); 5.010 `cmd_received` (4, 3) and (1, 4); 6.010 `frame_rejected` (8) (`CrcMismatch`; FARM-1 given E9, no change); 7.010 `cmd_received` (4, 5). The downlink has no loss, so the ground node raises no other event; every command executes as in `a3-nominal` (the latest arrival is 7.010 s).

#### `a3-uplink-lockout`

| Fault | Index | Unit | Purpose |
|---|---|---|---|
| `drop` | down 16 | the frame of 2.0 s (an OID frame), whose CLCW is the last to show the old N(R) after the *Set V(S)* | without it FOP-1 would alert on N(R) (E13, Alert[NN(R)]) at 2.010 s, before the CLCW with the Lockout Flag reaches it (§4.5.4) |

| At (s) | `fop1_directive` entries, in array order (before the table upload of 2.0 s) |
|---|---|
| 0.0 | `"initiate_ad_without_clcw"` (the entry marked A3) |
| 2.0 | `"terminate_ad"`, then `{"set_vs":128}`, then `"initiate_ad_without_clcw"` |
| 2.25 | `"terminate_ad"` |
| 2.375 | `{"set_vs":1}` |
| 2.5 | `"initiate_ad_with_unlock"` |

The set-up at 2.0 s is *Terminate AD Service* (S1: Accept, Alert[term], Confirm, S6), *Set V(S)* to 128 (S6: accepted, V(S) = NN(R) = 128), and *Initiate AD Service (without CLCW check)* (E23, S1): the variant that precedes a lockout, since E14 raises Alert[Lockout] in S1 to S4 and ignores the Lockout Flag in S5 and S6 (§4.5.2). FDU 1 leaves in the slot of 2.0 s with N(S) = 128 while FARM-1 expects V(R) = 1: 128 is in the lockout area (5 to 252), so FARM-1 discards it, sets the Lockout Flag, and goes to S3 (E5). The CLCW of the slot 2.125 s (the slot 2.0 s is dropped) has Lockout 1 and N(R) = 1 and gives E14 at 2.135 s: Alert[Lockout], FOP-1 purges, negatively confirms FDU 1, and goes to S6. FDUs 2 and 3, requested at 2.25 s and 2.5 s, are rejected by S6 (E19) and the ground node does not resubmit them (APP-145). The recovery is *Terminate AD Service* in S6 (Accept, Confirm, no Alert, E29), *Set V(S)* to 1, the value FARM-1 reports as N(R), and *Initiate AD Service (with Unlock)* (E25, S6 to S5), which transmits a Type-BC frame in the slot of 2.5 s: the CLCW of that slot, sampled before the Unlock arrives, still shows Lockout, and E14 in S5 is Ignore. FARM-1 executes the Unlock at 2.510 s (E7: Lockout, Retransmit, and Wait reset, the FARM-B Counter counts it, S3 to S1), the CLCW of the slot 2.625 s is clean (N(R) = 1, V(S) = NN(R) = 1), and E1 in S5 confirms the directive at 2.635 s, releases the Type-BC frame, cancels its timer, and goes to S1. From 3.0 s every FDU leaves in its own slot.

| Index | t (s) | Frame | Arrival | FARM-1 |
|---|---|---|---|---|
| 0 | 1.0 | Type-AD, N(S) = 0, FDU 0 | 1.010 | E1 |
| 1 | 2.0 | Type-AD, N(S) = 128, FDU 1 (`01 55 00 7F 80` `41`) | 2.010 | E5: discarded, S1 to S3 |
| 2 | 2.5 | Type-BC, Unlock (`31 55 00 07 00` `00`, 8 octets) | 2.510 | E7: S3 to S1, FARM-B Counter 1 |
| 3 | 3.0 | Type-AD, N(S) = 1, FDU 4 | 3.010 | E1 |
| 4 | 4.0 | Type-AD, N(S) = 2, FDU 5 | 4.010 | E1 |
| 5 | 5.0 | Type-AD, N(S) = 3, FDU 6 | 5.010 | E1 |
| 6 | 6.0 | Type-AD, N(S) = 4, FDU 7 | 6.010 | E1 |

**Ground events** (the unlisted ones are those of `a3-nominal` with the FDU and N(R) numbers of the table above):

| t (s) | Events |
|---|---|
| 0.0 | `fop1_directive` (1, 1, 0), (3, 1, 0), `fop1_state` (1) |
| 1.135 | `clcw_changed` 16 777 217, `fop1_transfer` (3, 0) |
| 2.0 | `fop1_directive` (1, 5, 0), `fop1_alert` (8, 29, 1), `fop1_directive` (3, 5, 0), `fop1_state` (6); `fop1_directive` (1, 7, 128), (3, 7, 128); `fop1_directive` (1, 1, 0), (3, 1, 0), `fop1_state` (1) |
| 2.135 | `frame_gap` (0, 0, 1), `frame_gap` (1, 7, 1), `clcw_changed` 16 785 409 (N(R) = 1, Lockout), `fop1_transfer` (4, 1), `fop1_alert` (3, 14, 1), `fop1_state` (6) |
| 2.25 | `fop1_directive` (1, 5, 0), (3, 5, 0) (no Alert, no change of state); `fop1_transfer` (2, 2) |
| 2.375 | `fop1_directive` (1, 7, 1), (3, 7, 1) |
| 2.5 | `fop1_directive` (1, 3, 0), `fop1_state` (5); `fop1_transfer` (2, 3) (S5 rejects the request) |
| 2.635 | `clcw_changed` 16 777 729 (N(R) = 1, FARM-B Counter 1), `fop1_directive` (3, 3, 0), `fop1_state` (1) |
| 3.135, 4.135, 5.135, 6.135 | `clcw_changed` 16 777 730, 16 777 731, 16 777 732, 16 777 733, with `fop1_transfer` (3, 4), (3, 5), (3, 6), (3, 7) |

The timer of FDU 1 (2.0 s to 3.0 s) is cancelled by the Alert at 2.135 s and that of the Type-BC frame (2.5 s to 3.5 s) by E1 at 2.635 s. **Flight events:** 1.010 `cmd_received` (1, 0); 2.010 `frame_discarded` (2, 5), `farm1_state` (3); 2.510 `farm1_unlock`, `farm1_state` (1); 3.010 `cmd_received` (3, 1); 4.010 `cmd_received` (2, 2); 5.010 `cmd_received` (4, 3), (1, 4); 6.010 `cmd_received` (4, 5). **On board:** the table upload is lost (FDU 1 negatively confirmed, FDUs 2 and 3 rejected), so no `table_received`; at 10.0 s `CMD_EXECUTED` (3, 1, 0) and `TABLE_REJECTED` (1, 1, 0) (no table) replace `TABLE_ACTIVATED`, and housekeeping reports no activated table; every other command executes as in `a3-nominal` (the latest arrival is 6.010 s).

#### `a3-uplink-limit`

| Fault | Index | Unit | Purpose |
|---|---|---|---|
| `drop` | up 7, 8, 9 | the frame of N(S) = 7 at 6.0 s, and its retransmissions at 7.0 s and 8.0 s | the same frame dropped until Transmission_Limit = 3 transmissions are made: Alert[T1] (E17) at 9.0 s |

| At (s) | `fop1_directive` entries |
|---|---|
| 0.0 | `"initiate_ad_without_clcw"` |
| 10.0 | `{"set_vs":7}` |
| 11.0 | `"initiate_ad_with_clcw"` |

Indexes 0 to 6 are those of `a3-nominal`. FDU 7 leaves at 6.0 s (index 7), 7.0 s (index 8, T1 expired with the count at 1: E16, count 2), and 8.0 s (index 9, E16, count 3), and every copy is dropped. At 9.0 s the timer expires with the count at the limit (E17, Timeout_Type 0): Alert[T1], FOP-1 purges, negatively confirms FDU 7, and goes to S6 with V(S) = NN(R) = 8 (OI-60). The CLCWs show N(R) = 7 throughout (E5: Ignore), and in S6 every CLCW is ignored. The recovery sets V(S) to 7, the N(R) of the CLCWs (Table 4-1 note 2), in S6 at 10.0 s (E35), and *Initiate AD Service (with CLCW check)* at 11.0 s (E24, S6 to S4, timer 11.0 s to 12.0 s): the CLCW of the slot 11.0 s reaches FOP-1 at 11.010 s with N(R) = V(S) = NN(R) = 7 and no flag (E1 in S4), which confirms the directive, cancels the timer, and goes to S1.

**Ground events** (the events of `a3-nominal` up to 5.135 s, with FDUs 0 to 6, are unchanged; after it the CLCW word stays 16 777 223):

| t (s) | Events |
|---|---|
| 7.0, 8.0 | the timer expires (E16); the frame is sent again (a `unit_sent`, dropped); no event |
| 9.0 | `fop1_transfer` (4, 7), `fop1_alert` (2, 17, 1), `fop1_state` (6) |
| 10.0 | `fop1_directive` (1, 7, 7), `fop1_directive` (3, 7, 7) |
| 11.0 | `fop1_directive` (1, 2, 0), `fop1_state` (4) |
| 11.010 | `fop1_directive` (3, 2, 0), `fop1_state` (1) |

**Flight events:** 1.010 `cmd_received` (1, 0); 2.510 `table_received` (1, 300); 3.010 `cmd_received` (3, 1); 4.010 `cmd_received` (2, 2); 5.010 `cmd_received` (4, 3), (1, 4). **On board:** `SET_PAYLOAD_MODE` (on) never arrives, so the payload mode stays *off* after 20.0 s: no `PAYLOAD_MODE` (1) event (8 event packets), no payload packet at 24.5, 26.5, or 28.5 s (10 payload packets), and housekeeping reports payload mode 0 from 20.0 s. The packet projection differs from `a3-nominal` and is not part of APP-003. Timers: the three timers of FDU 7 are 1 000 000 µs each (6.0 s, 7.0 s, 8.0 s), and none of the others expires.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-151 | The cases `a3-nominal`, `a3-uplink-loss`, `a3-uplink-disorder`, `a3-uplink-lockout`, and `a3-uplink-limit` SHALL be those of §4.5.5 and §6.8, each at the link level `cop1` in the scenario file with its golden trace, with the ops, directive, and fault scripts, the uplink schedules, and the events that the tables of §6.8 give; the fault and directive scripts SHALL NOT change after the traces are blessed (APP-005), and the retransmission instants and indexes of each case SHALL be those of its table, derived from the cells of 232.1 Tables 5-1 and 6-1. | 232.1 §5.1.9.1, §5.1.10, §6.1.8.3.1, Tables 5-1, 6-1 (E1 to E10, E13, E14, E16, E17, E23 to E29, E35); COP-001, COP-002 | T |
| APP-152 | At the `cop1` level the harness SHALL apply the check of APP-141 to the nominal uplink schedule of §4.2 (one frame per uplink slot in release order, no fault, no retransmission, and no `fop1_directive` entry considered), and SHALL reject a file that contains an `immediate` command. For the cases of §6.8 the arrival of each command under the faults and directives of the case is that of its table, and every execution time is later than that arrival (the latest is 7.010 s, against an execution time of 24.0 s). | APP-141, APP-014 | T |


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
| `packet_in` | `t_us`, `kind`, `node`, `apid`, `hex` | A packet is delivered to an application of `node`. Only the ground node's telemetry packets are `packet_in`: the flight node's command and table-upload packets appear as the application events `cmd_received` and `table_received` (APP-129), never as `packet_in` |
| `app_event` | `t_us`, `kind`, `node`, `code`, `name`, `args` | A node raises an application event (§7.4); `args` is a list of unsigned integers |

Library-internal events and step instants are **not** recorded, so an implementation that drives the engines through the C ABI or the C++ wrapper can produce the same trace as the Rust nodes.

### 7.4 Application events

A closed vocabulary with stable codes, mapped by each node from library events and its own state, in the spirit of API-080. Codes are never renumbered or reused. A2 adds frame-level codes in 100–199, A3 COP-1 codes (FOP-1 notifications and alerts, FARM-1 state) in 200–299: 200 to 203 of the ground node, 210 and 211 of the flight node.

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
| 100 | `frame_gap` | ground | scope (0 master channel, 1 virtual channel), VCID (0 for scope 0), missing count |
| 101 | `frame_discontinuity` | ground | scope, VCID |
| 102 | `frame_rejected` | either | error code of the catalogue in [03 §9](03-api-model.md#9-error-catalogue) |
| 103 | `frame_discarded` | either | layer (1 frame receiver, 2 FARM-1), reason (layer 1: 1 `UnknownChannel`, 2 `NotServed`; layer 2: the number of the FARM-1 event whose cell discards the frame: E1 in Lockout, E2 to E5, and E9; 232.1 Table 6-1); the ground node also raises it with layer 1, reason 2 for a data frame on the OID VC (APP-138) |
| 104 | `clcw_changed` | ground | the OCF word, as an unsigned 32-bit integer |
| 105 | `partial_discarded` | ground | VCID, octets discarded |
| 106 | `extraction_error` | ground | VCID, kind (1 `PacketTooLong`, 2 `InvalidPvn`, 3 `LengthInconsistent`) |
| 110 | `sequence_violation` | flight | MAP ID |
| 111 | `unknown_map` | flight | MAP ID |
| 112 | `sdu_too_long` | flight | MAP ID |
| 113 | `downlink_refused` | flight | error code of the catalogue in [03 §9](03-api-model.md#9-error-catalogue), and the VCID of the packet when a generator refused one (absent when a slot failed) |
| 114 | `uplink_refused` | ground | stage (1 submit to the segmenter, 2 request for the next FDU, 3 FOP-1 input), error code of the catalogue in [03 §9](03-api-model.md#9-error-catalogue), MAP ID (stage 1 when the APID has a MAP; 0 when it has none, which is error `UnknownChannel`, and for the other stages) |
| 200 | `fop1_alert` | ground | reason (1 Limit, 2 T1, 3 Lockout, 4 Synch, 5 NN(R), 6 CLCW, 7 LLIF, 8 Term: the Alert reasons of 232.1 Table 4-4 in that order), the number of the FOP-1 event that caused it (1 to 46, 101 to 104, the E of Table 5-1), the FOP-1 state before it (1 to 6 for S1 to S6) |
| 201 | `fop1_transfer` | ground | notification (2 Reject, 3 Positive Confirm, 4 Negative Confirm: the order of 232.1 Table 4-6, whose Accept, 1, is not an event), request identifier of the FDU (§6.8) |
| 202 | `fop1_directive` | ground | notification (1 Accept, 2 Reject, 3 Positive Confirm, 4 Negative Confirm: 232.1 Table 4-2), Directive Type (the code of §4.5.2), Qualifier (V\*(S) of *Set V(S)*; 0 when there is none) |
| 203 | `fop1_state` | ground | the new FOP-1 state (1 to 6 for S1 to S6) |
| 210 | `farm1_state` | flight | the new FARM-1 state (1 Open, 2 Wait, 3 Lockout; 232.1 §6.1.2) |
| 211 | `farm1_unlock` | flight | none |

The frame-level events are raised on a change or an error, never per frame, so that a trace of 280 frames stays readable. `frame_gap` and `frame_discontinuity` are the `FrameGap` and `CountDiscontinuity` actions of the `FrameReceiver` (LNK-051): a count ahead of the expected one, with the number missing (modulo 256), and a count that repeats or goes back, which resets the expectation to the count received plus one. `frame_rejected` is a failure of the strict codec, including the FECF (TM-005, TC-006); the `CallerDiscard` that the node gives the receiver for a FECF failure (LNK-080) counts it, and the node does not map the `FrameDiscarded` action it returns to `frame_discarded`. `frame_discarded` is a frame that the codec accepted and an engine discards: no case of A2 raises it (the VCs and MAPs of the codec and the engines agree, and Type-BD frames are always accepted), and A3 gives it its FARM-1 reasons. `clcw_changed` is raised when the OCF word of a frame differs from that of the last frame decoded (initially `0x01000000`, §4.4.1): a CLCW or any other OCF; a frame that is rejected leaves it as it was. `partial_discarded` is the `Discarded` action of the extractor, a packet in progress thrown away because the VC count had a gap or a discontinuity (LNK-061); only a gap of the VC's own count is flagged on its data field, since a frame lost by another VC cuts none of its packets. The reassembly events are those of `ReassemblyEvent` (LNK-070) that TC can raise; `GapDiscard` is USLP's. `downlink_refused` is the flight node's error of the downlink: a generator refused a packet that was built, or an engine failed in a slot and no frame was output (APP-142). The COP-1 events are raised on a notification, an Alert, or a change of state, never per frame (APP-149). A change of state is raised after the other events of the cell that caused it, which is when 232.1 §5.3.1 and §6.3.1 update the State variable; an Alert is raised after the Negative Confirms and Rejects it causes (232.1 §5.2.15). The Accept of an FDU, whether FOP-1 takes it at once or later from its Wait_Queue, is not an event, so that there is one `fop1_transfer` for each FDU that is rejected or confirmed; the Accept of a directive is `fop1_directive` (1, …). A retransmission is no event: it is a `unit_sent` of the uplink. The codes of the reasons, notifications, directives, and states are this scenario's numbering of the Blue Book's names (§4.5.2, §4.5.4), which the Blue Books do not number.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-138 | For every TM frame delivered the ground node SHALL output, in this order: `frame_rejected` if the strict decode or the validation fails, and nothing further; otherwise the receiver's actions in the engine's order: the `frame_gap` or `frame_discontinuity` of the master channel, then that of the VC, `clcw_changed` if the OCF word changed, and, for a data frame on VC 0 or 1, the extractor's `partial_discarded` or `extraction_error` and then, for every packet extracted in order, the events of APP-130 (`packet_rejected` or `packet_in` and what follows it); an OID frame gives no data and no packet event; a data frame on the OID VC (VC 7), which carries OID frames only (§4.1) and has no extractor, is delivered by the receiver and discarded by the node, which outputs `frame_discarded` (layer 1, reason 2, "not served") as the last output of the frame, since no application serves the VC; `frame_discarded` is the last output of a frame that the receiver discards, but not for the discard that the node reports for a FECF failure. The codes and arguments are those of §7.4. | LNK-051, LNK-052, LNK-060…LNK-064, LNK-080; 132.0 §4.3 | T |
| APP-139 | For every TC frame delivered the flight node SHALL output, in this order: `frame_rejected` if the strict decode or the validation fails (TC-021, TC-023), with FARM-1 given the invalid-frame input (E9, no change of its state), whose discard is not mapped to `frame_discarded`, and nothing further; `frame_discarded` if the receiver or FARM-1 discards the frame, and nothing further; otherwise, for the FDU that FARM-1 accepts, the reassembly events in the engine's order (`sequence_violation`, `unknown_map`, `sdu_too_long`; a valid frame whose MAP ID is not configured is accepted and counted by FARM-1 and gives `unknown_map`, OI-98) and then, for each packet the MAP reassembler delivers, in order, the handling of A1 (`cmd_received`, `table_received`, or `packet_rejected`, APP-129, APP-131); an FDU that the MAP reassembler refuses gives `packet_rejected` with the error code. A valid frame in sequence raises no frame-level event. | LNK-050, LNK-070, LNK-071; 232.0 §4.4.1, §4.4.5, §4.4.9; 232.1 Table 6-1 | T |
| APP-142 | When a generator of the flight node refuses a downlink packet that was built (for example `WouldBlock`, the field and the queue cannot take it), the node SHALL count the failure and raise `downlink_refused` (§7.4) with the error code and the VCID, in the step in which it happened; the `packet_out` record of the packet stays. When an engine fails in a step with `tx_slot`, the node SHALL count the failure, raise `downlink_refused` with the error code alone, and output no frame for that slot. This extends APP-131 to the frame level on the transmit side; `frame_rejected` stays the receive-side failure of APP-138 and APP-139. | LNK-015, LNK-076; API-083 | T |

### 7.5 Comparison, location, and versioning

| Item | Rule |
|---|---|
| Comparison | Byte equality of the whole file. On a mismatch the harness prints the first differing record, decoded with the strict codecs |
| Strict decoding | CI decodes every `unit_sent` and every packet of every golden trace with the strict codecs (from A1) |
| Location | `refapp/golden/<case>.jsonl`, owned by track A |
| Versioning | A change to a released golden trace bumps its `trace_version` and records the reason in `refapp/golden/CHANGES.md`, as TV-021 does for vectors |
| Vectors | Golden traces are whole-system regression baselines, **not** conformance vectors: a frame or packet from a golden trace becomes a vector only with `hand-derived` or `cross-checked` provenance (TV-020; OI-23) |
| Framework trace | Each node's step inputs (`now`, delivered units, slot) can be extracted from a golden trace, so that the framework (after v1) can define the FW-013 trace to match and replay both nodes from the golden traces (OI-25) |

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

At A1, `tx_slot` is true in every step and a node may output any number of units. From A2, the flight node outputs exactly one unit when `tx_slot` is true and none otherwise; the ground node outputs at most one unit when `tx_slot` is true and none otherwise. `next_deadline` is the earliest instant after `now` at which the node needs a step, or none; a deadline not after `now` breaks the contract and ends the run with an error. Rust nodes (A1–A4) implement this as a trait in-process.

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

The harness sends one message and waits for its answer; it never reads the wall clock (CI job timeouts bound a hung node). A malformed message, an unexpected type, or a closed stream ends the run with an error and no trace. The harness side of the protocol, with both transports, is implemented in ACFS WP-2, before A5 (ACFS D18, OI-38); it may be refined in A5, before any trace depends on it. The cFS flight node (§10) carries the same messages over a loopback TCP stream instead (§10.6).

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-090 | Every node SHALL implement the step contract of §8.1, including the `tx_slot` rules per link level. | LNK-030 | T |
| APP-091 | Nodes SHALL be sans-I/O: they SHALL NOT read clocks, perform I/O, or hold global state (GEN-030…032 applied to the application). | GEN-031 | I |
| APP-092 | A node running out of process SHALL use the node protocol of §8.2, and the harness SHALL produce the same trace for an in-process and an out-of-process node with the same behaviour. | — | T |

## 9. Implementation rules

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-100 | `refapp/` crates SHALL depend on Aldis only through the `aldis` facade, by path, with default features off and only the features each crate uses. They SHALL NOT depend on any other `aldis-*` crate, on `aldis-testkit`, or on a `test-hooks` feature, also not as dev-dependencies, and SHALL NOT use items hidden from the documentation. From A5 the C++ node SHALL use only the C header and the C++ wrapper header (CABI-072). | — | A/I |
| APP-101 | Every API pain point found in `refapp/` SHALL be filed as a GitHub issue labelled `api-feedback` for track R; the workaround SHALL cite the issue, and each A-milestone plan SHALL list its findings and the track-R pull request that resolved each; when track R resolves such an issue with an API change, `refapp/` SHALL adopt the change and drop the workaround, in that pull request or in a track-A pull request the issue links, before the A milestone that found it is called complete. `refapp/` SHALL NOT ask for an application-specific API. | — | I |
| APP-102 | `refapp-flight` SHALL be `no_std` without a heap and SHALL pass the flight lint set (VER-001); it SHALL build for `thumbv7em-none-eabihf`. From A6 it SHALL also pass the panic-freedom link check (VER-010 method). | GEN-010, GEN-020 | A |
| APP-103 | `refapp-ground` SHALL pass the flight lint set's restriction lints, because it parses untrusted downlink; it MAY use `std`. | GEN-020 | A |
| APP-104 | `refapp-scenario` and `refapp-harness` SHALL use the tooling lint profile (RS-017). | — | A |
| APP-105 | Every crate in `refapp/` SHALL declare `#![forbid(unsafe_code)]`. | GEN-070 | A |
| APP-106 | `refapp/` SHALL be a separate Cargo workspace on the pinned toolchain (RS-001). CI SHALL check its formatting (VER-004), Clippy, tests, the `thumbv7em-none-eabihf` build of `refapp-flight`, and its licences and advisories (VER-003); from A1 CI SHALL reproduce every golden trace. | VER-001 | A |
| APP-107 | All scenario values SHALL come from one language-neutral file in `refapp/scenario/`, validated against a published JSON Schema; the Rust constants SHALL be generated from it (a C++ header from A5), and CI SHALL check that the generated files are current. | — | A |
| APP-108 | Tests in `refapp/` SHALL carry `// req:` annotations (VER-020), and `cargo xtask trace` SHALL read them and the `requirements` of golden-trace headers. | VER-020 | A |
| APP-153 | The sequence diagram in `README.md` SHALL be generated, between marker comments, from the golden trace of `a3-uplink-loss`: every frame the diagram draws SHALL be decoded with the strict decoders of APP-086, frames the link dropped and retransmissions SHALL be marked, and `cargo xtask readme-diagram` SHALL fail while the region differs from the trace (`cargo xtask fix` rewrites it). | APP-086, APP-151 | T |
| APP-154 | `refapp-harness demo` SHALL run the case `a3-uplink-loss`, print one line per event of the run (instant, direction, virtual channel and frame type, N(S) or the CLCW report, and whether the frame was lost or sent again), every figure decoded with the strict decoders of APP-086 from the diagram rows of APP-153, and end with a verdict that the run matches or differs from the golden trace, failing when it differs. `README.md` SHALL give the command and, between marker comments, its output as `refapp-harness demo` prints it, which `cargo xtask readme-diagram` keeps current. | APP-086, APP-153 | T |

## 10. cFS flight side

**Status: accepted (2026-10-21, #78).** Plan: [ACFS cFS flight side](milestones/acfs-flight-side.md), decisions D1–D19, accepted as recommended. APP-110…APP-125 are verified phase by phase as ACFS lands. The cFS facts this section relies on were read from the cFS repositories at the release the plan proposes to pin (nasa/cFS `v7.0.1`); the plan lists them with their source files as F1–F18 (§1.1), and this section cites them by those numbers.

### 10.1 Purpose

The Rust flight node (§2.1) shows Aldis in a sans-I/O application written for it. Most integrators will instead call Aldis from a flight framework with heritage, through the C ABI. The cFS flight node shows that integration with NASA's Core Flight System: cFS applications on cFE, OSAL, and the pc-linux PSP that build packets and frames with `aldis.h`, exchange messages on the cFE Software Bus, and run under the cFE executive. Because it reproduces the golden traces of the Rust flight node, it is also interoperability evidence: the same octets on the link, whichever framework produces them.

### 10.2 What stays the same

| Item | Rule |
|---|---|
| Scenario | Unchanged: applications and per-instant order (§2), APIDs and packet formats (§3), link plan (§4), time (§5), link model, faults, and cases (§6). Values come from `refapp/scenario/scenario.json` (APP-107) |
| Ground side | The Rust ground node, in-process in the harness, unchanged |
| Harness | Unchanged in what it owns: time, the link, the faults, and the trace (APP-070). The cFS process is an out-of-process flight node (§8.2, §10.6) |
| Golden traces | The same files, `refapp/golden/<case>.jsonl`, compared by byte equality (APP-085). A cFS run's trace equals the Rust flight node's at the link interface (every `fault`, `unit_sent`, and `unit_delivered` record) and also in every `packet_out`, `packet_in`, and `app_event` record, because the harness records those from the node's step outputs (§7.3) |
| Trace format | No change to APP-080…APP-088 (§10.5). The run manifest (APP-084) names the cFS release and the `aldis-ffi` build |

### 10.3 Architecture

```
harness (refapp-harness, std)        cFS process core-cpu1 (pc-linux PSP, SIMULATION=native)
┌──────────────────────────────┐
│ clock, link model, faults,   │ node protocol   refapp_step (bridge and sequencer)
│ trace writer, comparator;    │◀─── TCP ──────▶   │ WAKE(now, units, slot): one app at a time, in this order
│ Rust ground node, in-process │  127.0.0.1        ├──▶ refapp_ci  ──┐
└──────────────────────────────┘                   ├──▶ refapp_cmd ──┤ telemetry, events, DONE
                                                   ├──▶ refapp_hk  ──┤ (Software Bus, one pipe)
                                                   ├──▶ refapp_pl  ──┤
                                                   └──▶ refapp_to ◀──┘
                                                          │ packets, frames, other outputs, DONE
                                                          └──▶ refapp_step ──▶ STEP_RESULT

                                     refapp_aldis: cFE library wrapping libaldis_ffi.a (aldis.h)
                                     cFE core services (ES, EVS, SB, TBL, TIME), as shipped
```

| cFS module | Replaces | Role | Aldis use (`aldis.h`) |
|---|---|---|---|
| `refapp_aldis` (cFE library) | — | Wraps the static `libaldis_ffi.a` built from the same commit. The startup script loads it first, so its symbols resolve the apps loaded after it (F4, F17) | — |
| `refapp_step` | SCH_LAB | **Bridge and sequencer.** Holds the harness connection (§10.6); turns each STEP into WAKE messages to the scenario apps in the order of §2.1, one app at a time (§10.5); collects the outputs and the next deadlines; answers STEP_RESULT | None |
| `refapp_ci` | CI_LAB | **Command ingest.** Decodes the delivered link units with the strict decoders and puts each command and table-upload packet on the Software Bus. P1: Space Packets. P2: TC frames, MAP demultiplexing, and segment reassembly, with the FARM-B Counter for the CLCW. P3: FARM-1 through its C ABI handle | `aldis_spp_*`; from P2 the receiving-side engine handles with the CLCW; from P3 FARM-1 |
| `refapp_to` | TO_LAB | **Telemetry output.** Receives the scenario's telemetry and output messages in one pipe. Builds each Space Packet as the message arrives (per-APID sequence count, secondary header of §3.2, on-board time of §5); from P2 also the TM frames of §4.1 (VC packet generation, mux, idle fill, OID frames, frame counts, the CLCW in the OCF); forwards every other output to `refapp_step` in order | `aldis_spp_*`, `aldis_time_*`; from P2 the sending-side engine handles of the M4 link engines (`aldis_tm_*`, the OID generator, CRC-16 behind them) |
| `refapp_cmd` | — | Command handler with the time-tag queue, the table store, and the memory dump (§2.1, §2.3): the applications that share the command path | `aldis_time_*` (execution times), `aldis_spp_segmenter_*` (dump), CRC-16 |
| `refapp_hk` | — | Housekeeping (§3.3.1), from status messages of `refapp_cmd`, `refapp_pl`, and from P3 `refapp_ci` (FARM-1 status) | None |
| `refapp_pl` | — | Payload (§3.3.3) | None |

The event reporter of §2.1 is not an app of its own: each app sends its on-board events as Software Bus messages to `refapp_to`, which makes them `0x011` packets in arrival order. cFE's own event service (EVS) and housekeeping are not part of the scenario and never reach the trace. The module names avoid the `aldis_` and `ALDIS_` prefixes, which belong to the C ABI ([C ABI profile](profiles/c-abi.md) §2).

**Link between the processes.** The flight node's link units cross the node protocol, never a socket of their own. `refapp_to` hands each unit to `refapp_step`, which returns it in STEP_RESULT; the harness applies the link model and the faults and delivers the unit to the Rust ground node, and the ground node's units reach the flight node in a later STEP. The lab apps' UDP ports (F13, F14) are not used.

**Code structure.** Each app keeps its scenario logic and its Aldis calls in C functions that take the WAKE inputs and return outputs, with no cFE, OSAL, or PSP call (a sans-I/O core, like the engines of [03 §5](03-api-model.md#5-engines-the-sans-io-contract)). The cFS glue (main loop, pipes, subscriptions, the socket) is separate, so the cores can be unit-tested on the host without cFS, and an integrator can lift a core into another framework.

### 10.4 Phasing

Each phase starts when the Rust milestone it follows has released its golden traces and the C ABI it needs is merged, and reproduces those traces:

| Phase | Link level | Needs from track R (C ABI) | Needs from track A | Reproduces | Link code in the cFS apps |
|---|---|---|---|---|---|
| P1 Packets | Space Packets, one per unit | The M1 codecs (time, SPP, EPP): M2 WP-0c, merged (#72) | A1 exited | `a1-*` | None |
| P2 Frames | TM down, TC Type-BD up | The M4 link engine handles (the frame engines and multiplexers, with the CLCW, CRC-16, and the OID generator behind them) | A2 exited | `a2-*` | None: the engine handles, as in A2's Rust nodes (plan D10) |
| P3 COP-1 | FARM-1 on board | The FARM-1 handle (M3) | A3 exited | `a3-*` | None: as P2, with FARM-1 |

The AOS and USLP variants of §4.3 (`a4-aos-nominal`, `a4-uslp-nominal`) are not part of any phase; they can be added as cases once their C ABI exists.

### 10.5 Determinism and the golden trace

cFS runs in real time. SCH_LAB's schedule and cFE TIME's 1 Hz both run from OSAL timers on the PSP's timebase `cFS-Master`, which counts wall-clock time (F6, F8, F12), and cFE TIME interpolates between tones with the PSP's monotonic clock (F8). On a Linux host without privileges every task also runs at the same priority (F16), so the order in which apps run within a tick is the Linux scheduler's. A golden trace must be the same on every run (APP-077). The options:

| Option | How | Trace change | Assessment |
|---|---|---|---|
| A. cFS timing from simulated time | Configure cFE TIME as a server with an external tone and MET (F9), let the harness step the tone, and drive an SCH-like scheduler from it | None | cFE TIME checks each tone against the PSP's monotonic clock and rejects one that does not come about 1 s after the last (F10), and interpolates with that clock between tones (F8). The clock comes from a PSP module that a mission cannot replace (F5, F6), so simulated time that runs faster or slower than real time needs a change to the PSP or a clock shim (`LD_PRELOAD`). Even then the order of apps within a tick is the scheduler's (F16). OI-36 |
| B. Real time, normalised comparison | Run cFS as shipped (SCH_LAB, UDP) and compare only packets and frames, with time fields and instants masked | A new comparison mode; exceptions to APP-085 and APP-004 | The CUC time stamps are inside the packet octets (§3.2), so masking reaches into the units themselves. The time-tagged commands, the dump schedule, and every fault case depend on timing. Weaker evidence than the Rust and C++ nodes give |
| **C. Lockstep sequencing** (recommended) | The harness is the clock. `refapp_step` turns each STEP into WAKE messages that carry `now` and wakes one app at a time. Scenario time is `now`, converted by Aldis (APP-060), never cFE TIME. cFE's services run as shipped and stay outside the trace | None to APP-080…APP-088; the node protocol gains a transport (§10.6) | Byte-equal traces for every case, faults included; no change to cFS; cFE TIME untouched. It replaces SCH_LAB, as missions replace the lab apps with their own |
| D. Hybrid: C in CI, B as a demonstration | C for the nightly comparison, plus a free-running build with real-time scheduling, compared by the packet projection (APP-003) with time masked, run by hand | As C | A second build for a demonstration no check covers; left for later (§10.7) |

**Recommendation: C.** It is the only option that keeps byte equality without changing cFS, and the only one under which the fault cases of A2 and A3 can be reproduced at all. Its cost is that cFS's own scheduler does not drive the scenario; the plan records this limit, and OI-36 keeps the question of driving cFE TIME open for later.

How the sequencing keeps the order of §2.1 and of the trace records:

| Rule | Detail |
|---|---|
| One active app | `refapp_step` wakes `refapp_ci` (§2.1 step 1: delivered units), `refapp_cmd` (step 1: delivered commands, then steps 2 and 3), `refapp_hk` (step 4), `refapp_pl` (step 5), and last `refapp_to` with the transmit slot. It wakes the next app only after the previous one's DONE marker has come back |
| One ordered output stream | Every app sends its telemetry, its application events, and then its DONE marker to the one pipe of `refapp_to`, which packetises telemetry as it arrives and forwards everything else, the marker last, to `refapp_step`. A pipe is first in, first out, and only one app produces at a time, so the records reach the trace in the order the apps raise them |
| Time | Every WAKE carries `now`. On-board time is T0 + `now` (§5), converted with the Aldis time functions; no scenario value is read from cFE TIME or from an OSAL or PSP clock |
| Deadlines | Each DONE marker carries the app's next deadline, and `refapp_step` reports the earliest (§8.1). A WAKE at an instant where nothing is due changes nothing (APP-074) |
| No timeouts in the scenario path | Apps pend on their pipes without a timeout; the CI job's time limit bounds a hung run, as in §8.2 |

What the trace format needs: nothing in APP-080…APP-088. There are two consequences elsewhere. (1) The out-of-process node protocol of §8.2 is needed in P1, before A5, and over another transport, so ACFS WP-2 builds it (§10.6, ACFS D18, OI-38). (2) A golden trace's header lists the requirements its case verifies and is shared by every implementation, so the requirements below are traced by the `// req:` annotations of the harness tests and C sources that run the cFS node (APP-125), not by golden-trace headers. How the trace's timing fields relate to cFS time is OI-37.

### 10.6 Node protocol transport

§8.2 carries the node protocol over the child's stdin and stdout. A cFS process cannot use them, because the OSAL console writes to stdout (F15). The cFS node carries the same messages, in the same lockstep, over a TCP stream on the loopback interface, opened through the OSAL socket API (F15) so that the bridge stays portable to the other OSAL targets. The harness listens, starts `core-cpu1` in its staging directory (F3), and accepts one connection, which `refapp_step` opens once cFE start-up has completed (F18). The harness ends the process after END_ACK. The stdin/stdout transport stays as it is for the A5 and A6 nodes.

### 10.7 Non-goals

| Not in the first version | Why |
|---|---|
| RTEMS, VxWorks, or a PSP other than pc-linux | pc-linux runs on the CI host; the other targets need emulators or licences and add nothing to the trace |
| A port of cFS to Rust, or Rust code in the cFS apps | The point is the C ABI, as integrators use it |
| Changes to cFE, OSAL, PSP, or the lab apps | Mission configuration (defs directory, configuration headers, startup script, tables) is not a change; anything that needs a patch is an open issue (OI-36) |
| cFE TIME driven by simulated time | OI-36 |
| A free-running real-time demonstration with SCH_LAB and UDP | Option D of §10.5; later |
| A ground side in cFS or in another ground system | The ground node is the Rust node of §2.2 |

### 10.8 Requirements

**T** is a harness test annotated `// req:` that runs the cFS node in the nightly job and compares its trace (APP-123); **A** is a check; **I** is review.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-110 | For every case of the link levels its phase covers (§10.4), the cFS flight node SHALL reproduce byte for byte the golden trace that the Rust flight node reproduces, with the Rust ground node and the harness unchanged. | GEN-030 | T |
| APP-111 | The cFS flight node SHALL run on the cFS release (cFE, OSAL, PSP) that its plan pins, built for the pc-linux PSP with `SIMULATION=native`, without changes to the cFE, OSAL, PSP, or lab-app sources or build files. Mission configuration (defs directory, configuration headers, startup script, tables) is not a change. cFS sources SHALL NOT be committed to this repository. | — | A/I |
| APP-112 | The cFS apps SHALL call Aldis only through the C header, linked from the static library built from the same commit (CABI-001) through one cFE library module, and SHALL NOT use other code of `refapp/`, another Aldis crate, or anything the header does not declare. | CABI-001, CABI-002 | A/I |
| APP-113 | The telemetry-output app SHALL build every downlink Space Packet with the Aldis SPP and time functions, keeping the per-APID sequence counts of §3.1 and the secondary headers of §3.2, and from phase P2 every TM frame of §4.1 with the Aldis TM functions. It SHALL be the only source of downlink link units. | APP-020…APP-023, APP-030…APP-035 | T |
| APP-114 | The command-ingest app SHALL decode every delivered link unit with the Aldis strict decoders (API-030): Space Packets in P1, TC frames with MAP demultiplexing and segment reassembly from P2, and FARM-1 through its C ABI handle from P3. It SHALL put each accepted command and table-upload packet on the Software Bus and supply the CLCW to the telemetry-output app. | APP-024, APP-041…APP-044 | T |
| APP-115 | The flight applications of §2.1 SHALL be cFS apps that exchange Software Bus messages only, and SHALL take scenario time only from the `now` of their WAKE message, never from cFE TIME or an OSAL or PSP clock. | APP-060 | T/I |
| APP-116 | The bridge SHALL answer each STEP by waking the apps one at a time in the order of §10.5, each after the previous app's DONE marker has passed through the output stream, and SHALL report the outputs in the order they arrive and the earliest next deadline the apps report. | APP-010, APP-074, APP-090 | T |
| APP-117 | No trace record of a cFS run SHALL depend on wall-clock time, task priorities, or operating-system scheduling. The nightly job SHALL run every case twice and fail if the two traces differ. | GEN-031, APP-077 | T |
| APP-118 | The cFS node SHALL carry the node protocol of §8.2, unchanged in its messages and lockstep, over a TCP stream on the loopback interface opened through the OSAL socket API, with the harness listening. The harness SHALL produce the same trace through either transport (APP-092). | APP-092 | T |
| APP-119 | Each phase of §10.4 SHALL start only when the Rust milestone it follows has exited and the C ABI it needs is merged, and SHALL end when that milestone's cases are reproduced (APP-110). | — | I |
| APP-120 | The cFS apps SHALL take every scenario value from a C header generated from the scenario file (APP-107), and CI SHALL check that the header is current. | APP-107 | A |
| APP-121 | Each cFS app SHALL keep its scenario logic and its Aldis calls in functions that make no cFE, OSAL, or PSP call, separate from the cFS glue, and those functions SHALL be unit-tested on the host without cFS. | GEN-030 | I/T |
| APP-122 | The C sources in `refapp/cfs/` SHALL be C99, as the cFS build compiles them (F3), and SHALL follow the SEI CERT C Coding Standard, checked with a free analyser (VER-051). | VER-051, VER-055 | A |
| APP-123 | The cFS build and the trace reproduction SHALL run in the nightly workflow and on manual dispatch only, never as a gating check of the verification run, and a failure SHALL open an issue (VER-059). | VER-059 | A |
| APP-124 | Every cFS component used SHALL be under a licence that VER-055 allows, recorded with its tag and commit in the plan. | VER-055 | I |
| APP-125 | The tests and C sources of the cFS node SHALL carry `// req:` annotations, and `cargo xtask trace` SHALL read them. | VER-020 | A |

Not applicable to the cFS node, which is C code inside a framework with tasks and global state (plan D16): APP-091 (APP-117 and APP-121 take its place), the rule on threads in APP-077 (APP-116, APP-117), the Cargo rules of APP-100 (APP-112), and APP-102 to APP-106, which are rules for the Rust crates.

## 11. Interoperability profile (AINT)

**Plan:** [AINT Interoperability](milestones/aint-interoperability.md) (decisions D1–D9, accepted 2026-11-11). The scenario of §2 to §6 is run against an independent ground system, used as a black box, over a transport driver outside the crates. This section is written from the repository's own specification before any driver exists; it fixes what a person needs to configure the other system and to judge a run, and it changes no node, no case, and no golden trace (APP-002, APP-004, APP-005). The loss script's drop indexes are not here yet (§11.6).

### 11.1 The ground system

| Item | Value |
|---|---|
| System | YAMCS (mission control system), run as a black box (plan D1, D2) |
| Release | **5.14.0** (public release page https://github.com/yamcs/yamcs/releases, read 2026-11-11: marked "Latest", released 31 October) |
| Licence | AGPL-3.0, read from the repository's public README ("Affero GPLv3"). The pages read do not say whether it is -only or -or-later, or whether it has exceptions: unconfirmed. It runs as a separate process and is neither modified, copied into the repository, shipped in an artefact, nor linked (APP-155) |
| Fetch and build | The public release page shows no official container image. The project **builds its own image from the pinned release** (the release's published distribution, not its source tree), in a container recipe this project writes; the recipe is fetched-input only and the image is never committed. The recipe and the build command are written by WP-3 (first run) and fixed by WP-5 |
| Image digest | **Not yet existing.** Filled in by WP-3 (first image) and confirmed by WP-5 (the exit record); never invented here |
| Configuration files | Written by this project and kept under `refapp/interop/yamcs/` (instance, TM and TC frame links, mission database of the scenario's packets and commands, ops script as client calls), by WP-3 (links) and WP-4 (database, ops script). Nothing else of the system is in the repository |
| Documentation read | https://docs.yamcs.org/yamcs-server-manual/links/ccsds-frame-processing/ (public). No source code is read (plan D2 c) |

What the public documentation shows of the frame links, and what it does not (read 2026-11-11):

| Documented option | Meaning | Profile value |
|---|---|---|
| `UdpTmFrameLink` with `errorDetection` CRC16, codec NONE | TM frames on UDP, FECF check, no Reed-Solomon | 223 octets, FECF present (§4.1) |
| `UdpTcFrameLink`, `cltuEncoding` absent | TC frames on UDP without a CLTU | APP-006 |
| `useCop1`, `clcwStream`, `initialClcwWait` | COP-1 on; stream the CLCW is read from; seconds to wait for the first CLCW | on |
| `slidingWindowWidth` | FOP sliding window width K; default 10 | 4 |
| `cop1T1` | T1 in whole seconds; default 3 | 1 (1 000 000 µs) |
| `cop1TxLimit` | "number of retransmissions for each un-acknowledged frame", after which operations are suspended | see §11.2 |
| command attribute `cop1Bypass` | sends a command as a Type-BD frame | not used by `a3-nominal` or the lossy-link case |

Not shown on the pages read: a timeout-type option, the *Initiate AD Service* variant, the attached sync marker of TM frames, the Segment Header and MAP IDs of TC frames, and whether the frame link takes a Control Word Type or CLCW Version other than the ones the profile fixes. Each is a setting WP-3 or WP-4 finds out by running, from the frames on the wire and the system's own log and API, not from its source (plan D2 c); a difference from the profile is a discrepancy, classified as in plan D2 (d) (APP-166).

### 11.2 The open point: Timeout_Type

232.1 §5.1.10.3 gives two Timeout_Type values and Table 7-1 lets a project choose; the profile fixes 0, Alert when the limit is reached (§4.2). The public documentation of YAMCS names no timeout-type option, and says only that after `cop1TxLimit` retransmissions "operations are suspended". That sentence resembles the Suspend behaviour of 232.1 (§5.1.10.3, suspend the service with Suspend_State, instead of raising Alert[Limit]) rather than the Alert, but the page does not name it, does not say how operations resume, and gives no default for `cop1TxLimit`.

**Settled from the public documentation: nothing more.** What follows is what the run cannot check from the documentation and must observe:

1. Whether YAMCS's limit is reached after Transmission_Limit = 3 transmissions in all (the first and two retransmissions, 232.1 §5.1.10.2) or after `cop1TxLimit` retransmissions beyond the first. The profile sets `cop1TxLimit` so that no case reaches it (the lossy-link case needs at most one retransmission of a frame, §6.8); the value is chosen in WP-3 from what the run shows.
2. Which timeout type YAMCS then applies. No case of this milestone reaches the limit (APP-165), so the criteria of §11.5 do not depend on it; the cases `a3-uplink-limit` and `a3-uplink-lockout`, which do, are not run against YAMCS (§11.5).

Neither is an open issue about a Blue Book reading (232.1 settles both values); the first run's observation of a difference from the profile's Timeout_Type 0 is a discrepancy of the other system's configuration (plan D2 d, §6 of the plan).

### 11.3 Managed parameters, as a configuration table

Every value is the one of the cited clause of this document; where the ground system has a documented option for it, §11.1 names it. A person sets up a ground system from this table and the Blue Book, without the Rust sources.

| Parameter | Value | Fixed by |
|---|---|---|
| Spacecraft ID | `0x155` (341), fictional (OI-26); TM, TC, same for both directions | §3.4, APP-026 |
| TM Transfer Frame Version Number; length | 1 (`00`); 223 octets | §4.1 |
| TM VCs | 0 (HK `0x010`, events `0x011`), 1 (payload `0x020`, dump `0x021`), 7 (OID frames only) | §4.1, §3.1 |
| TM frame layout | primary header 6, data field 211, OCF 4, FECF 2; no FSH; Packet data field content | §4.1 |
| TM FECF | present (CRC-16) | §4.1 |
| OCF in TM frames | present in every frame, including OID frames: the CLCW of the uplink VC, CLCW Version 1, COP in Effect 1 (bits 6–7 `01`, word `0x01000000` + flags + N(R)); reported 8 per second | §4.1, §4.2, §4.5.3 |
| TC VC | one VC, VC 0; Type-AD, Type-BD, and Type-BC frames | §4.2 |
| TC frame length | at most 128 octets; AD frames of the scenario carry first octets `01 55` | §4.2, §4.5.4 |
| TC frame layout | primary header 5, Segment Header 1, user data at most 120, FECF 2 | §4.2 |
| TC FECF | present | §4.2 |
| Segment Header; MAP IDs | present; MAP 0 commands (blocking, no segmentation, packet `0x080`), MAP 1 table upload (segmentation, no blocking, packet `0x081`; three segments of 120, 120, 69) | §4.2, §3.1 |
| Coding, CLTU, synchronisation marker | none; one whole frame per UDP datagram | APP-006, APP-159 |
| FOP-1 sliding window K | 4 | §4.2 |
| FARM-1 window W; PW; NW | 8; 4; 4 | §4.2, §4.5.3 |
| T1_Initial | 1 000 000 µs (at least the 395 000 µs of §4.2); YAMCS takes whole seconds: 1 | §4.2, §11.1 |
| Transmission_Limit | 3 | §4.2, §4.5.4 |
| Timeout_Type | 0 (Alert); see §11.2 | §4.2, §11.2 |
| FOP-1 start variant | *Initiate AD Service (without CLCW check)*, directive code 1, at 0.0 s, in `a3-nominal` and `a3-uplink-loss`; the other variants (with CLCW check, with Unlock, with Set V(R)) are not used | §4.5.2, §6.8 |
| Downlink slot; uplink slot | 125 000 µs; 250 000 µs, one frame per slot, so at most one frame per 250 000 µs from the ground system and exactly one per 125 000 µs from the flight node | §6.1, APP-071 |
| Time code of the packets | CUC Level 1, 4 coarse + 2 fractional octets, T0 = 2 100 000 000 s | §5 |

The packets and commands of the ops script are those of §2.4 and §3.3 and are described to the ground system by its mission database (plan D7).

### 11.4 What the driver does

The transport driver is a program outside the crates, in the library and the binary of `refapp-interop` (plan D4). It is the only new I/O of the milestone and it steps the nodes through the contract of §8.1; the nodes do not change. A datagram is one transfer frame. The driver's own tests run in virtual time against the Rust ground node over loopback (plan D8 a, APP-106).

### 11.5 Pass criteria

A pass is a set of observed facts, not a byte comparison (plan D6): the golden traces hold the harness's simulated time and the schedule of Aldis's own FOP-1, which another FOP-1 on a real clock does not reproduce, and APP-004 is a property of the Aldis implementations. A criterion that fails is a discrepancy (APP-166), not a failed milestone. Cases that reach the Transmission_Limit or the Lockout of FARM-1 (`a3-uplink-limit`, `a3-uplink-lockout`, `a3-uplink-disorder`) are not run against the ground system.

### 11.6 The loss script

The loss script of the lossy-link case drops uplink and downlink frames by (direction, index), as §6.5 does, where the index counts the units the driver sees in order in that direction. The cells of FARM-1 it must reach are those of `a3-uplink-loss` (§6.8): E3 (a frame behind a lost one is discarded and sets Retransmit), E1 again on the retransmitted frames and Retransmit reset, E4 (a retransmitted frame already accepted, because its acknowledging CLCWs were lost, is discarded as a duplicate), and a lone lost frame with no frame behind it, which only T1 retransmits.

**TODO (WP-1b, after WP-3):** the list of drop indexes. They depend on the order in which the ground system sends its frames, which only the first run (WP-3) shows; the list is added to this section in WP-1b, together with a statement of every cell that the other system's schedule makes unreachable and why (plan D6).

### 11.7 Requirements

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| APP-155 | The independent ground system SHALL be YAMCS release 5.14.0, run as a separate process from an image built by the project from the pinned release. Nothing of it SHALL be copied into the repository, shipped in an artefact, linked, or modified; the repository SHALL hold only the configuration files of §11.1; no source code of it SHALL be read. A change of the pin repeats the licence check of plan D1 (a). | VER-055 | I |
| APP-156 | The ground system SHALL be configured with the managed parameters of §11.3, and every parameter it has no option for, or that it takes in another form (T1 in whole seconds), SHALL be recorded in the run record with the setting used. | — | I |
| APP-157 | The driver SHALL step the flight node per §8.1: `now` is the microseconds since the start of the run from its clock, the `unit` outputs are sent as datagrams to the ground system's TM port, the datagrams received on the TC port are `delivered`, and the next step is at the earliest of the node's `next_deadline`, the next downlink slot, and an arriving datagram. | APP-090 | T |
| APP-158 | The driver SHALL set `tx_slot` in the step at every 125 000 µs of the downlink slot and in no other step. | LNK-030 | T |
| APP-159 | The driver SHALL carry one transfer frame per datagram, in each direction, SHALL add no synchronisation marker, coding, or CLTU, and SHALL NOT reorder, merge, or split datagrams. | APP-006 | T |
| APP-160 | The driver SHALL read time only through a clock trait, with a monotonic implementation for the binary and a manual one for its tests, which SHALL run in virtual time. | GEN-030 | T |
| APP-161 | The driver SHALL change no node, no engine, no scenario value, and no golden trace; it SHALL use the nodes only through the contract of §8.1 and the Aldis facade (APP-100). | APP-090, APP-100 | I |
| APP-162 | The driver SHALL write a run log in JSON Lines with a header line (the ground system's version and image digest, the commit of the node, the case, the loss script) and one record per datagram (direction, monotonic instant, octets in hexadecimal), per node event (the events of §7.4), and per `fault` of the loss script. The run log is not a golden trace and SHALL NOT be committed, except the run kept as evidence by the exit. | — | T |
| APP-163 | The driver SHALL have a loss script: a list of `drop` entries by (direction, index) as §6.5, applied before a unit reaches its receiver and recorded in the run log. A script that reaches the FARM-1 cells of §11.6 SHALL exist for the lossy-link case, with the indexes of §11.6. | APP-139 | T |
| APP-164 | A run of `a3-nominal` against the ground system SHALL pass when: the flight node accepts every command and table segment of the ops script, in order and exactly once (its `packet_in` projection and the projection of the commands equal those of `a3-nominal`, APP-003); every CLCW advances N(R) with no Lockout and no Retransmit flag; the ground system reports every command as acknowledged; and it raises no alert. | APP-003, APP-150 | I |
| APP-165 | A run of the lossy-link case against the ground system SHALL pass when: the flight node's events show the discard of an out-of-sequence frame (E3) and the Retransmit flag; the ground system retransmits the lost frames, including a lone lost frame retransmitted only by its timer T1, and a frame whose CLCW was lost; at the end the projection of the commands equals that of `a3-nominal`; and no case reaches the Transmission_Limit, Lockout, or an alert. | APP-003, APP-151 | I |
| APP-166 | A failed criterion, and every difference between what the ground system does and what §11.3 or the Blue Books say, SHALL be classified from the wire (the run log), the Blue Books, the public documentation, and the system's own log and API: a defect of an Aldis node (a track-R fix with a vector), a reading the Blue Book does not settle (an open issue of [09](09-open-issues.md)), or a defect or deviation of the other system (an issue here, reported upstream by the maintainer only). It SHALL be filed and listed in §6 of the plan; no run SHALL be reported as passing with an unfiled failure. | — | I |
