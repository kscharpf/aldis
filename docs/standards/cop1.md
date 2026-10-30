# COP-1 (FOP-1 and FARM-1) — CCSDS 232.1-B-2 (+ Cor. 1)

**Crate:** `aldis-cop1` (Layer 2 engine) · **Reference:** `specs/pdf/232x1b2e2c1.pdf` ([how to download](../../specs/README.md)) · **Acronyms:** [glossary](../glossary.md)

COP-1 provides the Sequence-Controlled (AD) and Expedited (BD) services of TC and of USLP. **FOP-1** runs at the sending end, typically the ground. **FARM-1** runs at the receiving end, typically the spacecraft. One instance of each runs per Virtual Channel.

## 1. Normative basis

The FOP-1 State Table (232.1 Table 5-1: states S1–S6 × events E1–E46, E101–E104) and FARM-1 State Table (Table 6-1: states S1–S3 × events E1–E11) are normative. The cell-by-cell review record against CCSDS 232.1-B-2 and Cor. 1 is published in [cop1-tables.md](../verification/reviews/cop1-tables.md) (COP-003, VER-041).

FOP-1 protocol states (232.1 §5.1.2): S1 (`Active`), S2 (`Retransmit without wait`), S3 (`Retransmit with wait`), S4 (`Initializing without BC frame`), S5 (`Initializing with BC frame`), S6 (`Initial`). Suspension is tracked by the state variable `Suspend_State` ($SS \in \{0..4\}$) while the engine resides in S6 (§5.1.11).

FARM-1 protocol states (232.1 §6.2): S1 (`Open`), S2 (`Wait`), S3 (`Lockout`).

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| COP-001 | FOP-1 SHALL implement **every cell** of Table 5-1: states S1–S6, events E1–E46 **and E101–E104** (added in Rev. B), with the exact actions and next states, including the Cor. 1 changes. | 232.1 §5.3, Table 5-1; Cor. 1 | V |
| COP-002 | FARM-1 SHALL implement every cell of Table 6-1: states S1–S3, events E1–E11. | 232.1 §6.3, Table 6-1 | V |
| COP-003 | Each state table SHALL be represented in the implementation as **data** (a table of state × event → actions + next state), not as scattered conditionals. The table SHALL be reviewed cell by cell against the Blue Book (inspection record kept). | — | I |
| COP-004 | Every engine transition SHALL be attributable to an event number. Status snapshots and alert events SHALL include the event number and prior state that caused them. | 232.1 §4.3 note (Alert) | T |
| COP-006 | Each engine SHALL implement the step contract of [03 §5](../03-api-model.md#5-engines-the-sans-io-contract) (API-057, API-058): `Fop1::step` and `Farm1::step` map (state, event, `now`) to the next state and the action list of the table cell, in the order the cell lists them. Their action bounds `N` SHALL be derived from the table data (the longest action list of any cell, with a Transmit action producing one frame), not written by hand, and recorded in the M3 plan. The pure step function (or `decide`) is what the cell-by-cell check of COP-003 and the Kani targets address. | 232.1 Tables 5-1, 6-1; API-057 | T/V |
| COP-005 | The conformance suite SHALL contain at least one vector per non-"Not applicable" cell of each table, and one per "Not applicable" cell asserting that the engine treats it as `InvalidState` (unreachable by construction). | — | V |

## 2. FOP-1 engine

### 2.1 Configuration (managed parameters, §7.2)

`Fop1Config { vc: FrameTarget (TC VC config | USLP VC+MAP config), t1_initial: Duration, transmission_limit: u8 ≥ 1, timeout_type: TT0 | TT1, sliding_window_k: 1..=255 (≤ PW of the peer FARM), bc_repetitions, ad_repetitions }`

Capacities (compile-time): `SENT_QUEUE ≥ K` frames; `MAX_FRAME ≤ 1024` (TC) or the configured USLP max.

### 2.2 Inputs

The inputs are the variants of `Fop1Input`; every one is fed to `step(state, input, now, out)` (API-057) and returns the next state and the actions of §2.3.

| Input | Maps to events | Notes |
|---|---|---|
| `RequestAd(fdu)` | E19, E20 | Wait_Queue holds at most one FDU (§5.1.4). The request handle is returned in the accept or reject notification action |
| `RequestBd(fdu)` | E21, E22 | |
| `Directive(directive)` | E23–E40 | Initiate AD (without CLCW check / with CLCW check / with Unlock / with Set V(R)), Terminate, Resume, Set V(S), Set K, Set T1_Initial, Set Transmission_Limit, Set Timeout_Type |
| `Clcw(clcw)` | E1–E15, E101–E103 | The engine classifies the CLCW into the event. A CLCW whose bit pattern is invalid under COP-1 (e.g. COP in Effect ≠ `01`, or a non-zero Control Word Type) is E15. A CLCW for a different VCID SHALL be rejected with `UnknownChannel` without any state change, because it is a routing error, not a COP-1 event |
| `Timeout` | E16–E18, E104 | Single timer T1. The engine classifies it with the state of the timer; a `Timeout` before the deadline changes nothing (API-053) |
| `TransmitDone(handle, Accept\|Reject)` | E41–E46 | Lower-Procedure response for AD/BC/BD frames (Out flags) |

### 2.3 Actions

The actions of one input, in the order of the table cell (COP-006). The frame octets are written to the output area (API-061).

| Action | Content |
|---|---|
| `Transmit` | complete frame octets (range of the output area), kind ∈ {AD, BC, BD}, N(S), retransmission flag, repetitions value, transmit handle |
| `Notify(TransferNotify)` | request handle + {Accept, Reject, PositiveConfirm, NegativeConfirm} (§4.4, Table 4-5) |
| `Notify(DirectiveNotify)` | directive handle + {Accept, Reject, PositiveConfirm, NegativeConfirm} (§4.2, Table 4-2) |
| `Notify(Alert)` | reason ∈ {Limit, T1, Lockout, Synch, NnR, Clcw, Llif, Term} + event number + prior state (§4.3, Table 4-4) |
| `Notify(Suspend)` | (§4.3) |

### 2.4 Requirements

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| COP-010 | FOP-1 SHALL build Type-AD frames with N(S) = V(S), Type-BD frames with N(S) = 0, and Type-BC frames carrying Unlock or Set V(R) (TC-005). It SHALL use the TC or USLP codec selected by the frame target. | 232.0 §4.1.2.8 note 3; 232.1 §5.2 | V |
| COP-011 | FOP-1 SHALL keep master copies of all unacknowledged AD frames (Sent_Queue) and SHALL retransmit them in order. On retransmission the frames SHALL be byte-identical to the original transmission. | 232.1 §5.1.7 | V |
| COP-012 | FOP-1 SHALL compare N(R) arithmetic modulo 256 using NN(R) and V(S) exactly as the "valid N(R)" conditions in Table 5-1 define. | 232.1 §5.1.8, Table 5-1 | V |
| COP-013 | The T1 timer SHALL be started or restarted on every AD or BC transmission (including retransmissions) and cancelled when the Sent_Queue empties. The timer is state, and `next_deadline` SHALL report its deadline (API-053). BD frames SHALL NOT start the timer. | 232.1 §5.1.9.1 | V |
| COP-014 | Suspend/Resume SHALL record and restore states via Suspend_State (SS = 0–4), as specified for E18/E104 and E30–E34. | 232.1 §5.1.11 | V |
| COP-015 | The Set K directive SHALL be rejected (Reject response) if K ∉ 1..=min(255, SENT_QUEUE capacity). | 232.1 §5.1.12 | T |
| COP-016 | Systematic retransmission (repetitions) is **outside** FOP-1. The engine SHALL only annotate transmit actions with the configured repetition value. It SHALL NOT duplicate frames itself. | 232.1 §5.1.9.3 note; 232.0 §2.4.2 | T |
| COP-017 | After an Alert, no further Alert other than Llif SHALL be generated until an Initiate AD directive is accepted. | 232.1 §4.3 | V |

## 3. FARM-1 engine

### 3.1 Configuration (managed parameters, §7.3)

`Farm1Config { vc_id, status: ClcwStatus, window: Farm1Window }`, where `Farm1Window` is built by `retransmission_allowed(W)` (PW = NW = W/2, W even 2..=254) or `retransmission_not_allowed(W, PW)` (1 ≤ PW ≤ W ≤ 256, NW = W − PW ≤ 127, Table 7-2), so an invalid pair cannot exist (API-040, RS-020). `Farm1::new(config, v_r)` starts in S1 with the flags and the FARM-B Counter at 0 (OI-58).

The engine holds no delivery buffer: the buffers of 232.1 §3.3.2 belong to the caller, which tells FARM-1 whether a buffer is available (COP-033). The `Internal{depth}` buffer model of earlier drafts is withdrawn with COP-032.

### 3.2 Inputs and actions

| Input | Events |
|---|---|
| `Frame(ValidatedTcFrame \| ValidatedUslpFrame, buffer_available: bool)` | E1–E9 (frames must already have passed Frame Validation Checks, TC-021). `buffer_available` selects E1 or E2 for a Type-AD frame with N(S) = V(R) (COP-033). E1 in S2 is the cell Table 6-1 marks "Not applicable": the step fails with `InvalidState` (COP-005), and the caller feeds `BufferReleased` first |
| `InvalidFrame` | E9: a frame the Lower Procedures rejected. A Type-BC frame carrying neither Unlock nor Set V(R) is `Frame` with no command and is E9 too (COP-031) |
| `BufferReleased` | E10 (COP-033) |
| `clcw(state) -> Clcw` | E11 (a pure query of the state, any time; not an input) |
| `SetPhysicalFlags(no_rf_available, no_bit_lock)` | sets CLCW flags (not part of COP) |

| Action | Content |
|---|---|
| `Deliver` | accepted FDU (AD or BD; range of the output area from offset 0), frame type, N(S) (0 for BD) |
| `Notify(FrameDiscarded)` | event number + state before: with Table 6-1 they say why (E2 no buffer, E3 gap, E4 duplicate, E5 outside the window, any frame in S3 Lockout; COP-004) |
| `Notify(StateChanged)` | event number, old → new state |
| `Notify(Unlocked)` | an Unlock command was executed (E7; COP-036) |

### 3.3 Requirements

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| COP-030 | FARM-1 SHALL classify each Type-AD frame's N(S) relative to V(R) into the E1/E2 (N(S) = V(R)), E3 (positive window), E4 (negative window), or E5 (lockout area) cases, using modulo-256 arithmetic and the configured PW/NW. | 232.1 §6.1.8.3 | V |
| COP-031 | Type-BC frames whose data field is not a valid Unlock or Set V(R) command SHALL be treated as invalid frames (E9). | 232.1 §6.3.2.1 | V |
| COP-032 | Withdrawn: the `Internal{depth}` buffer model put a delivery queue inside FARM-1, which API-059 excludes. The delivery buffer is the caller's (COP-033; OI-56). | — | — |
| COP-033 | "Buffer available" is an **input**. The frame input SHALL carry a buffer-available flag, which the caller sets from its own delivery buffer: true when there is a buffer for the FDU of an accepted Type-AD frame, and false otherwise, which selects E2 instead of E1 (Discard, Retransmit_Flag := 1, Wait_Flag := 1). When Wait_Flag is set, the caller SHALL feed the buffer-released input (E10) once buffer space is available for at least one more maximum-size frame, which clears Wait_Flag. A Type-BD frame is accepted whatever the buffer-available flag says, and an FDU erased in its favour is the caller's to report (the optional Aborted Indication). A caller without flow control SHALL always set the buffer-available flag; FARM-1 then never enters Wait. The engine SHALL report Wait_Flag in its CLCW and status snapshot. | 232.1 §3.3.2, §6.3.2.3, Table 6-1 E1, E2, E10 | T |
| COP-034 | The FARM-B Counter SHALL increment modulo 4 for every accepted Type-B frame. | 232.0 §4.2.1.9 | V |
| COP-035 | The CLCW built from the state SHALL reflect the state after the most recent input. The CLCW SHALL carry COP in Effect = `01`, the configured VCID and Status Field, and the current flags. | 232.0 §4.2.1; 232.1 E11 | V |
| COP-036 | The Unlock command SHALL reset only FARM-1. The engine SHALL return an unlocked notification action so that higher procedures can purge their buffers. | 232.1 §6.3.2.2 note | T |

## 4. Interaction and closed-loop testing

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| COP-040 | The test suite SHALL include closed-loop scenarios connecting a FOP-1 and a FARM-1 through a simulated link with configurable loss, duplication, reordering, delay, and CLCW sampling rate. It SHALL assert in-order, complete, duplicate-free delivery, or an Alert, in every scenario. | 232.1 §1 (service guarantee) | V/F |
| COP-041 | A property test SHALL drive FOP-1 and FARM-1 with random event sequences and assert that no state outside the tables is reachable and that GEN-020 holds. | — | F |
