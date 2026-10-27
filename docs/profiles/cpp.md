# C++ profile

This profile binds the language-neutral specification to the **native C++ implementation**. The Rust crates remain the reference implementation and the recommended choice. The native C++ library exists for legacy flight software that cannot easily take a Rust toolchain. It exposes the same C ABI (CABI-070) and the same C++ wrapper API (CABI-072) as the Rust-backed library, so an application can switch between them without source changes.

**Terms and acronyms:** see the [glossary](../glossary.md).

**Status:** Withdrawn (2026-10-27, `chore/retire-track-c`). Track C retired from v1 and deferred to roadmap Phase 3. Native C++ codebase archived at git tag `c0-archive`. All 34 `CPP-*` requirements below are withdrawn.

## 0. How the C++ implementation is assured

The evidence for the C++ library comes from two places, and neither replaces the other:

| Property | Evidence |
|---|---|
| **Correct behavior** (wire octets, errors, anomalies, engine outputs) | The shared [test vectors](../07-test-vectors.md) (CPP-053), and differential tests against the Rust crates on generated inputs (VER-053, §7). Equivalence lets the Blue Book reviews, models, and vectors behind the Rust crates also stand behind the C++ library, for the inputs tested. |
| **Absence of undefined behavior, aborts, and allocation** | C++-specific evidence only: the language subset (§4), warnings and static analysis (§5), the link check, sanitizers, coverage, fuzzing, and mutation testing (§6). The Rust panic-freedom, Miri, and Clippy results say nothing about C++ code. |

## 1. Toolchain and language

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CPP-001 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-002 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-003 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |

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
| CPP-010 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-011 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-012 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |

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
| CPP-020 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-021 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-022 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-023 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-024 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-025 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-026 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |

## 4. Language subset and safety rules

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CPP-030 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-031 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-032 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-033 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-034 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-035 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |

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
| CPP-040 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-041 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-042 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-043 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-044 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |

## 6. Verification

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CPP-050 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-051 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-052 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-053 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-054 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-055 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-056 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-057 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |

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
| CPP-060 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CPP-061 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
