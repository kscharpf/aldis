# Review: COP-1 State Tables (CCSDS 232.1-B-2 and Cor. 1)

This review fulfills [COP-003](file:///Users/kevinscharpf/school/ccsds/code/docs/standards/cop1.md#L15) and the first inspection pass of [VER-041](file:///Users/kevinscharpf/school/ccsds/code/docs/06-verification.md#L69) for Milestone 3 ([M3 plan](file:///Users/kevinscharpf/school/ccsds/code/docs/milestones/m3-cop1.md#L73-L77) WP-1a). Every cell of Table 5-1 (FOP-1, 300 cells) and Table 6-1 (FARM-1, 33 cells) is verified against the normative text of CCSDS 232.1-B-2 (September 2010) and Technical Corrigendum 1 (CCSDS 232.1-B-2 Cor. 1 / TC 19-11, April 2021).

## Metadata

- **Date:** 2026-10-27
- **Reviewer:** Pair programming assistant (Antigravity), WP-1a spec-readings review
- **Primary Sources:**
  - CCSDS 232.1-B-2: *Communications Operation Procedure-1*, Blue Book, September 2010 (`specs/pdf/232x1b2e2c1.pdf`)
  - CCSDS 232.1-B-2 Cor. 1: Technical Corrigendum 1, April 2021 (`specs/pdf/232x1b2e2c1_tc1911.pdf`)
  - CCSDS 232.0-B-4 Cor. 1: *TC Space Data Link Protocol*, Blue Book
  - CCSDS 732.1-B-1: *Unified Space Data Link Protocol*, Blue Book
- **Artifacts Changed:**
  - Field catalogue in [docs/03-api-model.md §9.1](file:///Users/kevinscharpf/school/ccsds/code/docs/03-api-model.md#L210) (codes 150–164 reserved and generated)
  - Milestone plan [docs/milestones/m3-cop1.md](file:///Users/kevinscharpf/school/ccsds/code/docs/milestones/m3-cop1.md#L10-L16) (WP-1a/b/c decomposition)
  - Specification notes in [docs/standards/cop1.md](file:///Users/kevinscharpf/school/ccsds/code/docs/standards/cop1.md)
  - Review record: this document ([docs/verification/reviews/cop1-tables.md](file:///Users/kevinscharpf/school/ccsds/code/docs/verification/reviews/cop1-tables.md))
- **Status:** **Approved for implementation.** No protocol discrepancies remain; all normative readings, state names, actions, and Cor. 1 updates are established.

---

## 1. Normative Readings & Findings

### 1.1 FOP-1 State Names and Suspend_State Variable (SS)
- **Normative text (232.1 §5.1.2):** The six protocol states of FOP-1 are:
  - **S1:** `Active`
  - **S2:** `Retransmit without Wait`
  - **S3:** `Retransmit with Wait`
  - **S4:** `Initializing without BC Frame`
  - **S5:** `Initializing with BC Frame`
  - **S6:** `Initial`
- **Finding:** Draft roadmap and milestone texts informally referred to S6 as "Suspended" or "Uninitialized". The Blue Book is explicit: S6 is `Initial`.
- **Suspend representation (232.1 §5.1.10, §5.1.11):** Suspension is **not** a distinct protocol state column in Table 5-1. When suspension occurs (Timeout Type 1 expiration, E18/E104), FOP-1 sets variable `Suspend_State` ($SS \in \{1, 2, 3, 4\}$) to record the state it was in, emits an `Event::Suspend`, and transitions to **S6 (Initial)**. If $SS = 0$, the service is not suspended. On receipt of `Resume AD Service` in S6 (events E30..E34), FOP-1 checks $SS$ to return to the appropriate protocol state (S1, S2, S3, or S4).

### 1.2 FARM-1 State Names
- **Normative text (232.1 §6.2, §6.3.3):** The three protocol states of FARM-1 are:
  - **S1:** `Open`
  - **S2:** `Wait`
  - **S3:** `Lockout`

### 1.3 Technical Corrigendum 1 (Cor. 1 / TC 19-11) Amendments
1. **USLP Integration:** Wherever CCSDS 232.0 (TC Space Data Link Protocol) is referenced, CCSDS 732.1 (Unified Space Data Link Protocol) applies equally (e.g. Type-AD, BD, and protocol control BC frames carrying Unlock and Set V(R)).
2. **FARM Sliding Window Parameters (232.1 §6.1.8.2, §6.1.8.3, Table 7-2):**
   - **Retransmission Allowed (§6.1.8.3.1):** $W$ is an even integer $2 \le W \le 254$. Positive window $PW$ and negative window $NW$ are fixed to $PW = NW = W / 2$.
   - **Retransmission Not Allowed (§6.1.8.3.2):** $W$ can be any integer $1 \le W \le 256$. $PW$ can be $1 \le PW \le 256$, and $NW = W - PW \in [0, 127]$. Setting $PW > NW$ is explicitly permitted.
3. **Table 5-1 Amendments (Rev. B / Cor. 1):**
   - Events E101 and E102 handle Transmission_Limit = 1 vs > 1 under Retransmit Flag = 1 and new acknowledgements.
   - Event E103 handles Retransmit Flag 1 $\to$ 0 under Wait Flag = 1.
   - Event E104 handles Timeout_Type = 1 when Transmission_Count < Transmission_Limit in S4 (Setting $SS := 4$ and Suspending).

### 1.4 Modulo-256 Circular Window Arithmetic (232.1 §5.1.8, §6.1.8.3)
- All sequence number comparisons ($V(S)$, $V(R)$, $NN(R)$, $N(R)$, $N(S)$) are performed strictly modulo 256.
- In FOP-1, $N(R)$ in a CLCW is valid if and only if $NN(R) \le N(R) \le V(S)$ in circular sequence space. An $N(R)$ outside this arc indicates sequence desynchronization and triggers `E13` (`Alert[NN(R)]`).
- In FARM-1, $N(S)$ of an incoming Type-AD frame is partitioned relative to $V(R)$:
  - $N(S) = V(R)$: Expected frame (E1 buffer available, E2 buffer unavailable).
  - $N(S) \in [V(R) + 1, V(R) + PW - 1] \pmod{256}$: Positive window (E3, premature frame, missing predecessors).
  - $N(S) \in [V(R) - NW, V(R) - 1] \pmod{256}$: Negative window (E4, duplicate frame already delivered).
  - Outside window: Lockout area (E5, causes transition to S3 Lockout).

### 1.5 Alert Suppression (232.1 §4.3)
- After an Alert is generated (transition to S6), no further Alert other than `Llif` (Lower-Layer Interface Failure, E42/E44/E46) can be emitted until an `Initiate AD Service` directive is accepted.

---

## 2. Table 6-1: FARM-1 State Table Cell Review (33 Cells)

FARM-1 operates on states $S1$ (`Open`), $S2$ (`Wait`), and $S3$ (`Lockout`).

| Event | Condition | S1 (Open) | S2 (Wait) | S3 (Lockout) |
|---|---|---|---|---|
| **E1** | Valid Type-AD frame arrives, $N(S) = V(R)$, buffer available | Accept frame, $V(R) := V(R) + 1$, `Retransmit_Flag := 0` $\to$ **S1** | *Not applicable* | Discard $\to$ **S3** |
| **E2** | Valid Type-AD frame arrives, $N(S) = V(R)$, no buffer available | Discard, `Retransmit_Flag := 1`, `Wait_Flag := 1` $\to$ **S2** | Discard $\to$ **S2** | Discard $\to$ **S3** |
| **E3** | Valid Type-AD frame arrives, $N(S) \neq V(R)$ in positive window | Discard, `Retransmit_Flag := 1` $\to$ **S1** | Discard $\to$ **S2** | Discard $\to$ **S3** |
| **E4** | Valid Type-AD frame arrives, in negative window | Discard $\to$ **S1** | Discard $\to$ **S2** | Discard $\to$ **S3** |
| **E5** | Valid Type-AD frame arrives, outside sliding window | Discard, `Lockout_Flag := 1` $\to$ **S3** | Discard, `Lockout_Flag := 1` $\to$ **S3** | Discard $\to$ **S3** |
| **E6** | Valid Type-BD frame arrives | Accept, increment `FARM-B_Counter` $\to$ **S1** | Accept, increment `FARM-B_Counter` $\to$ **S2** | Accept, increment `FARM-B_Counter` $\to$ **S3** |
| **E7** | Valid Unlock Type-BC frame arrives | Increment `FARM-B_Counter`, `Retransmit_Flag := 0` $\to$ **S1** | Increment `FARM-B_Counter`, `Retransmit_Flag := 0`, `Wait_Flag := 0` $\to$ **S1** | Increment `FARM-B_Counter`, `Retransmit_Flag := 0`, `Wait_Flag := 0`, `Lockout_Flag := 0` $\to$ **S1** |
| **E8** | Valid Set V(R) Type-BC frame arrives with value $V^*(R)$ | Increment `FARM-B_Counter`, `Retransmit_Flag := 0`, $V(R) := V^*(R)$ $\to$ **S1** | Increment `FARM-B_Counter`, `Retransmit_Flag := 0`, `Wait_Flag := 0`, $V(R) := V^*(R)$ $\to$ **S1** | Increment `FARM-B_Counter` $\to$ **S3** |
| **E9** | Invalid frame arrives (or invalid BC command) | Discard $\to$ **S1** | Discard $\to$ **S2** | Discard $\to$ **S3** |
| **E10** | Buffer release signal | Ignore $\to$ **S1** | `Wait_Flag := 0` $\to$ **S1** | `Wait_Flag := 0` $\to$ **S3** |
| **E11** | CLCW report time | Generate CLCW with $V(R)$, flags, `FARM-B_Counter` $\to$ **S1** | Generate CLCW with $V(R)$, flags, `FARM-B_Counter` $\to$ **S2** | Generate CLCW with $V(R)$, flags, `FARM-B_Counter` $\to$ **S3** |

### Cell Analysis Notes for FARM-1
- **Cell (S2, E1):** Marked "Not applicable" in Table 6-1 because in State S2, `Wait_Flag = 1` indicates that the buffer is full; an incoming frame with buffer available is contradictory. Per [COP-005](file:///Users/kevinscharpf/school/ccsds/code/docs/standards/cop1.md#L17), the implementation maps this to `InvalidState` / discard without transition.
- **Cell (S3, E8):** In Lockout state, `Set V(R)` increments the FARM-B counter but does **not** update $V(R)$ and does not clear Lockout. Only `Unlock` (E7) can exit Lockout (COP-031, COP-036).
- **Cell (S3, E10):** A buffer release signal while in Lockout clears `Wait_Flag` but leaves the engine in S3 (`Lockout`).

---

## 3. Table 5-1: FOP-1 State Table Cell Review (300 Cells)

FOP-1 operates on states:
- **S1:** `Active`
- **S2:** `Retransmit without Wait`
- **S3:** `Retransmit with Wait`
- **S4:** `Initializing without BC Frame`
- **S5:** `Initializing with BC Frame`
- **S6:** `Initial`

### 3.1 Part 1: CLCW Arrival Events (E1–E15, E101–E103)

Events evaluate a received CLCW with `COP in Effect = 01` matching the VCID:

| Event | Trigger Condition | S1 (Active) | S2 (Retr no Wait) | S3 (Retr with Wait) | S4 (Init no BC) | S5 (Init with BC) | S6 (Initial) |
|---|---|---|---|---|---|---|---|
| **E1** | Lockout=0, $N(R) = V(S)$, Retransmit=0, Wait=0, $N(R) = NN(R)$ | Ignore $\to$ **S1** | Alert[synch] $\to$ **S6** | Alert[synch] $\to$ **S6** | Confirm, Cancel Timer $\to$ **S1** | Confirm, Release BC copy, Cancel Timer $\to$ **S1** | Ignore $\to$ **S6** |
| **E2** | Lockout=0, $N(R) = V(S)$, Retransmit=0, Wait=0, $N(R) \neq NN(R)$ | Remove acked from Sent_Queue, Cancel Timer, Look for FDU $\to$ **S1** | Remove acked from Sent_Queue, Cancel Timer, Look for FDU $\to$ **S1** | Remove acked from Sent_Queue, Cancel Timer, Look for FDU $\to$ **S1** | *Not applicable* | *Not applicable* | Ignore $\to$ **S6** |
| **E3** | Lockout=0, $N(R) = V(S)$, Retransmit=0, Wait=1 | Alert[CLCW] $\to$ **S6** | Alert[CLCW] $\to$ **S6** | Alert[CLCW] $\to$ **S6** | Alert[CLCW] $\to$ **S6** | Alert[CLCW] $\to$ **S6** | Ignore $\to$ **S6** |
| **E4** | Lockout=0, $N(R) = V(S)$, Retransmit=1 | Alert[synch] $\to$ **S6** | Alert[synch] $\to$ **S6** | Alert[synch] $\to$ **S6** | Alert[synch] $\to$ **S6** | Ignore $\to$ **S5** | Ignore $\to$ **S6** |
| **E5** | Lockout=0, $NN(R) \le N(R) < V(S)$, Retransmit=0, Wait=0, $N(R) = NN(R)$ | Ignore $\to$ **S1** | Alert[synch] $\to$ **S6** | Alert[synch] $\to$ **S6** | *Not applicable* | *Not applicable* | Ignore $\to$ **S6** |
| **E6** | Lockout=0, $NN(R) \le N(R) < V(S)$, Retransmit=0, Wait=0, $N(R) \neq NN(R)$ | Remove acked from Sent_Queue, Look for FDU $\to$ **S1** | Remove acked from Sent_Queue, Look for FDU $\to$ **S1** | Remove acked from Sent_Queue, Look for FDU $\to$ **S1** | *Not applicable* | *Not applicable* | Ignore $\to$ **S6** |
| **E7** | Lockout=0, $NN(R) \le N(R) < V(S)$, Retransmit=0, Wait=1 | Alert[CLCW] $\to$ **S6** | Alert[CLCW] $\to$ **S6** | Alert[CLCW] $\to$ **S6** | *Not applicable* | *Not applicable* | Ignore $\to$ **S6** |
| **E101** | Lockout=0, $NN(R) \le N(R) < V(S)$, Retransmit=1, Transmission_Limit=1, $N(R) \neq NN(R)$ | Remove acked from Sent_Queue, Alert[Limit] $\to$ **S6** | Remove acked from Sent_Queue, Alert[Limit] $\to$ **S6** | Remove acked from Sent_Queue, Alert[Limit] $\to$ **S6** | *Not applicable* | *Not applicable* | Ignore $\to$ **S6** |
| **E102** | Lockout=0, $NN(R) \le N(R) < V(S)$, Retransmit=1, Transmission_Limit=1, $N(R) = NN(R)$ | Alert[Limit] $\to$ **S6** | Alert[Limit] $\to$ **S6** | Alert[Limit] $\to$ **S6** | *Not applicable* | *Not applicable* | Ignore $\to$ **S6** |
| **E8** | Lockout=0, $NN(R) \le N(R) < V(S)$, Retransmit=1, Transmission_Limit>1, $N(R) \neq NN(R)$, Wait=0 | Remove acked from Sent_Queue, Initiate AD Retransmission, Look for FDU $\to$ **S2** | Remove acked from Sent_Queue, Initiate AD Retransmission, Look for FDU $\to$ **S2** | Remove acked from Sent_Queue, Initiate AD Retransmission, Look for FDU $\to$ **S2** | *Not applicable* | *Not applicable* | Ignore $\to$ **S6** |
| **E9** | Lockout=0, $NN(R) \le N(R) < V(S)$, Retransmit=1, Transmission_Limit>1, $N(R) \neq NN(R)$, Wait=1 | Remove acked from Sent_Queue $\to$ **S3** | Remove acked from Sent_Queue $\to$ **S3** | Remove acked from Sent_Queue $\to$ **S3** | *Not applicable* | *Not applicable* | Ignore $\to$ **S6** |
| **E10** | Lockout=0, $NN(R) \le N(R) < V(S)$, Retransmit=1, Transmission_Limit>1, $N(R) = NN(R)$, Transmission_Count < Limit, Wait=0 | Initiate AD Retransmission, Look for FDU $\to$ **S2** | Ignore $\to$ **S2** | Initiate AD Retransmission, Look for FDU $\to$ **S2** | *Not applicable* | *Not applicable* | Ignore $\to$ **S6** |
| **E11** | Lockout=0, $NN(R) \le N(R) < V(S)$, Retransmit=1, Transmission_Limit>1, $N(R) = NN(R)$, Transmission_Count < Limit, Wait=1 | Ignore $\to$ **S3** | Ignore $\to$ **S3** | Ignore $\to$ **S3** | *Not applicable* | *Not applicable* | Ignore $\to$ **S6** |
| **E12** | Lockout=0, $NN(R) \le N(R) < V(S)$, Retransmit=1, Transmission_Limit>1, $N(R) = NN(R)$, Transmission_Count $\ge$ Limit, Wait=0 | Ignore $\to$ **S2** | Ignore $\to$ **S2** | Ignore $\to$ **S2** | *Not applicable* | *Not applicable* | Ignore $\to$ **S6** |
| **E103** | Lockout=0, $NN(R) \le N(R) < V(S)$, Retransmit=1, Transmission_Limit>1, $N(R) = NN(R)$, Transmission_Count $\ge$ Limit, Wait=1 | Ignore $\to$ **S3** | Ignore $\to$ **S3** | Ignore $\to$ **S3** | *Not applicable* | *Not applicable* | Ignore $\to$ **S6** |
| **E13** | Invalid $N(R)$ ($N(R) < NN(R)$ or $N(R) > V(S)$ mod 256) | Alert[NN(R)] $\to$ **S6** | Alert[NN(R)] $\to$ **S6** | Alert[NN(R)] $\to$ **S6** | Alert[NN(R)] $\to$ **S6** | Ignore $\to$ **S5** | Ignore $\to$ **S6** |
| **E14** | Lockout Flag = 1 | Alert[Lockout] $\to$ **S6** | Alert[Lockout] $\to$ **S6** | Alert[Lockout] $\to$ **S6** | Alert[Lockout] $\to$ **S6** | Ignore $\to$ **S5** | Ignore $\to$ **S6** |
| **E15** | Bit pattern invalid under COP-1 (COP in Effect $\neq 01$, or bad Type) | Alert[CLCW] $\to$ **S6** | Alert[CLCW] $\to$ **S6** | Alert[CLCW] $\to$ **S6** | Alert[CLCW] $\to$ **S6** | Alert[CLCW] $\to$ **S6** | Ignore $\to$ **S6** |

---

### 3.2 Part 2: Timer T1 Expiration Events (E16–E18, E104)

Timer T1 monitors transmission acknowledgement deadlines:

| Event | Condition | S1 (Active) | S2 (Retr no Wait) | S3 (Retr with Wait) | S4 (Init no BC) | S5 (Init with BC) | S6 (Initial) |
|---|---|---|---|---|---|---|---|
| **E16** | Count < Limit, Timeout_Type = 0 | Initiate AD Retransmission, Look for FDU $\to$ **S1** | Initiate AD Retransmission, Look for FDU $\to$ **S2** | Ignore $\to$ **S3** | Alert[T1] $\to$ **S6** | Initiate BC Retransmission, Look for Directive $\to$ **S5** | *Not applicable* |
| **E104** | Count < Limit, Timeout_Type = 1 | Initiate AD Retransmission, Look for FDU $\to$ **S1** | Initiate AD Retransmission, Look for FDU $\to$ **S2** | Ignore $\to$ **S3** | $SS := 4$, Suspend $\to$ **S6** | Initiate BC Retransmission, Look for Directive $\to$ **S5** | *Not applicable* |
| **E17** | Count $\ge$ Limit, Timeout_Type = 0 | Alert[T1] $\to$ **S6** | Alert[T1] $\to$ **S6** | Alert[T1] $\to$ **S6** | Alert[T1] $\to$ **S6** | Alert[T1] $\to$ **S6** | *Not applicable* |
| **E18** | Count $\ge$ Limit, Timeout_Type = 1 | $SS := 1$, Suspend $\to$ **S6** | $SS := 2$, Suspend $\to$ **S6** | $SS := 3$, Suspend $\to$ **S6** | $SS := 4$, Suspend $\to$ **S6** | Alert[T1] $\to$ **S6** | *Not applicable* |

---

### 3.3 Part 3: Transfer Request Events (E19–E22)

Higher-procedure requests to transmit user data:

| Event | Condition | S1 (Active) | S2 (Retr no Wait) | S3 (Retr with Wait) | S4 (Init no BC) | S5 (Init with BC) | S6 (Initial) |
|---|---|---|---|---|---|---|---|
| **E19** | Request AD, Wait_Queue empty | Add to Wait_Queue, Look for FDU $\to$ **S1** | Add to Wait_Queue, Look for FDU $\to$ **S2** | Add to Wait_Queue $\to$ **S3** | Reject $\to$ **S4** | Reject $\to$ **S5** | Reject $\to$ **S6** |
| **E20** | Request AD, Wait_Queue not empty | Reject $\to$ **S1** | Reject $\to$ **S2** | Reject $\to$ **S3** | Reject $\to$ **S4** | Reject $\to$ **S5** | Reject $\to$ **S6** |
| **E21** | Request BD, `BD_Out_Flag = Ready` | Accept, Transmit BD frame $\to$ **S1** | Accept, Transmit BD frame $\to$ **S2** | Accept, Transmit BD frame $\to$ **S3** | Accept, Transmit BD frame $\to$ **S4** | Accept, Transmit BD frame $\to$ **S5** | Accept, Transmit BD frame $\to$ **S6** |
| **E22** | Request BD, `BD_Out_Flag = Not_Ready` | Reject $\to$ **S1** | Reject $\to$ **S2** | Reject $\to$ **S3** | Reject $\to$ **S4** | Reject $\to$ **S5** | Reject $\to$ **S6** |

---

### 3.4 Part 4: Management Directive Events (E23–E40)

Management directives controlling the lifecycle and parameters of FOP-1:

| Event | Directive | S1 (Active) | S2 (Retr no Wait) | S3 (Retr with Wait) | S4 (Init no BC) | S5 (Init with BC) | S6 (Initial) |
|---|---|---|---|---|---|---|---|
| **E23** | Initiate AD (without CLCW check) | Reject $\to$ **S1** | Reject $\to$ **S2** | Reject $\to$ **S3** | Reject $\to$ **S4** | Reject $\to$ **S5** | Accept, Initialize, Confirm $\to$ **S1** |
| **E24** | Initiate AD (with CLCW check) | Reject $\to$ **S1** | Reject $\to$ **S2** | Reject $\to$ **S3** | Reject $\to$ **S4** | Reject $\to$ **S5** | Accept, Initialize, Start Timer $\to$ **S4** |
| **E25** | Initiate AD (with Unlock), `BC_Ready` | Reject $\to$ **S1** | Reject $\to$ **S2** | Reject $\to$ **S3** | Reject $\to$ **S4** | Reject $\to$ **S5** | Accept, Initialize, Transmit Unlock BC $\to$ **S5** |
| **E26** | Initiate AD (with Unlock), `BC_Not_Ready` | Reject $\to$ **S1** | Reject $\to$ **S2** | Reject $\to$ **S3** | Reject $\to$ **S4** | Reject $\to$ **S5** | Reject $\to$ **S6** |
| **E27** | Initiate AD (with Set V(R)), `BC_Ready` | Reject $\to$ **S1** | Reject $\to$ **S2** | Reject $\to$ **S3** | Reject $\to$ **S4** | Reject $\to$ **S5** | Accept, Initialize, $V(S) := V^*(R)$, $NN(R) := V^*(R)$, Transmit Set V(R) BC $\to$ **S5** |
| **E28** | Initiate AD (with Set V(R)), `BC_Not_Ready`| Reject $\to$ **S1** | Reject $\to$ **S2** | Reject $\to$ **S3** | Reject $\to$ **S4** | Reject $\to$ **S5** | Reject $\to$ **S6** |
| **E29** | Terminate AD Service | Accept, Alert[term], Confirm $\to$ **S6** | Accept, Alert[term], Confirm $\to$ **S6** | Accept, Alert[term], Confirm $\to$ **S6** | Accept, Alert[term], Confirm $\to$ **S6** | Accept, Alert[term], Confirm $\to$ **S6** | Accept, Confirm $\to$ **S6** |
| **E30** | Resume AD, $SS = 0$ | Reject $\to$ **S1** | Reject $\to$ **S2** | Reject $\to$ **S3** | Reject $\to$ **S4** | Reject $\to$ **S5** | Reject $\to$ **S6** |
| **E31** | Resume AD, $SS = 1$ | *Not applicable* | *Not applicable* | *Not applicable* | *Not applicable* | *Not applicable* | Accept, Resume, Confirm $\to$ **S1** |
| **E32** | Resume AD, $SS = 2$ | *Not applicable* | *Not applicable* | *Not applicable* | *Not applicable* | *Not applicable* | Accept, Resume, Confirm $\to$ **S2** |
| **E33** | Resume AD, $SS = 3$ | *Not applicable* | *Not applicable* | *Not applicable* | *Not applicable* | *Not applicable* | Accept, Resume, Confirm $\to$ **S3** |
| **E34** | Resume AD, $SS = 4$ | *Not applicable* | *Not applicable* | *Not applicable* | *Not applicable* | *Not applicable* | Accept, Resume, Confirm $\to$ **S4** |
| **E35** | Set V(S) to $V^*(S)$ | Reject $\to$ **S1** | Reject $\to$ **S2** | Reject $\to$ **S3** | Reject $\to$ **S4** | Reject $\to$ **S5** | If $SS = 0$: Accept, $V(S) := V^*(S)$, $NN(R) := V^*(S)$, Confirm; Else Reject $\to$ **S6** |
| **E36** | Set Sliding Window Width K | Accept, Set K, Confirm $\to$ **S1** | Accept, Set K, Confirm $\to$ **S2** | Accept, Set K, Confirm $\to$ **S3** | Accept, Set K, Confirm $\to$ **S4** | Accept, Set K, Confirm $\to$ **S5** | Accept, Set K, Confirm $\to$ **S6** |
| **E37** | Set T1_Initial | Accept, Set T1, Confirm $\to$ **S1** | Accept, Set T1, Confirm $\to$ **S2** | Accept, Set T1, Confirm $\to$ **S3** | Accept, Set T1, Confirm $\to$ **S4** | Accept, Set T1, Confirm $\to$ **S5** | Accept, Set T1, Confirm $\to$ **S6** |
| **E38** | Set Transmission_Limit | Accept, Set Limit, Confirm $\to$ **S1** | Accept, Set Limit, Confirm $\to$ **S2** | Accept, Set Limit, Confirm $\to$ **S3** | Accept, Set Limit, Confirm $\to$ **S4** | Accept, Set Limit, Confirm $\to$ **S5** | Accept, Set Limit, Confirm $\to$ **S6** |
| **E39** | Set Timeout_Type | Accept, Set TT, Confirm $\to$ **S1** | Accept, Set TT, Confirm $\to$ **S2** | Accept, Set TT, Confirm $\to$ **S3** | Accept, Set TT, Confirm $\to$ **S4** | Accept, Set TT, Confirm $\to$ **S5** | Accept, Set TT, Confirm $\to$ **S6** |
| **E40** | Invalid Directive | Reject $\to$ **S1** | Reject $\to$ **S2** | Reject $\to$ **S3** | Reject $\to$ **S4** | Reject $\to$ **S5** | Reject $\to$ **S6** |

---

### 3.5 Part 5: Lower-Procedure Responses (E41–E46)

Lower-Procedure confirmations for transmitted frames:

| Event | Response | S1 (Active) | S2 (Retr no Wait) | S3 (Retr with Wait) | S4 (Init no BC) | S5 (Init with BC) | S6 (Initial) |
|---|---|---|---|---|---|---|---|
| **E41** | AD_Accept | `AD_Out = Ready`, Look for FDU $\to$ **S1** | `AD_Out = Ready`, Look for FDU $\to$ **S2** | `AD_Out = Ready` $\to$ **S3** | `AD_Out = Ready` $\to$ **S4** | `AD_Out = Ready` $\to$ **S5** | `AD_Out = Ready` $\to$ **S6** |
| **E42** | AD_Reject | Alert[LLIF] $\to$ **S6** | Alert[LLIF] $\to$ **S6** | Alert[LLIF] $\to$ **S6** | Alert[LLIF] $\to$ **S6** | Alert[LLIF] $\to$ **S6** | Alert[LLIF] $\to$ **S6** |
| **E43** | BC_Accept | `BC_Out = Ready` $\to$ **S1** | `BC_Out = Ready` $\to$ **S2** | `BC_Out = Ready` $\to$ **S3** | `BC_Out = Ready` $\to$ **S4** | `BC_Out = Ready`, Look for Directive $\to$ **S5** | `BC_Out = Ready` $\to$ **S6** |
| **E44** | BC_Reject | Alert[LLIF] $\to$ **S6** | Alert[LLIF] $\to$ **S6** | Alert[LLIF] $\to$ **S6** | Alert[LLIF] $\to$ **S6** | Alert[LLIF] $\to$ **S6** | Alert[LLIF] $\to$ **S6** |
| **E45** | BD_Accept | `BD_Out = Ready`, Accept $\to$ **S1** | `BD_Out = Ready`, Accept $\to$ **S2** | `BD_Out = Ready`, Accept $\to$ **S3** | `BD_Out = Ready`, Accept $\to$ **S4** | `BD_Out = Ready`, Accept $\to$ **S5** | `BD_Out = Ready`, Accept $\to$ **S6** |
| **E46** | BD_Reject | Alert[LLIF] $\to$ **S6** | Alert[LLIF] $\to$ **S6** | Alert[LLIF] $\to$ **S6** | Alert[LLIF] $\to$ **S6** | Alert[LLIF] $\to$ **S6** | Alert[LLIF] $\to$ **S6** |

---

## 4. Primitive Actions Catalogue for Code Representation

The implementation represents table cells as arrays of primitive action tokens (COP-003, RS-021). The reviewed set of actions across all cells consists of:

### 4.1 FARM-1 Action Tokens (`Farm1Action`)
- `Discard`: Drop incoming frame without delivery.
- `DeliverFdu`: Hand accepted FDU to higher procedure / delivery queue.
- `IncrementVr`: $V(R) := (V(R) + 1) \pmod{256}$.
- `SetVr(u8)`: $V(R) := V^*(R)$ from Set V(R) command.
- `SetRetransmitFlag(bool)`: Update CLCW Retransmit Flag.
- `SetWaitFlag(bool)`: Update CLCW Wait Flag.
- `SetLockoutFlag(bool)`: Update CLCW Lockout Flag.
- `IncrementFarmBCounter`: Modulo-4 increment on Type-B frame.
- `GenerateClcw`: Produce current CLCW report.

### 4.2 FOP-1 Action Tokens (`Fop1Action`)
- `TransmitAd`: Build Type-AD frame with $N(S) = V(S)$, append to `Sent_Queue`, start/restart T1.
- `TransmitBd`: Build Type-BD frame with $N(S) = 0$.
- `TransmitBc(BcCommand)`: Transmit Type-BC frame carrying Unlock or Set V(R), start/restart T1.
- `InitiateAdRetransmission`: Retransmit unacknowledged frames on `Sent_Queue`, reset Transmission_Count, start T1.
- `InitiateBcRetransmission`: Retransmit BC frame, start T1.
- `RemoveAckedFrames`: Purge acknowledged frames from `Sent_Queue` where $N(S) < N(R) \pmod{256}$.
- `PurgeSentQueue`: Generate Negative Confirm for each queued frame and clear queue.
- `PurgeWaitQueue`: Generate Reject for queued FDU.
- `StartTimer`: Arm timer T1 with `T1_Initial`.
- `RestartTimer`: Restart timer T1.
- `CancelTimer`: Disarm timer T1.
- `LookForFdu`: Check Wait_Queue and window $K$ to initiate next transmission if ready.
- `LookForDirective`: Check if directive waiting for `BC_Out_Flag = Ready`.
- `SetVs(u8)`: Set $V(S)$ and $NN(R)$.
- `Initialize`: Set $V(S) := 0$, $NN(R) := 0$, purge queues, cancel timer.
- `SetSuspendState(u8)`: $SS := \text{current state}$.
- `Resume`: Clear $SS := 0$, restore saved protocol state.
- `Alert(AlertReason)`: Emit `Event::Alert{reason, event, prior_state}`.
- `TransferNotify(Status)`: Accept, Reject, PositiveConfirm, NegativeConfirm.
- `DirectiveNotify(Status)`: Accept, Reject, PositiveConfirm, NegativeConfirm.
- `SetOutFlagReady(FrameKind)`: Mark lower-procedure ready for AD, BC, or BD.

---

## 5. Review Conclusion

Table 5-1 and Table 6-1 are completely verified against CCSDS 232.1-B-2 and Cor. 1.
- **Table 6-1 (FARM-1):** 3 states × 11 events = 33 cells verified. Ready for `farm1_table.rs` (WP-1b).
- **Table 5-1 (FOP-1):** 6 states × 50 events = 300 cells verified. Ready for `fop1_table.rs` (WP-1c).
- **Field codes:** Block 150–164 verified and integrated into the catalogue and C header.
- **Traceability:** COP-003 and VER-041 inspection criteria are satisfied for the spec-readings phase.
