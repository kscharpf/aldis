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

**Status:** Milestone 1 (`v0.2.0-m1`) complete: the specification, verification tooling, and `aldis-core` (Layer 0, Milestone 0, `v0.1.0-m0`), and the Layer 1 codecs for time codes, Space Packets, and Encapsulation Packets (`aldis-time`, `aldis-spp`, `aldis-epp`). The frame codecs (the rest of Layer 1) and Layers 2–3 are skeletons pending Milestones 2–6. The native C++ foundations (C0) and the reference application scenario (A0) are in progress ([roadmap](docs/08-roadmap.md)).

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
│   ├── aldis-uslp        L1  732.1 USLP frames, truncated frames, TFDF
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

MIT. See [LICENSE](LICENSE).

## Where to start

1. [docs/README.md](docs/README.md): reading order and document map (contributors and coding agents: also read [AGENTS.md](AGENTS.md))
2. [docs/01-architecture.md](docs/01-architecture.md): layers and crates
3. [docs/09-open-issues.md](docs/09-open-issues.md): decisions still needed
