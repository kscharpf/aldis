### Added
- `crosscheck/`: NASA cFE's `MSG` module (v7.0.1, Apache-2.0) as a second independent oracle for
  Space Packets (VER-040, M2 D15, OI-32;
  [docs/verification/cross-checks.md](docs/verification/cross-checks.md)). The five cFE sources
  and eight headers the check needs are vendored unmodified under `crosscheck/vendor/cfe/` with
  cFE's licence and their SHA-256, compiled by the `cc` crate with small stubs for the OSAL and
  cFE headers they include (`crosscheck/cfe-stubs/`), and called through `crosscheck/src/cfe_msg.rs`,
  the only `unsafe` code of the cross-checks. cFE reads and builds the primary header of every
  well-formed packet in `test-vectors/spp/` against `aldis-spp`, checks the expected values it can
  produce (decoding, encoding, Octet String, Idle Packets, segments, streams, and its CUC 4+2
  telemetry time in `spp/secondary_header_time` and `time/cuc`), and compares 4200 random packets
  from `aldis_testkit::strategies` with a fixed seed. No disagreement and no known discrepancy.
