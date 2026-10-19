### Added
- C0 WP-1: the `cpp/` CMake project of the native C++ implementation ([cpp/README.md](cpp/README.md)). C++20 throughout (CPP-001); `aldis::core` as a header-only `INTERFACE` target, so far with `config.hpp` (language-standard check, version and `ALDIS_LIFETIMEBOUND` macros) and the umbrella `core.hpp`; every header also compiled on its own with the library warning set; options `ALDIS_BUILD_TESTS`, `ALDIS_BUILD_EXERCISER`, `ALDIS_HEADER_CHECKS`, `ALDIS_CRC_TABLE`, `ALDIS_TEST_HOOKS`, `ALDIS_SANITIZE`, `ALDIS_COVERAGE`.
- Presets `host-gcc`, `host-clang`, `host-clang-sanitize`, `host-clang-coverage`, `s390x-gcc`, `cortex-m4f-{gcc,clang}`, `cortex-r5f-{gcc,clang}`, `rv32imac-{gcc,clang}`, and `leon3-gcc`, each with a build and a test preset and a configure-time check against `cpp/toolchains.lock`; toolchain files with the CPU and ABI flags of the Rust bare-metal targets.
- The warning flags of `docs/profiles/cpp.md` §5 (CPP-040), `cpp/.clang-format` (CPP-044), and `cpp/.clang-tidy` with the naming rules and the CPP-002 include allow-list (CPP-041); dev-only dependencies GoogleTest v1.18.0 and nlohmann/json v3.12.0 pinned by commit and SHA-256 (CPP-003).
- Installed CMake package: `find_package(aldis)` exports `aldis::core` (CPP-012).
- CI jobs `cpp-format`, `cpp-build` (x86_64 and aarch64, GCC and Clang, with a consumer of the installed package), `cpp-tidy`, `cpp-sanitize`, `cpp-big-endian` (s390x under QEMU), `cpp-bare-metal` (three targets, both compilers), and the non-gating `cpp-leon3`.

### Changed
- `docs/profiles/cpp.md` §5: `modernize-use-trailing-return-type` disabled (style only), and a second table lists the checks tests and tools relax for GoogleTest.
