# Milestone C0 — Foundations (native C++)

**Goal:** a C++ project, CI, and verification tooling for the native C++ implementation, plus a complete, verified `aldis_core` that matches `aldis-core` on the shared vectors. Every later C++ milestone builds on this.
**Roadmap entry:** [08-roadmap.md](../08-roadmap.md) §2 (track C) · **Builds on:** [M0](m0-foundations.md) (`aldis-core`, `v0.1.0-m0`) · **Profile:** [C++ profile](../profiles/cpp.md) · **Release:** none (track C milestones do not cut releases, roadmap §5)

**Status: in progress (2026-10-19).** Entry gate: none (roadmap §2). Decisions D1–D19 accepted by the maintainer 2026-10-19 (D19 with OI-28). WP-0 (toolchains), WP-1 (CMake project), and WP-2 (xtask) are in review.

## 1. Starting point

| In place | Missing for C0 |
|---|---|
| Specification: [C++ profile](../profiles/cpp.md) (area CPP), [C ABI profile](../profiles/c-abi.md), VER-050…055; OI-19…OI-22 resolved (C++20, capacities as template parameters, M1 C ABI in M2 WP-0, free tools only) | The `cpp/` directory: CMake project, presets, toolchain files |
| The reference: `crates/aldis-core` at `0655f2a`, 12 modules (`error`, `mode`, `anomaly`, `bits`, `ids`, `time`, `crc`, `oid`, `counter`, `container`, `security`, `layout`), 100 % covered, fuzzed, inspected | All `aldis_core` code, and the two C++-only modules the profile asks for: byte access (CPP-031) and arithmetic helpers (CPP-032) |
| Core vectors: `test-vectors/core/{bits,counters,crc16,errors,layout,oid}.json`, 77 live cases, 9 operations | A C++ vector loader, host runner, and build-time conversion to constant data (CPP-053) |
| The Rust checks to mirror: CI jobs `fmt`, `clippy`, `test`, `no-std`, `big-endian`, `exerciser` (link check, canary, QEMU on Cortex-M), `coverage`, `spec`; nightly `stack-sizes`; `xtask` (`spec-lint`, `trace`, `coverage`, `layers`) | Every C++ CI job; C++ toolchains in CI; a freestanding exerciser; CodeQL and IKOS |
| `xtask trace` reads `// req:` from `.rs` files under `crates/`, `xtask/`, `fuzz/`, `exerciser/`; `spec-lint` reads `md`, `rs`, `json`, `yml`, `yaml`, `toml`, `py` | Scanning of C++ sources (CPP-057) |

## 2. Decisions

| # | Decision | Choice | Status |
|---|---|---|---|
| D1 | Pinned compilers and how CI installs them (CPP-001, CPP-061, VER-055) | **GCC 14 and Clang 23**, one major version of each across host and bare-metal builds, so both compilers give the same warnings everywhere. Clang 23 matches the LLVM major of Rust 1.99.1 (LLVM 23.1.1), which CPP-061 needs for the differential fuzz targets in C1. Clang, clang-tidy, clang-format, `llvm-cov`, and `llvm-profdata` come from the official LLVM release tarball, pinned by version and SHA-256. Host GCC is Ubuntu 24.04's `gcc-14`/`g++-14`, with the exact package version asserted. All versions and checksums are in one file, `cpp/toolchains.lock`, read by a composite action `.github/actions/setup-cpp` (the counterpart of `setup-rust`). The CMake presets fail at configure time if the compiler's version differs from the lock. Bumping a version is a PR that changes the lock file and the changelog (CPP-001). Alternatives: `apt.llvm.org` packages (the point release floats); GCC 15 from the Ubuntu toolchain PPA (no matching s390x cross compiler in Ubuntu 24.04, see D3) | Accepted (2026-10-19) |
| D2 | Bare-metal toolchains (CPP-011, CPP-040; OI-08) | **GCC:** Arm GNU Toolchain 14.x (`arm-none-eabi`) for Cortex-M4F (`thumbv7em-none-eabihf`) and Cortex-R5F (`armv7r-none-eabihf`), and xPack `riscv-none-elf-gcc` 14.x for `riscv32imac`, as release tarballs pinned by SHA-256. **Clang:** the pinned Clang 23 with `--target` and the CPU flags of the matching Rust target, using the GCC toolchain's libstdc++ headers and `libgcc` (`--gcc-toolchain`/`--sysroot`). Neither build links libstdc++, libsupc++, or a C library (D6). Both compilers build all three targets, because CPP-040 asks for both pinned compilers and does not exempt bare metal. **LEON3/SPARC:** Gaisler BCC2 (GCC-based, free download; licence checked before adoption, VER-055) builds the library and the exerciser in a **non-gating** job; a run under QEMU `leon3_generic` is tried. The result is reported against OI-08, where native C++ may be the better LEON path than tier-3 Rust. Alternative for Clang on Arm: Arm Toolchain for Embedded (LLVM-based, with picolibc and libc++), which has no RISC-V counterpart | Accepted (2026-10-19) |
| D3 | CI targets for the C++ suite (CPP-053; VER-021, VER-030 applied to C++) | Host: `x86_64` on `ubuntu-24.04` and `aarch64` on `ubuntu-24.04-arm` (the matrix row `aarch64-unknown-linux-gnu`), each with GCC and Clang. Big-endian: Ubuntu's `g++-14-s390x-linux-gnu` with QEMU user mode runs the host tests and the vector runner, as `cross` does for Rust. Bare metal: the three targets of D2, executed under QEMU (D6). No macOS job: Apple Clang is not the pinned Clang. "Every CI target" in the exit criteria means these seven configurations; LEON3 is outside the gate (D2) | Accepted (2026-10-19) |
| D4 | Test framework and dev-only dependencies (CPP-003) | **GoogleTest** for host tests and **nlohmann/json** for the vector loader, each pinned to a commit and fetched by CMake `FetchContent` from the commit's archive URL with `URL_HASH SHA256` (a pinned commit that cannot change under us). `FETCHCONTENT_SOURCE_DIR_<NAME>` allows offline builds. `cpp/cmake/Dependencies.cmake` lists each dependency with its licence (both MIT-compatible: BSD-3-Clause, MIT); the list is reviewed in every PR that changes it, since `cargo deny` (VER-003) does not see C++. Test executables may use exceptions and RTTI; library code never does (CPP-002). No property-test library in C0: the few randomized tests (CRC forms, OID forms, containers against a model) use a seeded generator in test support. The library for native property tests is chosen in C1 with CPP-054. Alternatives: doctest (smaller, single header); git submodules (pinned too, but every checkout pays for them) | Accepted (2026-10-19) |
| D5 | Vector runner design (CPP-053, CPP-034, TV-012) | One **loader** (host only, nlohmann/json) reads a vector file into C++ case structs that are literal types (fixed-capacity octet arrays, no heap in the structs themselves). The same **check functions** (`cpp/vectors/checks/*.hpp`, one per operation, `constexpr`) run in three places: (1) the **host runner** (CTest), which loads the JSON at run time and so also runs under the sanitizers; (2) a **code generator** (`aldis_vecgen`, a host tool built from the same loader) that writes the cases as `constexpr` arrays into a generated header for the freestanding runner (D6); (3) `static_assert`s over a subset of the generated cases (CPP-034). Exact-match semantics follow `aldis-testkit`: superseded cases are skipped, every `expect` key must be produced and no other (TV-012), and an unknown operation fails. Cross builds take the generator from a host build (`ALDIS_VECGEN` cache variable; the bare-metal presets build it first through `ExternalProject`). Alternatives: generate the header with `cargo xtask` from the `aldis-testkit` model (one parser for both languages, but the C++ build would need Rust), or a Python script | Accepted (2026-10-19) |
| D6 | Freestanding exerciser and link check (CPP-050, CPP-053) | One binary per target, `cpp/exerciser/`, as in Rust: it calls every public `aldis_core` API with the generated vector cases (inputs pass through an optimisation barrier, the counterpart of `black_box`) and reports through semihosting under QEMU. Linked with `-nostdlib -nostartfiles`: only `libgcc` (integer and float helpers) and the exerciser's own start-up code, `memcpy`, `memmove`, and `memset` (the only things CABI-004 lets the library need). Any reference to `operator new`/`delete`, `__cxa_throw`, `__cxa_allocate_exception`, `__cxa_guard_acquire`/`release`, `__cxa_pure_virtual`, `abort`, `__assert_func`/`__assert_fail`, `std::__glibcxx_assert_fail`, `__gxx_personality_v0`, or `__aeabi_unwind_cpp_pr0…2` is then undefined and fails the link. `cargo xtask cpp link-check <elf>` also checks with `nm` that none of these is defined, so adding a library to the link line cannot hide one. Thread-safe statics stay on, so a guarded static shows up as `__cxa_guard_acquire`. Four **canaries** (CMake option `ALDIS_EXERCISER_CANARY`: `new`, `guard`, `abort`, `assert`) must each fail to link naming the expected symbol, on every target and compiler. Built at `-O2` and `-Os`. QEMU machines are the ones M1 WP-5 settles on (M1 D6): `mps2-an386` (Cortex-M4F), the Cortex-R machine chosen there, and `virt` for RISC-V; if no Cortex-R machine works, Cortex-R stays link-only and the deviation from CPP-053 is recorded, as M1 D6 does for VER-010 | Accepted (2026-10-19) |
| D7 | Module, header, and namespace layout (cpp.md §2) | `aldis_core` is a **header-only** `INTERFACE` target `aldis::core`, because CPP-034 makes its code `constexpr` and later engines are class templates (CPP-024). One header per Rust module, `cpp/include/aldis/core/<module>.hpp`, plus `bytes.hpp` (D9), `arith.hpp` (D10), `config.hpp` (`ALDIS_LIFETIMEBOUND`, version macros), and an umbrella `core.hpp`. Types live in `aldis::core`; free functions live in a nested namespace named after the Rust module (`aldis::core::bits::get`, `aldis::core::crc::crc16`), so call sites read like the Rust ones. Internals go in `aldis::core::detail`. Naming as in cpp.md §3: types `PascalCase`, functions and members `snake_case`, data members with a trailing `_`, enumerators `PascalCase`; enforced by `readability-identifier-naming`. Every header is also compiled on its own by a generated translation unit, so each header is self-contained and gets the full warning set and clang-tidy. Alternative: a static library with explicit instantiations (stable symbols for stack measurement, but `constexpr` code still has to live in headers) | Accepted (2026-10-19) |
| D8 | `Result`, `Error`, and identifiers (CPP-020, CPP-021, CPP-026) | `Result<T>` as cpp.md §3 shows it: `ok()` and `error()` return pointers (`nullptr` in the wrong state), so no accessor can be misused into undefined behaviour. Storage is a `union` of `T` and `Error` plus a tag; `T` must be trivially copyable and trivially destructible (a concept), which keeps `Result` `constexpr` and needs no `<variant>` (not on the CPP-002 allowlist, and `std::get` can throw). `Result<void>` is a specialization. A `Result` converts implicitly from `T` and from `Error`, so `return error;` propagates without macros. `ErrorKind` is an `enum class : std::uint16_t` with the codes of the catalogue, checked against `errors.json` by the vector runner. `ErrorDetail` is a class with private members and factories for `None`, `Needed`, and `Expected`. `FieldId` wraps a pointer to a static string name, as `aldis-core` does today, compared by content with a `constexpr` loop bounded by a maximum name length. OI-27 is resolved: field identifiers get numeric codes (API-082), catalogued and added to the Rust `FieldId` in M2 WP-0; the C++ `FieldId` gains the code in the same way when the catalogue lands. C0 does not depend on it. Identifiers: sub-width ones (`Apid`, `SeqCount`, `PacketVersion`, `SpacecraftId<Bits>`, `VcId<Bits>`, `MapId<Bits>`) only through `static constexpr Result<…> make(raw)`; full-width `FrameSeqNum` through a `constexpr explicit` constructor; `value()` as in cpp.md §3 (Rust: `get()`) | Accepted (2026-10-19) |
| D9 | Byte- and element-access module (CPP-031) | `aldis/core/bytes.hpp` is the **only** place with pointer arithmetic, `operator[]` on spans and arrays, and `#pragma clang unsafe_buffer_usage begin/end`. It offers checked element access over `std::span<T>` for any trivially copyable `T` (returning `T*`/`const T*` or `nullptr`), `subspan` that returns `Result`, copy and fill, and octet cursors (`ByteReader`, `ByteWriter`) whose reads and writes return `Result`. CPP-031 names a "byte-access" module, but the containers (`FixedQueue`, `FixedVec`) also need element access to arrays of `T`; putting that in the same module keeps one audited opt-out, which is what CPP-031 is for. `cargo xtask cpp unsafe-optouts` fails if the pragma, or a `NOLINT` for the bounds checks (`cppcoreguidelines-pro-bounds-*`, and `…-avoid-unchecked-container-access` where the pinned clang-tidy has it), appears anywhere else. `std::array::at`, `std::span::at` (C++26), and `std::optional::value` are not used: they throw | Accepted (2026-10-19) |
| D10 | Arithmetic helpers (CPP-032, CPP-051) | `aldis/core/arith.hpp`: `wrapping_add/sub<Bits>`, `checked_add/sub/mul` (returning `Result` with `OutOfRange`), `saturating_add/sub`, and checked narrowing `narrow<To>(from)`, with the field width as a template parameter (GEN-024). They are written so that **no unsigned operation overflows** (compare-then-subtract instead of add-then-mask), so they need no exemption from `unsigned-integer-overflow`. CPP-051 allows them to be exempt; the attribute (`no_sanitize`, Clang only, behind a macro because GCC warns on unknown sanitizer names) is used only if a measured cost requires it. Shifts and `static_cast` between integer widths appear only in `arith.hpp` and `bits.hpp`, as `as` appears only in the Rust bit primitives | Accepted (2026-10-19) |
| D11 | Containers without placement new (CPP-030) | Placement new is allowed only in the C ABI layer (CPP-035), so `FixedQueue<T, N>`, `FixedVec<T, N>`, and `ByteQueue<N>` store elements in a `std::array<T, N>` and require `T` to be trivially copyable and default-constructible (a concept). `aldis-core` already requires `Copy + Default` for `FixedVec`; for `FixedQueue` the C++ constraint is stricter than Rust's. `push` returns `Result<void>` with `WouldBlock` on full where Rust returns the item back (`Result<(), T>`): the caller still holds a copy. Listed as an API difference, not a behaviour divergence (roadmap §6.5) | Accepted (2026-10-19) |
| D12 | Where the C++ tooling lives (CPP-057; VER-020) | Building and testing use only CMake, Ninja, and CTest, so a C++ user needs no Rust. Repository checks that parse tool output live in `xtask` beside their Rust counterparts, as `cargo xtask cpp <check>`: `layers` (CMake `--graphviz` target graph against the layer table in `xtask/src/layers.rs`, CPP-010), `link-check` (D6), `coverage` (D14), `stack-usage` (D15), `unsafe-optouts` (D9), `statics` (no writable `aldis::` data symbols in the exerciser ELF, CPP-030), `deviations` (D13). `trace` reads `// req:` from `.hpp`, `.cpp`, and `.h` under `cpp/` (later also `difftest/`), labelled `test (C++)`, and `spec-lint` checks requirement IDs in those files too (CPP-057). The CPP-002 include allowlist is enforced by clang-tidy's `portability-restrict-system-includes`, configured in `cpp/.clang-tidy` for library headers and relaxed by a nested `.clang-tidy` for tests and tools | Accepted (2026-10-19) |
| D13 | CodeQL Coding Standards (CPP-043, VER-052, VER-055) | The CodeQL **CLI** runs in a CI job `cpp-codeql`, pinned to the CLI version that the chosen release of the `github/codeql-coding-standards` packs supports, with the AUTOSAR C++14 and CERT C++ packs. The database is built from the host GCC build of `aldis_core`, its header TUs, and the exerciser. SARIF goes to GitHub code scanning (for review in the UI) and to an artefact. The job fails on any result not matched by an entry in the deviation log `docs/verification/cpp-deviations.md` (rule, file, symbol, justification; matched by `cargo xtask cpp deviations`, not by line number). If the packs' own deviation-record mechanism (`coding-standards.yml`) fits, the log is generated from it instead; settled in WP-6. Findings on C++20-only constructs are classified by hand and logged as such (CPP-043). The CLI is free here only because the repository is public and OSI-licensed (MIT), as OI-22 records. Alternative: default code scanning through `github/codeql-action` only; it shows alerts but does not gate a PR without repository rulesets, and cannot check the deviation log | Accepted (2026-10-19) |
| D14 | Coverage (CPP-052, VER-024) | Clang source-based coverage (`-fprofile-instr-generate -fcoverage-mapping`) of the host tests and the host vector runner, gated by `cargo xtask cpp coverage` at 100 % of regions, functions, lines, and branches of `cpp/include/aldis/`, the same four metrics as the Rust gate. Waivers go in the existing `coverage-waivers.toml`, keyed by C++ module name (`aldis_core`), so one file is reviewed per release. Uninstantiated templates produce no coverage records, so the gate also requires each public template to be instantiated by the tests (checked against the exerciser's list of public APIs). An MC/DC report (`-fcoverage-mcdc`) is produced as an artefact for information (CPP-052, OI-09) | Accepted (2026-10-19) |
| D15 | Stack usage (CPP-056) | GCC bare-metal builds of a probe translation unit that wraps each public function in a non-inlined `extern` function, compiled with `-fstack-usage -fcallgraph-info=su`. `cargo xtask cpp stack-usage` computes the worst case of each wrapper over its call graph and fails if any call graph is unbounded (recursion, CPP-030, or an indirect call). The table is a CI artefact per target on every PR, and is published per release | Accepted (2026-10-19) |
| D16 | Sanitizers (CPP-051) | One Clang 23 host job with `-fsanitize=address,undefined,integer,implicit-conversion -fno-sanitize-recover=all` on all `aldis` code and tests, and libstdc++'s `_GLIBCXX_ASSERTIONS`. GoogleTest and nlohmann/json are built with ASan and UBSan only, not with `integer`, whose unsigned-overflow check would fire inside them. Host Clang builds use the system libstdc++ (GCC 14), so both compilers see the same standard library headers. Alternative: libc++ in its debug hardening mode, if the LLVM tarball ships it for the host | Accepted (2026-10-19) |
| D17 | CI structure (stable job names) | A separate workflow `.github/workflows/cpp.yml` (name "C++"), triggered like `ci.yml` and with **no path filters**, so its checks can be required without hanging on PRs that do not touch `cpp/`. **Amended 2026-10-19 (maintainer): Rust-only changes do not run the C++ pipeline.** A first job, `changes`, compares a pull request with its base; when nothing under `cpp/`, `test-vectors/`, `xtask/`, `cpp.yml`, or `.github/actions/setup-cpp/` changed, every `cpp-*` job is skipped. Pushes to `main` and manual runs always run everything. A final job `cpp-ok` passes when every gating job passed or was skipped, and is the one check to require. Job IDs carry a `cpp-` prefix and never change once required: `cpp-format`, `cpp-build` (host × compiler matrix), `cpp-tidy`, `cpp-sanitize`, `cpp-coverage`, `cpp-big-endian`, `cpp-bare-metal` (target × compiler matrix: build, link check, canaries, QEMU, stack table), `cpp-layers`, `cpp-codeql`, and the non-gating `cpp-leon3`. The `spec` job of `ci.yml` gains `cargo xtask trace --scope c0` (without `--strict` until the C0 exit, then with it, as M1 D8). The maintainer adds `cpp-ok` to branch protection once it is green on `main`. Alternative: the same jobs in `ci.yml`; one file, but track R and track C would both edit it. **Amended 2026-10-19 (OI-31): pull requests merge on a local `cargo xtask verify` record (VER-058).** Its C++ checks carry these job names (`cpp-build-<compiler>`, `cpp-bare-metal-<target>-<compiler>` for the matrix jobs), and it skips them under the same path rule as `changes` (`CPP_PATHS` in `xtask/src/verify/mod.rs`). `cpp.yml` runs nightly on both architectures (VER-059), without `changes` and `cpp-ok`; nothing is added to branch protection but `aldis/verify` | Accepted (2026-10-19) |
| D18 | Scope of the exit criteria and port baseline | (1) Besides the roadmap's list, C0 also traces **CPP-020, CPP-021, CPP-023, CPP-025, and CPP-026**, because `aldis_core` is where identifiers, `Result`/`Error`, the "parse, don't validate" types, and the first configuration type (`SecurityLayout`) are defined. CPP-022 (decoders) and CPP-024 (engines) wait for C1 and C3. If this is accepted, the roadmap §2 row is updated to match. (2) The C0 exit table lists only CPP IDs: `xtask trace` cannot tell a Rust artefact from a C++ one for shared IDs (GEN, CMN, API), so listing them would show them traced before any C++ exists. The C++ evidence for those requirements is the core vector run on every target (CPP-053) and the ported unit tests. (3) The port follows `aldis-core` at `0655f2a`. Later changes to `aldis-core` are ported in C0 if they merge before the exit, otherwise listed in §6 and ported in C1 | Accepted (2026-10-19) |
| D19 | Language standard and header-only code (CPP-001; OI-28) | CPP-001 says the standard is "not overridable by a consumer's settings", but header-only and template code (CPP-034, CPP-024) is compiled in the consumer's translation units, under the consumer's standard. Reading accepted with OI-28 (CPP-001 reworded): every Aldis build (presets, CI, the exerciser, the installed package's `cxx_std_20` requirement) is exactly C++20; `config.hpp` rejects anything older than C++20 with a `static_assert` on `__cplusplus`; a consumer on a later standard is not verified, and the installed package's documentation says so. Alternative: reject every standard other than C++20 in `config.hpp`, which would also reject C++23 projects that could otherwise use the library | Accepted (2026-10-19, OI-28) |

## 3. Work packages

Size is relative effort: **S** ≈ small, **M** ≈ medium, **L** ≈ large. WP-0 and WP-1 come first. WP-2 and WP-3 then run in parallel. WP-4 needs the first modules of WP-3, and WP-5 needs WP-4's generator. CI jobs (WP-6) are added as each check exists, not at the end.

```
WP-0 toolchains ─▶ WP-1 CMake project ─┬─▶ WP-2 xtask C++ support ─────────────────────────┐
                                       └─▶ WP-3 aldis_core: bytes → arith → error → … → layout ─┤
                                           WP-4 vector loader, runner, codegen ──────────────┤
                                           WP-5 exerciser, link check, QEMU ─────────────────┤
                                           WP-6 CI jobs, CodeQL, IKOS (grows with each WP) ──┴─▶ WP-7 exit review
```

### WP-0 Toolchains and pinning (S) — CPP-001, CPP-003

| Task | Output |
|---|---|
| `cpp/toolchains.lock`: GCC 14 (host and s390x cross), Clang/LLVM 23 tarball, Arm GNU Toolchain 14.x, xPack RISC-V GCC 14.x, BCC2, CMake, Ninja, QEMU, CodeQL CLI and pack versions; URLs and SHA-256 | file |
| `.github/actions/setup-cpp`: installs exactly the locked versions (tarballs into the tool cache, verified by checksum; apt packages with the exact version), prints every version | composite action |
| Configure-time version check in the presets (D1) | `cpp/cmake/AldisToolchainCheck.cmake` |
| Licence check of every tool against VER-055 (BCC2 in particular), recorded in the PR | PR description |

**Done when:** a CI job installs the whole set from the lock file on `ubuntu-24.04` and `ubuntu-24.04-arm`, and a changed checksum fails it.

### WP-1 CMake project, presets, toolchain files, lint configuration (M) — CPP-001, CPP-002, CPP-003, CPP-010, CPP-011, CPP-012, CPP-040…042, CPP-044

| Task | Output |
|---|---|
| Top-level project: C++20 with `CXX_STANDARD_REQUIRED ON` and `CXX_EXTENSIONS OFF`, `cxx_std_20` compile feature on every target, `aldis::core` `INTERFACE` target, header self-containment TUs (D7), options `ALDIS_BUILD_TESTS`, `ALDIS_BUILD_EXERCISER`, `ALDIS_CRC_TABLE` (counterpart of the `crc-table` feature), `ALDIS_TEST_HOOKS` (counterpart of `test-hooks`, never installed) | `cpp/CMakeLists.txt`, `cpp/src/…` |
| Warning flags of cpp.md §5 as one CMake function applied to every Aldis target, plus `-Wunsafe-buffer-usage` on Clang and `-fstack-usage` on GCC bare metal | `cpp/cmake/AldisWarnings.cmake` |
| Presets: `host-gcc`, `host-clang`, `host-clang-sanitize`, `host-clang-coverage`, `s390x-gcc`, `<target>-gcc` and `<target>-clang` for `cortex-m4f`, `cortex-r5f`, `rv32imac`, and `leon3-gcc`; each with a build and a test preset | `cpp/CMakePresets.json` |
| Toolchain files with the CPU and ABI flags of the Rust targets (`thumbv7em-none-eabihf`, `armv7r-none-eabihf`, `riscv32imac-unknown-none-elf`), `-ffreestanding`, `-fno-exceptions -fno-rtti`, `-ffunction-sections -fdata-sections` | `cpp/cmake/toolchains/*.cmake` |
| Install and export: `find_package(aldis)` with `aldis::core`; a test project in CI that consumes the installed package. `aldis.h` and `aldis.hpp` join the package in C1 and C5 (CPP-012) | `cpp/cmake/aldisConfig.cmake.in` |
| `cpp/.clang-format` (based on the LLVM style, 100 columns to match `rustfmt`) and `cpp/.clang-tidy` with the groups of cpp.md §5, `WarningsAsErrors: '*'`, naming rules (D7), include allowlist (D12). The first run on `aldis_core` will show which checks conflict with the profile (for example `modernize-use-trailing-return-type` is style only): each disable is a row with its reason in cpp.md §5, in the PR that disables it (CPP-041) | config files |
| Dev-only dependencies (D4) | `cpp/cmake/Dependencies.cmake` |

**Done when:** an empty `aldis::core` (just `config.hpp`) builds warning-free on every preset; a planted warning, a planted clang-tidy finding, a planted formatting error, and an `#include <vector>` in a library header each fail the right check (checked once, then reverted).

### WP-2 `xtask` support for C++ (M) — CPP-057, CPP-010, VER-020

| Task | Output |
|---|---|
| `trace` scans `.hpp`, `.cpp`, and `.h` under `cpp/` for `// req:`; artefacts labelled `test (C++) <path>:<line>` | `xtask/src/trace.rs` |
| `spec-lint` scans the same extensions, so a C++ test cannot cite an undefined ID | `xtask/src/spec.rs` |
| `cargo xtask cpp layers`: target graph from `cmake --graphviz`, checked against the crate layer table (CPP-010; the GEN-002 exceptions apply) | `xtask/src/cpp/…` |
| `cargo xtask cpp link-check`, `statics`, `unsafe-optouts` (D6, D9, D12); `coverage` (D14), `stack-usage` (D15), and `deviations` (D13) follow with their WPs | `xtask/src/cpp/…` |
| Self-tests for each new check, with `// req:` annotations | `xtask` tests |
| `cargo xtask trace --scope c0` in CI job `spec` (D17) | `ci.yml` |

**Done when:** a `// req: CPP-031` comment in a C++ test file shows up in `target/spec/trace.md`, and a reference to an undefined requirement ID in one fails `spec-lint`.

### WP-3 `aldis_core` (L) — CPP-020, CPP-021, CPP-023, CPP-025, CPP-026, CPP-030…034

Each module is done when it builds warning-free on every preset and passes clang-tidy; its Rust unit tests are ported to GoogleTest with the same `// req:` IDs, plus tests for C++-only behaviour; its vectors pass (WP-4); and it reaches 100 % coverage (D14). Every public item's documentation cites the CCSDS clause and requirement IDs, as the Rust item does (RS-015 applied to C++). Behaviour comes from the Rust module and the Blue Book clauses it cites; no protocol value is written from memory.

| Order | Header | Rust source | Content | Requirements |
|---|---|---|---|---|
| 1 | `config.hpp` | — | `ALDIS_LIFETIMEBOUND`, version macros, language-standard check (OI-28) | CPP-001 |
| 2 | `bytes.hpp` | — | Checked element access, `subspan`, copy/fill, `ByteReader`/`ByteWriter` (D9); the only opt-out from `-Wunsafe-buffer-usage` | CPP-031, GEN-013, GEN-020 |
| 3 | `arith.hpp` | `error.rs` (private conversions) | Wrapping, checked, saturating helpers and checked narrowing (D10) | CPP-032, GEN-024 |
| 4 | `error.hpp` | `error.rs` | `ErrorKind` (catalogue codes), `ErrorDetail`, `FieldId`, `Error`, `Result<T>`, `Result<void>` (D8); `code()`, `from_code()`, `name()`, `from_name()` | CPP-021, CPP-026, GEN-040, GEN-041 |
| 5 | `mode.hpp` | `mode.rs` | `DecodeMode { Strict, Lenient }`, default `Strict` | API-030 |
| 6 | `anomaly.hpp` | `anomaly.rs` | `Anomaly` concept over an `enum class` with an `AnomalyTraits<A>` table of stable bit indices; `static_assert` that indices are below 32 and unique (the counterpart of `anomaly_flags!`); `AnomalySet<A>` with `empty`, `insert`, `contains`, `is_empty`, `size`, `bits`, and iteration | GEN-043, GEN-081 |
| 7 | `bits.hpp` | `bits.rs` | `bits::get(span, bit_offset, width)`, `bits::set(…)` for fields up to 64 bits, big-endian bit numbering | GEN-050, GEN-051, CPP-033 |
| 8 | `ids.hpp` | `ids.rs` | `SpacecraftId<Bits>`, `VcId<Bits>`, `MapId<Bits>`, `Apid` (`is_idle`), `SeqCount`, `FrameSeqNum`, `PacketVersion`; `make` factories and `classify`/`next` (D8) | CPP-020, CPP-023, API-001, API-002 |
| 9 | `time.hpp` | `time.rs` | `Instant`, `Duration` (µs), saturating arithmetic, backwards = zero elapsed. Not named in the roadmap's C0 row, but part of `aldis-core` and used by every engine signature (cpp.md §3) | API-003 |
| 10 | `crc.hpp` | `crc.rs` | Bitwise CRC-16 and the 256-entry table (a `constexpr` array), incremental update, `crc16` selected by `ALDIS_CRC_TABLE`, `verify_fecf`, `syndrome`, `write_fecf` | CMN-001…004 |
| 11 | `oid.hpp` | `oid.rs` | `OidGenerator` (Fibonacci) and `OidGeneratorGalois`, `next_bit`, `next_octet`, `fill`; randomized test that both forms agree | CMN-020…023 |
| 12 | `counter.hpp` | `counter.rs` | `ModCounter` (1–64 bits), `next`, `increment`, `distance_to`, `classify` → `Continuity` | CMN-030, CMN-031, GEN-024 |
| 13 | `container.hpp` | `container.rs` | `FixedQueue<T, N>`, `ByteQueue<N>`, `FixedVec<T, N>` (D11); randomized tests against a model | GEN-010…012, API-061 |
| 14 | `security.hpp` | `security.rs` | `SecurityLayout` (validating factory), `Protocol`, `FrameCtx` (one factory per protocol, private members), `SecuredFields`, `SecurityHook` concept, `NoSecurity`; `FixedPatternSecurity` in a test-only header behind `ALDIS_TEST_HOOKS` | CPP-023, CPP-025, API-070, API-071, API-073, API-075 |
| 15 | `layout.hpp` | `layout.rs` | `OctetRange` (the counterpart of `Range<usize>`, start ≤ end by construction), `LayoutSpec`, `FrameLayout::compute`, accessors, `ranges()` | CMN-040, CMN-041 |

Also in WP-3: `aldis_core` is `constexpr` throughout where the Rust function is `const fn` or could be (CPP-034); every public function is `noexcept` (CPP-026, checked by clang-tidy and by a `static_assert(noexcept(…))` per public API in the tests); no recursion (`misc-no-recursion`), no mutable statics (`cppcoreguidelines-avoid-non-const-global-variables`, plus `xtask cpp statics`), no bit-field members (CPP-030, CPP-033).

**Done when:** all 15 headers are in; the ported tests, the vector runner (WP-4), and clang-tidy pass on both compilers; coverage is 100 %.

### WP-4 Vector loader, host runner, code generation (M) — CPP-053, CPP-034, TV-012

| Task | Output |
|---|---|
| Case model (literal types), loader with the exact-match rules of `aldis-testkit` (D5), and self-tests: a corrupted, an incomplete, and an over-specified vector each fail | `cpp/vectors/` |
| Check functions for the nine core operations: `bits_get`, `bits_set`, `counter_classify`, `counter_increment`, `crc16`, `error_code`, `frame_layout`, `oid_fill`, `oid_fill_sequence` | `cpp/vectors/checks/` |
| Host runner registered with CTest: one test per vector file, reporting failing case IDs | `aldis_vectors` |
| `aldis_vecgen`: writes `constexpr` case arrays into a generated header; build integration for cross presets (D5) | host tool |
| `static_assert` over a subset of each core file's cases (at least one success and one error case per operation), so constant evaluation checks them at compile time (CPP-034) | `cpp/tests/constexpr_vectors.cpp` |

**Done when:** all 77 live core cases pass on the host presets, under the sanitizers, and on `s390x`; a corrupted vector fails the run.

### WP-5 Freestanding exerciser, link check, QEMU (M) — CPP-050, CPP-011, CPP-053

| Task | Output |
|---|---|
| Start-up code and linker scripts per target, semihosting exit, `memcpy`/`memmove`/`memset`, optimisation barrier (D6) | `cpp/exerciser/` |
| Calls every public `aldis_core` API, with the generated vector cases where they exist and fixed inputs elsewhere (ids, containers, security, anomaly sets) | `cpp/exerciser/exercise.cpp` |
| Link check and the four canaries on each target and compiler, at `-O2` and `-Os` | CI `cpp-bare-metal` |
| QEMU runs: exit code reports pass/fail; one retry and a timeout around the QEMU step only (M0 risk table) | CI `cpp-bare-metal` |
| Stack table per target (D15) | artefact `cpp-stack-<target>` |
| LEON3 build, link check, and QEMU attempt (D2), non-gating | CI `cpp-leon3` |

**Done when:** the exerciser links on all three targets with both compilers; each canary fails to link with its symbol; under QEMU it passes every case and fails when a vector is corrupted.

### WP-6 CI jobs and static analysis (M) — CPP-040…044, CPP-051, CPP-052, VER-052, VER-055

| Task | Output |
|---|---|
| `cpp.yml` with the jobs of D17, each added as soon as its check exists | workflow |
| Sanitizer job (D16) | `cpp-sanitize` |
| Coverage gate and MC/DC report (D14) | `cpp-coverage`, `xtask cpp coverage` |
| CodeQL Coding Standards job and deviation log (D13); every initial finding fixed or logged with its reason | `cpp-codeql`, `docs/verification/cpp-deviations.md` |
| **IKOS evaluation** (OI-22). Scope: the current IKOS release on `aldis_core` through the exerciser's entry point and one harness per module, with the default numerical domain and one relational domain. Questions: does its LLVM front end (older than Clang 23) compile the C++20 headers; which checks it proves (buffer overflow, division by zero, signed and unsigned overflow, null dereference, uninitialized reads, shift count); run time; how many warnings, and how many are false positives. Output: an evaluation record with a recommendation (adopt as a CI gate, run nightly as a report, or do not adopt) for the maintainer to decide | `docs/verification/ikos-evaluation.md` |
| Analysis records for every A/I requirement in the exit table (CPP-001…003, CPP-010…012, CPP-030…033, CPP-040…044, CPP-050, CPP-052, CPP-056, CPP-057) | `docs/verification/analysis-records.md` |

**Done when:** every job is green on `main` and has failed once on a planted violation (warning, clang-tidy finding, formatting, include outside the allowlist, sanitizer finding, uncovered branch, CodeQL finding without a log entry, canary).

### WP-7 Exit review (S) — CPP-057, VER-020

| Task | Output |
|---|---|
| `cargo xtask trace --scope c0 --strict` in CI | `ci.yml` |
| §4 results column; roadmap §2 row (C0 complete; D18 scope); open issues updated (OI-08, OI-22, OI-27, OI-28) | docs |
| Changelog fragment for the C0 exit | `changelog.d/` |

## 4. Exit criteria

C0 is complete when every item below is green on `main`. The criteria apply to `aldis_core`; later modules meet the same requirements in their own milestones.

| Area | Criteria |
|---|---|
| Toolchain, build, packaging | CPP-001, CPP-002, CPP-003, CPP-010, CPP-011, CPP-012 (package exports `aldis::core`) |
| API shapes in `aldis_core` | CPP-020, CPP-021, CPP-023, CPP-025, CPP-026 (D18) |
| Language subset and safety | CPP-030, CPP-031, CPP-032, CPP-033, CPP-034 |
| Warnings, lints, formatting, coding standards | CPP-040, CPP-041, CPP-042, CPP-043, CPP-044 |
| Freestanding link check, sanitizers, coverage, vectors | CPP-050, CPP-051, CPP-052 (100 % for `aldis_core`), CPP-053 |
| Stack usage | CPP-056 (table for each bare-metal target) |
| Trace | CPP-057 (`cargo xtask trace --scope c0 --strict` passes) |
| Conformance | All live cases of `test-vectors/core/` pass on every C++ CI target of D3, with both compilers |
| Evaluation | IKOS evaluated and the recommendation recorded (OI-22); LEON3 result reported against OI-08 |
| Docs | Roadmap §2 row updated; changelog fragment; deviation log and analysis records current; open issues updated |

Not in C0: CPP-022 (decoders, C1), CPP-024 (engines, C3), CPP-035 and the C ABI layer (C1), CPP-054 (native fuzz and property tests, C1), CPP-055 (mutation testing, C1), CPP-060 and CPP-061 (differential tests, C1, although D1 already picks the Clang version CPP-061 needs), and VER-053 for any module.

## 5. Risks

| Risk | Mitigation |
|---|---|
| Clang bare-metal builds with the GCC toolchains' headers and `libgcc` hit driver or multilib problems | Try Clang on `thumbv7em` first in WP-1; fall back to Arm Toolchain for Embedded for Arm (D2 alternative) and record the change |
| `libgcc` or our own objects reference the Arm unwinder (`__aeabi_unwind_cpp_pr0`) even with `-fno-exceptions`, so the link check fails for a reason that is not a defect | Build with `-fno-unwind-tables -fno-asynchronous-unwind-tables`; inspect which object pulls the symbol. Never define the symbol in the exerciser: that would make the check vacuous |
| The CodeQL Coding Standards packs are qualified for C++14/17; C++20 code (concepts, `std::span`, `constexpr` unions) may produce many findings or extraction errors | Classify by rule in WP-6; log C++20-only findings as such (CPP-043); keep the C++20 features used to those OI-19 lists |
| IKOS cannot compile C++20 with its LLVM version | That is a valid evaluation result; record it and the version tried |
| Coverage misses templates that are never instantiated | D14: the gate checks that every public template is instantiated by the tests |
| `aldis-core` changes during C0 (M1 additions, review fixes), so the port chases a moving reference | D18 (3): baseline commit, port list in §6, vectors as the contract |
| No CI-usable QEMU machine for Cortex-R | Same fallback as M1 D6: link check only, recorded as a deviation from CPP-053 |
| Toolchain downloads make CI slow or flaky | Cache the tarballs by checksum; one retry around downloads only |
| `-fsanitize=integer` reports intended wrap-around | D10: helpers written without overflow; any exemption is limited to them (CPP-051) |

## 6. Divergences and pending ports

Roadmap §6.5: divergences from the Rust crates found during C0 are listed here until a track-R fix merges.

| Item | Status |
|---|---|
| *(none yet)* | |
