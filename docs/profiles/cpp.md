# C++ profile

This profile binds the language-neutral specification to the **native C++ implementation**. The Rust crates remain the reference implementation and the recommended choice. The native C++ library exists for legacy flight software that cannot easily take a Rust toolchain. It exposes the same C ABI (CABI-070) and the same C++ wrapper API (CABI-072) as the Rust-backed library, so an application can switch between them without source changes.

**Terms and acronyms:** see the [glossary](../glossary.md).

**Status:** Draft, 2026-10-19. No C++ code exists yet; the work is planned as track C of the [roadmap](../08-roadmap.md) §2. The language is C++20 (OI-19) and engine capacities are template parameters (OI-20). Every tool is free (VER-055); the coding guidelines are CERT C++ and AUTOSAR C++14 (VER-052, OI-22).

## 0. How the C++ implementation is assured

The evidence for the C++ library comes from two places, and neither replaces the other:

| Property | Evidence |
|---|---|
| **Correct behavior** (wire octets, errors, anomalies, engine outputs) | The shared [test vectors](../07-test-vectors.md) (CPP-053), and differential tests against the Rust crates on generated inputs (VER-053, §7). Equivalence lets the Blue Book reviews, models, and vectors behind the Rust crates also stand behind the C++ library, for the inputs tested. |
| **Absence of undefined behavior, aborts, and allocation** | C++-specific evidence only: the language subset (§4), warnings and static analysis (§5), the link check, sanitizers, coverage, fuzzing, and mutation testing (§6). The Rust panic-freedom, Miri, and Clippy results say nothing about C++ code. |

## 1. Toolchain and language

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CPP-001 | Every Aldis build (presets, CI, exerciser) and the compile requirement of the installed CMake package SHALL use C++20 (OI-19). The headers SHALL reject a standard older than C++20 at compile time. A consumer MAY compile the headers under a later standard; that use is not verified, and the package documentation SHALL say so (OI-28). The library SHALL build with pinned versions of both GCC and Clang. Changing the standard or a pinned compiler version is a configuration-controlled change recorded in the changelog. | — | A |
| CPP-002 | Layers 0–2 SHALL be compiled with `-fno-exceptions -fno-rtti`. They SHALL include only standard headers on an allowlist of header-only, allocation-free headers (initially `<array>`, `<bit>`, `<concepts>`, `<cstddef>`, `<cstdint>`, `<limits>`, `<optional>`, `<span>`, `<type_traits>`, `<utility>`). The C ABI layer MAY also include `<new>` for placement new (CPP-035). CI SHALL reject any other include. | GEN-001, GEN-010 | A |
| CPP-003 | Layers 0–2 SHALL have no third-party runtime dependencies. Dev-only dependencies (test framework, fuzzing and property-test libraries, JSON parser for the vector runner) SHALL be pinned to a commit hash. | GEN-001 | A |

## 2. Build and layout

The C++ implementation lives in `cpp/` at the repository root, outside the Cargo workspace (like `fuzz/` and `exerciser/`). It is built with **CMake** and Ninja, configured through `cpp/CMakePresets.json`, with one toolchain file per bare-metal target of the [CI matrix](../06-verification.md#4-targets-and-ci-matrix). Modules mirror the crates:

| Crate | C++ module | Namespace | CMake target | Headers |
|---|---|---|---|---|
| `aldis-core` | `aldis_core` | `aldis::core` | `aldis::core` | `include/aldis/core/` |
| `aldis-time` | `aldis_time` | `aldis::time` | `aldis::time` | `include/aldis/time/` |
| `aldis-spp`, `aldis-epp`, `aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp` | `aldis_<std>` | `aldis::<std>` | `aldis::<std>` | `include/aldis/<std>/` |
| `aldis-link`, `aldis-cop1` | `aldis_link`, `aldis_cop1` | `aldis::link`, `aldis::cop1` | `aldis::link`, `aldis::cop1` | `include/aldis/{link,cop1}/` |
| `aldis-ffi` | `aldis_capi` (CPP-035) | — (`extern "C"`) | `aldis::capi` | `include/aldis/aldis.h` |

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CPP-010 | Each module SHALL be its own CMake library target. A target SHALL link only targets of lower layers, with the same exceptions as GEN-002. CI SHALL check the target graph. | GEN-002, GEN-003 | A |
| CPP-011 | Every module SHALL build for each bare-metal target of the CI matrix with exceptions and RTTI disabled and without the hosted standard library. | GEN-005 | A |
| CPP-012 | The build SHALL install a CMake package (`find_package(aldis)`) that exports one target per module and the C ABI library, together with `aldis.h` and `aldis.hpp`. | — | A |

## 3. API shapes

```cpp
namespace aldis::core {

// Results: no exceptions and no unchecked access (CPP-021).
template <typename T>
class [[nodiscard]] Result {
public:
    constexpr const T* ok() const noexcept;          // nullptr on error
    constexpr const Error* error() const noexcept;   // nullptr on success
};

// Identifiers: private representation, checked construction only (CPP-020).
class Apid {
public:
    static constexpr Result<Apid> make(std::uint16_t raw) noexcept;
    constexpr std::uint16_t value() const noexcept;
    friend constexpr bool operator==(Apid, Apid) noexcept = default;
private:
    constexpr explicit Apid(std::uint16_t raw) noexcept : raw_{raw} {}
    std::uint16_t raw_;
};

}  // namespace aldis::core

namespace aldis::spp {

// Views borrow input (CPP-022).
class SpacePacket {
public:
    static constexpr core::Result<SpacePacket> decode(
        std::span<const std::byte> bytes ALDIS_LIFETIMEBOUND, core::DecodeMode mode) noexcept;
    static constexpr core::Result<std::size_t> peek_length(std::span<const std::byte> prefix) noexcept;
    constexpr core::Apid apid() const noexcept;
    constexpr std::span<const std::byte> data_field() const noexcept;
    constexpr core::AnomalySet<SppAnomaly> anomalies() const noexcept;
    constexpr std::size_t len() const noexcept;
    constexpr SpacePacketHeader header() const noexcept;   // owned (API-014)
};

}  // namespace aldis::spp

namespace aldis::cop1 {

// Engines: capacities as template parameters (OI-20), hooks as concepts (CPP-024).
template <std::size_t Sent, std::size_t MaxFrame, core::SecurityHook H = core::NoSecurity>
class Fop1 {
public:
    static core::Result<void> init(Fop1& storage, const Fop1Config& cfg) noexcept;   // in place
    core::Result<RequestHandle> request_ad(core::Instant now, std::span<const std::byte> fdu) noexcept;
    core::Result<void> handle_clcw(core::Instant now, tc::Clcw clcw) noexcept;
    void handle_timeout(core::Instant now) noexcept;
    core::Result<std::optional<Transmit>> poll_transmit(std::span<std::byte> out) noexcept;
    std::optional<Event> poll_event() noexcept;
    std::optional<core::Instant> poll_timeout() const noexcept;
    core::Result<void> transmit_done(TransmitHandle h, LowerResponse r) noexcept;
    Fop1Status status() const noexcept;
};

}  // namespace aldis::cop1
```

`ALDIS_LIFETIMEBOUND` expands to `[[clang::lifetimebound]]` on Clang and to nothing elsewhere, because GCC warns on unknown scoped attributes.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CPP-020 | Identifier types SHALL be classes with a private representation. Sub-width identifiers SHALL be constructed only through a `static constexpr` factory that returns `Result`. Full-width identifiers SHALL provide a `constexpr explicit` constructor. No identifier SHALL convert implicitly to or from an integer type, and the public API SHALL NOT provide an unchecked constructor that can produce an out-of-range value. | API-001 | T |
| CPP-021 | `Error` SHALL be a trivially copyable struct of `ErrorKind`, an optional field identifier carrying its field code (API-082), and `ErrorDetail`, mirroring RS-011. `ErrorKind` SHALL be an `enum class` with a fixed underlying type whose enumerators carry the codes of the [error catalogue](../03-api-model.md#9-error-catalogue). `Result<T>` SHALL be `[[nodiscard]]`, and no accessor SHALL throw, abort, or have undefined behavior when called in the wrong state. | GEN-040, GEN-041 | T |
| CPP-022 | Decoders SHALL take input as `std::span<const std::byte>` marked `ALDIS_LIFETIMEBOUND`, and return views whose sub-spans lie within that input. Overloads that would bind a view to a temporary owning container SHALL be deleted. | GEN-013, API-010 | T |
| CPP-023 | **Parse, don't validate**, as RS-020 states it for Rust: a public type SHALL be able to hold only values valid for it; data members of types with an invariant SHALL be private; untrusted input SHALL be checked once, by the factory or decoder that produces the type. | API-001, GEN-060 | I |
| CPP-024 | Engines SHALL be class templates whose capacities are template parameters (OI-20), so that `sizeof` gives their footprint. Security hooks SHALL be a template parameter constrained by a `SecurityHook` concept, defaulting to `NoSecurity`, with no virtual dispatch. An engine SHALL be initializable in caller-provided storage, without a temporary copy of the engine on the stack. | GEN-011, GEN-012, API-073 | T |
| CPP-025 | Configuration types SHALL have private data members, validating factories, a defaulted `operator==`, and no pointer or reference members. They SHALL be trivially copyable, and constructible in constant evaluation where their validation is. | GEN-060, API-041 | T |
| CPP-026 | Every public function SHALL be `noexcept`. | GEN-020 | I |

## 4. Language subset and safety rules

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CPP-030 | Layers 0–2 SHALL NOT use `new` or `delete` (except placement new in the C ABI layer, CPP-035), `throw`, `try`, `dynamic_cast`, or `typeid`. They SHALL NOT have mutable objects of static or thread storage duration, or static objects with dynamic initialization. No function SHALL be recursive, directly or indirectly. | GEN-010, GEN-022, GEN-031 | A |
| CPP-031 | Pointer arithmetic, subscripting of raw pointers, and unchecked element access SHALL appear only in one audited byte-access module of `aldis_core`. All other code SHALL read and write octets through that module's bounds-checked cursors and span operations, which return `Result`. Clang's `-Wunsafe-buffer-usage` SHALL be enabled for every module, with its opt-out confined to that one module. | GEN-013, GEN-020 | A |
| CPP-032 | Arithmetic on wire fields, counters, and lengths SHALL use the `aldis_core` wrapping, checked, and saturating helpers, which state the field's modulus (GEN-024). Integer promotion turns arithmetic on narrow unsigned types into `int` arithmetic, so no other code SHALL compute with them directly. Implicit narrowing and sign conversions SHALL be compile errors (`-Wconversion -Wsign-conversion`). | GEN-020, GEN-024 | A/T |
| CPP-033 | Wire fields SHALL NOT be accessed through C++ bit-field members, or by copying or casting structs onto octets. All field extraction and insertion SHALL go through the `aldis_core` bit-field primitives. | GEN-050, GEN-051 | I |
| CPP-034 | Layer 0 and Layer 1 codecs SHALL be `constexpr`. A subset of each module's vectors SHALL be evaluated in `static_assert`, so that constant evaluation rejects undefined behavior on those inputs at compile time. | GEN-020, API-020 | T |
| CPP-035 | The C ABI (CABI-070) SHALL be implemented in a single module, `aldis_capi`, of `extern "C"` functions over the C++ API. It is the only module that converts pointer/length pairs into spans, and SHALL check every pointer and length before it does (CABI-022). No C++ type SHALL cross the ABI. | CABI-010, CABI-022 | I/T |

## 5. Formatting and lint configuration

Compiler flags for every module (CPP-040):

```cmake
# GCC and Clang
-fno-exceptions -fno-rtti
-Wall -Wextra -Wpedantic -Werror
-Wconversion -Wsign-conversion -Wshadow -Wold-style-cast -Wcast-align
-Wnull-dereference -Wdouble-promotion -Wimplicit-fallthrough -Wswitch-enum -Wundef
# Clang only
-Wunsafe-buffer-usage        # opt-out only in the aldis_core byte-access module (CPP-031)
# GCC only
-fstack-usage                # bare-metal builds (CPP-056)
```

clang-tidy (`cpp/.clang-tidy`, CPP-041) enables the groups below. Checks within them that are disabled are listed with their reason in this section, as RS-019 does for Clippy.

```yaml
Checks: >
  bugprone-*, cert-*, clang-analyzer-*, cppcoreguidelines-*, hicpp-*,
  misc-*, modernize-*, performance-*, portability-*, readability-*
WarningsAsErrors: '*'
```

| Disabled check | Reason |
|---|---|
| `modernize-use-trailing-return-type` | Style only: it would rewrite every declaration into `auto f() -> T`, which the API shapes of §3 do not use. No safety rule depends on it |
| `readability-identifier-length` | Style only: it rejects one- and two-letter names such as `a`, `b`, `i`, `n`, and `s` in short scopes, which the code shares with `aldis-core` (and with the arithmetic it implements). Neither CERT C++ nor AUTOSAR C++14 sets a minimum length |
| `bugprone-easily-swappable-parameters` | Arithmetic and range helpers take operands of one type by nature (`checked_add(a, b)`, `wrapping_sub(a, b, bits)`), and the codec signatures mirror `aldis-core`. Where a mix-up matters, the parameters already have distinct types (identifiers, `OctetRange`, `SecurityLayout`), and the tests and vectors cover the order |

Tests and tools (`cpp/tests/.clang-tidy`, and its copy `cpp/vectors/.clang-tidy` for the vector loader, runner, and generator; both inherit the configuration above) also disable the checks below, mostly because GoogleTest's `TEST` macros expand to code they flag. Library code never has these exemptions.

| Disabled in tests and tools | Reason |
|---|---|
| `cert-err58-cpp` | `TEST` registers each test through a static object whose initialization may throw; tests run with exceptions enabled (D4 of the [C0 plan](../milestones/c0-foundations.md)) |
| `cppcoreguidelines-avoid-non-const-global-variables` | `TEST` defines a mutable static `test_info_` per test |
| `cppcoreguidelines-owning-memory` | `TEST` allocates its test factory with `new` |
| `cppcoreguidelines-special-member-functions`, `hicpp-special-member-functions` | `TEST` classes delete copy operations without declaring move operations |
| `cppcoreguidelines-avoid-magic-numbers`, `readability-magic-numbers` | Test inputs and expected values are literal by design, often copied from the Rust tests or a vector |
| `readability-function-cognitive-complexity` | Each `EXPECT_*` expands to branches, so a test with many expectations exceeds the threshold without being complex |
| `portability-restrict-system-includes` (allow-list widened to `*`) | Tests and tools use the test framework, the JSON parser, and the hosted standard library (D4, D12) |

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CPP-040 | Every module SHALL build without warnings on both pinned compilers with the flags above. | VER-001 | A |
| CPP-041 | clang-tidy SHALL run in CI on every module with `cpp/.clang-tidy`, with all findings treated as errors. A check SHALL be disabled for the whole code base only by adding it, with its reason, to the table above. | VER-001 | A |
| CPP-042 | A warning or clang-tidy finding SHALL be suppressed only on the smallest region, with `// NOLINT(<check>): <reason>` or a `NOLINTBEGIN`/`NOLINTEND` pair naming the check, or the compiler's diagnostic push/pop. File-, directory-, or build-wide suppressions SHALL NOT be added without updating this section. | VER-001 | A/I |
| CPP-043 | The CodeQL Coding Standards query packs for AUTOSAR C++14 and CERT C++ SHALL run in CI on Layers 0–2 and the C ABI layer, with every finding fixed or recorded in a deviation log under `docs/verification/` (VER-052). Findings on C++20-only constructs, which the packs are not qualified for, SHALL be reviewed by hand. | VER-052, VER-055 | A |
| CPP-044 | C++ sources SHALL be formatted with `clang-format` using `cpp/.clang-format`, checked in CI. | VER-004 | A |

## 6. Verification

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CPP-050 | For each bare-metal target, CI SHALL build a freestanding C++ **exerciser** that calls every public API with inputs from the vectors. It SHALL link with no definition of `operator new`/`delete`, `__cxa_throw`, `__cxa_allocate_exception`, `__cxa_guard_acquire`/`release`, `__cxa_pure_virtual`, `abort`, the assertion handlers, or the unwinder personality routines. A successful link shows that no allocation, exception, abort, or guarded static is reachable. A canary build that calls one of them SHALL fail to link. The check does not show the absence of undefined behavior, which rests on CPP-031, CPP-032, CPP-051, and static analysis (CPP-043; IKOS is evaluated in C0, OI-22). | GEN-020, GEN-021 | A |
| CPP-051 | All host tests, the vector runner, and the fuzz and property tests SHALL also run under AddressSanitizer and UndefinedBehaviorSanitizer with recovery disabled, under Clang's `integer` and `implicit-conversion` sanitizers, and with the standard library's hardening assertions enabled. The `aldis_core` wrapping helpers SHALL be the only functions exempt from the unsigned-overflow check. | GEN-020, CABI-071 | T |
| CPP-052 | Coverage of Layers 0–2 SHALL meet VER-024 (100 % statement and branch), measured with Clang source-based coverage on the host, with waivers as VER-024 defines. An MC/DC report SHALL be published with each release for information (OI-09). | VER-024 | A |
| CPP-053 | A C++ vector runner SHALL run the full shared vector suite on every CI target. On bare-metal targets, the vectors SHALL be converted at build time into constant data and checked by a freestanding runner under QEMU. | VER-021, CABI-071 | V |
| CPP-054 | Every decoder and engine input SHALL have a native libFuzzer target asserting what VER-022 asserts, named as the Rust target for the same input. Invariants internal to the C++ code (GEN-042 on rejected inputs, container invariants) SHALL have property tests. Every crash SHALL become a regression vector. | VER-022, VER-023 | F |
| CPP-055 | Mutation testing (Mull or an equivalent LLVM-based tool) SHALL run on Layers 0–2 before each release, with the vector runner and native tests as the test command. Surviving mutants SHALL be triaged in a record under `docs/verification/`. | VER-025 | A |
| CPP-056 | Stack usage of every public function SHALL be measured for each bare-metal target (`-fstack-usage` or an equivalent static analysis) and published as a table per release. | VER-011 | A |
| CPP-057 | C++ tests SHALL name the requirements they cover with `// req: <ID>` comments, and the trace (VER-020) SHALL scan C++ sources and headers. | VER-020 | A |

`cargo xtask spec-lint` and `cargo xtask trace` scan only `md`, `rs`, `json`, `yml`, `yaml`, `toml`, and `py` files today. CPP-057 needs them to scan `cpp/` (`.hpp`, `.cpp`, `.h`) as well.

## 7. Differential testing against the Rust reference

VER-053 requires every native implementation to match the Rust crates on generated inputs. For C++ this is done in a separate Cargo workspace, `difftest/` (dev-only, like `fuzz/`):

- `build.rs` builds the C++ library with CMake, and the C ABI bindings are generated from `aldis.h`. The bridge crate is the one tooling crate allowed `unsafe` (GEN-071).
- The Rust side links the Rust crates directly, not `aldis-ffi`, because `aldis-ffi` exports the same `aldis_*` symbols as the C++ library.
- **Codecs:** property tests draw inputs from the shared strategies (VER-054), call both implementations, and compare results field by field. Shrinking reports a minimal diverging input, which becomes a regression vector.
- **Engines:** state-machine property tests (`proptest-state-machine`) generate sequences of `handle_*`, `transmit_done`, and `handle_timeout` calls with their `Instant`s, drive the Rust and C++ engines in lockstep, and compare every `poll_*` output. Determinism (GEN-030) makes the comparison exact.
- **Fuzzing:** cargo-fuzz targets do the same on raw bytes. The C++ library is built with `-fsanitize=fuzzer-no-link` so that libFuzzer also gets coverage feedback from the C++ code.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CPP-060 | The differential harness SHALL link the Rust codec and engine crates directly and SHALL NOT link `aldis-ffi`. | VER-053 | I |
| CPP-061 | The C++ library SHALL be built for the differential fuzz targets with Clang of the same LLVM major version as the Rust toolchain that builds them, so that sanitizer runtimes and coverage instrumentation are compatible. | VER-053 | A |
