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

**Panic handler** (OI-49). No panic path is reachable in the library (GEN-020, VER-010). The static library of a `no_std` target built with the `panic-handler` feature nevertheless defines `#[panic_handler]`, so that it links; if it ever runs, it executes a trapping instruction (`core::arch::asm!`, the one `unsafe` block of the handler, GEN-070) and only then spins: `udf #0` on Arm (thumbv7em, armv7r), `ebreak` on RISC-V (riscv32imac), `brk #0` on AArch64, `ud2` on x86. The trap raises the target's undefined-instruction or breakpoint exception, so the fault handler, a debugger, or a watchdog sees the failure where it happened, and the handler adds no undefined symbol (CABI-004). On a target where `asm!` is not stable, such as SPARC V8 (LEON3), the handler only spins, and a watchdog is the integrator's means to end it. The exerciser's `panic-unreachable` handler (VER-010) is another handler and unchanged.

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

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CABI-030 | Engine capacities (const generics in Rust) SHALL be fixed when the library is built. They SHALL be read from a build configuration file (`aldis-ffi/capacities.toml`) and emitted as `ALDIS_*_MAX_*` constants in the header. `aldis_limits(aldis_limits_t *)` SHALL report them at run time. | GEN-011 | A |
| CABI-031 | `*_init` SHALL validate storage size and alignment and fail with `ALDIS_ERR_BUFFER_TOO_SMALL` / `ALDIS_ERR_INVALID_FIELD` if they are inadequate. | — | T |
| CABI-032 | A handle SHALL carry a magic/generation tag. Calls with a deinitialized or foreign handle SHALL fail with `ALDIS_ERR_INVALID_HANDLE` where detectable. | — | T |
| CABI-033 | Configuration structs SHALL begin with a `uint32_t struct_size` field set by the caller, for forward-compatible extension. | GEN-080 | T |

## 6. Engines as step calls, without callbacks

An engine call is the step function of [03 §5](../03-api-model.md#5-engines-the-sans-io-contract) behind a handle. It accesses the engine's state **in place**, in the caller's storage, as `&mut` for the duration of one call, and uses the `decide`/`commit` form that API-058 permits, so a failed call leaves the state unchanged without a copy of the state on the stack (FOP-1's state is at least 16 KiB, against the 10 240-byte stack budget of the C ABI; OI-45, OI-57). The handle's tag check (CABI-032) is the one of `handle.rs`. The conventions of [§12](#12-conventions-adopted-2026-10-29-implementation-pending) (flat input structs, flat action structs, ranges) apply.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CABI-040 | Each engine input SHALL be one call that runs the step function of API-057 and returns its actions (illustratively `uint32_t aldis_fop1_step(aldis_fop1_t *h, const aldis_fop1_input_t *in, aldis_instant_t now, uint8_t *out, size_t out_cap, aldis_fop1_actions_t *actions, aldis_error_detail_t *err)`), and `next_deadline` and the status snapshot SHALL be calls that only read. The engine's state SHALL be accessed in place in the caller's storage, for the duration of one call, as `decide` followed by `commit` where API-058 applies; the call SHALL NOT load the state onto the stack by value and write it back. The input SHALL be a flat struct with a `kind` field (an input enumeration, a plain integer with `ALDIS_*` constants, §12 item 2) that the library parses once into the Rust input type. The `actions` struct SHALL hold a fixed-capacity array, `ALDIS_*_MAX_ACTIONS` (the bound `N`, CABI-030), of flat action structs written only by the library: a `kind` field (an output enumeration, §12 item 2) and the fields of every kind, with no union (the header stays C99, CABI-003); a field that a kind does not use SHALL be zero. The octets of each action SHALL be an `aldis_range_t` (`{ size_t offset; size_t len; }`) into `out` (API-061). A call that fails SHALL leave the state unchanged (API-058) and SHALL write no actions. No `poll_*` call exists. No engine API SHALL take a callback. | API-050, API-057, API-058, API-061 | I/T |
| CABI-041 | Withdrawn: engines take no security hook (API-050, API-074; OI-56). A C caller applies SDLS itself to the ranges that a `Transmit` action returns, and finishes the frame with the codec's writer call; on reception it checks the FECF and applies SDLS before the frame input. The library has no `aldis_security_ops_t`. | — | — |

## 7. Threading

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CABI-050 | Functions on distinct handles SHALL be callable concurrently from different threads. Calls on the same handle SHALL be serialized by the caller (GEN-033). Codec functions are stateless and fully reentrant. | GEN-033 | I |

## 8. Versioning

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CABI-060 | The header SHALL define `ALDIS_ABI_VERSION_MAJOR/MINOR/PATCH`. `aldis_abi_version()` SHALL return the library's version. The major version SHALL change on any ABI-breaking change; while the major version is 0 (pre-release, GEN-080), the minor version SHALL change instead, as in Cargo's semantic versioning of 0.x releases. | GEN-080 | T |

## 9. C++ wrapper API (aldis.hpp)

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| CABI-070 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CABI-071 | Withdrawn: Track C retired and deferred to Phase 3 (chore/retire-track-c). | — | — |
| CABI-072 | The C++ API SHALL be a header-only, idiomatic wrapper (`aldis.hpp`) over the Rust-backed C ABI: RAII handle types with storage in `alignas(N) std::byte[N]` members (`std::aligned_storage` is deprecated in C++23), `std::span` views, and `expected`-like results, in C++20 (OI-19). Engine capacities SHALL be template parameters, checked with `static_assert` against the build-time `ALDIS_*_MAX_*` constants (CABI-030, OI-20). It SHALL NOT use exceptions. | — | T |

## 10. Details settled for the M1 codecs

The C ABI of `aldis-time`, `aldis-spp`, and `aldis-epp` ([M2 plan](../milestones/m2-frame-codecs.md) WP-0c) settles these details of the rules above, following D4 and D5. The frame codecs and the engines follow them.

- **Status codes** (CABI-020): the constants are negative, `ALDIS_ERR_<KIND> = -code`, as CABI-020's example gives them, so a function returns the constant itself; for example a too-short prefix returns `ALDIS_ERR_INCOMPLETE` (−2, M2 plan D14). The header writes each negative constant in parentheses.
- **Detail** (CABI-021): `aldis_error_detail_t` is `{ uint32_t struct_size; uint32_t kind; uint16_t field; uint16_t detail; uint32_t reserved; uint64_t needed, expected, found; }`, 40 octets. The caller sets `struct_size`; the library writes, only when the call fails, each member that lies wholly inside it, and leaves everything else, including `struct_size`, as it was. A `struct_size` below `ALDIS_ERROR_DETAIL_MIN_SIZE` (12) fails the call before it does anything; a larger one than the library knows gets the known members only, so a later ABI version can append members. `kind` is the catalogue code, `field` the field code of 03 §9.1 or 0, `detail` one of `ALDIS_DETAIL_NONE`, `_NEEDED`, `_EXPECTED`, and `reserved` 0. The field codes are published as `ALDIS_FIELD_*` constants, generated from the catalogue.
- **Argument checks** (CABI-022): `NULL` is allowed for a buffer of length 0 and for the detail; any other `NULL`, a pointer misaligned for its type, a buffer range that wraps or exceeds `PTRDIFF_MAX`, an output buffer that overlaps an input buffer of the same call or a helper's storage, a `struct_size` smaller than the struct, and an enumeration argument with no constant are each `ALDIS_ERR_INVALID_FIELD` with field 0 and no detail. A misaligned detail pointer fails the call before it does anything.
- **Configuration size** (CABI-033): a configuration whose `struct_size` is at least the struct this library knows is accepted; the library reads only that struct. A struct embedded in another (a time code format inside a secondary header layout) carries its own `struct_size`. `aldis_limits(aldis_limits_t *)` reads the caller's `struct_size` and fills the struct (CABI-030).
- **One-shot encoders** (D4, API-020, API-023; OI-57): the C ABI has no two-phase writer. A frame or packet is built in one call, `aldis_<std>_encode`, that takes every variable field and keeps no state; an Only Idle Data frame by `aldis_tm_encode_oid`, `aldis_aos_encode_oid`, or `aldis_uslp_encode_oid`, which take an `aldis_oid_generator_t`. The earlier `_writer_begin` and `_writer_finish` functions (OI-50) are removed. A frame codec whose one-shot encoder has no field for the Security Header and Trailer (TC, USLP) writes zeros there (API-072); a caller that applies SDLS through the C ABI uses TM or AOS, or a later encoder variant.
- **Flags and enumerations** (CABI-010): flags in input structs are `uint8_t` 0 or 1; flags in output structs are `bool`. Enumerations are `uint8_t` or `uint32_t` fields with `ALDIS_*` constants.
- **Time codes** (D4): one flat format struct and one flat value struct for CUC, CDS, CCS, and agency-defined codes, selected by `code`. CUC fine time is `uint8_t fine[10]`, left-aligned, with `fine_len`; octets from `fine_len` on must be 0. A reserved time code ID decoded in lenient mode is `ALDIS_TIME_UNKNOWN`, with its P-field and no value; it cannot be encoded through the C ABI. Conversions go directly between CDS, CCS, and ASCII (`aldis_time_convert`); the intermediate UTC instant does not cross the ABI.
- **Stateful helpers** (D5, CABI-030…032): storage of at least `ALDIS_*_STORAGE_SIZE` octets, aligned to `ALDIS_*_STORAGE_ALIGN`, which is 8 for every helper, so a C caller declares it as a `uint64_t` array; the sizes hold on every supported target. `_init(storage, storage_len, …, aldis_*_t **out, err)` fails with `ALDIS_ERR_BUFFER_TOO_SMALL` (with `needed`) or `ALDIS_ERR_INVALID_FIELD`. The storage starts with a tag: a magic number per helper, the layout generation, and the storage's own address; a call fails with `ALDIS_ERR_INVALID_HANDLE` when the tag is not that of its helper at that address, which detects uninitialized, released (`_deinit`), foreign, and copied storage. A `NULL` handle is `ALDIS_ERR_INVALID_FIELD`. `_deinit` returns nothing and ignores an invalid handle. Buffers are passed on every call and never stored (API-060): the segmenter takes the same SDU on each call (another length is `ALDIS_ERR_LENGTH_MISMATCH`), and the reassembler the same reassembly buffer. The tracker's capacity is a build-time maximum, `ALDIS_SPP_TRACKER_MAX_APIDS` (`capacities.toml` `[spp]`), and its configuration chooses a capacity up to it.

## 11. Details settled for the frame codecs

The C ABI of `aldis-tm` ([M2 plan](../milestones/m2-frame-codecs.md) WP-2 module 7, #94) settles these details for the frame codecs, following D14. The C ABI of `aldis-tc`, `aldis-aos`, and `aldis-uslp` follows them (maintainer decisions 2026-10-24).

- **Arrays of versioned structs** (CABI-033): an array of configuration structs, such as the virtual channels of a master channel, is read with the `struct_size` of its first element as the stride. Every element repeats that `struct_size`, which must be at least the size of the struct this library knows and a multiple of its alignment; otherwise the call is `ALDIS_ERR_INVALID_FIELD`. A caller built against a later, longer struct thereby still works.
- **Master channels**: a master channel configuration embeds its physical channel configuration as a struct with its own `struct_size`, as the Rust constructor takes it. A call that needs the per-VC configuration takes one master channel; a caller with several master channels on one physical channel reads the SCID with the header decoder first. The Rust lookup over several master channels has no C form.
- **Security-layout lookups**: the `_with` pairs exist only for decoders. A decoder without a suffix takes the physical channel configuration alone and decodes as on a channel without SDLS; its `_with` variant takes the master channel configuration and the security layout of each virtual channel from it. A call that takes a master channel, validation among them, always uses its layouts. There is no lookup-mode argument.
- **Frame contents**: a one-shot frame encoder takes the frame's variable fields (TM: the Secondary Header data, the Security Header, the data field, the Security Trailer, and the OCF) as `(const uint8_t *, size_t)` members of one content struct (`aldis_tm_content_t`), read once per call and never kept (API-060). Each member pair is checked as a buffer argument is (CABI-022): `NULL` only with length 0, and no overlap with the output buffer. The virtual channel array stays a pair of arguments; no configuration struct holds a pointer.
- **Stateless finish of a writer that zero-fills**: Withdrawn (OI-57). The C ABI has no `_writer_finish`, and the Rust `resume` it called is removed.
- **Decoders that need the channel table** (TC, AOS, USLP; maintainer decision 2026-10-24): when a codec's decoder cannot run without its channel configuration, the codec has one decoder with no `_with` suffix, which takes the channel configuration and always uses its security layouts; so do its validation and its encoders. `_with` pairs exist only where a decoder can also run on the physical channel alone (TM).
- **Validation reasons** (TC-021, USLP-014): a validation call reports why a frame was rejected through a separate `uint32_t *reason` out-parameter, with `ALDIS_<STD>_REASON_*` codes: 0 (`_REASON_NONE`) on success, the reason code on a rejection, whose error the call also returns, and unchanged on a configuration or argument error. The reason does not travel in `aldis_error_detail_t`.
- **Delimiters** (TC-020, USLP-015): a delimiter is a stateless call that takes the whole data unit and an offset (`aldis_<std>_next_frame(unit, unit_len, offset, …, out, err)`, with the channel arguments after `offset` where the codec needs them) and returns the next item's kind (a `uint8_t`), offset, length, and declared length (`len` for a frame, more than `len` for one cut short, 0 otherwise); the kind is `ALDIS_<STD>_DELIMITED_END` when nothing remains. The caller loops, adding each item's length to the offset.
- **Truncated USLP frames** (USLP-002; OI-48, OI-52): the C view of a decoded truncated frame, `aldis_uslp_frame_t`, has `truncated` true and `kind` `ALDIS_USLP_KIND_EXPEDITED`, since Annex D1.3.1 note 3 makes a truncated frame expedited user data, so that a caller that ignores `truncated` still routes it as the Blue Book says; the members only a full frame has (`ocf_flag`, `vcf_count_len`, `vcf_count`, `has_fecf`) stay 0 or false. A VC with a truncated frame length carries only truncated frames: the encode calls reject a non-truncated frame on it with `ALDIS_ERR_INVALID_FIELD` (`end_of_frame_primary_header_flag`), the decode, validate, and peek-length calls reject one with the same error (validation reason `ALDIS_USLP_REASON_NON_TRUNCATED_FRAME_NOT_ALLOWED`, 18), and the delimiter reports `ALDIS_USLP_DELIMITED_NON_TRUNCATED_NOT_ALLOWED` (5), discarding the rest of the data unit.
- **TM frame length** (TM-001; OI-51): `aldis_tm_config_check` and every call that takes a physical channel reject a frame length above 4 095 octets with `ALDIS_ERR_INVALID_CONFIG` (`frame_len`, `expected` 4095), since the CRC-16 misses some double-bit errors beyond it (VER-064).
- **Empty channel table** (TC-024, AOS-020, USLP-017; OI-55): `aldis_tc_mc_config_check`, `aldis_aos_mc_config_check`, `aldis_uslp_vc_table_check`, and every call that builds the channel table from a VC array reject `vc_count` 0 with `ALDIS_ERR_INVALID_CONFIG` (`vc_id`). A physical channel with no virtual channel has no Spacecraft ID to accept an OID frame for.
- **Channel shapes**: a codec whose Rust configuration has a master channel type passes the master channel, which embeds its physical channel and carries the SCID: TC with separate arrays of VC and MAP structs, AOS and TM with an array of VC structs whose structs carry no SCID. A codec whose Rust configuration has no master channel type (USLP, whose `VcTable` spans every master channel of the physical channel) passes the physical channel configuration with flat arrays of VC and MAP structs, each tagged with its SCID, as the Rust constructor takes them.

## 12. Conventions adopted 2026-10-29 (implementation pending)

The maintainer decided seven conventions for the C ABI on 2026-10-29 (OI-57; decision log). The project is pre-release, so backward compatibility of the C ABI is not a constraint, and the goal is the simplest C ABI that keeps the high-assurance properties. **The code still implements the conventions of §1 to §11 until the follow-up pull requests below land.** Until then, a row of §3 to §5, §8, §10, or §11 that this section changes describes the code that exists and stays as written: it is the as-built behaviour, and this section is what the implementing pull request makes true. Each implementing pull request rewrites or withdraws the rows it changes, in the same change as the code. The engine rows (CABI-040 and the introduction of §6) have no code yet and are rewritten now.

| # | Convention | Rationale |
|---|---|---|
| 1 | **One-shot encoders only.** The C ABI has no two-phase writers: the 15 functions `aldis_<std>_writer_begin` and `_writer_finish` (TM, AOS, TC, USLP, SPP, EPP) and their `*_layout_t` structs go. Frames and packets are built with the one-shot encoders (`aldis_tm_encode`, `aldis_tc_encode`, `aldis_uslp_encode`, `aldis_uslp_encode_oid`, `aldis_spp_encode`, `aldis_epp_encode`, …), plus a one-shot Only Idle Data encoder for TM and AOS where one is missing. The Rust typestate writers (`begin`/`finish`, RS-021) stay; their `resume`, which exists only for the stateless C finish, goes. | A stateless finish cannot know that the buffer holds what `begin` wrote: finish twice, other arguments, wrong layout, an unchecked security layout, and no check at all for SPP and EPP (OI-50). No requirement mandates zero-copy payload fill from C. Not chosen: an opaque writer state in caller storage. |
| 2 | **Enumerations.** A value the caller passes in stays a plain `uint8_t` or `uint32_t` with a generated `ALDIS_*` constant and is parsed once on the Rust side; an out-of-range value from C never becomes a Rust enum (that is undefined behaviour). A C `enum` type (`#[repr(u32)]` in Rust, emitted by cbindgen) may be used only for a field the library writes. No union crosses the C ABI (item 7). FFI-local `#[repr(u32)]` mirror enums with exhaustive conversions and one generic parser replace the hand-written constant and `match` pairs. | Keeps invalid values unrepresentable (RS-020) at the one place untrusted input enters. Relaxes CABI-010's "never as C enum types in struct fields" for output fields only. |
| 3 | **Status codes.** A fallible function returns `uint32_t`: `0` is OK, any other value is `ErrorKind::code()`, generated from `aldis-core`'s `ErrorKind`. The negative `ALDIS_ERR_*` convention and the hand-written mirror go. | One source of truth for the catalogue of 03 §9. Not chosen: keeping the negatives and the hand mirror. |
| 4 | **Typed caller storage.** The storage of each stateful helper is a typed struct, `typedef struct aldis_x { uint64_t words[N]; } aldis_x_t;`, declared by the caller as an object of that type, so the C compiler supplies size and alignment. The `ALDIS_*_STORAGE_SIZE` and `_STORAGE_ALIGN` constants, the `aldis_*_storage_size` and `_storage_align` functions, and the `storage_len` argument go. The tag check of `handle.rs` (magic, generation, address) stays. | Removes the size and alignment checks and their error paths. |
| 5 | **No `struct_size`, no `aldis_limits`.** Configuration and detail structs lose `struct_size`. The ABI major version (`aldis_abi_version`, `ALDIS_ABI_VERSION_MAJOR`) is the only compatibility mechanism. `aldis_limits` and `aldis_limits_t` go; the header already carries the `ALDIS_*_MAX_*` capacities. | Accepted cost: after the first release, extending a struct needs a new function or a major version change. |
| 6 | **Engine state in place.** An engine call accesses its state in the caller's storage as `&mut` for the duration of the call, with the `decide`/`commit` form of API-058, instead of loading it by value and writing it back. | FOP-1's state is at least 16 KiB (`capacities.toml` `[fop1]`: 16 frames of 1 024 octets) against the 10 240-byte C ABI stack budget (OI-45). Spike 3 below measures it. |
| 7 | **Engine input and output shape.** Inputs cross as flat structs with a `kind` field (an input enumeration, item 2) that the library parses once into the Rust input type. Actions are a fixed-capacity array of flat structs that only the library writes: a `kind` field and the fields of every kind, unused fields zero, with `aldis_range_t { size_t offset; size_t len; }` ranges into the caller's output area (API-061). No union: cbindgen emits a `#[repr(C, u32)]` enum as an anonymous union, which is C11, and the header stays C99 (CABI-003; maintainer, 2026-10-29); C does not check which union member is read, and MISRA C:2012 Rule 19.2 advises against unions. `aldis_range_t` is also the type of every offset and length pair in views (CABI-012). A failed call writes no actions. | One shape for every engine; ranges stay valid if the caller moves the buffer. |

### 12.1 Requirements each convention will change

| Convention | Rows changed when implemented |
|---|---|
| 1 | API-023 (the `resume` and stateless-finish text; OI-50 superseded); §10 "Two-phase writers"; §11 "Stateless finish of a writer that zero-fills"; the C ABI parts of the writer requirements of the codecs (TM-009, TC-004, TC-007, AOS-019, USLP-010); new rows for the one-shot Only Idle Data encoders |
| 2 | CABI-010 (enumerations clause); §10 "Flags and enumerations"; CABI-022 (an enumeration argument with no constant stays `INVALID_FIELD`) |
| 3 | CABI-020 and CABI-022 (the returned type and the value of an error); §10 "Status codes"; the §2 naming example (`ALDIS_ERR_CRC_MISMATCH`); the `ALDIS_ERR_*` mentions in §5, §10, §11 |
| 4 | §5 text and example; CABI-031 (`_init` no longer validates size and alignment); §10 "Stateful helpers"; CABI-072 (the C++ wrapper's `alignas(N) std::byte[N]` members become the typed struct) |
| 5 | CABI-021 (`struct_size` of the detail, `ALDIS_ERROR_DETAIL_MIN_SIZE`); CABI-030 (`aldis_limits`); CABI-033 (withdrawn); CABI-060 (stays: the sole mechanism); §10 "Detail" and "Configuration size"; §11 "Arrays of versioned structs" (the stride is `sizeof` of the struct) and the embedded-struct text of "Master channels" |
| 6 and 7 | CABI-040 and the §6 introduction (rewritten now); CABI-012 (ranges as `aldis_range_t`) |

CABI-001 to CABI-004, CABI-011, CABI-013, CABI-032, CABI-050, and CABI-041 (withdrawn) are not affected.

### 12.2 Order of the follow-up pull requests

1. Delete the C two-phase writers (and their layout structs and header entries) and the Rust `resume`; add the missing one-shot Only Idle Data encoders for TM and AOS.
2. Typed storage (item 4).
3. In-place engine state (item 6): the `handle.rs` mechanism for engines, before any engine has a C module.
4. Enumeration glue per codec (item 2): mirror enums and the generic parser.
5. Status codes (item 3).
6. `aldis_range_t` in views (item 7).
7. Drop `struct_size` and `aldis_limits` (item 5).

Then the FARM-1 and FOP-1 C modules (M3), built on items 2, 3, 4, 6, and 7. Each pull request changes one track's directories and keeps the Rust crates as the reference.

### 12.3 Spikes (2026-10-29, throwaway, not committed)

Run with cbindgen 0.29.4 and the language, `[export]`, and `[enum]` settings of `crates/aldis-ffi/cbindgen.toml` (`language = "C"`, `cpp_compat`, `style = "type"`, `usize_is_size_t`, `prefix_with_name`, `rename_variants = "ScreamingSnakeCase"`), on a scratch crate. None contradicts a decision; points for the implementing pull requests are marked **Open point**.

**Spike 1: `#[repr(u32)]` fieldless enum.** `#[repr(u32)] pub enum aldis_kind_t { Alpha = 0, BetaGamma = 7 }` is emitted as an `enum` with a fixed underlying type under C23 and C++, plus, in C before C23, a `typedef uint32_t`.

So in C99 a field of the type is a `uint32_t` (fixed width, never `int`-sized), and the constants are enumerators of an `enum` that no field uses. `prefix_with_name` makes each constant `<TYPE NAME>_<VARIANT>` with the type's `_t` inside (`ALDIS_KIND_T_ALPHA`). **Open point:** to get a constant such as `ALDIS_TM_KIND_ALPHA`, the Rust enum is named for the constants it should produce, without the `_t`, and the typedef name is given by a rename, or the variants carry the full name; the enumeration-glue pull request (item 4) chooses. The header stays valid C99 and C++11 (CABI-003); the C23 branch should also be compiled once by the `c-smoke` check.

**Spike 2a: typed storage.** `#[repr(C)] pub struct aldis_x_t { pub words: [u64; N_WORDS] }` with `pub const N_WORDS: usize = 4;` is emitted as a struct. The array length keeps the constant's name, and the constant becomes a `#define`. **Open point:** a length that is an arithmetic expression of constants (`(FRAMES * FRAME_LEN + FRAMES * 2 + 8) / 8 + 1`) made cbindgen emit the type as an opaque forward declaration (`typedef struct aldis_fop1_t aldis_fop1_t;`) with no body and no warning. The word count of each storage struct must therefore be one `pub const` (computed in Rust, with a `const` assertion that the state fits), and the implementing pull request checks the generated header for the body of every storage type.

**Spike 2b: `#[repr(C, u32)]` enum with payload variants.** Variants `None`, `Transmit { range: aldis_range_t, vc: u8 }`, and `Event { code: u32 }` are emitted as a tag enum, one body struct per variant, and a struct of the tag and an anonymous union.

`aldis_range_t` is `typedef struct { size_t offset; size_t len; } aldis_range_t;`, and a fixed array of the union in a struct is `aldis_action_t actions[3];`. cbindgen therefore emits the shape of convention 7. The union is **anonymous**, which is C11 (a GNU extension in C99), and CABI-003 requires valid C99. **Settled (maintainer, 2026-10-29):** the header stays C99, so convention 7 uses flat action structs and no union crosses the C ABI.

**Spike 3: stack use of in-place access.** A scratch crate on `thumbv7em-none-eabihf`, release profile with `opt-level = "s"` and `panic = "abort"`, holds a 16 420-octet state (16 frames of 1 024 octets, 16 lengths, a head counter) in `aldis_fop1_t { words: [u64; 2053] }`. Two `extern "C"` functions run the same `push` of a frame:

- `inplace(s: *mut aldis_fop1_t, …)` casts to `&mut State` and calls `push`. Its own frame is 8 octets (`push {r7, lr}`) and it ends in a tail call to `push`, which saves 20 octets (`push {r4-r7, lr}`) and has no local frame. **Worst case about 28 octets.**
- `byvalue(s: *mut aldis_fop1_t, …)` does `ptr::read` of the state, calls `step(self, …) -> (State, u32)`, and `ptr::write`s the result back: the by-value form of the former CABI-040 wording. Its frame is 32 octets of saved registers plus `sub sp, #32768` and `sub sp, #80`, so **32 880 octets**; `step` adds 24 and `push` 20: **worst case about 32 924 octets**, two copies of the state and over three times the 10 240-byte budget.

The numbers are the frames in the compiler's assembly (`cargo rustc --release --target thumbv7em-none-eabihf -- --emit=asm`), summed along the deepest call path; they were not taken with `cargo xtask stack`, which builds the workspace with `-Z build-std` on the tooling nightly and does not see a scratch crate. The implementing pull request for convention 6 measures the real engine with `cargo xtask stack`. In-place access fits the budget for any state that fits the caller's storage; by-value access cannot hold FOP-1 at its default capacity at all.
