# Aldis

[![CI](https://github.com/kscharpf/aldis/actions/workflows/ci.yml/badge.svg)](https://github.com/kscharpf/aldis/actions/workflows/ci.yml)

> [!WARNING]
> **Pre-release, under active development.** Aldis is not ready for use:
> - Most of v1 is still to be built (see **Status** below).
> - It has not been qualified or certified for flight or any other mission-critical use.
> - Its API may change in any 0.x release (Semantic Versioning, GEN-080); the [changelog](CHANGELOG.md) lists the breaking changes.
> - The crates are not published to crates.io.
> - Outside contributions are not being accepted yet.
>
> The repository is public so that the free tools it relies on for open-source projects can be used (VER-055, OI-22).

Libraries that add CCSDS protocol support to spacecraft flight software and ground systems. The reference implementation is in Rust (`no_std`, no heap, panic-free), with a C ABI for C and C++ applications. Other languages are planned on top of that ABI.

## Status

| Milestone or track | State |
|---|---|
| M0 Foundations (`v0.1.0-m0`) | Released: the specification, the verification tooling, and `aldis-core` (Layer 0) |
| M1 Packets and time (`v0.2.0-m1`) | Released: the Layer 1 codecs for time codes, Space Packets, and Encapsulation Packets (`aldis-time`, `aldis-spp`, `aldis-epp`) |
| M2 Frame codecs | In progress. The four frame codecs are implemented, each with its C ABI: TM (`aldis-tm`); TC with the CLCW, frame delimiting, and validation (`aldis-tc`); AOS in both editions with the Frame Header Error Control, the M_PDU, and the B_PDU (`aldis-aos`); USLP with truncated frames, delimiting, and validation (`aldis-uslp`). The remaining verification gates and the exit review are next |
| M3–M6 | Not started: the Layer 2–3 crates (COP-1, link engines, framework) are skeletons |
| C0 native C++ foundations | In progress |
| A0 reference application scenario | In progress |

See the [roadmap](docs/08-roadmap.md) for every milestone and track.

## How Aldis is verified

Aldis is built to the practices of safety- and mission-critical software. It is **not** qualified or certified (see the warning above); these practices are how it is built, not a claim of fitness for flight. Every rule below is a numbered requirement in [`docs/`](docs/README.md), and most are enforced by a check of the merge gate rather than by review alone.

**Code rules** ([02-design-rules](docs/02-design-rules.md), [Rust profile](docs/profiles/rust.md))
- `no_std` and no heap: fixed capacities through const generics (GEN-010, GEN-011).
- No panics on any input, proven at link time: the bare-metal exerciser fails to link if any panic path is reachable (GEN-020, VER-010).
- No `unsafe` outside the C ABI crate, enforced by `unsafe_code = "forbid"` (GEN-070). Every `unsafe` block in `aldis-ffi` carries a `// SAFETY:` argument.
- What Aldis assumes of the platform, above all memory protected against single-event upsets, and what happens without it: [hardware assumptions](docs/hardware-assumptions.md) (GEN-015).
- Every loop bounded and no recursion (GEN-022), recorded per crate in [inspection records](docs/verification/inspections/).
- **Parse, don't validate** (RS-020): restricted values are their own types, so invalid values cannot be constructed; `cargo xtask rs020` checks the public API mechanically. State machines make invalid states unrepresentable (RS-021).
- A strict lint set (Clippy `all` and `pedantic` denied, plus restriction lints), with every exception local and reasoned (RS-016).

**Testing** ([06-verification](docs/06-verification.md), [07-test-vectors](docs/07-test-vectors.md))
- **100 % region, function, line, and branch coverage** of the codec crates and `aldis-core` (VER-024).
- **Test vectors with provenance**, shared by every implementation and run through both the Rust API and the C ABI. Each hand-derived file is computed by an **independent model** written from the Blue Book by an author who has not read the implementation (TV-023).
- **Cross-checks** against independent implementations where they exist (NASA cFE for Space Packets) ([cross-checks](docs/verification/cross-checks.md)).
- **Property tests** from shared strategies (VER-023, VER-054), **fuzzing** of every decoder and of the C ABI against the Rust decoders (VER-022), and **mutation testing** with every surviving mutant triaged (VER-025).
- **Bounded model checking** with Kani for selected kernels, nightly (OI-42, [formal-proofs](docs/verification/formal-proofs.md)).
- The bare-metal exerciser runs every public API and every vector on Cortex-M4F, Cortex-R5F, and RV32IMAC under QEMU; a big-endian (s390x) build runs the tests.

**Traceability and the merge gate**
- Every requirement cites its CCSDS Blue Book clause; every test names the requirements it covers; `cargo xtask trace` checks that each requirement of a milestone is traced (VER-020).
- **The merge gate is a reproducible run** of every check in a container image pinned by digest, on the exact commit (VER-056). It produces an **evidence record**, published as a git note, that a pull request must carry to merge (VER-057, VER-058). Every night, every check runs again from scratch on `main` (VER-059).
- Independent review of every pull request reads the published evidence and audits what no check covers: the Blue Book readings, `unsafe`, the API shape, and every suppression ([AGENTS.md](AGENTS.md)).
- Every maintainer decision is logged ([decision log](docs/process/decisions.md)), and open questions are tracked as numbered issues ([open issues](docs/09-open-issues.md)).

**Still to come in Milestone 2** ([plan](docs/milestones/m2-frame-codecs.md) WP-7, WP-8): a worst-case stack bound per target, a mechanical check of loop bounds against the inspection records, bit-flip completeness tests for every frame check sequence, an assurance case, and a hazard and failure-mode analysis.

## Layout

```
code/
├── Cargo.toml            workspace (edition 2024, shared lint set)
├── AGENTS.md             guide for contributors and coding agents
├── docs/                 the specification: start at docs/README.md
├── specs/                download manifest + fetch.sh for the CCSDS standards (PDFs git-ignored)
├── crates/
│   ├── aldis-core        L0  errors, bit fields, IDs, CRC-16, OID PN, counters, security hooks
│   ├── aldis-time        L1  301.0 time codes
│   ├── aldis-spp         L1  133.0 Space Packet
│   ├── aldis-epp         L1  133.1 Encapsulation Packet
│   ├── aldis-tm          L1  132.0 TM frames
│   ├── aldis-tc          L1  232.0 TC frames, CLCW, delimiting, validation
│   ├── aldis-aos         L1  732.0 AOS frames, FHEC, M_PDU, B_PDU
│   ├── aldis-uslp        L1  732.1 USLP frames, truncated frames, TFDF, delimiting, validation
│   ├── aldis-link        L2  sans-I/O generators, mux, receiver, extraction, segmentation
│   ├── aldis-cop1        L2  232.1 FOP-1 / FARM-1
│   ├── aldis-framework   L3  optional transport/clock traits and stacks
│   ├── aldis             facade (feature-gated re-exports)
│   ├── aldis-ffi         C ABI (staticlib/cdylib, cbindgen, build-time capacities)
│   └── aldis-testkit     tooling: test-vector model, loader, runner (dev-only)
├── xtask/                tooling: `cargo xtask help` (spec lint, trace, vectors, layers, symbols, coverage)
├── exerciser/            bare-metal panic-freedom check and on-target vector run (separate workspace)
├── fuzz/                 cargo-fuzz targets (separate workspace; run on the tooling nightly)
├── crosscheck/           vectors checked against independent implementations (separate workspace)
├── refapp/               reference application: flight and ground nodes, harness, golden traces
│                         (separate workspace; docs/reference-application.md)
└── test-vectors/         shared, language-neutral conformance vectors
```

Each crate has a README with its purpose, status, and layer, and an example for the implemented crates: [aldis-core](crates/aldis-core/README.md), [aldis-time](crates/aldis-time/README.md), [aldis-spp](crates/aldis-spp/README.md), [aldis-epp](crates/aldis-epp/README.md), [aldis-tm](crates/aldis-tm/README.md), [aldis-tc](crates/aldis-tc/README.md), [aldis-aos](crates/aldis-aos/README.md), [aldis-uslp](crates/aldis-uslp/README.md), [aldis-link](crates/aldis-link/README.md), [aldis-cop1](crates/aldis-cop1/README.md), [aldis-framework](crates/aldis-framework/README.md), [aldis](crates/aldis/README.md), [aldis-ffi](crates/aldis-ffi/README.md), [aldis-testkit](crates/aldis-testkit/README.md).

## Building

The toolchain is pinned to stable Rust 1.99.1 by `rust-toolchain.toml`. rustup selects it automatically, together with the bare-metal targets.

```sh
cargo check --workspace                                                     # hosted
cargo clippy --workspace --all-features                                     # lint set (VER-001)
cargo check -p aldis --no-default-features --features full --target thumbv7em-none-eabihf   # bare metal (GEN-005)
cargo build -p aldis-ffi --no-default-features --features panic-handler \
            --target thumbv7em-none-eabihf --release                        # C static library for bare metal
```

Nightly Rust is used only for tooling, at the date pinned in `tooling-nightly`:

```sh
cargo xtask coverage                                   # VER-024 gate; needs cargo-llvm-cov >= 0.9
cargo xtask verify                                     # every check in the pinned image: the merge gate (VER-056)
(cd exerciser && cargo run --release --target thumbv7em-none-eabihf)   # vectors under QEMU (stable)
(cd fuzz && cargo +$(cat ../tooling-nightly) fuzz run crc_variants)    # needs cargo-fuzz
```

## License

Licensed under either of

- the Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), or
- the MIT license ([LICENSE-MIT](LICENSE-MIT)),

at your option. This covers the code, the test vectors, and the documentation.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

## Where to start

1. [docs/README.md](docs/README.md): reading order and document map (contributors and coding agents: also read [AGENTS.md](AGENTS.md))
   - [docs/glossary.md](docs/glossary.md): what OCF, CLCW, FHP, and the other acronyms mean, and the project's own terms
2. [docs/01-architecture.md](docs/01-architecture.md): layers and crates
3. [docs/09-open-issues.md](docs/09-open-issues.md): decisions still needed
