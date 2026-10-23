### Added
- Verification spike (OI-42): Kani 0.68.0 proof harnesses in `aldis-core` (CRC-16 against a
  shift-register reference, the two FECF methods, the CMN-031 counter classification, the bit-field
  and octet-integer helpers), `aldis-spp` (every primary header, `SpacePacket::decode` against
  SPP-001/002/004, header and whole-packet encode→decode), and `aldis-time` (the TIME-011 CUC
  conversions and round trips for 1 and 2 fine octets, and the conversion to nanoseconds for 3; the
  others did not finish within 30 minutes), compiled only under `cfg(kani)`. `cargo xtask kani` runs them and
  prints each harness's result and time; it is no check of `cargo xtask verify` and gates nothing.
  `docs/verification/formal-proofs.md` records the bounds, the trusted base, the canaries, and the
  limits; `cargo xtask trace` lists the harnesses as `proof` artefacts.

### Changed
- `docs/09-open-issues.md`: new OI-42, formal verification method and tool (open).
- The workspace `unexpected_cfgs` check accepts `cfg(kani)`.
