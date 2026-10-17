# Inspection: GEN-022 and GEN-023 in `aldis-core`

| | |
|---|---|
| Requirements | GEN-022 (every loop bounded by an input length, a compile-time capacity, or a constant; no recursion), GEN-023 (time bounded by O(input length + capacities); nothing blocks or waits) |
| Scope | `crates/aldis-core/src/*.rs`, library code only (test modules excluded), at `main` 6dd89b7 |
| Inspected | 2026-10-16, prepared by Claude Code |
| Approved | 2026-10-16, maintainer |
| Repeat | when `aldis-core` gains a loop or a call between its own functions, and before each release |
| Revised | 2026-10-17 (M1 WP-0): `mode` and `anomaly` added; awaiting maintainer approval |

## Loops

Every loop in library code, with its bound. `ids`, `layout`, `counter`, `time`, and `mode` contain none.

| Location | Loop | Bound |
|---|---|---|
| `bits.rs` `get`, `set` | fold over the field's octets; reverse write-back in `set` | ≤ 9 octets (a field of ≤ 64 bits spans at most 9) |
| `crc.rs` `step_bitwise` | `while bit < 8` | constant 8 |
| `crc.rs` `crc16_update_bitwise` | `while let [octet, tail @ ..]` | input length; the slice shrinks by one octet per pass |
| `crc.rs` `TABLE` | `while i < 256` | constant 256; evaluated at compile time |
| `crc.rs` `crc16_update_table` | fold over `data` | input length |
| `oid.rs` `next_octet` (both forms) | `(0..8)` | constant 8 |
| `oid.rs` `fill` (both forms) | over `buf` | output length |
| `container.rs` `FixedQueue::clear` | `while self.pop().is_some()` | ≤ N: each `pop` lowers `len`, and `len` ≤ N |
| `container.rs` `ByteQueue::read_at`, `write_at` | zip of the ring with the buffer | min(buffer length, N) |
| `error.rs` `ErrorKind::from_code`, `from_name` | `find` over `ErrorKind::ALL` | constant: the catalogue size |
| `anomaly.rs` `Iter::next` | `while self.bits != 0`, clearing the lowest set bit each pass | constant 32: the width of the `u32` bit set |
| `anomaly.rs` `FromIterator::from_iter` | `for flag in flags` | input length (the caller's iterator) |
| `security.rs` `FixedPatternSecurity` (feature `test-hooks` only) | 2-element array; `fill` / `all` over each range | 2 × range length, each range checked against the frame length first |

## Recursion

No function calls itself, directly or through another `aldis-core` function. A scan for self-calls flagged only same-named methods of other types: `bits::get` calls `<[u8]>::get`, `SecurityLayout::new` calls `Error::new`, and `Instant`/`Duration::saturating_add` call `u64::saturating_add`. The internal call graph is acyclic and at most three deep (for example `fill` → `next_octet` → `next_bit`, `clear` → `pop`, `crc16` → `crc16_update` → `crc16_update_table`). `AnomalySet::insert` and `contains` call `mask`, which calls `Anomaly::bit`; `Iter::next` calls `Anomaly::from_bit`. Both trait methods are implemented by the codec crates and call nothing in `aldis-core`.

## Blocking

There is no I/O, locking, atomics, sleep, or spin-wait. `std` and `alloc` are only behind features and in test modules. `aldis-core` has no engine operations; every function is O(input length + N) by the table above. GEN-023 applies to engines in full from M3/M4, where VER-012 documents worst-case paths.
