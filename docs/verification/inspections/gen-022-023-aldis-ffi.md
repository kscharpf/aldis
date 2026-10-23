# Inspection: GEN-022 and GEN-023 in `aldis-ffi`

| | |
|---|---|
| Requirements | GEN-022 (every loop bounded by an input length, a compile-time capacity, or a constant; no recursion), GEN-023 (time bounded by O(input length + capacities); nothing blocks or waits) |
| Scope | **Partial.** `crates/aldis-ffi/src/{crc,ocf,oid}.rs` (M2 WP-1) and the shared modules they call, `abi.rs`, `handle.rs`, and `error.rs`, library code only (test modules excluded), on branch `m2/wp1-c-abi` (based on `main` 46bfe0f). Not yet inspected: the M1 codec modules `spp.rs`, `spp_state.rs`, `time.rs`, `epp.rs`, and `limits.rs` of WP-0c (#72), which added no record; the generated `field.rs` and `capacities.rs` hold only `const` items. The whole crate is due in M2 WP-7 (plan §3, "GEN-022/023 inspection of the four crates and `aldis-ffi`") |
| Inspected | 2026-10-23, prepared by Claude Code, pending exit review (VER-061) |
| Approved | — |
| Repeat | when `aldis-ffi` gains a loop or a call between its own functions, and before each release |
| History | First prepared 2026-10-23 on branch `m2/wp1-c-abi` (M2 WP-1, C ABI part) |

## Loops

Every loop in the inspected library code, with its bound. `crc.rs`, `ocf.rs`, `oid.rs`, `handle.rs`, and `error.rs` contain none of their own. Copies of fixed-size values (the four OCF octets, a handle's tag and state, `aldis_error_detail_t`) run over a constant length and are not listed. Loops inside `aldis-core` functions that these modules call are bounded in the [`aldis-core` record](gen-022-023-aldis-core.md) and listed here only by the bound they take from the call.

| Location | Loop | Bound |
|---|---|---|
| `abi.rs` `bytes_mut` | `any` over the call's input buffers, to reject an overlap | constant per call site: the length of the `inputs` array literal (1 for `aldis_oid_generator_fill`, its storage) |
| `crc.rs` `aldis_crc16`, through `aldis_core::crc::crc16` | over the input octets | input length (`len`) |
| `oid.rs` `GeneratorState::fill`, through `OidGenerator::fill` or `OidGeneratorGalois::fill` | over the output octets, 8 bits each | output length (`len`) × 8 |

## Recursion

No function calls itself, directly or through another `aldis-ffi` function. The internal call graph of the inspected modules is acyclic; its longest chain has five functions: `aldis_oid_generator_fill` → `abi::call` → (closure) `generator` → `handle::get` → `abi::invalid_argument`. The calls:

- `aldis_crc16` → `abi::call`; in its closure `abi::bytes` → `range`, `abi::out`, `Out::write`, and `aldis_core::crc::crc16`.
- `aldis_ocf_classify` → `abi::call`; in its closure `octets` → `abi::bytes`, `abi::invalid_argument`; `abi::out`, `Out::write`, `report`, and `Ocf::decode`.
- `aldis_ocf_check` → `abi::call`; in its closure `octets`, then one of the `TryFrom` constructors of `Type1Report`, `ProjectSpecificReport`, `FsrReport`, or `abi::invalid_argument`.
- `aldis_oid_generator_init` → `abi::call`; in its closure `abi::out`, `OidGenerator::new` or `OidGeneratorGalois::new` or `abi::invalid_argument`, `handle::init` → `abi::invalid_argument` or `Error::buffer_too_small`, and `Out::write`.
- `aldis_oid_generator_fill` → `abi::call`; in its closure `generator` → `handle::get`, `Handle::range`, `abi::bytes_mut` → `range`, `Handle::load`, `GeneratorState::fill`, and `Handle::store`.
- `aldis_oid_generator_deinit` → `handle::deinit` → `handle::get`.
- `aldis_oid_generator_storage_size` and `_storage_align` return a constant.
- `abi::call` → `write_detail` (straight-line: one guarded write per member), `aldis_error_detail_t::from` (one `match`), and `error::status`.

## Blocking

There is no I/O, locking, atomics, sleep, or spin-wait. `std` is only behind a feature and in test modules; the `no_std` panic handlers (`lib.rs`) are never reached (VER-010). Every inspected function is O(input length) or O(1) by the table above.
