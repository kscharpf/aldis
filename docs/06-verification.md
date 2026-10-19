# 06 — Verification (mission-grade assurance)

**Assurance target:** *mission-grade*. The code is designed so that a later qualification against a specific standard (e.g. NASA NPR 7150.2 Class B/C, ECSS-E-ST-40C/Q-80C, DO-178C) needs evidence collection rather than redesign. This document does **not** claim compliance with any of those standards.

## 1. Static rules

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| VER-001 | All crates SHALL build warning-free with `#![deny(warnings)]` in CI. They SHALL also pass Clippy with the workspace lint set in [profiles/rust.md §5](profiles/rust.md#5-lint-configuration): the `clippy::all` and `clippy::pedantic` groups denied, plus the restriction lints `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `arithmetic_side_effects`, `cast_possible_truncation`, `unreachable`, `todo`, and `unimplemented`. `as_conversions` is also denied outside the `aldis-core` bit primitives. Exceptions follow RS-016. | GEN-020 | A |
| VER-002 | `#![forbid(unsafe_code)]` SHALL be present in every crate except `aldis-ffi` and the differential-test bridge (GEN-070, GEN-071). | GEN-070 | A |
| VER-003 | Dependency policy SHALL be enforced by a license, advisory (RustSec), and source check: `cargo-deny`, configured in `deny.toml`. Layers 0–2 SHALL have zero non-dev dependencies (GEN-001), checked by the CI `layers` job. | GEN-001 | A |
| VER-004 | Formatting SHALL be enforced (`rustfmt`, and `clang-format` for C artefacts). | — | A |

## 2. Panic freedom and bounded resources

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| VER-010 | For each crate, CI SHALL build a `no_std` **exerciser** binary that calls every public API with inputs from the vectors. It SHALL link with a `#[panic_handler]` that references an undefined external symbol. A successful link proves that no panic path survives optimization in the exercised code. | GEN-020, GEN-021 | A |
| VER-011 | Stack usage of every public function SHALL be measured for each embedded target (e.g. `-Z emit-stack-sizes` or an equivalent static analysis) and published as a table per release. | GEN-023 | A |
| VER-012 | Worst-case execution paths SHALL be documented for engine `handle_*` and `poll_*` functions as a function of input length and capacities. | GEN-023 | I |

## 3. Dynamic testing

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| VER-020 | Every requirement ID SHALL be referenced by at least one test, vector, fuzz target, or analysis record. A trace matrix (requirement → artefacts → result) SHALL be generated in CI from test annotations, e.g. `// req: SPP-002`, and published with each release. | — | A |
| VER-021 | All shared [test vectors](07-test-vectors.md) SHALL pass on every CI target. | — | V |
| VER-022 | Every decoder and every engine input SHALL have a fuzz target (`cargo-fuzz`/libFuzzer) asserting: no panic; `decode(encode(x)) == x` for accepted inputs; GEN-042 (no state change on rejected inputs). Each target SHALL run ≥ 1 CPU-hour per release, and every crash SHALL become a regression vector. | GEN-020, API-020, GEN-042 | F |
| VER-023 | Property-based tests SHALL cover round trips for every codec, counter arithmetic (CMN-030), FHP placement (LNK-011), COP-1 closed loop (COP-040/041), and segmentation/reassembly (LNK-040–072). They SHALL draw their inputs from the shared strategies of VER-054. | — | F |
| VER-024 | Coverage targets: **100 % statement and 100 % branch coverage** for Layers 0–2, measured with `cargo llvm-cov` on the host. Any uncovered region SHALL be justified in a coverage waiver file reviewed per release. MC/DC is reserved for a future certification effort ([open issues](09-open-issues.md) OI-09). | — | A |
| VER-025 | Mutation testing (e.g. `cargo-mutants`) SHALL run on Layers 0–2 before each release. Surviving mutants SHALL be triaged. | — | A |
| VER-026 | The FFI layer SHALL be tested under Miri and with AddressSanitizer/UBSan from a C test harness. | GEN-071 | T |
| VER-054 | Property-test input strategies (generators of valid and invalid identifiers, configurations, headers, data units, and engine input sequences) SHALL be defined once, in a tooling crate (RS-017), and used by both the Rust property tests (VER-023) and the differential tests (VER-053). | — | I |

## 4. Targets and CI matrix

| Target triple | Class | Builds | Tests |
|---|---|---|---|
| `x86_64-unknown-linux-gnu` | host / ground | all features | full suite, fuzz, coverage |
| `aarch64-unknown-linux-gnu` | flight Linux / ground | all features | full suite (QEMU or native) |
| `thumbv7em-none-eabihf` | Cortex-M4/M7 | no features | exerciser + vectors under QEMU |
| `armv7r-none-eabihf` | Cortex-R | no features | exerciser + vectors under QEMU |
| `riscv32imac-unknown-none-elf` | RISC-V MCU | no features | exerciser + vectors under QEMU |
| `sparc-unknown-none-elf` | LEON3/4 (tier 3) | no features (`-Z build-std`) | build + exerciser link check ([open issues](09-open-issues.md) OI-08) |

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| VER-030 | At least one big-endian target SHALL run the full vector suite (GEN-007). | GEN-007 | V |
| VER-031 | CI SHALL verify that a minimal build containing only `aldis-core` and `aldis-spp` compiles without any frame-protocol code (GEN-003), by inspecting symbols. | GEN-003 | A |

## 5. Independent cross-checks

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| VER-040 | Vectors SHALL be cross-checked against at least one independent, existing implementation per standard where one is available (open-source libraries or agency tools), and against examples printed in the Blue Books (e.g. the OID sequence, CMN-020). Discrepancies SHALL be resolved by reference to the Blue Book text and recorded. | — | V |
| VER-041 | Every cell of each COP-1 state table SHALL be reviewed by two people against the Blue Book. The review record SHALL be kept (COP-003). | — | I |

## 6. C side (C ABI and native C/C++)

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| VER-050 | Generated C headers SHALL compile cleanly as C99, C11, and C++11/17 with `-Wall -Wextra -Wpedantic -Werror` on GCC and Clang. | — | A |
| VER-051 | C example and test code, and the native C implementation, SHALL follow **MISRA C:2012**, checked with a static analyser. Deviations SHALL be recorded in a deviation log. | — | A |
| VER-052 | The native C++ implementation SHALL follow **MISRA C++:2023**, checked with a static analyser in CI ([C++ profile](profiles/cpp.md) §6, [open issues](09-open-issues.md) OI-22). Deviations, including any use of language features newer than the C++ standard MISRA C++:2023 addresses (OI-19), SHALL be recorded in a deviation log. Exceptions and RTTI SHALL be disabled, and there SHALL be no dynamic allocation. | — | A |
| VER-053 | Each native implementation SHALL be tested **differentially** against the Rust reference crates. Every decoder, encoder, and engine input that has a fuzz target (VER-022) or a property test (VER-023) SHALL also have a differential fuzz target and a differential property test that run the Rust crates and the native implementation, called through its C ABI (CABI-070), on the same inputs. They SHALL assert identical results: output octets, consumed length, error kind, field identifier, error detail, and anomaly flags; for engines, every `poll_*` output and event, in order, for generated input sequences. Every divergence SHALL be resolved by reference to the Blue Book, fixed in the defective implementation, and added as a regression [test vector](07-test-vectors.md). | GEN-030, API-020 | F |
