# 06 — Verification (mission-grade assurance)

**Assurance target:** *mission-grade*. The code is designed so that a later qualification against a specific standard (e.g. NASA NPR 7150.2 Class B/C, ECSS-E-ST-40C/Q-80C, DO-178C) needs evidence collection rather than redesign. This document does **not** claim compliance with any of those standards.

## 1. Static rules

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| VER-001 | All crates SHALL build warning-free with `#![deny(warnings)]` in CI. They SHALL also pass Clippy with the workspace lint set in [profiles/rust.md §5](profiles/rust.md#5-lint-configuration): the `clippy::all` and `clippy::pedantic` groups denied, plus the restriction lints `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `arithmetic_side_effects`, `cast_possible_truncation`, `unreachable`, `todo`, and `unimplemented`. `as_conversions` is also denied outside the `aldis-core` bit primitives. Exceptions follow RS-016. | GEN-020 | A |
| VER-002 | `#![forbid(unsafe_code)]` SHALL be present in every crate except `aldis-ffi` and the differential-test bridge (GEN-070, GEN-071). | GEN-070 | A |
| VER-003 | Dependency policy SHALL be enforced by a license, advisory (RustSec), and source check: `cargo-deny`, configured in `deny.toml`. Layers 0–2 SHALL have zero non-dev dependencies (GEN-001), checked by the CI `layers` job. | GEN-001 | A |
| VER-004 | Formatting SHALL be enforced (`rustfmt`, and `clang-format` for C artefacts). | — | A |
| VER-055 | Every tool and document that a requirement of this specification depends on SHALL be available free of charge for this project: open-source, or under a free licence for open-source projects. No requirement SHALL depend on a commercial tool or on a document that must be purchased. A free licence that holds only for public open-source repositories SHALL be named as such where the tool is required ([open issues](09-open-issues.md) OI-22). | — | I |

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
| VER-051 | C example and test code, and the native C implementation, SHALL follow the **SEI CERT C Coding Standard**, checked with a free static analyser (VER-055). Deviations SHALL be recorded in a deviation log. | — | A |
| VER-052 | The native C++ implementation SHALL follow the **SEI CERT C++ Coding Standard** and the **AUTOSAR C++14 Guidelines**, both freely available, checked in CI with free static analysers ([C++ profile](profiles/cpp.md) §5, CPP-043). Deviations, including uses of C++17 and C++20 features that the AUTOSAR C++14 rules do not address (OI-19), SHALL be recorded in a deviation log. Exceptions and RTTI SHALL be disabled, and there SHALL be no dynamic allocation. | — | A |
| VER-053 | Each native implementation SHALL be tested **differentially** against the Rust reference crates. Every decoder, encoder, and engine input that has a fuzz target (VER-022) or a property test (VER-023) SHALL also have a differential fuzz target and a differential property test that run the Rust crates and the native implementation, called through its C ABI (CABI-070), on the same inputs. They SHALL assert identical results: output octets, consumed length, error kind, field identifier, error detail, and anomaly flags; for engines, every `poll_*` output and event, in order, for generated input sequences. Every divergence SHALL be resolved by reference to the Blue Book, fixed in the defective implementation, and added as a regression [test vector](07-test-vectors.md). | GEN-030, API-020 | F |

## 7. Verification runs and the merge gate

The checks of this specification run on the contributor's machine, not on a hosted CI service. A pull request merges on the evidence record of a local run; GitHub Actions repeats every check each night as an independent run ([open issues](09-open-issues.md) OI-31).

Wherever this specification, a profile, or a milestone plan says that a check runs **in CI**, or names a CI job or a CI target, it means that check in the verification run of VER-056. A check that only runs at night says so.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| VER-056 | `cargo xtask verify` SHALL run every gating check of this specification on one commit and refuse a working tree with uncommitted or untracked changes. Each check SHALL run in a Linux container image of the host's native architecture, built from definitions in the repository: the base image pinned by digest, and every compiler, analyser, emulator, and build tool at a version pinned in the repository (`rust-toolchain.toml`, `tooling-nightly`, `cpp/toolchains.lock`, `xtask/verify/Containerfile`); the versions of the other installed packages are recorded (VER-057). A check that needs a macOS host runs on the host when it is macOS. A check SHALL be either **portable**, run on every supported host, or **host-bound**, needing a named operating system or architecture; a host-bound check the host cannot run SHALL be reported as not run, never as passed. The C++ checks MAY be skipped when the change against the tip of `main` touches none of the paths the C++ build, tests, or tooling read; the run SHALL then report them as skipped and record the path list and the base commit. The container engine SHALL be podman or Docker Engine (VER-055). | — | I |
| VER-057 | Every verification run SHALL write an **evidence record**: canonical JSON with the commit and tree hashes; the host's operating system and architecture; the ID of each container image, the version of each tool, and the version of every installed package; and for each check its name, command, result (`pass`, `fail`, `not-run`, or `skipped`, with the reason for the last two), duration, and the SHA-256 of its log. `cargo xtask verify --publish` SHALL store the record of a run in which no check failed as a git note on the verified commit under `refs/notes/verify` and push it. The logs SHALL stay with the contributor; for a release commit they SHALL be attached to the release. | — | A |
| VER-058 | A pull request SHALL merge into `main` only when its head commit contains the tip of `main` and carries a published record (VER-057) whose tree hash is the head commit's tree and in which every portable check passed. `cargo xtask verify --publish` SHALL then set the commit status `aldis/verify` to success on the head commit, naming the SHA-256 of the record, and SHALL set no status from any other record. Branch protection on `main` SHALL require that status and branches that are up to date. | — | I |
| VER-059 | GitHub Actions SHALL run every check of the verification run each night on `main` and on manual dispatch, through `cargo xtask verify` on `x86_64` and `aarch64` Linux, and as native jobs that do not use its images on both Linux architectures and macOS, together with the night-only checks (fuzzing, mutation testing, stack usage). The workflows SHALL NOT run on pull requests or pushes. A nightly failure SHALL open an issue. A nightly failure of a commit whose record passed SHALL be treated as a defect in the code, the images, or `cargo xtask verify` until the cause is found. | — | I |
