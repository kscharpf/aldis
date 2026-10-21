# C ABI profile

This profile defines the C interface. The Rust reference implementation exposes it through `aldis-ffi`, and the native C++ and C implementations SHALL expose the same interface. Because both expose one interface, a C or C++ application can link either library without source changes.

**Terms and acronyms:** see the [glossary](../glossary.md).

## 1. Linkage and artefacts

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CABI-001 | `aldis-ffi` SHALL build a static library (`libaldis_ffi.a`) for every target, and a shared library (`libaldis_ffi.so` / `.dylib` / `.dll`) for hosted targets. | — | A |
| CABI-002 | The public header `include/aldis/aldis.h` SHALL be generated from the Rust sources (cbindgen). It SHALL be committed, and CI SHALL fail if the regenerated header differs. | — | A |
| CABI-003 | The header SHALL be valid C99 and C++11 (VER-050). It SHALL wrap declarations in `extern "C"` under `__cplusplus`, and SHALL include only `<stdint.h>`, `<stddef.h>`, and `<stdbool.h>`. | — | A |
| CABI-004 | The library SHALL NOT require a C runtime heap, threads, or the Rust standard library on `no_std` targets. It SHALL require the integrator to provide nothing beyond `memcpy`/`memset` equivalents. | GEN-010 | A |

## 2. Naming

| Kind | Pattern | Example |
|---|---|---|
| Functions | `aldis_<module>_<verb>[_<object>]` | `aldis_spp_decode`, `aldis_fop1_handle_clcw` |
| Types | `aldis_<module>_<name>_t` | `aldis_spp_header_t`, `aldis_fop1_t` |
| Enumerations / constants | `ALDIS_<MODULE>_<NAME>` | `ALDIS_ERR_CRC_MISMATCH`, `ALDIS_SPP_APID_IDLE` |
| Macros | `ALDIS_` prefix | `ALDIS_ABI_VERSION` |

## 3. Types

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CABI-010 | Only fixed-width integer types (`uint8_t` … `uint64_t`, `int32_t`), `size_t`, `bool`, and POD structs of those types SHALL cross the ABI. Enumerations SHALL be passed as fixed-width integers with `#define`d or `enum` constants, never as C `enum` types in struct fields. | — | I |
| CABI-011 | Buffers SHALL be passed as `(const uint8_t *ptr, size_t len)` or `(uint8_t *ptr, size_t cap, size_t *out_len)`. | API-060, API-061 | I |
| CABI-012 | Views SHALL be returned as POD structs containing decoded field values, plus **offset/length pairs** into the caller's input buffer (not pointers), so that they remain valid if the buffer is moved. | API-010, GEN-013 | T |
| CABI-013 | `Instant` and `Duration` SHALL be `uint64_t` microseconds. | API-003 | I |

## 4. Errors

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CABI-020 | Every fallible function SHALL return `int32_t`: `0` (`ALDIS_OK`) for success, or `-code` using the [error catalogue](../03-api-model.md#9-error-catalogue). For example, `ALDIS_ERR_BUFFER_TOO_SMALL = -1`. | GEN-040 | V |
| CABI-021 | Additional error detail (field code (API-082), `needed` octets, expected/found) SHALL be available through an optional `aldis_error_detail_t *` out-parameter. Passing `NULL` SHALL be allowed. | GEN-041 | T |
| CABI-022 | Every pointer parameter SHALL be checked for `NULL`, and every length for consistency. Violations SHALL return `ALDIS_ERR_INVALID_FIELD`. They SHALL never cause undefined behavior inside the library. | GEN-020 | T |

## 5. Memory model

Engines are opaque objects placed in **caller-owned storage**. No heap is used.

```c
/* Storage requirements are compile-time constants (and also queryable at run time). */
#define ALDIS_FOP1_STORAGE_SIZE   /* generated */
#define ALDIS_FOP1_STORAGE_ALIGN  /* generated */

size_t  aldis_fop1_storage_size(void);
size_t  aldis_fop1_storage_align(void);

int32_t aldis_fop1_init(void *storage, size_t storage_len,
                        const aldis_fop1_config_t *cfg,
                        aldis_fop1_t **out_handle,
                        aldis_error_detail_t *err);
void    aldis_fop1_deinit(aldis_fop1_t *h);   /* no-op for memory; marks handle invalid */
```

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CABI-030 | Engine capacities (const generics in Rust) SHALL be fixed when the library is built. They SHALL be read from a build configuration file (`aldis-ffi/capacities.toml`) and emitted as `ALDIS_*_MAX_*` constants in the header. `aldis_limits(aldis_limits_t *)` SHALL report them at run time. | GEN-011 | A |
| CABI-031 | `*_init` SHALL validate storage size and alignment and fail with `ALDIS_ERR_BUFFER_TOO_SMALL` / `ALDIS_ERR_INVALID_FIELD` if they are inadequate. | — | T |
| CABI-032 | A handle SHALL carry a magic/generation tag. Calls with a deinitialized or foreign handle SHALL fail with `ALDIS_ERR_INVALID_HANDLE` where detectable. | — | T |
| CABI-033 | Configuration structs SHALL begin with a `uint32_t struct_size` field set by the caller, for forward-compatible extension. | GEN-080 | T |

## 6. Engines without callbacks; security hooks with callbacks

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CABI-040 | Engine inputs and outputs SHALL follow the pull model of [03 §5](../03-api-model.md#5-engines-the-sans-io-contract) (`aldis_fop1_poll_transmit`, `aldis_fop1_poll_event`, …). No engine API SHALL take a callback. | API-050 | I |
| CABI-041 | Security hooks SHALL be passed as a `const aldis_security_ops_t *` vtable `{ protect, verify, user_ctx }`. The library SHALL call them only synchronously, within the API call that needs them, and never concurrently. | API-074 | T |

## 7. Threading

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CABI-050 | Functions on distinct handles SHALL be callable concurrently from different threads. Calls on the same handle SHALL be serialized by the caller (GEN-033). Codec functions are stateless and fully reentrant. | GEN-033 | I |

## 8. Versioning

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CABI-060 | The header SHALL define `ALDIS_ABI_VERSION_MAJOR/MINOR/PATCH`. `aldis_abi_version()` SHALL return the library's version. The major version SHALL change on any ABI-breaking change. | GEN-080 | T |

## 9. Rules for native C/C++ implementations

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CABI-070 | A native C or C++ implementation SHALL expose this exact header, generated or hand-maintained, verified by an ABI comparison against the Rust-generated header in CI. A native C++ implementation SHALL provide it as an `extern "C"` layer over its C++ API. The header is also the interface through which the differential tests call the native implementation (VER-053). | — | A |
| CABI-071 | Native implementations SHALL meet the [design rules](../02-design-rules.md) in their language. That means no heap, no recursion, bounded loops, and no undefined behavior (verified with sanitizers and static analysis, VER-051/052). They SHALL pass the full [test-vector suite](../07-test-vectors.md) and the differential tests against the Rust reference (VER-053). | — | V |
| CABI-072 | The C++ API SHALL be a header-only, idiomatic wrapper (`aldis.hpp`) over the C ABI: RAII handle types with storage in `alignas(N) std::byte[N]` members (`std::aligned_storage` is deprecated in C++23), `std::span` views, and `expected`-like results, in C++20 (OI-19). Engine capacities SHALL be template parameters, checked with `static_assert` against the build-time `ALDIS_*_MAX_*` constants (CABI-030, OI-20). It SHALL NOT use exceptions. The native C++ implementation ([C++ profile](cpp.md)) SHALL keep the same C++ wrapper API. | — | T |
