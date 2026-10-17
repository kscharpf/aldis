# Changelog

All notable changes to Aldis are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) (GEN-080). Every fix to a deviation from a CCSDS Blue Book is called out under **Fixed**.

## [Unreleased]

### Added
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
