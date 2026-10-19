# Changelog

All notable changes to Aldis are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) (GEN-080). Every fix to a deviation from a CCSDS Blue Book is called out under **Fixed**.

## [Unreleased]

### Added
- RS-020 (Rust profile) and an `AGENTS.md` rule: parse, don't validate. Existing violations are tracked in #22.
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

### Changed
- **Breaking:** `aldis` facade crate no longer enables `core` by default (`default = []`), matching `docs/profiles/rust.md` §2; enable `core` or `full` explicitly.
- **Breaking:** `aldis-core::ids`: `FrameSeqNum`'s inner `u8` field is now private (use `FrameSeqNum::new` or `From<u8>`, and `FrameSeqNum::get` or `u8::from`), matching the other identifier types (RS-010); `SpacecraftId`, `VcId`, and `MapId` `TryFrom` errors now report snake_case `FieldId`s (`"spacecraft_id"`, `"vc_id"`, `"map_id"`) instead of type names (API-041).
- `aldis-core::security`: `SecurityLayout::new` is now a `const fn` (RS-012), `SecurityLayout::MAX_TRAILER_LEN` is public alongside `MAX_HEADER_LEN` (API-070, API-071), and `Protocol` is `#[non_exhaustive]` (GEN-081).
- Now `const fn`: `AnomalySet::iter`, `ByteQueue::clear`, `FixedVec::clear`, and `ModCounter::increment` in `aldis-core`; `CdsFormat::t_len` and `TimeCode::format` in `aldis-time` (RS-019).
- `aldis-testkit` model and runner types now derive `Eq` (RS-019).
- Forwarded `alloc`, `std`, and protocol sub-features across `aldis`, `aldis-spp`, `aldis-link`, `aldis-cop1`, `aldis-framework`, and `aldis-ffi`.

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
