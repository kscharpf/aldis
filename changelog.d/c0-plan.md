### Added
- Plan for milestone C0, the foundations of the native C++ implementation ([docs/milestones/c0-foundations.md](docs/milestones/c0-foundations.md)): pinned GCC 14 and Clang 23 toolchains, CMake presets for the host and bare-metal targets, C++ CI jobs, the freestanding link check, CodeQL Coding Standards, the IKOS evaluation, and the port of `aldis-core` to `aldis_core`. Decisions D1–D19 are proposed.
- Open issues OI-27 (how a field identifier crosses the C ABI) and OI-28 (CPP-001 and header-only code) in [docs/09-open-issues.md](docs/09-open-issues.md).

### Changed
- OI-27 resolved (maintainer): field identifiers get stable numeric field codes, catalogued in `docs/03-api-model.md` §9.1 (new API-082) and carried by the C ABI (CABI-021, CPP-021). The catalogue and the Rust `FieldId` code are added in M2 WP-0.
- OI-28 resolved (maintainer): CPP-001 reworded. Aldis builds and the installed package require exactly C++20, the headers reject older standards, and later standards are allowed but not verified.
