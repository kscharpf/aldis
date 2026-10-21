### Added
- `aldis-testkit`: the shared property-test strategies of VER-054, `aldis_testkit::strategies`
  (M2 plan D6), behind the feature `strategies` for `aldis-core` and `strategies-time`,
  `strategies-spp`, and `strategies-epp` for one codec crate each. They generate valid typed
  values built through the crates' own checked constructors and encoders (`aldis-core`
  identifiers, the decode mode, counters, bit fields, and frame layouts; time code formats,
  values, and ASCII codes; Space Packet and Encapsulation Packet fields, configurations, and
  whole packets), and invalid octet strings next to each decoder boundary (one octet too short,
  a length field off by one, a version or reserved value changed). `proptest` is re-exported,
  and `strategies::config` sets the case count and the regression file of a property test.
- Property tests: a rejected Encapsulation Packet encode and a rejected segment leave the
  buffer, the sequence counter, and the segmenter unchanged (API-022, SPP-013).

### Changed
- The property tests of `aldis-core`, `aldis-time`, `aldis-spp`, and `aldis-epp` are integration
  tests, `crates/<crate>/tests/properties.rs`, that draw their inputs from the shared strategies
  (VER-023, VER-054). The decoder properties run 1024 cases (256 before) on inputs that mix
  arbitrary octets with octets next to the boundaries; the others keep 256. The saved
  `proptest` regressions moved to `crates/<crate>/proptest-regressions/properties.txt`, and their
  shrunk inputs are also replayed as plain tests.
- The four crates no longer have a direct `proptest` dev-dependency; they use the one
  `aldis-testkit` re-exports.
