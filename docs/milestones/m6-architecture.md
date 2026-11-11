# Milestone 6 — Crate architecture simplification

**Goal:** the simplest crate architecture that still meets every requirement. Most protocols (TM, TC, AOS, USLP) were added one work package at a time by copying a sibling, so the codecs, the link engines, and the C ABI each restate protocol facts. The pattern of this milestone: **each protocol fact is stated once, in the codec crate or in `aldis-core`, and the engine, FFI, and feature layers are generic over it.** The work changes no wire behaviour: every vector, every requirement, and the trace stay as they are.
**Roadmap entry:** [08-roadmap.md](../08-roadmap.md) §1 · **Builds on:** [M5](m5-c-abi-completion.md) (complete; its release `v0.6.0-m5` was made 2026-11-06) · **Release:** none planned; the maintainer decides whether it ships as `v0.7.0-m6` or with the next release

**Status: Phases 1–2 merged; the OI-93 follow-up of WP-2d is #475; Phase 3 and the exit deferred.** Decisions D1–D8 (§2) accepted (maintainer, 2026-11-08 and 2026-11-09, chat). Entry gate: M5 (roadmap §1), met: its exit criteria are met on `main` ([M5 plan](m5-c-abi-completion.md) §4; exit pull request #283, `cargo xtask trace --scope m5 --strict`); its release `v0.6.0-m5` was made 2026-11-06. WP-3a–WP-3d and WP-4 wait until after the AINT interoperability milestone ([roadmap §4](../08-roadmap.md#4-overlap), maintainer, 2026-11-10); the OI-93 follow-up of WP-2d may still land before then.

| WP | State | PRs | Notes |
|---|---|---|---|
| WP-0 Phase 0: cheap clean-ups | | | #387, #389, #391 (via train #401) |
| WP-1a Channel-lookup trait | merged | #414 | Phase 1 step 1 |
| WP-1b One checking pipeline | merged | #424 | Phase 1 step 2; `check_frame` returns `Lenient<Checked<F>, A>` in both modes (empty anomaly set after a strict check); `Checked<F>` replaces the four `Validated…Frame` types, the `Plain…` types stay for WP-1c; refapp needs a companion PR (D8) |
| WP-1c Generic proof types | merged | #441, #442 | Phase 1 step 3 |
| WP-1d Generic frame stream | merged | #445 | Phase 1 step 4; `VcLookup` aliases removed from the four codecs; refapp needs a companion PR (D8) |
| WP-2a Frame geometry | merged | #449 | Phase 2 step 1; `FrameFormat` in `aldis-core` (geometry only), implemented by `TmFormat`, `TcFormat`, `AosFormat`, `UslpFormat`; `aldis-link`'s `Tm`, `Tc`, `Aos`, `Uslp` are those markers and exist with the codec feature (the `CltuPacker` impls too) |
| WP-2b Header writers and parsers | merged | #451, #458 | Phase 2 step 2; `bits` primitives (D2) |
| WP-2c Managed-parameter sets | merged | #456, #467 | Phase 2 step 3; first slice: the physical-channel set (`FrameFormat::Physical`, the codec's `*PhysicalChannelConfig`) is embedded by the mux and receiver channel types and taken by `Fop1Target::tc`, which no longer restate the frame length, FECF, Insert Zone, FHEC, or edition; second slice: the VC sets, the mux's `TmVc`, `AosVc`, `UslpVc` and the receiver's `TmVc` embed the codec's VC configuration (`TmVirtualChannelConfig`, `AosVirtualChannelConfig`, `VcConfig` of `aldis-uslp`), the segmenters take the FECF and the VCF Count Lengths from the codec's sets, and `UslpChannel` takes `mc_ocf` (#465); the receiver's AOS, TC, and USLP VC params stay (OI-92) |
| WP-2d One frame writer | merged | #472, #475 | Phase 2 step 4; the writer (first PR, #472); FOP-1 builds its frames through `FrameFormat` and the codec's frame-writer plans (OI-93; second PR, #475) |
| WP-2e Link codec dependencies | | | Cut: OI-97 resolved (a), the codec dependencies stay optional (maintainer, chat, 2026-11-10) |
| WP-3a `InPlace` and `place` | | | Phase 3 step 1 |
| WP-3b Generic engine entry points | | | Phase 3 step 2 |
| WP-3c One FFI module per engine | | | Phase 3 step 3 |
| WP-3d Generated sizes and constants | | | Phase 3 step 4 |
| WP-4 Exit | | | `cargo xtask trace` against the entry baseline; exit checklist |

A pull request that starts or merges a work package changes only the Notes column of its row (roadmap §6 rule 1). The State and PRs columns are derived by `cargo xtask status-table` and written by the coordinator; title a pull request `refactor(m6/wp-1b): <description>`, the type being a conventional-commit type and the scope naming the milestone and work package ([pr-body.md](../process/pr-body.md)).

History:
- 2026-11-08: crate-architecture review (three read-only agents over the codecs and core, the link and COP-1 engines, and the FFI and testkit); D1–D3 decided by the maintainer in chat.
- 2026-11-09: D4 decided; plan drafted; the open decisions proposed as OI-91…OI-97.

## 1. Starting point

| In place after M5 and Phase 0 | Missing for M6 |
|---|---|
| One codec crate per Blue Book (GEN-003), each with its own copy of the channel lookup (four variants with different signatures), of the frame check (TM `validate`, AOS `validate`, TC and USLP `validate_frame`, three shapes), and of the proof types (a `Plain…Frame` and a `Validated…Frame` in each of four crates) | One lookup trait, one `check` pipeline with a generic `Rejection<R>`, and orthogonal `Plain<F>` and `Checked<F>` proof types in `aldis-core` |
| TC and USLP frame streams that mirror each other item for item | One `FrameStream<D, L>` over a protocol delimiter trait |
| `aldis-link` engines each generic over their own protocol trait (nine of them), with the header length and the primary-header length restated at dozens of sites; the mux writes TM, AOS, and USLP headers by hand beside the codec writers; USLP frames come from two encoders (the mux and FOP-1) | One frame-format trait per codec (geometry, header writer, parser) that the mux and FOP-1 use |
| Three or four configuration models of one physical channel (the codec's, the mux's, the receiver's, FOP-1's), with the cross-parameter checks (LNK-078) spread over them | One checked managed-parameter set per physical channel and VC, defined in the codec |
| `protect`, `finalize`, `len`, `octets`, and `into_octets` re-implemented in each writer's typestate tail | One `FrameWriter` over the existing frame layout in `aldis-core`, handling fixed- and variable-length frames and pre-FECF hooks |
| Field extraction and insertion by hand-written shifts in the codecs and the mux, against GEN-051 | The `aldis_core::bits` primitives (with `const`/infallible fixed-slice helpers) used by every codec and the mux (D2) |
| `aldis-ffi`: every link engine's handle hand-writes its protocol dispatch, tag numbers, `unsafe` blocks, and entry-point boilerplate; helpers imported across the `link_*` modules; storage word counts and protocol constants typed by hand | A trait for in-place construction, one generic placement, a generic or macro-generated entry-point set, one FFI module per engine, and generated sizes |

## 2. Decisions

| # | Decision | Choice | Status |
|---|---|---|---|
| D1 | Direction | Phases 1–3 of the review (§3): one checking pipeline, one frame-format trait per protocol, then a generic FFI. Phase 0 has merged | Accepted (maintainer, 2026-11-08) |
| D2 | GEN-051 | Met by adopting the `aldis_core::bits` primitives in the codecs and the mux, not by amending the rule | Accepted (maintainer, 2026-11-08) |
| D3 | GEN-006 and features | GEN-006 withdrawn; the `alloc` and `std` features removed from Layers 0–2 ([#391](https://github.com/kscharpf/aldis/pull/391)) | Accepted (maintainer, 2026-11-08) |
| D4 | `refapp/Cargo.lock` | [#387](https://github.com/kscharpf/aldis/pull/387) may change `refapp/Cargo.lock`, a track A path, because the facade change forces it ([roadmap §6](../08-roadmap.md) rule 2 otherwise keeps a pull request to one track's directories) | Accepted (maintainer, 2026-11-09) |
| D5 | Kept as they are | One codec crate per Blue Book (GEN-003); `aldis-sync`; the facade; the `security`, `layout`, and `finishing` primitives and `DecodeMode`/`Lenient` of `aldis-core`; `engine::Actions`; FOP-1's state table; the FFI handle lifecycle (the handle types and their create, use, and destroy rules in `handle.rs`; WP-3a and WP-3b may add the in-place trait and the generic dispatchers to that file), `mirror.rs`, and `abi::call`; the separation of the testkit's model and oracle. No work package rewrites them, and a pull request that must touch one says why | Accepted (maintainer, 2026-11-09, chat) |
| D6 | Behaviour-preserving | No wire behaviour, vector, or status code changes, and no requirement's meaning changes. Requirement text may be reworded to the new names (WP-1b: `validate` to `check`, as in `docs/standards/tm-frame.md`; WP-1c: RS-023 and API-015…017), and each such pull request lists the rewordings. Field codes change only through the decision of OI-94. A released vector is never edited (TV-021). The Rust API may break (0.x); a change of the C ABI follows the checklist "Adding C ABI functions" and raises `ALDIS_ABI_VERSION_MINOR`, and the C ABI conventions listed in §5 are the maintainer's | Accepted (maintainer, 2026-11-09, chat) |
| D7 | Trace gate | The exit table of §4 lists, in its Criteria column, the requirements the refactor touches, so that `cargo xtask trace --scope m6 --strict` measures them (roadmap §6 rule 1; the "Milestone exit" checklist); the scope reads those IDs from the table, and adding `--scope m6` to `cargo xtask ci`, `ci.yml`, `xtask/src/trace.rs` (`test_scope_ids`), and the check `spec` is part of WP-1a, as in M5 D1. The milestone adds no requirement, so the unscoped `cargo xtask trace` must also report the same coverage at the exit as at the entry | Accepted (maintainer, 2026-11-09, chat) |
| D8 | Track A files | Roadmap §6 rule 2 stands. An M6 change that breaks `refapp/` gets a companion track-A pull request in the same train; M6 pull requests do not edit `refapp/` sources. D4 was a one-time exception for #387 and sets no precedent | Accepted (maintainer, 2026-11-09, chat) |

## 3. Work packages

Size: **S** ≈ small, **M** ≈ medium, **L** ≈ large. Every package is a series of pull requests that each leave `main` green (`cargo xtask verify`), so that the milestone can stop after any of them. Remove the old API in the pull request that migrates its last caller; keep no compatibility layer.

```
WP-0 ─▶ WP-1a ─▶ WP-1b ─▶ WP-1c ─▶ WP-1d ─▶ WP-2a ─▶ WP-2b ─▶ WP-2c ─▶ WP-2d ─┬─▶ WP-3a ─▶ WP-3b ─▶ WP-3c ─▶ WP-3d ─▶ WP-4
                                                                  └─▶ WP-2e (OI-97) ─┘
```

Both Phase 1 and Phase 2 edit the four frame codecs (`aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp`), so Phase 1 (`WP-1a`–`WP-1d`) lands before Phase 2 modifies the codec crates; only the `aldis-link`-internal protocol-trait consolidation of `WP-2a` may overlap before `WP-1d` finishes.

### WP-0 Phase 0: cheap clean-ups (S)
- #387, #389, #391 (via train #401). They hold the no-op `cop1` `tc` feature, the `DeliveryService`/`ReassemblyService` renames, the FFI `link_common` module and the single `qos`, clamps in the mux, USLP, and EPP turned into explicit errors, the writer fallbacks audited, the A2 plan wording, and GEN-006 withdrawn with the `alloc`/`std` features removed (D3). The later packages build on them. The part of the review's Phase 0 that is not yet done is left to the packages below: the small enums, the retired field code, and the testkit driver are open decisions (§5).

### WP-1 Phase 1, one checking pipeline (L) — about 1.5k lines removed — GEN-060, GEN-061, API-015…017, API-040, API-041, API-073, API-074, LNK-050, TM-011, AOS-010, AOS-016, TC-020…023, USLP-014…016, RS-020, RS-023
- **WP-1a:** one channel-lookup trait in `aldis-core`, generic over the identifier widths and the VC configuration, with the MAP lookup a second trait; it replaces the four `VcLookup` variants and the duplicated room check (while keeping `NoSdls` / unparameterized TM decode entry points working without a dummy physical-channel table). Also registers `--scope m6` in `cargo xtask ci`, `ci.yml`, `xtask/src/trace.rs` (`test_scope_ids`), and the `spec` check (D7).
- **WP-1b:** one checking pipeline in `aldis-core` defining `Checked<F>` and `Rejection<R>` (`R = ()` for TM and AOS; `R = Reason` for TC 1–11 and USLP 1–18), returning `Result<Checked<F>, Rejection<R>>` (or `Result<Lenient<Checked<F>, A>, Rejection<R>>` in lenient mode). For TC and USLP it provides a byte-slice entry point (`check_frame(bytes, &lookup, mode)`) that keeps the order of today's `validate_frame`; the Blue Books list the tests (a)–(e) without an order (232.0 §4.4.9.3.3, 732.1 §4.3.11.3.4), so the order is an implementation choice. Test (d) compares `bytes.len()` with the Frame Length of a non-truncated frame (232.0 §4.4.9.3.3 d; 732.1 §4.3.11.3.4 d)1)) and, for a truncated USLP frame, with the VC's Truncated Transfer Frame Length managed parameter (732.1 §4.3.11.3.4 d)2)), as the current USLP check does; for TM and AOS (and already-decoded frames whose layout matches `&lookup`) it provides `frame.check(&lookup)` (removing `AosFrame::unconfigured`). `validate.rs` becomes `checked.rs`, matching the RS-020 and RS-023 vocabulary. Checking operates whether or not SDLS is configured on the channel (unlike `into_plain(NoSecurity)`, which requires empty security fields).
- **WP-1c:** generic `Plain<F>` (SDLS verified or `NoSecurity`, API-073, API-074; taken by `FrameReceiver`, which also routes frames on unconfigured VCs and MAPs per LNK-050 and OI-77) alongside `Checked<F>` (channel-checked against `&L`, API-016; taken by FARM-1's intake per TC-023 and yielded by `FrameStream`) as orthogonal proof wrappers next to `Lenient` (`into_plain` converts `F` or `Checked<F>` via a small security-slice trait in `aldis-core`), replacing the eight per-crate proof newtypes (which have drifted: only one is `Copy`, only one has `as_frame`) and, as OI-92 (b) decides, returning proof types from EPP `EppConfig::check` and SPP `check_len`/`check_parts`. Updates the wording of RS-023 and API-015…017 so `Checked` names `Checked<F>` unambiguously.
- **WP-1d:** a generic `FrameStream<D, L>` over a delimiter trait `D`, replacing the mirrored TC and USLP streams (TC-020, USLP-016). `D` parameterizes the protocol's stream-discard reason enum (TC reasons 21–22 vs USLP reasons 21–23) and its header-peek and incomplete-frame rules (TC's `check_head` on version-0 truncated frames; USLP's OID VC 63 fallback and advance past an unexpected-version frame when its declared or VC-truncated length fits in the remaining slice vs fall-through to `Fill`/`LeftoverFill`). WP-1d also removes the per-crate `VcLookup` bound aliases that WP-1a leaves in place, so the generic code names `ChannelLookup` directly.
- Order: lookup trait, checking pipeline + `Checked<F>`, `Plain<F>` + caller migration, stream. Each step migrates every caller (the link receiver, FARM-1's intake, `aldis-ffi`, the testkit, the exerciser, the fuzz targets); `refapp/` is migrated by a companion track-A pull request (D8).
- Checklist: "Adding a codec module" for the moved items; the facade docs and the crates' READMEs follow.

### WP-2 Phase 2, one frame-format trait per protocol (L) — the core of the milestone — GEN-051, LNK-001, LNK-035, LNK-078, COP-010, COP-011
- **WP-2a Geometry:** each codec exposes a frame-format trait that carries the geometry (the primary-header length and the lengths derived from the managed parameters). The nine per-engine protocol traits of `aldis-link` shrink to the geometry of the codec plus the few capabilities in which engines differ (LNK-001 still holds: an engine is generic over a protocol trait). Start here: it is incremental and removes the restated lengths.
- **WP-2b Header writers and parsers:** the trait gains the header writer and the parser (preserving the existing check order of the `Upid::OnlyIdleData` rule `001` and `ConstructionRule` (732.1 §4.1.4.1.9 defines the OID frame fields) and replacing `receiver/uslp.rs`'s duplicate header/TFDF parser with `UslpFrame`'s view methods); the mux and FOP-1 use the codec writers (`Fop1Output::Transmit` still writes encoded TC and USLP frames to `out` per COP-010, COP-011, and `fop1.json`, delegating frame construction to the codec's trait rather than maintaining a second USLP/TC encoder in `fop1.rs`; OI-93). Under OI-96, the shared COP frame-kind enum moves to `aldis-core` (shared by `aldis-tc`, `aldis-uslp`, `aldis-link`, and `aldis-cop1`) and the unified MAP service enum lives in `aldis-link`. This removes the hand-written TM, AOS, and USLP headers of the mux and the second USLP encoder. The codecs and the mux read and write fields through the `aldis_core::bits` primitives (D2), extended with `const`-friendly, fixed-slice helpers that avoid `u128` iterator folds (keeping Kani proof times fast, OI-42, VER-059) and unreachable `Err` branches on already-sized buffers (preserving 100 % coverage without waivers, VER-024, and keeping existing outer length-error field codes).
  Status: #451 implements the header writers, `Offer<P>`, the USLP receiver change, and the COP frame kind; #458 implements the MAP service enum (OI-96) and the remaining codec bit-shift cleanup; FOP-1 construction through the trait (OI-93) moves to WP-2d.
- **WP-2c Managed-parameter sets:** one checked managed-parameter set per physical channel and VC, defined in the codec (it is Blue Book table data); the mux, the receiver, the segmenter/reassembler, and FOP-1 embed or borrow it plus their engine-only scheduling/service extras (weights, initial OCF/FSH buffers, delivery services, deadlines). The cross-parameter checks of LNK-078 live in its constructor. This replaces the duplicated framing parameters across the codec's, the mux's, the receiver's, and FOP-1's channel configurations. The VC-level slice embeds the codec's whole VC configs (`TmVirtualChannelConfig` and its siblings) in the engines' VC params (maintainer, 2026-11-10). Exception to D6 (maintainer, 2026-11-10): the USLP receiver refuses a frame length above 65 536 octets or below the codec minimum, which it accepted before.
  Exception to D6 (maintainer, 2026-11-10): five Rust-API refusals of configs the mux or segmenter could not serve; the C ABI is unchanged.
  Decision (maintainer, 2026-11-10): embedding the codec's VC configuration in the AOS, TC, and USLP receiver VC params is deferred to WP-3, to be decided with OI-92, which settles whether the C configuration gains an SCID, a maximum frame length, and a Segment Header.
  Decision (maintainer, 2026-11-10): the TM OID VC check stays in `ChannelMuxConfig::new`.
  Status of the VC-level slice: the mux VCs (`TmVc`, `AosVc`, `UslpVc`) and the receiver's `TmVc` embed the codec's VC configuration; the VC identifier, the SDLS lengths, the OCF and Secondary Header lengths, and the TM content (`DataFieldContent`, replacing `TmRole`) are the codec's, and the engine adds the initial slot values, MAP and `QoS`, COP-1, and whether the receiver serves the VC. `TcSegmenterParams` takes the codec's physical channel (the FECF), and `UslpSegmenterParams` the codec's `VcParams` (the VCF Count Lengths). The AOS, TC, and USLP receiver VC params do not embed the codec's VC configuration (it needs an SCID, a maximum frame length, or a Segment Header that the C configuration does not carry, OI-92) and the reassembler's channel parameters are unchanged.
- **WP-2d One frame writer:** a core `FrameWriter<'b, S>` over the existing frame layout writes FECF and SDLS finishing once; each codec supplies only its header and data-field plan. It supports both fixed-length frames (TM and AOS, where payload length is fixed at `begin` and `protect(hook, ctx)` seals in place) and variable-length frames (TC and USLP, where `begin` reserves the maximum buffer and `protect(data_len, hook, ctx)` / `unsecured(data_len)` relocate the Security Trailer, rewrite the Primary Header length and USLP version, and write the USLP OCF at its final place, the FECF being computed last by `finalize`), a pre-FECF protocol hook (such as AOS FHEC calculation), and an unsealed/zeroed FHEC-and-FECF mode for `ChannelMux` (LNK-035). The writers' differing `finalize` results are settled here.
  Also here: FOP-1 builds its TC and USLP frames through the `FrameFormat` trait (OI-93, moved from WP-2b by the maintainer, chat, 2026-11-10).
- **WP-2e Link codec dependencies:** decide whether `aldis-link`'s codec dependencies become non-optional. A scope question for the maintainer (OI-97); the package is cut if the answer is no. Cut: the maintainer resolved OI-97 (a) in chat on 2026-11-10, the codec dependencies stay optional.
- The mux `Offer` that carries fields "ignored by AOS and USLP" becomes `Offer<P>` with the protocol's own offer metadata, as part of WP-2b.
- Checklist: "Adding an engine" (the stack check at the `capacities.toml` sizes) for every engine that changes shape; "Adding a codec module".

### WP-3 Phase 3, the C ABI generic over the engines (L) — about 2–3k lines removed; after WP-2 — GEN-070, CABI-002, CABI-030…032, CABI-040
- **WP-3a:** an `unsafe trait InPlace` defined in `crates/aldis-ffi/src/handle.rs`, with its `unsafe impl`s for the `aldis-link` and `aldis-sync` engines and for `Fop1State` also in `aldis-ffi` (a local trait on foreign types; GEN-070 allows `unsafe` only there, so `aldis-core`, `aldis-link`, and `aldis-sync` stay plain Rust with no layout constants). The trait names the head type and the buffers type, and its offsets are derived with `core::mem::offset_of!`, never written by hand; it has a `fn finish_place(&mut self) {}` hook for post-init setup such as `ChannelMux` filling its Insert Zone slot with `idle_pattern`. Its `# Safety` section states what each impl guarantees: the offsets are the real field offsets of the engine (guaranteed by `offset_of!`), the buffers type is valid when all of its bytes are zero, and `Head` and `Buffers` are the only fields of the engine, so that the zeroed slot with the head written is a valid engine before `finish_place(&mut self)` runs. One generic `unsafe fn place<E: InPlace>` asserts at compile time (`const { assert!(...) }`) that `Head` and `Buffers` are disjoint and fit within `size_of::<E>()`, zeroes the entire `size_of::<E>()` slot first (initializing all zero-valid buffer arrays and padding bytes for Miri, VER-026), writes `head` at the head offset, and calls `finish_place` on the placed engine (OI-91). It replaces the per-engine head, offset, and placement functions across `aldis-link`, `aldis-sync` (`AsmSearch`), and `aldis-ffi` (`Fop1State`), `aldis_link::inplace`, and the `engine_in_place!` macro.
- **WP-3b:** generic dispatcher helpers in `handle.rs` (covering the three `_step` shapes: timed with `out`, timed without `out` for `bitstream_generator`, and untimed with `out`) called by thin top-level `#[unsafe(no_mangle)] pub unsafe extern "C" fn` entry points (so `cbindgen` parses every symbol without macro expansion, CABI-002, and each keeps its RS-015 doc comment); the hand-numbered variant tags and the roughly twenty `variant_payload` calls go with it.
- **WP-3c:** one FFI module per engine, mirroring `aldis-link/src`; the shared helpers move out of `link_gen.rs` to a module of their own.
- **WP-3d:** `xtask` generates or verifies the storage word counts (`ALDIS_*_WORDS`, via `aldis-ffi` layout emission since `xtask` does not link `aldis-ffi`) and the protocol/action constants (`ALDIS_*_PRIMARY_HEADER_LEN`, `ALDIS_*_MAX_ACTIONS`, and the like) from `capacities.toml` and the Rust constants; `cargo xtask fix` lists the generator and `cargo xtask ci` checks it current.
- The exported symbols, struct layouts, and header text stay as they are unless a decision of §5 says otherwise; the C vectors, the C++ tests of `aldis.hpp`, and `crates/aldis-ffi/tests/abi.rs` prove it.
- Checklist: "Adding C ABI functions"; the C++ owner told when the header changes (roadmap §6 rule 4).

### WP-4 Exit (S) — VER-020, RS-015
- The exit checklist ("Milestone exit"); `cargo xtask trace --scope m6 --strict` and the unscoped trace compared with the entry baseline (D7); the roadmap row, README status, and this plan's status; open issues of §5 resolved or moved.

## 4. Exit criteria

Milestone 6 is complete when every item below is green on `main`. The results column is filled at the WP-4 pull request.

| Area | Criteria | Results / evidence |
|---|---|---|
| Phases 1–3 | Phase 1 (WP-1), Phase 2 (WP-2, with WP-2e decided), and Phase 3 (WP-3) merged | |
| Trace | `cargo xtask trace --scope m6 --strict` passes (GEN-051, GEN-060, GEN-061, GEN-070, LNK-001, LNK-035, LNK-050, LNK-078, COP-010, COP-011, TM-011, AOS-010, AOS-016, TC-020…023, USLP-014…016, API-015…017, API-040, API-041, API-073, API-074, CABI-002, CABI-030…032, CABI-040, RS-015, RS-020, RS-023, VER-020; every ID in the headings of §3 is in this list), and the unscoped `cargo xtask trace` reports the same requirement coverage as at the entry: no requirement loses its test, vector, or record (D7) | |
| Behaviour | The shared vectors pass through the Rust codecs and engines, the C ABI, and `aldis.hpp`, with no vector file edited (D6); the golden traces of track A are unchanged | |
| Single statement of protocol facts | One lookup trait, one check pipeline, one proof-type pair, one stream, one frame-format trait per codec, one frame writer, one in-place trait; no hand-written header in `aldis-link`; no field extraction by hand-written shifts in a codec or the mux (GEN-051, D2) | |
| Gates | Coverage at 100 % without a raised waiver (VER-024), mutants triaged (VER-025), `stack` within its budgets at the `capacities.toml` sizes (VER-011), the exerciser panic-free link check (VER-010), the fuzz targets building | |
| Open decisions | OI-91…OI-97 resolved by the maintainer, or moved to a later milestone with a note | |

Not in M6: new protocol features, new requirements, the framework ([05](../05-framework.md)), and the C++ native track.

## 5. Open decisions

Each is the maintainer's (decision rights: public API and C ABI conventions, scope) and is proposed as an open issue in [09](../09-open-issues.md) with a recommended option. Work that the answer does not touch continues.

| OI | Question | Recommended | Blocks |
|---|---|---|---|
| OI-91 | `#[repr(C)]` head and buffers layout in the Rust engines | Plain Rust engines; zero-first in-place construction through an `unsafe trait InPlace` and `place<E>`, both in `aldis-ffi` | WP-3a |
| OI-92 | Flat C config struct checked twice, and EPP and SPP `check` functions | Protocol-tagged C config (WP-3b, WP-3c); EPP and SPP return proof types (WP-1c). **Partly resolved (maintainer, 2026-11-10, chat): (b);** the EPP and SPP part is implemented in WP-1c, the C config is left to WP-3b and WP-3c | WP-3b, WP-3c |
| OI-93 | FOP-1 internal frame encoding | FOP-1 delegates frame construction to the codec's frame-format trait (`Fop1Output::Transmit` still writes frames to `out`). **Resolved (b) by the maintainer, chat, 2026-11-10; implemented by #451, #472 and #475** | WP-2b, WP-2d |
| OI-94 | Retired field-code kind (field code 18) | A `retired` kind in [03 §9.1](../03-api-model.md#91-field-codes); the maintainer decides; then the pull request that implements the `retired` kind edits the field code 18 row of 03 §9.1 (crate `core`, API-061), and no other pull request does | none |
| OI-95 | Testkit generic `ScenarioEngine` driver | Low priority; after WP-3 | none |
| OI-96 | Duplicated small enums (COP frame kinds in several places, MAP service shapes) | Fold into WP-2b (`aldis-core` for COP frame kinds, `aldis-link` for MAP service). **Resolved (b) by the maintainer, chat, 2026-11-10; implemented in part by #451 (COP frame kind), the MAP service enum by #458** | WP-2b |
| OI-97 | Do `aldis-link`'s codec dependencies become non-optional | Decide after WP-2c, when the dependencies are visible. **Resolved (a) by the maintainer, chat, 2026-11-10; WP-2e cut** | WP-2e |

## 6. Risks

| Risk | Mitigation |
|---|---|
| A refactor that preserves the tests and loses a requirement's evidence | D7: `cargo xtask trace` before and after; each pull request lists the requirements whose tests it moved |
| Stack growth of an engine whose type gets another generic layer | The `stack` check at the `capacities.toml` sizes in every WP-2 and WP-3 pull request (VER-011) |
| A large series stalls with `main` half-migrated | Each step migrates all callers and deletes the old API (§3); the series may stop after any step |
| The generic code hides a clamp or a panic path | Clippy with the gate's lints, and the exerciser link check; the "no clamped fallbacks" rule of [AGENTS.md](../../AGENTS.md) applies to code moved as to new code |
| Track A rides on the APIs that change | The refapp uses the facade. An M6 pull request that breaks `refapp/` gets a companion track-A pull request in the same train; M6 pull requests do not edit `refapp/` sources (D8, roadmap §6 rule 2) |

## 7. Hand-off to the other tracks

| Track | Needs from M6 | When |
|---|---|---|
| A (all) | Unchanged behaviour through the facade; the Rust API of codecs and engines changes in places (0.x); a change that breaks `refapp/` comes with a companion track-A pull request in the same train (D8) | Each WP |
| C ABI integrators and `aldis.hpp` | Unchanged symbols and layouts unless OI-91 or OI-92 is decided otherwise; the header regenerated | WP-3 |
