### Added
- C0 WP-3: `aldis_core`, the native C++ port of `aldis-core` (`0655f2a`): `error.hpp` (catalogue, `Error`, `Result<T>`, `Result<void>`), `mode.hpp`, `anomaly.hpp`, `bits.hpp`, `ids.hpp`, `time.hpp`, `crc.hpp`, `oid.hpp`, `counter.hpp`, `container.hpp`, `security.hpp`, `layout.hpp`, and the umbrella `core.hpp` under `cpp/include/aldis/core/`; header-only, `constexpr`, and `noexcept` throughout (CPP-020, CPP-021, CPP-023, CPP-025, CPP-026, CPP-034).
- The C++-only modules `bytes.hpp`, the one audited place with raw element access (CPP-031), and `arith.hpp`, the wrapping, checked, and saturating helpers that no unsigned operation in them overflows (CPP-032).
- The test-only `FixedPatternSecurity` hook in `cpp/testing/` (target `aldis::core_test_hooks`, never installed).
- GoogleTest ports of every `aldis-core` unit test with the same `// req:` IDs, plus tests of the C++-only behaviour and `noexcept`/trivially-copyable checks of the public API; the property tests become seeded randomized tests.

### Changed
- `docs/profiles/cpp.md` §5: `readability-identifier-length` and `bugprone-easily-swappable-parameters` disabled for the whole C++ code base, each with its reason (CPP-041); `cpp/.clang-format` keeps `NOLINT` comments on one line.
- `docs/milestones/c0-foundations.md` §6 lists the API differences between `aldis_core` and `aldis-core`; no behaviour divergence was found.

### Fixed
- `cargo xtask verify` failed with `reference is not a tree` when HEAD was detached on a commit that no branch or tag reaches; the container now fetches the commit itself as well.
- Concurrent `cargo xtask verify` runs on one machine shared the C++ build directories in the cache volume, so a run could delete another's build output or pass checks against objects built from another commit. A run now holds a lock on the volume and waits for any run already in progress.
