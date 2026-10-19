# Inspection: GEN-022 and GEN-023 in `aldis-epp`

| | |
|---|---|
| Requirements | GEN-022 (every loop bounded by an input length, a compile-time capacity, or a constant; no recursion), GEN-023 (time bounded by O(input length + capacities); nothing blocks or waits) |
| Scope | `crates/aldis-epp/src/*.rs`, library code only (test modules excluded), on branch `m1/epp` (M1 WP-3), based on `main` b817d0f |
| Inspected | 2026-10-18, prepared by Claude Code |
| Approved | awaiting maintainer approval |
| Repeat | when `aldis-epp` gains a loop or a call between its own functions, and before each release |

## Loops

Every loop in library code, with its bound. `view.rs`, `config.rs`, and `anomaly.rs` contain none; the `small_id!` and `anomaly_flags!` expansions contain none at run time.

| Location | Loop | Bound |
|---|---|---|
| `header.rs` `EncapsulationHeader::auto_len` | `find` over the candidate header lengths | constant: at most 3 candidates |
| `header.rs` `EncapsulationHeader::encode` | zip of the output with the 8-octet header array | ≤ 8 (header length) |
| `writer.rs` `put_header` | zip of the output with the header array, `take(n)` | ≤ 8 (header length) |
| `writer.rs` `encode` | zip of the output after the header with `data` | input length (`data`) |
| `writer.rs` `idle` | over the output after the header | output length (`total_len`, checked against `out` first) |

## Recursion

No function calls itself, directly or through another `aldis-epp` function. The internal call graph is acyclic and at most three deep: `encode` → `EncapsulationHeader::for_data` → `auto_len`; `EncapsulationPacket::decode` and `peek_length` → `EncapsulationHeader::parse`; `EncapsulationPacket::validate` → `EppConfig::check`; `EncapsulationWriter::begin` and `finish` → `put_header` → `EncapsulationHeader::octets`.

## Blocking

There is no I/O, locking, atomics, sleep, or spin-wait. `std` and `alloc` are only behind features and in test modules. Every function is O(input length) or O(1) by the table above.
