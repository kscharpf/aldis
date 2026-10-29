### Added

- `cargo xtask fix` runs the generators (field codes, vector schema, SANA tables, C header), then `consistency`.

### Changed

- The coverage gate, the `features` crate list, and the C ABI vector areas are derived from the workspace (crates of Layers 0-2 with code; `aldis-ffi/src/<area>.rs`) instead of stored in `GATED`, `CRATES`, and `FFI_AREAS`; `consistency` no longer carries the four checks that compared those lists.
