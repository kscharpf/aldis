# Changelog

All notable changes to Aldis are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) (GEN-080). Every fix to a deviation from a CCSDS Blue Book is called out under **Fixed**.

## [Unreleased]

### Added
- `xtask` tooling (`cargo xtask help`): `spec-lint` (requirement tables and every ID reference), `trace [--scope m0] [--strict]` (requirement → artefact matrix), `vectors validate`, `vectors schema [--check]`, `links`, `layers`, and `ci`.
- `aldis-testkit`: test-vector model, loader, and exact-match runner; JSON Schemas for vector files generated from the model (`test-vectors/schema/`).
- Core vectors: `errors.json`, `bits.json`, `counters.json` (hand-derived, independent Python reference).
- `docs/verification/analysis-records.md` for requirements verified by analysis or inspection.
- RS-017: tooling lint profile.
- CI job `spec`: spec lint, links, vector validation, schema check, and M0 trace matrix (artefact `trace-m0`).

- `aldis-core` (M0 WP-4): `error` (stable catalogue), `bits` (big-endian bit fields), `ids` (width-checked identifiers), `time` (`Instant`, `Duration`), `crc` (FECF CRC-16, bitwise and table), `oid` (OID generator, Fibonacci and Galois forms), `counter` (modular counters, gap classification), `container` (`FixedQueue`, `ByteQueue`, `FixedVec`), `security` (SDLS reservation hooks), `layout` (frame field layout). 100 % region/function/line coverage.
- Core vector `layout.json`; all core vectors run against `aldis-core`.
- `clippy.toml` and RS-018: lint exemptions for test code.

### Changed
- M0 exit scope: API-072, API-074, GEN-043, GEN-052, GEN-053, and TV-010 moved to the codec and engine milestones that implement them.
- CI `layers` job runs `cargo xtask layers` (replaces `.github/scripts/check_layers.py`) and also checks that tooling crates are only dev-dependencies.

### Fixed
- Requirement rows API-055 and EPP-008 contained unescaped `|` in code spans, which split the table cells (also mis-rendered on GitHub).
- GEN area now maps to both `01-architecture.md` and `02-design-rules.md`, where its requirements are defined.
- GitHub Actions CI: formatting, Clippy (`all` + `pedantic` + restriction lints), tests on Linux and macOS, `no_std` builds for Cortex-M, Cortex-R and RISC-V, big-endian tests (s390x), dependency layering check, and `cargo-deny`.
- Nightly workflow: big-endian bare-metal build and a CCSDS document URL check.
- `specs/fetch.sh --check-urls` (HEAD-only check; downloads nothing).
- Repository housekeeping: CODEOWNERS, pull-request template, Dependabot, this changelog.

## [0.1.0-spec] — 2026-10-16

### Added
- Specification draft 0.1 for v1 (Space Packet, Encapsulation Packet, TM, TC/CLCW, AOS, USLP, COP-1, time codes), with Rust and C ABI profiles, verification plan, and test-vector format.
- Cargo workspace skeleton (`aldis-*` crates), pinned Rust 1.99.1.
- Download manifest and fetch script for the CCSDS standards (not redistributed).
- Seed conformance vectors: CRC-16 check value, OID idle pattern.
