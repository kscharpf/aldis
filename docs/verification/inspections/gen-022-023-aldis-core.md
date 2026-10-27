# Inspection: GEN-022 and GEN-023 in `aldis-core`

| | |
|---|---|
| Requirements | GEN-022 (every loop bounded by an input length, a compile-time capacity, or a constant; no recursion), GEN-023 (time bounded by O(input length + capacities); nothing blocks or waits) |
| Scope | `crates/aldis-core/src/*.rs`, library code only (test modules excluded), at `main` 5f40e8b: the crate as released in `v0.2.0-m1`. Its library code last changed in 0655f2a (issue #22), which the last revision below covers |
| Inspected | 2026-10-20, prepared by Claude Code: pre-release re-inspection for `v0.2.0-m1` (M1 exit). Every loop, every call between the crate's own functions, and every blocking construct re-checked; the table and the call graph below are confirmed and made more complete. No unbounded loop, no recursion |
| Approved | 2026-10-26, maintainer |
| Repeat | when `aldis-core` gains a loop or a call between its own functions, and before each release |
| History | First inspected 2026-10-16 at `main` 6dd89b7, approved 2026-10-16 maintainer; revised since at `m1/core` 3b51e87 and later as below |
| Revised | 2026-10-17 (M1 WP-0): `mode` and `anomaly` added; approved 2026-10-17 maintainer. Revised again 2026-10-17 after [review round 1](../reviews/m1-core.md) finding 1: `FromIterator` (unbounded caller iterator) replaced by `AnomalySet::from_flags(&[A])`, and `Iter::next` bounded by a forward-only position over `0..CAPACITY`; approved 2026-10-17 maintainer |
| Revised | 2026-10-19, issue #22 (RS-020): `FrameCtx` is an enum and its accessors are single `match`es; `SecuredFields::new` is straight-line; `SecuredFields::from_layout` calls the new `FrameLayout` accessors, which clone one field each; the sealed `Anomaly` trait adds no code. No loop and no recursion added; the call graph stays acyclic. Confirmed 2026-10-19, maintainer |
| Revised | 2026-10-20, branch `m2/wp0a-field-codes` (M2 WP-0a, API-082), prepared by Claude Code: `error.rs` and the new, generated `field.rs`. `FieldId` holds a `NonZeroU16` code and a name; `FieldId::catalogued` is a `const fn` whose code check is an inline `const` block, evaluated at compile time, and is called only by the `const` items of `field.rs`, so it runs at compile time; `code` and `name` return a field; the manual `PartialEq::eq` compares one `NonZeroU16` and `Hash::hash` hashes one, so the derived `PartialEq`/`Hash` of `Error` stay straight-line. `field.rs` holds only `const` items: 64 `FieldId` constants and the array `ALL`, with no run-time code. `Error::out_of_range` takes a `FieldId` and calls `with_field`, no longer `FieldId::new`; `bits.rs`, `container.rs`, `counter.rs`, `ids.rs`, `layout.rs`, and `security.rs` pass constants where they passed names. No loop and no recursion added; the call graph stays acyclic. Approved 2026-10-20 maintainer |
| Revised | 2026-10-21, branch `m2/wp1-ocf` (M2 WP-1, CMN-010, CMN-011), prepared by Claude Code: the new `ocf.rs` and the two generated constants added to `field.rs` (`OCF_TYPE_FLAG`, `OCF_REPORT_USE`). `ocf.rs` has no loop: `bit` masks the first octet of a fixed 4-octet array; `Ocf::decode` calls `bit` twice and is a single `match`, and `Ocf::to_octets` is a single `match`; the `new` constructors and the `TryFrom` impls of `Type1Report`, `ProjectSpecificReport`, and `FsrReport` each call `Ocf::decode` once and `match` on it, and an error arm calls `wrong_bit`, which calls `Error::new`, `with_field`, `with_detail`, and `u8_to_u64` once each; the `From` conversions call `decode` or `to_octets`, or build a variant. The derived `Clone`/`PartialEq`/`Hash` copy or compare 4 octets. No loop and no recursion added; the call graph stays acyclic, and its longest chain is still the six functions of `verify_fecf` (the longest new ones have three: `new` or `try_from` → `Ocf::decode` → `bit`, and `try_from` → `wrong_bit` → `with_detail`). Approved 2026-10-21 maintainer |
| Revised | 2026-10-24, m2/wp7-checks (WP-7), prepared by the agent, pending exit review: the Loops table gains the first column `ID`, which names each loop that `cargo xtask loops` finds (file, item, and index; GEN-022, M2 plan WP-7), or holds `—` for a row that names none; the other columns are unchanged |

## Loops

Every loop in library code, with its bound. `ids`, `layout`, `counter`, `time`, `mode`, `ocf`, and `field` contain none. Copies and comparisons of fixed-size arrays (`to_be_bytes`, derived `Clone`/`PartialEq`/`Hash`) run over a constant length and are listed only where the length is a capacity.

| ID | Location | Loop | Bound |
|---|---|---|---|
| `bits.rs get #1`, `bits.rs set #1 #2` | `bits.rs` `get`, `set` | fold over the field's octets; reverse write-back in `set` | ≤ 9 octets (a field of ≤ 64 bits spans at most 9) |
| `crc.rs step_bitwise #1` | `crc.rs` `step_bitwise` | `while bit < 8` | constant 8 |
| `crc.rs crc16_update_bitwise #1` | `crc.rs` `crc16_update_bitwise` | `while let [octet, tail @ ..]` | input length; the slice shrinks by one octet per pass |
| `crc.rs TABLE #1` | `crc.rs` `TABLE` | `while i < 256` | constant 256; evaluated at compile time |
| `crc.rs crc16_update_table #1` | `crc.rs` `crc16_update_table` | fold over `data` | input length |
| `oid.rs OidGenerator::next_octet #1`, `oid.rs OidGeneratorGalois::next_octet #1` | `oid.rs` `next_octet` (both forms) | `(0..8)` | constant 8 |
| `oid.rs OidGenerator::fill #1`, `oid.rs OidGeneratorGalois::fill #1` | `oid.rs` `fill` (both forms) | over `buf` | output length |
| `container.rs FixedQueue::clear #1` | `container.rs` `FixedQueue::clear` | `while self.pop().is_some()` | ≤ N: each `pop` lowers `len`, and `len` ≤ N |
| `container.rs ByteQueue::read_at #1`, `container.rs ByteQueue::write_at #1` | `container.rs` `ByteQueue::read_at`, `write_at` | zip of the ring with the buffer | min(buffer length, N) |
| — | `container.rs` `FixedQueue::new`, `ByteQueue::new`, `FixedVec::new` | array initialisers `[const { None }; N]`, `[0; N]`, `[T::default(); N]` | capacity `N` |
| — | `container.rs` `FixedVec` `PartialEq::eq` | slice comparison of the stored items | ≤ N |
| `bits.rs octet_int! #1` | `bits.rs` `read_u16` … `write_u64` | `copy_from_slice` / array conversion of the integer's octets | constant 2, 4, or 8 |
| `crc.rs write_fecf #1` | `crc.rs` `write_fecf` | `copy_from_slice` of the FECF | constant 2 |
| `error.rs ErrorKind::from_code #1`, `error.rs ErrorKind::from_name #1` | `error.rs` `ErrorKind::from_code`, `from_name` | `find` over `ErrorKind::ALL`; `from_name` compares each name with the input | constant: the catalogue size (17), times the shorter of the two names |
| `anomaly.rs Iter::next #1` | `anomaly.rs` `Iter::next` | `for bit in self.next..CAPACITY`; `self.next` only rises, so one call and the whole iteration each visit an index at most once | constant `CAPACITY` = 32 |
| `anomaly.rs AnomalySet::from_flags #1` | `anomaly.rs` `AnomalySet::from_flags` | over the `flags` slice | input length |
| — | `anomaly.rs` `anomaly_flags!` | none at run time: the repeated index checks are a `const` item, evaluated at compile time | — |
| `security.rs FixedPatternSecurity::protect #1 #2`, `security.rs FixedPatternSecurity::verify #1` | `security.rs` `FixedPatternSecurity` (feature `test-hooks` only) | 2-element array; `fill` / `all` over each range | 2 × range length, each range checked against the frame length first |

## Recursion

No function calls itself, directly or through another `aldis-core` function. A scan of the call graph (2026-10-20, by `Type::function`) flagged only same-named methods of other types: `bits::get` calls `<[u8]>::get`, `SecurityLayout::new` calls `Error::new`, and `Instant`/`Duration::saturating_add` call `u64::saturating_add`. The internal call graph is acyclic; its longest chain has six functions, `verify_fecf` → `crc16` → `crc16_update` → `crc16_update_bitwise` → `step_bitwise` → `u8_to_u16` (without the `crc-table` feature); shorter ones are `fill` → `next_octet` → `next_bit`, `clear` → `pop`. `AnomalySet::from_flags` calls `insert`; `insert` and `contains` call `mask`, which calls `Anomaly::bit`; `Iter::next` calls `bit_mask` and `Anomaly::from_bit`. In `ocf`, each report's `new` and `TryFrom::try_from` call `Ocf::decode`, which calls `bit`, and `try_from` calls `wrong_bit`; `From<[u8; 4]> for Ocf` calls `Ocf::decode`; `Ocf::to_octets` calls the report's `to_octets`. `Ocf::decode` builds the report variants directly and calls no constructor, so no cycle runs through it. Both trait methods are implemented by the codec crates and call nothing in `aldis-core`.

## Blocking

There is no I/O, locking, atomics, sleep, or spin-wait. `std` and `alloc` are only behind features and in test modules. `aldis-core` has no engine operations; every function is O(input length + N) by the table above. GEN-023 applies to engines in full from M3/M4, where VER-012 documents worst-case paths.
