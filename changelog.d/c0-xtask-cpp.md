### Added
- C0 WP-2: `cargo xtask trace` reads `// req:` annotations from the C++ sources and headers under `cpp/` (artefacts labelled `test (C++)`), and `cargo xtask spec-lint` checks the requirement IDs in them (CPP-057).
- `cargo xtask cpp layers [<build-dir>]`: the CMake target graph (`cmake --graphviz`) against the crate layer rules, GEN-002 exceptions included (CPP-010); CI job `cpp-layers`.
- `cargo xtask cpp unsafe-optouts`: bounds-checking opt-outs (`unsafe_buffer_usage` pragmas, bounds-check `NOLINT`s, `.clang-tidy` disables) only in `cpp/include/aldis/core/bytes.hpp`, and every `NOLINT` names its check (CPP-031, CPP-042). Part of `cargo xtask ci`.
- `cargo xtask cpp link-check <elf>` and `cargo xtask cpp statics <elf>` for the freestanding exerciser of WP-5: no allocation, exception, abort, assertion, guard, or unwinder symbol, and no writable `aldis::` data (CPP-050, CPP-030).
- CI job `spec` reports `cargo xtask trace --scope c0` (strict from the C0 exit on).

### Changed
- `AGENTS.md`: the checks to run when changing `cpp/`.
