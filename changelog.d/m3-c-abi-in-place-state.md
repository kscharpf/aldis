### Changed
- `aldis-ffi`: `handle.rs` lends the state in caller storage to a closure in place (`Handle::read`, `Handle::update`), the mechanism for engine calls without a copy of the state on the stack (OI-57 convention 6; CABI-040). The SPP tracker, the SPP reassembler, and OID generator calls that take no caller buffer use it. The C ABI is unchanged.
- `docs/`: the introduction of `docs/profiles/c-abi.md` §6 and §12 describe the mechanism.
