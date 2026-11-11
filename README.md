# Aldis

[![CI](https://github.com/kscharpf/aldis/actions/workflows/ci.yml/badge.svg)](https://github.com/kscharpf/aldis/actions/workflows/ci.yml)

Aldis is a set of libraries that add CCSDS protocol support to spacecraft flight software and ground systems. The reference implementation is in Rust (`no_std`, no heap, panic-free), with a C ABI for C and C++ applications. Other languages are planned on top of that ABI.

Aldis exists for two reasons. It aims to be a usable CCSDS stack that comes with the correctness evidence expected of safety- and mission-critical software. It is also a showcase of how embedded high-assurance Rust can be built and verified: [How Aldis is verified](#how-aldis-is-verified) lists the practices that back this claim.

The evidence, in short:
- No panic path is reachable from any input, proven at link time (GEN-020, VER-010; [code rules](#how-aldis-is-verified)).
- 100 % region, function, line, and branch coverage of the codec crates and `aldis-core` (VER-024; [testing](#how-aldis-is-verified)).
- Test vectors computed by blind independent models written from the Blue Book alone (TV-023; [testing](#how-aldis-is-verified)).
- The merge gate is a reproducible run in a digest-pinned image that produces a published evidence record (VER-056–VER-058; [merge gate](#how-aldis-is-verified)).

The second thing Aldis showcases is how it is built: by coding agents under a merge gate. Implementation agents work in their own worktrees and commit under the reproducible gate. A fresh, independent reviewer agent reads every pull request ([review protocol](docs/process/review.md)), and test-vector models are written blind, from the Blue Book and the docs only (TV-023). Who decides what is fixed by the [decision rights](docs/process/decision-rights.md) (VER-062): the maintainer decides scope, API and ABI, dependencies, releases, and every merge, and a [coordinator](docs/process/coordinator.md) runs the merge trains. The process is measured with `cargo xtask metrics` (VER-063). The rules for agents are in [AGENTS.md](AGENTS.md).

Aldis has not been qualified against any standard. The goal is to reach that point: it is designed so that a later qualification against a standard (for example NASA NPR 7150.2 Class B/C, ECSS-E-ST-40C/Q-80C, or DO-178C) needs evidence collection rather than redesign ([assurance target](docs/06-verification.md)). It claims compliance with none of them.

## Where Aldis stands

> [!NOTE]
> **Pre-release, under active development.**
> - v1.0 is not released: it waits on M6 and the reference-application milestones A2–A6 ([roadmap §5](docs/08-roadmap.md#5-releases); see **Status** below).
> - It has not been qualified against any standard for flight or any other mission-critical use.
> - Its wire behaviour has not yet been exercised against an independent implementation; that evidence is planned as the AINT milestone (see **Status** below).
> - Its API may change in any 0.x release (Semantic Versioning, GEN-080); the [changelog](CHANGELOG.md) lists the breaking changes.
> - The crates are not published to crates.io.
> - Outside contributions are not being accepted yet.
>
> The repository is public as a showcase; being public also lets it use the free tools that VER-055 requires (OI-22).

## Status

| Milestone or track | State |
|---|---|
| M0 Foundations (`v0.1.0-m0`) | Released: the specification, the verification tooling, and `aldis-core` (Layer 0) |
| M1 Packets and time (`v0.2.0-m1`) | Released: the Layer 1 codecs for time codes, Space Packets, and Encapsulation Packets (`aldis-time`, `aldis-spp`, `aldis-epp`) |
| M2 Frame codecs (`v0.3.0-m2`) | Released: the Layer 1 codecs for TM, TC, AOS, and USLP Transfer Frames (`aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp`), each with its C ABI functions, the hazard analysis, and the assurance case |
| M3 COP-1 (`v0.4.0-m3`) | Released: `aldis-cop1` (FOP-1 and FARM-1 as step functions over their state tables), their C ABI handles, the closed-loop tests, the fuzz targets, and the hazard analysis ([plan §4](docs/milestones/m3-cop1.md)) |
| M4 Link engines (`v0.5.0-m4`) | Released: `aldis-link` (the PacketGenerator, Access and Bitstream generators, ChannelMux, FrameReceiver, PacketExtractor, MapSegmenter, MapReassembler, and CLTU packer over TM, TC, AOS, and USLP), their C ABI handles, the blind vectors, the fuzz targets, and the hazard analysis ([plan §4](docs/milestones/m4-link-engines.md)) |
| M5 C ABI completion (`v0.6.0-m5`) | Released: `aldis-sync` (ASM attach, the TM pseudo-randomizer, and the ASM search engine of 131.0 §9–10) with its C ABI, the C harness under sanitizers and the C ABI tests under Miri, header checks across C and C++ standards, `aldis.hpp` over the engine handles, the open v1 requirements closed, and the hazard analysis ([plan §4](docs/milestones/m5-c-abi-completion.md)) |
| M6 Crate architecture simplification | Phases 1–2 merged; Phase 3 and the exit deferred until after AINT ([roadmap §4](docs/08-roadmap.md#4-overlap)): one checking pipeline, one frame-format trait per codec, one frame writer, and a generic C ABI over them, without a change of wire behaviour ([plan](docs/milestones/m6-architecture.md)) |
| A0 reference application scenario | Complete: the scenario ([`docs/reference-application.md`](docs/reference-application.md), area APP) and the `refapp/` skeleton ([plan](docs/milestones/a0-scenario.md)) |
| A1 reference application packets | Complete: the flight and ground nodes of `refapp/` exchange Space Packets with CUC time stamps through the harness, and the golden traces of the four `a1-*` cases are decoded with the strict codecs and reproduced in CI ([plan](docs/milestones/a1-packets.md)) |
| A2 reference application frames | In progress: TM and TC frames multiplexed and demultiplexed by the `aldis-link` engines ([plan](docs/milestones/a2-frames.md)) |
| AINT interoperability against an independent ground system | Planned, after A3: the Rust flight node exchanges TM and TC frames and COP-1 with an independent mission control system run as a black box (YAMCS recommended, OSDLP the alternative; the milestone plan decides) over a real transport, and every discrepancy is reported upstream or filed as an open issue ([roadmap §3](docs/08-roadmap.md#3-track-a--reference-application-a0a6)) |

See the [roadmap](docs/08-roadmap.md) for every milestone and track, and its [coverage table](docs/08-roadmap.md#9-ccsds-document-coverage) for the CCSDS documents and sections each milestone covers.

## How Aldis is verified

Aldis is built to the practices of safety- and mission-critical software. These practices are how it is built, and they back its purpose as a showcase of embedded high-assurance Rust, not a claim of fitness for flight. Every rule below is a numbered requirement in [`docs/`](docs/README.md), and most are enforced by a check of the merge gate rather than by review alone.

**Code rules** ([02-design-rules](docs/02-design-rules.md), [Rust profile](docs/profiles/rust.md))
- `no_std` and no heap: fixed capacities through const generics (GEN-010, GEN-011).
- No panics on any input, proven at link time: the bare-metal exerciser fails to link if any panic path is reachable (GEN-020, VER-010).
- No `unsafe` outside the C ABI crate, enforced by `unsafe_code = "forbid"` (GEN-070). Every `unsafe` block in `aldis-ffi` carries a `// SAFETY:` argument.
- What Aldis assumes of the platform, above all memory protected against single-event upsets, and what happens without it: [hardware assumptions](docs/hardware-assumptions.md) (GEN-015).
- Every loop bounded and no recursion (GEN-022), verified by static object-level call-graph cycle detection and formal proof harnesses ([analysis record](docs/verification/records/GEN-022.md)).
- Worst-case stack bounds computed per embedded target for every public entry point and checked against fixed budgets ([stack bounds](docs/verification/records/VER-011.md), VER-011).
- **Parse, don't validate** (RS-020): restricted values are their own types, so invalid values cannot be constructed; `cargo xtask rs020` checks the public API mechanically. State machines make invalid states unrepresentable (RS-021).
- A strict lint set (Clippy `all` and `pedantic` denied, plus restriction lints), with every exception local and reasoned (RS-016).

**Testing** ([06-verification](docs/06-verification.md), [07-test-vectors](docs/07-test-vectors.md))
- **100 % region, function, line, and branch coverage** of the codec crates and `aldis-core` (VER-024).
- **Test vectors with provenance**, shared by every implementation and run through both the Rust API and the C ABI. Each hand-derived file is computed by an **independent model** written from the Blue Book by an author who has not read the implementation (TV-023).
- **Cross-checks** against independent implementations where they exist (NASA cFE for Space Packets) ([cross-checks](docs/verification/cross-checks.md)).
- **Property tests** from shared strategies (VER-023, VER-054), **fuzzing** of every decoder and of the C ABI against the Rust decoders (VER-022), and **mutation testing** with every surviving mutant triaged (VER-025).
- **Bounded model checking** with Kani for selected kernels, nightly (OI-42, [formal-verification](docs/verification/formal-verification.md)).
- The bare-metal exerciser runs every public API and every vector on Cortex-M4F, Cortex-R5F, and RV32IMAC under QEMU; a big-endian (s390x) build runs the tests.

**Traceability and the merge gate**
- Every requirement cites its CCSDS Blue Book clause; every test names the requirements it covers; `cargo xtask trace` checks that each requirement of a milestone is traced (VER-020).
- **The merge gate is a reproducible run** of every check in a container image pinned by digest, on the exact commit (VER-056). It produces an **evidence record**, published as a git note, that a pull request must carry to merge (VER-057, VER-058). Every night, every check runs again from scratch on `main` (VER-059).
- Independent review of every pull request reads the published evidence and audits what no check covers: the Blue Book readings, `unsafe`, the API shape, and every suppression ([AGENTS.md](AGENTS.md)).
- Open questions are tracked as numbered issues ([open issues](docs/09-open-issues.md)).
- An [assurance case](docs/verification/assurance-case.md) and systematic [hazard analysis](docs/verification/hazards.md) argue fitness against failure modes and claims (VER-065, VER-066).

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
│   ├── aldis-sync        L1  131.0 ASM, CADU, pseudo-randomizer, ASM search
│   ├── aldis-link        L2  sans-I/O generators, mux, receiver, extraction, segmentation
│   ├── aldis-cop1        L2  232.1 FOP-1 / FARM-1
│   ├── aldis             facade (feature-gated re-exports)
│   ├── aldis-ffi         C ABI (staticlib, cbindgen, build-time capacities)
│   └── aldis-testkit     tooling: test-vector model, loader, runner (dev-only)
├── xtask/                tooling: `cargo xtask help` (spec lint, trace, vectors, layers, symbols, coverage)
├── exerciser/            bare-metal panic-freedom check and on-target vector run (separate workspace)
├── fuzz/                 cargo-fuzz targets (separate workspace; run on the tooling nightly)
├── crosscheck/           vectors checked against independent implementations (separate workspace)
├── refapp/               reference application: flight and ground nodes, harness, golden traces
│                         (separate workspace; docs/reference-application.md)
└── test-vectors/         shared, language-neutral conformance vectors
```

Each crate has a README with its purpose, status, and layer, and an example for the implemented crates: [aldis-core](crates/aldis-core/README.md), [aldis-time](crates/aldis-time/README.md), [aldis-spp](crates/aldis-spp/README.md), [aldis-epp](crates/aldis-epp/README.md), [aldis-tm](crates/aldis-tm/README.md), [aldis-tc](crates/aldis-tc/README.md), [aldis-aos](crates/aldis-aos/README.md), [aldis-uslp](crates/aldis-uslp/README.md), [aldis-sync](crates/aldis-sync/README.md), [aldis-link](crates/aldis-link/README.md), [aldis-cop1](crates/aldis-cop1/README.md), [aldis](crates/aldis/README.md), [aldis-ffi](crates/aldis-ffi/README.md), [aldis-testkit](crates/aldis-testkit/README.md).

## Building

The toolchain is pinned to stable Rust 1.99.1 by `rust-toolchain.toml`. rustup selects it automatically, together with the bare-metal targets. The supported targets, and what each is tested with, are listed in [06-verification §4](docs/06-verification.md#4-targets-and-ci-matrix).

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

## About the name

Aldis is named after the Aldis lamp, the signal lamp that ships use to send Morse code by light.
