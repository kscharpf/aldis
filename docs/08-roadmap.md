# 08 — Roadmap

v1 is built in two active tracks against one specification (Track C, native C++, was retired from v1 and deferred to roadmap Phase 3; archived at git tag `c0-archive`). Each track has its own milestones and can be worked on independently, but a track milestone starts only when its **entry gate** (results merged on `main` from other tracks) is met.

| Track | Purpose | Milestones | Directories it owns |
|---|---|---|---|
| **R — Rust reference** | The reference implementation and the recommended choice: Rust crates, the C ABI over them (`aldis-ffi`), and the C++ engine-handle wrapper | M0–M6 (§1) | `crates/`, `fuzz/`, `exerciser/`, `crosscheck/`, the generated C header |
| **A — Reference application** | An end-to-end flight↔ground application that uses only the public APIs, grown one milestone at a time. It gives API feedback before v1 freezes and is the integrators' starting point | A0–A6 (§3) | `refapp/` |
| **C — Native C++ (Phase 3)** | Retired from v1 and deferred to roadmap Phase 3 (archived at tag `c0-archive`) | C0–C5 (§2) | — |

§4 shows how the tracks overlap, §5 defines the releases, and §6 the rules that let separate contributors or agents work on the tracks at the same time.

## 1. Track R — Rust reference (M0–M6)

Every milestone defines the **C ABI for what it adds**: a codec milestone adds its codecs' functions to `aldis.h`, and an engine milestone adds its engines' handles.

| Milestone | Content | Entry gate | Exit criteria |
|---|---|---|---|
| **M0 Foundations** ([plan](milestones/m0-foundations.md)) | Repository + GitHub Actions CI, lint set, exerciser/panic-link check, vector harness, `xtask` trace/spec tooling, `aldis-core` (errors, bit fields, IDs, time, CRC-16, OID generator, counters, containers, security hooks, frame layout) | — | **Complete** (2026-10-16, `v0.1.0-m0`); exit criteria in the plan §4 |
| **M1 Packets & time** ([plan](milestones/m1-packets-time.md)) | `aldis-time`, `aldis-spp`, `aldis-epp` | M0 | **Complete** (2026-10-20, `v0.2.0-m1`); exit criteria in the plan §4 |
| **M2 Frame codecs** ([plan](milestones/m2-frame-codecs.md)) | **WP-0:** the M1 codecs' part of the C ABI (time, SPP, EPP: `aldis-ffi` functions and generated header, OI-21), the field-code catalogue with `FieldId` carrying its code (API-082, OI-27), and the shared property-test strategies in `aldis-testkit` (VER-054). Then `aldis-tm`, `aldis-tc` (incl. CLCW, delimiting, validation), `aldis-aos` (incl. FHEC, editions), `aldis-uslp` (incl. truncated), each with its C ABI functions | M1 | **Complete** (2026-10-27, `v0.3.0-m2`); exit criteria in the plan §4 |
| **M3 COP-1** | `aldis-cop1` FOP-1 + FARM-1, state tables as data, closed-loop tests; protocol states as closed enums whose transition functions are generated from, or checked against, the state tables, and the engines' lifecycles as typestates (RS-021); a decision on hardening the engines' long-lived state against single-event upsets beyond GEN-015 ([hardware assumptions](hardware-assumptions.md) §4); FOP-1/FARM-1 handles in the C ABI, with the capacities configuration (CABI-030) | M2 | One vector per table cell; two-person table review recorded. **Released** as `v0.4.0-m3` (2026-10-31, [plan §4](milestones/m3-cop1.md)) |
| **M4 Link engines** | `aldis-link` generators, mux, receiver, extractor, segmenter/reassembler, CLTU packer, each with its C ABI handle ([plan](milestones/m4-link-engines.md)) | M3 | LNK requirements traced; property tests for FHP placement and reassembly. **Complete** (2026-11-04, [plan §4](milestones/m4-link-engines.md)); released `v0.5.0-m4` (2026-11-05) |
| **M5 C ABI completion** ([plan](milestones/m5-c-abi-completion.md)) | CADUs and the TM randomizer (OI-12): a new Layer 1 crate for ASM attach and search and the TM pseudo-randomizer (131.0 §9–10), with its C ABI; C test harness with sanitizers and Miri (VER-026), header compile checks (VER-050), ABI versioning (CABI-060), and the header-only C++ wrapper `aldis.hpp` of the engine handles (CABI-072) | M4 | All CABI requirements traced; codec vectors pass via the C ABI, engine-scenario vectors also via `aldis.hpp`. **Complete** (2026-11-06, [plan §4](milestones/m5-c-abi-completion.md)); released `v0.6.0-m5` (2026-11-06) |
| **M6 Crate architecture simplification** ([plan](milestones/m6-architecture.md)) | One checking pipeline in `aldis-core` (channel lookup, `check`, generic proof types, generic frame stream), one frame-format trait per codec that the mux and FOP-1 use, one frame writer, and a generic C ABI over the engines (`InPlace`, one dispatcher); each protocol fact stated once, no wire behaviour changed | M5 | Phases 1–3 merged; `cargo xtask trace` unchanged in requirement coverage. **Phases 1–2 merged** (2026-11-10, #472); Phase 3 (WP-3) and the exit (WP-4) deferred until after the interoperability milestone of §3 (maintainer, 2026-11-10) |

Layer 3 (`aldis-framework`) is not part of v1: it has no crate in the workspace (removed 2026-11-08, maintainer decision), and it will be Rust-only. The framework is deferred work: see the "later" row of §7.

## 2. Track C — Native C++ (Retired from v1, deferred to Phase 3)

Track C (native C++, C0–C5) was retired from v1 and deferred to roadmap Phase 3 (`chore/retire-track-c`). The C0 foundations code was archived at git tag `c0-archive`. All 34 `CPP-*` requirements and `VER-052`/`VER-053` were withdrawn.

## 3. Track A — Reference application (A0–A6, AINT, ACFS)

The reference application lives in its own workspace, `refapp/`, and uses only the public APIs, as an integrator would. Its scenario is specified in [reference-application.md](reference-application.md) (area APP). Its flight and ground sides talk over a loopback link owned by a harness: the Rust nodes run in-process, and the A5 C++ and A6 QEMU flight nodes run as child processes over a lockstep node protocol ([OI-24](09-open-issues.md), [reference-application.md](reference-application.md) §8.2), as does the ACFS cFS flight node, over the same protocol on a loopback TCP stream (APP-118, [reference-application.md](reference-application.md) §10.6, [OI-38](09-open-issues.md)). The engines are deterministic (GEN-030), so each run of a scenario produces a **golden trace** of every data unit exchanged. Each implementation must reproduce the trace byte for byte.

| Milestone | Content | Entry gate | Exit criteria |
|---|---|---|---|
| **A0 Scenario** ([plan](milestones/a0-scenario.md)) | The scenario specification, a new document [`docs/reference-application.md`](reference-application.md) with requirement area APP: flight and ground roles, APIDs, virtual channels, rates, injected faults, and the golden-trace format. `refapp/` workspace skeleton with a CI build job | — | Scenario approved by the maintainer (exited 2026-11-06) |
| **A1 Packets** | Flight side produces housekeeping Space Packets with CUC time stamps; ground side decodes them; first golden trace | A0; M1 | Trace reproduced in CI (exited 2026-11-07; [plan](milestones/a1-packets.md)) |
| **A2 Frames** | Downlink TM frames and uplink TC frames, multiplexed and demultiplexed by the `aldis-link` engines (frame engines and multiplexers; [D3](milestones/a1-packets.md)) | A1; M4 | Trace reproduced in CI; API issues found are filed. **In progress** |
| **A3 COP-1** | FOP-1 on the ground, FARM-1 on the flight side, CLCW in the OCF, through the COP-1 engines; injected frame loss exercises retransmission | A2; M4 | Trace reproduced in CI, including the fault cases |
| **A4 Variants** | The AOS and USLP variants of [reference-application.md](reference-application.md) §4.3, on the same engines. The engines are used from A2 on, so A4 no longer replaces loops | A3; M4 | `a4-*` traces reproduced in CI, with the packet projection of the nominal case (APP-003) |
| **A5 C and C++** | A C++ flight side using `aldis.hpp` for the engine handles and the C header for the codecs, built against the Rust-backed library (`aldis-ffi`) | A4; M5 | The Rust and Rust-via-C-ABI builds reproduce the same golden trace |
| **A6 Target** | The flight side runs bare-metal under QEMU; footprint, stack, and timing figures published. The application keeps its own run loop; the move onto `aldis-framework` stacks and trace/replay is after v1 (§7) | A5 | End-to-end ground↔flight demo; figures included in the v1.0 tables |
| **AINT Interoperability** | The A3 Rust flight node exchanges TM and TC frames and COP-1 (FOP-1 on the ground system's side, FARM-1 on the flight side) with an independent ground system run as a black box, over a real transport driver that steps the node against a socket and a clock, outside the crates (the only new I/O; the crates stay sans-I/O, GEN-030–032). Candidate implementations: YAMCS (recommended) or OSDLP; the plan's D1 chooses, after checking the licence against VER-055 (YAMCS is AGPL-3.0: used as a black box, never read at source level, [decision-rights](process/decision-rights.md) §2.8). Every discrepancy is filed as an open issue or reported upstream by the maintainer. The plan is written before the milestone starts (§6 rule 1): `docs/milestones/aint-interoperability.md`, trace scope `aint` | A3 | The nominal and lossy-link COP-1 cases pass against the independent implementation; every discrepancy filed. Gates no release |
| **ACFS cFS flight side** ([plan](milestones/acfs-flight-side.md)) | A second flight side: NASA cFS apps (cFE, OSAL, pc-linux PSP) that call Aldis through `aldis.h`, run in lockstep with the harness, and reproduce the Rust flight side's golden traces ([reference-application.md](reference-application.md) §10). Three phases, below | Per phase, below | Every phase's traces reproduced in the nightly job; gates no release |

ACFS and AINT are independent: ACFS replaces the flight side through the C ABI, AINT replaces the ground side. Running the cFS node against the independent ground system is an optional later cross-check that no phase requires.

ACFS phases ([plan](milestones/acfs-flight-side.md) D1). Each phase follows one Rust-side milestone of track A and the C ABI it needs:

| Phase | Content | Entry gate | Exit criteria |
|---|---|---|---|
| **P1 Packets** | cFS apps exchange Space Packets with the harness; command ingest and telemetry output through the M1 codecs' C ABI | A1; the M1 codecs' C ABI (M2 WP-0c, #72) | `a1-*` traces reproduced in nightly CI |
| **P2 Frames** | TM and TC frames through the C ABI handles of the `aldis-link` engines | P1; A2; the M4 engine handles in the C ABI | `a2-*` traces reproduced in nightly CI |
| **P3 COP-1** | FARM-1 through its C ABI handle | P2 (so M4); A3; the M3 FARM-1 handle | `a3-*` traces reproduced in nightly CI, fault cases included |

The alternative to one milestone with phases is three milestones, one per row; the plan's D1 compares them.

## 4. Overlap

Each row is a set of milestones expected to run at the same time. The rows are not time boxes: a track milestone starts as soon as its entry gate is met.

Order of work (maintainer, 2026-11-10):

1. Finish A2 (its remaining work packages WP-5, WP-6, WP-7).
2. A3 COP-1, starting with its plan; its headline case is a COP-1 retransmission over a lossy link.
3. AINT, interoperability against an independent ground system (YAMCS recommended, OSDLP the alternative; the plan's D1 decides); entry gate A3.
4. M6 Phase 3 (WP-3a–WP-3d) and the M6 exit (WP-4) after AINT; the OI-93 follow-up of WP-2d may still land before A2's last work packages (the coordinator's call).
5. In parallel, gated on nothing above: a README rewrite for a first-time reader.

| Stage | Track R | Track A |
|---|---|---|
| 1 | M1 (complete, `v0.2.0-m1`) | A0 |
| 2 | M2 (complete, `v0.3.0-m2`) | A1 |
| 3 | M3 | |
| 4 | M4 | A2 (entry gate M4, D3) |
| 5 | M5 (complete, `v0.6.0-m5`) | A2 (in progress) |
| 6 | M6 Phases 1–2 (merged) | A3 |
| 7 | | AINT |
| 8 | M6 Phase 3 and exit | A4, A5 |
| 9 | | A6 |
| 10 | v1.0 release (M6 and A6 exited) | |

## 5. Releases

| Release | Contents | Gate |
|---|---|---|
| `v0.N.0-mN` | Rust crates after milestone MN (as today) | MN exit |
| **v1.0** | Rust crates, C ABI, `aldis.hpp`; docs, trace matrix, coverage waivers, stack-usage tables, changelog | M6 and A6 exited; all v1 R-track requirements verified (the FW requirements are later). The `CHANGELOG.md` sections of 0.5.0-m4 and 0.6.0-m5 are written first ([#294](https://github.com/kscharpf/aldis/issues/294)) |

A milestones do not cut releases. Their exit is recorded in their milestone plans.

## 6. Working in parallel

These rules let separate contributors, or separate agents, each own one track.

1. **Plans first.** Before a milestone starts, its plan is written in `docs/milestones/` (`m<N>-…`, `c<N>-…`, `a<N>-…`) with an Area/Criteria exit table, so that `cargo xtask trace --scope <id>` measures it. The plan records the entry gate as met, with the PRs that met it. While the milestone runs, the plan's status is a short sentence (state, entry gate, decisions accepted) and a **work-package status table**, one row per WP or sub-part, with the columns WP, State (planned, in progress, or merged), PRs, and Notes; the State and PRs columns are derived from the pull requests of GitHub by `cargo xtask status-table [--write]` (a coordinator tool, not a check of the gate), which finds a PR by the milestone and work package its title scope names ([pr-body.md](process/pr-body.md)); a PR changes only the Notes column of its row. The history of decisions and open issues goes in a short list below the table. Plans quote no counts that the tooling computes: `cargo xtask trace --scope <id>` reports how many in-scope requirements are traced. The definition of done for the recurring kinds of work (a codec module, a crate, a vector file, a fuzz target, C ABI functions, a spec-readings PR, a milestone exit) is in [milestones/checklists.md](milestones/checklists.md).
2. **Ownership.** A PR changes the directories of one track (table at the top), plus shared files. A change needed in another track's directories is a separate PR on that track's branch prefix. Shared files are `docs/`, `test-vectors/`, `xtask/`, `.github/`, `AGENTS.md`, and `README.md`. Changes are recorded by the PR title (a conventional commit), never in `CHANGELOG.md` itself: `cargo xtask changelog` generates the unreleased section from the merged titles, so changelog entries never conflict.
3. **Branches.** `m<N>/<topic>`, `c<N>/<topic>`, `a<N>/<topic>` for track work; `spec/` for a spec-readings PR; any other PR takes the conventional-commit type of its title as its prefix (`docs/process-retro` for `docs(process): …`, any type of [pr-body.md](process/pr-body.md)). Every PR targets `main`, except a stacked PR (rule 10) until the PR below it merges; there are no long-lived track branches.
4. **The C header belongs to track R.** It is generated from `aldis-ffi` (CABI-002). Track C or A asks for a change through an issue, and track R makes it.
5. **The Rust crates decide.** When a native implementation diverges from the Rust crates, the Blue Book decides which is wrong (VER-053). If the Rust crates are wrong, the fix is a track-R PR with a regression vector, and the C++ PR waits for it. The divergence is listed in the C++ milestone plan until then. Native code never changes the spec to match itself.
6. **Vectors.** Any track may add vectors, following [07](07-test-vectors.md). A vector merges only with the Rust crates passing it.
7. **IDs.** Area CPP belongs to track C and area APP to track A; the other areas are shared. Requirement and OI numbers are allocated when a PR is written. If two open PRs take the same number, the one that merges second renumbers (`cargo xtask spec-lint` reports duplicates); two OI rows with the same title are one issue written twice, and the second PR keeps one row. Field codes are settled the same way, but each new crate takes them from a block of its own, reserved when its plan is accepted ([03 §9.1](03-api-model.md#91-field-codes)), so pull requests for different crates never need to renumber them; only the common block of shared fields can still clash.
8. **Open issues.** A track that hits an open spec question adds or updates an OI in [09](09-open-issues.md) instead of choosing silently in code.
9. **Merge gate.** A PR merges on the published record of a passing verification run, `cargo xtask verify` on the contributor's machine or the `verify` workflow started by hand ([06 §7](06-verification.md#7-verification-runs-and-the-merge-gate), VER-058, VER-060); GitHub Actions repeats every check each night (VER-059). How to run it: [AGENTS.md](../AGENTS.md#toolchain-and-checks).
10. **Stacking.** Stack a PR on another when the two merge in order and the second needs the first. Branch from the first PR's branch and open the second against that branch; GitHub retargets it to `main` when the first merges. Never push to the first PR's branch. A stacked PR rides the same merge train as its base, after it: `scripts/train` refuses it unless the PR of its base branch is earlier in the same train, and the train PR is what gets verified ([coordinator.md](process/coordinator.md#merge-trains), VER-058). If the first PR changes or is rejected, rebase the second onto the new base. Independent PRs on different files gain nothing from stacking. When both PRs append a row to the open-issue table ([09](09-open-issues.md)), the merge of `main` keeps both rows, in number order; if they carry the same number, `spec-lint` fails and the PR that merges second renumbers its row and references by hand.

## 7. Later standards (proposed order)

After v1, SDLS comes before the framework (maintainer, 2026-11-06). The TM codes follow in v1.2 (maintainer, 2026-11-06). The v1.1 and v1.2 rows are decided; the rows after them are the proposed order.

| Release | Standards | Rationale |
|---|---|---|
| v1.1 | Minimal TC sync/coding: CLTU (BCH) build/parse and the TC randomizer (231.0 §3, §5–6); the TM part (ASM, randomizer) is in M5 (OI-12), and the UTC/TAI leap-second table trait | Lets v1 frames reach real radios without an external modem library |
| v1.1 | 355.0 SDLS baseline: the 355.0 procedures with the 352.0 baseline algorithms (AES-GCM authenticated encryption). Cryptography goes through a pluggable provider trait: no third-party runtime dependency in Layers 0–2 (GEN-001), a test provider for the vectors, real providers outside the core. Scope and provider boundary: OI-84, OI-85 | Maintainer decision 2026-11-06: SDLS before the framework. Hooks already reserved in v1 (API-070…075) |
| v1.1 | Carry-overs from M4: the FOP-1 hand-off of Segment Header FDUs and per-FDU TFDF headers ([#235](https://github.com/kscharpf/aldis/issues/235)); the FOP-1 decision committed to another engine or after a state change ([#254](https://github.com/kscharpf/aldis/issues/254)); the `FrameReceiver` stripping the SDLS Security Header and Trailer ([#230](https://github.com/kscharpf/aldis/issues/230)); the `scripts/test-reverify` flake ([#261](https://github.com/kscharpf/aldis/issues/261)); the `cargo xtask metrics` review count ([#287](https://github.com/kscharpf/aldis/issues/287)); from the M5 retrospective: the C vector harness beyond CRC-16, SPP, EPP, TM, FARM-1 and sync ([#289](https://github.com/kscharpf/aldis/issues/289)), CERT C++ analysis of `aldis.hpp` ([#278](https://github.com/kscharpf/aldis/issues/278)), the rise of the verify run time ([#290](https://github.com/kscharpf/aldis/issues/290)), agent-worktree cleanup ([#295](https://github.com/kscharpf/aldis/issues/295)), a guard against pushing to a merged PR's branch ([#291](https://github.com/kscharpf/aldis/issues/291)) | #235 was planned for M4 WP-7 and did not land. #254 changes the API and the C ABI; the fix option is the maintainer's, decided when the work starts. #230 is done with the SDLS baseline (row above). #261 is fixed before v1.1 work starts |
| v1.2 | TM Reed-Solomon (255,223) with interleaving, and convolutional coding (rate 1/2, k=7), from 131.0 | The codes the TM links use most; completes the TM side beyond the CADU of M5 |
| v1.2 | Carry-over from M4: allow selected lints in test code, revisiting `allow_attributes_without_reason` ([#258](https://github.com/kscharpf/aldis/issues/258)); from the M5 retrospective: `sync_ffi` fuzz throughput ([#292](https://github.com/kscharpf/aldis/issues/292)) and CodeQL, deferred in M5 ([#293](https://github.com/kscharpf/aldis/issues/293)) | Lint policy ([rust.md](profiles/rust.md) §5) |
| later | The framework `aldis-framework` (a new crate, created when scheduled; FW requirements of [05](05-framework.md); the FW-013 trace format extractable from, or sharing the record encoding of, the reference application's golden traces, OI-25), and the application's move onto its stacks and trace/replay. Not scheduled, and after SDLS (v1.1) and TM coding (v1.2). Done when the FW requirements are traced and a loopback stack is tested end to end | Builds its adapters on a released v1.0 C ABI. Taken up when a second user, or the application's move onto it, justifies it; the application's own run loop serves until then. The FW requirements are outside the v1 trace and the v1.0 gate (`DEFERRED_AREAS` in `xtask/src/spec.rs`) |
| later | 355.1 extended procedures (key and SA management over the link) | Needs the 355.0 baseline first |
| v2 | 727.0 CFDP (Class 1 and Class 2, sans-I/O, filestore trait) | Highest-demand application protocol |
| v3 | 131.0 LDPC and turbo, 231.0 LDPC (RS and convolutional are v1.2); 211.x Proximity-1 + COP-P | Compute-heavy; needs per-target optimization |
| v3 | 734.x DTN (BP/LTP), 702.1 IPoC | Networking |
| v4 | Ground formats: 502–509 navigation messages, 660.0 XTCE; SLE/CSTS (911–922, 913.1) | Ground segment; `std` likely |

Each later standard follows the active tracks: the Rust crate first, then its use in the reference application.

## 8. Language rollout

| Phase | Language | Approach |
|---|---|---|
| 1 | Rust | Reference implementation (track R); the recommended choice |
| 1 | C / C++ | C ABI over Rust (`aldis-ffi`) + header-only C++ wrapper of the engine handles (`aldis.hpp`, track R, M5, CABI-072) |
| 2 | Python | Bindings over the C ABI (or PyO3 over the Rust crates) with an idiomatic API; vectors run under pytest |
| 2 | Go | cgo bindings over the C ABI |
| 2 | Java / C# | Panama (FFM) or JNI / P/Invoke over the C ABI |
| 3 | Native C / C++ | Full native implementations of this specification; same C ABI and C++ wrapper API |

Every phase is gated on passing the shared vector suite (TV-*).

## 9. CCSDS document coverage

Which CCSDS document, and which of its sections, each milestone of track R covers. This is coverage at the level of documents and sections, written by hand; it is not requirement tracing. Editions are those of [`specs/manifest.tsv`](../specs/manifest.tsv). Per-requirement tracing is checked at each milestone exit by `cargo xtask trace --scope <id> --strict` and the plan's §4, and is not repeated here (rule 1 of §6).

Track A (A0–A6, AINT, ACFS) implements no CCSDS document: it uses the public APIs of the milestones below. M5 adds no CCSDS document either; the C ABI, its header, and the C++ wrapper expose what the milestones above implement.

| Milestone | CCSDS document | Sections covered | Coverage |
|---|---|---|---|
| M0 | 132.0-B-3 TM Space Data Link Protocol; 232.0-B-4 TC Space Data Link Protocol; 732.0-B-5 AOS Space Data Link Protocol; 732.1-B-3 Unified Space Data Link Protocol | The parts the four share ([common link features](standards/common-link-features.md)): the FECF CRC-16 (132.0 §4.1.6, 232.0 §4.1.4, 732.0 §4.1.6, 732.1 Annex B), the OCF classification (132.0 §4.1.5.4–5, 732.0 §4.1.5.4–5, 732.1 §4.1.5.2.1–2), the OID idle pattern (132.0 §4.1.4.6.2, 732.0 §4.1.4.1.5.2, 732.1 §4.1.4.1.10) | Partial: shared algorithms and types only; the frame codecs are M2 |
| M0 | 355.0-B-2 Space Data Link Security Protocol | Hooks that reserve and place the Security Header and Trailer (132.0, 232.0, 732.0, 732.1 §6.3; API-070…075) | Partial: placement hooks only; no SDLS procedure or cryptography (roadmap §7, v1.1) |
| M1 | 301.0-B-4 Time Code Formats | §3.1–§3.5 (CUC, CDS, CCS, ASCII), Annex A | Partial: agency-defined codes (§3.6) only as raw octets; TAI/UTC conversion (Annex D) and leap-second tables deferred ([time codes](standards/time-codes.md) §6, roadmap §7) |
| M1 | 133.0-B-2 Space Packet Protocol | §4.1, §4.2.2, §4.3.2.2, §5.2; §3.4 Octet String service | Full for the codec and its sequence-count and idle-packet helpers |
| M1 | 133.1-B-3 Encapsulation Packet Protocol | §4.1, §4.2.1.1.2, §4.3.1.1.3, §5 | Full |
| M2 | 132.0-B-3 TM Space Data Link Protocol | §4.1 frame, §5 managed parameters, §6.3 Security Header and Trailer placement | Partial: the codec; procedures (§4.2–§4.3) are M4; SDLS hooks only |
| M2 | 232.0-B-4 TC Space Data Link Protocol, with Cor. 1 | §4.1 frame, §4.2 CLCW, §5 managed parameters, the frame validity checks of §4.4.9 (All Frames Reception), §6.3 placement | Partial: the codec, CLCW, and validation; the sending-end procedures (§4.3) and the other receiving-end functions (§4.4.1–§4.4.8) are M4; SDLS hooks only |
| M2 | 732.0-B-5 AOS Space Data Link Protocol | §4.1 frame (FHEC, M_PDU, B_PDU, Insert Zone), §5 managed parameters, §6.3 placement | Partial: the codec; procedures (§4.2–§4.3) are M4; SDLS hooks only |
| M2 | 732.0-B-4 AOS Space Data Link Protocol, Issue 4 (Silver Book) | The Issue 4 differences of the header and M_PDU (AOS-002, AOS-003) | Partial: the `Issue4Compatible` edition only |
| M2 | 732.1-B-3 Unified Space Data Link Protocol | §4.1 frame, Annex D truncated frame, §5 managed parameters, §6.3 placement | Partial: the codec; procedures (§4.2–§4.3) are M4; SDLS hooks only; the PLCW (COP-P) is carried as an opaque OCF |
| M3 | 232.1-B-2 Communications Operation Procedure-1, with Cor. 1 | §3 inter-procedure interfaces, §4 service definition, §5 FOP-1 (variables, actions, state table), §6 FARM-1, §7 managed parameters | Full: FOP-1 and FARM-1 as step functions over the state tables |
| M4 | 132.0-B-3, 232.0-B-4, 732.0-B-5, 732.1-B-3 | The procedures at the sending and receiving ends: 132.0 §4.2–§4.3, 232.0 §4.3 and §4.4.1–§4.4.8 (the validity checks of §4.4.9 are M2), 732.0 §4.2–§4.3, 732.1 §4.2–§4.3 ([04](04-engines.md) §1) | Full: the procedures as sans-I/O engines; the FOP-1 hand-off of 232.1 §4.4; the CLTU packer only groups frames (232.0 §4.3.8.4–6, 732.1 §4.2.11.7–8), and the coding itself is out (231.0, v1.1; the TM ASM and randomizer are M5, OI-12) |
| M5 | 131.0-B-6 TM Synchronization and Channel Coding | §9 (ASM, CADU, ASM search) and §10 (pseudo-randomizer, both sequences of §10.4) ([TM synchronization](standards/tm-sync.md)); C ABI completion, header checks, ABI versioning, and the C++ wrapper over the documents above | Partial: §9 and §10 for the no-code, no-slicing CADU; the codes (§3–§8) and slicing are out of v1 (OI-82, roadmap §7) |

Documents the specification cites that no milestone of v1 implements: 131.0-B-6 TM Synchronization and Channel Coding (beyond the ASM and randomizer of M5, OI-12; codes and slicing) and 231.0-B-4 TC Synchronization and Channel Coding (minimal subset for v1.1, OI-12, §7), 355.0-B-2 procedures and the 352.0-B-2 baseline algorithms (v1.1), 355.1-B-1 procedures (later), 727.0-B-4 CFDP (v2), 211.0-B-6 Proximity-1 and COP-P (v3). The [glossary](glossary.md#3-blue-book-documents) lists every document and where Aldis specifies it.
