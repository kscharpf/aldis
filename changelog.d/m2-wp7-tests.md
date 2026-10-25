### Added
- A test for every restricted type (M2 WP-7, RS-020, VER-023): `crates/<crate>/tests/restricted.rs`
  in the eight codec crates shows, for each public type with a checked constructor, that it accepts
  exactly the range its Blue Book clause or requirement gives (every `u8`, `u16`, and `bool` input
  exhaustively), rejects every other value with the documented error, and carries every accepted
  value through its wire field. `cargo xtask restricted` (in `cargo xtask ci` and the verify check
  `spec`) fails on a restricted type that no test names in a `// restricted: <Type>` marker, and on
  a marker that names none; `cargo xtask restricted list` prints the types. New out-of-range and
  field strategies in `aldis_testkit::strategies` (VER-054).
- Bit-flip completeness of the FECF (M2 WP-7): `crates/aldis-{tm,tc,aos,uslp}/tests/bit_flips.rs`
  check every single-bit and double-bit error of fixed valid frames, the outcome stated per region
  (AOS header errors in the FHEC's symbols are corrected, the rest rejected), within the 4 095
  octets up to which the CRC-16 detects every double-bit error, derived in
  `docs/verification/derivations/crc16-bit-flips.md`. The gated tests sample the longest frames;
  the nightly job `bit-flips` runs them exhaustively. Helpers in `aldis_testkit::bitflip`.

### Changed
- The verify checks that read the Rust crates also read `specs/sana/**`, which the USLP UPID test
  compares with the generated table.
