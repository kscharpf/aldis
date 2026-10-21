# Work-package checklists

A definition of done for the kinds of work that recur in every milestone. Each item names the requirement or check that enforces it. Items marked **(consistency)** are checked by `cargo xtask consistency`, part of `cargo xtask ci`; items marked **(ci)** by another command of `cargo xtask ci`; the rest by `cargo xtask verify` where a check is named, and otherwise by review. A milestone plan may add items for its own work packages; it does not repeat these.

Every pull request also: changes one track's directories ([roadmap](../08-roadmap.md) §6 rule 2); adds one fragment to [`changelog.d/`](../../changelog.d/README.md) and never edits `CHANGELOG.md` (GEN-080; `cargo xtask changelog check`, **(ci)**); updates the requirement table in the same change when behaviour changes, with new IDs taking the next free number (`cargo xtask spec-lint`, **(ci)**); updates only its own row of the plan's work-package table (§6 rule 1); and merges on a published, passing record of `cargo xtask verify` (VER-058).

## Adding a codec module to an existing crate

- [ ] Wire behaviour taken from the Blue Book clause cited, never from memory; open questions recorded as OIs ([09](../09-open-issues.md)), not settled in code.
- [ ] Public items cite their clause and requirement IDs (RS-015, `missing_docs`); range-restricted fields are their own types with checked constructors (RS-020).
- [ ] No panic, heap, I/O, or `unsafe` (GEN-010, GEN-020, GEN-030, GEN-070): Clippy with the workspace lint set (VER-001, verify check `clippy`).
- [ ] Unit tests with `// req: <ID>` on every test (VER-020; `cargo xtask trace`, **(ci)**); test modules marked `#[cfg_attr(coverage_nightly, coverage(off))]`.
- [ ] Fields the module's errors name are rows of the field catalogue ([03 §9.1](../03-api-model.md#91-field-codes)), regenerated with `cargo xtask fields --write` (API-082; `cargo xtask fields`, **(ci)**).
- [ ] Anomaly flags listed with their bit indices in the standard document (API-033, review).
- [ ] Vectors where the Verif. column says V (see below), a property-based round trip drawn from the shared strategies of `aldis-testkit` (VER-023, VER-054), and the fuzz target the plan names (below).
- [ ] 100 % regions, functions, lines, and branches (VER-024, verify check `coverage`).
- [ ] The exerciser calls every new public function with `black_box` inputs (VER-010, verify checks `exerciser-<target>`).
- [ ] The crate's GEN-022/023 inspection revised for every new loop and call: a `Revised` row "prepared by <author>, pending exit review" (VER-061, review).
- [ ] The crate's `README.md` status, and the plan's WP row.

## Adding or activating a crate

A crate *has code* once its `src/` declares a function. In the change that gives it code:

- [ ] Layer assigned in `LAYERS` of `xtask/src/layers.rs` and in [01-architecture.md](../01-architecture.md) §2; dependencies only on lower layers (GEN-001…004, `cargo xtask layers`, **(ci)**).
- [ ] In the coverage gate, `GATED` in `xtask/src/coverage.rs` (VER-024) **(consistency)**.
- [ ] In the `features` check, `CRATES` in `xtask/src/features.rs`, with `alloc` and `std` gating only the crate-root `extern crate` lines (GEN-006, verify check `features`) **(consistency)**.
- [ ] In the `-p` list of the nightly `mutants` job, `.github/workflows/nightly.yml` (VER-025) **(consistency)**; survivors triaged in [mutants-triage.md](../verification/mutants-triage.md) before the release.
- [ ] An exerciser dependency in `exerciser/Cargo.toml`, a module calling its API, and a vector group in `exerciser/build.rs` and `Group` (`exerciser/src/vectors.rs`) (VER-010) **(consistency)** for the dependency and the group.
- [ ] A `README.md` in the crate, linked from the root `README.md` **(consistency)**, and the crate named in the root `README.md` status.
- [ ] A facade feature in `crates/aldis/Cargo.toml` that enables it, its `alloc` and `std` forwarded, and the feature in `full` **(consistency)**; the facade docs list it (RS-015).
- [ ] Fuzz crate dependency in `fuzz/Cargo.toml` when it gets a target (below).
- [ ] A first GEN-022/023 inspection record in [`docs/verification/inspections/`](../verification/inspections/), linked from [`records/GEN-022.md`](../verification/records/GEN-022.md) (VER-061).
- [ ] The records of VER-010, VER-024, VER-025, and GEN-006 in [`docs/verification/records/`](../verification/records/) name the crate.

## Adding a vector file

- [ ] Format of [07](../07-test-vectors.md): every case lists its requirements and operation (TV-001, TV-003); encode outputs as exact hex and every field the view exposes (TV-002); error expectations with the field code where the error names a field (M2 plan D3) (`cargo xtask vectors validate`, **(ci)**).
- [ ] Provenance and a `provenance_note` (TV-020); a hand-derived file's independent model, when it is a program, committed under [`test-vectors/models/`](../../test-vectors/models/README.md) and named in the note, with `--check` (TV-023; `cargo xtask vectors validate` runs it, **(ci)**).
- [ ] Blue Book examples first where they exist; a worked derivation in `docs/verification/derivations/` for at least one case of a hand-derived file; a second review of hand-derived vectors before the release (M1 §5, M2 WP-6).
- [ ] A row in [`test-vectors/README.md`](../../test-vectors/README.md) **(consistency)**.
- [ ] Run by a host test in `crates/<crate>/tests/` through the `aldis-testkit` exact-match runner and a handler (VER-021, TV-012) **(consistency)** for a test naming the file.
- [ ] Embedded and run by the exerciser when its crate is exercised (VER-010, M1 D6) **(consistency)**.
- [ ] For a codec that has C ABI functions, run through the C ABI by the `aldis-ffi` vector runner, `crates/aldis-ffi/tests/vectors.rs` (M2 plan D4) **(consistency)**: every file of `test-vectors/{time,spp,epp}/` (`FFI_AREAS` in `xtask/src/consistency.rs`), and every file of a directory the runner names at least one file of.
- [ ] Never edit a released case: add a new case and mark the old one `superseded` (TV-021, `cargo xtask vectors validate`).

## Adding a fuzz target

- [ ] A `[[bin]]` in `fuzz/Cargo.toml` and the target in `fuzz/fuzz_targets/`, asserting no panic, `decode(encode(x)) == x` for accepted inputs, and no state change on rejected inputs (VER-022, GEN-042); against an independent model where one exists.
- [ ] The target in the nightly `fuzz` matrix of `.github/workflows/nightly.yml` **(consistency)**.
- [ ] The fuzz crate still builds on stable: `cargo clippy --manifest-path fuzz/Cargo.toml --all-targets` (RS-002).
- [ ] The target and what it asserts added to [`records/VER-022.md`](../verification/records/VER-022.md).
- [ ] At least one CPU-hour per target before the release (VER-022; `nightly.yml` with `fuzz-seconds`).

## Adding C ABI functions

- [ ] Functions in the codec's own module of `crates/aldis-ffi` (M2 plan §3), named `aldis_<std>_…` ([C ABI profile](../profiles/c-abi.md)), in the shape of M2 plan D4 and D14; every pointer and length checked (CABI-022); every `unsafe` block with a `// SAFETY:` comment (GEN-070).
- [ ] The generated header `include/aldis/aldis.h` regenerated with `cargo xtask header --write`, never merged by hand (CABI-002; verify check `header`).
- [ ] Capacities of stateful helpers in `crates/aldis-ffi/capacities.toml` and reported by `aldis_limits` (CABI-030).
- [ ] The functions called by the exerciser's `aldis-ffi` group, `exerciser/src/ffi.rs` (M2 plan D16, VER-010; the dependency, the `ffi` group run by `exerciser/src/exercise.rs`, and `REPORTED` in `xtask/src/coverage.rs` **(consistency)**) and compared with the Rust decoders by the `ffi_decode` fuzz target (M2 WP-7).
- [ ] Every vector file of the codec run through the C ABI by the `aldis-ffi` vector runner (D4); add the codec's directory to `FFI_AREAS` in `xtask/src/consistency.rs` **(consistency)**.
- [ ] The C++ owner told when the header changes (roadmap §6 rule 4).

## A spec-readings pull request before a codec

Before a codec's work package starts, one pull request settles the Blue Book questions it raises (as `m2/frame-spec`, PR #61, did for the frame codecs):

- [ ] Each reading cites the clause it rests on; no text copied from the Blue Book (AGENTS.md).
- [ ] Requirement tables and the standard document changed in the same pull request; new requirements take the next free ID (`cargo xtask spec-lint`, **(ci)**); new requirements in scope added to the plan's exit table, so that `cargo xtask trace --scope <id>` measures them.
- [ ] OIs answered, updated, or opened in [09](../09-open-issues.md), and what is left to the maintainer named.
- [ ] Anomaly lists and error kinds settled before the codec ships them (API-032, API-033).
- [ ] No code: the codec follows in its own pull request.

## Milestone exit

- [ ] Every exit criterion of the plan's §4 met on `main`, with the evidence in a results column; `cargo xtask trace --scope <id> --strict` passes and replaces the non-strict run in `cargo xtask ci`, `ci.yml`, and the verify check `spec` (VER-020).
- [ ] **The GEN-022/023 inspections of every crate the milestone changed approved by the maintainer at the exit review**, covering every revision since the last approval; the approval recorded in each record's `Approved` row (VER-061).
- [ ] Coverage at 100 % for the gated crates (VER-024); mutants triaged (VER-025); every fuzz target run one CPU-hour (VER-022); stack sizes published (VER-011); the exerciser run on all bare-metal targets (VER-010).
- [ ] Cross-checks run, or the search recorded in [cross-checks.md](../verification/cross-checks.md) (VER-040); the second review of the hand-derived vectors recorded in `docs/verification/reviews/`.
- [ ] Crate docs cite clauses and requirement IDs (RS-015, reviewed); facade features documented.
- [ ] **The process metrics for the milestone reviewed** (`cargo xtask metrics --since <the milestone's start>`, or the pinned issue "Process metrics", [06 §9](../06-verification.md#9-process-metrics)), and the top friction item, such as the file that conflicts most or the check group that costs most, opened as a follow-up issue (VER-063, review).
- [ ] Open issues updated; the roadmap row, the root `README.md` status, and the plan's status updated.
- [ ] For a track-R milestone: `cargo xtask changelog release <version> <date>`, the workspace version, and the release tag; vectors are immutable from then on (TV-021). Track C and A milestones cut no release (roadmap §5).
