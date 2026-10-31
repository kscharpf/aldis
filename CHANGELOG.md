# Changelog

All notable changes to Aldis are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) (GEN-080). Every fix to a deviation from a CCSDS Blue Book is called out under **Fixed**.

## [Unreleased]

Generated at release: `cargo xtask changelog` lists the pull requests merged after the base commit below, from their titles (conventional commits: `feat` is Added, `fix` is Fixed, `perf`, `refactor`, and any `!` are Changed, with **Breaking:** for `!`; `docs`, `chore`, `test`, `ci`, and `build` are not listed); `cargo xtask changelog release <version> <YYYY-MM-DD>` writes the section.

<!-- changelog-base: 945701a978603ce01530fe034f7676d48fba5745 -->

## [0.4.0-m3] — 2026-10-31

Milestone 3: COP-1 (FOP-1 and FARM-1 engines, their C ABI, hazard analysis).

### Added
- `cargo xtask check`: format, clippy, tests, the bare-metal clippy build, and `cargo xtask ci`, plus the extras of each touched directory, in one command.
- `.claude/hooks/session-start.sh` (SessionStart hook): warns when the checkout is behind `origin/main` or its branch is merged or its upstream gone.
- `scripts/reverify` (merge `origin/main`, check, push, and dispatch the verification run for open PRs) and `scripts/prune-agent-branches` (dry-run cleanup of finished agent worktrees and branches).
- `scripts/pr-status` (is a pull request ready to merge: required contexts, the newest verify run, the latest review, a `READY` or `NOT READY` verdict), `scripts/cleanup-merged` (remove the worktree and branches of a merged pull request, only when its tip is on `origin/main`), `scripts/test-cleanup-merged` (its test, with no network and no real `gh`), and `docs/process/agent-brief.md` (the conventions for briefing and reporting of a dispatched agent).
- `scripts/agent-env.sh`: helper script configuring agent shell environment variables (`NO_COLOR=1`, `GH_FORCE_TTY=0`, `GH_NO_UPDATE_NOTIFIER=1`) and `gh run-status` alias (#139).
- VER-068: `cargo xtask check` runs every command of the `fmt`, `clippy`, and `test` scripts of the verification run, each with `--locked`: the formatting of `exerciser/`, `fuzz/`, and `crosscheck/` when they change, `cargo doc` with `RUSTDOCFLAGS=-D warnings`, `cargo clippy` of `fuzz/` (after a change to a crate or to `fuzz/`) and of `crosscheck/`, so a change that passes `check` no longer fails the gate on them. A test fails when a line of those scripts has no step.
- CABI-005: `cargo xtask consistency` checks that every `aldis_` and `ALDIS_` name in `docs/profiles/c-abi.md` and `crates/aldis-ffi/README.md` is declared by `include/aldis/aldis.h` (outside its comments), or is listed with its reason in `NOT_IN_HEADER`; placeholders such as `ALDIS_<STD>_REASON_*` are not checked. The `c-smoke` step compiles the usage example of the ffi README against the header.
- `aldis-tc`: in-place `FrameStream` iterator unifying frame delimiting, fill handling, and channel validation over ingested data units with `DiscardReason` and `IntakeItem` (TC-020, TC-021).
- `aldis-uslp`: in-place `FrameStream` iterator unifying frame delimiting, fill handling, and channel validation over ingested data units with `DiscardReason` and `IntakeItem` (USLP-014, USLP-015).
- `aldis-ffi`: unified stream reception C ABI entry points `aldis_tc_stream_next` and `aldis_uslp_stream_next` with associated types and status codes (CABI-001).
- Nightly check `audit`: runs `cargo audit` on all workspace `Cargo.lock` files against the RustSec Advisory Database, reporting advisories immediately (VER-059).
- `scripts/reverify` takes a lock per pull request (shared by all worktrees, released at the PR's final line or however the script ends): a PR number is digits only (`07` is `7`); a second run of the same PR is refused with the holder's pid, `--wait` queues behind it, a lock whose process is gone is taken over. `--stale-retries <n>` (default 2) brings a STALE PR up to date and verifies it again; a FAILED one is never retried. `--status <pr>` prints the lock or the last final line (with its head) in one line with no network, and an aborted run leaves no record, and `--status --wait <pr>` waits for it. `scripts/test-reverify` tests the script with no network, container, or real `gh` and `cargo`.
- `aldis-core`: allocated `ErrorKind::InternalError = 18` for reporting violations of internal invariants and data consistency checks (API-001, GEN-020).
- `aldis-ffi`: exposed `ALDIS_ERR_INTERNAL_ERROR = -18` in C ABI (CABI-001).
- `cargo xtask fuzz smoke`: fast smoke iteration (`-runs=1 -max_total_time=2`) across all fuzz targets with corpus seeds when present, preventing stale harness assertions from passing merge gates (#135).
- Verify check `fuzz-smoke`: gating verification check running `cargo xtask fuzz smoke` on changes to Rust crates, fuzz targets, or workspace dependencies (#135).
- `cargo xtask verify part <group>` and `verify assemble`: the checks of a hosted verification run in four parts, assembled into one record under the unchanged merge gate (VER-067).
- `cargo xtask fix` runs the generators (field codes, vector schema, SANA tables, C header), then `consistency`.
- `aldis-tm`, `aldis-aos`: the one-shot Only Idle Data encoders `encode_oid` and `encode_oid_galois` (TM-012, AOS-021).
- `aldis-ffi`: `aldis_tm_encode_oid` and `aldis_aos_encode_oid`, the one-shot Only Idle Data frame encoders, which take an `aldis_oid_generator_t` as `aldis_uslp_encode_oid` does (TM-012, AOS-021).
- `xtask`: `cargo xtask status [--write]`, which checks the error catalogue and generates `crates/aldis-ffi/src/status.rs` from it.
- `docs/verification/reviews/cop1-tables.md`: cell-by-cell review record of CCSDS 232.1-B-2 Table 5-1 (FOP-1, 300 cells) and Table 6-1 (FARM-1, 33 cells) including Corrigendum 1 amendments (COP-003, VER-041).
- `docs/03-api-model.md`: reserved and allocated COP-1 field codes (150–164) in `aldis_core::field`, `test-vectors/core/fields.json`, and C ABI constants in `crates/aldis-ffi/src/field.rs` (API-082).
- `docs/milestones/m3-cop1.md`: decomposed WP-1 into subtasks WP-1a (spec readings & cell review), WP-1b (common types & FARM-1 table), and WP-1c (FOP-1 table).
- `docs/milestones/m3-cop1.md`: detailed milestone plan for Milestone 3 (COP-1) covering FOP-1 and FARM-1 state tables as data, blind vectors, closed-loop simulation, and C ABI handles.
- `cargo xtask trace --scope m3`: added Milestone 3 scope tracking to `cargo xtask ci` and the verify `spec` check.
- `aldis-core`: module `finishing` with the sealed `FrameState` and its states `Unprotected`, `Protected`, and `Finalized`, and `require_unsecured` (RS-023).
- `aldis-core`: the mandatory `SecurityHook::layout` (every hook states an exact `SecurityLayout` or `HookLayout::Any`; `NoSecurity` is `Exact(NONE)`), `security::HookLayout`, `finishing::require_layout`, and the test-only `PreFilledSecurity`. A writer's `protect` rejects a hook whose layout differs from the channel's, so `NoSecurity` cannot be applied to a channel with SDLS fields.
- `aldis_core::Lenient::try_map`, so a lenient decode followed by `check_fecf` or `validate` keeps its anomalies (API-032).
- **Breaking:** the C function `aldis_tm_validate` has a new `anomalies` out parameter before `err`, which receives the `ALDIS_TM_ANOMALY_*` flags of the frame it accepted; the ABI minor version is 7 (CABI-060).
- `aldis_core::Lenient` and `DecodeMode::dispatch`, for code that carries the mode as a value.
- Requirements API-015 and API-016; API-030 to API-032 restated for the two entry points.
- `aldis-uslp`: `UslpFrame::layout`, `UslpFullFrame::layout`, `UslpTruncatedFrame::layout`, and `UslpFrameWriter::layout` return the `FrameLayout` of the frame, as the TM, TC, and AOS views do (CMN-040).
- `crates/aldis-cop1/src/notify.rs`: defined common COP-1 types `Directive`, `AlertReason`, `Alert`, `TransferNotification`, `DirectiveNotification`, and `TimeoutType` (CCSDS 232.1-B-2 Section 4, COP-004).
- `crates/aldis-cop1/src/farm1_table.rs`: represented FARM-1 State Table 6-1 as `const` data with closed enums `Farm1State`, `Farm1Event`, and `Farm1Action` (COP-002, COP-003, RS-021).
- `crates/aldis-cop1/tests/farm1_table.rs`: added exhaustive 33-cell unit tests covering all states S1–S3 across events E1–E11 against CCSDS 232.1-B-2 Table 6-1 and `docs/verification/reviews/cop1-tables.md` (COP-002, COP-003, COP-004, RS-021).
- `crates/aldis-cop1/src/fop1_table.rs`: represented FOP-1 State Table 5-1 as `const` data with closed enums `Fop1State`, `Fop1Event` (50 variants), `Fop1Action`, `FrameKind`, and `BcCommand` (COP-001, COP-003, COP-004, RS-021).
- `crates/aldis-cop1/tests/fop1_table.rs`: added exhaustive 300-cell unit tests covering all states S1–S6 across events E1–E46 and E101–E104 against CCSDS 232.1-B-2 Table 5-1, Cor. 1, and `docs/verification/reviews/cop1-tables.md` (COP-001, COP-003, COP-004, COP-005, RS-021).
- `test-vectors/cop1/`: eight generated files (`fop1_table.json`, `farm1_table.json`, six by topic) with one scenario per cell of CCSDS 232.1 Table 5-1 (FOP-1, 300 cells) and Table 6-1 (FARM-1, 33 cells), "Not applicable" cells asserting `InvalidState`, and scenarios for the normal run, retransmission, the sliding window, the timer, suspension, and the FARM-1 window (COP-001, COP-002, COP-005, TV-011).
- `test-vectors/models/cop1.py`: the independent model of FOP-1 and FARM-1 behind those scenarios, written blind from the Blue Books (TV-023).
- `aldis-cop1`: the FARM-1 engine (M3 WP-3; COP-002, COP-006, COP-030…COP-036). `Farm1::step` is a pure step function over plain-data state that executes the cells of `FARM1_TABLE` (232.1 Table 6-1) with exhaustive matching, returns the next state and at most `FARM1_MAX_ACTIONS` actions (derived from the table at compile time), and writes an accepted FDU to a caller-supplied output area; `Farm1::clcw` is the pure E11 query. `Farm1Window` checks W, PW, and NW against 232.1 §6.1.8 and Table 7-2. Also `Actions` and `OutRange`, the action list and output-area range shared by the engines (API-057, API-061).
- `aldis-tc`: `FarmBCounter::next`, the increment modulo 4 (232.1 §6.1.6).
- `docs/09-open-issues.md`: OI-58, the start-up values of FARM-1.
- `exerciser`: calls the FARM-1 API; registers the `cop1` vector group, empty until M3 WP-2.

### Changed
- `cargo xtask check` prints each step's wall-clock time, after the step and in the final summary line.
- `specs/fetch.sh` links `specs/pdf` from the main checkout when it has the PDFs; `.gitignore` ignores `specs/pdf` also as a symlink.
- spec-lint: a withdrawn requirement starts `Withdrawn: <reason>` and in no other form; `trace` and the assurance check recognise only that form (00-introduction.md §5.2).
- `docs/06-verification.md` §7 holds the merge-gate commands that AGENTS.md carried.
- A change to `scripts/` alone runs only `spec` in a verification run, as documentation does: no check runs or reads those scripts (VER-056).
- `AGENTS.md`: added conventions on workspace search boundaries (avoiding `.claude/`, `.git/`, and external caches; preferring `git grep`) and structured GitHub CLI queries to prevent runaway scans and background timeouts (#139).
- Documentation overhead (OI-63): requirement rows state obligations, and `cargo xtask assurance` rejects code names in them (`docs/verification/assurance-evidence.md` §2.5a); a requirement verified by a check of the verification run has no prose record, and `cargo xtask trace` reads the check from a table; provenance is stated once per file (TV-024); pull request bodies have three sections (`docs/process/pr-body.md`); documentation mismatches are reviewed as one batch of nits; crate READMEs keep the example and "where it sits".
- Removed 20 verification record files that restated a check.
- `cargo xtask assurance`: a code span that names an `aldis_` symbol, a `.rs`, `.h`, or `.toml` path, or a `::` path is an error in the hazard analysis, the assurance case, the verification records, the cross-checks, and the formal-proofs file (VER-065, VER-066); the new reference kind `sym:` resolves an `aldis_` name against the C header.
- Documentation that restated code is removed: wire diagrams, configuration tables, and vector lists of `docs/standards/`, the API sketches of the Rust profile, the illustrative C of the C ABI profile, and the status text of the crate READMEs. Dated reviews carry a banner.
- Docs and comments: removed leftover output-queue and `poll_*` wording from the M3 plan, the container module doc, and the engine-scenario `expect_timeout_us` description (OI-56).
- `xtask`: `cargo xtask loops` is a check again, in `cargo xtask ci` and the verify check `spec`. A loop over a slice, an array, or a closed range is bounded by its type; every `while`, `while let`, `loop`, and open or endless iteration needs a `// bound: <reason>` comment directly above it, and a comment that is not above a loop fails (GEN-022).
- `docs/verification/records/GEN-022.md`, `docs/06-verification.md`, `docs/verification/assurance-case.md`, `AGENTS.md`: GEN-022 loop bounds are checked by `cargo xtask loops`; recursion stays with the `stack` check; no inspection tables to maintain.
- `aldis-core`, `aldis-ffi`: five loops annotated with their bounds (no behaviour change).
- `docs/`: removed the inspection-approval residue: `records/VER-061.md`, the decision-rights row for approving inspection records, and the glossary entry "Inspection record"; the VER-061 row is marked Withdrawn (the ID is not reused).
- `xtask`: `trace --scope` leaves requirements marked Withdrawn or Deleted out of the scope.
- `README.md`: updated verification section to link to worst-case stack bounds (VER-011), assurance case, and hazard analysis following Milestone 2 completion, removing the outdated "Still to come in Milestone 2" placeholder.
- `docs/02-design-rules.md`: GEN-022 and GEN-023 verification method changed from Inspection (`I`) to Analysis (`A`), backed by static object-level call-graph cycle detection (`cargo xtask stack`, VER-011), `#![no_std]` architecture invariants, and formal proofs.
- `xtask`: `cargo xtask loops` removed from CI and verify checks; simplified to a diagnostic utility `cargo xtask loops [list]` that scans library crates for loops.
- Decoupled shared toolchain pins (`QEMU_VERSION`, `LLVM_VERSION`, `GCC_VERSION`, `GCC_S390X_*`) to root `toolchains.lock`.
- Reopened OI-08: LEON/SPARC support remains best-effort Rust (tier 3 nightly with `-Z build-std=core`, non-gating check `rust-sparc-leon3`); no qualified LEON path is claimed for v1.
- Updated `docs/08-roadmap.md`, `README.md`, `docs/hardware-assumptions.md`, `docs/profiles/c-abi.md`, and `docs/profiles/cpp.md`.
- `scripts/reverify` verifies a pull request locally, with no image and no engine, when its merged head needs no container, as for a stacked pull request whose base has merged; other heads still go to the hosted run.
- `docs/08-roadmap.md` §6 gains a stacking rule (rule 10) and a test (`scope.rs`) shows that merging `main` into a stacked branch reuses every check (VER-056).
- `cargo xtask verify --publish` and `verify part` refuse a head that does not contain the tip of `origin/main` before running any check (`--stale-ok` runs it anyway); the first step of `verify.yml` stops such a run within seconds, and a run that finishes behind `main` ends with a `STALE` line, a `Behind main` annotation, and a job-summary headline (VER-058, VER-060).
- `scripts/reverify` labels a run that did not succeed STALE or FAILED by whether the head contains the tip of `origin/main`.
- `docs/process/agent-brief.md` carries the rules for running `scripts/reverify` (one run per PR, leave the branch alone while it runs, wait for the final line, STALE retries); `AGENTS.md` says that implementation agents commit and stop unless the brief grants the gate.
- `scripts/reverify`: prepares and dispatches every pull request first, then waits for all verify runs together and prints one summary line per PR; a branch already up to date with `main` reuses a successful or in-progress run of its head instead of dispatching another.
- `aldis-core`, `aldis-tm`, `aldis-tc`, `aldis-uslp`, `aldis-time`: eliminated silent default-fallback slice indexing on untrusted wire inputs. Replaced wire input fallbacks with explicit errors (`Incomplete`, `LengthMismatch`, `InvalidField`, `Malformed`).
- The hosted verification run's groups are balanced by measured durations (`fuzz-smoke` joins `coverage`, the Cortex-R exerciser joins `embedded`), and a part starts pulling the image while it checks out the repository; the runner's disk is cleared only when it has less than 40 GB free (VER-067).
- The hosted verification run (`verify.yml`, VER-060) makes the checks in four parallel parts and assembles one record from them (VER-067), in place of one serial job.
- The typical durations that order a verification run are the measured durations of a hosted run, so that the quickest checks still run first.
- The hosted verification run builds xtask once and passes it to the other jobs, which install no Rust toolchain (the composite action `.github/actions/get-xtask` decides whether a job may use it); `prepare` finds the image by a manifest lookup instead of pulling it, and builds and pushes it only when the registry says the manifest does not exist (new `cargo xtask verify image --ref <repository>`). The `no-std-*` builds move from the `cross` group to `host`, and the typical durations that order and balance a run are the means of three full hosted runs (VER-060, VER-067).
- The coverage gate, the `features` crate list, and the C ABI vector areas are derived from the workspace (crates of Layers 0-2 with code; `aldis-ffi/src/<area>.rs`) instead of stored in `GATED`, `CRATES`, and `FFI_AREAS`; `consistency` no longer carries the four checks that compared those lists.
- Specification only: the maintainer decided seven C ABI conventions on 2026-10-29 (one-shot encoders only, input enumerations as plain integers, `uint32_t` status codes, typed caller storage, no `struct_size` or `aldis_limits`, engine state accessed in place, engine inputs and actions as flat structs with `aldis_range_t` ranges, no unions, header C99). They are recorded in `docs/profiles/c-abi.md` section 12 and OI-57, with the spikes that checked them; CABI-040 is rewritten now, and the other rows change in the follow-up pull requests that implement each convention. OI-50 is superseded.
- `aldis-ffi`: the C ABI's enumerations are mirror enums (`src/mirror.rs`): one fieldless `#[repr(u8)]` or `#[repr(u32)]` enum per closed set of `ALDIS_*` values, parsed by one generic `parse` into a Rust enum, then converted to the core type by an exhaustive `match` (OI-57 convention 2; CABI-010, CABI-022). The hand-written constant and `match` pairs of the AOS, OCF, OID, SPP, TC, time, TM, and USLP modules and of the decode mode are gone.
- `include/aldis/aldis.h`: the `ALDIS_*` enumeration constants keep their names and values, but are enumerators of an `enum` that no field uses and no longer `#define`s, so `#if ALDIS_X` on one no longer works. C ABI version 0.4.0 (CABI-060).
- `docs/`: `docs/profiles/c-abi.md` (CABI-010, CABI-022, §10, §12) describes the mirror enums.
- `aldis-ffi`: `handle.rs` lends the state in caller storage to a closure in place (`Handle::read`, `Handle::update`), the mechanism for engine calls without a copy of the state on the stack (OI-57 convention 6; CABI-040). The SPP tracker, the SPP reassembler, and OID generator calls that take no caller buffer use it. The C ABI is unchanged.
- `docs/`: the introduction of `docs/profiles/c-abi.md` §6 and §12 describe the mechanism.
- `aldis-ffi`: **Breaking:** the ABI version is 0.2.0, since the two-phase writers are gone.
- `docs/`: API-023 describes the Rust typestate writers only; the C ABI keeps no state between calls (OI-57). OI-50 is superseded and the stateless-finish rows of `docs/profiles/c-abi.md` are withdrawn.
- `aldis-ffi`: **Breaking:** every offset and length pair of a C view is one `aldis_range_t { size_t offset; size_t len; }` member (OI-57 convention 7; CABI-012): `data_offset`/`data_len` of `aldis_spp_packet_t` and `aldis_epp_packet_t` are `data`; the layouts of TM, AOS, TC, and USLP (`aldis_tm_layout_t`, `aldis_aos_layout_t`, `aldis_tc_layout_t`, `aldis_uslp_layout_t`) hold `fsh_data`, `insert_zone`, `segment_header`, `security_header`, `tfdf_header`, `data`, `zone`, `tfdz`, `security_trailer`, `ocf`, and `fecf` ranges; `aldis_spp_split_t` holds `secondary_header`, `user_data`, and `ancillary`; the TC and USLP stream items hold the `range` of the item in the unit. A range of length 0 is an absent part at the offset where it would start.
- `aldis-ffi`: **Breaking:** configuration and detail structs have no `struct_size` (OI-57 convention 5). `aldis_error_detail_t` is `{ kind, field, detail, needed, expected, found }`, 32 octets; the library writes it whole when a call fails, and `reserved`, which only padded it, is gone. An array of configuration structs is a plain C array. The ABI major version is the only compatibility mechanism. C ABI version 0.6.0 (CABI-060).
- `docs/`: `docs/profiles/c-abi.md` (CABI-012, CABI-021, CABI-030, §10, §11, §12) describes ranges and the structs without `struct_size`.
- `aldis-ffi`: a fallible C function returns a `uint32_t`: `ALDIS_OK` (0) or the catalogue code of the error, the value of `ErrorKind::code()` (OI-57 convention 3; CABI-020). The `ALDIS_ERR_*` constants keep their names and are positive (`ALDIS_ERR_BUFFER_TOO_SMALL` is 1, not −1); they are the enumerators of the mirror enum `aldis_status`, generated from the error catalogue of `docs/03-api-model.md` §9 by the new `cargo xtask status [--write]` (run by `cargo xtask fix` and `ci`), with a compile-time assertion that each equals `ErrorKind::<Kind>.code()`; a test checks that every `ErrorKind::ALL` entry has its constant. `aldis_error_detail_t::kind` equals the returned status. `aldis_tc_stream_item_t::error_code` and `aldis_uslp_stream_item_t::error_code` are `uint32_t`. C ABI version 0.5.0 (CABI-060).
- `test-vectors/core/errors.json`: the `c_abi` expectation recorded the negated code; each case is superseded by an `-002` case with `status` (TV-021).
- `docs/`: `docs/profiles/c-abi.md` (CABI-010, CABI-020, §10, §12) describes the status codes.
- `aldis-ffi`: **Breaking:** the ABI version is 0.3.0. The storage of `aldis_spp_tracker_t`, `aldis_spp_segmenter_t`, `aldis_spp_reassembler_t`, and `aldis_oid_generator_t` is a typed struct of `uint64_t words[N]` that the caller declares, so the C compiler supplies its size and alignment. Their `_init` functions take a pointer to that struct, which is also the handle: the `storage_len` and `out` parameters go (OI-57 convention 4).
- `docs/`: CABI-031 and CABI-072, the memory model of `docs/profiles/c-abi.md` §5, and GEN-012 describe the typed storage.
- Breaking: the engine contract of `docs/03-api-model.md` §5–§8 is now a pure step function over plain-data state that returns the next state and a bounded action list (API-050…API-062, API-083), in place of output queues drained by `poll_*` calls. The deadline is a pure query, FARM-1 buffer availability is an input (COP-033), and SDLS is applied by the caller around an engine's frames, in the order of API-074 (OI-56).
- `docs/standards/cop1.md`, `docs/04-engines.md`, `docs/05-framework.md` (FW-014), `docs/profiles/rust.md` (RS-022, RS-023), `docs/profiles/c-abi.md` (CABI-040), and the glossary follow the new contract.
- `aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp`: the two-phase frame writers are a finishing typestate (RS-023, API-074). `begin` returns the writer `Unprotected`; `protect(hook, ctx)` (SDLS) or `unsecured()` (no SDLS) gives it `Protected`; `finalize()` computes the FHEC (AOS) and the FECF last and gives it `Finalized`, the only state with `octets`, `into_octets`, and `len`. `finish` is removed. On TC and USLP `finalize` returns a `Result` (`InternalError` instead of an ignored CRC result), and a failed `protect` restores the unfinished header. For TC and USLP, `protect` and `unsecured` take the User Data or TFDZ length that `finish` took. The AOS writer no longer computes the FHEC in `begin`.
- **Breaking:** the decoders of every codec crate have separate strict and lenient entry points instead of a `DecodeMode` argument (API-030): `decode` is strict and returns the view alone, and `decode_lenient` (`decode_with_lenient`, `decode_unchecked_lenient`, `decode_with_pfield_lenient`, `decode_implicit_lenient`, `validate_frame_lenient`, `LenientFrameStream`) returns the view in the new `aldis_core::Lenient`, which holds the `AnomalySet`. The views no longer have an `anomalies` accessor or field. The C ABI keeps its `mode` argument and `anomalies` fields.
- **Breaking:** `TmFrame::validate` and `EncapsulationPacket::validate` take the view by value and return `ValidatedTmFrame` and `ValidatedEncapsulationPacket` instead of `Result<()>` (API-016, RS-020). `validate_frame` of `aldis-tc` and `aldis-uslp` returns `ValidatedTcFrame` and `ValidatedUslpFrame`. The frame streams of `aldis-tc` and `aldis-uslp` yield validated frames, and `Farm1Frame::from_tc` takes a `ValidatedTcFrame` (API-016).
- **Breaking:** `TmFrame::decode_unchecked` returns an `UncheckedTmFrame`, which becomes a `TmFrame` only through `check_fecf` (API-015, TM-005). This is the receiving half of the RS-023 typestate (`Checked`, `Validated`); `Plain` and the engine inputs remain for the engine work packages.
- `aldis-spp`: a Space Packet no longer stores its decode mode; `split` is strict and `split_lenient` is lenient (SPP-008).
- **Breaking:** `aldis-uslp`: `UslpFrameWriter::security_header_mut`, `tfdz_mut`, and `security_trailer_mut` return `Result<&mut [u8]>`, and `InternalError` replaces an empty slice for a field outside the buffer (GEN-020).
- **Breaking:** `aldis-uslp`: `UslpFrame`, `UslpFullFrame`, and `UslpTruncatedFrame` are no longer `Copy`, because they hold their `FrameLayout`.
- `aldis-uslp`: the decoder, the writers, and the configuration check compute the frame overhead with `aldis_core::layout::FrameLayout` instead of three separate sums; the decoder and the truncated decoder no longer cut the frame with clamped slices, and report `InternalError` for a field outside the frame instead of an empty field (GEN-020). Frames, errors, and reasons are unchanged.
- `aldis-ffi`: the USLP functions take the field offsets of `aldis_uslp_layout_t` from the frame layout instead of subtracting pointer addresses (CABI-012). The C ABI is unchanged.
- `aldis-ffi`: the USLP layout conversion reports `InternalError` for a TFDF Header longer than the data field instead of a zero-length TFDZ (CABI-012).
- `docs/09-open-issues.md`: OI-59 (FOP-1 start-up values), OI-60 (NN(R) after a purge), OI-61 (the two Accepts of E21 and E45), resolved by the maintainer; `docs/standards/cop1.md` records the Abort Request action, event precedence, and `StateChanged` rules.
- `cargo xtask trace` ignores vectors that no runner runs yet; OI-62 (E15 precedence) opened.
- `aldis-testkit`: an engine-scenario file holds many scenarios with named, shared setups (`ScenarioFile::resolve`); `expect_timeout_us` is a deadline and `expect_no_timeout` asserts none (`null` is refused); the released-file check treats setups as append-only. `cargo xtask vectors generated` fails when `test-vectors/cop1/` differs from what the model writes.
- `docs/milestones/m3-cop1.md`: record completion and merge of WP-1a (#145) and WP-1b (#147).

### Removed
- `aldis-ffi`: eliminated legacy delimiter and validate entry points (`aldis_tc_next_frame`, `aldis_tc_validate`, `aldis_uslp_next_frame`, `aldis_uslp_validate`) and delimited types in favor of the single-pass stream intake APIs (`aldis_tc_stream_next`, `aldis_uslp_stream_next`).
- The decision log, `docs/process/decisions.md`, and `cargo xtask decisions` (VER-062 keeps the decision rights and no longer asks for a log line). The history stays in git: `git log -- docs/process/decisions.md`. `cargo xtask metrics` no longer counts the log's entries or treats it as bookkeeping.
- `docs/verification/inspections/`: retired all 9 `gen-022-023-*.md` Markdown inspection tables to eliminate maintenance churn and merge conflicts across parallel branches.
- `docs/06-verification.md`: deleted requirement VER-061 (per-PR inspection record maintenance).
- Retired Track C (native C++) from v1 and deferred to roadmap Phase 3; archived native C++ codebase at git tag `c0-archive`. Deleted `cpp/` and `.github/workflows/cpp.yml`.
- Withdrew 34 `CPP-*` requirements in `docs/profiles/cpp.md`, plus `VER-052`, `VER-053`, `CABI-070`, and `CABI-071`. Dropped CMake, Ninja, CodeQL, and GNU bare-metal toolchains from `Containerfile`.
- `aldis-ffi`: **Breaking:** the two-phase writer functions `aldis_{tm,aos,tc,uslp,spp,epp}_writer_begin` and `_writer_finish`, `aldis_tm_oid_writer_begin`, `aldis_tm_oid_writer_finish`, and `aldis_aos_oid_writer_begin`, and the structs `aldis_spp_writer_layout_t` and `aldis_epp_writer_layout_t` (OI-57 convention 1). Frames and packets are built with the one-shot encoders. `aldis_tm_layout_t`, `aldis_aos_layout_t`, `aldis_tc_layout_t`, and `aldis_uslp_layout_t` stay: they are the layouts of decoded frames.
- `aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp`: **Breaking:** `TmFrameWriter::resume`, `TcFrameWriter::resume`, `AosFrameWriter::resume`, and `UslpFrameWriter::resume`, which existed only for the C stateless finish. The typestate `begin` and `finish` stay.
- `aldis-ffi`: **Breaking:** `aldis_limits`, `aldis_limits_t`, and `ALDIS_ERROR_DETAIL_MIN_SIZE` (CABI-030, CABI-021); the `ALDIS_*_MAX_*` constants of the header are the only report of the capacities. CABI-033 (`struct_size` of configuration structs) is withdrawn.
- `xtask`: the post-processing that put negative `#define` values of the header in parentheses; the header has none.
- `aldis-ffi`: **Breaking:** the constants `ALDIS_OID_GENERATOR_STORAGE_SIZE`, `ALDIS_OID_GENERATOR_STORAGE_ALIGN`, `ALDIS_SPP_TRACKER_STORAGE_SIZE`, `_ALIGN`, `ALDIS_SPP_SEGMENTER_STORAGE_SIZE`, `_ALIGN`, `ALDIS_SPP_REASSEMBLER_STORAGE_SIZE`, and `_ALIGN`, and the functions `aldis_oid_generator_storage_size`, `_storage_align`, `aldis_spp_tracker_storage_size`, `_storage_align`, `aldis_spp_segmenter_storage_size`, `_storage_align`, `aldis_spp_reassembler_storage_size`, and `_storage_align`. The constants `ALDIS_OID_GENERATOR_WORDS`, `ALDIS_SPP_TRACKER_WORDS`, `ALDIS_SPP_SEGMENTER_WORDS`, and `ALDIS_SPP_REASSEMBLER_WORDS` take their place as the word counts of the storage types.
- Requirements withdrawn in the specification: API-051, API-081 (the output queues), COP-032 (the internal FARM-1 buffer model), RS-014 and CABI-041 (security hooks inside engines).

### Fixed
- `docs/profiles/c-abi.md`: two names of the USLP non-truncated discard that the header no longer declares (`ALDIS_USLP_REASON_NON_TRUNCATED_FRAME_NOT_ALLOWED`, `ALDIS_USLP_DELIMITED_NON_TRUNCATED_NOT_ALLOWED`) are now the discard reason `ALDIS_USLP_DISCARD_NON_TRUNCATED_FRAME_NOT_ALLOWED`, and the naming example `aldis_spp_header_t` is `aldis_spp_packet_t`.
- Dangling names in the hazard analysis, the assurance evidence rules, and the USLP standard (`aldis_tc_stream_next`, `truncated_frame_len`), and mentions of the removed `resume` and C two-phase writers.
- `cargo xtask sana check-live` and `sana import` follow the pager of a SANA registry page (25 rows a page; the `per_page` query is ignored), sort the rows by value, and drop duplicates. The nightly job `sana-registries` failed on the 32 records of `uslp_protocol_id` (#155). Fetching fails only without data; the comparison with the snapshot is the check.

### Added
- intake: unified frame intake stream for TC and USLP (#151)
- cop1: FARM-1 engine as a pure step function (M3 WP-3) (#162)
- ffi: lend engine state in place from caller storage (#168)
- codecs: frame finishing typestate in the codec writers (RS-023) (#180)
- typed decode results (H2, H3, RS-023 receiving half) (#184)
- Implement FOP-1 as a step function (#193)
- FOP-1 USLP frame target (Type-AD, BD, and protocol control BC frames) (#196)
- Closed-loop COP-1 harness, event fuzzing, and TC/USLP carry-overs (#195)
- FOP-1 decide and commit in place (#202)
- C ABI handles for FOP-1 and FARM-1 (#201)
- m3: FARM-1 refuses a frame of another virtual channel (#207)

### Changed
- Agent tooling: guard against runaway searches and unstructured gh queries (#139) (#140)
- Add fuzz-smoke check to cargo xtask verify (#135) (#142)
- M3 plan: COP-1 detailed milestone plan and --scope m3 trace gate (#143)
- COP-1 spec readings, cell review record, and field codes 150–164 (WP-1a) (#145)
- COP-1 common types & FARM-1 Table 6-1 as const data (WP-1b) (#147)
- COP-1 FOP-1 Table 5-1 as const data (WP-1c) (#149)
- uslp: back the frame views, writer, and C ABI with FrameLayout (#153)
- **Breaking:** ffi: one-shot encoders only; remove the two-phase writers and resume (OI-57 item 1) (#161)
- **Breaking:** ffi: typed caller storage for the stateful helpers (OI-57 convention 4) (#165)
- **Breaking:** ffi: mirror enums for the C ABI's enumerations (PR C, OI-57 convention 2) (#172)
- **Breaking:** ffi: status codes are the ErrorKind code (OI-57 convention 3) (#174)
- **Breaking:** ffi: aldis_range_t in views; drop struct_size and aldis_limits (OI-57 conventions 7 and 5) (#176)
- test-vectors(cop1): blind model and one scenario per table cell (M3 WP-2) (#183)
- Implement FARM-1 flags derived from the protocol state (#189)

### Fixed
- m3: Rank invalid CLCW pattern before Lockout and N(R) (OI-62) (#200)

## [0.3.0-m2] — 2026-10-27

### Added
- Native C++ vector tooling (C0 WP-4, [docs/milestones/c0-foundations.md](docs/milestones/c0-foundations.md) D5;
  CPP-053, CPP-034, TV-012), test-only, in `cpp/vectors/`: a case model of literal types and one
  `constexpr` check function per core operation, shared by every runner; a host loader
  (nlohmann/json) that follows the vector schemas and the exact match of TV-012, and keeps the
  optional field code of error expectations (M2 plan D3) without comparing it until C1; the host
  runner `aldis_vectors`, one CTest test per file of `test-vectors/core/`; and `aldis_vecgen`,
  which writes the cases as `constexpr` arrays for the freestanding exerciser (WP-5) and the
  `static_assert`s of CPP-034, built for the host by an `ExternalProject` in bare-metal presets
  (or taken from `ALDIS_VECGEN`). All 77 live core cases pass against `aldis_core`.
- `docs/process/review.md`: independent review protocol moved from `AGENTS.md` (#119).
- [docs/profiles/rust.md](docs/profiles/rust.md) §5: adopted additional Clippy restriction lints (`undocumented_unsafe_blocks`, `alloc_instead_of_core`, `dbg_macro`, `print_stdout`, `print_stderr`, `string_slice`, `infinite_loop`, `mem_forget`, `as_pointer_underscore`, `lossy_float_literal`) and selected nursery lints (`fallible_impl_from`, `non_send_fields_in_send_ty`, `transmute_undefined_repr`, `debug_assert_with_mut_call`, `needless_pass_by_ref_mut`, `large_stack_frames`, `collection_is_never_read`) across workspace and crate lint tables (RS-019, GEN-010, GEN-020, GEN-030, GEN-070).
- A `README.md` in every crate under `crates/`: purpose and standard, status and milestone, layer
  and dependencies, features, and, for the implemented crates, an example and links to the
  verification evidence. The crate-level rustdoc stays the API reference (RS-015). The examples in
  `aldis-core`, `aldis-time`, `aldis-spp`, `aldis-epp`, and `aldis` run as doctests, and each
  crate's `Cargo.toml` names its README. The root `README.md` links them.
- Decision rights, [docs/process/decision-rights.md](docs/process/decision-rights.md): who decides
  what among the maintainer, the coordinating session, and the agent of a work package, for twelve
  categories (spec readings, scope, plans, API and ABI shape, vectors, review findings, process and
  tooling, dependencies and licences, releases and anything external, merges to `main`,
  housekeeping, approvals), each with an example from 2026-10-20 and 2026-10-21 and what to do
  instead of asking; how to ask the maintainer (one question, a recommended option first, the
  alternatives, what continues meanwhile; batched unless work is blocked); what never needs
  asking; and what continues while a question waits.
- The decision log, `docs/process/decisions.md` (removed 2026-10-30): one line per
  maintainer decision, `- YYYY-MM-DD · <category> · <#N or chat> · <decision>`, appended by the
  coordinator, backfilled with the decisions of 2026-10-20 and 2026-10-21.
- New VER-062 ([docs/06-verification.md](docs/06-verification.md) §10): decisions are taken by the
  role the decision rights give them to, and every maintainer decision is logged.
  `cargo xtask decisions` checks the log's format (known category slugs, each defined in the
  decision rights; real dates in order; a `#N` or `chat` source), in `cargo xtask ci`, the verify
  check `spec`, and the nightly `ci.yml` job `spec`.
- `cargo xtask metrics`: where each pull request's time went. A stage timeline (first commit,
  opened, first passing record, first review, last maintainer decision, approval, final passing
  record, merged) with the time between stages, raw and in the maintainer's active time (within
  30 min of a maintainer comment, review, or merge); the raw wait for the first review is labelled
  not actionable. The approval is the first `APPROVED` review, else the maintainer's last word if
  only merges followed it, else the merge (docs/06-verification.md §9, VER-063).
- `cargo xtask metrics`: the process-created waits, per pull request and per period: order
  inversions (a pull request approved while one it depends on by branch ancestry was unreviewed),
  verification after approval, failed runs with the time spent after their first gating failure
  (from the run's artefact), cancelled runs, rework after a maintainer decision, and repeated
  conflicts. A "Recommendations input" section compares them with fixed thresholds and with the
  previous period of the same length (`--no-previous` skips it). VER-063 states these (maintainer
  decision 2026-10-24).
- `cargo xtask metrics`: per merged pull request, the cycle time, size and areas, verification
  runs and minutes per check group, the checks the final record ran, skipped, and reused, merges
  from `main` with the paths that conflicted (`git merge-tree`), reviews and fixes after them, and
  follow-up issues; aggregated per period, with the maintainer's decision log counted per
  category; as Markdown or JSON (docs/06-verification.md §9, VER-063). Needs `gh` and the network,
  so it is not part of `cargo xtask ci`.
- Nightly job `metrics` (`.github/workflows/nightly.yml`): keeps the report of the last 30 days in
  the pinned issue "Process metrics" and the JSON as an artefact, without failing the night or
  opening the nightly-failure issue (VER-063).
- The milestone exit reviews the process metrics and opens the top friction item as a follow-up
  (`docs/milestones/checklists.md`).
- `cargo xtask metrics`: the process overhead. Each pull request's changed paths, and those of its
  commits other than merges, are product (the code, the vectors, the specification) or process
  (tooling, workflows, plans, process and verification records, the changelog, the root files);
  the report gives the shares of pull requests, commits, and verification minutes that are
  process-only, product-only, or mixed, and the bookkeeping-only commits (only the decision log, a
  plan's status rows, or verification records). The "Recommendations input" lists the process-only
  pull requests when they take 33 % or more of the verification minutes, and each pull request
  with two or more bookkeeping-only commits (docs/06-verification.md §9, VER-063).
- `.gitattributes`: `docs/process/decisions.md` merges with `merge=union`, keeping the lines of
  both pull requests; `cargo xtask decisions` still reports a line out of date order or repeated.
- `README.md`: a notice that Aldis is pre-release and under development. It has not been
  qualified for flight use, its API may change in any 0.x release, its crates are not published,
  and outside contributions are not accepted yet. The notice also says why the repository is
  public (VER-055, OI-22).
- `cargo xtask consistency`, part of `cargo xtask ci` and the nightly `ci.yml` job `spec`: fails
  when the registrations disagree. Every Layer 0–2 crate that declares a function is in the
  coverage gate, the `features` check, the nightly `mutants` job, and the exerciser (with a
  vector group), and no skeleton is; every crate has a README linked from `README.md` and a
  facade feature in `full`; the fuzz targets of `fuzz/Cargo.toml` and the nightly fuzz matrix
  are the same; every vector file is listed in `test-vectors/README.md`, named by a test in
  `crates/*/tests/`, and embedded by the exerciser when its crate is exercised; `aldis-ffi`, once
  it has code, is in the coverage report and the exerciser, and every file of
  `test-vectors/{time,spp,epp}/`, and of any directory it names a file of, is run through the C ABI
  by `crates/aldis-ffi/tests/vectors.rs`; the exerciser has its `ffi` group.
- The independent models of the hand-derived vectors are committed under `test-vectors/models/`
  (new TV-023, [docs/07-test-vectors.md](docs/07-test-vectors.md) §4): `aos_fhec.py` for
  `test-vectors/aos/fhec_*.json` and `core_ocf.py` for `test-vectors/core/ocf.json`, each with
  `--check` (recompute every expected value; exit 1 on a difference) and `--write`.
  `cargo xtask vectors validate` runs every model that a `provenance_note` names, which needs
  `python3` (installed in the verification image). The provenance notes of those six files,
  not released yet, name their model. `test-vectors/models/README.md` records that the M1
  models were not kept. TV-023 is in the M2 exit criteria.
- `docs/milestones/checklists.md`: the definition of done for the recurring kinds of work (a
  codec module, a crate, a vector file, a fuzz target, C ABI functions, a spec-readings pull
  request, a milestone exit), each item citing the requirement or check that enforces it and
  marking those `cargo xtask consistency` checks. Linked from `AGENTS.md`, `docs/README.md`, and
  roadmap §6 rule 1.
- Specification: RS-021 (Rust profile). A state machine makes invalid states and transitions
  unrepresentable: typestate where the caller's call sequence decides the next state, and a closed
  enum with a transition function generated from or checked against the state table where an
  external event decides it. M2 WP-7 plans a mechanical RS-020 check and a property test for every
  restricted type.
- Specification: GEN-015 and `docs/hardware-assumptions.md`. Aldis assumes the memory holding its
  state between calls is protected against single-event upsets (EDAC with scrubbing). The
  document states what an upset in unprotected memory does to Aldis, and what the integrator does.
- `aldis-ffi`: the C ABI of the AOS Transfer Frame codec `aldis-aos` (M2 plan WP-4 module 8,
  D4, D14; CABI-010…012, CABI-033), following the frame codec conventions of
  `docs/profiles/c-abi.md` §11: `aldis_aos_fhec_encode` and `aldis_aos_fhec_decode`;
  `aldis_aos_config_check` and `aldis_aos_mc_config_check`; `aldis_aos_peek_length` and
  `aldis_aos_header_decode`; the frame decoder `aldis_aos_decode`; the data unit decoders
  `aldis_aos_mpdu_decode` and `aldis_aos_bpdu_decode`; the one-shot `aldis_aos_encode`, which
  takes the frame's variable fields in an `aldis_aos_content_t`; the two-phase writer
  `aldis_aos_writer_begin` and `aldis_aos_writer_finish`; and the OID frame writer
  `aldis_aos_oid_writer_begin` on VCID 63, which takes an `aldis_oid_generator_t` and which
  `aldis_aos_writer_finish` finishes. A physical channel is an `aldis_aos_config_t` with its
  edition; a master channel an `aldis_aos_mc_config_t`, one SCID holding its physical
  channel, with an array of `aldis_aos_vc_config_t` and its count. The packet offset of an
  `M_PDU` crosses as a `uint16_t` checked as a `PacketOffset`, and the count of valid bits of
  a `B_PDU` crosses as a `uint16_t` checked as a `ValidBitCount`. The header
  `include/aldis/aldis.h` is regenerated.
- `aldis-aos`: `AosFrameWriter::resume`, which takes up a frame `begin` or `begin_oid` started
  with the same arguments without zero-filling it, for writers that keep no state between
  calls, such as the C ABI's `aldis_aos_writer_finish` (API-023).
- Every live case of `test-vectors/aos/` runs through the C ABI (`aos` joins `FFI_AREAS` of
  `cargo xtask consistency`); `crates/aldis-ffi/tests/aos.rs` checks the channel arrays, the
  layouts, the FHEC and data unit decoders, and the stateless writers; the exerciser's
  `aldis-ffi` group, the C smoke test, and the fuzz target `ffi_decode` call the new
  functions.
- `aldis-aos`: the AOS Transfer Frame codec of 732.0-B-5 EC1, in Issue 5 and in the wire format of
  732.0-B-4 and earlier (M2 WP-4, modules 2–7; AOS-001…AOS-003, AOS-010…AOS-019, GEN-062): the
  edition `AosEdition` (default `Issue5`); the managed parameters `AosPhysicalChannelConfig` and
  `AosVirtualChannelConfig` behind the `VcLookup` trait and its fixed-capacity `VcTable`, rejecting
  VCID 63 for a data VC, an SCID above 255 in Issue 4, and the B_PDU limits of AOS-018;
  `AosPrimaryHeader::decode`, which corrects the header with the FHEC before any field is read; the
  zero-copy `AosFrame` decoder, with the FECF checked over the corrected header, the Insert Zone,
  the SDLS slices, the OCF, and the data field as a `DataUnit`; `Mpdu` and `Bpdu` with their
  pointers classified (`PacketPointer`, `BitstreamPointer` with its count of valid bits
  `ValidBitCount`, 1 to `0x3FFE`); the one-shot `encode`, the two-phase
  `AosFrameWriter`, and OID frames on VCID 63 (`begin_oid`). Lenient-mode flags `AosAnomaly` with
  bit indices.
- Field codes 100–106 for the AOS fields and parameters (`docs/03-api-model.md` §9.1). AOS shares
  `vcf_count` (93) and `insert_zone_len` (97) with USLP, and the Spacecraft ID, VCID, frame data
  field, security lengths, Transfer Frame Version Number, frame length, and First Header Pointer
  codes with the other codecs.
- Vectors `test-vectors/aos/` (11 frame files) from the independent model
  `test-vectors/models/aos_frame.py`, which uses the FHEC model `aos_fhec.py`, with a worked
  derivation in `docs/verification/derivations/aos-frame.md`.
- `aldis-testkit`: AOS strategies (feature `strategies-aos`); `aldis-aos` property tests.
- Fuzz targets `aos_decode` and `aos_roundtrip`; the `aldis-aos` exerciser group covers the frame
  codec and every `aos/` vector.
- OI-43 item 1 (resolved by the maintainer): an OID frame on VCID 63 carries neither an OCF nor
  SDLS fields.
- `aldis-aos`: the AOS Frame Header Error Control (732.0-B-5 EC1 §4.1.2.6.5, the same code as
  732.0-B-4; AOS-013, AOS-014; M2 plan D10). `fhec::encode` and `fhec::write` compute the four
  check symbols by division by g(x) over GF(2⁴); `fhec::decode` returns a `CorrectedHeader` with up
  to two corrected symbols, reported by `corrected_symbols()`, or `HeaderUncorrectable`. The
  decoder finds the error locator in closed form, searches only the ten transmitted positions, and
  takes the error values from Forney's formula.
- `aldis-testkit`: `oracle::fhec`, an independent table model of the FHEC built from every pattern
  of at most two symbol errors; the decoder agrees with it on all 65 536 syndromes (unit test) and
  in the new fuzz target `aos_fhec_model`, which joins the nightly fuzz matrix.
- Vectors `test-vectors/aos/{fhec_encode,fhec_correct_1_symbol,fhec_correct_2_symbols,fhec_miscorrection,fhec_uncorrectable}.json`,
  with one encoding and one correction worked by hand in
  `docs/verification/derivations/aos-fhec.md`; the exerciser runs them on the three bare-metal
  targets. `aldis-aos` joins the coverage gate and the nightly mutation testing, and has a
  GEN-022/023 inspection record.
- `cargo xtask assurance [--strict]` (M2 WP-8; VER-065, VER-066, specified in
  [docs/verification/assurance-evidence.md](docs/verification/assurance-evidence.md)): every
  reference of the hazard analysis and the assurance case resolves (a file, test, proof, vector
  case, record, gating or nightly-only check, step of `cargo xtask ci`, nightly job, fuzz target,
  open issue, hazard, claim, assumption, defeater, or proposal), evidence that runs only at night
  says so by its kind, both documents follow their formats, and every hazard supports a claim;
  `--strict` also fails on anything left as TODO. It runs in `cargo xtask ci` and the verify
  check `spec`, and writes `target/spec/assurance.json` and `target/spec/assurance.md`.
- Verification: the hazard and failure-mode analysis (`docs/verification/hazards.md`, one file per
  frame codec in `docs/verification/hazards/`), the assurance case
  (`docs/verification/assurance-case.md`), and the specification of the check
  `cargo xtask assurance` (`docs/verification/assurance-evidence.md`), with the new requirements
  VER-065 and VER-066 (M2 WP-8). The analysis finds that `validate_frame` detects a TC or USLP
  Frame Length error only when the frame's boundary does not come from the delimiter, and that a
  bit error in the USLP End of Frame Primary Header Flag can make a frame read as a truncated
  frame, which has no FECF; both are raised as proposals.
- `crosscheck/`: NASA cFE's `MSG` module (v7.0.1, Apache-2.0) as a second independent oracle for
  Space Packets (VER-040, M2 D15, OI-32;
  [docs/verification/cross-checks.md](docs/verification/cross-checks.md)). The five cFE sources
  and eight headers the check needs are vendored unmodified under `crosscheck/vendor/cfe/` with
  cFE's licence and their SHA-256, compiled by the `cc` crate with small stubs for the OSAL and
  cFE headers they include (`crosscheck/cfe-stubs/`), and called through `crosscheck/src/cfe_msg.rs`,
  the only `unsafe` code of the cross-checks. cFE reads and builds the primary header of every
  well-formed packet in `test-vectors/spp/` against `aldis-spp`, checks the expected values it can
  produce (decoding, encoding, Octet String, Idle Packets, segments, streams, and its CUC 4+2
  telemetry time in `spp/secondary_header_time` and `time/cuc`), and compares 4200 random packets
  from `aldis_testkit::strategies` with a fixed seed. No disagreement and no known discrepancy.
- [docs/verification/cross-checks.md](docs/verification/cross-checks.md) §4.7–§4.10: evaluated seven
  independent frame candidates (`eu.dariolucia.ccsds`, Yamcs, NASA CryptoLib, `spacepackets`,
  `gr-satellites`, Wireshark dissectors, `ccsds` crate) and Orekit across TM, TC, AOS, and USLP (VER-040,
  M2 D15, WP-6). None is wired into `crosscheck/`: Java candidates require a JVM runtime (deferred under
  OI-32) or are AGPL-3.0 server frameworks; NASA CryptoLib covers SDLS (355.0) only; `spacepackets` covers
  no TM/TC/AOS and only partial unsegmented USLP full frames without insert zone, SDLS, truncated frames, or
  delimiting/validation; deframers (`gr-satellites`, `ccsds` crate) lack frame header codecs; upstream
  Wireshark has no frame dissectors. Frame vectors keep `hand-derived` provenance, verified by written
  derivations, blind Python models (TV-023), and the approved second review ([m2-vectors.md](docs/verification/reviews/m2-vectors.md)).
- [docs/09-open-issues.md](docs/09-open-issues.md) OI-32: updated with the M2 frame candidate evaluation
  findings and status `Open (JVM harness)`.
- Specification: USLP-016, the TFDZ Construction Rule must match the VC's frame type
  (`000`–`010` fixed-length, `011`–`111` variable-length), `InvalidField` in both modes
  (732.1 §4.1.4.2.2.1.1, Table 4-3; OI-35 item 3).
- Specification: bit indices for the `TmAnomaly`, `AosAnomaly`, and `UslpAnomaly` flags
  (`docs/standards/tm-frame.md`, `aos-frame.md`, `uslp-frame.md`).
- `specs/`: CCSDS 700.1-G-1, the USLP Green Book, as a `ref` document (OI-05).
- Verification spike (OI-42): Kani 0.68.0 proof harnesses in `aldis-core` (CRC-16 against a
  shift-register reference, the two FECF methods, the CMN-031 counter classification, the bit-field
  and octet-integer helpers), `aldis-spp` (every primary header, `SpacePacket::decode` against
  SPP-001/002/004, header and whole-packet encode→decode), and `aldis-time` (the TIME-011 CUC
  conversions and round trips for 1 and 2 fine octets, and the conversion to nanoseconds for 3; the
  others did not finish within 30 minutes), compiled only under `cfg(kani)`. `cargo xtask kani` runs them and
  prints each harness's result and time.
- The non-gating check `kani` of `cargo xtask verify` runs `cargo xtask kani` (OI-42, maintainer
  decision 2026-10-23). The verification image installs Kani 0.68.0: the driver from crates.io, and
  the release bundle checked against GitHub's SHA-256 of the asset, under `/opt/kani`.
  `docs/verification/formal-proofs.md` records the bounds, the trusted base, the canaries, and the
  limits; `cargo xtask trace` lists the harnesses as `proof` artefacts.
- Verification: non-gating check `rust-sparc-leon3` (OI-08). It builds the facade and the C ABI
  library for `sparc-unknown-none-elf` with `-C target-cpu=leon3` on the tooling nightly
  (`-Z build-std=core`), runs the exerciser's panic-freedom link check and canary (VER-010), and
  runs every vector case under QEMU `leon3_generic`. The exerciser gains LEON3 start-up code, a
  trap table with window overflow and underflow handlers, an APBUART console, and an error-mode
  exit (`exerciser/src/target.rs`, `exerciser/link-leon3.x`).
- Verification image: Clippy on the tooling nightly, and Ubuntu's `binutils-sparc64-linux-gnu`
  (GNU ld 2.42) and `qemu-system-sparc` (QEMU 8.2.2), with their upstream versions asserted.
- Plan for milestone M2, frame codecs ([docs/milestones/m2-frame-codecs.md](docs/milestones/m2-frame-codecs.md)): WP-0 with the C ABI of the M1 codecs, the field-code catalogue, and the shared property-test strategies, after which C1 can start; then `aldis-tm`, `aldis-tc`, `aldis-aos`, and `aldis-uslp`, each with its C ABI functions, vectors through the Rust API and the C ABI, and the M1 gates. Decisions D1–D17 accepted 2026-10-20. Linked from the roadmap and the docs index.
- Open issues OI-33 (frame anomaly flags and API-032), OI-34 (frame requirements that cross crates under GEN-002), and OI-35 (USLP and AOS readings found while planning M2) in [docs/09-open-issues.md](docs/09-open-issues.md); OI-04 records the reading of 732.0-B-4, which confirms AOS-002, AOS-003, and the FHEC for `Issue4Compatible`.
- USLP-014 (USLP Frame Validation Checks, 732.1 §4.3.11.3) and USLP-015 (Frame Delimiting and Fill
  Data Removal, 732.1 §4.3.11.2) in [docs/standards/uslp-frame.md](docs/standards/uslp-frame.md),
  for a physical channel over TC Synchronization and Channel Coding.
- `specs/manifest.tsv`: CCSDS 732.0-B-4 (Silver Book) in the `ref` group, for `Issue4Compatible`.
- `docs/verification/reviews/human-301-discrepancies.md`: a review packet for the maintainer to
  answer, from CCSDS 301.0-B-4 alone, the four readings on which `spacepackets` and Aldis differ
  (cross-checks.md §3, D1–D4), with hand exercises against the named vector cases and a record to
  fill in.
- Open issue OI-39: CUC P-field octet 2, 301.0 §3.1.1 against §3.2.2, raised by the review's Q3;
  deferred with octet 2 still accepted, and tracked in #84.
- Open issue OI-40: rounding nanoseconds to CUC fine time, raised by the review's Q4, and
  resolved for rounding down, which a follow-up pull request implements.
- `xtask`: `cargo xtask rs020`, part of `cargo xtask ci` and the verify check `spec`, flags every bare
  integer in a public field or enum payload of a codec crate unless `xtask/rs020-exemptions.toml`
  exempts it with a reason, and every exemption without a reason or that matches nothing (RS-020,
  M2 plan WP-7).
- `aldis-aos`: `PacketOffset`, the M_PDU packet offset of 0 to `0xFFFD`, with `new`, `get`, and
  `TryFrom<u16>` (RS-020).
- `aldis-tm`: `PacketOffset`, the packet offset of 0 to `0x7FD`, below the two reserved First
  Header Pointer values, with `new`, `get`, and `TryFrom<u16>` (RS-020).
- `aldis-uslp`: `TfdzOffset`, the offset of 0 to `0xFFFE` that the First Header and Last Valid
  Octet Pointers hold below `POINTER_NONE`, with `new`, `get`, and `TryFrom<u16>` (RS-020).
- `aldis-ffi`: the C ABI of the TC Space Data Link Protocol codec `aldis-tc` and the CLCW (M2
  plan WP-3 module 8, D4, D14; CABI-010…012, CABI-033): `aldis_tc_config_check` and
  `aldis_tc_mc_config_check`; `aldis_tc_peek_length` and `aldis_tc_header_decode`; the decoder
  `aldis_tc_decode`; the Frame Validation Checks `aldis_tc_validate`, which returns the reason
  of a rejection as an `ALDIS_TC_REASON_*` code; the one-shot `aldis_tc_encode`, which takes
  the User Data or Control Command in an `aldis_tc_content_t`; the two-phase writer
  `aldis_tc_writer_begin` and `aldis_tc_writer_finish`; the Frame Delimiting and Fill Removal
  Procedure as the stateless `aldis_tc_next_frame`, which takes the data unit and an offset;
  the Control Commands `aldis_tc_control_decode` and `aldis_tc_control_encode`; and the CLCW as
  a 32-bit word, `aldis_clcw_decode` and `aldis_clcw_encode`, with the POD `aldis_clcw_t`. A
  master channel is an `aldis_tc_mc_config_t`, holding its physical channel and Spacecraft ID,
  with an array of `aldis_tc_vc_config_t` and an array of `aldis_tc_map_config_t`, each with
  its count and its own stride; every call that takes it uses the security layouts of the
  frame's VC or MAP. Views and writer layouts give every field as an offset and a length
  (`aldis_tc_layout_t`). The header `include/aldis/aldis.h` is regenerated.
- `aldis-ffi`: the build-time capacity `[tc] max_maps` of `capacities.toml`, published as
  `ALDIS_TC_MAX_MAPS` and reported by `aldis_limits` as `tc_max_maps` (CABI-030).
- `aldis-tc`: `TcFrame::peek_header`, the Primary Header of a frame from its first five octets
  without a channel lookup, for example to find the Spacecraft ID of a frame on a physical
  channel that carries several master channels (API-014); and `TcFrameWriter::resume`, which
  takes up a frame begun with the same arguments without writing to it, for writers that keep
  no state between calls, such as the C ABI's `aldis_tc_writer_finish` (API-023).
- Every live case of `test-vectors/tc/` runs through the C ABI (`tc` joins `FFI_AREAS` of
  `cargo xtask consistency`); `crates/aldis-ffi/tests/tc.rs` checks the channel and MAP
  arrays, the security layouts of a VC or MAP, the stateless writers and delimiter, the
  validation reasons, the Control Commands, and the CLCW; the exerciser's `aldis-ffi` group,
  the C smoke test, and the fuzz target `ffi_decode` call the new functions.
- `aldis-tc`: the TC Transfer Frame codec of 232.0-B-4 Cor. 1 (M2 WP-3, modules 1–7; TC-001…TC-008,
  TC-020…TC-023, TC-030, TC-031): the managed parameters the codec reads (`PhysicalChannelConfig`,
  `VcConfig`, `MapConfig`) behind the `VcLookup` trait and its fixed-capacity `VcTable`; the
  zero-copy `TcFrame` decoder and `peek_length`; the one-shot `encode` and the two-phase
  `TcFrameWriter`, with SDLS fields per VC or per MAP; the Control Commands Unlock and Set V(R);
  the CLCW (`Clcw`); Frame Delimiting and Fill Removal as the stateless `next_frame` and the
  `FrameDelimiter` iterator; and the Frame Validation Checks (`validate_frame`) with stable reason
  codes (`docs/standards/tc-frame.md` §4.1). Lenient-mode flags `TcAnomaly` with bit indices.
- Field codes 67–80 for the TC frame and CLCW fields (`docs/03-api-model.md` §9.1); `user_len`
  (54) is shared with the TC two-phase writer.
- Vectors `test-vectors/tc/` (11 files) from the independent model `test-vectors/models/tc_frame.py`,
  with a worked derivation in `docs/verification/derivations/tc-frame.md`.
- `aldis-testkit`: TC strategies (feature `strategies-tc`); `aldis-tc` property tests.
- Fuzz targets `tc_decode`, `tc_roundtrip`, `tc_delimit`, and `tc_clcw`; the `aldis-tc` exerciser
  group; `aldis-tc` in the coverage gate, the `features` check, and the nightly mutants job.
- OI-41 (resolved): empty User Data in a TC frame is rejected, and the delimiter takes as a valid header at least five octets with TFVN `00` and a Frame Length of at least six.
- `aldis-ffi`: the C ABI of the TM Transfer Frame codec `aldis-tm` (M2 plan WP-2 module 7, D4,
  D14; CABI-010…012, CABI-033): `aldis_tm_config_check` and `aldis_tm_mc_config_check`;
  `aldis_tm_peek_length` and `aldis_tm_header_decode`; the decoders `aldis_tm_decode`,
  `aldis_tm_decode_with`, `aldis_tm_decode_unchecked`, and `aldis_tm_decode_unchecked_with`;
  `aldis_tm_validate`; the one-shot `aldis_tm_encode`, which takes the frame's variable
  fields in an `aldis_tm_content_t`; the
  two-phase writer `aldis_tm_writer_begin` and `aldis_tm_writer_finish`; and the OID frame
  writer `aldis_tm_oid_writer_begin` and `aldis_tm_oid_writer_finish`, which take an
  `aldis_oid_generator_t`. A physical channel is an `aldis_tm_config_t`; a master channel an
  `aldis_tm_mc_config_t`, holding its physical channel, with an array of
  `aldis_tm_vc_config_t` and its count, whose stride is the structs' `struct_size`. The `_with`
  decoders take each virtual channel's security layout from the master channel, and so does
  `aldis_tm_validate`; the plain decoders take the physical channel alone. Views and
  writer layouts give every field as an offset and a length (`aldis_tm_layout_t`). The header
  `include/aldis/aldis.h` is regenerated.
- `aldis-tm`: `TmFrameWriter::resume`, which takes up a frame begun with the same arguments
  without zero-filling it, for writers that keep no state between calls, such as the C ABI's
  `aldis_tm_writer_finish` (API-023).
- Every live case of `test-vectors/tm/` runs through the C ABI (`tm` joins `FFI_AREAS` of
  `cargo xtask consistency`); `crates/aldis-ffi/tests/tm.rs` checks the channel arrays, the
  security layout variants, and the stateless writers; the exerciser's `aldis-ffi` group, the
  C smoke test, and the fuzz target `ffi_decode` call the new functions.
- `aldis-tm`: the TM Transfer Frame codec (132.0-B-3; TM-001…TM-011; M2 WP-2). Managed
  parameters in `TmPhysicalChannelConfig`, `TmMasterChannelConfig`, and `TmVirtualChannelConfig`,
  whose constructors reject what 132.0 §5 forbids (an MC_FSH with a VC_FSH, an MC_OCF with a
  VC_OCF, a frame without a data octet, a VCID twice). `TmFrame::decode`, `decode_with` (the
  Security Header and Trailer of each VC from a `VcLookup`), `decode_unchecked`, and
  `peek_length`: the FECF checked before any field, the Secondary Header version and length, the
  OCF as an `Ocf`, the First Header Pointer classified as `PacketPointer` (an offset outside the
  data field is `Malformed` in both modes), and the Packet Order Flag and Segment Length
  Identifier checked in strict mode and reported as `TmAnomaly` flags in lenient mode.
  `TmFrame::validate` checks a frame against its master channel. The one-shot `encode`, the
  two-phase `TmFrameWriter`, and `TmFrameWriter::begin_oid` for Only Idle Data frames from an
  `OidGenerator` compute the Secondary Header Identification Field, the OCF Flag, and the FECF.
- Field codes 67–76 for the TM frame fields (`frame_version`, `frame_len`, `ocf_flag`,
  `fsh_flag`, `sync_flag`, `packet_order_flag`, `segment_length_id`, `first_header_pointer`,
  `fsh_version`, `fsh_len`), in `docs/03-api-model.md` §9.1 and the generated files.
- Vectors `test-vectors/tm/{minimal,fsh_ocf_fecf,fhp_values,oid_sequence,security_layout,invalid}.json`,
  computed by the independent model `test-vectors/models/tm_frame.py`, with one decode and one
  encode worked by hand in `docs/verification/derivations/tm-frame.md`; the exerciser runs them
  on the bare-metal targets.
- `aldis-testkit`: `strategies::tm` (feature `strategies-tm`), and the property tests
  `crates/aldis-tm/tests/properties.rs`. Fuzz targets `tm_decode` and `tm_roundtrip` join the
  nightly matrix; `aldis-tm` joins the coverage gate, the features check, and the nightly
  mutation testing, and has a GEN-022/023 inspection record.
- `aldis-ffi`: the C ABI of the USLP Transfer Frame codec `aldis-uslp` (M2 plan WP-5 module 9,
  D4, D12, D14; CABI-010…012, CABI-033): `aldis_uslp_config_check` and
  `aldis_uslp_vc_table_check`; `aldis_uslp_peek_length`; the decoder `aldis_uslp_decode`, whose
  `aldis_uslp_frame_t` carries a full or a truncated frame with a `truncated` tag;
  `aldis_uslp_validate`, which writes a `uint32_t` reason code `ALDIS_USLP_REASON_*`; the one-shot
  `aldis_uslp_encode`, which takes the TFDF Header, the Insert Zone, the TFDZ, and the OCF in an
  `aldis_uslp_content_t`; the two-phase writer `aldis_uslp_writer_begin` and
  `aldis_uslp_writer_finish`; `aldis_uslp_encode_truncated`; `aldis_uslp_encode_oid`, which takes
  an `aldis_oid_generator_t` of either form; and the stateless delimiter
  `aldis_uslp_next_frame`, which takes the whole data unit and an offset. A channel table is an
  `aldis_uslp_config_t` with arrays of `aldis_uslp_vc_config_t` and `aldis_uslp_map_config_t`,
  each naming its SCID, built on every call into a table of the new `capacities.toml` section
  `[uslp]` (`ALDIS_USLP_MAX_VCS`, `ALDIS_USLP_MAX_MAPS`), which `aldis_limits_t` reports. The UPID
  crosses as a 5-bit `uint8_t`, assigned or not, with an `ALDIS_USLP_UPID_*` constant for each
  value of the SANA snapshot. The header `include/aldis/aldis.h` is regenerated.
- `aldis-uslp`: `UslpFrameWriter::resume`, which takes up a frame begun with the same arguments
  without zero-filling its Security Header and Trailer, for writers that keep no state between
  calls, such as `aldis_uslp_writer_finish` (API-023); and `encode_oid_galois`, an OID frame from
  the Galois form of the generator (CMN-022).
- Every live case of `test-vectors/uslp/` runs through the C ABI (`uslp` joins `FFI_AREAS` of
  `cargo xtask consistency`); `crates/aldis-ffi/tests/uslp.rs` checks the channel arrays and
  capacities, the tagged view, the stateless writers, the validation reasons, the delimiter, and
  the OID generator handle; the exerciser's `aldis-ffi` group, the fuzz target `ffi_decode`, and
  the C smoke test cover the new functions.
- `aldis-uslp`: the USLP Transfer Frame codec of 732.1-B-3 (M2 WP-5, modules 1–8; USLP-001…USLP-010,
  USLP-012…USLP-016): `Upid`, typed by a table generated from the SANA registry `uslp_protocol_id`;
  the managed parameters the codec reads (`PhysicalChannelConfig`, `VcConfig` with `VcParams`,
  `MapConfig`) behind the `VcLookup` trait and its fixed-capacity `VcTable`; the zero-copy
  `UslpFrame` decoder, full or truncated (`UslpFullFrame`, `UslpTruncatedFrame`), and
  `peek_length`; the TFDF Header (`TfdfHeader`, `TfdzRule` with its First Header or Last Valid
  Octet Pointer); the one-shot `encode`, the two-phase `UslpFrameWriter`, `encode_truncated`, and
  `encode_oid`; Frame Delimiting and Fill Data Removal (`next_frame`, `FrameDelimiter`); and the
  USLP Frame Validation Checks (`validate_frame`) with stable reason codes
  (`docs/standards/uslp-frame.md` §3.1). Lenient-mode flags `UslpAnomaly` with bit indices.
- `cargo xtask sana`: SANA registry snapshots in `specs/sana/` with URL, retrieval date, and
  SHA-256 (`import`), the offline check of the snapshot and its generated table (part of
  `cargo xtask ci`), and the nightly comparison with the live registry (`check-live`, job
  `sana-registries`; M2 plan D13).
- Field codes 89–99 for the USLP fields (`docs/03-api-model.md` §9.1); `spacecraft_id`, `vc_id`,
  `map_id`, `frame_data_field`, `user_len`, `frame_version`, `frame_len`, `ocf_flag`, and
  `max_frame_len` are shared with USLP.
- Vectors `test-vectors/uslp/` (13 files, 408 cases) from the independent model
  `test-vectors/models/uslp_frame.py`, with worked derivations in
  `docs/verification/derivations/uslp-frame.md`.
- `aldis-testkit`: USLP strategies (feature `strategies-uslp`); `aldis-uslp` property tests and a
  USLP-013 test that decodes COP-1 control commands with `aldis-tc` (a dev-dependency).
- Fuzz targets `uslp_decode`, `uslp_truncated`, and `uslp_roundtrip`; the `aldis-uslp` exerciser
  group; `aldis-uslp` in the coverage gate, the `features` check, and the nightly mutants job.
- OI-44 (open, recommendations implemented): the OID header checked on reception with the new flag
  `OidHeaderMismatch`, at least one TFDZ octet, and the delimiter's valid header.
- [docs/verification/reviews/m2-vectors.md](docs/verification/reviews/m2-vectors.md): the second review of
  the hand-derived TM, TC, AOS, and USLP vectors (M2 plan WP-6), blind, with the maintainer's decisions
  of 2026-10-25, the resolution of each finding, and the list of follow-up cases.
- TC-009: the TC writer builds a Type-C frame from a Control Command only and a Type-D frame from a
  Frame Data Unit only (232.0-B-4 §4.1.2.3.2.2, Table 4-1); the three vector cases that pin it cite it.
- OI-46: the USLP VCF Count Length is read per QoS (Table 5-3), resolved.
- Field catalogue: every field identifier that `aldis-core`, `aldis-time`, `aldis-spp`, and
  `aldis-epp` name has a stable numeric code, listed in `docs/03-api-model.md` §9.1 (64 codes;
  API-082, OI-27, M2 plan D2).
- `aldis-core`: the module `field`, generated from the catalogue, with one `FieldId` constant per
  code and `field::ALL`. `aldis-time`, `aldis-spp`, and `aldis-epp` re-export the constants they
  use as `aldis_time::field`, `aldis_spp::field`, and `aldis_epp::field`.
- `aldis-core`: `FieldId::code()`, the field code that the C ABI will carry (CABI-021).
- `cargo xtask fields [--write]`: checks the catalogue and that `aldis_core::field` and
  `test-vectors/core/fields.json` match it, or regenerates both; part of `cargo xtask ci` and of
  the `spec` check of `cargo xtask verify`.
- `cargo xtask trace --scope m2`, without `--strict`, in `cargo xtask ci`, the `spec` check, and
  the nightly `ci.yml` (M2 plan D1).
- Test vectors: the error expectation of a codec case may give `field`, the code of the field the
  error names; the testkit runner and the exerciser compare it when present (TV-012, M2 plan D3).
  `test-vectors/core/fields.json` (schema `test-vectors/schema/fields.schema.json`) publishes the
  catalogue for the C++ and binding runners; `test-vectors/spp/field_codes.json` adds four cases
  with field codes.
- `aldis-testkit`: the shared property-test strategies of VER-054, `aldis_testkit::strategies`
  (M2 plan D6), behind the feature `strategies` for `aldis-core` and `strategies-time`,
  `strategies-spp`, and `strategies-epp` for one codec crate each. They generate valid typed
  values built through the crates' own checked constructors and encoders (`aldis-core`
  identifiers, the decode mode, counters, bit fields, and frame layouts; time code formats,
  values, and ASCII codes; Space Packet and Encapsulation Packet fields, configurations, and
  whole packets), and invalid octet strings next to each decoder boundary (one octet too short,
  a length field off by one, a version or reserved value changed). `proptest` is re-exported,
  and `strategies::config` sets the case count and the regression file of a property test.
- Property tests: a rejected Encapsulation Packet encode and a rejected segment leave the
  buffer, the sequence counter, and the segmenter unchanged (API-022, SPP-013).
- `aldis-ffi`: the C ABI of the M1 codecs (M2 plan WP-0c, D4; CABI-001…004, CABI-010…012,
  CABI-020…022, CABI-033): `aldis_spp_*`, `aldis_time_*`, and `aldis_epp_*` decoders, encoders,
  two-phase writers as two stateless calls (`_writer_begin`, `_writer_finish`), idle builders,
  `peek_length`, conversions, and configuration checks. Every pointer and length is checked;
  views are POD structs with offset/length pairs; configuration structs start with
  `struct_size`; `aldis_error_detail_t` starts with a caller-set `struct_size`, within which the
  library writes the error kind, the field code, and the detail.
  The `ALDIS_FIELD_*` constants are generated from the field catalogue by `cargo xtask fields`.
- The generated header `include/aldis/aldis.h`, made by the pinned `cbindgen` 0.29.4 (MPL-2.0)
  with `cargo xtask header --write`; `cargo xtask header` and the new verify check `header`
  fail on a stale header, and compile and run a C program with the static and shared libraries
  as C99 and C++11 with the pinned GCC and Clang (CABI-001…003). The verification image
  installs `cbindgen`.
- `capacities.toml` `[spp] tracker_max_apids`, published as `ALDIS_SPP_TRACKER_MAX_APIDS` and
  reported by `aldis_limits` (CABI-030).
- `aldis-ffi`: `aldis_spp_tracker_*`, `aldis_spp_segmenter_*`, and `aldis_spp_reassembler_*`
  in caller-owned storage, with `_STORAGE_SIZE` and `_STORAGE_ALIGN` constants and queries
  (M2 plan D5). `_init` checks the storage's size and alignment (CABI-031); every call checks
  the storage's tag, a magic number, a layout generation, and the storage's address, and fails
  with `ALDIS_ERR_INVALID_HANDLE` on uninitialized, released, foreign, or moved storage
  (CABI-032). The SDU and the reassembly buffer are passed on every call (API-060).
- `aldis-spp`: `Segmenter::suspend` and `Reassembler::suspend` return a `SegmenterState` or
  `ReassemblerState` without the borrowed buffer, which `resume` continues with the same
  buffer, for callers that cannot keep the borrow between calls (API-060).
- An `aldis-ffi` integration test runs every live case of `test-vectors/{time,spp,epp}/`
  through the C ABI with the testkit runner.
- The exerciser calls every function `aldis-ffi` exports, so the panic-freedom link check
  covers the C ABI on all three bare-metal targets (VER-010, M2 plan D16); its panic handler
  is now `aldis-ffi`'s, with the new feature `panic-unreachable`. The fuzz target `ffi_decode`
  checks that the C ABI decoders agree with the Rust decoders. `cargo xtask coverage` measures
  and reports `aldis-ffi` without gating it.
- `aldis-core`: `FieldId::from_code`, the catalogued field of a code that crossed the C ABI,
  and `PartialOrd`/`Ord` on `FieldId` by code alone, written by hand so the display name never
  enters the comparison (API-082; issue #65). `FieldId` gets no `Display`: `name()` is the
  display name, and `Error`'s `Display` prints it.
- `aldis-ffi`: the C ABI of the parts of `aldis-core` the frame codecs share (M2 plan WP-1,
  D14): `aldis_crc16`, the CRC-16 of a buffer (CMN-001); `aldis_ocf_classify`, the report four
  Operational Control Field octets hold (`ALDIS_OCF_CLCW`, `_PROJECT_SPECIFIC`, `_SDLS_FSR`),
  and `aldis_ocf_check`, which checks them against a report and names the Type Flag or bit 1
  when they differ (CMN-010, CMN-011); and the OID generator `aldis_oid_generator_t` in
  caller-owned storage, in the Fibonacci or the Galois form (`ALDIS_OID_FIBONACCI`,
  `ALDIS_OID_GALOIS`), with `_storage_size`, `_storage_align`, `_init`, `_fill`, and `_deinit`
  and the storage checks and handle tags of the SPP helpers (CMN-020…023; D5, CABI-031,
  CABI-032). The header `include/aldis/aldis.h` is regenerated.
- The C ABI vector runner runs `test-vectors/core/{crc16,ocf,oid}.json`; the exerciser's
  `aldis-ffi` group, the C smoke test, and the fuzz target `ffi_decode` call the new functions.
- `docs/verification/inspections/gen-022-023-aldis-ffi.md`: the GEN-022/023 inspection of the
  new `aldis-ffi` modules and the shared checks they call, pending exit review (VER-061).
- `aldis-core`: the module `ocf`, the Operational Control Field of TM, AOS, and USLP frames
  (CMN-010, CMN-011; M2 plan WP-1). `Ocf::decode` classifies four octets by bits 0–1 into
  `Ocf::Clcw(Type1Report)`, `Ocf::ProjectSpecific(ProjectSpecificReport)`, or
  `Ocf::SdlsFsr(FsrReport)` (132.0 §4.1.5.4–5, 732.0 §4.1.5.4–5, 732.1 §4.1.5.2.1–2); all four
  values of the two bits are assigned, so it cannot fail. Each report type checks its flag bits in
  `new` and `TryFrom<[u8; 4]>` and keeps the octets opaque; `Ocf::to_octets` gives the octets
  to write. `ocf::OCF_LEN` is `layout::OCF_LEN`.
- Field catalogue: codes 65 `ocf_type_flag` and 66 `ocf_report_use`, named by the
  `InvalidField` errors of the report constructors (`docs/03-api-model.md` §9.1, API-082).
- Test vectors: `test-vectors/core/ocf.json`, 20 hand-derived cases of classification and of the
  report constructors, with field codes; run by `aldis-core`'s vector suite and by the
  exerciser on the three bare-metal targets.
- Fuzz target `ocf_classify`, in the nightly matrix (VER-022).
- `cargo xtask stack` and the gating verify check `stack` (VER-011, GEN-022): the worst-case stack of
  every entry point of the library, Rust and C ABI, on thumbv7em, armv7r, and riscv32imac, from the
  frames and relocations of the `aldis-ffi` static library and of the exerciser, checked complete
  against their LLVM IR (no indirect call or recursion that an exemption does not bound), within the
  budgets of `xtask/stack-budgets.toml`; the Rust entry points are the public functions by their
  source visibility, monomorphisations included; the report `target/spec/stack-<target>.md` is the
  VER-011 table.
- `cargo xtask loops`, a step of `cargo xtask ci` and the verify check `spec` (GEN-022): every loop
  of the library code is named, with its bound, in its crate's GEN-022/023 inspection record, and
  every loop a record names exists; `cargo xtask loops list` prints the loops found.
- `aldis-core`: the Kani harness `error_kind_lookup_is_bounded`, the GEN-022 bound of
  `ErrorKind::from_code` for every code.
- A test for every restricted type (M2 WP-7, RS-020, VER-023): `crates/<crate>/tests/restricted.rs`
  in the eight codec crates shows, for each public type with a checked constructor, that it accepts
  exactly the range its Blue Book clause or requirement gives (every `u8`, `u16`, and `bool` input
  exhaustively), rejects every other value with the documented error, and carries every accepted
  value through its wire field. `cargo xtask restricted` (in `cargo xtask ci` and the verify check
  `spec`) fails on a restricted type that no test names in a `// restricted: <Type>` marker, and on
  a marker that names none; `cargo xtask restricted list` prints the types. New out-of-range and
  field strategies in `aldis_testkit::strategies` (VER-054).
- Bit-flip completeness of the FECF (M2 WP-7): `crates/aldis-{tm,tc,aos,uslp}/tests/bit_flips.rs`
  check every single-bit and double-bit error of fixed valid frames, the outcome stated per region
  (AOS header errors in the FHEC's symbols are corrected, the rest rejected), within the 4 095
  octets up to which the CRC-16 detects every double-bit error, derived in
  `docs/verification/derivations/crc16-bit-flips.md`. The gated tests sample the longest frames;
  the nightly job `bit-flips` runs them exhaustively. Helpers in `aldis_testkit::bitflip`. New
  requirement VER-064 (docs/06-verification.md).
- `docs/glossary.md`: a glossary in three parts. Acronyms and abbreviations (OCF, CLCW, FHP, M_PDU,
  GMAP ID, CUC, CLTU, …), each with its expansion, a plain-language meaning, the defining Blue Book
  clause, and where Aldis uses it, noting where the Blue Books spell a term differently and where an
  abbreviation is only an Aldis convention (FHEC, LVO, PC); the Aldis terms (tracks and milestones,
  work packages, decisions, open issues, every requirement area, views and writers, strict and
  lenient decoding, provenance, the verification run and merge gate, golden traces); and the CCSDS
  documents cited, with their titles and whether `specs/manifest.tsv` lists them. Linked from
  `docs/README.md`, `docs/00-introduction.md` §4, the root `README.md`, `AGENTS.md`, and the top of
  every standard and profile document.
- Reference application: a cFS flight side, `docs/reference-application.md` §10, with
  APP-110…APP-125: cFS apps on cFE, OSAL, and the pc-linux PSP that call Aldis through
  `include/aldis/aldis.h`, run in lockstep with the harness, and reproduce the Rust flight side's
  golden traces in four phases tied to the C ABI (packets, frames, COP-1, engines).
- Plan `docs/milestones/acfs-flight-side.md` (milestone ACFS): the cFS facts read at
  nasa/cFS `v7.0.1`, decisions D1–D19 (accepted), work packages, exit criteria, and risks;
  the ACFS row and phases in `docs/08-roadmap.md` §3.
- Open issues OI-36 (cFE TIME from simulated time), OI-37 (trace timing and cFS time), and OI-38
  (node protocol transport and timing): OI-37 and OI-38 resolved, OI-36 deferred.
- Glossary: cFS, cFE, OSAL, PSP, SB, SCH and SCH_LAB, CI_LAB, TO_LAB, MET, and STCF.

### Changed
- `cpp/include/aldis/core/config.hpp`: the port follows `aldis-core` 0.2.0 (commit `0655f2a`,
  `v0.2.0-m1`), not 0.1.0; `ALDIS_VERSION_*` stays 0.1.0 until differential verification against
  a Rust release starts in C1 (VER-053).
- `cargo xtask verify` (`cpp-tidy`) and the nightly `cpp.yml` run clang-tidy on `cpp/vectors/` too;
  `cargo xtask cpp layers` lists the vector libraries as tooling; the nightly bare-metal and LEON3
  jobs install the host GCC, which builds `aldis_vecgen`.
- `AGENTS.md`: trimmed to actionable agent rules by removing duplicate rows, condensing tool internals and role-specific protocols, and organizing directory-specific commands into a table (#119).
- `docs/06-verification.md`: added container engine memory and disk setup notes to §7 (#119).
- `docs/README.md`: indexed `docs/process/review.md` under process documents (#119).
- Specification: TV-023 requires an independent model to be written blind, by an author who has
  not read the implementation it checks. M2 WP-7 plans a worst-case stack gate, a loop-bound
  consistency check, and bit-flip completeness tests; WP-8 plans an assurance case and a hazard and
  failure-mode analysis.
- `AGENTS.md` has a "Decision rights" section; `docs/README.md` and `docs/glossary.md` list the
  decision rights and the decision log; the checklists and the pull request template point to
  them where an item involves a maintainer decision.
- Licensing: Aldis is dual licensed under MIT OR Apache-2.0, at the user's option (`LICENSE-MIT`,
  `LICENSE-APACHE`), for its code, test vectors, and documentation (OI-01). It was MIT only.
- Field codes are allocated in blocks (M2 plan D2, amended 2026-10-24): codes 1–106 stay as they
  are, each new crate gets a block of 50 when its plan is accepted (`aldis-cop1` 150–199,
  `aldis-link` 200–249), and fields shared by several crates take the next code of the common
  block 107–149. `cargo xtask fields` checks that every code lies in its owner's block
  (docs/03-api-model.md §9.1, API-082).
- `cargo xtask verify`: the workspace builds of the checks pass `--locked`, and `verify inside`
  refuses a checkout that building xtask changed, so a commit with a stale `Cargo.lock` fails
  instead of being resolved again in the container (VER-056).
- `cargo xtask verify`: a change to `.gitignore` alone runs only `spec`, as documentation does
  (VER-056).
- `docs/verification/analysis-records.md` is split into one file per record in
  `docs/verification/records/`, named after the first requirement ID it covers, so that a pull
  request that changes one requirement's record touches one small file. `cargo xtask trace` reads
  every file there and fails on a malformed record, one named after another ID, or a requirement
  with two records (VER-020); the trace results are unchanged. `analysis-records.md` describes
  the format.
- Milestone plans carry a work-package status table (WP, State, PRs, Notes), one row per work
  package or sub-part, and quote no requirement counts: `cargo xtask trace --scope <id>` reports
  them ([docs/08-roadmap.md](docs/08-roadmap.md) §6 rule 1, `AGENTS.md`). The M2 plan's status
  paragraph becomes a status sentence, the table, and a short history; the C0 and A0 status
  paragraphs carry no counts and stay as they are.
- GEN-022/023 inspections are approved once per milestone (maintainer decision 2026-10-21; new
  VER-061, [docs/06-verification.md](docs/06-verification.md) §8): the change that adds a loop
  or a call revises the crate's record in `docs/verification/inspections/`, prepared by its
  author and marked pending exit review, and the maintainer approves every revision since the
  last approval at the milestone exit review or before a release. No pull request waits for an
  approval. `AGENTS.md` and the M2 plan (WP-7 and a new §4 exit row) follow; past approvals
  stand.
- `cargo xtask verify` takes a check's result from an earlier published record when the check's
  inputs are unchanged (VER-056): besides the record covering the merge base with `origin/main`,
  it searches the records of the branch's earlier commits (first-parent ancestors not on `main`,
  nearest first, at most 20). After a branch merges `main`, a check whose inputs the commits from
  `main` did not touch is `reused` from the branch's previous record instead of run; a check whose
  inputs did not change against the base is `skipped` as before. Each such check names its
  `source`, the commit and record SHA-256 of the run in which it actually passed, and `verify
  check` and `--publish` re-derive every skip and reuse from git and that record (VER-058).
- Records are now format `aldis-verify-record/3` (VER-057). Formats 1 and 2 are still read, and
  a tool that knows only those refuses format 3. The record's `image` names the input hash and
  where the image came from: `built`, or `pulled` with its reference, digest, and revision.
- The `verify` workflow (VER-060) pulls the verification image from
  `ghcr.io/kscharpf/aldis-verify`, tagged by the hash of its inputs and architecture, instead of
  building it, when the image's labels name the same input hash and a commit whose image inputs
  hash to it; otherwise it builds the image and pushes it. The nightly run builds the image and
  pushes it for `main`. `cargo xtask verify image --push <repository>` pushes from GitHub Actions
  only; `ALDIS_VERIFY_PULL=<repository>` opts a local run in to pulling.
- `cargo xtask verify` runs only the checks a change needs (VER-056): each check declares its
  inputs in `xtask/src/verify/checks.rs` and is skipped, as `skipped` with the base in the
  reason, when none of them changed against the merge base with `origin/main` and a published
  passing record covers that base. A documentation-only change (`docs/`, `changelog.d/`, the root
  `*.md`, `specs/README.md`, `specs/manifest.tsv`) runs only `spec`. Every check runs when the
  change is not known, a changed path is an input of no check, no record covers the base, or with
  the new `--all` flag, which the nightly run uses. This replaces the C++-only path rule
  (`CPP_PATHS`); `--all-cpp` still forces the C++ checks.
- Records are now format `aldis-verify-record/2`, with a `scope` object (base, covering record,
  changed paths, each check's inputs, and whether each check ran and why) in place of `cpp`.
  `cargo xtask verify check` and `--publish` re-derive every skip from git and refuse a record
  whose skips do not hold (VER-057, VER-058); format 1 records are still read. `cargo xtask
  verify list` and the new `verify scope [<rev>] [--main <rev>]` show which checks a commit needs.
- `cargo xtask verify` runs the checks quickest first (gating, then non-gating, by the typical
  duration each declares), and with `--publish` stops at the first gating check that fails:
  such a run cannot publish, so the checks it did not reach are recorded as not run.
  `--no-fail-fast` and `--fail-fast` override the default (docs/06-verification.md §7, VER-056).
- `cargo xtask verify` starts no container, and pulls or builds no image, when the input rule
  skips or reuses every gating check; the record says that no image was used and names the
  hash of the image inputs (VER-057). `verify list` says whether a run would start one.
- `verify.yml` gets and pushes the image within the run (`verify --push-image`), so a run that
  needs no image fetches none (VER-060).
- `cargo xtask ci` also runs `cargo xtask header`, so a stale `include/aldis/aldis.h` fails it
  in seconds; it needs the pinned `cbindgen`. A step that cannot run counts as a problem, and
  the other steps still run.
- The check `kani` is nightly-only: it runs only with `verify --all`, as the nightly run does,
  and every other run records it as not run, "nightly only (VER-059)"; it stays non-gating
  (OI-42). In the nightly run, `cargo xtask verify nightly` fails the job when it did not pass,
  so a failing harness opens the nightly issue.
- VER-056, VER-057, and VER-060 state the fail-fast stop, the run without a container, the
  record without an image, and nightly-only checks (docs/06-verification.md §7).
- `verify --publish` publishes a passing record even when `main` moved during the run, without
  setting the `aldis/verify` status, so that the merge from `main` can reuse its results;
  `verify check` fails a head that does not contain the tip of `main` (VER-057, VER-058).
- `cargo xtask verify`: a change to the GitHub configuration that no check reads (the workflows,
  the Rust setup action, the C++ setup action's definition, the pull-request template,
  `CODEOWNERS`, and `dependabot.yml`) runs only `spec`, as documentation does, instead of the Rust
  checks or every check (VER-056).
- **Breaking:** Missed in the 0.2.0-m1 notes: `aldis-core::bits::get` and `set` report a field
  wider than the whole buffer as `buf_bits` (expected: the field width in bits, found: the
  buffer length in bits) instead of `bit_offset`. `bits::get(&[0], 0, 16)` returned
  `OutOfRange (bit_offset): expected 8, found 0` in 0.1.0-m0 and returns
  `OutOfRange (buf_bits): expected 16, found 8` since 0.2.0-m1.
- **Breaking:** Missed in the 0.2.0-m1 notes: `aldis-core::bits::get` and `set` report width 0
  as `OutOfRange (width): expected 1, found 0` instead of `expected 64, found 0`.
- **Breaking:** Missed in the 0.2.0-m1 notes: `aldis-core::container::ByteQueue::push` on a
  queue smaller than its 4-octet length prefix reports `capacity` (expected 4, found the
  capacity) instead of `record_len`. `ByteQueue::<0>::push(&[])` returned
  `OutOfRange (record_len): expected 0, found 0` in 0.1.0-m0 and returns
  `OutOfRange (capacity): expected 4, found 0` since 0.2.0-m1.
- **Breaking:** Missed in the 0.2.0-m1 notes: the `bit_offset` error of `aldis-core::bits::get`
  and `set` gives the largest valid offset as `expected`, not the buffer length in bits.
  `bits::get(&[0; 2], 9, 8)` returned `OutOfRange (bit_offset): expected 16, found 9` in
  0.1.0-m0 and returns `expected 8, found 9` since 0.2.0-m1.
- **Breaking:** Missed in the 0.2.0-m1 notes: `aldis-core::bits::read_u16` and `write_u16` give
  the largest valid offset as `expected`, and report a buffer shorter than two octets as
  `buf_len`. `read_u16(&[0; 4], 3)` went from `OutOfRange (offset): expected 4, found 3` to
  `expected 2, found 3`, and `read_u16(&[0], 0)` from `OutOfRange (offset): expected 1, found 0`
  to `OutOfRange (buf_len): expected 2, found 1`.
- **Breaking:** Missed in the 0.2.0-m1 notes: `aldis-core::counter::ModCounter::new` reports
  0 bits as `OutOfRange (bits): expected 1, found 0` instead of `expected 64, found 0`.
- TV-021: `superseded.by` is optional, as the vector schema and the `aldis-testkit` model already
  allowed. A wrong released case with no correct replacement is superseded with a reason only;
  a `by` that is present must name an existing case that is not itself superseded.
  `cargo xtask vectors validate` no longer requires it.
- AOS-002, AOS-003, AOS-012, AOS-017: the error kinds and fields of the strict checks are stated,
  and a Bitstream Data Pointer naming the zone's last bit is `InvalidField` in both modes
  (732.0 §4.1.4.3.3.3).
- **Breaking:** `aldis-time`: `Cuc::from_seconds_nanos`, and the C ABI function
  `aldis_time_cuc_from_ns`, round the fraction down instead of up (TIME-011, OI-40), as
  `CucValue::to_seconds_nanos` already did. The coarse time is now always the given seconds:
  999 999 999 ns no longer carries into the next second, and a full coarse time no longer fails with
  `OutOfRange` (`coarse`) on a carry. The round trip `CucValue → ns → CucValue` is exact if and only
  if the value is a multiple of 2⁻⁹ s, so always for 0 and 1 fine octets but no longer for every
  value of 2 or 3. The `cuc_from_ns` vectors `time-cuc-020`, `-022`, `-023`, `-025`, and `-026` are
  superseded by `time-cuc-029`…`-033`, and `time-cuc-034` is added. The cross-check discrepancy D4
  with `spacepackets` is closed (docs/verification/cross-checks.md).
- TIME-011 and the docs of `CucValue::to_seconds_nanos`, `Cuc::from_seconds_nanos`,
  `aldis_time_cuc_to_ns`, and `aldis_time_cuc_from_ns` state that the conversions take the basic
  time unit to be the second (301.0 §1.2, §3.2.1). For a Level 2 code they give the time in seconds
  only if the agency's unit is the second (#82). Behaviour is unchanged.
- Recorded maintainer exit-review approval for changed M2 inspection records of `aldis-core`, `aldis-time`, `aldis-epp`, and `aldis-spp` (VER-061); closed assurance defeater DF-4.
- Resolved OI-45 with maintainer decision for option (d) (keep and document channel-table stack requirements, confirming the 4 096-byte Rust and 10 240-byte C ABI budgets on all embedded targets); closed assurance defeater DF-1.
- Recorded maintainer decisions to keep signed verification evidence parked (#110, DF-5) and to amend M2 plan D15 deferring the JVM cross-check harness to future milestones while satisfying frame cross-checks via candidate search (VER-040), hand derivations, blind Python models, and the approved second review (OI-32).
- `docs/verification/records/VER-065.md`: conducted independent M2 exit review of the hazard and failure-mode analysis across `hazards.md` and `hazards/{tm,tc,aos,uslp}.md` (VER-065); recorded review outcome and approval row.
- `docs/verification/assurance-case.md`: closed defeater DF-15 following exit review confirmation that all failure modes are represented within the 12 categories of §1.2.
- `docs/verification/hazards/aos.md`: recorded maintainer dispositions for P-AOS-02 (accepted option (a), deferred to M4 link engine LNK-053) and P-AOS-05 (declined for M2); re-rated HZ-AOS-04 and HZ-AOS-15 to `residual`.
- `docs/standards/aos-frame.md`: documented fixed Insert Zone placement and extraction on rejected frames for the M4 link engine.
- `docs/verification/inspections/gen-022-023-aldis-ffi.md`: completed the call-graph inspection for the M1 modules (`spp.rs`, `spp_state.rs`, `time.rs`, `epp.rs`, `limits.rs`), removing the partial scope marker.
- Recorded maintainer exit-review approval for changed M2 crate inspections: `aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp`, and `aldis-ffi` (VER-061).
- `.gitignore`, `xtask/src/repo.rs`: skip `.agents` and `skills-lock.json` from file scanning and git status.
- Enforce `--strict` for `trace --scope m2` (VER-020, D1) and `assurance` (VER-066) in `cargo xtask ci`, the `spec` check of `cargo xtask verify`, and `.github/workflows/ci.yml`.
- C ABI doc-comments updated for `aldis_uslp_peek_length`, `aldis_uslp_decode`, `aldis_uslp_encode`, `aldis_tm_decode_with`, and `aldis_tm_validate`.
- Defeaters DF-3 (1 CPU-hour fuzz run) and DF-8 (nightly run) closed in `docs/verification/assurance-case.md`.
- Milestone 2 completed, milestone plan §4 exit criteria marked complete with evidence, roadmap and README statuses updated to complete.
- Specification: the frame anomaly lists and API-032 follow M2 D9 (OI-33). `FhpBeyondDataField`,
  `PointerBeyondZone`, `FhecCorrected`, `VcfLengthMismatchForQos`, and `UnknownUpid` are removed:
  out-of-range pointers are `Malformed` and a VCF Count Length mismatch is `LengthMismatch` in both
  modes (TM-008, AOS-017, USLP-004), the FHEC reports `corrected_symbols()` (AOS-014), and unknown
  UPIDs decode as `Upid::Other(n)` (USLP-007). API-032 names the TM Segment Length Identifier.
- Specification: AOS-013 and AOS-014 follow the wording of 732.0-B-5 §4.1.2.6.5: a (15,11) code
  shortened to a binary (40,24) code. AOS-014 states that three or more symbol errors can be
  miscorrected, and that the FECF is checked over the corrected header. AOS-018 bounds the B_PDU zone
  at 2 047 octets, and in Issue 5 the frame at 2 048 octets (OI-35 items 5–7).
- Specification: USLP-002 and the USLP configuration allow truncated frames only on
  variable-length VCs (Annex D1.2.1; OI-35 item 2). USLP-005 is no longer an interpretation: OI-05 is
  resolved from 732.1 §4.1.4.2 and confirmed by 700.1-G-1 and Yamcs 5.13.7.
- OI-35 items 1 and 8 resolved by the maintainer 2026-10-20: a truncated frame's construction rule
  and UPID are exposed and not checked (USLP-002); `FrameDelimiter` delimits successive frames and
  takes a truncated frame's length from its VC's configuration (USLP-015). OI-35 is resolved.
- **Breaking:** `TmFrameWriter::resume`, `TcFrameWriter::resume`, `AosFrameWriter::resume`, and `UslpFrameWriter::resume`, and the C ABI's `aldis_tm_writer_finish`, `aldis_tm_oid_writer_finish`, `aldis_tc_writer_finish`, `aldis_aos_writer_finish`, and `aldis_uslp_writer_finish`, now check that the buffer holds the unfinished headers that `begin` wrote for the same arguments (the Primary Header, and the TC Segment Header, the AOS data unit header, the USLP Insert Zone and TFDF Header, or the TM Secondary Header Identification Field). They fail with `InvalidState` (`ALDIS_ERR_INVALID_STATE`) and leave the buffer unchanged on a difference or on a buffer that was never begun, and they no longer rewrite the headers (API-023; OI-50).
- **Breaking:** `TcFrameWriter::begin` and `UslpFrameWriter::begin` store the reserved frame length in the Frame Length field of the unfinished header, where they stored 0 (TC) or 1 (USLP), so that a changed reservation is caught at `resume`. The TC unfinished frame is still rejected by every decoder as `UnsupportedVersion`, and the USLP one still has TFVN `1111` (API-023; OI-50).
- **Breaking:** `aldis-tm`: `TmPhysicalChannelConfig::new` rejects a frame length above 4 095
  octets (`MAX_FRAME_LEN`) with `InvalidConfig` (`frame_len`, `expected` 4095), since above it the
  CRC-16 misses some double-bit errors (OI-51, VER-064, `docs/standards/tm-frame.md` TM-001). The
  C ABI follows: `aldis_tm_config_check` and every call with a physical channel.
- **Breaking:** `aldis-uslp`: a VC with a truncated frame length carries only truncated frames
  (OI-48; 732.1 Annex D1.2.1, `docs/standards/uslp-frame.md` USLP-002). `encode`,
  `UslpFrameWriter::begin` and `resume`, and the C calls `aldis_uslp_encode`,
  `aldis_uslp_writer_begin`, and `aldis_uslp_writer_finish` reject a non-truncated frame on it
  with `InvalidField` (`end_of_frame_primary_header_flag`); `UslpFrame::decode`, `validate_frame`
  (new reason 18, `Reason::NonTruncatedFrameNotAllowed`, `ALDIS_USLP_REASON_NON_TRUNCATED_FRAME_NOT_ALLOWED`),
  and `UslpFrame::peek_length` reject one with the same error, in both modes. `Delimited` has a
  new variant `NonTruncatedNotAllowed` (C: `ALDIS_USLP_DELIMITED_NON_TRUNCATED_NOT_ALLOWED`, 5),
  which discards the rest of the data unit. A bit error in the End of Frame Primary Header Flag
  can no longer make a frame pass as a truncated frame without an FECF (`hz:HZ-USLP-02`).
- **Breaking:** the C view of a decoded truncated USLP frame (`aldis_uslp_frame_t`) reports `kind`
  as `ALDIS_USLP_KIND_EXPEDITED`, the processing of Annex D1.3.1 note 3 (OI-52), where it was 0
  (sequence-controlled).
- `aldis-ffi`: the `panic-handler` feature executes a trapping instruction before it spins:
  `udf #0` on Arm, `ebreak` on RISC-V, `brk #0` on AArch64, `ud2` on x86 (OI-49;
  `docs/profiles/c-abi.md` §1). A target without stable inline assembly (SPARC V8) still spins.
- **Breaking:** `aldis-tm`: the decoders that take the channel table reject an Only Idle Data frame
  (Synchronization Flag 0, First Header Pointer `0x7FE`) on a VC that uses SDLS (OI-54; 132.0
  §6.4.6.2, TM-010). `TmFrame::decode_with` and `decode_unchecked` return `InvalidField`
  (`vc_id`) in strict mode; in lenient mode they accept it and set the new anomaly
  `TmAnomaly::OnlyIdleDataOnSdlsVc` (bit 2; C: `ALDIS_TM_ANOMALY_ONLY_IDLE_DATA_ON_SDLS_VC`).
  `TmFrame::validate` rejects it in both modes. `TmFrame::decode`, which has no channel table,
  cannot check and never reports it.
- **Breaking:** `aldis-tc`, `aldis-aos`, `aldis-uslp`: `VcTable::new` returns `InvalidConfig`
  (`vc_id`) when no Virtual Channel is given (OI-55; TC-024, AOS-020, USLP-017). It was accepted,
  and every frame was then rejected, Only Idle Data frames included. The C ABI follows:
  `aldis_tc_mc_config_check`, `aldis_aos_mc_config_check`, `aldis_uslp_vc_table_check`, and every
  call that builds the table, with `vc_count` 0.
- `aldis-uslp`: `TruncatedFrameLen::new` and `get` are `const fn` (RS-012), so a `VcConfig` with a
  truncated frame length can be built in a `static`. Not breaking.
- `docs/09-open-issues.md`: new OI-42, formal verification method and tool (open).
- The workspace `unexpected_cfgs` check accepts `cfg(kani)`.
- `docs/06-verification.md` §4: the LEON3 row now runs the vectors under QEMU, non-gating.
  `docs/09-open-issues.md` OI-08 records the spike's findings, among them that the static
  library does not meet CABI-004 on SPARC (`compiler_builtins` refers to the `_Q_*` quad-float
  routines). OI-08 is resolved (maintainer, 2026-10-21): Rust on LEON stays best-effort in v1, with
  the native C++ implementation as the primary LEON path, and CABI-004 on SPARC is a recorded
  deviation (`docs/verification/records/CABI-004.md`) with an integrator note in
  `crates/aldis-ffi/README.md`: link with `--gc-sections`, or supply the `_Q_*` routines.
- AOS-003 is confirmed against CCSDS 732.0-B-4; OI-04 is resolved (M2 D11).
- USLP-010: the USLP writer takes the TFDZ as given; the Encapsulation Idle Packet fill is LNK-013's.
  OI-34 is resolved (M2 D8).
- API-013: a codec whose length depends on managed parameters (TM, AOS, fixed-length and truncated
  USLP frames) takes the channel configuration in `peek_length` (M2 D14).
- OI-33 resolved (M2 D9); OI-06 partly resolved for USLP UPIDs (M2 D13); OI-32 and OI-35 updated
  (M2 D15, D17).
- **Breaking:** `aldis-aos`: `PacketPointer::StartsAt` holds a `PacketOffset` instead of a `u16`, so
  an offset at a reserved pointer value can no longer be built (RS-020).
- **Breaking:** `aldis-tm`: `PacketPointer::StartsAt` holds a `PacketOffset` instead of a `u16`, so
  an offset that `to_raw` wrote as "no packet start" can no longer be built (RS-020). The C ABI is
  unchanged: `aldis_tm_fields_t::packet_offset` stays a `uint16_t`, and an offset above `0x7FD`
  fails as before.
- **Breaking:** `aldis-uslp`: `FirstHeaderPointer::PacketStartsAt` and `LastValidOctet::At` hold a
  `TfdzOffset` instead of a `u16`, so an offset of `0xFFFF`, which `raw` wrote as "none", can no
  longer be built (RS-020).
- `test-vectors/tm/security_layout.json`: the `validate` operation decodes the frame with the
  master channel's security layouts before checking it (c-abi.md §11), in the model and in the
  Rust, exerciser, and C ABI runners; `tm-sec-val-001`, which validated a frame decoded
  without SDLS, is superseded by `tm-sec-val-002`.
- `aldis-tm`, `aldis-ffi`: `TmFrameWriter::begin_oid` and `aldis_tm_oid_writer_begin` (both generator
  forms) reject a VC that uses SDLS with `InvalidField` naming `vc_id`, after the `sync_flag` check and
  writing nothing (TM-010). No signature changes.
- `aldis-uslp`: `VcTable`'s `VcLookup::vc` answers VCID 63 of a known Spacecraft ID on a fixed-length
  Physical Channel without an entry, as a fixed-length VC with the defaults of `VcParams::new`; an entry
  still overrides them, and VCID 63 stays invalid on a variable-length channel (USLP-009,
  `docs/standards/uslp-frame.md` §2.1). No signature changes.
- API-031 lists two exceptions (a CLCW with COP in Effect other than `01`; OID data, which a stateless
  decoder cannot check); API-071 limits the Security Trailer to 65 535 octets; the OID sources of
  CMN-020…022 include the AOS and USLP Blue Books.
- Test vectors (unreleased): descriptions, requirement citations, and provenance notes of the `tm/`,
  `tc/`, `aos/`, `uslp/`, and `core/ocf.json` files, regenerated from their models. `tm-oid-002` and
  `uslp-oid-019` are superseded by `tm-oid-005` and `uslp-oid-042`. The follow-up cases of the review (TM 8, TC 29, AOS 1, USLP 31) were added by a blind second model writer, and the models' `--write` generated them.
- **Breaking:** `aldis-core`: `FieldId` holds a code and a display name and is built only by the
  constants of `aldis_core::field`; `FieldId::new(name)` is removed. Equality and hashing use the
  code alone (API-082). `Error::out_of_range` takes a `FieldId` instead of a name.
- **Breaking:** errors that named `version` now name `packet_version` (`aldis-spp`, `aldis-epp`),
  the CDS day segment is `cds_day` instead of `day` (`aldis-time`), and the frame layout's data
  field is `frame_data_field` instead of `data` (`aldis-core`). Only the display text changes;
  compare fields by their constants.
- **Breaking:** `aldis-testkit`: `Outcome::Err` is a struct variant with `kind` and `field`.
- The property tests of `aldis-core`, `aldis-time`, `aldis-spp`, and `aldis-epp` are integration
  tests, `crates/<crate>/tests/properties.rs`, that draw their inputs from the shared strategies
  (VER-023, VER-054). The decoder properties run 1024 cases (256 before) on inputs that mix
  arbitrary octets with octets next to the boundaries; the others keep 256. The saved
  `proptest` regressions moved to `crates/<crate>/proptest-regressions/properties.txt`, and their
  shrunk inputs are also replayed as plain tests.
- The four crates no longer have a direct `proptest` dev-dependency; they use the one
  `aldis-testkit` re-exports.
- `cargo xtask fields` also generates the `ALDIS_FIELD_*` constants of the C ABI
  (`crates/aldis-ffi/src/field.rs`), and checks that every "Used by" entry of the field
  catalogue names a codec crate: `core`, `time`, `spp`, `epp`, `tm`, `tc`, `aos`, or `uslp`
  (issue #65).
- `cargo xtask consistency`: the C ABI runner must run the `core/` files of the `aldis-core`
  modules that have a C ABI (`FFI_FILES`), and no longer requires the rest of `core/`, whose
  modules have none.
- The Loops tables of `docs/verification/inspections/` gain an `ID` column that names each loop;
  the `aldis-ffi` table gains the loops of `lib.rs`, `spp_state.rs`, and `time.rs`.
- The nightly job `stack-sizes` runs `cargo xtask stack <target>`; `cargo xtask stack-sizes`, which
  reported the exerciser's frames, is removed.
- `cargo mutants` leaves out the Kani proof modules (`.cargo/mutants.toml`), and the nightly job
  `mutants` logs disk and memory before, during, and after the run and keeps the report on failure
  (issue #96).
- **Breaking:** `aldis_aos::PacketPointer::to_raw` and `header_octets` return `Option`, `None`
  for an offset that does not fit the edition's pointer, instead of saturating to "no Packet
  starts"; `PacketPointer::classify` in `Issue4Compatible` rejects a raw value wider than the
  11-bit pointer as `Malformed` (`first_header_pointer`). Values off the wire, the writers'
  errors, and the C ABI are unchanged.
- `TcFrame::decode` and `UslpFrame::decode` document that a bit error in the Frame Length field
  can make them accept a shorter frame, so a receiver of delimited frames runs `validate_frame`;
  `aldis_aos::PacketPointer::classify` documents that `raw` is the edition's masked pointer field.
- The verify checks that read the Rust crates also read `specs/sana/**`, which the USLP UPID test
  compares with the generated table.

### Fixed
- `cargo xtask header c-smoke` reads the static library's native dependencies with cargo's colour
  off, so `CARGO_TERM_COLOR=always` no longer leaves an escape sequence in the link line (#86).
- The nightly big-endian job no longer runs the `xtask` tests in the `cross` image, whose git is
  older than `git merge-tree --write-tree` needs; the verify check `big-endian` still runs them (#86).
- `cargo xtask features` checks `aldis-aos` too (GEN-006): it joined the coverage gate in M2
  WP-4 but not the `features` list, which `cargo xtask consistency` reported. Its `alloc` and
  `std` features are additive.
- `xtask` tests: the temporary git repositories of the vector and verify-scope tests no longer use
  `git init -b` or `git diff --no-relative` (git 2.28), which failed the nightly big-endian job,
  whose `cross` image has an older git (#62).
- `cargo xtask verify` no longer skips a check because the record covering the base skipped it
  under an older rule: a result is taken only from the record in which the check ran, with inputs
  identical to the commit's. Before, a C++ check skipped by a format 1 record (whose C++ paths did
  not list `crates/*/Cargo.toml`, an input of every check since VER-056) could keep being skipped.
- `cargo xtask verify` no longer skips the C++ checks when `git diff` fails, and now runs them
  when a file they read outside `cpp/` changes: the image inputs (`rust-toolchain.toml`,
  `tooling-nightly`) and what building xtask reads (`Cargo.toml`, `Cargo.lock`, the crate
  manifests, `aldis-testkit`).
- `aldis-ffi`: prove non-null pointer invariants to static analysis (CodeQL alerts #1–#11, #127–#129)
  by using `NonNull` in `abi::call`, `abi::write_detail`, and `handle::Handle`, validating nullness
  before casting in `abi::config` and `abi::configs`, and reordering null checks before slot pointer
  derivation in `handle::init`. No public ABI or behavior changes.
- `cargo xtask vectors validate` (TV-021): compares only with release tags (`v[0-9]*`) reachable
  from HEAD, so a branch or `git bisect` step that predates a newer release, or a stray tag
  such as `vendor-snapshot`, no longer fails validation; reads released files without the
  `git cat-file --batch` deadlock that hung on tags with thousands of files; lists released
  files with `git ls-tree -z`, so a released file with a non-ASCII name can no longer be
  deleted unnoticed; reports a problem with one tag or file and goes on instead of stopping;
  reports a current file that is not valid JSON once, as a parse error, instead of also as
  "removed or renamed"; reports each problem once, naming the oldest release, instead of
  once per release and twice for a missing `by`. Its git logic is tested against temporary
  repositories.
- `aldis-core`: the internal `usize` narrowing helpers also check against `u8::MAX` and
  `u16::MAX`, so a limit above the target type can never truncate a value (GEN-020). No
  public behaviour changes.
- CI / `aldis-spp`: bound `Segmenter::next_packet` test loops in `segment.rs` and `vectors.rs` to prevent heap exhaustion when mutated, resolving OOM terminations in nightly mutation testing (issue #125, VER-025); shard the nightly `mutants` job into a per-crate matrix with `PROPTEST_CASES: 32` to run within timeout bounds.
- fuzz: update the USLP `ffi_decode` harness to assert `ALDIS_USLP_KIND_EXPEDITED` for truncated
  frames (OI-52), gate `truncated_frame_len` on variable-length channels to restore coverage of full
  frames (OI-48), and add regression vector `uslp-trunc-056` for issue #124 (VER-022, TV-020).
- `cargo xtask fields` writes the `aldis-ffi` field assertions as rustfmt lays out a comparison
  too long for one line.
- `docs/standards/uslp-frame.md` §2.1 cited 732.1-B-3 Table 5-2 note 2 for the opposite of what it
  says; API-032 named the TM Packet Order Flag with Sync Flag = 1 where Sync Flag = 0 is the anomaly.

## [0.2.0-m1] — 2026-10-20

Milestone 1, Packets and time: the time code, Space Packet, and Encapsulation Packet codecs `aldis-time`, `aldis-spp`, and `aldis-epp`, with their vectors, cross-checks, fuzz targets, and on-target exerciser runs ([plan](docs/milestones/m1-packets-time.md)). Each section lists the M1 changes first, then the shared verification tooling (the local merge gate), then the work in progress on the native C++ foundations (C0) and the reference application (A0).

### Added
- VER-055: every tool and document the specification depends on is free for this project; no commercial tools or purchased documents. PR template checklist item.
- Unified roadmap ([docs/08-roadmap.md](docs/08-roadmap.md)): three v1 tracks, Rust reference (M0–M6), native C++ (C0–C5), and reference application (A0–A6), with entry gates, an overlap table, release rules, and rules for working on the tracks in parallel. `AGENTS.md` points contributors and agents to their track.
- Spec: a native C++ implementation is developed alongside the Rust crates for legacy flight software; the Rust crates stay the reference and the recommended choice. New draft [C++ profile](docs/profiles/cpp.md) (area CPP: CPP-001…CPP-061), VER-053 (differential testing of native implementations against the Rust reference), VER-054 (shared property-test strategies), and OI-19…OI-22 (C++ standard, capacities in the C++ API, C ABI for the M1 codecs, C++ static analysis tools).
- RS-020 (Rust profile) and an `AGENTS.md` rule: parse, don't validate. The violations found by the audit (#22) are fixed; see **Changed**.
- `aldis-epp`: `DataUnit`, a user data unit checked against the managed parameters by `EppConfig::data_unit`; `EppConfig::header` sizes a checked packet's header from its length. `encode` takes a `&DataUnit` and `EncapsulationWriter::begin` takes the `EppConfig`, so the sending-end check of 133.1 §4.2.1.1.2 cannot be skipped (EPP-010). `EncapsulationFields` has private fields and the constructors `new`, `extended`, `with_user_defined`, and `from_parts`; `EpiSet::from_bits`, `ExtendedEpiSet::from_bits`.
- `aldis-time`: `ReservedPField` (wrapped by `TimeCode::Unknown`), `TimeCodeId::{Reserved000, Reserved011, Reserved111}` and `is_reserved`, and `Cuc::from_seconds_nanos` returning a `Cuc` (replacing `CucValue::from_seconds_nanos`).
- `aldis-time` value types follow parse, don't validate: every field of `CcsValue` and `CdsValue` is a type that can hold only values 301.0 allows, decoders parse each field into its type as they read it, and `Ccs::new` / `Cds::new` check only the fit to the format (variation, sub-second digits, day segment length, resolution).
- `aldis-time` binary codes (M1 WP-1, part 1): P-field parsing with undefined-extension handling (`PField`, `PFieldTail`, `TimeCodeId`); CUC (`CucFormat`, `CucValue` with left-aligned fine time and nanosecond conversions, `Cuc`); CDS (`CdsFormat`, `CdsValue`, `Cds`, with range-checked `MsOfDay`, `Micros`, `Picos`, and `SubMillis`); CCS (`CcsFormat`, `CcsValue`, `Ccs`); range-checked calendar fields `Year`, `Month`, `Hour`, `Minute`, `Second`, and `CalendarDate`, which exists only for real dates (module `calendar`); agency-defined codes (`AgencyFormat`, `Agency`); `decode_with_pfield`, `decode_implicit`, `TimeCode::encode`. TIME-001…TIME-006, TIME-010…TIME-014, TIME-020…TIME-022, TIME-030, TIME-034.
- TIME-005, TIME-006, and `TimeAnomaly::UndefinedPFieldExtension` (bit 3); TIME-021, TIME-030, and TIME-033 clarified (OI-18).
- Vectors `test-vectors/time/{pfield,cuc,cds,ccs}.json`; fuzz targets `time_pfield`, `time_cuc`, `time_cds`, `time_ccs`; `aldis-time` joins the coverage gate, the nightly mutants job, and the exerciser.
- `aldis-epp` (M1 WP-3): CCSDS 133.1-B-3 Encapsulation Packet codec. Zero-copy `EncapsulationPacket::decode` (strict and lenient) and `peek_length` for all four header lengths; owned `EncapsulationHeader`; one-shot `encode` with the `HeaderSize::{Auto, Fixed}` policy, the two-phase `EncapsulationWriter`, and `idle`; managed parameters `EppConfig` with caller-supplied `EpiSet` and `ExtendedEpiSet` (no SANA registry, D4). EPP-001…EPP-012.
- EPP-012 and the EPP-008/EPP-010 clarifications recorded in OI-17.
- Vectors `test-vectors/epp/` (8 files, hand-derived from an independent model of 133.1-B-3); fuzz targets `epp_decode` and `epp_roundtrip`; `aldis-epp` joins the coverage gate, the nightly mutants job, and the exerciser.
- `aldis-core::mode`: `DecodeMode { Strict, Lenient }`, default `Strict` (API-030; M1 D1).
- `aldis-core::anomaly`: `Anomaly` trait (stable bit index per flag), `CAPACITY` (32), and `AnomalySet<A>`, a `Copy` bit set with `empty`, `from_flags`, `insert`, `contains`, `iter`, `is_empty`, `len`, `bits` (GEN-043; M1 D1). Re-exported at the crate root with `DecodeMode`.
- `aldis_core::anomaly_flags!`: declares a `#[non_exhaustive]` flag enum (GEN-081) with its `Anomaly` impl and `ALL` from one table; out-of-range or duplicate bit indices fail to compile.
- Anomaly flag enums `TimeAnomaly`, `SppAnomaly`, and `EppAnomaly` in the codec crates, with bit indices listed in the standard documents (API-033). Bit 1 of `TimeAnomaly` and bit 2 of `EppAnomaly` are reserved and never raised (OI-15, OI-16).
- TIME-034 (CCS day-of-year high nibble). TIME-002, TIME-003, TIME-020, SPP-004, EPP-005, and API-032 clarified by OI-15 and OI-16.
- Fuzz target `anomaly_set_model` (VER-022).
- `aldis-core::bits`: big-endian `read_u32`, `write_u32`, `read_u64`, `write_u64` octet helpers alongside `read_u16`/`write_u16`.
- `aldis-core::crc`: `PRESET` constant and incremental `crc16_update`, `crc16_update_bitwise`, `crc16_update_table` functions (CMN-001…004).
- `aldis-core::ids`: `MAX` constants and `From<Id>` conversions for `Apid`, `SeqCount`, `FrameSeqNum`, and `PacketVersion`; infallible `SeqCount::classify`, `FrameSeqNum::classify`, and `From<{SeqCount, FrameSeqNum}> for ModCounter` (CMN-031).
- `aldis-core::container`: `FixedVec::{as_mut_slice, capacity, is_full}`, `PartialEq`/`Eq` on `FixedVec`, and `ByteQueue::clear`.
- `as_conversions = "deny"` in `[workspace.lints.clippy]` (VER-001).
- Selected Clippy nursery lints denied in every lint table: `missing_const_for_fn`, `derive_partial_eq_without_eq`, `too_long_first_doc_paragraph`, `redundant_clone`. The nursery group stays off, and each lint considered is recorded with its decision in `docs/profiles/rust.md` §5 (RS-019).
- `aldis-time` ASCII codes and conversions (M1 WP-1, part 2): `AsciiTime` parses and formats complete codes A and B with up to 12 fractional digits (`Fraction`, `AsciiOptions`, `MAX_ASCII_LEN`, `MAX_FRACTION_DIGITS`); `AsciiSubset` parses any subset of 301.0 §3.5.1.3; `UtcInstant` converts between CDS, CCS, and ASCII (`AsciiFormat`), truncating toward zero, with the CDS leap-second millisecond mapped to 23:59:60; instants order chronologically. The day arithmetic is closed-form (M1 D3). Bounded calendar fields implement `Default` (their smallest value). TIME-031…TIME-033.
- OI-29 (ASCII grammar details and second 60 in conversions), resolved; [time-codes.md](docs/standards/time-codes.md) §5.1 and §5.2 record the grammar and conversion rules.
- Vectors `test-vectors/time/{ascii,convert}.json`; fuzz target `time_ascii`.
- `aldis-spp` (M1 WP-2, part 1): CCSDS 133.0-B-2 Space Packet codec. Zero-copy `SpacePacket::decode` (strict and lenient) and `peek_length`; owned `SpacePacketHeader` with `PacketType`, `SequenceFlags`, and `packet_name` for telecommand packets; `SppConfig` (Maximum Packet Length) and the checked `PacketData`; one-shot `encode` and the two-phase `SpacePacketWriter`, which derive the Secondary Header Flag and Packet Data Length; `IdlePacket` builder with a repeating pattern; `SequenceCounter` and `SequenceTracker<N>`; `OctetStringEncoder`; `SecondaryHeaderLayout` and `SpacePacket::split` for a caller-supplied per-APID secondary header. SPP-001…SPP-007, SPP-009…SPP-012, GEN-053.
- OI-30 (time secondary header codes and P-field, layout API, tracker capacity, reassembly rules), resolved; [space-packet.md](docs/standards/space-packet.md) §5 records the behavior details.
- Vectors `test-vectors/spp/` (8 files, hand-derived from an independent model of 133.0-B-2); fuzz targets `spp_decode`, `spp_stream`, `spp_roundtrip`, and `spp_tracker_model`; `aldis-spp` joins the coverage gate, the nightly mutants job, and the exerciser.
- `aldis-spp` segmentation and reassembly (M1 WP-2, part 2): `Segmenter` splits an SDU across packets of one APID (flags `11`, or `01`, `00`…, `10`); `Reassembler` rejoins them into a caller buffer, detects sequence gaps, and never gets stuck: each `Outcome` reports the discarded partial SDU (`Discard`) and the packet's `Status`. SPP-013.
- `aldis-spp` time secondary header (feature `time`): `TimeHeaderLayout` (CUC or CDS, with or without its P-field, then an Ancillary Data Field) as `SecondaryHeaderLayout::Time`; `SplitPacket::time_code` and `ancillary_data`. With a P-field the time code ID must match the configured code (OI-30). SPP-008.
- Vectors `test-vectors/spp/{segment,reassemble,secondary_header_time}.json`; fuzz targets `spp_segment_reassemble` and `spp_time_header`; the exerciser enables `aldis-spp/time`.
- `crosscheck/` workspace (M1 WP-4, VER-040): runs the `spp/` vectors through `spacepackets`
  0.18.0 and fails on any disagreement; run with its own tests by the `spec` check of
  `cargo xtask verify` and traced by `cargo xtask trace` (VER-040). Its own
  `crosscheck/deny.toml` adds one advisory exception (RUSTSEC-2024-0436, `paste` unmaintained);
  evaluation, coverage, and results in `docs/verification/cross-checks.md`.
- `crosscheck/` (M1 WP-4, VER-040): checks `time/pfield`, `time/cuc`, `time/cds`, ASCII
  formatting in `time/ascii`, and the Time Code Field of `spp/secondary_header_time` against
  `spacepackets` 0.18.0, with no disagreement. Four differences in `spacepackets` are resolved
  against 301.0 and reported as known discrepancies under the exact condition each describes: the
  CUC fine-time divisor, the reserved CDS sub-millisecond code, the ignored P-field extension
  flag, and the rounding from nanoseconds to fine time (`docs/verification/cross-checks.md` §3).
  §4 there records, for each feature `spacepackets` does not cover, what was searched and which
  candidates qualify.
- `docs/09-open-issues.md`: OI-32 records the independent implementations M1 found but does not
  run (VER-040), targeted at M2.
- `cargo xtask features` and the verify check `features` (GEN-006): no API of `aldis-core`,
  `aldis-time`, `aldis-epp`, or `aldis-spp` is gated on `alloc` or `std`, those features enable
  nothing else, and every test, the shared vectors among them, passes alike without them, with
  `alloc`, and with `std`. The nightly CI workflow runs it as the job `features`.
- `aldis`: feature `spp-time`, which enables `spp`, `time`, and the `time` feature of
  `aldis-spp`, so the Space Packet time secondary header (SPP-008) is reachable through the
  facade without `full` (maintainer decision 2026-10-19). `full` now includes it.
- `cargo xtask trace --scope m1` runs, without `--strict` until the M1 exit, in the `spec`
  check of `cargo xtask verify`, in `cargo xtask ci`, and in the nightly CI (M1 D8, VER-020).
- Exerciser (M1 WP-5, D6, VER-010): runs under QEMU with semihosting on all three bare-metal
  targets, not only Cortex-M: Cortex-R5F on QEMU's `none` machine (`-cpu cortex-r5f`; the
  Cortex-R52 board `mps3-an536` needs QEMU 9.0, newer than Ubuntu 24.04's 8.2.2) and RV32 on
  `virt`. Start-up code, fault vectors that exit with failure, and a one-line report per run
  (`exerciser/src/target.rs`, `link-cortex-r.x`, `link-riscv.x`). The verify checks
  `exerciser-<target>` and the nightly `exerciser` job run all three, with one retry.
- Exerciser (M1 WP-5, D7): `build.rs` embeds every live case of `test-vectors/{time,spp,epp}/`,
  and a handler per crate compares every output exactly, as the host suites do (TV-012).
- [Review of the hand-derived M1 vectors](docs/verification/reviews/m1-vectors.md) (EPP, CCS,
  ASCII; M1 risk table): approved, with two corrections to files not yet released, the
  provenance note of `time/ccs.json` and the case order of `epp/invalid.json`.
- Changelog fragments: each pull request adds one file to `changelog.d/` (since removed) instead of editing `CHANGELOG.md`, so parallel pull requests do not conflict. `cargo xtask changelog check` (part of `cargo xtask ci`), `show`, and `release <version> <date>`. The entries that were under **Unreleased** moved to `changelog.d/unreleased-before-fragments.md`.
- `cargo xtask verify` (`xtask/src/verify/`): the verification run that gates merges (VER-056…VER-058, OI-31). It runs one check per former pull-request job of `ci.yml` and `cpp.yml`, under the same names, in a pinned image (`xtask/verify/Containerfile`) on a fresh checkout of HEAD. It writes an evidence record to `target/verify/<commit>/record.json`; `--publish` pushes the record as a git note under `refs/notes/verify` and sets the commit status `aldis/verify`. `verify check` re-checks a published record.
- `.github/workflows/verify.yml` (VER-060): the same verification run on a GitHub-hosted runner, started only by hand on a branch (`gh workflow run verify.yml --ref <branch>`). It publishes the record and status like a local run; the record's new `source` field and the status's link name the run.
- `setup_cpp.py versions`: checks the installed C++ tools against `cpp/toolchains.lock` without reinstalling them; the script no longer calls `sudo` when it runs as root.
- Plan for milestone C0, the foundations of the native C++ implementation ([docs/milestones/c0-foundations.md](docs/milestones/c0-foundations.md)): pinned GCC 14 and Clang 23 toolchains, CMake presets for the host and bare-metal targets, C++ CI jobs, the freestanding link check, CodeQL Coding Standards, the IKOS evaluation, and the port of `aldis-core` to `aldis_core`. Decisions D1–D19 are proposed.
- Open issues OI-27 (how a field identifier crosses the C ABI) and OI-28 (CPP-001 and header-only code) in [docs/09-open-issues.md](docs/09-open-issues.md).
- C0 WP-0: pinned C++ toolchains in `cpp/toolchains.lock` (CPP-001, CPP-003): LLVM 23.1.3, Ubuntu 24.04 GCC 14.2.0 (host and s390x cross), Arm GNU Toolchain 14.3.Rel1, xPack RISC-V GCC 14.3.0-1, Gaisler BCC2 2.2.4 (GCC 10.2.0, LEON3, non-gating), CMake 4.4.4, Ninja 1.13.2, QEMU 8.2.2, and the CodeQL 2.21.4 bundle with the Coding Standards packs 2.62.0, each with its download URL and SHA-256 or exact package version.
- Composite action `.github/actions/setup-cpp`, the counterpart of `setup-rust`: installs exactly the locked versions, caches archives by checksum, and checks and prints every version.
- `cpp/cmake/AldisToolchainCheck.cmake`: configure fails when the compiler differs from the lock (D1).
- Workflow `.github/workflows/cpp.yml` (name "C++", D17) with job `cpp-toolchains`, which installs the whole set on `ubuntu-24.04` and `ubuntu-24.04-arm` and checks that a changed checksum and a mismatching compiler are rejected.
- C0 WP-1: the `cpp/` CMake project of the native C++ implementation (`cpp/README.md`). C++20 throughout (CPP-001); `aldis::core` as a header-only `INTERFACE` target, so far with `config.hpp` (language-standard check, version and `ALDIS_LIFETIMEBOUND` macros) and the umbrella `core.hpp`; every header also compiled on its own with the library warning set; options `ALDIS_BUILD_TESTS`, `ALDIS_BUILD_EXERCISER`, `ALDIS_HEADER_CHECKS`, `ALDIS_CRC_TABLE`, `ALDIS_TEST_HOOKS`, `ALDIS_SANITIZE`, `ALDIS_COVERAGE`.
- Presets `host-gcc`, `host-clang`, `host-clang-sanitize`, `host-clang-coverage`, `s390x-gcc`, `cortex-m4f-{gcc,clang}`, `cortex-r5f-{gcc,clang}`, `rv32imac-{gcc,clang}`, and `leon3-gcc`, each with a build and a test preset and a configure-time check against `cpp/toolchains.lock`; toolchain files with the CPU and ABI flags of the Rust bare-metal targets.
- The warning flags of `docs/profiles/cpp.md` §5 (CPP-040), `cpp/.clang-format` (CPP-044), and `cpp/.clang-tidy` with the naming rules and the CPP-002 include allow-list (CPP-041); dev-only dependencies GoogleTest v1.18.0 and nlohmann/json v3.12.0 pinned by commit and SHA-256 (CPP-003).
- Installed CMake package: `find_package(aldis)` exports `aldis::core` (CPP-012).
- CI jobs `cpp-format`, `cpp-build` (x86_64 and aarch64, GCC and Clang, with a consumer of the installed package), `cpp-tidy`, `cpp-sanitize`, `cpp-big-endian` (s390x under QEMU), `cpp-bare-metal` (three targets, both compilers), and the non-gating `cpp-leon3`.
- C0 WP-2: `cargo xtask trace` reads `// req:` annotations from the C++ sources and headers under `cpp/` (artefacts labelled `test (C++)`), and `cargo xtask spec-lint` checks the requirement IDs in them (CPP-057).
- `cargo xtask cpp layers [<build-dir>]`: the CMake target graph (`cmake --graphviz`) against the crate layer rules, GEN-002 exceptions included (CPP-010); CI job `cpp-layers`.
- `cargo xtask cpp unsafe-optouts`: bounds-checking opt-outs (`unsafe_buffer_usage` pragmas, bounds-check `NOLINT`s, `.clang-tidy` disables) only in `cpp/include/aldis/core/bytes.hpp`, and every `NOLINT` names its check (CPP-031, CPP-042). Part of `cargo xtask ci`.
- `cargo xtask cpp link-check <elf>` and `cargo xtask cpp statics <elf>` for the freestanding exerciser of WP-5: no allocation, exception, abort, assertion, guard, or unwinder symbol, and no writable `aldis::` data (CPP-050, CPP-030).
- CI job `spec` reports `cargo xtask trace --scope c0` (strict from the C0 exit on).
- C0 WP-3: `aldis_core`, the native C++ port of `aldis-core` (`0655f2a`): `error.hpp` (catalogue, `Error`, `Result<T>`, `Result<void>`), `mode.hpp`, `anomaly.hpp`, `bits.hpp`, `ids.hpp`, `time.hpp`, `crc.hpp`, `oid.hpp`, `counter.hpp`, `container.hpp`, `security.hpp`, `layout.hpp`, and the umbrella `core.hpp` under `cpp/include/aldis/core/`; header-only, `constexpr`, and `noexcept` throughout (CPP-020, CPP-021, CPP-023, CPP-025, CPP-026, CPP-034).
- The C++-only modules `bytes.hpp`, the one audited place with raw element access (CPP-031), and `arith.hpp`, the wrapping, checked, and saturating helpers that no unsigned operation in them overflows (CPP-032).
- The test-only `FixedPatternSecurity` hook in `cpp/testing/` (target `aldis::core_test_hooks`, never installed).
- GoogleTest ports of every `aldis-core` unit test with the same `// req:` IDs, plus tests of the C++-only behaviour and `noexcept`/trivially-copyable checks of the public API; the property tests become seeded randomized tests.
- Plan for milestone A0 (reference application scenario): [docs/milestones/a0-scenario.md](docs/milestones/a0-scenario.md), with proposed decisions on the mission story, APID and channel plans, time model, loopback link, scripted faults, golden-trace format, and the `refapp/` workspace.
- Open issues OI-23 (golden traces and the vector suite), OI-24 (out-of-process nodes in the reference application), OI-25 (framework trace format), and OI-26 (spacecraft ID for the reference scenario) in [docs/09-open-issues.md](docs/09-open-issues.md).
- Scenario of the reference application, [docs/reference-application.md](docs/reference-application.md), with the new requirement area APP (APP-001…APP-108): flight and ground roles, nominal timeline, APIDs and packet layouts, TM/TC link plan with COP-1 parameters, time model, link model and scripted faults, golden-trace format, node interface and protocol, and implementation rules for `refapp/`. The spacecraft ID `0x155` is proposed and labelled fictional pending OI-26.
- `refapp/`: workspace skeleton of the reference application (A0 WP-2): `refapp-scenario` (generated scenario constants and the node interface), empty `refapp-flight` (`no_std`, no heap) and `refapp-ground` nodes, and `refapp-harness` (scheduler and link model, golden-trace writer, canonical-form check, comparator, scenario schema and code generator). The scenario file `refapp/scenario/scenario.json` and the header-only golden trace `refapp/golden/a0-empty.jsonl`. See [refapp/README.md](refapp/README.md).
- `cargo xtask refapp-deps` (also in `cargo xtask ci`): `refapp/` uses Aldis only through the `aldis` facade and forbids unsafe code (APP-100, APP-105).
- CI job `refapp`: fmt, Clippy, tests, the `thumbv7em-none-eabihf` build of `refapp-flight`, generated-file and golden-trace checks; `cargo deny` also covers `refapp/` (APP-106).

### Changed
- **Breaking (RS-020):** `aldis-core::security::FrameCtx` is a `#[non_exhaustive]` enum with one variant per protocol carrying typed `SpacecraftId`, `VcId`, and `MapId` (accessors `protocol`, `scid`, `vcid`, `map_id`); `SecuredFields` has private ranges with `new` (checks order and adjacency) and `from_layout`; `FrameLayout` has private fields with accessors and `frame_len`; the `Anomaly` trait is sealed, implemented by `anomaly_flags!`.
- OI-22 resolved (maintainer): free tooling only. VER-052 now requires SEI CERT C++ and AUTOSAR C++14 instead of MISRA C++:2023, and VER-051 SEI CERT C instead of MISRA C:2012. CPP-043 names the CodeQL Coding Standards packs and clang-tidy `cert-*`; NASA IKOS is evaluated in C0.
- Roadmap: every Rust milestone defines the C ABI for what it adds (engine handles from M3, not M5); M2 WP-0 adds the M1 codecs' C ABI and the shared strategies (VER-054); M5 becomes C ABI completion; the end-to-end demo moves from M6 to the reference application (A6).
- OI-19, OI-20, and OI-21 resolved (maintainer): C++20, engine capacities as template parameters, and the M1 C ABI slice in M2 WP-0. CPP-001, CPP-022, CPP-024, and CABI-072 updated; the C++11 form of CABI-072 is dropped.
- Spec for the native C++ track: CABI-070 (native C++ also exposes `aldis.h`, used by the differential tests), CABI-071 (native implementations pass VER-053), CABI-072 (`alignas` storage instead of the deprecated `std::aligned_storage`; standard and capacities open in OI-19/OI-20), VER-052 (MISRA C++:2023), VER-023 (inputs from the shared strategies), GEN-071, VER-002, and RS-017 (`unsafe` allowed in the dev-only differential-test bridge). Roadmap: native C++ moves to phase 1, and the codec part of the C ABI is defined with each codec's milestone (§3.1).
- **Breaking:** `aldis` facade crate no longer enables `core` by default (`default = []`), matching `docs/profiles/rust.md` §2; enable `core` or `full` explicitly.
- **Breaking:** `aldis-core::ids`: `FrameSeqNum`'s inner `u8` field is now private (use `FrameSeqNum::new` or `From<u8>`, and `FrameSeqNum::get` or `u8::from`), matching the other identifier types (RS-010); `SpacecraftId`, `VcId`, and `MapId` `TryFrom` errors now report snake_case `FieldId`s (`"spacecraft_id"`, `"vc_id"`, `"map_id"`) instead of type names (API-041).
- `aldis-core::security`: `SecurityLayout::new` is now a `const fn` (RS-012), `SecurityLayout::MAX_TRAILER_LEN` is public alongside `MAX_HEADER_LEN` (API-070, API-071), and `Protocol` is `#[non_exhaustive]` (GEN-081).
- Now `const fn`: `AnomalySet::iter`, `ByteQueue::clear`, `FixedVec::clear`, and `ModCounter::increment` in `aldis-core`; `CdsFormat::t_len` and `TimeCode::format` in `aldis-time` (RS-019).
- `aldis-testkit` model and runner types now derive `Eq` (RS-019).
- Forwarded `alloc`, `std`, and protocol sub-features across `aldis`, `aldis-spp`, `aldis-link`, `aldis-cop1`, `aldis-framework`, and `aldis-ffi`.
- `aldis-spp`: the vector test runs without the `time` feature too, skipping only
  `secondary_header_time`.
- `aldis-time`, `aldis-spp`, `aldis-epp`, `aldis`: every public item's docs cite its CCSDS
  clause and requirement IDs (RS-015); the crate docs list the clauses of each module and
  document the features. `docs/09-open-issues.md` records M1 D4 against OI-06, which stays open.
- `aldis-core` tests: five mutants that survived the M1 pre-release mutation run are now caught
  (`FixedVec::eq`, `SeqCount::get`, both `FrameSeqNum` conversions, `PacketVersion::get`), and
  `docs/verification/mutants-triage.md` records the full run over all four M1 crates (VER-025).
- M1 exit: `cargo xtask trace --scope m1` runs with `--strict` in the `spec` check of
  `cargo xtask verify`, in `cargo xtask ci`, and in the nightly CI, so every M1 requirement must
  keep an artefact (VER-020, M1 D8); all 91 are traced.
- GEN-022 and GEN-023 re-inspected before the release in `aldis-core`, `aldis-time`,
  `aldis-spp`, and `aldis-epp` (`docs/verification/inspections/`): every loop is bounded, there
  is no recursion, and nothing blocks.
- Workspace version 0.2.0.
- C++ CI (`.github/workflows/cpp.yml`) no longer runs on pull requests that change only Rust or documentation. A `changes` job skips every C++ job unless the change touches `cpp/`, `test-vectors/`, `xtask/`, or the C++ workflow and setup action; pushes to `main` and manual runs build everything. A new `cpp-ok` job summarises the result and is the check to require in branch protection. C0 plan D17 amended.
- Verification: the merge gate is a local verification run instead of GitHub Actions (OI-31). New `docs/06-verification.md` §7: `cargo xtask verify` runs every gating check in pinned containers (VER-056) and writes an evidence record tied to the commit's tree (VER-057); a pull request merges on a published passing record and the commit status `aldis/verify` (VER-058); GitHub Actions runs every check nightly (VER-059). "In CI" in the specification now means the verification run. The pull-request workflows stay the gate until `cargo xtask verify` is implemented.
- `ci.yml` and `cpp.yml` run nightly and on manual dispatch only; `nightly.yml` adds a `verify` job on both Linux architectures; every workflow opens an issue when a scheduled run fails (VER-059). `cpp.yml` drops its `changes` and `cpp-ok` jobs, which existed only for pull requests.
- `docs/verification/analysis-records.md` names the checks of `cargo xtask verify` instead of CI jobs; VER-056, VER-057, VER-059, and VER-060 are recorded.
- Branch protection on `main` requires the commit status `aldis/verify` and branches that are up to date with `main` (VER-058); `docs/verification/analysis-records.md` records it.
- OI-27 resolved (maintainer): field identifiers get stable numeric field codes, catalogued in `docs/03-api-model.md` §9.1 (new API-082) and carried by the C ABI (CABI-021, CPP-021). The catalogue and the Rust `FieldId` code are added in M2 WP-0.
- OI-28 resolved (maintainer): CPP-001 reworded. Aldis builds and the installed package require exactly C++20, the headers reject older standards, and later standards are allowed but not verified.
- Milestone C0 started ([docs/milestones/c0-foundations.md](docs/milestones/c0-foundations.md)).
- `docs/profiles/cpp.md` §5: `modernize-use-trailing-return-type` disabled (style only), and a second table lists the checks tests and tools relax for GoogleTest.
- `AGENTS.md`: the checks to run when changing `cpp/`.
- `docs/profiles/cpp.md` §5: `readability-identifier-length` and `bugprone-easily-swappable-parameters` disabled for the whole C++ code base, each with its reason (CPP-041); `cpp/.clang-format` keeps `NOLINT` comments on one line.
- `docs/milestones/c0-foundations.md` §6 lists the API differences between `aldis_core` and `aldis-core`; no behaviour divergence was found.
- C0 plan D6: the native C++ exerciser runs on the QEMU machines M1 WP-5 settled on, naming the
  Cortex-R one: QEMU's `none` machine with `-cpu cortex-r5f` (M1 D6, PR #50).
- Milestone A0 plan approved (maintainer, 2026-10-19): decisions D1–D16 accepted; its exit table lists the APP requirements A0 verifies.
- Roadmap §3 links the A0 plan and the scenario, and names the in-process or out-of-process question for the A5 and A6 nodes as open (OI-24).
- OI-06, OI-11, and OI-23 to OI-26 cross-reference the scenario; OI-26 records the SANA registry check and the proposed spacecraft ID.
- `cargo xtask trace` reads `// req:` annotations in `refapp/` and the requirement IDs in golden-trace headers; `spec-lint` checks requirement IDs in `.jsonl` files (APP-108).
- OI-23…OI-26 resolved (maintainer), as proposed: golden traces are not conformance vectors (APP-087, APP-088); the A5 C++ and A6 QEMU flight nodes run as child processes over the lockstep node protocol of `docs/reference-application.md` §8.2, so `refapp/` needs no `unsafe` and GEN-071 is unchanged; M6 defines the FW-013 trace format to match the golden traces; spacecraft ID `0x155`, fictional (APP-026 reworded). Roadmap §3 and the M6 row updated.

### Fixed
- `aldis-spp`: a time secondary header cut off inside its time code now reports a `LengthMismatch`
  `expected` length that includes the Ancillary Data Field (a lower bound inside a P-field
  extension). SPP-008.
- Exerciser: public items of `aldis-time` and `aldis-epp` that were never called are now
  called (the `From` and `Default` impls of the bounded time fields, `Picos::get`, the
  per-format `pfield()`, `From<ReservedPField> for PField`, `EpiSet::from_bits`,
  `ExtendedEpiSet::from_bits`, `From<UserDefined> for u8`), so VER-010's link-time proof covers
  them.
- `cargo xtask verify` failed with `reference is not a tree` when HEAD was detached on a commit that no branch or tag reaches; the container now fetches the commit itself as well.
- Concurrent `cargo xtask verify` runs on one machine shared the C++ build directories in the cache volume, so a run could delete another's build output or pass checks against objects built from another commit. A run now holds a lock on the volume and waits for any run already in progress.

## [0.1.0-m0] — 2026-10-16

Milestone 0, Foundations: repository, CI, verification tooling, and a verified `aldis-core` ([plan](docs/milestones/m0-foundations.md)).

### Added
- `xtask` tooling (`cargo xtask help`): `spec-lint` (requirement tables and every ID reference), `trace [--scope m0] [--strict]` (requirement → artefact matrix), `vectors validate`, `vectors schema [--check]`, `links`, `layers`, and `ci`.
- `aldis-testkit`: test-vector model, loader, and exact-match runner; JSON Schemas for vector files generated from the model (`test-vectors/schema/`).
- Core vectors: `errors.json`, `bits.json`, `counters.json` (hand-derived, independent Python reference).
- `docs/verification/analysis-records.md` for requirements verified by analysis or inspection.
- RS-017: tooling lint profile.
- GitHub Actions CI: formatting, Clippy (`all` + `pedantic` + restriction lints), tests on Linux and macOS, `no_std` builds for Cortex-M, Cortex-R and RISC-V, big-endian tests (s390x), dependency layering check, and `cargo-deny`.
- Nightly workflow: big-endian bare-metal build and a CCSDS document URL check.
- `specs/fetch.sh --check-urls` (HEAD-only check; downloads nothing).
- Repository housekeeping: CODEOWNERS, pull-request template, Dependabot, this changelog.
- CI job `spec`: spec lint, links, vector validation, schema check, and M0 trace matrix (artefact `trace-m0`).
- `aldis-core` (M0 WP-4): `error` (stable catalogue), `bits` (big-endian bit fields), `ids` (width-checked identifiers), `time` (`Instant`, `Duration`), `crc` (FECF CRC-16, bitwise and table), `oid` (OID generator, Fibonacci and Galois forms), `counter` (modular counters, gap classification), `container` (`FixedQueue`, `ByteQueue`, `FixedVec`), `security` (SDLS reservation hooks), `layout` (frame field layout). 100 % region/function/line coverage.
- Core vector `layout.json`; all core vectors run against `aldis-core`.
- `clippy.toml` and RS-018: lint exemptions for test code.
- `exerciser/` (M0 WP-5): bare-metal binary that runs every core vector and calls every public `aldis-core` API. Its panic handler calls an undefined symbol, so a release build links only if no panic path survives (VER-010, GEN-021); an `inject-panic` canary proves the check fails. Runs under QEMU (Cortex-M4) with a semihosting exit code.
- `cargo xtask symbols` (VER-031), `cargo xtask stack-sizes <target>` (VER-011), `cargo xtask coverage` (VER-024: 100 % of regions, functions, lines, and branches for `aldis-core`; waivers in `coverage-waivers.toml`).
- `fuzz/` (M0 WP-6, VER-022): cargo-fuzz targets `bits_roundtrip`, `crc_variants`, `oid_forms`, `counter_classify`, `container_model`, `layout_ranges`.
- CI jobs `exerciser` (link check and canary on three targets, QEMU vector run on Cortex-M4) and `coverage`; `layers` also runs `cargo xtask symbols`; `fmt` and `clippy` cover the exerciser and fuzz workspaces. Nightly jobs: fuzzing (600 s per target; crashes open an issue), stack sizes, and mutation testing (report only).
- `tooling-nightly`: the one nightly date used by tooling (RS-002, OI-14).
- `docs/verification/mutants-triage.md`: first mutation-testing triage. Three test gaps in `aldis-core` closed (`FixedQueue::is_full`, `FixedVec::is_empty`, `SecurityLayout::is_none`); six equivalent mutants recorded.

### Changed
- The nightly big-endian bare-metal build uses the pinned `tooling-nightly` instead of a floating nightly.
- M0 exit scope: API-072, API-074, GEN-043, GEN-052, GEN-053, and TV-010 moved to the codec and engine milestones that implement them.
- CI `layers` job runs `cargo xtask layers` (replaces `.github/scripts/check_layers.py`) and also checks that tooling crates are only dev-dependencies.

### Fixed
- Removed an editor swap file committed by mistake (`crates/aldis-core/src/.layout.rs.swp`); editor files are now git-ignored.
- Requirement rows API-055 and EPP-008 contained unescaped `|` in code spans, which split the table cells (also mis-rendered on GitHub).
- GEN area now maps to both `01-architecture.md` and `02-design-rules.md`, where its requirements are defined.

## [0.1.0-spec] — 2026-10-16

### Added
- Specification draft 0.1 for v1 (Space Packet, Encapsulation Packet, TM, TC/CLCW, AOS, USLP, COP-1, time codes), with Rust and C ABI profiles, verification plan, and test-vector format.
- Cargo workspace skeleton (`aldis-*` crates), pinned Rust 1.99.1.
- Download manifest and fetch script for the CCSDS standards (not redistributed).
- Seed conformance vectors: CRC-16 check value, OID idle pattern.
