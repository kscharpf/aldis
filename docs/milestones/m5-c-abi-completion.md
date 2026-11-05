# Milestone 5 — C ABI completion

**Goal:** the last milestone of track R for v1. A new Layer 1 crate for CADUs and the TM pseudo-randomizer (ASM attach and search, 131.0 §9–10; OI-12), with its C ABI; a C test harness run under AddressSanitizer and UBSan, and the C ABI under Miri (VER-026); header compile checks across C and C++ standards and compilers (VER-050); the header-only C++20 wrapper `aldis.hpp` (CABI-072); and every v1 requirement of track R traced. Verified to the project standard: 100 % coverage, bare-metal panic-freedom, fuzzing, mutation testing, worst-case stack measurement, and hazard analysis.
**Roadmap entry:** [08-roadmap.md](../08-roadmap.md) §1 · **Builds on:** [M4](m4-link-engines.md) (`v0.5.0-m4`) · **Release:** `v0.6.0-m5`; v1.0 follows when M5 and A6 have exited (roadmap §5)

**Status: plan drafted; decisions D1–D8 (§2) await the maintainer.** Entry gate: M4 (roadmap §1), met by `v0.5.0-m4`.

| WP | State | PRs | Notes |
|---|---|---|---|
| WP-0 Plan | in progress | | This plan, D1–D8, `--scope m5` trace gate, field-code block 250–299 |
| WP-1 Spec readings: sync | planned | | Area SYNC from 131.0 §9–10, clause by clause; field codes. No code |
| WP-2 Blind model & vectors: sync | planned | | Python model written blind (TV-023); vectors for ASM attach, search, and the randomizer |
| WP-3 `aldis-sync` crate | planned | | ASM attach, randomizer, ASM search engine |
| WP-4 `aldis-sync` C ABI | planned | | Codec functions and the search handle |
| WP-5 Sanitizers & Miri | planned | | C harness under ASan/UBSan; `aldis-ffi` tests under Miri (VER-026) |
| WP-6 Header & C checks | planned | | C99/C11/C++11/C++17 on GCC and Clang (VER-050); CERT C on the C code (VER-051) |
| WP-7a `aldis.hpp`: codecs | planned | | Header-only C++20 wrapper of the codec functions (CABI-072) |
| WP-7b `aldis.hpp`: engines | planned | | RAII handles; vectors pass via `aldis.hpp` |
| WP-8 Open v1 requirements & OI-79 | planned | | The untraced requirements of §1 that no other WP closes; OI-79 decided |
| WP-9 Gates & hazards | planned | | |
| WP-10 Exit | planned | | `trace --scope m5 --strict`; the full v1 trace of track R |

A pull request that starts or merges a work package changes only its row (roadmap §6 rule 1).

History:
- 2026-11-05: plan drafted following the M4 exit; OI-12 resolved into M5 by the maintainer the same day.

## 1. Starting point

| In place after M4 | Missing for M5 |
|---|---|
| Frame codecs and link engines of TM, TC, AOS, USLP, each with its C ABI functions or handle; `ALDIS_ABI_VERSION_MINOR` 11 | A sync layer: CADUs (ASM attach and search) and the TM pseudo-randomizer (131.0 §9–10; OI-12) |
| `cargo xtask header c-smoke`: the libraries, and the header compiled as C99, C23, and C++11 (`crates/aldis-ffi/tests/c/smoke.c`, the `header` check) | C11 and C++17, GCC and Clang, `-Wall -Wextra -Wpedantic -Werror` (VER-050); a C test harness that runs the vectors; sanitizers and Miri (VER-026); CERT C analysis of the C code (VER-051) |
| `ALDIS_ABI_VERSION_*` and `aldis_abi_version()` (CABI-060, traced) | The version policy at v1.0 (D6) |
| CABI-072 (C++20, OI-19, OI-20); `docs/profiles/cpp.md` is the native C++ profile, withdrawn to Phase 3 | `aldis.hpp` and its tests |
| `cargo xtask trace`: v1 requirements of every area outside `DEFERRED_AREAS` | Untraced live requirements of track R: API-054, API-059, API-080, CABI-013, CABI-050, CABI-072, CMN-024, GEN-033, GEN-080, RS-013, USLP-011, VER-012, VER-026, VER-041, VER-050, VER-051; the APP rows are track A's (A6). Withdrawn rows are listed as untraced too (D7) |
| OI-79 (link edge cases no vector pins), target M5 | Each case read or recorded as undefined, with a vector or a unit test |

## 2. Decisions

| # | Decision | Choice (recommended) | Status |
|---|---|---|---|
| D1 | Trace scope | Add `--scope m5` from §4 to `cargo xtask ci`, the `spec` check, and `ci.yml`, without `--strict`, in this plan's PR; `--strict` at the M5 exit (as M4 D1) | Proposed |
| D2 | Sync crate | `aldis-sync`, Layer 1, depending on `aldis-core` only; requirement area **SYNC** in a new `docs/standards/tm-sync.md`; field-code block **250–299** reserved for it ([03](../03-api-model.md) §9.1) | Proposed |
| D3 | Sync scope | ASM attach and the pseudo-randomizer as stateless codec functions over a caller's buffer; ASM search as a sans-I/O engine over octet chunks that finds the ASM at any bit offset with a configured number of tolerated bit errors, its states a closed enum (RS-021). The values (ASM patterns, the randomizer polynomial and seed) come from 131.0, read in WP-1, never from memory. No frame-sync flywheel or lock thresholds beyond what WP-1 finds in 131.0; anything else becomes an OI | Proposed |
| D4 | Where Miri and the sanitizers run | Two checks of `cargo xtask verify`, `ffi-miri` and `c-sanitizers`, in the gate when `aldis-ffi` or `include/` change (VER-056 inputs); if Miri over the `aldis-ffi` tests exceeds about 10 min, it runs nightly-only as `kani` does (VER-059) and the plan records that | Proposed |
| D5 | `aldis.hpp` | Hand-written (not generated) at `include/aldis/aldis.hpp`, C++20, no exceptions, on GCC and Clang; its tests run the shared vectors through it in `crates/aldis-ffi/tests/cpp/`; compiled and run by the `header` check | Proposed |
| D6 | ABI version at v1.0 | `ALDIS_ABI_VERSION_MAJOR` stays 0 through `v0.6.0-m5` (incompatible changes bump the minor, CABI-033); it becomes 1.0.0 with the v1.0 release, after which CABI-060 and GEN-080 apply in full | Proposed |
| D7 | Withdrawn rows in the trace | `cargo xtask trace` leaves rows that start `Withdrawn:` out of the count and the untraced list (`spec.rs` already recognises them); a small `xtask` PR of WP-0 | Proposed |
| D8 | Process freeze | None (as M4 D5): a process fix is its own pull request | Proposed |

## 3. Work packages

Size: **S** ≈ small, **M** ≈ medium, **L** ≈ large.

```
WP-0 Plan ─┬─▶ WP-1 Spec: sync ─▶ WP-2 Blind: sync ─▶ WP-3 aldis-sync ─▶ WP-4 sync C ABI ─┐
           ├─▶ WP-5 Sanitizers & Miri ──────────────────────────────────────────────────────┤
           ├─▶ WP-6 Header & C checks ─▶ WP-7a aldis.hpp codecs ─▶ WP-7b aldis.hpp engines ─┼─▶ WP-9 Gates ─▶ WP-10 Exit
           └─▶ WP-8 Open v1 requirements & OI-79 ───────────────────────────────────────────┘
```

The sync chain and the C/C++ chain are independent and run in parallel. If WP-4 merges after WP-7b, the sync handle is wrapped in `aldis.hpp` by WP-4.

### WP-0 Plan (S)
- This plan, D1–D8, the `--scope m5` trace gate (D1), the block 250–299 row (D2), the roadmap row linking the plan; D7 as its own `xtask` pull request.

### WP-1 Spec readings: sync (M) — SYNC area, API-082
- `docs/standards/tm-sync.md`: the SYNC requirements, each citing its clause of 131.0 §9 (ASM, CADU) and §10 (pseudo-randomizer), and the frame protocols they apply to (TM, AOS, fixed-length USLP).
- The search engine's action bound and output-area bound (API-057, API-062); field codes from block 250–299.
- Checklist: "A spec-readings pull request before a codec". No code.

### WP-2 Blind model & vectors: sync (M) — SYNC area, TV-023
- Python model written blind from the Blue Book and `docs/` only, no implementation path given to its author: `test-vectors/models/sync.py`.
- Vectors in `test-vectors/sync/`: CADUs per protocol, the randomizer sequence over a full period, ASM search at every bit offset, with errors up to and past the tolerance, and an ASM split across chunks.
- Checklist: "Adding a vector file".

### WP-3 `aldis-sync` crate (L) — SYNC area, RS-021, VER-023
- ASM attach and the randomizer (codec functions); the ASM search engine (D3).
- Round-trip property test: attach, randomize, derandomize, search, at every bit offset (VER-023).
- Checklists: "Adding or activating a crate", "Adding a codec module", "Adding an engine", "Adding a fuzz target".

### WP-4 `aldis-sync` C ABI (M) — CABI-030…032, CABI-040
- Codec functions and the search handle, a `[sync]` entry in `capacities.toml`, the exerciser group, the generated header; minor version raised.
- Checklist: "Adding C ABI functions".

### WP-5 Sanitizers & Miri (M) — VER-026, GEN-071
- A C test harness in `crates/aldis-ffi/tests/c/` that runs the shared vectors through the C ABI, built with `-fsanitize=address,undefined` on Clang and GCC.
- The `aldis-ffi` Rust tests under Miri; the checks of D4 with their inputs declared.

### WP-6 Header & C checks (M) — VER-050, VER-051, VER-055
- `header c-smoke` extended to C99, C11, C++11, and C++17 on GCC and Clang with `-Wall -Wextra -Wpedantic -Werror`.
- CERT C analysis of the C harness and examples with a free analyser (VER-055), deviations in a deviation log; the record of VER-051 in `docs/verification/records/`.

### WP-7a `aldis.hpp`: codecs (M) — CABI-072
- `std::span` views and `expected`-like results over the codec functions of time, SPP, EPP, TM, TC, AOS, and USLP; no exceptions (D5).
- The codec vectors pass via `aldis.hpp`.

### WP-7b `aldis.hpp`: engines (L) — CABI-072, CABI-030, CABI-031
- RAII handle types with storage in the typed storage structs (CABI-031); capacities as template parameters with `static_assert` against `ALDIS_*_MAX_*` (CABI-030, OI-20).
- The engine-scenario vectors (COP-1, link) pass via `aldis.hpp`.

### WP-8 Open v1 requirements & OI-79 (M) — API-054, API-059, API-080, CABI-013, CABI-050, CMN-024, GEN-033, GEN-080, RS-013, USLP-011, VER-012, VER-041
- Each requirement traced by a test or a record (method I or A, [analysis-records.md](../verification/analysis-records.md)): engines `Send` and without `Sync`-dependent interior mutability (GEN-033, RS-013, CABI-050), `uint64_t` microseconds (CABI-013), the worst-case paths of each step function (VER-012), the two-person COP-1 table review record (VER-041), the versioning policy (GEN-080, with D6).
- A requirement that turns out to belong to v1.1 is moved there by the maintainer, not left untraced.
- OI-79: each case decided with a clause or recorded as undefined, and pinned by a vector or a unit test; may split into one PR per side (sending, receiving).

### WP-9 Gates & hazards (M) — VER-010, VER-011, VER-022…025, VER-065, VER-066
- 100 % coverage, mutants triaged, fuzz targets, stack bounds at the `capacities.toml` sizes, and QEMU on three targets, for `aldis-sync` and its handle.
- Hazard analysis `docs/verification/hazards/sync.md` (false lock, missed ASM, randomizer misalignment) and the C/C++ boundary (wrong storage size or alignment through `aldis.hpp`); the assurance case.

### WP-10 Exit (S) — VER-020, RS-015
- `cargo xtask trace --scope m5 --strict` replaces the non-strict run; the full trace has no untraced track-R requirement of v1.
- Rustdoc citations (RS-015); roadmap, README, glossary (PN row), and open issues.
- The releases `v0.6.0-m5` and, with A6, v1.0 are the maintainer's.

## 4. Exit criteria

Milestone 5 is complete when every item below is green on `main`. The results column is filled at the WP-10 exit pull request.

| Area | Criteria | Results / evidence |
|---|---|---|
| Sync crate | The SYNC requirements of WP-1, RS-021 | |
| Sync C ABI | CABI-030…032, CABI-040, API-082 | |
| Sanitizers & Miri | VER-026, GEN-071 | |
| Header & C code | VER-050, VER-051, VER-055 | |
| C++ wrapper | CABI-072 | |
| ABI versioning | CABI-060, GEN-080 | |
| Concurrency | GEN-033, RS-013, CABI-050 | |
| Remaining API | API-054, API-059, API-080, CABI-013, CMN-024, USLP-011 | |
| COP-1 review & WCET | VER-041, VER-012 | |
| Panic freedom & resources | VER-010, VER-011 | |
| Verification gates | VER-022…025 | |
| Vectors and trace | TV-023, VER-020, VER-023 | |
| Hazards & assurance case | VER-065, VER-066 | |

The roadmap exit: all CABI requirements traced; vectors pass via the C ABI and via `aldis.hpp`.

Not in M5: CLTU (BCH) and the TC randomizer (231.0, v1.1, OI-12); the framework (M6, v1.1).

## 5. Risks

| Risk | Mitigation |
|---|---|
| ASM search details (bit-level alignment, error tolerance, lock) that 131.0 leaves to the implementation | WP-1 separates what the clause says from what it leaves open; the open part becomes an OI before WP-3 (D3) |
| Miri too slow, or unable to run part of `aldis-ffi` | D4 fallback to nightly-only; a test Miri cannot run is listed with its reason |
| `aldis.hpp` storage size or alignment drifts from the header | Sizes come from the typed storage structs, never restated; `static_assert` on capacities (CABI-030); WP-9 hazard |
| The untraced list hides requirements that need design, not a record | WP-8 starts early and in parallel; one that needs design goes to the maintainer as one question |
| v1.0 waits on track A (A6) | A5 needs `aldis.hpp` (WP-7b), so WP-7 is not left to the end |

## 6. Hand-off to the other tracks

| Track | Needs from M5 | When |
|---|---|---|
| A — A5 (C++ flight side) | `aldis.hpp` over the C ABI | WP-7b merged |
| A — A6 (target) | CADUs and the randomizer for the end-to-end demo, if the scenario uses them | WP-4 merged |
| ACFS | The sync handle and `aldis.hpp` | M5 exit |
| R — M6 (v1.1) | A released v1.0 C ABI to build the framework adapters on | v1.0 |
