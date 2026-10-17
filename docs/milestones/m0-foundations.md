# Milestone 0 — Foundations

**Goal:** a working repository, CI, and verification tooling, plus a complete, verified `aldis-core`. Every later milestone builds on this unchanged.
**Roadmap entry:** [08-roadmap.md](../08-roadmap.md) §1 · **CI host:** GitHub Actions

## 1. Starting point

| Already in place | Not yet started |
|---|---|
| Cargo workspace with all crate skeletons; facade; FFI crate | Branch protection, CODEOWNERS, Dependabot |
| Pinned toolchain 1.99.1 + bare-metal targets (`rust-toolchain.toml`) | CI workflows |
| Lint set: Clippy `all` + `pedantic` + restriction lints, denied | Panic-freedom link check (VER-010) |
| `AGENTS.md`, `README.md`, `LICENSE` (MIT), `.gitignore` | Vector harness, JSON schemas |
| `specs/` download manifest and `fetch.sh` (PDFs not in the repo) | Trace matrix and spec-lint tooling |
| GitHub repository <https://github.com/kscharpf/aldis>, initial commit on `main` | |
| Seed vectors: `core/crc16.json`, `core/oid.json` (verified) | All `aldis-core` code |

## 2. Decisions

| # | Decision | Choice | Status |
|---|---|---|---|
| D1 | CI host | GitHub Actions | **Decided** |
| D2 | Repository | <https://github.com/kscharpf/aldis>; its root is the local `code/` folder | **Decided** |
| D3 | Project tooling | An `xtask` crate (Rust, `std`, dev-only, excluded from Layer 0–2 rules) for vectors, traceability, spec checks, and the panic check | Proposed |
| D4 | Bare-metal execution | Cortex-M (`thumbv7em-none-eabihf`): exerciser **runs** under QEMU (`qemu-system-arm`, semihosting). Cortex-R and RISC-V: build + panic-link check only in M0; QEMU execution added in M1 | Proposed |
| D5 | Big-endian testing (VER-030) | Host test suite on `s390x-unknown-linux-gnu` via `cross` (QEMU user mode). `armebv7r-none-eabihf` has no prebuilt `core` for 1.99.1 (checked), so a big-endian **bare-metal** build runs only in `nightly.yml` with `-Z build-std` | Proposed |
| D6 | Vector parsing | `serde` + `serde_json` as **dev-dependencies** of a test-support crate only (GEN-001 permits dev-dependencies). Bare-metal runs use vectors compiled into Rust arrays by `xtask` | Proposed |
| D7 | CCSDS PDFs in CI | Never downloaded in CI. Tests rely only on vectors (see [specs/README.md](../../specs/README.md)) | **Decided** |

## 3. Work packages

Size is relative effort: **S** ≈ small, **M** ≈ medium, **L** ≈ large. WP-4 items run in the order listed. WP-1 to WP-3 can run in parallel with WP-4 after WP-0.

```
WP-0 repo ──▶ WP-1 CI ──────────────▶ WP-6 coverage & fuzz gates ──▶ exit review
          └─▶ WP-2 xtask ─▶ WP-3 vector harness ─▶ WP-5 exerciser ─┘
          └─▶ WP-4 aldis-core: error → bits → ids → time → crc → oid → counter → container → security → layout
```

### WP-0 Repository bootstrap (S)

| Task | Output |
|---|---|
| ~~Create the GitHub repository; push `code/` as its root~~ (done) | Remote with `main` |
| Branch protection on `main`: PRs required, CI required, linear history | Settings |
| Add `CHANGELOG.md` (Keep a Changelog), `.github/CODEOWNERS`, PR template with "requirements touched" and "vectors added" checkboxes | Files |
| Dependabot for GitHub Actions (pinned by SHA) and Cargo dev-dependencies | `.github/dependabot.yml` |
| ~~Fill `repository` in `Cargo.toml`~~ (done) | Commit |

**Done when:** a PR to `main` cannot merge without passing CI.

### WP-1 CI workflows (M) — VER-001…004, VER-030, VER-031, GEN-005

`.github/workflows/ci.yml`, triggered on PRs and pushes to `main`. All actions are pinned by commit SHA. Rust comes from `rust-toolchain.toml` via `rustup toolchain install` (no arguments: installs the pinned toolchain, components, and targets), never from a floating version.

| Job | Runner | Steps | Requirements |
|---|---|---|---|
| `fmt` | ubuntu | `cargo fmt --all --check` | VER-004 |
| `clippy` | ubuntu | `cargo clippy --workspace --all-features --all-targets` with `RUSTFLAGS=-Dwarnings` | VER-001, VER-002 |
| `test` | ubuntu, macOS (arm64) | `cargo test --workspace --all-features` | VER-021 |
| `no-std` | ubuntu | matrix `thumbv7em-none-eabihf`, `armv7r-none-eabihf`, `riscv32imac-unknown-none-elf`: `cargo build -p aldis --no-default-features --features full --target $T` | GEN-005 |
| `big-endian` | ubuntu | `cross test --target s390x-unknown-linux-gnu --workspace` | VER-030, GEN-007 |
| `minimal-build` | ubuntu | build `aldis-spp` alone; `xtask symbols` asserts no frame-protocol symbols | GEN-003, VER-031 |
| `deny` | ubuntu | `cargo deny check` (licenses: MIT-compatible only; advisories; sources) + `cargo audit` | VER-003, GEN-001 |
| `spec` | ubuntu | `xtask spec-lint`, `xtask links`, `xtask vectors validate` | TV-001…003 |
| `exerciser` | ubuntu | WP-5 panic-link check (all bare-metal targets); QEMU run (Cortex-M) | VER-010, GEN-021 |
| `coverage` | ubuntu | WP-6 | VER-024 |

A separate `nightly.yml` runs on a schedule: fuzzing (WP-6), the big-endian bare-metal build (`cargo +nightly build -Z build-std=core --target armebv7r-none-eabihf`), mutation testing (`cargo-mutants`, VER-025, report only in M0), and a `specs/fetch.sh --verify-only` dry check against the live CCSDS site. That last job only alerts when URLs or checksums go stale. It never commits PDFs.

**Done when:** every job above passes on the skeleton, and a deliberate lint violation, panic, or formatting error each fail the right job (checked once, then reverted).

### WP-2 `xtask` tooling (M) — VER-020, TV-003

| Command | Function |
|---|---|
| `xtask spec-lint` | Parses `docs/**/*.md` requirement tables. Requirement IDs are unique, every requirement has Source and Verif. columns, and IDs referenced in text exist. Emits `target/spec/requirements.json` |
| `xtask trace` | Joins requirements with `// req: ID` annotations in tests, vector `requirements` fields, fuzz targets, and analysis records into `target/spec/trace.md` and `trace.json`. Fails on any requirement in scope for the current milestone that has no verification artefact |
| `xtask vectors validate` | Validates `test-vectors/**/*.json` against `test-vectors/schema/`, checks unique case IDs, and checks that provenance is present |
| `xtask vectors codegen` | Generates `no_std` Rust arrays from vectors for the exerciser (D6) |
| `xtask links` | Checks relative Markdown links. `specs/pdf/` is treated as external |
| `xtask symbols` | Symbol inspection for VER-031 |

**Milestone scoping:** requirements carry no milestone field, so `xtask trace --scope m0` takes an explicit list: the GEN/API/CMN/VER/TV IDs in §4.

**Done when:** `spec-lint` passes on the current docs, and `trace --scope m0` lists every in-scope ID (initially all unverified).

### WP-3 Vector harness (M) — TV-001…012, VER-021

| Task | Output |
|---|---|
| JSON Schemas: `codec.schema.json`, `engine-scenario.schema.json`, common definitions (hex strings, error kinds, provenance) | `test-vectors/schema/` |
| Test-support crate `crates/aldis-testkit` (`publish = false`, `std`, dev-only): loads vectors, dispatches `operation` to handlers registered by each crate's tests, asserts **no unexpected outputs** (TV-012) | crate |
| Convert the two seed vectors to the final schema; add `core/counters.json`, `core/bits.json`, `core/layout.json`, `core/errors.json` (code ↔ kind table) | vectors |

**Done when:** `cargo test` runs every core vector through `aldis-testkit`, and a corrupted vector fails the run.

### WP-4 `aldis-core` implementation (L)

Each module is done when its requirements are covered by tests annotated `// req:` and by vectors where marked V. It must also be fuzzed where marked F, reach 100 % line and branch coverage, and pass the full lint set without new exceptions (RS-016).

| Order | Module | Content | Requirements | Notes |
|---|---|---|---|---|
| 1 | `error` | `ErrorKind` (codes 1–17, stable), `FieldId`, `ErrorDetail`, `Error` (`Copy`, `core::error::Error`) | GEN-040, GEN-041, GEN-081, RS-011, API catalogue §9 | Vector `core/errors.json` fixes the numeric codes |
| 2 | `bits` | Checked big-endian bit-field read/write over `&[u8]`/`&mut [u8]`: `get(bit_offset, width)`, `set(…)`, octet helpers; fields up to 64 bits | GEN-050, GEN-051, GEN-052, GEN-020, GEN-024 | The **only** place `as`/shift arithmetic is allowed (VER-001 exception); fuzzed get/set round trip |
| 3 | `ids` | `Apid`, `SpacecraftId<W>` (8/10/16-bit variants), `VcId<W>` (3/6-bit), `MapId<W>` (4/6-bit), `PacketVersion`, `SeqCount`, `FrameSeqNum` | API-001, API-002, RS-010 | `const fn new(..) -> Option<Self>`; no unchecked constructors |
| 4 | `time` | `Instant(u64 µs)`, `Duration`, saturating arithmetic, "backwards = zero elapsed" | API-003 | |
| 5 | `crc` | CRC-16 bitwise (default) and table (`crc-table` feature); incremental API; FECF verify both ways | CMN-001…004 | Vectors pass for both variants; property test: both equal on random input |
| 6 | `oid` | `OidGenerator` Fibonacci (reference) and Galois forms; `fill(&mut [u8])` | CMN-020…023 | Property test: forms identical over 10⁶ octets; cross-fill continuity |
| 7 | `counter` | `ModCounter<BITS>` 1–64 bits; `increment`, `distance`, `classify → InSequence / Gap{missing} / Repeat / Backwards` | CMN-030, CMN-031, GEN-024 | Exhaustive tests for 8-bit; property tests for wider |
| 8 | `container` | `FixedQueue<T, N>`, `ByteQueue<N>` (variable-length records in one byte ring), `FixedVec<T, N>` | GEN-010, GEN-011, GEN-012, API-051, API-061 | No `alloc`; `WouldBlock` on full; fuzz push/pop against a model |
| 9 | `security` | `SecurityLayout` (header ≤ 64), `SecurityHook` trait, `NoSecurity`, `FixedPatternSecurity` (test feature) | API-070…074 | Trait shape reviewed against 355.0 §2.3 / §4.1 |
| 10 | `layout` | `FrameLayout`: ordered, non-overlapping ranges for all frame fields per protocol; rejects empty data field | CMN-040, CMN-041 | Property test: ranges contiguous and cover the frame |

Also in WP-4: `aldis-core` crate docs. Every public item cites its clauses and requirement IDs (RS-015).

### WP-5 Exerciser and panic-freedom check (M) — VER-010, GEN-021, VER-011

| Task | Output |
|---|---|
| `exerciser/` (binary crate, `no_std`, `no_main`, workspace member excluded from default build): calls every public `aldis-core` API with the codegen'd vectors | crate |
| Link-time panic proof: a `#[panic_handler]` calls `extern "C" { fn aldis_panic_is_unreachable() -> !; }`, which is never defined. A release build with `panic = "abort"` and LTO links only if no panic path survives | CI job `exerciser` |
| Cortex-M run: `cortex-m-rt` + semihosting under `qemu-system-arm -machine mps2-an500`; exit code reports vector pass/fail | CI job |
| Stack usage: record `-Z emit-stack-sizes` output (nightly tooling, RS-002) for `aldis-core` functions; publish as a CI artefact | VER-011 report |

**Done when:** the exerciser links on all three bare-metal targets. Introducing an `unwrap()` into `aldis-core` makes the link fail (verified once).

### WP-6 Coverage and fuzzing gates (S) — VER-022, VER-024

| Task | Output |
|---|---|
| `cargo llvm-cov` job: 100 % line and branch coverage required for `aldis-core`; waivers in `coverage-waivers.toml` with justification | CI gate |
| `fuzz/` (cargo-fuzz): targets `bits_roundtrip`, `crc_variants`, `oid_forms`, `counter_classify`, `container_model`, `layout_ranges` | targets |
| `nightly.yml`: each target ≥ 10 CPU-minutes per night (≥ 1 CPU-hour before a release, VER-022); crashes open an issue automatically and become regression vectors | workflow |

## 4. Exit criteria

M0 is complete when every item below is green on `main`:

| Area | Criteria |
|---|---|
| Static | VER-001, VER-002, VER-003, VER-004 |
| Panic freedom | VER-010, GEN-020, GEN-021 (exerciser links on all bare-metal targets) |
| Portability | GEN-005, GEN-007, VER-030 (big-endian), VER-031 / GEN-003 (minimal build) |
| `aldis-core` requirements | GEN-010…012, GEN-022…024, GEN-040…043, GEN-050…053, API-001…003, API-070…074, CMN-001…004, CMN-020…023, CMN-030…031, CMN-040…041, RS-010, RS-011, RS-015, RS-016 |
| Vectors and trace | TV-001…003, TV-010, TV-012, TV-020…022, VER-020 (`xtask trace --scope m0` has no gaps), VER-021 |
| Gates | VER-022 (fuzz targets exist and run nightly), VER-024 (100 % coverage for `aldis-core`) |
| Docs | Roadmap updated; `CHANGELOG.md` entry for 0.1.0-m0; open issues updated |

Deferred from M0 on purpose: VER-012 (WCET documentation, first needed by engines in M3/M4), VER-025 gating (report only), VER-026 and VER-050…052 (FFI and C side, M5), and VER-040 cross-checks for frame codecs (M1/M2).

## 5. Risks

| Risk | Mitigation |
|---|---|
| The panic-link check fails because of panics inside `core` that Clippy can't see (e.g. slice bounds in iterator adaptors) | Prefer `get`/`split_at_checked`/`chunks_exact` APIs; investigate failures with `cargo asm`/symbol maps; WP-5 starts early, alongside WP-4 module 2 |
| 100 % branch coverage is hard for defensive branches | Design APIs so impossible states are unrepresentable; justify the remainder in the waiver file (reviewed per release) |
| QEMU semihosting flakiness in CI | Timeouts and a retry around the QEMU step only; a link-only check still gates |
| Pedantic Clippy churn on toolchain updates | The toolchain is pinned (RS-001); bump deliberately, in a PR that fixes new findings |
| CCSDS site URLs change | Nightly `fetch.sh --verify-only` alert; manual steps in `specs/README.md` |
