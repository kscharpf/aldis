# Changelog

All notable changes to Aldis are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) (GEN-080). Every fix to a deviation from a CCSDS Blue Book is called out under **Fixed**.

## [Unreleased]

Changes merged since the last release are kept as one file per pull request in [`changelog.d/`](changelog.d/README.md). `cargo xtask changelog show` lists them; `cargo xtask changelog release` moves them here.

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
- Changelog fragments: each pull request adds one file to [`changelog.d/`](changelog.d/README.md) instead of editing `CHANGELOG.md`, so parallel pull requests do not conflict. `cargo xtask changelog check` (part of `cargo xtask ci`), `show`, and `release <version> <date>`. The entries that were under **Unreleased** moved to `changelog.d/unreleased-before-fragments.md`.
- `cargo xtask verify` (`xtask/src/verify/`): the verification run that gates merges (VER-056…VER-058, OI-31). It runs one check per former pull-request job of `ci.yml` and `cpp.yml`, under the same names, in a pinned image (`xtask/verify/Containerfile`) on a fresh checkout of HEAD. It writes an evidence record to `target/verify/<commit>/record.json`; `--publish` pushes the record as a git note under `refs/notes/verify` and sets the commit status `aldis/verify`. `verify check` re-checks a published record.
- `.github/workflows/verify.yml` (VER-060): the same verification run on a GitHub-hosted runner, started only by hand on a branch (`gh workflow run verify.yml --ref <branch>`). It publishes the record and status like a local run; the record's new `source` field and the status's link name the run.
- `setup_cpp.py versions`: checks the installed C++ tools against `cpp/toolchains.lock` without reinstalling them; the script no longer calls `sudo` when it runs as root.
- Plan for milestone C0, the foundations of the native C++ implementation ([docs/milestones/c0-foundations.md](docs/milestones/c0-foundations.md)): pinned GCC 14 and Clang 23 toolchains, CMake presets for the host and bare-metal targets, C++ CI jobs, the freestanding link check, CodeQL Coding Standards, the IKOS evaluation, and the port of `aldis-core` to `aldis_core`. Decisions D1–D19 are proposed.
- Open issues OI-27 (how a field identifier crosses the C ABI) and OI-28 (CPP-001 and header-only code) in [docs/09-open-issues.md](docs/09-open-issues.md).
- C0 WP-0: pinned C++ toolchains in `cpp/toolchains.lock` (CPP-001, CPP-003): LLVM 23.1.3, Ubuntu 24.04 GCC 14.2.0 (host and s390x cross), Arm GNU Toolchain 14.3.Rel1, xPack RISC-V GCC 14.3.0-1, Gaisler BCC2 2.2.4 (GCC 10.2.0, LEON3, non-gating), CMake 4.4.4, Ninja 1.13.2, QEMU 8.2.2, and the CodeQL 2.21.4 bundle with the Coding Standards packs 2.62.0, each with its download URL and SHA-256 or exact package version.
- Composite action `.github/actions/setup-cpp`, the counterpart of `setup-rust`: installs exactly the locked versions, caches archives by checksum, and checks and prints every version.
- `cpp/cmake/AldisToolchainCheck.cmake`: configure fails when the compiler differs from the lock (D1).
- Workflow `.github/workflows/cpp.yml` (name "C++", D17) with job `cpp-toolchains`, which installs the whole set on `ubuntu-24.04` and `ubuntu-24.04-arm` and checks that a changed checksum and a mismatching compiler are rejected.
- C0 WP-1: the `cpp/` CMake project of the native C++ implementation ([cpp/README.md](cpp/README.md)). C++20 throughout (CPP-001); `aldis::core` as a header-only `INTERFACE` target, so far with `config.hpp` (language-standard check, version and `ALDIS_LIFETIMEBOUND` macros) and the umbrella `core.hpp`; every header also compiled on its own with the library warning set; options `ALDIS_BUILD_TESTS`, `ALDIS_BUILD_EXERCISER`, `ALDIS_HEADER_CHECKS`, `ALDIS_CRC_TABLE`, `ALDIS_TEST_HOOKS`, `ALDIS_SANITIZE`, `ALDIS_COVERAGE`.
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
