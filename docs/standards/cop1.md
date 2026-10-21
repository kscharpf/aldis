# COP-1 (FOP-1 and FARM-1) — CCSDS 232.1-B-2 (+ Cor. 1)

**Crate:** `aldis-cop1` (Layer 2 engine) · **Reference:** `specs/pdf/232x1b2e2c1.pdf` ([how to download](../../specs/README.md)) · **Acronyms:** [glossary](../glossary.md)

COP-1 provides the Sequence-Controlled (AD) and Expedited (BD) services of TC and of USLP. **FOP-1** runs at the sending end, typically the ground. **FARM-1** runs at the receiving end, typically the spacecraft. One instance of each runs per Virtual Channel.

## 1. Normative basis

The FOP-1 State Table (232.1 Table 5-1) and FARM-1 State Table (Table 6-1) are normative. This document does not restate their cells. It defines how the engines expose them.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| COP-001 | FOP-1 SHALL implement **every cell** of Table 5-1: states S1–S6, events E1–E46 **and E101–E104** (added in Rev. B), with the exact actions and next states, including the Cor. 1 changes. | 232.1 §5.3, Table 5-1; Cor. 1 | V |
| COP-002 | FARM-1 SHALL implement every cell of Table 6-1: states S1–S3, events E1–E11. | 232.1 §6.3, Table 6-1 | V |
| COP-003 | Each state table SHALL be represented in the implementation as **data** (a table of state × event → actions + next state), not as scattered conditionals. The table SHALL be reviewed cell by cell against the Blue Book (inspection record kept). | — | I |
| COP-004 | Every engine transition SHALL be attributable to an event number. Status snapshots and alert events SHALL include the event number and prior state that caused them. | 232.1 §4.3 note (Alert) | T |
| COP-005 | The conformance suite SHALL contain at least one vector per non-"Not applicable" cell of each table, and one per "Not applicable" cell asserting that the engine treats it as `InvalidState` (unreachable by construction). | — | V |

## 2. FOP-1 engine

### 2.1 Configuration (managed parameters, §7.2)

`Fop1Config { vc: FrameTarget (TC VC config | USLP VC+MAP config), t1_initial: Duration, transmission_limit: u8 ≥ 1, timeout_type: TT0 | TT1, sliding_window_k: 1..=255 (≤ PW of the peer FARM), bc_repetitions, ad_repetitions }`

Capacities (compile-time): `SENT_QUEUE ≥ K` frames; `MAX_FRAME ≤ 1024` (TC) or the configured USLP max.

### 2.2 Inputs

| Input | Maps to events | Notes |
|---|---|---|
| `request_ad(fdu) -> RequestHandle` | E19, E20 | Wait_Queue holds at most one FDU (§5.1.4) |
| `request_bd(fdu) -> RequestHandle` | E21, E22 | |
| `directive(Directive) -> DirectiveHandle` | E23–E40 | Initiate AD (without CLCW check / with CLCW check / with Unlock / with Set V(R)), Terminate, Resume, Set V(S), Set K, Set T1_Initial, Set Transmission_Limit, Set Timeout_Type |
| `handle_clcw(now, Clcw)` | E1–E15, E101–E103 | The engine classifies the CLCW into the event. A CLCW whose bit pattern is invalid under COP-1 (e.g. COP in Effect ≠ `01`, or a non-zero Control Word Type) is E15. A CLCW for a different VCID SHALL be rejected with `UnknownChannel` without any state change, because it is a routing error, not a COP-1 event |
| `handle_timeout(now)` | E16–E18, E104 | Single timer T1 |
| `transmit_done(handle, Accept\|Reject)` | E41–E46 | Lower-Procedure response for AD/BC/BD frames (Out flags) |

### 2.3 Outputs

| Output | Content |
|---|---|
| `Transmit` | complete frame octets, kind ∈ {AD, BC, BD}, N(S), retransmission flag, repetitions value, transmit handle |
| `Event::TransferNotify` | request handle + {Accept, Reject, PositiveConfirm, NegativeConfirm} (§4.4, Table 4-5) |
| `Event::DirectiveNotify` | directive handle + {Accept, Reject, PositiveConfirm, NegativeConfirm} (§4.2, Table 4-2) |
| `Event::Alert` | reason ∈ {Limit, T1, Lockout, Synch, NnR, Clcw, Llif, Term} + event number + prior state (§4.3, Table 4-4) |
| `Event::Suspend` | (§4.3) |

### 2.4 Requirements

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| COP-010 | FOP-1 SHALL build Type-AD frames with N(S) = V(S), Type-BD frames with N(S) = 0, and Type-BC frames carrying Unlock or Set V(R) (TC-005). It SHALL use the TC or USLP codec selected by `FrameTarget`. | 232.0 §4.1.2.8 note 3; 232.1 §5.2 | V |
| COP-011 | FOP-1 SHALL keep master copies of all unacknowledged AD frames (Sent_Queue) and SHALL retransmit them in order. On retransmission the frames SHALL be byte-identical to the original transmission. | 232.1 §5.1.7 | V |
| COP-012 | FOP-1 SHALL compare N(R) arithmetic modulo 256 using NN(R) and V(S) exactly as the "valid N(R)" conditions in Table 5-1 define. | 232.1 §5.1.8, Table 5-1 | V |
| COP-013 | The T1 timer SHALL be started or restarted on every AD or BC transmission (including retransmissions) and cancelled when the Sent_Queue empties. `poll_timeout` SHALL report its deadline. BD frames SHALL NOT start the timer. | 232.1 §5.1.9.1 | V |
| COP-014 | Suspend/Resume SHALL record and restore states via Suspend_State (SS = 0–4), as specified for E18/E104 and E30–E34. | 232.1 §5.1.11 | V |
| COP-015 | The Set K directive SHALL be rejected (Reject response) if K ∉ 1..=min(255, SENT_QUEUE capacity). | 232.1 §5.1.12 | T |
| COP-016 | Systematic retransmission (repetitions) is **outside** FOP-1. The engine SHALL only annotate `Transmit` outputs with the configured repetition value. It SHALL NOT duplicate frames itself. | 232.1 §5.1.9.3 note; 232.0 §2.4.2 | T |
| COP-017 | After an Alert, no further Alert other than `Llif` SHALL be generated until an Initiate AD directive is accepted. | 232.1 §4.3 | V |

## 3. FARM-1 engine

### 3.1 Configuration (managed parameters, §7.3)

`Farm1Config { vcid, window_w, window_mode: RetransmissionAllowed (PW = NW = W/2, W even 2..=254) | RetransmissionNotAllowed (1 ≤ PW ≤ W ≤ 256, NW = W − PW), clcw_status_field: u3, buffer_model: Internal{depth} | External }`

### 3.2 Inputs and outputs

| Input | Events |
|---|---|
| `handle_frame(now, ValidatedTcFrame \| ValidatedUslpFrame)` | E1–E9 (frames must already have passed Frame Validation Checks, TC-021) |
| `buffer_released()` (External model only) | E10 |
| `clcw() -> Clcw` | E11 (pure read, any time) |
| `set_physical_flags(no_rf_available, no_bit_lock)` | sets CLCW flags (not part of COP) |

| Output | Content |
|---|---|
| `Deliver` | accepted FDU (AD or BD), frame type, N(S) |
| `Event::FrameDiscarded` | event number + reason |
| `Event::StateChanged` | old → new state |

### 3.3 Requirements

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| COP-030 | FARM-1 SHALL classify each Type-AD frame's N(S) relative to V(R) into the E1/E2 (N(S) = V(R)), E3 (positive window), E4 (negative window), or E5 (lockout area) cases, using modulo-256 arithmetic and the configured PW/NW. | 232.1 §6.1.8.3 | V |
| COP-031 | Type-BC frames whose data field is not a valid Unlock or Set V(R) command SHALL be treated as invalid frames (E9). | 232.1 §6.3.2.1 | V |
| COP-032 | With `buffer_model = Internal{depth}`, "buffer available" (E1 vs E2) SHALL mean the delivery queue has space. When the caller drains the queue below full while the state is Wait, the engine SHALL process E10 automatically. | 232.1 §6.3.2.3 | V |
| COP-033 | With `buffer_model = External`, the caller SHALL declare buffer availability before each `handle_frame` (`set_buffer_available(bool)`), and SHALL signal E10 with `buffer_released()`. | 232.1 §6.3.2.3 | T |
| COP-034 | The FARM-B Counter SHALL increment modulo 4 for every accepted Type-B frame. | 232.0 §4.2.1.9 | V |
| COP-035 | `clcw()` SHALL reflect the state after the most recent input. The CLCW SHALL carry COP in Effect = `01`, the configured VCID and Status Field, and the current flags. | 232.0 §4.2.1; 232.1 E11 | V |
| COP-036 | The Unlock command SHALL reset only FARM-1. The engine SHALL emit `Event::Unlocked` so that higher procedures can purge their buffers. | 232.1 §6.3.2.2 note | T |

## 4. Interaction and closed-loop testing

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| COP-040 | The test suite SHALL include closed-loop scenarios connecting a FOP-1 and a FARM-1 through a simulated link with configurable loss, duplication, reordering, delay, and CLCW sampling rate. It SHALL assert in-order, complete, duplicate-free delivery, or an Alert, in every scenario. | 232.1 §1 (service guarantee) | V/F |
| COP-041 | A property test SHALL drive FOP-1 and FARM-1 with random event sequences and assert that no state outside the tables is reachable and that GEN-020 holds. | — | F |
