# Milestone 6 — Crate architecture simplification

**Goal:** the simplest crate architecture that still meets every requirement. Most protocols (TM, TC, AOS, USLP) were added one work package at a time by copying a sibling, so the codecs, the link engines, and the C ABI each restate protocol facts. The pattern of this milestone: **each protocol fact is stated once, in the codec crate or in `aldis-core`, and the engine, FFI, and feature layers are generic over it.** The work changes no wire behaviour: every vector, every requirement, and the trace stay as they are.
**Roadmap entry:** [08-roadmap.md](../08-roadmap.md) §1 · **Builds on:** [M5](m5-c-abi-completion.md) (`v0.6.0-m5`) · **Release:** none planned; the maintainer decides whether it ships as `v0.7.0-m6` or with the next release

**Status: planned.** Decisions D1–D4 (§2) accepted (maintainer, 2026-11-08 and 2026-11-09, chat); D5–D7 proposed. Entry gate: M5 (roadmap §1), met: its exit criteria are met on `main` ([M5 plan](m5-c-abi-completion.md) §4; exit pull request #283, `cargo xtask trace --scope m5 --strict`); the release `v0.6.0-m5` is the maintainer's and is not a condition of the gate.

| WP | State | PRs | Notes |
|---|---|---|---|
| WP-0 Phase 0: cheap clean-ups | | | Merged (State comes from `cargo xtask status-table`): the no-op `cop1` `tc` feature, the `DeliveryService`/`ReassemblyService` renames, the FFI `link_common` module and the single `qos`, clamps in the mux, USLP, and EPP turned into explicit errors, the writer fallbacks audited (#387); the A2 plan wording (#389); GEN-006 withdrawn and the `alloc`/`std` features removed (#391, D3). |
| WP-1a Channel-lookup trait | | | Phase 1 step 1 |
| WP-1b One checking pipeline | | | Phase 1 step 2 |
| WP-1c Generic proof types | | | Phase 1 step 3 |
| WP-1d Generic frame stream | | | Phase 1 step 4 |
| WP-2a Frame geometry | | | Phase 2 step 1 |
| WP-2b Header writers and parsers | | | Phase 2 step 2; `bits` primitives (D2) |
| WP-2c Managed-parameter sets | | | Phase 2 step 3 |
| WP-2d One frame writer | | | Phase 2 step 4 |
| WP-2e Link codec dependencies | | | Scope question OI-97; may be cut |
| WP-3a `InPlace` and `place` | | | Phase 3 step 1 |
| WP-3b Generic engine entry points | | | Phase 3 step 2 |
| WP-3c One FFI module per engine | | | Phase 3 step 3 |
| WP-3d Generated sizes and constants | | | Phase 3 step 4 |
| WP-4 Exit | | | `cargo xtask trace` against the entry baseline; exit checklist |

A pull request that starts or merges a work package changes only the Notes column of its row (roadmap §6 rule 1). The State and PRs columns are derived by `cargo xtask status-table` and written by the coordinator; title a pull request `refactor(m6/wp-1b): …` ([pr-body.md](../process/pr-body.md)).

History:
- 2026-11-08: crate-architecture review (three read-only agents over the codecs and core, the link and COP-1 engines, and the FFI and testkit); D1–D3 decided by the maintainer in chat.
- 2026-11-09: D4 decided; plan drafted; the open decisions proposed as OI-91…OI-97.

## 1. Starting point

| In place after M5 and Phase 0 | Missing for M6 |
|---|---|
| One codec crate per Blue Book (GEN-003), each with its own copy of the channel lookup (four variants with different signatures), of the frame check (TM `validate`, AOS `validate`, TC and USLP `validate_frame`, three shapes), and of the proof types (a `Plain…Frame` and a `Validated…Frame` in each of four crates) | One lookup trait, one `check` pipeline with one rejection type, generic `Plain<F>` and `Checked<F>` in `aldis-core` |
| TC and USLP frame streams that mirror each other item for item | One `FrameStream<D, L>` |
| `aldis-link` engines each generic over their own protocol trait (nine of them), with the header length and the primary-header length restated at dozens of sites; the mux writes TM, AOS, and USLP headers by hand beside the codec writers; USLP frames come from two encoders (the mux and FOP-1) | One frame-format trait per codec (geometry, header writer, parser) that the mux and FOP-1 use |
| Three or four configuration models of one physical channel (the codec's, the mux's, the receiver's, FOP-1's), with the cross-parameter checks (LNK-078) spread over them | One checked managed-parameter set per physical channel and VC, defined in the codec |
| `protect`, `finalize`, `len`, `octets`, and `into_octets` re-implemented in each writer's typestate tail | One `FrameWriter` over the existing frame layout in `aldis-core` |
| Field extraction and insertion by hand-written shifts in the codecs and the mux, against GEN-051 | The `aldis_core::bits` primitives used by every codec and the mux (D2) |
| `aldis-ffi`: every link engine's handle hand-writes its protocol dispatch, tag numbers, `unsafe` blocks, and entry-point boilerplate; helpers imported across the `link_*` modules; storage word counts and protocol constants typed by hand | A trait for in-place construction, one generic placement, a generic or macro-generated entry-point set, one FFI module per engine, and generated sizes |

## 2. Decisions

| # | Decision | Choice | Status |
|---|---|---|---|
| D1 | Direction | Phases 1–3 of the review (§3): one checking pipeline, one frame-format trait per protocol, then a generic FFI. Phase 0 is done or in a train | Accepted (maintainer, 2026-11-08) |
| D2 | GEN-051 | Met by adopting the `aldis_core::bits` primitives in the codecs and the mux, not by amending the rule | Accepted (maintainer, 2026-11-08) |
| D3 | GEN-006 and features | GEN-006 withdrawn; the `alloc` and `std` features removed from Layers 0–2 ([#391](https://github.com/kscharpf/aldis/pull/391)) | Accepted (maintainer, 2026-11-08) |
| D4 | `refapp/Cargo.lock` | [#387](https://github.com/kscharpf/aldis/pull/387) may change `refapp/Cargo.lock`, a track A path, because the facade change forces it ([roadmap §6](../08-roadmap.md) rule 2 otherwise keeps a pull request to one track's directories) | Accepted (maintainer, 2026-11-09) |
| D5 | Kept as they are | One codec crate per Blue Book (GEN-003); `aldis-sync`; the facade; the `security`, `layout`, and `finishing` primitives and `DecodeMode`/`Lenient` of `aldis-core`; `engine::Actions`; FOP-1's state table; the FFI handle lifecycle (`handle.rs`), `mirror.rs`, and `abi::call`; the separation of the testkit's model and oracle. No work package rewrites them, and a pull request that must touch one says why | Proposed |
| D6 | Behaviour-preserving | No wire behaviour, requirement, vector, or field or status code changes. A released vector is never edited (TV-021). The Rust API may break (0.x); a change of the C ABI follows the checklist "Adding C ABI functions" and raises `ALDIS_ABI_VERSION_MINOR`, and the C ABI conventions listed in §5 are the maintainer's | Proposed |
| D7 | Trace gate | The exit table of §4 lists, in its Criteria column, the requirements the refactor touches (GEN-051, LNK-001, LNK-078, API-015, API-017, RS-020, RS-023, and the rest it names), so that `cargo xtask trace --scope m6 --strict` measures them (roadmap §6 rule 1; the "Milestone exit" checklist); the scope reads those IDs from the table, and adding it to `cargo xtask ci`, `ci.yml`, and the check `spec` is a small `xtask` pull request of WP-0, as in M5 D1. The milestone adds no requirement, so the unscoped `cargo xtask trace` must also report the same coverage at the exit as at the entry | Proposed |

## 3. Work packages

Size: **S** ≈ small, **M** ≈ medium, **L** ≈ large. Every package is a series of pull requests that each leave `main` green (`cargo xtask verify`), so that the milestone can stop after any of them. Remove the old API in the pull request that migrates its last caller; keep no compatibility layer.

```
WP-0 ─▶ WP-1a ─▶ WP-1b ─▶ WP-1c ─▶ WP-1d ─┐
   └──▶ WP-2a ─▶ WP-2b ─▶ WP-2c ─▶ WP-2d ─┼─▶ WP-3a ─▶ WP-3b ─▶ WP-3c ─▶ WP-3d ─▶ WP-4
                      └─▶ WP-2e (OI-97)   ┘
```

Phase 1 and the first steps of Phase 2 touch different crates and may run side by side, except where both change `aldis-core` exports: land WP-1a before WP-2c.

### WP-0 Phase 0: cheap clean-ups (S) — merged
- #387, #389, #391 (see the table); all three have merged to `main`, and the later packages build on them. The part of the review's Phase 0 that is not yet done is left to the packages below: the small enums, the retired field code, and the testkit driver are open decisions (§5).

### WP-1 Phase 1, one checking pipeline (L) — about 1.5k lines removed — API-015, API-017, RS-020, RS-023
- **WP-1a:** one channel-lookup trait in `aldis-core`, generic over the identifier widths and the VC configuration, with the MAP lookup a second trait; it replaces the four `VcLookup` variants and the duplicated room check.
- **WP-1b:** one pipeline, decode, `into_plain`, then `check(&lookup)`, with one rejection type; it replaces the TM and AOS `validate` and the TC and USLP `validate_frame`. `validate.rs` becomes `checked.rs`, matching the RS-020 and RS-023 vocabulary.
- **WP-1c:** generic `Plain<F>` and `Checked<F>` next to `Lenient`, replacing the eight per-crate proof newtypes (which have drifted: only one is `Copy`, only one has `as_frame`).
- **WP-1d:** a generic `FrameStream<D, L>` over a delimiter, replacing the mirrored TC and USLP streams.
- Order: lookup trait, pipeline, proof types, stream. Each step migrates every caller (the link receiver, FOP-1's intake, `aldis-ffi`, the testkit, the exerciser, the fuzz targets, `refapp/` only through the facade).
- Checklist: "Adding a codec module" for the moved items; the facade docs and the crates' READMEs follow.

### WP-2 Phase 2, one frame-format trait per protocol (L) — the core of the milestone — GEN-051, LNK-001, LNK-078
- **WP-2a Geometry:** each codec exposes a frame-format trait that carries the geometry (the primary-header length and the lengths derived from the managed parameters). The nine per-engine protocol traits of `aldis-link` shrink to the geometry of the codec plus the few capabilities in which engines differ (LNK-001 still holds: an engine is generic over a protocol trait). Start here: it is incremental and removes the restated lengths.
- **WP-2b Header writers and parsers:** the trait gains the header writer and the parser; the mux and FOP-1 use the codec writers. This removes the hand-written TM, AOS, and USLP headers of the mux and the second USLP encoder. The codecs and the mux read and write fields through the `aldis_core::bits` primitives (D2).
- **WP-2c Managed-parameter sets:** one checked managed-parameter set per physical channel and VC, defined in the codec (it is Blue Book table data); the mux, the receiver, and FOP-1 take it plus their engine-only extras. The cross-parameter checks of LNK-078 live in its constructor. This replaces the codec's, the mux's, the receiver's, and FOP-1's channel configurations.
- **WP-2d One frame writer:** a core `FrameWriter<'b, S>` over the existing frame layout writes FECF and SDLS finishing once; each codec supplies only its header and data-field plan. The writers' differing `finalize` results are settled here.
- **WP-2e Link codec dependencies:** decide whether `aldis-link`'s codec dependencies become non-optional. A scope question for the maintainer (OI-97); the package is cut if the answer is no.
- The mux `Offer` that carries fields "ignored by AOS and USLP" becomes `Offer<P>` with the protocol's own offer metadata, as part of WP-2b.
- Checklist: "Adding an engine" (the stack check at the `capacities.toml` sizes) for every engine that changes shape; "Adding a codec module".

### WP-3 Phase 3, the C ABI generic over the engines (L) — about 2–3k lines removed; after WP-2 — CABI-030…032, CABI-040
- **WP-3a:** an `InPlace` trait (head type, buffers type, head offset) with no `unsafe`, and one generic `place<E>` in `handle.rs`, replacing the per-engine head, offset, and placement functions and the `engine_in_place!` macro.
- **WP-3b:** a `c_engine!` macro or a generic dispatcher that emits the init, step, status, deadline, and deinit entry points; the hand-numbered variant tags and the roughly twenty `variant_payload` calls go with it.
- **WP-3c:** one FFI module per engine, mirroring `aldis-link/src`; the shared helpers move out of `link_gen.rs` to a module of their own.
- **WP-3d:** `xtask` generates the storage word counts (now typed by hand, next to a generator that already feeds `capacities.toml`) and the protocol constants (`ALDIS_*_PRIMARY_HEADER_LEN` and the like) from the Rust constants; `cargo xtask fix` lists the generator and `cargo xtask ci` checks it current.
- The exported symbols, struct layouts, and header text stay as they are unless a decision of §5 says otherwise; the C vectors, the C++ tests of `aldis.hpp`, and `crates/aldis-ffi/tests/abi.rs` prove it.
- Checklist: "Adding C ABI functions"; the C++ owner told when the header changes (roadmap §6 rule 4).

### WP-4 Exit (S) — VER-020, RS-015
- The exit checklist ("Milestone exit"); `cargo xtask trace --scope m6 --strict` and the unscoped trace compared with the entry baseline (D7); the roadmap row, README status, and this plan's status; open issues of §5 resolved or moved.

## 4. Exit criteria

Milestone 6 is complete when every item below is green on `main`. The results column is filled at the WP-4 pull request.

| Area | Criteria | Results / evidence |
|---|---|---|
| Phases 1–3 | Phase 1 (WP-1), Phase 2 (WP-2, with WP-2e decided), and Phase 3 (WP-3) merged | |
| Trace | `cargo xtask trace --scope m6 --strict` passes (GEN-051, LNK-001, LNK-078, API-015, API-017, RS-020, RS-023), and the unscoped `cargo xtask trace` reports the same requirement coverage as at the entry: no requirement loses its test, vector, or record (D7) | |
| Behaviour | The shared vectors pass through the Rust codecs and engines, the C ABI, and `aldis.hpp`, with no vector file edited (D6); the golden traces of track A are unchanged | |
| Single statement of protocol facts | One lookup trait, one check pipeline, one proof-type pair, one stream, one frame-format trait per codec, one frame writer, one in-place trait; no hand-written header in `aldis-link`; no field extraction by hand-written shifts in a codec or the mux (GEN-051, D2) | |
| Gates | Coverage at 100 % without a raised waiver (VER-024), mutants triaged (VER-025), `stack` within its budgets at the `capacities.toml` sizes (VER-011), the exerciser panic-free link check (VER-010), the fuzz targets building | |
| Open decisions | OI-91…OI-97 resolved by the maintainer, or moved to a later milestone with a note | |

Not in M6: new protocol features, new requirements, the framework ([05](../05-framework.md)), and the C++ native track.

## 5. Open decisions

Each is the maintainer's (decision rights: public API and C ABI conventions, scope) and is proposed as an open issue in [09](../09-open-issues.md) with a recommended option. Work that the answer does not touch continues.

| OI | Question | Recommended | Blocks |
|---|---|---|---|
| OI-91 | `#[repr(C)]` head and buffers layout in the Rust engines | Plain Rust engines; in-place construction only through `InPlace` | WP-3a |
| OI-92 | Flat C config struct checked twice, and EPP and SPP `check` functions | Protocol-tagged C config; EPP and SPP return proof types | WP-3b, WP-3c |
| OI-93 | FOP-1 frame encoding | FOP-1 emits FDUs and a `FrameKind`, framed by the codec | WP-2b |
| OI-94 | Retired field-code kind (field code 18) | A `retired` kind in [03 §9.1](../03-api-model.md#91-field-codes) | none |
| OI-95 | Testkit generic `ScenarioEngine` driver | Low priority; after WP-3 | none |
| OI-96 | Duplicated small enums (COP frame kinds in several places, MAP service shapes) | Fold into WP-2 where the frame-format trait removes them | WP-2b |
| OI-97 | Do `aldis-link`'s codec dependencies become non-optional | Decide after WP-2c, when the dependencies are visible | WP-2e |

## 6. Risks

| Risk | Mitigation |
|---|---|
| A refactor that preserves the tests and loses a requirement's evidence | D7: `cargo xtask trace` before and after; each pull request lists the requirements whose tests it moved |
| Stack growth of an engine whose type gets another generic layer | The `stack` check at the `capacities.toml` sizes in every WP-2 and WP-3 pull request (VER-011) |
| A large series stalls with `main` half-migrated | Each step migrates all callers and deletes the old API (§3); the series may stop after any step |
| The generic code hides a clamp or a panic path | Clippy with the gate's lints, and the exerciser link check; the "no clamped fallbacks" rule of [AGENTS.md](../../AGENTS.md) applies to code moved as to new code |
| Track A rides on the APIs that change | The refapp uses the facade; a pull request that changes an API the refapp calls fixes the call in the same pull request, with D4 as the precedent for `refapp/Cargo.lock` |

## 7. Hand-off to the other tracks

| Track | Needs from M6 | When |
|---|---|---|
| A (all) | Unchanged behaviour through the facade; the Rust API of codecs and engines changes in places (0.x), and the pull request that changes it fixes the refapp call sites | Each WP |
| C ABI integrators and `aldis.hpp` | Unchanged symbols and layouts unless OI-91 or OI-92 is decided otherwise; the header regenerated | WP-3 |
