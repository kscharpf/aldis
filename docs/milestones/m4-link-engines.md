# Milestone 4 — Link engines

**Goal:** the link-layer engines of CCSDS frame generation and reception in `aldis-link`, as deterministic sans-I/O step functions with compile-time capacities: the PacketGenerator, the AOS BitstreamGenerator and VC Access generator, the ChannelMux with its multiplexing policies, the FrameReceiver, the PacketExtractor, the MapSegmenter and MapReassembler, and the CLTU packer, for TM, TC, AOS, and USLP. Each engine has its C ABI handle in `aldis-ffi`. Placement of the First Header Pointer and reassembly are verified by blind vectors and by property tests. Verified to the project standard: 100 % coverage, bare-metal panic-freedom, fuzzing, mutation testing, worst-case stack measurement, and hazard analysis.
**Roadmap entry:** [08-roadmap.md](../08-roadmap.md) §1 · **Builds on:** [M3](m3-cop1.md) (`v0.4.0-m3`) · **Release:** `v0.5.0-m4`

**Status: in progress.** Entry gate: M3 (roadmap §1), met by PR #211 (`v0.4.0-m3`); decisions D1–D5 (§2) accepted (maintainer, 2026-11-01).

| WP | State | PRs | Notes |
|---|---|---|---|
| Plan | in progress | | This plan, D1–D5, and `--scope m4` trace gate |
| WP-1 Spec readings | planned | | LNK rows checked clause by clause against the Blue Books; per-engine action bound and output-area bound; field codes 200–249 (API-057, API-062). No engine code |
| WP-2a Blind model & vectors: sending | planned | | Python model written blind (TV-023) and engine-scenario vectors for FHP placement, mux, segmentation |
| WP-2b Blind model & vectors: receiving | planned | | The same for the receiver, extraction, and reassembly |
| WP-3 PacketGenerator & PacketExtractor | planned | | Shared link config, action, and status types; absorbs issue #154 |
| WP-4 ChannelMux, policies, Access/Bitstream generators | planned | | |
| WP-5 FrameReceiver | planned | | |
| WP-6 MapSegmenter, MapReassembler, CLTU packer | planned | | FDUs fed to FOP-1 through `aldis-cop1` |
| WP-7 C ABI handles | planned | | One handle per engine; absorbs issue #217 |
| WP-8 Gates & hazards | planned | | |
| WP-9 Exit | planned | | |

A pull request that starts or merges a work package changes only its row (roadmap §6 rule 1).

History:
- 2026-11-01: plan drafted following the M3 release.

## 1. Starting point

| In place after M3 | Missing for M4 |
|---|---|
| `aldis-link`: skeleton crate in the workspace | All engine logic, configuration, action, and status types |
| Frame codecs `aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp`, and packet codecs `aldis-spp`, `aldis-epp`, with SPP and EPP tracker, segmenter, and reassembler in `aldis-core` where M1 placed them | Packet placement into data fields and extraction from them (FHP, M_PDU, TFDZ rule `000`), idle fill, release deadlines |
| `aldis-cop1`: FOP-1 and FARM-1 step functions, `buffer_available` and `BufferReleased` inputs of FARM-1, CLCW generation | FDUs of the segmenter fed to FOP-1 (LNK-043); CLCW delivery from the receiver to FOP-1 (LNK-053) |
| The engine step contract of [03-api-model.md](../03-api-model.md) §5–§6 (API-057…API-062, OI-56), proven by `Fop1` and `Farm1` | Its application to the link engines: action bound `N` and output-area bound of each engine |
| `aldis-ffi`: engine handle pattern (caller-owned opaque storage, magic tags, `capacities.toml`, limits) from `aldis_fop1_*` and `aldis_farm1_*` | Handles for the link engines; capacities entries for them |
| `test-vectors/schema/engine-scenario.schema.json`, the `aldis-testkit` engine-scenario loader, and the blind-model method of `test-vectors/models/cop1.py` | `test-vectors/link/` scenarios and a link model |
| Field-code block 200–249 reserved for `link` ([03](../03-api-model.md) §9.1) | Codes allocated for the link fields |
| Specification: [04-engines.md](../04-engines.md) (area LNK) | Clause-by-clause reading record; OI-11 settled |

## 2. Decisions

| # | Decision | Choice | Status |
|---|---|---|---|
| D1 | Trace scope | Add `--scope m4` from §4 to `cargo xtask ci`, the `spec` check of `cargo xtask verify`, and `ci.yml`, without `--strict`, in the plan PR, so progress is measured from the start; `--strict` at the M4 exit | Accepted (maintainer, 2026-11-01) |
| D2 | TM idle strategy (LNK-014, LNK-030; [OI-11](../09-open-issues.md)) | OI-11 is closed as written: a per-VC policy, defaulting to an OID frame on the dedicated idle VC (132.0 note 2). The reference scenario keeps its golden traces | Accepted (maintainer, 2026-11-01) |
| D3 | Protocol genericity (LNK-001) | Each engine is generic over a protocol trait with one implementation each for TM, TC, AOS, and USLP, not one type per protocol; protocol differences are configuration | Accepted (maintainer, 2026-11-01) |
| D4 | C ABI shape (CABI-030…033, CABI-040) | One opaque handle per engine type, the protocol chosen in its configuration, on the pattern of D10 of the [M3 plan](m3-cop1.md): caller-owned storage with magic tags, one step call per input, read-only queries | Accepted (maintainer, 2026-11-01) |
| D5 | Process freeze | None for M4 (unlike M3): a process fix is its own pull request | Accepted (maintainer, 2026-11-01) |

## 3. Work packages

Size: **S** ≈ small, **M** ≈ medium, **L** ≈ large. An engine work package starts after the blind model of its side has merged (TV-023).

```
                        ┌─▶ WP-2a Blind: sending ────┬─▶ WP-3 Generator & Extractor ─┬─▶ WP-4 Mux, generators ─┐
WP-0 Plan ─▶ WP-1 Spec ─┤                            │   (needs 2a and 2b)           └─▶ WP-5 FrameReceiver ───┼─▶ WP-7 C ABI ─▶ WP-8 Gates ─▶ WP-9 Exit
                        └─▶ WP-2b Blind: receiving ──┴─▶ WP-6 Segmenter, Reassembler, CLTU packer ───────────────┘
```

WP-6 needs more than WP-1 and `aldis-cop1`: its segmentation scenarios (LNK-040…042) come from WP-2a and its reassembly scenarios (LNK-070…072) from WP-2b, so it starts after both. LNK-043 (FOP-1 hand-off) and LNK-044 (CLTU packer) have no blind scenario: WP-6 tests them against `aldis-cop1` and with hand-derived cases.

### WP-0 Plan (S)
- This plan, D1–D5, and the `--scope m4` trace gate (D1); OI-11 resolved (D2).

### WP-1 Spec readings (M) — LNK-001…073, API-057, API-062
- Each LNK row checked clause by clause against 132.0, 232.0, 732.0, and 732.1; readings cite the clause.
- Per-engine action bound `N` and output-area bound (API-057, API-062); field codes from block 200–249 allocated in `docs/03-api-model.md` §9.1 and generated with `cargo xtask fields --write`.
- Checklist: "A spec-readings pull request before a codec". No engine code.

### WP-2a Blind model & vectors: sending (L) — LNK-011…014, 020, 021, 030…036, 040…042
- Python model written blind (TV-023) from the Blue Books and `docs/` only, with no implementation path given to its author.
- Engine-scenario vectors in `test-vectors/link/` for FHP placement, multiplexing, and segmentation.

### WP-2b Blind model & vectors: receiving (M) — LNK-050…054, 060…064, 070…072
- The same method for the receiver, extraction, and reassembly.

### WP-3 PacketGenerator & PacketExtractor (L) — LNK-010…015, 060…064, VER-023
- TM, AOS, and USLP fixed-length frames; release deadline, idle packets, gap resynchronisation.
- FHP placement and extraction round-trip property test (VER-023).
- Defines the shared link configuration, action, and status types, and the protocol trait (D3).
- Absorbs issue #154 (a `FrameLayout` constructor for a given data length).

### WP-4 ChannelMux, policies, Access and Bitstream generators (L) — LNK-020, 021, 030…036
- Fixed-priority, round-robin, and weighted policies; MC and VC counters; OID frames (D2); OCF and Insert Zone source slots.
- VC Access (VCA) and AOS Bitstream generators.

### WP-5 FrameReceiver (M) — LNK-050…054
- Validation, routing, gap events, OCF, Insert Zone, and FSH delivery; CLCW delivery for FOP-1.

### WP-6 MapSegmenter, MapReassembler, CLTU packer (L) — LNK-040…044, 070…073
- TC Segment Header and USLP variable-length rules; FDUs fed to FOP-1 (integration test through `aldis-cop1`).
- Reassembly round-trip property test.

### WP-7 C ABI handles (M) — CABI-030…033, CABI-040
- One opaque handle per engine (D4), the exerciser group, and the generated header.
- Absorbs issue #217 (consistency check: a `capacities.toml` entry for every engine).

### WP-8 Gates & hazards (M) — VER-010, VER-011, VER-022…025, VER-065, VER-066
- 100 % coverage, mutants triaged, fuzz targets, stack bounds at the `capacities.toml` sizes, QEMU on three targets.
- Hazard analysis `docs/verification/hazards/link.md` and the assurance case.

### WP-9 Exit (S) — VER-020, RS-015
- `cargo xtask trace --scope m4 --strict` replaces the non-strict run (VER-020); rustdoc citations (RS-015); roadmap, README, and open issues.
- The release `v0.5.0-m4` is the maintainer's.

## 4. Exit criteria

Milestone 4 is complete when every item below is green on `main`. The results column is filled at the WP-9 exit pull request.

| Area | Criteria | Results / evidence |
|---|---|---|
| Common engine behavior | LNK-001…003, RS-021 | |
| PacketGenerator | LNK-010…015 | |
| Access and Bitstream generators | LNK-020, LNK-021 | |
| ChannelMux | LNK-030…036 | |
| Segmenter and CLTU packer | LNK-040…044 | |
| FrameReceiver | LNK-050…054 | |
| PacketExtractor | LNK-060…064 | |
| Reassembler | LNK-070…073 | |
| Engine contract & sans-I/O | GEN-011, GEN-015, GEN-030…032, API-050, API-056, API-057, API-062, RS-020 | |
| C ABI handles | CABI-030…033, CABI-040, API-082 | |
| Panic freedom & resources | VER-010, VER-011, GEN-020…024 | |
| Portability | GEN-005…007, VER-030, VER-031 | |
| Verification gates | VER-022…025, VER-054 | |
| Vectors and trace | TV-001…003, TV-010…012, TV-020, TV-021, TV-023, VER-020, VER-021, VER-023 | |
| Hazards & assurance case | VER-065, VER-066 | |

Property tests for FHP placement and reassembly (VER-023) are part of the roadmap exit. The release `v0.5.0-m4` is the maintainer's.

Not in M4: C ABI versioning and the C++ wrapper `aldis.hpp` (M5); framework runtime stacks (M6); sync and channel coding (v1.1, OI-12).

## 5. Risks

| Risk | Mitigation |
|---|---|
| FHP and idle-packet edge cases that span frames (a packet header split across two data fields, a packet ending exactly at the field end, idle fill shorter than a packet header) | WP-1 reads each case against the Blue Book; WP-2a and WP-2b blind vectors name them; round-trip property tests of WP-3 (VER-023); fuzz targets of WP-8 |
| Scope: four protocols times seven engines | D3 makes protocols a trait, not a type each; WP-3 defines the shared types once and WP-4 to WP-6 reuse them; protocol-specific rules are configuration |
| C ABI handle sizes differ across targets | Handles built at the `capacities.toml` sizes in their first PR (checklist "Adding an engine"); sizes and alignments asserted and measured by `stack` and on three QEMU targets |
| Engine output bounds: `N` and the output area differ per engine | WP-1 fixes them per engine (API-057, API-062) before code; derived at compile time, as `FARM1_MAX_ACTIONS` is |
| A blind model written after the code agrees by accident | WP-2a and WP-2b merge before the engine WPs start (TV-023); the author receives no implementation path |

## 6. Hand-off to the other tracks

| Track | Needs from M4 | When |
|---|---|---|
| A — A4 (refapp engines) | The `aldis-link` engines replacing the hand-written loops of A2 and A3 | M4 exit (A4 entry gate) |
| ACFS — P4 (engines) | The C ABI handles of the link engines | M4 exit |
| R — M5 | The link handles in the C ABI for the sanitizer harness, header checks, and ABI versioning | M4 exit |
