# Aldis

[![CI](https://github.com/kscharpf/aldis/actions/workflows/ci.yml/badge.svg)](https://github.com/kscharpf/aldis/actions/workflows/ci.yml)

Aldis is a CCSDS protocol stack for spacecraft flight software and ground systems, written in Rust (`no_std`, no heap, panic-free) with a C ABI for C and C++ applications. It implements Space Packets, Encapsulation Packets, time codes, TM, TC, AOS, and USLP Transfer Frames, frame synchronization, COP-1, and the sans-I/O engines that link them: you hand the library bytes and time, and it hands back bytes and events.

Aldis has two aims. It is meant to be a usable CCSDS stack that comes with the correctness evidence expected of safety- and mission-critical software. It is also a showcase of how embedded high-assurance Rust can be built and verified, largely by coding agents under a reproducible merge gate.

> [!NOTE]
> **Pre-release, under active development.** It has not been qualified against any standard, its wire behaviour has not yet been exercised against an independent implementation, its API may change in any 0.x release, and the crates are not published to crates.io. Details: [status](#status) and [limits](#limits).

## Example

Encode a Space Packet and decode it again, through the `aldis` facade with the features `core` and `spp`. This code is compiled and run as a doctest of [`crates/aldis/README.md`](crates/aldis/README.md), and `cargo xtask consistency` checks that it is the same code.

```rust
use aldis::core::ids::{Apid, SeqCount};
use aldis::spp::{
    PRIMARY_HEADER_LEN, PacketFields, PacketType, SequenceFlags, SpacePacket, SppConfig, encode,
};

fn main() -> Result<(), aldis::core::Error> {
    let fields = PacketFields {
        packet_type: PacketType::Telemetry,
        apid: Apid::try_from(0x123)?,
        sequence_flags: SequenceFlags::Unsegmented,
        sequence_count: SeqCount::try_from(42)?,
    };
    let packet = SppConfig::DEFAULT.packet(fields, &[], b"hello")?;

    let mut buf = [0_u8; 64];
    let len = encode(&packet, &mut buf)?;
    assert_eq!(len, PRIMARY_HEADER_LEN + 5);

    let decoded = SpacePacket::decode(&buf[..len])?;
    assert_eq!(decoded.apid().get(), 0x123);
    assert_eq!(decoded.sequence_count().get(), 42);
    assert_eq!(decoded.data_field(), b"hello");
    Ok(())
}
```

The decoded packet borrows the buffer: nothing is allocated, and every error is a value, never a panic. Each crate has its own README with an example; the [crate list](#layout) links them.

## Status

| Milestone or track | State |
|---|---|
| M0 Foundations (`v0.1.0-m0`) | Released: the specification, the verification tooling, and `aldis-core` (Layer 0) |
| M1 Packets and time (`v0.2.0-m1`) | Released: the Layer 1 codecs for time codes, Space Packets, and Encapsulation Packets (`aldis-time`, `aldis-spp`, `aldis-epp`) |
| M2 Frame codecs (`v0.3.0-m2`) | Released: the Layer 1 codecs for TM, TC, AOS, and USLP Transfer Frames (`aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp`), each with its C ABI functions, the hazard analysis, and the assurance case |
| M3 COP-1 (`v0.4.0-m3`) | Released: `aldis-cop1` (FOP-1 and FARM-1 as step functions over their state tables), their C ABI handles, the closed-loop tests, the fuzz targets, and the hazard analysis ([plan §4](docs/milestones/m3-cop1.md)) |
| M4 Link engines (`v0.5.0-m4`) | Released: `aldis-link` (the PacketGenerator, Access and Bitstream generators, ChannelMux, FrameReceiver, PacketExtractor, MapSegmenter, MapReassembler, and CLTU packer over TM, TC, AOS, and USLP), their C ABI handles, the blind vectors, the fuzz targets, and the hazard analysis ([plan §4](docs/milestones/m4-link-engines.md)) |
| M5 C ABI completion (`v0.6.0-m5`) | Released: `aldis-sync` (ASM attach, the TM pseudo-randomizer, and the ASM search engine of 131.0 §9–10) with its C ABI, the C harness under sanitizers and the C ABI tests under Miri, header checks across C and C++ standards, `aldis.hpp` over the engine handles, the open v1 requirements closed, and the hazard analysis ([plan §4](docs/milestones/m5-c-abi-completion.md)) |
| M6 Crate architecture simplification | In progress; Phases 1–2 merged; the OI-93 follow-up of WP-2d is #475; Phase 3 and the exit deferred until after AINT ([roadmap §4](docs/08-roadmap.md#4-overlap)): one checking pipeline, one frame-format trait per codec, one frame writer, and a generic C ABI over them, without a change of wire behaviour ([plan](docs/milestones/m6-architecture.md)) |
| A0 reference application scenario | Complete: the scenario ([`docs/reference-application.md`](docs/reference-application.md), area APP) and the `refapp/` skeleton ([plan](docs/milestones/a0-scenario.md)) |
| A1 reference application packets | Complete: the flight and ground nodes of `refapp/` exchange Space Packets with CUC time stamps through the harness, and the golden traces of the four `a1-*` cases are decoded with the strict codecs and reproduced in CI ([plan](docs/milestones/a1-packets.md)) |
| A2 reference application frames | Complete: the flight and ground nodes of `refapp/` exchange TM and TC frames, multiplexed and demultiplexed by the `aldis-link` engines, and the golden traces of the four `a2-*` cases are decoded with the strict codecs and reproduced in CI ([plan](docs/milestones/a2-frames.md)) |
| A3 reference application COP-1 | Complete: the ground node sends every uplink Frame Data Unit as a Type-AD frame through `Fop1`, the flight node accepts frames through `Farm1` and reports its state in the CLCW, and the golden traces of the five `a3-*` cases, retransmission over a lossy link included, are decoded with the strict codecs and reproduced in CI ([plan](docs/milestones/a3-cop1.md)) |
| AINT interoperability against an independent ground system | Planned, entry gate A3 met (exited 2026-11-11): the Rust flight node exchanges TM and TC frames and COP-1 with an independent mission control system run as a black box (YAMCS recommended, OSDLP the alternative; the milestone plan decides) over a real transport, and every discrepancy is filed as an open issue or reported upstream by the maintainer ([roadmap §3](docs/08-roadmap.md#3-track-a--reference-application-a0a6-aint-acfs)) |

See the [roadmap](docs/08-roadmap.md) for every milestone and track, and its [coverage table](docs/08-roadmap.md#9-ccsds-document-coverage) for the CCSDS documents and sections each milestone covers.

## Limits

- v1.0 is not released: it waits on M6 and the reference-application milestones A2 to A6 ([roadmap §5](docs/08-roadmap.md#5-releases)).
- Aldis has not been qualified against any standard and claims compliance with none. It is designed so that a later qualification (for example NASA NPR 7150.2 Class B/C, ECSS-E-ST-40C/Q-80C, or DO-178C) needs evidence collection rather than redesign ([assurance target](docs/06-verification.md)).
- Its wire behaviour has not yet been exercised against an independent implementation; the AINT milestone plans that.
- Its API may change in any 0.x release (GEN-080); the [changelog](CHANGELOG.md) lists the breaking changes.
- Outside contributions are not being accepted yet. The repository is public as a showcase, which also lets it use the free tools VER-055 requires (OI-22).

## Evidence

Every claim of correctness is backed by a check that the merge gate runs or by a record a person signed, not by assertion. The README does not repeat them; the documents do.

- [Verification](docs/06-verification.md): the lints, link-time panic freedom, coverage, fuzzing, mutation testing, test vectors from blind independent models, cross-checks, and the targets (including bare-metal QEMU runs).
- [Test vectors](docs/07-test-vectors.md) and [cross-checks](docs/verification/cross-checks.md): the shared conformance data and what it is compared against.
- [Traceability](docs/06-verification.md): every requirement cites its Blue Book clause and every test names the requirements it covers; `cargo xtask trace` reports what is traced.
- [The merge gate](docs/06-verification.md#7-verification-runs-and-the-merge-gate): a reproducible run of every check in a digest-pinned image, with a published evidence record.
- [Assurance case](docs/verification/assurance-case.md) and [hazard analysis](docs/verification/hazards.md): the argument from claims to evidence, and the failure modes.
- [Design rules](docs/02-design-rules.md) and [hardware assumptions](docs/hardware-assumptions.md): what the code may and may not do, and what it assumes of the platform.

## How it is built

Aldis is built by coding agents under rules a person sets. Implementation agents work in their own worktrees, a fresh independent reviewer agent reads every pull request, and the maintainer decides scope, API and ABI, dependencies, releases, and every merge.

- [AGENTS.md](AGENTS.md): the rules for coding agents.
- [Decision rights](docs/process/decision-rights.md): who decides what.
- [Independent review](docs/process/review.md) and the [coordinator](docs/process/coordinator.md): the review protocol and the merge trains.
- `cargo xtask metrics` measures the process (VER-063).

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

## Where to start

1. [docs/README.md](docs/README.md): reading order and document map (contributors and coding agents: also read [AGENTS.md](AGENTS.md))
   - [docs/glossary.md](docs/glossary.md): what OCF, CLCW, FHP, and the other acronyms mean, and the project's own terms
2. [docs/01-architecture.md](docs/01-architecture.md): layers and crates
3. [docs/09-open-issues.md](docs/09-open-issues.md): decisions still needed

## License

Licensed under either of

- the Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), or
- the MIT license ([LICENSE-MIT](LICENSE-MIT)),

at your option. This covers the code, the test vectors, and the documentation.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

## About the name

Aldis is named after the Aldis lamp, the signal lamp that ships use to send Morse code by light.
