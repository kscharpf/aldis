# Aldis — native C++ implementation

The native C++20 implementation of Aldis, for flight software that cannot take a Rust toolchain. The Rust crates in [`../crates`](../crates) are the reference; this library is verified against them on the shared [test vectors](../test-vectors/README.md) and by differential tests. Requirements: the [C++ profile](../docs/profiles/cpp.md) (area CPP). Current milestone: [C0](../docs/milestones/c0-foundations.md).

## Language standard

Every Aldis build is **exactly C++20**, and the headers reject an older standard at compile time (CPP-001). You may compile the headers under a later standard, but **that use is not verified**: the tests, vectors, analysis, and coverage all run under C++20.

## Building

Configure through a preset. Each preset pins its compiler to [`toolchains.lock`](toolchains.lock) and fails at configure time if the compiler differs (CPP-001); CI installs the pinned tools with [`.github/actions/setup-cpp`](../.github/actions/setup-cpp/action.yml). You need CMake ≥ 3.28 and Ninja.

```sh
cmake --preset host-gcc      # or host-clang, host-clang-sanitize, host-clang-coverage, s390x-gcc,
cmake --build --preset host-gcc   # cortex-m4f-{gcc,clang}, cortex-r5f-{gcc,clang},
ctest --preset host-gcc           # rv32imac-{gcc,clang}, leon3-gcc
```

Build output goes to `../target/cpp/<preset>/`. For a local experiment with an unpinned compiler, add `-DALDIS_ALLOW_UNPINNED_COMPILER=ON`: the configure then warns instead of failing, and the result verifies nothing (CI refuses the option).

| Option | Default | Effect |
|---|---|---|
| `ALDIS_BUILD_TESTS` | on when top-level | Host tests (GoogleTest) |
| `ALDIS_HEADER_CHECKS` | on when top-level | Compile every public header on its own with the library warning set |
| `ALDIS_CRC_TABLE` | off | Table-driven CRC-16 (the Rust `crc-table` feature) |
| `ALDIS_TEST_HOOKS` | off | Test-only security hook (the Rust `test-hooks` feature); never installed |
| `ALDIS_SANITIZE` | off | Clang sanitizers (CPP-051) |
| `ALDIS_COVERAGE` | off | Clang source-based coverage (CPP-052) |
| `ALDIS_VECGEN` | empty | Host `aldis_vecgen` for a bare-metal build; empty: a bare-metal preset builds it first with the pinned host GCC |
| `ALDIS_VECGEN_ONLY` | off | Build only the vector loader and `aldis_vecgen` (the host build of a bare-metal preset) |

## Conformance vectors

`cpp/vectors/` runs the shared [test vectors](../test-vectors/README.md) against the library (CPP-053): `ctest` runs `aldis_vectors` once per file of `test-vectors/core/`, with the exact comparison of TV-012, and `aldis_vecgen` writes the same cases as `constexpr` arrays (`aldis/vectors/generated/core.hpp`) for the freestanding exerciser and the compile-time checks of CPP-034. Test tooling only: nothing in `cpp/vectors/` is installed.

## Using the installed package

```cmake
find_package(aldis 0.1 REQUIRED)
target_link_libraries(app PRIVATE aldis::core)
```

The library is header-only and needs no exceptions, RTTI, heap, or hosted standard library (CPP-002, CPP-011).
