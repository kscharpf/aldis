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
| CABI-010 | Only fixed-width integer types (`uint8_t` … `uint64_t`, `int32_t`), `size_t`, `bool`, pointers to these types and to the library's structs, and POD structs of those types SHALL cross the ABI. A struct member SHALL be a pointer only as a `const` pointer to an input buffer, paired with the buffer's length, and only in a per-call input struct that the library reads during the call and does not keep (`aldis_tm_content_t`, §11). Enumerations SHALL be passed as fixed-width integers with `#define`d or `enum` constants, never as C `enum` types in struct fields. | — | I |
| CABI-011 | Buffers SHALL be passed as `(const uint8_t *ptr, size_t len)` or `(uint8_t *ptr, size_t cap, size_t *out_len)`. | API-060, API-061 | I |
| CABI-012 | Views SHALL be returned as POD structs containing decoded field values, plus **offset/length pairs** into the caller's input buffer (not pointers), so that they remain valid if the buffer is moved. | API-010, GEN-013 | T |
| CABI-013 | `Instant` and `Duration` SHALL be `uint64_t` microseconds. | API-003 | I |

## 4. Errors

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CABI-020 | Every fallible function SHALL return `int32_t`: `0` (`ALDIS_OK`) for success, or `-code` using the [error catalogue](../03-api-model.md#9-error-catalogue). For example, `ALDIS_ERR_BUFFER_TOO_SMALL = -1`. | GEN-040 | V |
| CABI-021 | Additional error detail (field code (API-082), `needed` octets, expected/found) SHALL be available through an optional `aldis_error_detail_t *` out-parameter. Passing `NULL` SHALL be allowed. The detail SHALL begin with a `uint32_t struct_size` set by the caller (CABI-033); the library SHALL write only the members that lie wholly inside `struct_size` and SHALL never write `struct_size` itself. A `struct_size` below the fixed prefix of `struct_size`, kind, field code, and detail kind (`ALDIS_ERROR_DETAIL_MIN_SIZE`, 12 octets) SHALL fail the call with `ALDIS_ERR_INVALID_FIELD` before it does anything, with the detail unwritten. | GEN-041 | T |
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

## 10. Details settled for the M1 codecs

The C ABI of `aldis-time`, `aldis-spp`, and `aldis-epp` ([M2 plan](../milestones/m2-frame-codecs.md) WP-0c) settles these details of the rules above, following D4 and D5. The frame codecs and the engines follow them.

- **Status codes** (CABI-020): the constants are negative, `ALDIS_ERR_<KIND> = -code`, as CABI-020's example gives them, so a function returns the constant itself; for example a too-short prefix returns `ALDIS_ERR_INCOMPLETE` (−2, M2 plan D14). The header writes each negative constant in parentheses.
- **Detail** (CABI-021): `aldis_error_detail_t` is `{ uint32_t struct_size; uint32_t kind; uint16_t field; uint16_t detail; uint32_t reserved; uint64_t needed, expected, found; }`, 40 octets. The caller sets `struct_size`; the library writes, only when the call fails, each member that lies wholly inside it, and leaves everything else, including `struct_size`, as it was. A `struct_size` below `ALDIS_ERROR_DETAIL_MIN_SIZE` (12) fails the call before it does anything; a larger one than the library knows gets the known members only, so a later ABI version can append members. `kind` is the catalogue code, `field` the field code of 03 §9.1 or 0, `detail` one of `ALDIS_DETAIL_NONE`, `_NEEDED`, `_EXPECTED`, and `reserved` 0. The field codes are published as `ALDIS_FIELD_*` constants, generated from the catalogue.
- **Argument checks** (CABI-022): `NULL` is allowed for a buffer of length 0 and for the detail; any other `NULL`, a pointer misaligned for its type, a buffer range that wraps or exceeds `PTRDIFF_MAX`, an output buffer that overlaps an input buffer of the same call or a helper's storage, a `struct_size` smaller than the struct, and an enumeration argument with no constant are each `ALDIS_ERR_INVALID_FIELD` with field 0 and no detail. A misaligned detail pointer fails the call before it does anything.
- **Configuration size** (CABI-033): a configuration whose `struct_size` is at least the struct this library knows is accepted; the library reads only that struct. A struct embedded in another (a time code format inside a secondary header layout) carries its own `struct_size`. `aldis_limits(aldis_limits_t *)` reads the caller's `struct_size` and fills the struct (CABI-030).
- **Two-phase writers** (D4, API-023): `_writer_begin` checks the arguments, writes an unfinished header (SPP: version `111`; EPP: Packet Length 0), and returns the offsets and capacities of the parts the caller fills in. `_writer_finish` takes the same arguments plus the length used, and writes the final header; neither keeps state.
- **Flags and enumerations** (CABI-010): flags in input structs are `uint8_t` 0 or 1; flags in output structs are `bool`. Enumerations are `uint8_t` or `uint32_t` fields with `ALDIS_*` constants.
- **Time codes** (D4): one flat format struct and one flat value struct for CUC, CDS, CCS, and agency-defined codes, selected by `code`. CUC fine time is `uint8_t fine[10]`, left-aligned, with `fine_len`; octets from `fine_len` on must be 0. A reserved time code ID decoded in lenient mode is `ALDIS_TIME_UNKNOWN`, with its P-field and no value; it cannot be encoded through the C ABI. Conversions go directly between CDS, CCS, and ASCII (`aldis_time_convert`); the intermediate UTC instant does not cross the ABI.
- **Stateful helpers** (D5, CABI-030…032): storage of at least `ALDIS_*_STORAGE_SIZE` octets, aligned to `ALDIS_*_STORAGE_ALIGN`, which is 8 for every helper, so a C caller declares it as a `uint64_t` array; the sizes hold on every supported target. `_init(storage, storage_len, …, aldis_*_t **out, err)` fails with `ALDIS_ERR_BUFFER_TOO_SMALL` (with `needed`) or `ALDIS_ERR_INVALID_FIELD`. The storage starts with a tag: a magic number per helper, the layout generation, and the storage's own address; a call fails with `ALDIS_ERR_INVALID_HANDLE` when the tag is not that of its helper at that address, which detects uninitialized, released (`_deinit`), foreign, and copied storage. A `NULL` handle is `ALDIS_ERR_INVALID_FIELD`. `_deinit` returns nothing and ignores an invalid handle. Buffers are passed on every call and never stored (API-060): the segmenter takes the same SDU on each call (another length is `ALDIS_ERR_LENGTH_MISMATCH`), and the reassembler the same reassembly buffer. The tracker's capacity is a build-time maximum, `ALDIS_SPP_TRACKER_MAX_APIDS` (`capacities.toml` `[spp]`), and its configuration chooses a capacity up to it.

## 11. Details settled for the frame codecs

The C ABI of `aldis-tm` ([M2 plan](../milestones/m2-frame-codecs.md) WP-2 module 7, #94) settles these details for the frame codecs, following D14. The C ABI of `aldis-tc`, `aldis-aos`, and `aldis-uslp` follows them (maintainer decisions 2026-10-24).

- **Arrays of versioned structs** (CABI-033): an array of configuration structs, such as the virtual channels of a master channel, is read with the `struct_size` of its first element as the stride. Every element repeats that `struct_size`, which must be at least the size of the struct this library knows and a multiple of its alignment; otherwise the call is `ALDIS_ERR_INVALID_FIELD`. A caller built against a later, longer struct thereby still works.
- **Master channels**: a master channel configuration embeds its physical channel configuration as a struct with its own `struct_size`, as the Rust constructor takes it. A call that needs the per-VC configuration takes one master channel; a caller with several master channels on one physical channel reads the SCID with the header decoder first. The Rust lookup over several master channels has no C form.
- **Security-layout lookups**: the `_with` pairs exist only for decoders. A decoder without a suffix takes the physical channel configuration alone and decodes as on a channel without SDLS; its `_with` variant takes the master channel configuration and the security layout of each virtual channel from it. A call that takes a master channel, validation among them, always uses its layouts. There is no lookup-mode argument.
- **Frame contents**: a one-shot frame encoder takes the frame's variable fields (TM: the Secondary Header data, the Security Header, the data field, the Security Trailer, and the OCF) as `(const uint8_t *, size_t)` members of one content struct (`aldis_tm_content_t`), read once per call and never kept (API-060). Each member pair is checked as a buffer argument is (CABI-022): `NULL` only with length 0, and no overlap with the output buffer. The virtual channel array stays a pair of arguments; no configuration struct holds a pointer.
- **Stateless finish of a writer that zero-fills** (D4, API-023): when the Rust writer's `begin` zero-fills the frame, `_writer_finish` cannot run `begin` again. The codec then offers a public `resume` that checks the arguments as `begin` does and writes nothing, and `_writer_finish` calls it (TM: `TmFrameWriter::resume`). `resume` compares the unfinished headers that `begin` wrote with those it would write for the arguments and fails with `InvalidState`, which `_writer_finish` returns as `ALDIS_ERR_INVALID_STATE`, on a difference or a buffer never begun (API-023; OI-50). The C ABI does not compute the FECF or headers itself.
- **Decoders that need the channel table** (TC, AOS, USLP; maintainer decision 2026-10-24): when a codec's decoder cannot run without its channel configuration, the codec has one decoder with no `_with` suffix, which takes the channel configuration and always uses its security layouts; so do its validation and its writers. `_with` pairs exist only where a decoder can also run on the physical channel alone (TM).
- **Validation reasons** (TC-021, USLP-014): a validation call reports why a frame was rejected through a separate `uint32_t *reason` out-parameter, with `ALDIS_<STD>_REASON_*` codes: 0 (`_REASON_NONE`) on success, the reason code on a rejection, whose error the call also returns, and unchanged on a configuration or argument error. The reason does not travel in `aldis_error_detail_t`.
- **Delimiters** (TC-020, USLP-015): a delimiter is a stateless call that takes the whole data unit and an offset (`aldis_<std>_next_frame(unit, unit_len, offset, …, out, err)`, with the channel arguments after `offset` where the codec needs them) and returns the next item's kind (a `uint8_t`), offset, length, and declared length (`len` for a frame, more than `len` for one cut short, 0 otherwise); the kind is `ALDIS_<STD>_DELIMITED_END` when nothing remains. The caller loops, adding each item's length to the offset.
- **Channel shapes**: a codec whose Rust configuration has a master channel type passes the master channel, which embeds its physical channel and carries the SCID: TC with separate arrays of VC and MAP structs, AOS and TM with an array of VC structs whose structs carry no SCID. A codec whose Rust configuration has no master channel type (USLP, whose `VcTable` spans every master channel of the physical channel) passes the physical channel configuration with flat arrays of VC and MAP structs, each tagged with its SCID, as the Rust constructor takes them.
