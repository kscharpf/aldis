# Milestone A1 — Packets

**Goal:** the first working reference application: the flight node produces every telemetry packet of the scenario's nominal timeline (housekeeping, events, payload, dump) as Space Packets with CUC time stamps and executes the uplink commands; the ground node decodes the packets with the strict codecs, tracks sequence counts, decodes time codes, reassembles dumps, and runs the ops script; the harness reproduces the first golden traces byte for byte in CI. The link level is packets: one Space Packet per link unit ([reference-application.md](../reference-application.md) §6.1).
**Roadmap entry:** [08-roadmap.md](../08-roadmap.md) §3 (track A) · **Builds on:** [A0](a0-scenario.md) (scenario, skeleton, harness) and [M1](m1-packets-time.md) (`v0.2.0-m1`: `aldis-time`, `aldis-spp`) · **Specification:** [reference-application.md](../reference-application.md) (area APP) · **Track:** A, owns `refapp/`; cuts no release (roadmap §5)

**Status: exit pending the adoption of the API findings (APP-101).** WP-0 to WP-7 are on `main`; every criterion of §4 is met and `cargo xtask trace --scope a1 --strict` passes. Decisions D1–D5 (§2) accepted by the maintainer 2026-11-06. Entry gate: M1 met (`v0.2.0-m1`); A0 exited (D1: the merge of #33, #35, and #37 on 2026-10-19 is its approval). A track-A milestone cuts no release. A1 is complete when the adoption pull request (#351) merges (see History).

| WP | State | PRs | Notes |
|---|---|---|---|
| WP-0 Plan | merged | #296 | This plan, D1–D5, `--scope a1` trace gate (D2), A0 close-out (D1) |
| WP-1 Scenario additions | merged | #298 | Only what [reference-application.md](../reference-application.md) lacks for A1: the extra cases (D4), the ops-script and fault-script entries of the scenario file, the check of the derived values; no code |
| WP-2 Harness: ops script and faults | merged | #301 | Ops-script instants in the scheduler, scripted faults, the `fault` record, `ops` in `StepInput` |
| WP-3a Flight node: telemetry | merged | #307 | On-board time, housekeeping, events, payload |
| WP-3b Flight node: commands and dump | merged | #315 | Command handler, time-tag queue, table store, memory dump |
| WP-4 Ground node | merged | #308 | Strict decoding, sequence tracking, time decoding, dump reassembly, ops script |
| WP-5 Golden traces | merged | #323, #324 | `a1-*` cases blessed, hand-checked against the Blue Books, invariants tested |
| WP-6 Strict decoding of golden traces | merged | #333 | APP-086 in the `refapp` check |
| WP-7 Exit | merged | #335 | `trace --scope a1 --strict`; roadmap, README, ACFS entry gate |

A pull request that starts or merges a work package changes only its row ([roadmap](../08-roadmap.md) §6 rule 1).

History:
- 2026-11-07: WP-7: `trace --scope a1 --strict` gates `cargo xtask ci`, `ci.yml`, and the check `spec` (D2); the §4 results column filled; roadmap, READMEs, and the ACFS P1 entry gate say A1 exited (complete only once the adoption of 2026-11-07 below lands).
- 2026-11-07: maintainer decision: APP-101 amended, so `refapp/` adopts an API change that resolves an `api-feedback` issue before its A milestone is called complete; #312 (`Cuc::to_instant`) and #305 (`Cuc::from_instant`) adopted in #351; §6 lists every finding with its track-R pull request. A1 is complete when #351 merges.
- 2026-11-06: plan drafted after the M5 exit; D1–D5 accepted by the maintainer. A0 close-out (D1) and the `--scope a1` trace gate (D2) are in the plan's pull request; the amendments of D3 (roadmap, `reference-application.md`, ACFS plan) are made in the follow-up pull request `a1/d3-fold-a2-a4`.

## 1. Starting point

| In place | Missing for A1 |
|---|---|
| **Entry gate, M1:** met. Released as `v0.2.0-m1`, status "complete (2026-10-20)" in [m1-packets-time.md](m1-packets-time.md); `aldis-time` (CUC, P-field, implicit and explicit decode) and `aldis-spp` (packet codec, `Segmenter`, `Reassembler`, `SequenceTracker`, the `spp-time` secondary header) are on `main`. M2 to M5 have exited too (`v0.3.0-m2` to `v0.6.0-m5`), so no library is missing for later milestones either | |
| **Entry gate, A0:** the work is on `main`: the scenario document with area APP (#33), the `refapp/` skeleton and the `xtask` checks and CI job (#35), the plan (#28), OI-23 to OI-26 decided (#37), all merged 2026-10-19. **Recorded as exited** by this plan's pull request (D1): the maintainer confirmed on 2026-11-06 that merging #33, #35, and #37 was the WP-4 approval, and the status lines of [a0-scenario.md](a0-scenario.md), the roadmap §3 A0 row, and the root `README.md` say so | |
| The scenario for A1: applications and command handling ([§2](../reference-application.md#2-roles-and-applications)), APIDs and every packet layout ([§3](../reference-application.md#3-identifiers-and-packet-formats)), time ([§5](../reference-application.md#5-time)), the link model, scheduler and fault kinds ([§6](../reference-application.md#6-link-model-and-faults)), the trace format and application events ([§7](../reference-application.md#7-golden-traces)), the node interface ([§8](../reference-application.md#8-node-interface)), implementation rules ([§9](../reference-application.md#9-implementation-rules)), and the nominal timeline with its expected results | The cases `a1-immediate` ("defined in A1", §6.4), and, if D4 is accepted, `a1-rejects` and `a1-downlink-faults`; how the ops script and the fault script are written in the scenario file (APP-107); the derived values of §2.4 (CRC-16 of the table and of the dump) are stated "to be cross-checked when A1 implements them" |
| `refapp/`: the four crates, `Node`/`StepInput`/`Output` in `refapp-scenario`, the scheduler with slots, delay and tie order, the trace writer, canonical-form check and comparator, the generated scenario constants, the case `a0-empty` reproduced in CI by the check `refapp` | Applications in the nodes (`refapp-flight` and `refapp-ground` have no logic); the `Case` of the scenario file has no ops script and no fault script, and the harness passes `ops: &[]` to every step (`refapp/refapp-harness/src/sim.rs`); the facade features the nodes need (`spp-time`, `time`) are not enabled |
| `cargo xtask trace` scans `refapp/` for `// req:` and reads the `requirements` of every golden-trace header (APP-108); `cargo xtask refapp-deps` enforces the facade-only rule (APP-100, APP-105) | `trace --scope a1`: see D2 |
| The A1 row of the roadmap and of [§2.1 of the A0 plan](a0-scenario.md): "All applications of D1; CUC on-board time; per-APID sequence counts (SPP-011); dump segmented into packets (SPP-013)" on the flight side; "Packet decoding, time-code decoding, sequence tracking (SPP-011), dump reassembly; ops script" on the ground side | |

What A1 does not do: frames, FECF, virtual channels, Idle Packets, and OID frames (A2); COP-1 (A3); the `aldis-link` engines (used from A2 on, D3); any C or C++ node (A5, ACFS). The `packet projection` of APP-003 is established here as the baseline and checked against the second link level in A2, so APP-003 is not in A1's exit table.

## 2. Decisions

All accepted by the maintainer on 2026-11-06 as recommended; the rows keep the options considered.

| # | Decision | Choice (recommended) | Status |
|---|---|---|---|
| D1 | Closing A0 | The merge of #33, #35, and #37 (2026-10-19) is the WP-4 approval (confirmed by the maintainer 2026-11-06). This plan's pull request records it: (a) the A0 status line and the two WP status lines of `a0-scenario.md` (`:6`, `:130`, `:143`) say exited or merged, (b) the A0 row of roadmap §3 is marked exited, (c) the root `README.md` row says complete, (d) the A1 status line states the A0 exit. Not a new review: the document was reviewed in #33 and the skeleton in #35. *Alternative, not taken:* a dedicated WP-4 review of the scenario document | Accepted (maintainer, 2026-11-06) |
| D2 | Trace scope | `cargo xtask trace --scope a1` needs no change in the trace tool: `scope_ids` finds `docs/milestones/a1-*.md` and reads the `Area` / `Criteria` table of §4, and the tool already scans `refapp/` and the golden headers. This plan's pull request adds the non-strict run to `cargo xtask ci` (the `CI` list in `xtask/src/main.rs`), the check `spec` (`xtask/src/verify/checks.rs`), and `ci.yml`, as the M5 plan did for `--scope m5`; `--strict` replaces it at the exit (WP-7), as for M4 and M5. No separate work package | Accepted (maintainer, 2026-11-06) |
| D3 | Hand-written frame loops in A2, or the engines from A2 on | **Fold the hand-written loops into A4: the application uses the `aldis-link` engines from A2 on.** *For:* one milestone less, since A4 ("Same trace as A3") would no longer exist as a replacement step; the engines are released (`v0.5.0-m4`, M5 done), so the loops would be written only to be thrown away; the engines are already cross-checked by blind models (TV-023) and the engine-scenario vectors; the application gets API feedback on the engines a milestone or two earlier, before the v1.0 freeze; the ACFS phases P2 to P4 shrink in the same way. *Against:* the hand-written loops (A2, A3) are an independent implementation of the same link rules, and "A4 reproduces the A3 trace" is a cross-check of the engines by code written without them, which the blind models cover only per unit and not for a whole run; a divergence found only at A4 shows where the spec leaves room (the A0 plan §5 risk), and it would go unseen; the trace freeze of the A0 plan (D2 rule 3, APP-004) would apply to a trace made by the engines themselves. *Alternative:* keep A2 to A4 as the roadmap has them. A1 has no frame loop and is the same either way. Accepted: a follow-up pull request, not this one, amends roadmap §3 (the A2 and A4 rows and the §4 stage table), the A0 plan's §2.1 (not edited here, it is a record), `reference-application.md` §1 and APP-004, and the ACFS plan's phases | Accepted (maintainer, 2026-11-06) |
| D4 | Cases beyond the nominal one | The scenario defines `a1-nominal` and `a1-immediate` for A1 (§6.4). Add two cases, each by APP-002 (a case, no change of an existing one): **`a1-rejects`**, one command per rejection reason of §3.3.2 (unknown function, time tag in the past, malformed arguments, time-tag queue full, dump in progress) plus `ACTIVATE_TABLE` with no uploaded table and with a wrong CRC (`TABLE_REJECTED` reasons 1 and 2, APP-017), and the immediate-execution path, so that APP-013, APP-017, and APP-024 are exercised beyond the nominal commands; **`a1-downlink-faults`**, scripted `drop`, `duplicate`, `delay`, and `truncate` of downlink packets (no `flip`: there is no FECF at packet level), so that the ground node's `seq_gap`, `seq_repeat`, `dump_discarded`, and `packet_rejected` events (APP-018) exist before A2 puts them behind frames. *For:* the rejection paths and the ground's error paths would otherwise first run in A2 or later, in frame context, where a defect is harder to place; the fault engine of the harness is needed in A2 anyway. *Against:* it moves the fault machinery from A2 to A1 and so enlarges A1 (the A0 plan D8 and §6.4 list the downlink faults under A2); the fault script of such a case is then fixed here, not in A2 (APP-005). If the maintainer prefers A0's split, A1 keeps `a1-rejects` only and APP-075 leaves §4 | Accepted (maintainer, 2026-11-06) |
| D5 | Independent check of the first golden traces | A golden trace records what the implementation did (A0 plan §5). For A1: (a) every unit is decoded with the strict codecs in CI (APP-086, WP-6); (b) the first packet of each APID and the two derived values are checked by hand against 133.0 and 301.0 and written up in the WP-5 pull request, naming the clauses; (c) an independent program, written blind from `reference-application.md` and the Blue Books only (TV-023, no implementation path given to its author), regenerates the `packet_out` records of `a1-nominal` and is run once, its result recorded in [cross-checks.md](../verification/cross-checks.md) (VER-040); the program is Python, like the models of `test-vectors/models/`, not a Cargo crate, so `cargo xtask refapp-deps` (APP-100, APP-105) does not apply; it is committed under `refapp/model/` and gets no CI step. *Alternative:* run it in CI as `cargo xtask vectors validate` does for the vector models, which adds a Python dependency to the `refapp` check and a mechanism to maintain for a trace that A2 and A3 re-test through the packet projection (APP-003). *Alternative:* (a) and (b) only | Accepted (maintainer, 2026-11-06) |

Raised with this plan: no open issue. The pain points that `refapp/` finds in the public API of M1 are filed as GitHub issues labelled `api-feedback` for track R (APP-101); this plan lists them as found (§6).

## 3. Work packages

Size: **S** ≈ small, **M** ≈ medium, **L** ≈ large. The nodes are written against the node interface of A0 and need the scenario format of WP-1 and the harness inputs of WP-2 only to run; their unit tests run on scripted inputs.

```
WP-0 Plan ─▶ WP-1 Scenario additions ─▶ WP-2 Harness ─┬─▶ WP-3a Flight telemetry ─▶ WP-3b Flight commands ─┐
                                                      └─▶ WP-4 Ground node ────────────────────────────────┼─▶ WP-5 Golden traces ─▶ WP-6 Strict decode ─▶ WP-7 Exit
```

WP-3a and WP-4 run in parallel. WP-3b needs the ground node's command builder (WP-4) only for the integrated run of WP-5.

### WP-1 Scenario additions (S) — area APP, no code
Only what the document lacks; nothing in it is restated.
- The cases of D4 in §6.4 with their end instants, ops scripts, and fault scripts, each with its expected result in the manner of §2.4 (what the flight node reports, which application events the ground node raises). `a1-immediate` is defined from §2.3 and APP-012: which commands are sent as immediate commands and when. The scenario version stays 1 (APP-002 allows added cases).
- How an ops-script entry and a fault-script entry are written in the language-neutral scenario file and its schema (APP-107): an entry names its release instant and kind (`command`, `table_upload`; `fop1_directive` from A3) and the parameters of §3.3.5 and §3.3.6, not the octets, so that the ground node builds the packets and the A5 C++ node can read the same file.
- The two derived values of §2.4 (CRC-16 of the table `0x0C4F` and of the 600 dumped octets `0x80A8`) checked by a hand derivation against CMN-001 and the simulated memory of §3.3.4, and the result written into §2.4 in place of "to be cross-checked".
- The A1 readings the nodes rely on, checked clause by clause against the Blue Books before code: the sequence count continuity and its reset (133.0 §4.1.3.4.3.3–4), which the Blue Book defines only as a count modulo 16384, so that the classification of a gap, a repeat, and a reset is Aldis's own rule (SPP-011, CMN-030) and needs no Blue Book reading; segmentation of the dump (SPP-013; 133.0 §4.1.3.4.2.2); the implicit and explicit P-field (SPP-008; 301.0 §3.2.2); the conversion of an `Instant` to a CUC value (TIME-011). A reading the Blue Book does not settle becomes an OI for the maintainer (decision rights §2.1); the scenario already avoids the known ones (APP-061).
- New requirements take the next free ID of area APP (the decades per section are full, so the next free number after the last row of the area) and go in the exit table (§4).
- Checklist: "A spec-readings pull request before a codec", for the readings.
- **Done when:** `cargo xtask spec-lint` passes.

**Results (WP-1).** Added to [reference-application.md](../reference-application.md): §6.5 (script entries), §6.6 (the cases `a1-immediate`, `a1-rejects`, `a1-downlink-faults` with their expected results), and APP-126…APP-130, which are in the exit table (§4). The harness's scenario model and the generated schema carry the `ops` and `faults` arrays; the entries of the cases were kept in a temporary file until WP-5 moved them into `scenario.json` with their golden traces (a case in `scenario.json` needs a golden trace, which no node could produce before WP-3 and WP-4). The readings, checked against the Blue Books on 2026-11-06:

| Reading | Clauses | Result |
|---|---|---|
| Sequence count, continuity, reset | 133.0 §4.1.3.4.3.3–4; §4.3.2.2 | One count per APID, continuous modulo 16 384, reset only if unavoidable (note 2: completeness is then unknown); the extraction function checks continuity. No classification of gap, repeat, or reset is given, so it is Aldis's own rule (SPP-011, CMN-030, CMN-031), written out in [reference-application.md](../reference-application.md) §6.6. No OI |
| Segmentation of the dump | 133.0 §4.1.3.4.2.1 note, §4.1.3.4.2.2 a to d, §4.1.3.4.2.3 | `01` first, `00` continuation, `10` last, `11` unsegmented, as SPP-013 and APP-016 say; use of the flags is optional. The Octet String service forces `11`, so the dump APID is a Packet service path, which the link plan has (APP-006). No OI |
| Secondary header with a time code | 133.0 §4.1.4.2.1.5 a, §4.1.4.2.2.3 to 5; 301.0 §3.2.1, §3.2.2 | Time Code Field only; the code, and the presence of the P-field, fixed per managed data path; the P-field precedes the T-field. `0x1E` is extension 0, ID `001`, 4 coarse octets, 2 fractional octets. Implicit for `0x010`, `0x020`, `0x021` and explicit for `0x011` is allowed (SPP-008, OI-30 point 2). No OI |
| `Instant` to CUC | 301.0 §3.2.1; TIME-011, OI-40 | The conversion from nanoseconds truncates (TIME-011), where the A0 plan's D12 says "ceiling"; the requirement governs and the A0 plan is a record. Moot for the scenario: every on-board time is a multiple of 125 000 µs, which 2 fractional octets hold exactly (APP-061), and APP-128 keeps the arrival of an immediate command on that grid |
| CRC-16 of the table and of the dump | 132.0 §4.1.6.2.2; CMN-001 | `0x0C4F` and `0x80A8` confirmed by evaluating the defining expression by long division over GF(2), on the 300 table octets of §3.3.6 and the 600 memory octets of §3.3.4. The 300 memory octets of `a1-rejects` give `0x7FE0` |

### WP-2 Harness: ops script and faults (M) — APP-072…075, APP-082
- `Case` of `refapp-harness/src/scenario.rs` gets the ops script and the fault script; the schema and the generated constants are regenerated (`gen-scenario`, APP-107).
- The scheduler wakes at ops-script instants (§6.2), passes the due entries as `StepInput::ops`, and applies scripted faults by (direction, unit index), recording the `fault` record just before the unit's `unit_sent` and the `copy` of a duplicate (§7.3). `flip` and the frame-level rules stay for A2.
- Tests on scripted test nodes, as for A0, each with `// req:`; the case `a0-empty` stays byte-identical.
- **Done when:** a scripted node pair reproduces a hand-written trace with every fault kind of D4, and `compare` for `a0-empty` is unchanged.

### WP-3a Flight node: telemetry (L) — APP-010, APP-015, APP-021…023, APP-060
- `refapp-flight` (`no_std`, no heap, flight lint set, APP-102): on-board time as T0 + `Instant` converted by `aldis-time` to CUC (APP-060); housekeeping, the event reporter, and the payload application with the layouts of §3.3.1 to §3.3.3, each APID with its own sequence count (SPP-011/012) and its secondary header form (SPP-008); the per-instant order of §2.1.
- Features `core`, `time`, `spp-time` of the facade only (APP-100); every API pain point filed as `api-feedback` (APP-101).
- Unit tests with `// req:`; the `thumbv7em-none-eabihf` build and Clippy stay green.
- Checklist: none of the library ones applies (no codec, crate, or engine is added); the `refapp` check is the gate.

### WP-3b Flight node: commands and dump (L) — APP-012, APP-013, APP-016, APP-017, APP-024
- Command handler: execution mode, time-tag queue of 8, rejection reasons (§2.3, §3.3.2); table store with the CRC-16 check (APP-017); memory dump segmented with `Segmenter` (SPP-013), one segment every 125 000 µs, `DUMP_COMPLETED` in the step of the last packet (APP-016); `cmd_received` and `table_received` application events (§7.4).
- Unit tests with `// req:`, including every rejection reason and the dump-in-progress rule.

### WP-4 Ground node (M) — APP-011, APP-018, APP-019, APP-024
- `refapp-ground` (restriction lints, APP-103): decodes every unit in `Strict` mode (API-030), delivers `packet_in`; per-APID `SequenceTracker` events (`seq_gap`, `seq_repeat`, `seq_reset`); CUC decoding with implicit and explicit P-fields; dump reassembly with `Reassembler` (`dump_reassembled`, `dump_discarded`); `packet_rejected` for a strict decoder error.
- The ops script: builds the command and table-upload packets from the entries of WP-1 and releases them at their instants (`packet_out`).
- Unit tests with `// req:`.

### WP-5 Golden traces (M) — APP-019, APP-082, APP-083, APP-090
- The `a1-*` cases added to the scenario file with their requirement lists; `bless` writes `refapp/golden/<case>.jsonl`; trace version 1 needs no row in `CHANGES.md` (released traces only).
- Tests of the invariants each case states in WP-1: every command executed once and in order, the packet counts of §2.4, gaps only where a fault was injected, event order of §3.3.2.
- The independent check of D5, and its record.
- The packet projection of `a1-nominal` as extracted by a harness function or test, so that A2 can compare it (APP-003); no new command line.
- **Done when:** `compare` reproduces every `a1-*` trace in CI.

### WP-6 Strict decoding of golden traces (S) — APP-086
- `check-golden` (or a harness test run by the `refapp` check) decodes every `unit_sent` and every packet of every golden trace with the strict codecs; the harness gets the `spp` feature of the facade, which `cargo xtask refapp-deps` allows (APP-100).
- **Done when:** a planted malformed packet in a copy of a trace fails the check (checked once, then reverted).

### WP-7 Exit (S) — VER-020
- `trace --scope a1 --strict` replaces the non-strict run in `cargo xtask ci`, `ci.yml`, and the check `spec` (checklist "Milestone exit").
- The results column of §4; the A1 status line; roadmap §3 A1 row, `refapp/README.md` and root `README.md` status; the entry gate of ACFS P1 ([acfs-flight-side.md](acfs-flight-side.md): "A1 exited (not yet)") updated by the pull request that exits A1.
- The `api-feedback` issues listed in §6; process metrics for the milestone reviewed (VER-063).
- A track-A milestone cuts no release.

## 4. Exit criteria

A1 is complete when every item below is green on `main`. Requirements of the scenario that need no test are reviewed in A0's table and are not repeated. APP-003 and APP-004 belong to A2 and A3, which compare two link levels. The results column was filled in the WP-7 exit pull request; the artefacts of each requirement are listed by `cargo xtask trace --scope a1`, not repeated here.

| Area | Criteria | Results / evidence |
|---|---|---|
| Flight applications | APP-010, APP-015, APP-023 | Met. Flight node `refapp/refapp-flight` (`lib.rs`, `schedule.rs`, `event.rs`, `layout.rs`, `dump.rs`); unit tests and the golden traces `a1-nominal`, `a1-immediate`, `a1-rejects` |
| Commands and dump | APP-012, APP-013, APP-016, APP-017, APP-024 | Met. `command.rs`, `queue.rs`, `table.rs`, `dump.rs` with `command_tests.rs`; golden traces `a1-nominal`, `a1-immediate`, `a1-rejects` |
| Packet identity | APP-021, APP-022 | Met. Tests in `refapp-flight/src/lib.rs`, `command_tests.rs`, and `refapp-ground`; golden trace `a1-nominal`; the blind model `refapp/model/a1_nominal.py` matched all 65 `packet_out` records ([cross-checks.md](../verification/cross-checks.md) §7, VER-040) |
| Time | APP-060 | Met. CUC stamps and time tags tested in `refapp-flight/src/lib.rs` and `command_tests.rs`; golden trace `a1-nominal` |
| Ground node | APP-011, APP-018, APP-019 | Met. `refapp/refapp-ground` (`telemetry.rs`, `ops.rs`, `tests.rs`); `refapp-harness/tests/a1_golden.rs`, `ground_ops.rs`; golden traces `a1-nominal`, `a1-downlink-faults` |
| Link model at packet level | APP-072, APP-073, APP-074, APP-075 | Met. `refapp-harness/tests/run.rs` (delay, tie order, scripted faults), `a1_golden.rs`; golden trace `a1-downlink-faults` |
| Scenario scripts and A1 cases | APP-126, APP-127, APP-128 | Met. Ops and fault scripts in `refapp/scenario/scenario.json` with the schema tests in `refapp-harness/src/scenario.rs`; the cases `a1-immediate`, `a1-rejects`, `a1-downlink-faults` have golden traces |
| Event rules of the nodes | APP-129, APP-130, APP-131 | Met. `command_tests.rs` and `refapp-ground/src/tests.rs`; golden traces `a1-rejects`, `a1-downlink-faults` |
| Golden traces | APP-082, APP-083, APP-086, APP-090 | Met. Golden traces of the four `a1-*` cases in `refapp/golden/`; `refapp-harness/tests/trace.rs` decodes every unit with the strict codecs (APP-086, WP-6); `tests/run.rs`; hand check of the first packet per APID and the blind model (D5) |
| Reproduction in CI | The check `refapp` runs `compare` for every `a1-*` case (APP-106); `cargo xtask trace --scope a1 --strict` passes; the `thumbv7em-none-eabihf` build of `refapp-flight` is green | Met. Verify check `refapp` (APP-106); `cargo xtask trace --scope a1 --strict` replaces the non-strict run in `cargo xtask ci`, `ci.yml`, and the check `spec`; the bare-metal build and clippy of `refapp-flight` are in the `refapp` job of `ci.yml` |
| Docs | A0 recorded as exited (D1); roadmap §3, the READMEs, and the ACFS entry gate updated; the new requirements of WP-1 listed here | Met. A0 recorded as exited by D1; roadmap §3, `README.md`, `refapp/README.md`, and the ACFS P1 entry gate updated by the exit pull request; APP-126 to APP-131 are in this table |

APP-075 and the fault case leave the table if D4 is decided without `a1-downlink-faults`; the IDs of WP-1 join it.

## 5. Risks

| Risk | Mitigation |
|---|---|
| A golden trace freezes a defect of the first implementation, and every later level then reproduces it | D5: strict decoding of every unit in CI, a hand check of the first packet of each APID against the Blue Books, an independent regeneration of the flight packets, and invariants tested per case |
| The A1 timeline is "fixed in A1 and only extended" (APP-002), so a mistake here binds A2 to A6 | WP-1 re-reads the readings the timeline depends on before code is written; the cases beyond the nominal one are added now, while changing a case is still free |
| API pain points of M1 found now are costly after the v1.0 freeze | Track A starts them early by design; each one becomes an `api-feedback` issue as found (APP-101) and is listed in §6 |
| D3 is decided after A1 and changes the plans of A2 to A4 and the ACFS phases | A1 has no frame loop and is the same under either answer; the amendments of D3 are one docs pull request |
| The ops-script and fault-script format chosen in WP-1 is hard to extend for A3 (FOP-1 directives) and A5 (the C++ node reads it) | Entries name a kind and parameters, so a kind is added without changing the others; `fop1_directive` is reserved in the schema from A1 and ignored below A3 (APP-019) |

## 6. API feedback found

Each finding is a GitHub issue labelled `api-feedback`; this list names them as they are filed (APP-101); a "Resolved by" names the track-R pull request.

| Issue | Found in | Finding | Resolved by |
|---|---|---|---|
| #302 | WP-4 | `Reassembler`'s `Outcome` gives the reason for a discard, not the octets thrown away | #334 |
| #303 | WP-4 | `SpacePacket::decode` ignores trailing octets; a one-packet unit needs its own length check | #334 |
| #304 | WP-4 | `Apid::new` returns `Option`, so a constant APID needs an error path | #334 |
| #305 | WP-3a | No conversion from an `Instant` to a CUC time code; the caller splits microseconds into seconds and nanoseconds | #314 |
| #306 | WP-3a | `TimeCode::encode` needs a scratch buffer whose size the caller must guess | #314 |
| #312 | WP-3b | No conversion from a CUC time code to an `Instant`; the time tag of a command is converted by hand (reverse of #305) | #322 |
| #318 | WP-3b | aldis-core CRC functions return a bare `u16`; refapp wraps it in `Crc16` | #334 |
