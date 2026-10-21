### Added
- Native C++ vector tooling (C0 WP-4, [docs/milestones/c0-foundations.md](docs/milestones/c0-foundations.md) D5;
  CPP-053, CPP-034, TV-012), test-only, in `cpp/vectors/`: a case model of literal types and one
  `constexpr` check function per core operation, shared by every runner; a host loader
  (nlohmann/json) that follows the vector schemas and the exact match of TV-012, and keeps the
  optional field code of error expectations (M2 plan D3) without comparing it until C1; the host
  runner `aldis_vectors`, one CTest test per file of `test-vectors/core/`; and `aldis_vecgen`,
  which writes the cases as `constexpr` arrays for the freestanding exerciser (WP-5) and the
  `static_assert`s of CPP-034, built for the host by an `ExternalProject` in bare-metal presets
  (or taken from `ALDIS_VECGEN`). All 77 live core cases pass against `aldis_core`.

### Changed
- `cpp/include/aldis/core/config.hpp`: the port follows `aldis-core` 0.2.0 (commit `0655f2a`,
  `v0.2.0-m1`), not 0.1.0; `ALDIS_VERSION_*` stays 0.1.0 until differential verification against
  a Rust release starts in C1 (VER-053).
- `cargo xtask verify` (`cpp-tidy`) and the nightly `cpp.yml` run clang-tidy on `cpp/vectors/` too;
  `cargo xtask cpp layers` lists the vector libraries as tooling; the nightly bare-metal and LEON3
  jobs install the host GCC, which builds `aldis_vecgen`.
