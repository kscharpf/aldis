# 08 — Roadmap

v1 is built in three **tracks** against one specification. Each track has its own milestones and can be worked on independently, but a track milestone starts only when its **entry gate** (results merged on `main` from other tracks) is met.

| Track | Purpose | Milestones | Directories it owns |
|---|---|---|---|
| **R — Rust reference** | The reference implementation and the recommended choice: Rust crates, the C ABI over them (`aldis-ffi`), and the C++ wrapper | M0–M6 (§1) | `crates/`, `fuzz/`, `exerciser/`, `crosscheck/`, the generated C header |
| **C — Native C++** | A native C++ implementation for legacy flight software that cannot easily take a Rust toolchain ([C++ profile](profiles/cpp.md)). It follows the Rust crates module by module and must match them (VER-053) | C0–C5 (§2) | `cpp/`, `difftest/` |
| **A — Reference application** | An end-to-end flight↔ground application that uses only the public APIs, grown one milestone at a time. It gives API feedback before v1 freezes, shows that the Rust-backed and native libraries are interchangeable, and is the integrators' starting point | A0–A6 (§3) | `refapp/` |

§4 shows how the tracks overlap, §5 defines the releases, and §6 the rules that let separate contributors or agents work on the tracks at the same time.

## 1. Track R — Rust reference (M0–M6)

Every milestone defines the **C ABI for what it adds**: a codec milestone adds its codecs' functions to `aldis.h`, and an engine milestone adds its engines' handles. The native C++ track calls through that header for differential testing (VER-053), so it cannot wait for M5.

| Milestone | Content | Entry gate | Exit criteria |
|---|---|---|---|
| **M0 Foundations** ([plan](milestones/m0-foundations.md)) | Repository + GitHub Actions CI, lint set, exerciser/panic-link check, vector harness, `xtask` trace/spec tooling, `aldis-core` (errors, bit fields, IDs, time, CRC-16, OID generator, counters, containers, security hooks, frame layout) | — | **Complete** (2026-10-16, `v0.1.0-m0`); exit criteria in the plan §4 |
| **M1 Packets & time** ([plan](milestones/m1-packets-time.md)) | `aldis-time`, `aldis-spp`, `aldis-epp` | M0 | **Complete** (2026-10-20, `v0.2.0-m1`); exit criteria in the plan §4 |
| **M2 Frame codecs** ([plan](milestones/m2-frame-codecs.md)) | **WP-0:** the M1 codecs' part of the C ABI (time, SPP, EPP: `aldis-ffi` functions and generated header, OI-21), the field-code catalogue with `FieldId` carrying its code (API-082, OI-27), and the shared property-test strategies in `aldis-testkit` (VER-054). Then `aldis-tm`, `aldis-tc` (incl. CLCW, delimiting, validation), `aldis-aos` (incl. FHEC, editions), `aldis-uslp` (incl. truncated), each with its C ABI functions | M1 | All TM/TC/AOS/USLP requirements traced; 100 % coverage; vectors pass via the C ABI for every codec |
| **M3 COP-1** | `aldis-cop1` FOP-1 + FARM-1, state tables as data, closed-loop tests; FOP-1/FARM-1 handles in the C ABI, with the capacities configuration (CABI-030) | M2 | One vector per table cell; two-person table review recorded |
| **M4 Link engines** | `aldis-link` generators, mux, receiver, extractor, segmenter/reassembler, CLTU packer, each with its C ABI handle | M3 | LNK requirements traced; property tests for FHP placement and reassembly |
| **M5 C ABI completion** | C test harness with sanitizers and Miri (VER-026), header compile checks (VER-050), ABI versioning (CABI-060), and the header-only C++ wrapper `aldis.hpp` (CABI-072) | M4 | All CABI requirements traced; vectors pass via the C ABI and via `aldis.hpp` |
| **M6 Framework** | `aldis-framework` traits, stacks, loopback, std adapters, trace/replay (the FW-013 trace format extractable from, or sharing the record encoding of, the reference application's golden traces, OI-25) | M5 | FW requirements traced; loopback stack tested end to end (the reference application, A6, is the full demo) |

Layer 3 (`aldis-framework`) is Rust-only in v1. Native C++ users drive the engines from their own executive, as legacy flight software already does.

## 2. Track C — Native C++ (C0–C5)

Each C++ milestone ports a Rust milestone that has already exited, so the C++ track runs about one milestone behind track R. The Rust crates are never held back for the C++ port. A C++ module is done when it passes the vector suite and the differential tests against its Rust crate (VER-053) and meets the [C++ profile](profiles/cpp.md). The language is C++20 (OI-19), and engine capacities are template parameters (OI-20).

| Milestone | Content | Entry gate | Exit criteria |
|---|---|---|---|
| **C0 Foundations** ([plan](milestones/c0-foundations.md)) | `cpp/` CMake project, presets, and toolchain files for the bare-metal targets; CI jobs for both compilers, clang-tidy, clang-format, sanitizers, coverage, the freestanding link check with its canary, and stack usage; the CodeQL AUTOSAR C++14 and CERT C++ checks running (CPP-043) and IKOS evaluated (OI-22); `xtask` scans C++ sources for `// req:` (CPP-057); `aldis_core`; the C++ vector runner, on the host and under QEMU | — | `aldis_core` passes the core vectors on every CI target with 100 % coverage; CPP-001…003, CPP-010…012, CPP-020, CPP-021, CPP-023, CPP-025, CPP-026, CPP-030…034, CPP-040…044, CPP-050…053, CPP-056, CPP-057 traced for `aldis_core` ([plan](milestones/c0-foundations.md) D18) |
| **C1 Packets & time** | `aldis_time`, `aldis_spp`, `aldis_epp`; their part of `aldis_capi` (CPP-035) with the header comparison (CABI-070); the `difftest/` workspace (bridge, differential property tests and fuzz targets for codecs); native fuzz targets; mutation testing | C0; M1; M2 WP-0 | M1 vectors pass; VER-053 met for every M1 codec; CPP-035, CPP-054, CPP-055, CPP-060, CPP-061 traced |
| **C2 Frame codecs** | `aldis_tm`, `aldis_tc`, `aldis_aos`, `aldis_uslp` with their C ABI functions and differential tests | C1; M2 | M2 vectors pass; VER-053 met for every M2 codec |
| **C3 COP-1** | `aldis_cop1` with its C ABI handles; differential state-machine tests that drive the Rust and C++ engines in lockstep | C2; M3 | M3 vectors pass; VER-053 met for FOP-1 and FARM-1 |
| **C4 Link engines** | `aldis_link` with its C ABI handles and differential state-machine tests | C3; M4 | M4 vectors pass; VER-053 met for every engine |
| **C5 C ABI parity** | The native library passes the M5 C test harness, and `aldis.hpp` builds and passes unchanged over both the Rust-backed and the native library (CABI-072) | C4; M5 | Every CABI requirement met by the native library; native C++ release candidate (§5) |

## 3. Track A — Reference application (A0–A6)

The reference application lives in its own workspace, `refapp/`, and uses only the public APIs, as an integrator would. Its scenario is specified in [reference-application.md](reference-application.md) (area APP). Its flight and ground sides talk over a loopback link owned by a harness: the Rust nodes run in-process, and the A5 C++ and A6 QEMU flight nodes run as child processes over a lockstep node protocol ([OI-24](09-open-issues.md), [reference-application.md](reference-application.md) §8.2). The engines are deterministic (GEN-030), so each run of a scenario produces a **golden trace** of every data unit exchanged. Each implementation must reproduce the trace byte for byte.

| Milestone | Content | Entry gate | Exit criteria |
|---|---|---|---|
| **A0 Scenario** ([plan](milestones/a0-scenario.md)) | The scenario specification, a new document [`docs/reference-application.md`](reference-application.md) with requirement area APP: flight and ground roles, APIDs, virtual channels, rates, injected faults, and the golden-trace format. `refapp/` workspace skeleton with a CI build job | — | Scenario approved by the maintainer |
| **A1 Packets** | Flight side produces housekeeping Space Packets with CUC time stamps; ground side decodes them; first golden trace | A0; M1 | Trace reproduced in CI |
| **A2 Frames** | Downlink TM frames and uplink TC frames, multiplexed and demultiplexed by hand-written loops in the application | A1; M2 | Trace reproduced in CI; API issues found are filed |
| **A3 COP-1** | FOP-1 on the ground, FARM-1 on the flight side, CLCW in the OCF; injected frame loss exercises retransmission | A2; M3 | Trace reproduced in CI, including the fault cases |
| **A4 Engines** | The hand-written loops are replaced by the `aldis-link` engines | A3; M4 | Same trace as A3 for the same scenario |
| **A5 C and C++** | A C++ flight side using `aldis.hpp`, built once against the Rust-backed library and once against the native library | A4; M5; C5 to exit | The Rust, Rust-via-C-ABI, and native C++ builds reproduce the same golden trace |
| **A6 Framework and target** | The application moves onto `aldis-framework` stacks and trace/replay; the flight side runs bare-metal under QEMU; footprint, stack, and timing figures published | A5; M6 | End-to-end ground↔flight demo; figures included in the v1.0 tables |

## 4. Overlap

Each row is a set of milestones expected to run at the same time. The rows are not time boxes: a track milestone starts as soon as its entry gate is met.

| Stage | Track R | Track C | Track A |
|---|---|---|---|
| 1 | M1 (complete, `v0.2.0-m1`) | C0 | A0 |
| 2 | M2 (next) | C1 (after M2 WP-0) | A1 |
| 3 | M3 | C2 | A2 |
| 4 | M4 | C3 | A3 |
| 5 | M5 | C4 | A4 |
| 6 | M6 | C5 | A5 |
| 7 | v1.0 release | Native C++ release | A6 |

## 5. Releases

| Release | Contents | Gate |
|---|---|---|
| `v0.N.0-mN` | Rust crates after milestone MN (as today) | MN exit |
| **v1.0** | Rust crates, C ABI, `aldis.hpp`; docs, trace matrix, coverage waivers, stack-usage tables, changelog | M6 and A6 exited; all R-track requirements verified |
| **Native C++ 1.0** | The native C++ library, with `aldis.h` and `aldis.hpp` | C5 exited. It ships with v1.0 if ready, or after it. A native release carries the version of the Rust release it was differentially verified against (VER-053) |

C and A milestones do not cut releases. Their exit is recorded in their milestone plans.

## 6. Working in parallel

These rules let separate contributors, or separate agents, each own one track.

1. **Plans first.** Before a milestone starts, its plan is written in `docs/milestones/` (`m<N>-…`, `c<N>-…`, `a<N>-…`) with an Area/Criteria exit table, so that `cargo xtask trace --scope <id>` measures it. The plan records the entry gate as met, with the PRs that met it.
2. **Ownership.** A PR changes the directories of one track (table at the top), plus shared files. A change needed in another track's directories is a separate PR on that track's branch prefix. Shared files are `docs/`, `test-vectors/`, `xtask/`, `.github/`, `AGENTS.md`, and `README.md`. Changes are recorded in a new fragment file in [`changelog.d/`](../changelog.d/README.md), never in `CHANGELOG.md` itself, so changelog entries never conflict.
3. **Branches.** `m<N>/<topic>`, `c<N>/<topic>`, `a<N>/<topic>` for track work; `spec/`, `chore/`, `fix/` for the rest. Every PR targets `main`; there are no long-lived track branches.
4. **The C header belongs to track R.** It is generated from `aldis-ffi` (CABI-002). Track C or A asks for a change through an issue, and track R makes it.
5. **The Rust crates decide.** When a native implementation diverges from the Rust crates, the Blue Book decides which is wrong (VER-053). If the Rust crates are wrong, the fix is a track-R PR with a regression vector, and the C++ PR waits for it. The divergence is listed in the C++ milestone plan until then. Native code never changes the spec to match itself.
6. **Vectors.** Any track may add vectors, following [07](07-test-vectors.md). A vector merges only with the Rust crates passing it.
7. **IDs.** Area CPP belongs to track C and area APP to track A; the other areas are shared. Requirement and OI numbers are allocated when a PR is written. If two open PRs take the same number, the one that merges second renumbers (`cargo xtask spec-lint` reports duplicates).
8. **Open issues.** A track that hits an open spec question adds or updates an OI in [09](09-open-issues.md) instead of choosing silently in code.
9. **Merge gate.** A PR merges on the published record of a passing verification run, `cargo xtask verify` on the contributor's machine or the `verify` workflow started by hand ([06 §7](06-verification.md#7-verification-runs-and-the-merge-gate), VER-058, VER-060); GitHub Actions repeats every check each night (VER-059). How to run it: [AGENTS.md](../AGENTS.md#toolchain-and-checks).

## 7. Later standards (proposed order)

| Release | Standards | Rationale |
|---|---|---|
| v1.1 | Minimal sync/coding: ASM attach/search, CLTU (BCH) build/parse, PN randomizers (131.0 §9–10, 231.0 §3, §5–6), and the UTC/TAI leap-second table trait | Lets v1 frames reach real radios without an external modem library |
| v2 | 355.0/355.1 SDLS + 352.0 algorithms (with pluggable crypto providers, HSM-friendly) | Hooks already reserved in v1 (API-070…075) |
| v2 | 727.0 CFDP (Class 1 and Class 2, sans-I/O, filestore trait) | Highest-demand application protocol |
| v3 | 131.0 full FEC (RS, convolutional, LDPC, turbo), 231.0 LDPC; 211.x Proximity-1 + COP-P | Compute-heavy; needs per-target optimization |
| v3 | 734.x DTN (BP/LTP), 702.1 IPoC | Networking |
| v4 | Ground formats: 502–509 navigation messages, 660.0 XTCE; SLE/CSTS (911–922, 913.1) | Ground segment; `std` likely |

Each later standard follows the same three tracks: the Rust crate first, then its native C++ port and its use in the reference application.

## 8. Language rollout

| Phase | Language | Approach |
|---|---|---|
| 1 | Rust | Reference implementation (track R); the recommended choice |
| 1 | C / C++ | C ABI over Rust (`aldis-ffi`) + header-only C++ wrapper (track R, M5) |
| 1 | C++ (native) | Native implementation (track C); same header (CABI-070) and C++ wrapper API (CABI-072) |
| 2 | Python | Bindings over the C ABI (or PyO3 over the Rust crates) with an idiomatic API; vectors run under pytest |
| 2 | Go | cgo bindings over the C ABI |
| 2 | Java / C# | Panama (FFM) or JNI / P/Invoke over the C ABI |
| 3 | C (native) | Full native implementation of this specification; same header (CABI-070) |

Every phase is gated on passing the shared vector suite (TV-*).
