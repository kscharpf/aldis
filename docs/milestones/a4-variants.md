# Milestone A4 — Variants

**Goal:** the reference application runs the two variants of [reference-application.md](../reference-application.md) §4.3 on the same engines and the same application layer. `aos` is an AOS downlink with the TC uplink and COP-1 as in A3. `uslp` is a USLP fixed-length downlink and a USLP variable-length uplink, with COP-1 over USLP per [cop1.md](../standards/cop1.md) §2.1. Each variant has its own golden trace (`a4-aos-nominal`, `a4-uslp-nominal`), reproduced in CI, whose packet projection equals that of `a1-nominal` (APP-003, APP-045). Packets and the `a0-*` to `a3-*` traces do not change (APP-002, APP-005). A4 adds no C ABI surface and cuts no release.
**Roadmap entry:** [08-roadmap.md](../08-roadmap.md) §3 (track A) · **Builds on:** [A3](a3-cop1.md) (COP-1 nodes and traces), [A2](a2-frames.md), [M4](m4-link-engines.md) (`v0.5.0-m4`: the `aldis-link` engines) · **Specification:** [reference-application.md](../reference-application.md) (area APP) · **Track:** A, owns `refapp/`; cuts no release (roadmap §5) · **Review of the proposal:** issue #621

**Status: exited (2026-11-20, WP-7).** Decisions D1–D13 (§2) accepted by the maintainer on 2026-11-16, in chat, each with its recommended option. Entry gate: met. A3 exited 2026-11-11 ([a3-cop1.md](a3-cop1.md)) and M4 exited 2026-11-04 (`v0.5.0-m4`, [roadmap](../08-roadmap.md) §1). A track-A milestone cuts no release. Every exit criterion of §4 is met, and `cargo xtask trace --scope a4 --strict` passes in `cargo xtask ci`, `ci.yml`, and the check `spec` (D1).

| WP | State | PRs | Notes |
|---|---|---|---|
| WP-0 Plan | | | This plan, D1–D13, non-strict `--scope a4` trace gate (D1) |
| WP-1 Scenario additions | | | Docs only: §4.3 rows rewritten, AOS, USLP downlink and USLP uplink requirement tables, FARM-1 over USLP, CLCW in the AOS and USLP OCF, APP-138 amendment, §4.6 schedules; no code. New IDs in the exit table of §4; OI-99 and OI-100 filed and resolved (a) by the maintainer on 2026-11-18; every clause of the checklist read, none wrong |
| WP-2 Harness, features, variant skeleton | | | `variant` field, features and `refapp/Cargo.lock` once, `Variant` enum and dispatch in both nodes, strict decoding of the new frames |
| WP-3a Flight `FrameDownlink<P>` and AOS | | | Generic refactor of the flight downlink; AOS arm |
| WP-3b Flight USLP downlink | | | Stacked on WP-3a |
| WP-3c Flight `FrameUplink<P>` and FARM-1 over USLP | | | Generic refactor of the flight uplink; USLP arm |
| WP-4a Ground downlink generic and AOS | | | Generic refactor of the ground downlink; AOS arm |
| WP-4b Ground USLP downlink | | | Stacked on WP-4a |
| WP-4c Ground uplink generic over `Fop1Format`, FOP-1 USLP target | | | Generic refactor of the ground uplink; USLP arm |
| WP-5 Golden traces | | | `a4-*` blessed and frozen, hand check, blind model |
| WP-6 Strict decoding of `a4-*` traces | | | APP-086 for the variants |
| WP-7 Exit | | | `trace --scope a4 --strict`, follow-ups dispositioned, roadmap and READMEs |

A pull request that starts or merges a work package changes only its row ([roadmap](../08-roadmap.md) §6 rule 1).

History:
- 2026-11-16: plan drafted by the `planner` agent at `main` `f9a9a0d2`, reviewed independently, and revised once (issue #621); D1–D13 accepted by the maintainer, every recommended option; the scope of the C ABI node for A5 is left to the A5 plan.
- 2026-11-18: WP-1 (#697): D2 narrowed by the maintainer, `uslp` only at `cop1`; OI-99 (USLP downlink expedited) and OI-100 (one Control Command in a Type-BC TFDZ) resolved (a), both in the maintainer's chat.
- 2026-11-19: WP-3c follows WP-3b and WP-4c follows WP-4b, because they share `lib.rs` and the `Variant` arm switch (maintainer, in chat).
- 2026-11-20: WP-7 (exit): `trace --scope a4 --strict` replaces the non-strict run; APP-076 traced by an inspection record; §4 results filled; A4 exited. §6 dispositions of #718, #719 and #708 given by the maintainer in chat. No release.

## 1. Starting point

| In place | Missing for A4 |
|---|---|
| **Entry gate:** met. A3 exited 2026-11-11 (a3-cop1.md, status line) and M4 exited 2026-11-04 (roadmap §1). The `aldis-link` engines serve AOS and USLP already (M4), and the C ABI of the link engines covers them (`aldis-ffi/src/link.rs`) | |
| The TM downlink and TC uplink of A2, and COP-1 of A3: `FrameReceiver`, `PacketExtractor`, generators and multiplexer in the nodes; `Fop1` with `Fop1Target::tc`, `Farm1`; the CLCW in the TM OCF; golden traces `a0-*` to `a3-*` frozen (APP-004) | The nodes name the TM and TC types directly, so no second frame format fits; no `variant` field exists in the scenario file |
| The scenario document names the variants in §4.3 | The §4.3 `aos` and `uslp` rows disagree with D4 and D6, and no requirement states the AOS and USLP frame parameters, FARM-1 over USLP, or the CLCW in the AOS `VC_OCF` and the USLP MC OCF |
| `Fop1Target::uslp` takes one MAP and rule `111` only (`aldis-cop1/src/fop1.rs`); `Farm1Input::from_tc` builds a FARM-1 input from a TC frame | No `Farm1Input::from_uslp`; the MAP 0 / MAP 1 split of §4.2 cannot sit behind one FOP-1 (D6, D7) |

What A4 does not do: add C ABI surface; change an engine; the fault cases of the variants; the C and C++ nodes (A5, ACFS); the bare-metal target (A6); channel coding, synchronisation, SDLS (APP-006).

## 2. Decisions

D1 to D13 are accepted (maintainer, 2026-11-16, in chat; every recommended option). D1 and D11 to D13 follow A3 and A2 precedent. Library changes are never part of a refapp work package; findings go to §6.

| # | Decision | Choice | Status |
|---|---|---|---|
| D1 | Trace scope | A non-strict `cargo xtask trace --scope a4` in `cargo xtask ci`, the check `spec` and `ci.yml`, added by this plan's pull request; `--strict` replaces it at the exit (WP-7). A3 D1 precedent | Accepted (maintainer, 2026-11-16) |
| D2 | Variant selector | A per-case `variant` field (`tm_tc` default, `aos`, `uslp`), orthogonal to `level`, except that `uslp` applies only at `cop1` (the harness rejects `uslp` at `packets` and `frames`: its uplink is specified with COP-1 alone); the `a4-*` cases use `level: cop1` | Accepted (maintainer, 2026-11-16); `uslp` only at `cop1`: maintainer, 2026-11-18, #697 review 1 |
| D3 | Node architecture | Frame modules generic over the protocol type: `FrameDownlink<P>` (`Tm`, `Aos`, `Uslp`), `FrameUplink<P>` (`Tc`, `Uslp`), and a ground uplink generic over `Fop1Format`. A refapp-local trait and a closed `enum Variant`; one copy of the application layer. WP-2 builds the skeleton | Accepted (maintainer, 2026-11-16) |
| D4 | AOS downlink | 223 octets; FECF and FHEC present; no Insert Zone; VCs {0, 1, 7}, each with `VC_OCF`; M_PDU; SCID `0x155`. VC 7 uses `IdleField`, is offered only when VCs 0 and 1 offer nothing, and has the lowest priority. No OID frames: 732.0 §4.1.4.2.3.4 note 1 (OI-43). The idle-VC ordering is an engine fact (`aldis-link/src/generator.rs:582-588`, `mux.rs:485-487`) | Accepted (maintainer, 2026-11-16) |
| D5 | USLP downlink | Fixed length, 223 octets; FECF present; no Insert Zone; MC OCF required (OI-76, LNK-033); VCs {0, 1}; rule `000`; UPID by SANA name (OI-06); MAP 0; 1-octet VCF Count; Source-or-Destination = Source; one Encapsulation Idle Packet as idle fill (LNK-013); OID frames on VCID 63. WP-1 confirms the QoS (expedited recommended) and the Source reading | Accepted (maintainer, 2026-11-16) |
| D6 | USLP uplink | VC 0 and **one MAP (MAP 0)** for the commands and the 309-octet table upload, with blocking. Unsegmented (rule `111`); maximum frame length 512 octets (fit checked in WP-1); 1-octet sequence-controlled VCF Count; no expedited count (OI-66); no SDLS; Destination. This deviates from the MAP 0 / MAP 1 split of §4.2 because `Fop1Target::uslp` takes a single MAP and rule `111` only (`aldis-cop1/src/fop1.rs:446-452`); the deviation is recorded in WP-1, with #235 and OI-71 as reasons. The alternatives, a per-request MAP ID in track R and segmentation brought forward from #235, were not taken | Accepted (maintainer, 2026-11-16) |
| D7 | FARM-1 over USLP | Strict USLP decode, then the node builds `Farm1Frame`: `Ad` with N(S) from the VCF Count, `Bd`, or `Bc` with `ControlCommand::parse` (`aldis-tc/src/control.rs:26`; USLP-013). The USLP VCID is converted to `TcVcId`. Otherwise as A3 D4. `api-feedback` is filed for `Farm1Input::from_uslp` | Accepted (maintainer, 2026-11-16) |
| D8 | Cases | `a4-aos-nominal` and `a4-uslp-nominal`: the `a3-nominal` ops script, 35 s, no faults; schedules derived by hand in WP-1. No fault case for the variants | Accepted (maintainer, 2026-11-16) |
| D9 | Freeze and check | Frozen at bless (APP-004, APP-087); strict decoding in CI (APP-086); a hand check of the first frame of each kind; a blind model (TV-023) for both cases, recorded in `cross-checks.md` | Accepted (maintainer, 2026-11-16) |
| D10 | Events | Candidate code 115 for USLP `GapDiscard`. WP-1 amends APP-138 and §7.4: `frame_gap` counts modulo the count width, and scope 0 does not exist on AOS and USLP | Accepted (maintainer, 2026-11-16) |
| D11 | Unchanged | No change to packets, events or existing traces; `compare` on every case in every pull request (A3 D9 precedent) | Accepted (maintainer, 2026-11-16) |
| D12 | Features | `aos` and `uslp` on refapp-flight, refapp-ground and refapp-harness, added once in WP-2 together with `refapp/Cargo.lock` (A2 D3 precedent) | Accepted (maintainer, 2026-11-16) |
| D13 | Library findings | No engine changes. `api-feedback` issues for `Farm1Input::from_uslp` and for the single-MAP FOP-1 USLP target; #235 is cited for D6; dispositions recorded at exit | Accepted (maintainer, 2026-11-16) |

The scope of the C ABI node for A5 (whether it must reproduce `a4-*`, and whether the C ABI FOP-1 handle offers the USLP target, which is not verified) is decided in the A5 plan; A4 records it only.

## 3. Work packages

Size: **S** ≈ small, **M** ≈ medium. Branches are named `a4/<topic>`, titles `feat(a4/wp-N): …` (`docs(a4/wp-0|1)` for the docs-only WPs). Each pull request runs the default `cargo xtask check` tier in its first and final rounds, and `compare` on all golden cases.

```
WP-0 ─▶ WP-1 ─▶ WP-2 ─┬─▶ WP-3a ─▶ WP-3b ─┐
                      ├─▶ WP-3c ──────────┤
                      ├─▶ WP-4a ─▶ WP-4b ─┼─▶ WP-5 ─▶ WP-6 ─▶ WP-7
                      └─▶ WP-4c ──────────┘
```

**Order and stacking:**

1. WP-0, WP-1 and WP-2, one at a time.
2. WP-3a, WP-3c, WP-4a and WP-4c are the parallel set, at most two agents at once. Each flips one `Variant` arm in `lib.rs`.
3. WP-3b is stacked on WP-3a, and WP-4b on WP-4a.
4. WP-5, WP-6, WP-7, in order. The blind-model author may start after WP-1.

**Owners of shared files.** WP-2 owns the features, `refapp/Cargo.lock` and the `Variant` skeleton. The generic refactors are owned by WP-3a (flight downlink), WP-3c (flight uplink), WP-4a (ground downlink) and WP-4c (ground uplink); no other WP edits those modules.

### WP-0 Plan (S, after the entry gate) — `docs/milestones/a4-variants.md`, xtask and CI wiring
- [ ] Plan in the shape of `a3-cop1.md`: D1–D13 as accepted, WPs with owners, stacking and the parallel set, exit table, risks, API feedback
- [ ] Non-strict `trace --scope a4` in the `CI` list, the check `spec` (inputs declared) and `ci.yml`
- [ ] Plan link in `docs/README.md` and in the roadmap §3 A4 row
- [ ] `spec-lint` and the default `check` tier pass; no requirement counts written
- [ ] *(coordinator)* `status-table --write`

### WP-1 Scenario additions (M, docs only; after WP-0) — `docs/reference-application.md`, `docs/09-open-issues.md`
- [ ] §4.3 `aos` row rewritten to D4 and `uslp` row to D6 (single MAP, unsegmented, deviation stated). APP-031, APP-034 and APP-040 to APP-042 marked as TM/TC rules. The pull request lists APP-045
- [ ] AOS table as new APP IDs, citing 732.0-B-5: Tables 5-1 to 5-4, §4.1.2.2.3, §4.1.2.5, §4.1.2.6, §4.1.3, §4.1.4.2, §4.1.5.2, §4.1.6, §4.2.5, §4.3
- [ ] USLP downlink table, citing 732.1-B-3: Tables 5-1 to 5-5, §4.1.2.3.3, §4.1.2.7, §4.1.2.8 Table 4-1, §4.1.2.11–12, §4.1.4.1.5–11, §4.1.4.2.2–4, §4.1.5.2.1, §4.1.5.3, Annex B, §4.2.8–10; and 133.1-B-3 §4.1.2.3
- [ ] USLP uplink table, citing 732.1 §4.1.2.3.3, Table 4-1, §4.1.2.11.3, §4.1.2.12.3–5, §4.1.4.2.2.2.8 and Table 4-3, §4.2.7.4, §4.3.11; 232.0 §4.1.3.3 and §4.2; 232.1 as in A3. Includes its own Maximum Transfer Frame Length row, with the 309-octet fit arithmetic
- [ ] New IDs for FARM-1 over USLP, for the CLCW in the AOS `VC_OCF`, and for the CLCW in the USLP MC OCF (APP-146 and APP-147 are TC-specific)
- [ ] APP-138 and §7.4 amended per D10; `variant` added to §6.5; cases added to §6.4; new §4.6 with hand-derived schedules for both cases (every command arrives before its execution time)
- [ ] Open readings (USLP downlink QoS, Source-or-Destination, rule `111` content) each settled by a clause or filed as an OI with a recommendation; no OI resolved silently
- [ ] UPIDs by SANA name only; no Blue Book text copied; `spec-lint` passes; checklist "A spec-readings pull request before a codec"

### WP-2 Harness, features, variant skeleton (M, alone; after WP-1) — refapp `Cargo.toml` ×3, `refapp/Cargo.lock`, both node `lib.rs`, harness, scenario, generated files
- [ ] `aos` and `uslp` features added once; lockfile updated once; `thumbv7em` build of refapp-flight green
- [ ] `Variant` enum and `new_cop1(variant, …)` dispatch in both nodes; the `aos` and `uslp` arms return `Unsupported` until their WP lands
- [ ] `Case.variant` defaults to `tm_tc`; `aos`, `uslp_down` and `uslp_up` sections with range checks; uplink unit size validated per variant; unknown variant rejected
- [ ] Schema and `generated.rs` regenerated, never edited by hand
- [ ] `frame.rs` strictly decodes AOS, USLP fixed-length and USLP variable-length frames, and the CLCW
- [ ] All `a0-*` to `a3-*` traces byte-identical; scripted node pairs reproduce at `cop1`/`aos` and at `cop1`/`uslp`
- [ ] Tests have `// req:`; default tier passes

### WP-3a Flight `FrameDownlink<P>` and AOS (M; after WP-2; parallel set) — `refapp-flight/src/frames_down.rs`, one `lib.rs` arm
- [ ] TM traces unchanged
- [ ] AOS: FHEC and FECF in LNK-035 order; `VC_OCF` = `Farm1::clcw` in every frame; idle slots are an idle M_PDU on VC 7; VC counts start at 0; no OID frame in nominal
- [ ] `no_std`, no heap, `thumbv7em` green, features through the facade only; `// req:` tests

### WP-3b Flight USLP downlink (M; stacked on WP-3a)
- [ ] OID frames carry the MC OCF; partial TFDZs end in one Encapsulation Idle Packet; UPID from the SANA table; Source-or-Destination per WP-1
- [ ] TM and AOS unchanged; `thumbv7em` green; `// req:` tests

### WP-3c Flight `FrameUplink<P>` and FARM-1 over USLP (M; after WP-2; parallel set) — `refapp-flight/src/frames_up.rs`, one `lib.rs` arm
- [ ] `FrameReceiver<Uslp>` with strict decode, then `Farm1Frame` built per D7. `MapReassembler<Uslp>` on one MAP (MAP 0), whole SDUs plus blocked packets. Capacities from the `uslp_up` constants
- [ ] Tests: in-sequence AD, E3, E4, E5, Unlock, Set V(R), CLCW after each, blocked commands, whole table-upload SDU; a BC frame carrying another TFDZ is E9
- [ ] `api-feedback` filed for `Farm1Input::from_uslp` and for the single-MAP `Fop1Target::uslp`, both cited at the workaround
- [ ] TC path unchanged; `thumbv7em` green

### WP-4a Ground downlink generic and AOS (M; after WP-2; parallel set) — `refapp-ground/src/frames_down.rs`, one `lib.rs` arm
- [ ] `FrameReceiver<Aos>` and `PacketExtractor<Aos>` on VCs 0, 1 and 7; idle packets dropped; CLCW from `VC_OCF` to FOP-1
- [ ] APP-138 events as amended (no scope-0 `frame_gap`, no per-frame `frame_discarded`); TM unchanged; APP-103 lints; `// req:` tests

### WP-4b Ground USLP downlink (M; stacked on WP-4a)
- [ ] Fixed-length `FrameReceiver<Uslp>`; extractor PVNs {SPP, Encapsulation}; idle packets dropped
- [ ] An OID frame on VCID 63 produces no packet event, and its CLCW reaches FOP-1

### WP-4c Ground uplink generic over `Fop1Format`, FOP-1 USLP target (M; after WP-2; parallel set) — `refapp-ground/src/frames_up.rs`, `frames_up/ad.rs`, one `lib.rs` arm
- [ ] `Fop1Target::uslp` with one MAP; `MapSegmenter<Uslp, 1, …>` with MAP 0 blocking, segmentation prohibited, `max_fdu_len` from the target; USLP types sized from the `uslp_up` constants; TC constants untouched
- [ ] The TFDF of a whole SDU is the FOP-1 request; AD frames carry N(S) in the VCF Count; BC frames carry the "COP-1 Control Commands" UPID; the table upload goes in one frame; commands released together are blocked into one TFDZ; one frame per slot
- [ ] Tests: nominal acknowledgement, blocking, whole-SDU upload, byte-identical retransmission (COP-011), BC before AD
- [ ] APP-103 lints; TC path unchanged

### WP-5 Golden traces (M; after WP-3a to WP-4c) — `scenario.json`, `refapp/golden/a4-*.jsonl`, `refapp/model/`, `cross-checks.md`
- [ ] Both traces blessed and reproduced by `compare` in CI
- [ ] Packet projection equals `a1-nominal`; `packet_out` instants equal `a3-nominal`
- [ ] The WP-1 schedules match the traces, or each divergence is resolved against the Blue Book (never by tuning)
- [ ] The first frame of each kind hand-checked in the pull request, with clauses
- [ ] Blind model briefed with only `docs/reference-application.md` and the Blue Books (no `refapp/` or `crates/` paths); mismatch count recorded in `cross-checks.md`

### WP-6 Strict decoding of `a4-*` traces (S; after WP-5) — `refapp-harness/tests/trace.rs`
- [ ] Every `unit_sent` strictly decoded (AOS and USLP down, USLP up, CLCW, inner packets; uplink through FARM-1 and the reassembler)
- [ ] A planted bad USLP FECF and a planted bad AOS FHEC each fail the check; result recorded in the pull request's Evidence section

### WP-7 Exit (S; after WP-6) — VER-020
- [ ] `trace --scope a4 --strict` replaces the non-strict run in `cargo xtask ci`, `ci.yml` and the check `spec`
- [ ] Results column of §4, status line, roadmap §3 and §4, READMEs, and the A5 entry gate updated
- [ ] Every `api-feedback` and follow-up issue closed or deferred with a disposition (#490 D-e); process metrics reviewed (VER-063); no release
- [ ] *(coordinator)* `status-table --write`

## 4. Exit criteria

A4 is complete when every item below is green on `main`. The requirements WP-1 adds join this table in its pull request. The artefacts of each requirement are listed by `cargo xtask trace --scope a4`, not repeated here.

| Area | Criteria | Results / evidence |
|---|---|---|
| Variants exist | APP-045, APP-187 | Met. `refapp/refapp-harness/tests/variants.rs` (the `variant` field, its ranges and rejections, WP-2 #699); `refapp-harness/tests/a4_golden.rs` (APP-045, APP-188 packet projection); the `variant` entries of `refapp/scenario/scenario.json:407` and `:425` |
| Packet projection | APP-003 for both `a4-*` cases; the cases of APP-188 | Met. `refapp-harness/tests/a4_golden.rs` (projection of both `a4-*` cases equals `a1-nominal`; `packet_out` instants equal `a3-nominal`; WP-5 #725) |
| AOS downlink | APP-169…APP-174 (APP-173 is the CLCW in the `VC_OCF`) | Met. `refapp/refapp-flight/src/frames_down/aos.rs` (WP-3a #706), `refapp-ground/src/frames_down/tests_aos.rs` (WP-4a #707); `refapp-harness/tests/a4_golden.rs`; golden trace `a4-aos-nominal` |
| USLP downlink | APP-175…APP-180 (APP-179 is the CLCW in the MC OCF) | Met. `refapp/refapp-flight/src/frames_down/uslp.rs` (WP-3b #715), `refapp-ground/src/frames_down/tests_uslp.rs` (WP-4b #716); `refapp-harness/tests/a4_golden.rs`; golden trace `a4-uslp-nominal` |
| USLP uplink and COP-1 | APP-181…APP-184 (APP-182 is the Maximum Transfer Frame Length with the fit of the 309-octet packet); APP-044 at `uslp` | Met. `refapp/refapp-ground/src/frames_up/tests_uslp.rs` (WP-4c #720: FOP-1 USLP target, blocking, whole-SDU upload, retransmission); `refapp-flight/src/frames_up/tests/uslp.rs` (WP-3c #721); golden trace `a4-uslp-nominal` |
| FARM-1 over USLP | APP-185 (the flight node's events for the USLP uplink: APP-186) | Met. `refapp/refapp-flight/src/frames_up/uslp.rs` and its tests `frames_up/tests/uslp.rs` (WP-3c #721); golden trace `a4-uslp-nominal` |
| Events | APP-138 as amended (D10); APP-186 (the `GapDiscard` code 115) | Met. `refapp-ground/src/frames_down/tests_aos.rs` and `tests_uslp.rs` (APP-138 as amended); `refapp-flight/src/frames_up/tests/uslp.rs` (APP-186, code 115) |
| Harness | APP-071, APP-075, APP-076, APP-090, APP-100, APP-107 at the variants | Met. `refapp-harness/tests/variants.rs`, `variant_frames.rs`; the case list inspected against `scenario.json` ([record](../verification/records/APP-076.md), added by WP-7); `refapp-deps` for APP-100 ([record](../verification/records/APP-100.md)) |
| Golden traces | APP-082, APP-083, APP-086 for every `a4-*` trace | Met. `refapp/golden/a4-aos-nominal.jsonl` and `a4-uslp-nominal.jsonl`, blessed and frozen (WP-5 #725); `refapp-harness/tests/trace.rs` decodes every `unit_sent` strictly, with a planted bad USLP FECF and a planted bad AOS FHEC failing the check (WP-6 #728); the blind model `refapp/model/a4_cases.py` matches every record of both cases ([cross-checks.md](../verification/cross-checks.md) §7.4, #724) |
| Reproduction in CI | `compare` on every `a4-*` case (APP-106); `cargo xtask trace --scope a4 --strict` passes; the `thumbv7em-none-eabihf` build of `refapp-flight` is green | Met. `cargo run -p refapp-harness --locked -- compare` matches all 17 golden cases, both `a4-*` among them, with no trace, vector or scenario change in WP-7 (verify check `refapp compare`, APP-106); `cargo xtask trace --scope a4 --strict` reports every in-scope requirement traced and replaces the non-strict run in `cargo xtask ci`, `ci.yml`, and the check `spec`; `cargo build -p refapp-flight --target thumbv7em-none-eabihf --locked` is green (WP-7, 2026-11-20), and the bare-metal build and clippy are in the `refapp` job of `ci.yml` |
| New requirements | APP-169…APP-189, added by WP-1 (APP-187 and APP-189 select the variant and place its parameters in the scenario file; the others are in the rows above) | Met. APP-169…APP-189 added by WP-1 (#697); each is in `cargo xtask trace --scope a4 --strict`, which lists the artefacts of every one |
| Follow-up issues | Every A4 `api-feedback` and review follow-up issue closed, or deferred to a named target | Met. The `api-feedback` issues of A4 (#718, #719, #708) have a disposition in §6 (maintainer, 2026-11-20); #712 and #713 are follow-ups of reviews of #709 and #710, track-R work, not A4 issues |
| Docs | Roadmap §3 and §4, the READMEs and the A5 entry gate updated | Met. This plan, roadmap §3 and §4, the A5 entry gate, `docs/README.md`, the root `README.md` status and `refapp/README.md` updated by WP-7 |

## 5. Risks

| Risk | Mitigation |
|---|---|
| The single-MAP USLP uplink (D6) is a visible deviation that A5 and ACFS will copy | WP-1 records the deviation with its reasons (#235, OI-71); the two-MAP and segmented follow-up is named in §6 |
| The generic refactor could disturb the `a2-*` and `a3-*` traces | `compare` runs on every pull request (D11); WP-2 lands the skeleton first; each refactor has one owner (§3) |
| The hand-built `Farm1Frame` (D7) duplicates `Farm1Input::from_tc` | Bounded by WP-6 and the `api-feedback` issue (D13) |
| Frozen traces freeze any defect in them, and later milestones copy it | D9: strict decoding, a hand check of the first frame of each kind, a blind model, schedules derived by hand in WP-1 |
| A Blue Book clause number, the 512-octet fit, the downlink QoS or a SANA UPID name cited here is wrong: none is verified yet | WP-1 reads each clause and settles it or files an OI; nothing is written from memory (OI-06) |

## 6. API feedback found

Each finding is a GitHub issue labelled `api-feedback` (APP-101). A4 plans no engine change. #712 and #713 are not A4 issues: they come from reviews of #709 and #710, which are track-R work.

| Issue | Found in | Finding | Disposition |
|---|---|---|---|
| #718 | WP-3c | `Farm1Input::from_uslp` is missing; the node builds `Farm1Frame` by hand (D7) | Deferred to the A5 plan (maintainer, 2026-11-20) |
| #719 | WP-3c, WP-4c | `Fop1Target::uslp` takes a single MAP (D6); #235 and OI-71 are the open library questions | Deferred to the A5 plan, naming #235 (USLP segmentation) (maintainer, 2026-11-20) |
| #708 | WP-4a | `FrameReceiver<Aos>` routes on the uncorrected Primary Header, not the FHEC-corrected one; found in #707 | Deferred as a track-R library finding to the next track-R milestone, or post-v1 if none is planned (maintainer, 2026-11-20) |

Not covered by A4: the C ABI FOP-1 handle with the USLP target (A5 plan); per-FDU TFDF headers and a per-request MAP ID (#235); EPP user data (OI-06); USLP truncated frames, the Insert Zone, AOS B_PDU and VCA; fault cases for the variants.
