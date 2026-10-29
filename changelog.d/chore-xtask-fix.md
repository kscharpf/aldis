### Added

- `cargo xtask fix` runs the generators (field codes, vector schema, SANA tables, C header) and writes the registrations that can be derived: the nightly `mutants` and `fuzz` matrices, the exerciser's `aldis-*` dependencies, and the facade's dependencies and features. It then prints what `consistency` still finds missing.

### Changed

- The coverage gate, the `features` crate list, and the C ABI vector areas are derived from the workspace (crates of Layers 0-2 with code; `aldis-ffi/src/<area>.rs`) instead of stored in `GATED`, `CRATES`, and `FFI_AREAS`.
- `cargo xtask trace` skips withdrawn requirements (text starting `Withdrawn:`), so a scope range that covers one no longer counts it as untraced.
