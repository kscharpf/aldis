# Milestone 3 — COP-1

**Goal:** a complete, verified implementation of CCSDS 232.1-B-2 COP-1 in `aldis-cop1` (FOP-1 and FARM-1 engines as deterministic sans-I/O state machines with state tables represented as data), closed-loop testing across simulated links, blind conformance vectors covering every cell, C ABI handles in `aldis-ffi`, and resolution of long-lived engine state SEU hardening. Verified to the project standard: 100 % coverage, bare-metal panic-freedom, fuzzing, mutation testing, worst-case stack measurement, and hazard analysis.
**Roadmap entry:** [08-roadmap.md](../08-roadmap.md) §1 · **Builds on:** [M2](m2-frame-codecs.md) (`v0.3.0-m2`) · **Release:** `v0.4.0-m3`

**Status: planned.** Entry gate: M2 (roadmap §1), met by PR #138 (release `v0.3.0-m2`). Decisions D1–D13; each one's status is in §2.

| WP | State | PRs | Notes |
|---|---|---|---|
| Plan | merged | #143 | This plan, D1–D13, and `--scope m3` trace gate |
| WP-0 Infrastructure & setup | merged | #135, #140, #141, #143 | WP-0a Track C retirement (#141), WP-0b agent boundaries (#140), WP-0c fuzz smoke check (#135), WP-0d plan & CI scope (#143) |
| WP-1a Spec readings & cell review | merged | #145 | CCSDS 232.1 Table 5-1 and Table 6-1 cell review `cop1-tables.md`, field codes 150–199 (COP-003, VER-041, API-082) |
| WP-1b Common types & FARM-1 table | merged | #147 | Directives, alerts, notifications, FARM-1 S1–S3 × E1–E11 const table data, cell tests (COP-002, COP-004, RS-021) |
| WP-1c FOP-1 table as data | merged | #149 | FOP-1 S1–S6 × E1–E46/E101–E104 const table data, Cor. 1 verification, cell tests (COP-001, COP-004, RS-021) |
| WP-2 Blind vectors & model | merged | #183 | Python blind oracle model `test-vectors/models/cop1.py` (TV-023) and one scenario per cell of Tables 5-1 and 6-1 in `test-vectors/cop1/` (COP-005, TV-011); the testkit loader and the runners follow in WP-3 and WP-4 |
| WP-3 `aldis-cop1`: FARM-1 | merged | #162 | `Farm1::step` and `clcw` over `FARM1_TABLE`, window check, FARM-B counter, exerciser calls; `FARM1_MAX_ACTIONS` derived from the table (COP-030…036); OI-58 opened |
| WP-4 `aldis-cop1`: FOP-1 | planned | — | `Fop1::step` and `step_event` over `FOP1_TABLE` for TC frames, Sent_Queue master copies, Wait_Queue, T1, Suspend/Resume, COP-017 by the table (no Alert filter); `fop1_max_actions(Q)` derived from the table; all 317 FOP-1 scenarios of `test-vectors/cop1/` pass (COP-001, COP-010…017). USLP frame target not built |
| WP-5 Closed loop & carry-overs | planned | — | Closed-loop simulated link harness (COP-040), event fuzzing (COP-041), OI-47 delimiter fill bound in TC/USLP, USLP truncated check |
| WP-6 C ABI handles | planned | — | `aldis-ffi`: `aldis_fop1_*`, `aldis_farm1_*`, opaque handles (CABI-030…033, CABI-040), limits, exerciser group, header |
| WP-7 Verification gates & hazards | planned | — | 100 % coverage, mutants, 4 fuzz targets, stack bounds, QEMU ×3 exerciser, COP-1 hazard analysis, assurance case |
| WP-8 Docs, trace, release | planned | — | `--scope m3 --strict`, docs, RS-015 citations, changelog release `v0.4.0-m3` |

A pull request that starts or merges a work package changes only its row (roadmap §6 rule 1).

**Process freeze (maintainer, 2026-10-30).** Until the M3 exit, no pull request changes `xtask/`, `scripts/`, `.github/`, `docs/process/`, or the verification procedure of [06-verification.md](../06-verification.md), except to fix a gate that is broken and blocks merges. Process ideas wait for the M3 exit.

History:
- 2026-10-27: plan drafted following M2 exit and Track C retirement.

## 1. Starting point

| In place after M2 | Missing for M3 |
|---|---|
| `aldis-cop1`: skeleton crate with module declarations (`farm1`, `farm1_table`, `fop1`, `fop1_table`, `notify`) and dependencies on `aldis-core`, `aldis-tc`, and optional `aldis-uslp` | All engine logic, state tables, transition functions, directive handling, timer management, and event queues |
| `aldis-tc`: complete TC frame codec, CLCW decoder and writer, frame delimiter, and frame validation (TC-001…009, TC-020…023, TC-030, TC-031) | Direct feed of decoded CLCW into FOP-1 and FARM-1 CLCW generation |
| `aldis-uslp`: complete USLP frame codec (full and truncated frames, rules `000`–`111`, validation) | FOP-1 building USLP Type-AD, BD, and protocol control BC frames (COP-010, USLP-013) |
| `aldis-ffi`: capacities file `capacities.toml` declares `[fop1]` (`sent_queue_frames = 16`, `max_frame_octets = 1024`) and `[farm1]` (`delivery_queue_depth = 4`) | Exported functions `aldis_fop1_*` and `aldis_farm1_*`, opaque handle storage, limits reporting, and C ABI test harness |
| `test-vectors/schema/engine-scenario.schema.json`: schema for engine input/output scenarios (TV-010…012) | Conformance scenarios under `test-vectors/cop1/` and loader support in `aldis-testkit` |
| Verification harness: 100 % coverage gate for 8 crates, mutation testing, panic-freedom link check, QEMU bare-metal runs for Cortex-M4F, Cortex-R5F, and RISC-V | `aldis-cop1` joined to coverage gate, nightly mutants, fuzz suite, and exerciser |
| Specification: [cop1.md](../standards/cop1.md) (area COP), [04-engines.md](../04-engines.md), [hardware-assumptions.md](../hardware-assumptions.md) | Cell review record of Tables 5-1 and 6-1 (COP-003); SEU hardening resolution |

## 2. Decisions

| # | Decision | Choice | Status |
|---|---|---|---|
| D1 | Trace scope | Add `--scope m3` from §4 to `cargo xtask ci`, the `spec` check of `cargo xtask verify`, and `ci.yml`, without `--strict`, in the plan PR, so progress is measured from the start; `--strict` at the M3 exit | Proposed |
| D2 | State tables as data (COP-001…003, RS-021) | Table 5-1 (FOP-1: S1–S6 × E1–E46, E101–E104) and Table 6-1 (FARM-1: S1–S3 × E1–E11) are represented as `const` table data mapping `(State, Event) -> Cell { actions, next_state }`. The engine transition function executes the actions and updates state by reading the table, with exhaustive matching and no wildcard arms | Accepted (maintainer, 2026-10-30) |
| D3 | Typestates vs. closed enums (RS-021) | Protocol states (`Fop1State` S1–S6, `Farm1State` S1–S3) are closed enums checked against the state tables. High-level engine lifecycle (Uninitialized vs Initialized/Active) uses typestate pattern where call sequence enforces valid order | Accepted (maintainer, 2026-10-30) |
| D4 | Field codes for `aldis-cop1` (API-082) | Block 150–199 reserved in `docs/03-api-model.md` §9.1 (M2 plan D2 amendment); initial codes allocated for COP-1 fields (`v_s`, `v_r`, `nn_r`, `sliding_window_k`, `window_w`, etc.) and generated via `cargo xtask fields --write` | Proposed |
| D5 | Blind conformance vectors (COP-005, TV-011, TV-023) | Conformance vectors in `test-vectors/cop1/` generated from an independent model written blind from CCSDS 232.1-B-2 (+ Cor. 1) without reading implementation code. At least one vector per non-"Not applicable" cell, and one per "Not applicable" cell asserting `InvalidState` | Accepted (maintainer, 2026-10-30) |
| D6 | FARM-1 buffer availability (COP-033; COP-032 withdrawn) | One model: the delivery buffer is the caller's. The `Frame` input carries `buffer_available` (E1 or E2) and the caller feeds `BufferReleased` (E10) when space for one more maximum-size frame is available (232.1 §3.3.2, §6.3.2.3). The engine holds no delivery queue | Accepted (maintainer, 2026-10-29, OI-56) |
| D7 | Closed-loop link simulation (COP-040) | Test harness in `tests/closed_loop.rs` connects FOP-1 and FARM-1 across a simulated bidirectional channel with configurable loss, duplication, reordering, delay, and CLCW sampling rate, asserting reliable duplicate-free delivery for Sequence-Controlled (AD) service | Proposed |
| D8 | SEU hardening of long-lived state (GEN-015, hardware assumptions §4) | Confirm Option (a): rely on platform memory protection (EDAC with scrubbing, GEN-015) and mission reset trap. State corruption is an application reset condition; engines are restartable via Initiate AD directives. No runtime checksum overhead | Proposed |
| D9 | Formal proofs & Kani harnesses (OI-42) | Bounded Kani proof harnesses in `crates/aldis-cop1/tests/proofs.rs` verify state transition table exhaustiveness and modulo-256 window arithmetic invariants; runs in nightly non-gating suite (`verify --all`, VER-059). Deductive proof tools deferred | Proposed |
| D10 | C ABI handles and capacities (CABI-030…033, CABI-040) | Caller-owned opaque storage with magic/generation tags for `aldis_fop1_t` and `aldis_farm1_t`. Capacities fixed from `capacities.toml` (`[fop1]`, `[farm1]`). Each engine input is one step call that returns a fixed-capacity actions struct (`aldis_fop1_step`, `aldis_farm1_step`; no `poll_*` calls), `next_deadline` and status are read-only calls (CABI-040) | Accepted (maintainer, 2026-10-29, OI-56) |
| D11 | Engine step contract (API-057…API-059, API-061, RS-022; OI-56) | FOP-1 and FARM-1 are pure step functions over plain state: `step(&self, input, now, out) -> Result<(State, Actions<N>)>`, or `decide` plus an infallible `commit` for FOP-1 where copying the Sent_Queue is too costly. `N` is derived from the cells of Tables 5-1 and 6-1 (COP-006). No output queue, no `poll_*`; timer expiry is the `Timeout` input and `next_deadline` a pure query. Frame and SDU octets go to a caller-supplied output area, and the actions hold ranges of it | Accepted (maintainer, 2026-10-29, OI-56) |
| D12 | TC & USLP Delimiter fill bound (OI-47 carry-over) | `next_frame` in `aldis-tc` and `aldis-uslp` takes caller-specified maximum fill length (e.g. 6 for TC BCH) and reports excess leftover fill as `LeftoverFill` item (with C ABI code), closing hazard controls P-TC-01 and P-USLP-01 | Proposed |
| D13 | USLP truncated frame check with SDLS (carry-over #122) | `aldis_uslp::VcConfig::new` rejects a virtual channel configuration with truncated frame length < 28 when SDLS is configured (`SecurityScope::Vc`, RS-020) | Proposed |

## 3. Work packages

Size: **S** ≈ small, **M** ≈ medium, **L** ≈ large.

```
WP-0 Infra, setup ───▶ WP-1a Spec readings ─┬─▶ WP-1b FARM-1 table ─┬─▶ WP-3 FARM-1 ──┐
                     │                      └─▶ WP-1c FOP-1 table ──┼─▶ WP-4 FOP-1 ───┼─▶ WP-5 Closed loop ─▶ WP-6 C ABI ─▶ WP-7 Gates ─▶ WP-8 Exit
                     └─▶ WP-2 Blind vectors & model ────────────────┘
```

### WP-0 Infrastructure & setup (M) — VER-022, VER-056, RS-001
- **WP-0a:** Track C retirement, shared toolchains decoupling, OI-08 reopening (merged, #141).
- **WP-0b:** Agent search boundaries and structured CLI commands (merged, #140).
- **WP-0c:** Fuzz target smoke/corpus check in `cargo xtask verify` (#135).
- **WP-0d:** Milestone plan and `--scope m3` trace scope in CI (merged, #143).

### WP-1 Spec readings & tables as data (M) — COP-001…004, RS-021, VER-041, API-082
- **WP-1a Spec readings & cell review:** Table 5-1 (FOP-1: S1–S6, E1–E46, E101–E104) and Table 6-1 (FARM-1: S1–S3, E1–E11) cell-by-cell review against CCSDS 232.1-B-2 and Cor. 1 published in `docs/verification/reviews/cop1-tables.md`; allocate COP-1 field codes (150–199) in `docs/03-api-model.md` §9.1 and regenerate `aldis_core::field`. No crate engine code (spec-readings PR).
- **WP-1b Common types & FARM-1 table:** Define `Directive`, `AlertReason`, `TransferNotification`, `DirectiveNotification` in `crates/aldis-cop1/src/notify.rs`; represent FARM-1 Table 6-1 as `const` data in `crates/aldis-cop1/src/farm1_table.rs` with closed enums `Farm1State`, `Farm1Event`, and `Farm1Action`; exhaustive 33-cell unit tests.
- **WP-1c FOP-1 table as data:** Represent FOP-1 Table 5-1 as `const` data in `crates/aldis-cop1/src/fop1_table.rs` with closed enums `Fop1State`, `Fop1Event` (50 variants), and `Fop1Action`; verify Cor. 1 amendments; exhaustive 300-cell unit tests.

### WP-2 Blind conformance vectors & independent model (L) — COP-005, TV-010…012, TV-020, TV-021, TV-023
- Independent model written blind in Python (`test-vectors/models/cop1.py`) from 232.1 specification text only (TV-023).
- Conformance test scenarios in `test-vectors/cop1/` adhering to `engine-scenario.schema.json`.
- At least one scenario per non-"Not applicable" cell, and one per "Not applicable" cell asserting `InvalidState` (COP-005).
- Extend `aldis-testkit` loader to parse and execute engine scenario steps against FOP-1 and FARM-1.

### WP-3 `aldis-cop1`: FARM-1 engine (M) — COP-002, COP-030…036
- Protocol states S1 (Open), S2 (Wait), S3 (Lockout); events E1–E11.
- Modulo-256 window classification: V(R), positive window PW, negative window NW (COP-030).
- Command frame validation: Unlock and Set V(R) (COP-031, COP-036).
- Buffer availability as an input: `buffer_available` on the `Frame` input and the `BufferReleased` input (COP-033).
- FARM-B counter modulo 4 increment on Type-B frames (COP-034).
- CLCW generation matching current state, flags, and physical RF/bit-lock inputs (COP-035).
- `Farm1::step` is the pure step of D11 with `out` for the delivered FDU. `N` is `FARM1_MAX_ACTIONS`, computed at compile time from Table 6-1 as the most any cell can push: its Discard and Accept tokens plus the `Unlocked` and `StateChanged` notifications (COP-006); its value is 3.

### WP-4 `aldis-cop1`: FOP-1 engine (L) — COP-001, COP-010…017
- Protocol states S1 (Active), S2 (Retransmit without wait), S3 (Retransmit with wait), S4 (Initial), S5 (Uninitialized), S6 (Suspended); events E1–E46, E101–E104.
- `Fop1::step` bound (COP-006): `N` is the longest cell's action list, in which the Abort Request of Initiate AD (or BC) Retransmission counts, plus the Sent_Queue capacity, because a purge emits one Negative Confirm per queued frame.
- Frame building: Type-AD with N(S) = V(S), Type-BD with N(S) = 0, Type-BC carrying Unlock / Set V(R), for TC and USLP targets (COP-010).
- Sent_Queue master copy and ordered retransmission (COP-011).
- Modulo-256 N(R) validity check against NN(R) and V(S) (COP-012).
- T1 timer state, `next_deadline`, and the `Timeout` input (COP-013, API-053).
- Suspend/Resume handling with Suspend_State SS = 0..4 (COP-014).
- Directives (Initiate AD, Terminate, Resume, Set V(S), Set K, etc.) and Set K bound check (COP-015).
- Repetition metadata annotation (COP-016); no Alert after an Alert until Initiate AD or Resume, by Table 5-1 (COP-017, OI-65).

### WP-5 Closed-loop testing & carry-overs (M) — COP-040, COP-041, OI-47
- Closed-loop link test harness connecting FOP-1 and FARM-1 across simulated channel with loss, duplication, reordering, delay, and CLCW sampling (COP-040).
- Property-based testing with randomized event streams verifying no state outside tables is reached (COP-041).
- Implement OI-47: TC and USLP delimiter fill bound in `aldis-tc` and `aldis-uslp`.
- Implement USLP `VcConfig::new` truncated frame length check with SDLS.

### WP-6 C ABI handles & capacities (M) — CABI-030…033, CABI-040, API-082
- C ABI exported functions `aldis_fop1_*` and `aldis_farm1_*` in `crates/aldis-ffi`.
- Opaque storage with size/alignment validation and magic generation tags (CABI-031, CABI-032).
- Build-time engine capacities from `capacities.toml` (`[fop1]`, `[farm1]`) published as `ALDIS_*_MAX_*` constants of the header (CABI-030).
- Step-call APIs that return the actions of an input (CABI-040).
- Regenerate `include/aldis/aldis.h`; exerciser link check covers FOP-1 and FARM-1 handles.
- C ABI runner executing `cop1/` vectors through C interface.

### WP-7 Verification gates, hazards & assurance (M) — VER-010, VER-011, VER-022…025, VER-065, VER-066
- 100 % statement, branch, and line coverage for `aldis-cop1` (VER-024).
- Mutation testing across `aldis-cop1`; all survivors triaged (VER-025).
- Fuzz targets: `fop1_events`, `farm1_events`, `cop1_closed_loop`, `fop1_clcw` (VER-022).
- Worst-case stack analysis under budgets (VER-011).
- Bare-metal exerciser on thumbv7em, armv7r, riscv32imac under QEMU (VER-010).
- COP-1 hazard analysis in `docs/verification/hazards/cop1.md` (VER-065).
- Assurance case update in `docs/verification/assurance-case.md` (VER-066).

### WP-8 Docs, trace & release (S) — VER-020, RS-015
- Enforce `cargo xtask trace --scope m3 --strict` (VER-020).
- Rustdoc with clause citations for all public COP-1 items (RS-015).
- Update roadmap, README, and open issues.
- Release `v0.4.0-m3` (changelog generated from the PR titles).

## 4. Exit criteria

Milestone 3 is complete when every item below is green on `main`:

| Area | Criteria | Results / evidence |
|---|---|---|
| COP-1 state tables | COP-001…005, RS-021 | Traced (`trace --scope m3 --strict`). Table 5-1 and Table 6-1 transcribed as `const` data; cell review approved in `docs/verification/reviews/cop1-tables.md`; one vector per table cell in `test-vectors/cop1/` |
| FOP-1 engine | COP-010…017 | Traced. Complete FOP-1 state machine, sliding window, T1 timer, Suspend/Resume, AD/BD/BC frame building for TC and USLP |
| FARM-1 engine | COP-030…036 | Traced. Complete FARM-1 state machine, window classification, buffer models, FARM-B counter, CLCW generation |
| Closed-loop testing | COP-040, COP-041 | Traced. Simulated link harness tests loss, duplication, reordering, and delay; property tests assert no unreachable states and GEN-020 |
| Engine contract & sans-I/O | GEN-011, GEN-015, GEN-030…032, API-050, API-056, API-060, RS-020 | Traced. Pure sans-I/O engines; SEU platform protection confirmed (GEN-015); step functions with returned actions (API-057) and no output queues; compile-time generic capacities |
| C ABI handles | CABI-030…033, CABI-040, API-082 | Traced. `aldis_fop1_*` and `aldis_farm1_*` handles with opaque storage and magic checks; step-call functions; generated header `aldis.h`; vector runner passes scenarios through C ABI |
| Panic freedom & resources | VER-010, VER-011, GEN-020…024 | Gating check `stack` passes within budget; exerciser runs with `aldis-cop1` on Cortex-M4F, Cortex-R5F, and RISC-V under QEMU with 0 panics; link check succeeds |
| Portability | GEN-005…007, VER-030, VER-031 | Gating checks `no-std-*`, `features`, `big-endian` (s390x), and `layers` clean |
| Verification gates | VER-022…025, VER-054 | 100 % coverage across `aldis-cop1`; full mutation testing with survivors triaged; fuzz targets run ≥ 1 CPU-hour each |
| Vectors and trace | TV-001…003, TV-010…012, TV-020, TV-021, TV-023, VER-020, VER-021 | 100 % in-scope requirements traced (`trace --scope m3 --strict`); scenarios validated against `engine-scenario.schema.json`; blind Python model cross-check |
| Hazards & assurance case | VER-065, VER-066 | COP-1 hazard analysis approved in `docs/verification/hazards/cop1.md`; `cargo xtask assurance --strict` passes with 0 problems |
| Delimiter fill & USLP checks | OI-47, USLP-002, TC-020, USLP-015 | Delimiter fill bound implemented in TC and USLP; USLP truncated frame check with SDLS enforced |

Not in M3: Link layer multiplexing and packet generator/extractor engines (M4); full C ABI versioning and C++ wrapper `aldis.hpp` (M5); framework runtime stacks (M6); sync and channel coding (v1.1, OI-12).

## 5. Risks

| Risk | Mitigation |
|---|---|
| FOP-1 state table complexity (Table 5-1 has 6 states × 50 events) leads to subtle transition bugs | WP-1 cell-by-cell inspection with recorded review; WP-2 blind vectors from an independent Python model; COP-005 requires at least one vector per cell |
| Incomplete test coverage of race conditions in timer expiration and retransmission | Closed-loop test harness (COP-040) systematically injects dropped frames, delayed acknowledgements, and varied CLCW polling rates |
| Unbounded stack or memory usage in FOP-1 Sent_Queue | Queue capacities fixed at compile-time via const generics; `cargo xtask stack` measures worst-case stack bound |
| State corruption from SEUs in mission memory | Evaluated in D8: platform memory protection (EDAC with scrubbing, GEN-015) confirmed as baseline; invalid states rejected by table mapping; application reset handles uncorrectable trap |
| C ABI opaque storage size mismatch between targets | Storage sizes and alignments generated and asserted in `crates/aldis-ffi/src/cop1.rs` and tested on all bare-metal targets |

## 6. Hand-off to the other tracks

| Track | Needs from M3 | When |
|---|---|---|
| A — A3 (COP-1 reference scenario) | `aldis-cop1` FOP-1 on ground, FARM-1 on flight side; TC and USLP frame targets; closed-loop loopback | M3 exit (A3 entry gate) |
| R — M4 (Link engines) | FOP-1 frame submission interface for `MapSegmenter` (LNK-043) and FARM-1 `buffer_available` and `BufferReleased` inputs for `FrameReceiver`, whose caller owns the delivery buffer (LNK-053, COP-033) | M3 exit |
| ACFS — P3 (cFS flight side) | FARM-1 C ABI handle (`aldis_farm1_t`) | M3 exit |
