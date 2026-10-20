# Milestone 1 — Packets and time

**Goal:** complete, verified Layer 1 codecs for time codes, Space Packets, and Encapsulation Packets: `aldis-time`, `aldis-spp`, and `aldis-epp`. They are verified to the same standard as `aldis-core` in M0: vectors, panic freedom on bare metal, 100 % coverage, fuzzing, and cross-checks.
**Roadmap entry:** [08-roadmap.md](../08-roadmap.md) §1 · **Builds on:** [M0](m0-foundations.md) (`v0.1.0-m0`) · **Release:** `v0.2.0-m1`

**Status: complete (2026-10-20), release `v0.2.0-m1`.** The exit criteria (§4) are checked on `main` at 5f40e8b; the results column there gives the evidence for each. Decisions D1–D8 accepted 2026-10-17; OI-15 and OI-16 resolved 2026-10-18, OI-29 and OI-30 2026-10-19; D4 is recorded against OI-06, which stays open for USLP and the link layer. Merged on `main`, the M1 pull requests between #17 and #50 (the C0, A0, and shared pull requests in that range are not part of M1): WP-0 (PR #17); WP-3, `aldis-epp` (PR #18); WP-1 part 1, P-field, CUC, CDS, CCS (PR #19), its value types (PR #21), and part 2, ASCII codes and conversions (PR #30); the RS-019 nursery lints (PR #20) and the RS-020 audit (PR #24, issue #22); WP-2 part 1, the `aldis-spp` codec (PR #38), and part 2, segmentation, reassembly, and the time secondary header (PR #42, with the follow-up PR #45); WP-4, the cross-checks against `spacepackets` (PR #46); GEN-006 and the verify check `features` (PR #48); WP-6, the m1 trace in the gate, the RS-015 citations, and the facade feature `spp-time` (PR #49); WP-5, the exerciser under QEMU on all three bare-metal targets with every time, SPP, and EPP vector case (D6, D7; PR #50). The exit (branch `m1/exit`) switches `cargo xtask trace --scope m1` to `--strict` (D8), repeats the GEN-022/023 inspections before the release, sets the workspace version to 0.2.0, and releases the changelog. Three results come from the hand-started Nightly [run 39442459267](https://github.com/kscharpf/aldis/actions/runs/39442459267) (`fuzz-seconds=3600`): the 1 CPU-hour fuzz run (VER-022), the full mutants report (VER-025, triaged in PR #52), and the stack sizes (VER-011). The maintainer approved the GEN-022/023 re-inspection on 2026-10-20. The exit review found the WP-4 cross-checks of the time codes missing; PR #53 added them (VER-040), and the independent implementations found but not run are OI-32, targeted at M2. The second review of the hand-derived EPP, CCS, and ASCII vectors (§5) is [recorded](../verification/reviews/m1-vectors.md).

## 1. Starting point

| In place after M0 | Missing for M1 |
|---|---|
| `aldis-core`: errors, bit fields, IDs (`Apid`, `SeqCount`, `PacketVersion`), modular counters, containers | A shared **decode mode** (`Strict`/`Lenient`) and **anomaly set** type. API-030 and GEN-043 need them in every codec, and `aldis-core` has neither yet |
| Crate skeletons `aldis-time`, `aldis-spp`, `aldis-epp` with the module layout of [profiles/rust.md §4](../profiles/rust.md#4-module-layout-convention) | All codec code |
| Vector model with `mode` and `config` fields; exact-match runner (TV-012) | Vector files under `test-vectors/{time,spp,epp}/` |
| Exerciser and panic-link check for `aldis-core`; QEMU run on Cortex-M | Exerciser coverage of the new crates; QEMU runs on Cortex-R and RISC-V (M0 D4 deferred these to M1) |
| Gates: coverage (`aldis-core`), six fuzz targets, mutation report, stack sizes | The same gates for the three new crates |
| `xtask trace --scope m0 --strict` in CI | A scope for M1 |
| Specifications: [time-codes.md](../standards/time-codes.md), [space-packet.md](../standards/space-packet.md), [encapsulation-packet.md](../standards/encapsulation-packet.md) | — |

## 2. Decisions

| # | Decision | Choice | Status |
|---|---|---|---|
| D1 | Decode mode and anomaly set | Add `DecodeMode` (default `Strict`) and `AnomalySet<A>` (a `Copy` bit set over a `#[non_exhaustive]` anomaly enum of up to 32 flags) to `aldis-core`. The change is additive (GEN-080); the `aldis-core` gates run again on the new code | Accepted (2026-10-17) |
| D2 | CUC fine time | Store fine time as a left-aligned `u128` fraction of a second (up to 80 bits used, TIME-011). Embedded targets lower `u128` through `compiler_builtins`; WP-5 measures the stack cost. Alternative: `[u8; 10]` with octet arithmetic | Accepted (2026-10-17) |
| D3 | Calendar conversion (TIME-033) | Closed-form civil-from-days and days-from-civil (Hinnant), with no loops or tables (GEN-022). Leap seconds pass through as second 60; no time-scale change (§6 of the spec). A CDS value with an agency epoch (P-field bit 4 = 1) converts only if the caller supplies the epoch day; otherwise `InvalidConfig` | Accepted (2026-10-17) |
| D4 | SANA values (OI-06) | M1 embeds **no** SANA registry. Valid EPI and extended-EPI sets are caller-supplied managed parameters (EPP-010). OI-06 stays open for USLP and the link layer | Accepted (2026-10-17) |
| D5 | Independent cross-checks (VER-040) | A separate `crosscheck/` workspace (dev-only, like `fuzz/`) that runs each vector file through an independent implementation and reports disagreements. First candidate: the Rust `spacepackets` crate (SPP, CUC, CDS); its license and coverage are checked in WP-4. Where no independent implementation exists (expected for EPP, CCS, ASCII), the vectors are `hand-derived` with the derivation written down, plus the Blue Book examples | Accepted (2026-10-17) |
| D6 | Bare-metal execution | Run the exerciser under QEMU on all three targets: RISC-V on `qemu-system-riscv32 -machine virt` with semihosting; Cortex-R on a QEMU Cortex-R machine (candidate `mps3-an536`, Cortex-R52, which needs a newer QEMU than Ubuntu 24.04 ships; confirmed in WP-5). If no Cortex-R machine works in CI, Cortex-R stays at link-check only and the reason is recorded. **WP-5 (2026-10-19):** confirmed that Ubuntu 24.04's QEMU 8.2.2, in the verification image and on the GitHub runners, has no `mps3-an536` (QEMU 9.0 added it). Cortex-R therefore runs on QEMU's board-less `none` machine with `-cpu cortex-r5f`: the Cortex-R5F is ARMv7-R, the architecture of `armv7r-none-eabihf`, and QEMU 8.2 has it. The same image also runs on `mps3-an536` with QEMU ≥ 9.0 (checked with 9.2.3). No fallback is needed | Accepted (2026-10-17) |
| D7 | Exerciser structure | One exerciser workspace links all M1 crates, with one test group per crate. VER-010 asks for an exerciser "for each crate". One binary that calls every public API of every crate gives the same link-time proof, and keeps one QEMU job per target | Accepted (2026-10-17) |
| D8 | Trace scope | Add `--scope m1` from §4 below. CI keeps `--scope m0 --strict` and runs `--scope m1` without `--strict` until the M1 exit, then switches it to `--strict` | Accepted (2026-10-17) |

## 3. Work packages

Size is relative effort: **S** ≈ small, **M** ≈ medium, **L** ≈ large. WP-0 goes first. WP-1 and WP-3 can then run in parallel. WP-2's base codec can start after WP-0; only its time secondary header (SPP-008) waits for WP-1's CUC and CDS. WP-4 and WP-5 grow with each codec rather than coming at the end.

```
WP-0 core additions ─┬─▶ WP-1 aldis-time: pfield → cuc → cds → ccs → ascii → conversions ─┐
                     ├─▶ WP-2 aldis-spp: header/view/writer → idle → sequence → octet string → segment → time hdr ─┤
                     └─▶ WP-3 aldis-epp: header/view/writer → idle → config ───────────────┤
                         WP-4 vectors & cross-checks  (alongside each codec) ──────────────┤
                         WP-5 gates: exerciser, QEMU ×3, coverage, fuzz, mutants ──────────┴─▶ WP-6 exit review
```

Each codec module is done when:
- its requirements are covered by `// req:` tests and by vectors where the Verif. column says V;
- it has a fuzz target (VER-022) and a property-based round-trip test (VER-023);
- it reaches 100 % region, line, and branch coverage;
- it passes the full lint set with no new exceptions (RS-016).

### WP-0 `aldis-core` additions (S) — API-030, GEN-043, GEN-081

| Task | Output |
|---|---|
| `DecodeMode { Strict, Lenient }`, `Default = Strict` | `aldis-core::mode` |
| `AnomalySet<A: Anomaly>`: `empty`, `insert`, `contains`, `iter`, `is_empty`, `bits`. `Anomaly` maps each flag to a stable bit index (shared with the C ABI later). Strict decoders always return an empty set | `aldis-core::anomaly` |
| Unit tests, a fuzz target `anomaly_set_model`, the exerciser group, the coverage gate, a mutants run, and the GEN-022/023 inspection updated for the new modules | core gates stay green |

**Done when:** `aldis-core` is back at 100 % coverage and has no new surviving mutants. The anomaly types in the three skeletons compile against the new trait.

### WP-1 `aldis-time` (L) — TIME-001…033

| Order | Module | Content | Requirements |
|---|---|---|---|
| 1 | `pfield` | P-field parse/build: extension chain (at most 8 octets, undefined octets per TIME-005), time code ID, strict/lenient handling of reserved IDs (lenient: P-field only, OI-15), agency-defined codes (TIME-006). `decode_with_pfield` and `decode_implicit` entry points report the consumed length | TIME-001…003, TIME-005, TIME-006 |
| 2 | `cuc` | `CucFormat` (1–7 coarse, 0–10 fine; octet 2 when needed), `CucValue`, ns conversions (truncate down, ceiling up), `OutOfRange` on overflow, mission bits preserved | TIME-010…014 |
| 3 | `cds` | 16/24-bit day, ms of day, none/µs/ps sub-ms; strict range checks including the leap second; reserved sub-ms code | TIME-020…022 |
| 4 | `ccs` | BCD validation, month/day and day-of-year variations, DOY high nibble, leap-year day ranges, 0–6 sub-second octets | TIME-030, TIME-034 |
| 5 | `ascii` | Parse and format A and B, 0–12 fraction digits, optional `Z` preserved, second 60. Subset parsing behind `AsciiSubset`. Formatting writes into a caller buffer (no `alloc`) | TIME-031, TIME-032 |
| 6 | conversions | CDS ↔ CCS ↔ ASCII for the same UTC instant (D3) | TIME-033 |

Also: crate docs. TIME-004, TIME-014, and TIME-022 are verified by inspection and recorded in [analysis-records.md](../verification/analysis-records.md). The ns round-trip limits of TIME-011 go in the API docs.

**Done when:** every TIME requirement is traced, and the vector categories `time/cuc`, `time/cds`, `time/ccs`, `time/ascii`, and `time/convert` pass on every CI target.

### WP-2 `aldis-spp` (L) — SPP-001…013

| Order | Module | Content | Requirements |
|---|---|---|---|
| 1 | `header`, `view`, `writer` | Primary header with the owned header type (API-014). `SpacePacket::decode`: version check, length = 7 + field, `Incomplete{needed}`, consumes exactly one packet from a longer buffer. `peek_length` from 6 octets. One-shot `encode` and the two-phase `SpacePacketWriter`. Encoder rejects an empty data field, a zero-length secondary header, and length > `max_packet_len` | SPP-001…003, SPP-005…007, SPP-009, API-010, API-011, API-013, API-014, API-020…023, GEN-053 |
| 2 | `anomaly` | `SppAnomaly::IdleWithSecondaryHeader`; strict rejects it (SPP-004) | SPP-004, API-030…032 |
| 3 | `idle` | `IdlePacket` builder for any total length ≥ 7, caller-supplied pattern | SPP-010 |
| 4 | `sequence` | `SequenceCounter` (mod 16 384 per APID) and `SequenceTracker<const N: usize>` built on `aldis-core` `ModCounter` and `FixedVec` | SPP-011, GEN-011, GEN-024 |
| 5 | octet string | Octet String service encoder: Sequence Flags always `11` | SPP-012 |
| 6 | `segment` | Segmenter (`01`/`00`/`10`) and reassembler into a caller buffer, with gap detection | SPP-013 |
| 7 | time secondary header | Feature `time`: `SecondaryHeader::TimeCode(format) [+ ancillary len]`, decoded with `aldis-time` | SPP-008 |

**Done when:** every SPP requirement is traced, and `spp/decode_valid`, `decode_invalid`, `encode`, `idle`, `sequence_tracker`, `secondary_header_time`, and `stream` pass. `aldis-spp` without `time` still builds and passes VER-031.

### WP-3 `aldis-epp` (M) — EPP-001…012

| Order | Module | Content | Requirements |
|---|---|---|---|
| 1 | `header`, `view`, `writer` | All four header lengths, mixed in one stream; total-length semantics (not length − 1); 1-octet idle; checks on the Protocol ID Extension and CCSDS Defined Field; EPI `110` only with an extension field (OI-17); `HeaderSize::{Auto, Fixed}` | EPP-001…008, EPP-011, EPP-012, API-021 |
| 2 | `anomaly` | `EppAnomaly::{NonZeroEpiExtension, NonZeroCcsdsDefined}`; EPP-005 is an error in both modes (OI-16) | EPP-005…007, API-032 |
| 3 | `idle` | `idle(total_len)` for any length ≥ 1, smallest header that fits | EPP-009 |
| 4 | `config` | `EppConfig`: min/max data unit length and valid EPI / extended-EPI sets (D4); `validate` | EPP-010, GEN-060, GEN-061, RS-012, API-040, API-041 |

**Done when:** every EPP requirement is traced, and `epp/header_1`, `header_2`, `header_4`, `header_8`, `idle`, `auto_size_boundaries`, and `invalid` pass.

### WP-4 Vectors and cross-checks (M) — TV-001…003, TV-020…021, VER-040, API-020, API-031, API-032

| Task | Output |
|---|---|
| Vector files for every category named in the three standard documents. Every field the view exposes is listed (TV-002), and every error kind each decoder can return is covered | `test-vectors/{time,spp,epp}/` |
| Blue Book examples first, where the documents print any (provenance `blue-book-example`); then hand-derived cases with a worked derivation note | provenance per file |
| `crosscheck/` workspace (D5): runs the vectors through an independent implementation and fails on disagreement. Runs in CI. Every discrepancy is resolved against the Blue Book text and recorded | crate + `docs/verification/cross-checks.md` |
| Testkit handlers per crate, registered in each crate's tests and in the exerciser codegen (`exerciser/build.rs`, `exerciser/src/vectors.rs`; WP-5) | handlers |

**Done when:** every vector file names its provenance, the cross-check job passes, and any standard with no independent implementation is listed in `cross-checks.md` with what was searched.

### WP-5 Verification gates (M) — VER-010, VER-011, VER-022…025, GEN-020…023, VER-030

| Task | Output |
|---|---|
| Exerciser: one test group per new crate. Every public API is called with codegen'd vectors through `black_box`. `inject-panic` canary unchanged. Since PR #50: `build.rs` embeds every live case of `test-vectors/{time,spp,epp}/`, and a handler per crate compares every output exactly, as the host suites do (TV-012); the public API, checked against the symbols of an unoptimized build, is called in full | `exerciser/` |
| QEMU execution on Cortex-R and RISC-V (D6); Cortex-M continues. Since PR #50: `mps2-an386` (Cortex-M4), `none` with `-cpu cortex-r5f` (Cortex-R5F), and `virt` (RV32), each with semihosting, a time limit, and one retry | verify checks `exerciser-<target>`, nightly CI `exerciser` |
| Coverage gate: add `aldis-time`, `aldis-spp` (with `--all-features`, so `time` is included), and `aldis-epp` to `GATED` | `xtask/src/coverage.rs` |
| Fuzz targets: `time_pfield`, `time_cuc`, `time_cds`, `time_ccs`, `time_ascii`, `spp_decode`, `spp_stream`, `spp_roundtrip`, `spp_tracker_model`, `spp_segment_reassemble`, `epp_decode`, `epp_roundtrip`. Each one asserts no panic, `decode(encode(x)) == x`, and GEN-042 | `fuzz/`, `nightly.yml` matrix |
| Mutation testing for the three crates; triage of survivors before the release (VER-025) | `docs/verification/mutants-triage.md` |
| Stack-size table per target with the new crates (VER-011); u128 cost checked (D2) | nightly artefacts |
| GEN-022/023 inspection of the new crates (loops, recursion, blocking) | `docs/verification/inspections/` |
| 1 CPU-hour per fuzz target before the release (VER-022): dispatch Nightly with `fuzz-seconds=3600` | run link in the exit table |

**Done when:** every gate is green on `main` for all M1 crates.

### WP-6 Docs, trace, release (S) — VER-020, RS-015

| Task | Output |
|---|---|
| `--scope m1` in CI (D8); `--strict` at the exit | `spec` check of `cargo xtask verify`, `cargo xtask ci`, `ci.yml` |
| Crate docs cite clauses and requirement IDs (RS-015); the facade's `time`, `spp`, `epp` features documented; a facade feature that enables `aldis-spp/time`, so the time secondary header (SPP-008) is reachable through `aldis` (maintainer decision 2026-10-19): `spp-time` | rustdoc, `crates/aldis/Cargo.toml` |
| `README.md` status, `CHANGELOG.md` `0.2.0-m1` (`cargo xtask changelog release 0.2.0-m1 <date>`), roadmap row, workspace version `0.2.0` | docs |
| Open issues: D4 recorded against OI-06, any new spec questions as new OIs | `09-open-issues.md` |

## 4. Exit criteria

M1 is complete when every item below is green on `main`:

| Area | Criteria | Result |
|---|---|---|
| Time codes | TIME-001…006, TIME-010…014, TIME-020…022, TIME-030…034 | Traced (`trace --scope m1 --strict`). Vectors `time/{pfield,cuc,cds,ccs,ascii,convert}` pass on the host, on s390x, and on the three bare-metal targets |
| Space Packet | SPP-001…013 | Traced. Vectors `spp/` (11 files) pass on the same targets; segmentation and reassembly also by property test |
| Encapsulation Packet | EPP-001…012 | Traced. Vectors `epp/` (8 files) pass on the same targets |
| Codec contract | API-010, API-011, API-013, API-014, API-020…023, API-030…033, API-040, API-041, GEN-013, GEN-014, GEN-043, GEN-052, GEN-053, GEN-060, GEN-061, GEN-081, RS-012 | Traced: tests, vectors, and fuzz targets in the three crates; API-033, API-041, and GEN-081 by inspection ([analysis-records.md](../verification/analysis-records.md)) |
| Panic freedom and resources | VER-010, GEN-020…024 (exerciser runs on all three bare-metal targets, or links where D6 records why it cannot run), VER-011 | Verify checks `exerciser-<target>` pass on `main`'s tree (the record of 7706be0, 2026-10-20, same tree as 5f40e8b): the release build links on thumbv7em, armv7r, and riscv32imac, the `inject-panic` canary fails to link as required, and under QEMU (`mps2-an386`, `none` with `-cpu cortex-r5f`, `virt`) each run reports 822 cases, 0 failed, 1 not representable. GEN-022/023 re-inspected at 5f40e8b ([core](../verification/inspections/gen-022-023-aldis-core.md), [time](../verification/inspections/gen-022-023-aldis-time.md), [spp](../verification/inspections/gen-022-023-aldis-spp.md), [epp](../verification/inspections/gen-022-023-aldis-epp.md)): no unbounded loop, no recursion, nothing blocks; approved by the maintainer 2026-10-20. VER-011 stack sizes (Nightly [run 39442459267](https://github.com/kscharpf/aldis/actions/runs/39442459267), artefacts `stack-sizes-<target>`): the largest library frame is `Cuc::from_seconds_nanos`, 296 bytes on thumbv7em and armv7r and 256 on riscv32imac, so the `u128` fine time costs little stack (D2); every other library frame is at most 232 bytes. The exerciser's own `vectors::run` is 132 KiB (two packet-sized buffers, within its 256 KiB stack; PR #50) |
| Portability | GEN-005, GEN-006, GEN-007, VER-030, VER-031 | Verify checks `no-std-<target>` (3 targets), `features` (GEN-006), `big-endian` (full suite on s390x), and `layers` with `symbols` (VER-031) |
| Gates | VER-022 (all WP-5 fuzz targets, ≥ 1 CPU-hour each before the release), VER-023, VER-024 (100 % for the three crates), VER-025 (triaged) | VER-022: 20 fuzz targets in `nightly.yml`, 14 of them added in M1; 1 CPU-hour each: Nightly [run 39442459267](https://github.com/kscharpf/aldis/actions/runs/39442459267) (`fuzz-seconds=3600`, `main` 5f40e8b), all 20 jobs passed, each libFuzzer run 3601 s with no crash (from 40 million runs for `oid_forms` to 2.4 billion for `layout_ranges`). VER-023: property-based round trips in the three crates (11 tests). VER-024: `cargo xtask coverage` (2026-10-20, no waivers): `aldis-time` 1766/1766 regions, 208/208 functions, 1579/1579 lines, 202/202 branches; `aldis-spp` 668/668, 100/100, 709/709, 74/74; `aldis-epp` 588/588, 85/85, 561/561, 92/92; `aldis-core` 1062/1062, 208/208, 936/936, 88/88. VER-025: the `mutants` job of [run 39442459267](https://github.com/kscharpf/aldis/actions/runs/39442459267) over the four crates, 1553 mutants: 1096 caught, 424 unviable, 33 missed, 0 timeouts. 28 missed are the known equivalents; 5 were `aldis-core` test gaps, fixed in PR #52. Every remaining survivor is equivalent ([mutants-triage.md](../verification/mutants-triage.md)) |
| Vectors and trace | TV-001…003, TV-020, TV-021, VER-020 (`xtask trace --scope m1 --strict`), VER-021, VER-040 | 91/91 traced; the `spec` check, `cargo xtask ci`, and `ci.yml` run `--scope m1 --strict` from this exit. `vectors validate` and `vectors schema --check` pass. VER-040: `crosscheck/` against `spacepackets` 0.18.0 over 12 `spp/` and `time/` files (PR #46, PR #53): 86 agree, 118 skipped with a reason, 22 known (four `spacepackets` differences resolved against 301.0, [cross-checks.md](../verification/cross-checks.md) §3), 0 disagreements. §4 there records what was searched for each feature `spacepackets` does not cover; the qualifying candidates not run are OI-32 (maintainer decision 2026-10-20, targeted at M2), and those features keep `hand-derived` vectors (D5), reviewed a second time ([m1-vectors.md](../verification/reviews/m1-vectors.md)) |
| Docs | Roadmap updated; `CHANGELOG.md` entry for 0.2.0-m1; open issues updated | Roadmap, README, and this plan updated; `CHANGELOG.md` 0.2.0-m1 released from 25 fragments; workspace version 0.2.0. Open issues checked: none of the M1 issues is open (OI-15…OI-18, OI-29, OI-30 resolved); OI-06 stays open for USLP and the link layer, with D4 recorded |

Not in M1: frame codecs and their requirements (M2), including API-012 (fixed-length frames), engine requirements API-050…056 (M3/M4), and the C ABI (M5). The UTC/TAI leap-second table (TIME §6, OI-10) is deferred to v1.1.

## 5. Risks

| Risk | Mitigation |
|---|---|
| The Blue Books print few complete worked examples, so most vectors are hand-derived and can share a misreading with the code | Write vectors from the Blue Book text before writing the code; cross-check SPP, CUC, and CDS against an independent implementation (D5); ask a second person to review the hand-derived notes for EPP, CCS, and ASCII. **Done 2026-10-20:** [review](../verification/reviews/m1-vectors.md) by Gemini, independent of the sessions that derived the vectors; approved, with two corrections to unreleased files |
| No CI-usable QEMU machine for Cortex-R | D6 fallback: link check only, recorded as a deviation from VER-010 execution, revisited in M5. Did not occur: QEMU 8.2 runs the Cortex-R5F on its `none` machine (D6) |
| `u128` arithmetic or ASCII formatting brings in `core` code with panic paths (e.g. formatting machinery) | Format digits by hand, without `core::fmt`; the exerciser link check catches regressions, and WP-5 runs early, alongside WP-1 module 2 |
| Time-code edge cases (leap seconds, 24-bit days, 10 fractional octets) multiply the vector count | Property tests generate the combinations; vectors cover each boundary once |
| Ambiguities in the specifications found during implementation | Resolve against the Blue Book text, record as a new OI, and add a vector that pins the chosen reading |
