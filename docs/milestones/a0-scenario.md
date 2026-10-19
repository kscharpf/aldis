# Milestone A0 — Scenario

**Goal:** a reviewed specification of the reference application's scenario, in a new document `docs/reference-application.md` with requirement area APP, and a `refapp/` workspace skeleton built in CI. The scenario is designed once, here, so that A1 to A6 each extend it and never replace it, and so that every implementation can reproduce its golden traces byte for byte (GEN-030).
**Roadmap entry:** [08-roadmap.md](../08-roadmap.md) §3 (track A) · **Builds on:** [M0](m0-foundations.md) (`v0.1.0-m0`) tooling and CI only; A0 has no entry gate · **Track:** A, owns `refapp/`

**Status: plan approved (2026-10-19).** Entry gate: none (roadmap §3). Decisions D1–D16 accepted by the maintainer 2026-10-19. WP-0 and WP-1: [`docs/reference-application.md`](../reference-application.md) with area APP (APP-001…APP-108) is in review; its APP IDs are in the exit table (§4). OI-23 to OI-26 stay open for the WP-4 review.

## 1. Starting point

| In place | Missing for A0 |
|---|---|
| Roadmap §3 defines track A and the milestones A0–A6 with their entry gates | The scenario: roles, applications, APIDs, channels, rates, faults, golden-trace format |
| `aldis-core` released (`v0.1.0-m0`); `aldis-time`, `aldis-spp`, `aldis-epp` in progress (M1) | Requirement area APP, its row in [00 §5.2](../00-introduction.md#52-requirement-identifiers), and its document |
| Specifications for every v1 standard ([standards/](../standards/)), the engine contract ([03 §5](../03-api-model.md#5-engines-the-sans-io-contract)), engines ([04](../04-engines.md)), and framework ([05](../05-framework.md)) | `refapp/` workspace and its CI job |
| Separate-workspace pattern (`exerciser/`, `fuzz/`) and the pinned toolchain | `xtask trace` support for `refapp/`: today it scans only `crates/`, `xtask/`, `fuzz/`, and `exerciser/` for `// req:` |
| Test-vector model ([07](../07-test-vectors.md)) with engine scenarios (TV-010) | A trace format for whole-system runs; FW-013 names a framework trace but defines no format |

## 2. Decisions

All decisions were accepted by the maintainer on 2026-10-19. Values marked *indicative* are fixed in [`docs/reference-application.md`](../reference-application.md) (WP-0), where every protocol value cites its Blue Book clause.

| # | Decision | Choice | Status |
|---|---|---|---|
| D1 | Mission story and roles | A small Earth-orbiting spacecraft and its control centre, at a level an integrator recognises. **Flight node:** on-board time; a housekeeping application (periodic telemetry); an event reporter (command verification and fault events); a payload instrument (larger, lower-rate science packets); a memory-dump service (a long dump split into segmented packets); a command handler that executes immediate and **time-tagged** commands. **Ground node:** telemetry reception and decoding; packet-sequence and frame-gap monitoring; time-stamp decoding; a scripted operations timeline (the "ops script") that issues commands, a table upload, and a dump request; COP-1 sending end from A3. Both nodes are sans-I/O like the engines (D10). Alternative: a ground-only demo (decode recorded frames) — rejected, it exercises no sending path and no COP-1 loop | Accepted (2026-10-19) |
| D2 | Growth across A1–A6 | Table §2.1. Four rules keep the scenario a superset: (1) the **timeline** (what each application does, and when) is fixed in A1 and only extended; (2) the **packet projection** of the nominal case (every packet generated and delivered, its octets and order, without link timing) is identical from A1 to A6; (3) the **link-level trace** is frozen from A3 on: A4, A5, and A6 reproduce the A3 golden traces exactly; (4) later milestones **add cases**, they never redefine existing ones. Time-tagged commands (D1) make on-board behavior independent of uplink latency, which is what lets rule 2 hold when A2 adds frames and A3 adds retransmissions | Accepted (2026-10-19) |
| D3 | APID plan | Application APIDs are chosen freely in 0–2046: 133.0 §4.1.3.3.4.4 note 1 places no restriction except the Idle Packet APID 2047 (all ones), and 133.0 Annex B2.1 records the SANA APID policy as 0–2046 unmanaged (to be confirmed against the live registry, OI-06). One APID per managed data path (133.0 §4.1.3.3.4.2). Telemetry packets use Packet Type 0, commands Packet Type 1. Table §2.2 (*indicative* values). Idle Packets (APID 2047) appear only where the link procedures insert them (LNK-013) | Accepted (2026-10-19) |
| D4 | Downlink channel plan | **TM**, one master channel. Frame length **223 octets** (*indicative*): any length up to 2048 octets is valid uncoded (131.0-B-6 §11.3), and 223 also fits Reed-Solomon E = 16, I = 1 (131.0 §11.6.3) and the Turbo block lengths (131.0 §11.8.2), so the scenario stays valid when v1.1 adds coding ([roadmap](../08-roadmap.md) §7). FECF present (132.0 Table 5-1). **MC_OCF present** (132.0 Table 5-2), so every frame, OID frames included, carries the CLCW (LNK-033; 232.0 §4.2.1.1.1 note 3 asks for a minimum CLCW sampling rate). No FSH in the baseline. VCs in §2.3: real-time packets, payload packets, and a dedicated idle VC configured for packets (132.0 §4.1.4.6.3 requires a packet VC; its note 2 prefers a dedicated one). Mux: `FixedPriority` (LNK-031), real-time first. Partly filled data fields are released on a deadline and completed with an Idle Packet (LNK-012, LNK-013). OID frames follow the proposed default of **OI-11** (OID frame on the dedicated idle VC); if OI-11 is resolved otherwise, the scenario follows it with a new trace version (D9) | Accepted (2026-10-19) |
| D5 | Uplink channel plan and COP-1 | **TC**, one VC with Segment Header (232.0 Table 5-3), FECF present, maximum frame length well below 1024 (232.0 Table 5-2) so that the table upload needs segmentation. Two MAPs (232.0 Table 5-4): real-time commands (Packets, blocking permitted: commands released in the same uplink slot share one frame, LNK-040, LNK-071) and bulk upload (Packets, segmentation permitted, LNK-040, LNK-041). One frame per uplink data unit (232.0 Table 5-1 "Maximum Number of Transfer Frames Given to the Coding Sublayer as a Single Data Unit" = 1), so the A4 CLTU packer (LNK-044) produces the same units as the A3 loops. A2: Type-BD frames; the flight node's CLCW reports only the FARM-B Counter (COP-034 behavior, hand-written). A3 on: FOP-1 (ground) and FARM-1 (flight), retransmission allowed, W even (232.1 §6.1.8.3.1), T1_Initial at least the delay sum of 232.1 §5.1.9.2 computed from the link model (D7); K, W, T1_Initial, and Transmission_Limit fixed in WP-0 | Accepted (2026-10-19) |
| D6 | TM, AOS, or USLP downlink | **TM/TC is the baseline** for every milestone, as the roadmap describes A2 and A3. **AOS and USLP variants** are added at A4, when the `aldis-link` engines make the frame protocol a configuration choice: an AOS downlink variant (TC uplink unchanged), and a USLP variant in both directions (COP-1 over USLP, [cop1.md](../standards/cop1.md) §2.1). Each variant has its own golden traces and the same packet projection (D2 rule 2). The variants do not gate A4's exit but must exist before A6 exits, so that every v1 frame codec meets an integrator-style user before the v1.0 API freeze. Alternatives: TM only (cheapest; AOS and USLP APIs then get no application feedback before v1.0); all three from A2 (triples the hand-written loops that A4 throws away) | Accepted (2026-10-19) |
| D7 | Loopback link and scheduler | The **harness** owns time and the link; nodes never see each other. Simulated `Instant` starts at 0 (API-003 units, µs). Downlink: synchronous, one frame per fixed **frame slot** (*indicative* 8 frames/s); the flight node must supply a frame in every slot (LNK-030). Uplink: one data unit per **uplink slot** when the ground node has one. Both directions: constant one-way delay (*indicative* 10 ms). A discrete-event scheduler advances to the earliest of: frame and uplink slots, link arrivals, timeline instants, and each node's reported deadline (API-053). Ties at one instant are processed in a fixed order defined in WP-0 (arrivals, then ground, then flight; within a node, in output order). Step instants are not recorded, so an implementation that asks for extra wake-ups produces the same trace. No wall clock, threads, hash-map iteration order, or randomness anywhere in the harness (GEN-031 applied to the application) | Accepted (2026-10-19) |
| D8 | Fault injection | **Scripted**, not seeded. A case's fault script lists faults addressed by (direction, link-unit index): drop, duplicate, delay by d µs (which reorders), flip bit b of octet n (caught by the FECF), truncate to n octets. Faults are recorded in the trace before the unit they affect. Script-addressed faults are reviewable one by one and reproduce in C++ without porting a PRNG. A2 adds downlink faults (frame loss and corruption: ground frame-gap and packet-sequence detection, LNK-051, SPP-011). A3 adds uplink faults (loss, duplication, reordering, a lockout and its recovery with Unlock), exercising retransmission. Alternative: a seeded PRNG fully specified in the scenario (e.g. for a non-golden soak run in CI); not needed for A0–A6 | Accepted (2026-10-19) |
| D9 | Golden-trace format | **JSON Lines**, canonical: UTF-8, LF line ends, no insignificant whitespace, keys in a fixed order per record kind, integers in decimal, octets as **uppercase hex** (as in the vectors, [07 §2](../07-test-vectors.md#2-codec-vectors)). Line 1 is a header: scenario ID, scenario version, case, trace version, and the requirement IDs the case verifies. One record per observable event, each with `t_us` and `kind`: packet generated by an application, link unit sent, fault applied, link unit delivered, packet delivered to an application, application event (a closed, scenario-defined vocabulary with stable codes, mapped from library events and directives, in the spirit of API-080). Library-internal events and step instants are **not** recorded, so hand-written loops (A2, A3) and engines (A4) can produce the same trace. The implementation name goes in a separate run manifest, never in the trace. Comparison is byte equality; on a mismatch the harness prints the first differing record decoded with the strict codecs. Every link unit in a golden trace must decode with the strict codecs, which CI checks. A change to a released golden trace bumps its trace version and states the reason in `refapp/golden/CHANGES.md`, as TV-021 does for vectors | Accepted (2026-10-19) |
| D10 | Golden traces, vectors, and framework trace | Golden traces live in **`refapp/golden/<case>.jsonl`** (track A owns them) and are **not** conformance vectors: they are whole-system regression baselines produced by an implementation, for which TV-020 has no provenance category, while the vectors are the per-unit contract. A frame or packet from a golden trace becomes a codec vector only with `hand-derived` or `cross-checked` provenance. Raised as **OI-23**. The framework trace of FW-013 (M6) records replay inputs per stack, `(now, FrameSource input, FrameSink readiness)`. A0 proposes that M6 defines it so that each node's replay input can be extracted from a golden trace, and A6 replays both nodes from the golden traces. Raised as **OI-25** | Accepted (2026-10-19) |
| D11 | Node architecture, C++ and QEMU (A5, A6) | A node is a sans-I/O state machine: `step(now, delivered units, timeline inputs) -> (units to send, packets, application events, next deadline)`. Rust nodes run **in-process** (A1–A4). The A5 C++ flight node (`aldis.hpp`, CABI-072) and the A6 bare-metal flight node under QEMU run as **child processes** speaking a small length-prefixed **node protocol** in lockstep over stdin/stdout, designed in WP-0 and implemented in A5. The link, clock, faults, and trace writer stay in the harness, so the trace cannot depend on where a node runs. This keeps `refapp/` free of `unsafe` (an in-process C++ node needs an FFI bridge, which GEN-071 permits only in `aldis-ffi` and `difftest/`), but it departs from the roadmap's "in-process loopback link" for A5 and A6. Raised as **OI-24** | Accepted (2026-10-19) |
| D12 | Time model | Flight on-board time is **CUC Level 1** (time code ID `001`, 1958 January 1 TAI epoch, no leap seconds; 301.0 §3.2.1–3.2.2, TIME-014), 4 coarse and 2 fractional octets (*indicative*). On-board time = T0 + `Instant`, with a fixed T0 coarse value and no drift; conversion from the `Instant` follows TIME-011 (ceiling to the fraction). Housekeeping and payload packets carry the time code in the secondary header with an **implicit** P-field; event packets carry it with an **explicit** P-field (both allowed per data path, 133.0 §4.1.4.2.2.4–5; exercises SPP-008 both ways). Ground time is the receive `Instant`. Clock offset, drift, and time correlation are left for a later case | Accepted (2026-10-19) |
| D13 | `refapp/` workspace | A separate Cargo workspace like `fuzz/` and `exerciser/`, excluded from the root workspace, on the pinned toolchain. Crates (`publish = false`): `refapp-scenario` (scenario parameters and fault scripts, generated from one language-neutral file `refapp/scenario/*.json` validated by a schema, so the A5 C++ node gets the same values as a generated header); `refapp-flight` (`no_std`, no heap, the flight node); `refapp-ground` (the ground node); `refapp-harness` (`std` binary and library: scheduler, link, faults, trace writer, comparator, node protocol). Data: `refapp/golden/`. Added later: `refapp/cpp/` (A5, CMake, consumes the `find_package(aldis)` package of CPP-012 and `aldis.hpp`) and a bare-metal flight binary (A6). CI job `refapp` in `ci.yml` (D16) | Accepted (2026-10-19) |
| D14 | Code rules for `refapp/` | `refapp-flight` uses the **flight lint set** (VER-001: workspace lints with the restriction lints) and `no_std` without heap, because integrators copy it and A6 runs it bare-metal; at A6 it also gets the panic-link check (VER-010 method). `refapp-ground` uses the same restriction lints (it parses untrusted downlink) but may use `std`. `refapp-scenario` and `refapp-harness` use the **tooling profile** (RS-017). `#![forbid(unsafe_code)]` in every crate (D11). No application area exists for this today, so these rules become APP requirements | Accepted (2026-10-19) |
| D15 | Public API only, and API feedback | `refapp/` depends on the **`aldis` facade** only, by path, default features off, each node enabling just the features it uses; at A5 the C++ node uses only `aldis.h` and `aldis.hpp`. Never `aldis-testkit`, `test-hooks` features, or items hidden from the docs. A tooling check (WP-3) enforces the dependency rule. Every API pain point becomes a GitHub issue labelled `api-feedback` for track R (roadmap §6 rule 2), the workaround in `refapp/` cites the issue, and each A-milestone plan lists its findings. `refapp/` never asks for an application-specific API | Accepted (2026-10-19) |
| D16 | Area APP: numbering, verification, CI | `APP-NNN` IDs grouped in **decades per section** of `docs/reference-application.md`, as GEN, TIME, and LNK are (roles, applications and APIDs, link plan, time, link model and faults, golden traces, node protocol, implementation rules). Same table columns as every area; Source cites the constraining CCSDS clause or `—`; each requirement names the library requirements it exercises. Verification: **T** by a golden-trace reproduction test annotated `// req:` in `refapp/`, and the header of each golden trace lists the IDs its case verifies; **I** by review for scenario rules. `xtask trace` learns to read both (WP-3). CI job `refapp`: fmt, Clippy, tests, and a `thumbv7em-none-eabihf` build of `refapp-flight`; from A1 on it reproduces every golden trace | Accepted (2026-10-19) |

Raised with this plan: **OI-23** (golden traces and the vector suite), **OI-24** (out-of-process nodes and GEN-071), **OI-25** (FW-013 trace format), and **OI-26** (spacecraft ID for the scenario) in [09-open-issues.md](../09-open-issues.md). The scenario also depends on **OI-11** (TM idle strategy, D4) and **OI-06** (SANA registries: no Encapsulation Packet user data until an EPI is available, so EPP appears only as Idle Packets in the USLP variant, LNK-013).

### 2.1 What each milestone adds (D2)

| Milestone | Flight node | Ground node | New on the link | Libraries used |
|---|---|---|---|---|
| **A1** Packets | All applications of D1; CUC on-board time; per-APID sequence counts (SPP-011); dump segmented into packets (SPP-013) | Packet decoding, time-code decoding, sequence tracking (SPP-011), dump reassembly; ops script | Space Packets, one per link unit (packet-level loopback) | `aldis-spp` (with `time`), `aldis-time` |
| **A2** Frames | Hand-written VC packet generation (FHP, release deadline, idle fill), mux, OID frames, frame counts, OCF with CLCW; TC frame validation, MAP demultiplexing, segment reassembly | TM frame decoding, FECF, VC demultiplexing, packet extraction, frame-gap detection; TC Type-BD frames with blocking and segmentation | TM frames; TC Type-BD frames; downlink faults | + `aldis-tm`, `aldis-tc`, `aldis-core` (OID, counters) |
| **A3** COP-1 | FARM-1; CLCW from FARM-1; FARM-1 status in housekeeping (API-056) | FOP-1 with directives (Initiate AD, Unlock, Set V(R)); transfer and directive notifications as application events | Type-AD and Type-BC frames; uplink faults | + `aldis-cop1` |
| **A4** Engines | Loops replaced by `aldis-link` engines | Loops replaced by `aldis-link` engines | Nothing new (same traces as A3); AOS and USLP variants (D6) | + `aldis-link` |
| **A5** C and C++ | C++ flight node over `aldis.hpp`, built against the Rust-backed and the native library | Unchanged | Nothing new | `aldis.h`, `aldis.hpp`, native C++ library |
| **A6** Framework and target | `aldis-framework` stacks; flight node bare-metal under QEMU | `aldis-framework` stacks; replay from the golden traces | Nothing new | + `aldis-framework` |

### 2.2 APIDs (D3, indicative)

| APID | Direction | Application | Secondary header |
|---|---|---|---|
| `0x010` | downlink | Housekeeping, 1 Hz | CUC, implicit P-field |
| `0x011` | downlink | Event reports, sporadic | CUC, explicit P-field |
| `0x020` | downlink | Payload science, about 0.5 Hz, packets longer than one frame data field | CUC, implicit P-field |
| `0x021` | downlink | Memory dump, on command, segmented (SPP-013) | CUC, implicit P-field |
| `0x080` | uplink | Command handler (immediate and time-tagged commands) | none |
| `0x081` | uplink | Table upload (larger than one TC frame data unit) | none |
| `2047` | downlink | Idle Packets, inserted by the link procedures only | none (SPP-010) |

### 2.3 Virtual channels and MAPs (D4, D5, indicative)

| Channel | Carries | Notes |
|---|---|---|
| TM VC 0 | Housekeeping and event packets | Highest mux priority; short release deadline |
| TM VC 1 | Payload and dump packets | Packets span frames (LNK-011) |
| TM VC 7 | OID frames only | Configured for packets (132.0 §4.1.4.6.3); OI-11 default |
| TC VC 0, MAP 0 | Real-time commands | Blocking permitted |
| TC VC 0, MAP 1 | Table upload | Segmentation permitted |

The spacecraft ID is open (OI-26).

## 3. Work packages

Size is relative effort: **S** ≈ small, **M** ≈ medium, **L** ≈ large. WP-0 starts once this plan is approved. WP-2 and WP-3 can run alongside WP-0, because the skeleton does not depend on the scenario's values.

```
plan approved ─┬─▶ WP-0 scenario document ─▶ WP-1 spec integration ─┐
               ├─▶ WP-2 refapp/ skeleton ───────────────────────────┼─▶ WP-4 review and exit
               └─▶ WP-3 tooling and CI ─────────────────────────────┘
```

### WP-0 Scenario specification (L) — area APP

`docs/reference-application.md`, one section per decade of APP IDs (D16):

| Section | Content |
|---|---|
| Purpose and scope | What the application demonstrates; the superset rules (D2); what it does not cover (sync and coding, SDLS, CFDP) |
| Roles and applications | Flight and ground nodes, their applications, command set, time-tagged command semantics, ops script (D1) |
| Identifiers | APIDs, packet types, secondary-header layouts, spacecraft ID (D3, D12, OI-26) |
| Link plan | TM and TC managed parameters per 132.0 §5 and 232.0 §5; COP-1 parameters per 232.1 §5.1.9.2 and §6.1.8; mux, release, idle, and OID policy with the LNK requirements they follow (D4, D5); AOS and USLP variants (D6) |
| Time | `Instant`, T0, CUC format and P-field per data path (D12) |
| Link model and faults | Slots, delays, scheduler and tie order, fault kinds, the cases per milestone (D7, D8) |
| Golden traces | Record kinds and fields, canonical encoding, header, comparison, versioning, location (D9, D10) |
| Node interface | In-process trait and out-of-process node protocol (D11) |
| Implementation rules | Public API only, lint profiles, determinism, API feedback (D14, D15) |

Every protocol value in the document cites its Blue Book clause, read from the Blue Book, never from memory. Registry values are flagged against OI-06 or OI-26, not invented.

**Done when:** the document passes `cargo xtask spec-lint` with area APP registered, and the maintainer has reviewed it.

### WP-1 Spec integration (S)

| Task | Output |
|---|---|
| Area APP row in [00 §5.2](../00-introduction.md#52-requirement-identifiers); document row in [docs/README.md](../README.md) | shared docs |
| Roadmap §3: A0 row links this plan; the "in-process" wording follows the OI-24 decision | [08-roadmap.md](../08-roadmap.md) |
| OI-23 to OI-26 updated with the maintainer's decisions; OI-11 and OI-06 cross-referenced | [09-open-issues.md](../09-open-issues.md) |
| APP IDs added to the exit table (§4) | this plan |
| Changelog fragment | `changelog.d/` |

### WP-2 `refapp/` skeleton (M) — VER-001, VER-004, RS-017

| Task | Output |
|---|---|
| Separate workspace (D13), excluded from the root workspace; the four crates with lint tables per D14 | `refapp/` |
| Scenario file format and schema; generation of Rust constants (C++ header generation follows in A5) | `refapp/scenario/`, `refapp-scenario` |
| Node trait (D11); empty flight and ground nodes | `refapp-flight`, `refapp-ground` |
| Harness: scheduler and link model with an empty timeline; trace writer, canonical-form check, and comparator, tested with a hand-written sample trace | `refapp-harness` |
| `refapp/README.md`: how to run a case, compare, and bless a new trace version | doc |

**Done when:** the workspace builds and tests on the host, `refapp-flight` builds for `thumbv7em-none-eabihf`, and a header-only run of an empty case writes a canonical trace that the comparator accepts.

### WP-3 Tooling and CI (S) — VER-001, VER-004, VER-055

| Task | Output |
|---|---|
| CI job `refapp`: fmt, Clippy, tests, bare-metal build of `refapp-flight` (D16) | `.github/workflows/ci.yml` |
| `cargo deny` over the `refapp/` workspace (licenses, advisories) | CI |
| Dependency rule: `refapp/` crates depend on `aldis` (the facade) and no other `aldis-*` crate (D15) | `xtask` |
| `xtask trace` scans `refapp/` for `// req:` and reads the requirement IDs in golden-trace headers (D16) | `xtask` |

**Done when:** the `refapp` job is green on `main`, and a planted dependency on `aldis-testkit` fails the dependency check (checked once, then reverted).

### WP-4 Review and exit (S)

Maintainer review of the scenario document, D1–D16, and OI-23 to OI-26. The approval is recorded in this plan's status line, with the PRs.

## 4. Exit criteria

A0 is complete when every item below is green on `main`. The APP requirements listed are those A0 can verify: the scenario rules by the WP-4 review (method I), and the skeleton and tooling rules by `refapp/` tests and CI. The other APP requirements are verified from A1 on, by the milestone that implements them.

| Area | Criteria |
|---|---|
| Scenario | `docs/reference-application.md` merged with area APP registered in 00 §5.2; maintainer approval recorded (WP-4); scenario rules reviewed: APP-001, APP-002, APP-005, APP-006, APP-014, APP-020, APP-025, APP-026, APP-061, APP-062, APP-070, APP-076, APP-077, APP-084, APP-087, APP-088, APP-091, APP-101 |
| Skeleton | `refapp/` workspace builds and tests; `refapp-flight` builds `no_std` for `thumbv7em-none-eabihf`; flight node lint set VER-001, tooling crates RS-017, formatting VER-004; APP-102…APP-105; trace writer, canonical-form check, and comparator APP-080, APP-081, APP-085 |
| Tooling | VER-055 (the `refapp` job uses only free tools); dependency rule and trace scanning in `xtask` (WP-3); APP-100, APP-106, APP-107, APP-108 |
| Docs | Roadmap §3 linked; OI-23 to OI-26 decided or carried with an owner; changelog fragment |

## 5. Risks

| Risk | Mitigation |
|---|---|
| Hand-written loops (A2, A3) and the `aldis-link` engines (A4) choose differently where the spec leaves room (FHP release, idle fill, blocking, OID), so A4 cannot reproduce the A3 traces | The scenario pins each policy to its LNK requirement and managed parameter (D4, D5); a divergence is resolved against the Blue Book and the LNK text, never by tuning the trace; if the trace must change, it gets a new trace version with the reason (D9) |
| A golden trace records what the implementation did, not what is correct, so it can freeze a bug | CI decodes every link unit with the strict codecs (D9); each case states invariants checked by tests (all commands executed once and in order, packet counts, gaps only where faults were injected); the first frame of each kind is checked by hand against the Blue Book in A1–A3 |
| Open spec questions (OI-11, OI-24, OI-25, OI-26) are decided differently from the defaults assumed here | Each dependence is named in the decision; the scenario document is written against the decision, and a change becomes a new trace version |
| Trace files grow large and noisy in review | 223-octet frames, a short nominal case, one case per fault theme; comparison prints decoded differences instead of raw diffs |
| Process-boundary nodes (A5, A6) add latency and a protocol to maintain | Lockstep protocol with no timing dependence; the harness never reads wall-clock time; the protocol is small and specified in WP-0 |
| API feedback from track A forces API changes in track R late in v1 | Track A starts now (stage 1 of roadmap §4), issues are filed as soon as found, and the AOS and USLP variants run before the v1.0 freeze (D6) |
| M1 or M2 slips and A1 or A2 wait | A0 has no entry gate; WP-2 and WP-3 prepare the harness so that A1 can start the day M1 exits |
