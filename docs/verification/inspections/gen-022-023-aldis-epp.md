# Inspection: GEN-022 and GEN-023 in `aldis-epp`

| | |
|---|---|
| Requirements | GEN-022 (every loop bounded by an input length, a compile-time capacity, or a constant; no recursion), GEN-023 (time bounded by O(input length + capacities); nothing blocks or waits) |
| Scope | `crates/aldis-epp/src/*.rs`, library code only (test modules excluded), at `main` 5f40e8b: the whole crate as released in `v0.2.0-m1`, including the RS-020 changes (issue #22) and the documentation pass of M1 WP-6 (990a4cf, comments only) |
| Inspected | 2026-10-20, prepared by Claude Code: pre-release re-inspection for `v0.2.0-m1` (M1 exit). Every loop, every call between the crate's own functions, and every blocking construct re-checked; the table and the call graph below are confirmed. No unbounded loop, no recursion |
| Approved | 2026-10-20, maintainer |
| Repeat | when `aldis-epp` gains a loop or a call between its own functions, and before each release |
| History | First inspected 2026-10-18 on branch `m1/epp` (M1 WP-3, based on `main` b817d0f), approved 2026-10-18 maintainer; revised since as below |
| Revised | 2026-10-18, PR #18 review: the header-copy loop of `writer.rs` `put_header` and of `EncapsulationHeader::encode` is now the single `EncapsulationHeader::write_to` (same loop, same bound); `EncapsulationWriter::begin` no longer adjusts `max_data_len`. No loop added. Confirmed 2026-10-18, maintainer |
| Revised | 2026-10-19, issue #22 (RS-020): `EppConfig::data_unit` and `EppConfig::header` call `check` → `check_ids` / `check_len`, then (`header`) `EncapsulationHeader::for_data`; `EncapsulationWriter::begin` calls `check_fields` → `check_ids`, and `finish` calls `check_len`; `EncapsulationFields` constructors are straight-line. No loop and no recursion added. Confirmed 2026-10-19, maintainer |

## Loops

Every loop in library code, with its bound. `view.rs`, `config.rs`, and `anomaly.rs` contain none (`EpiSet` and `ExtendedEpiSet` are bit masks; `contains` is one AND); the `small_id!` and `anomaly_flags!` expansions contain none at run time. Copies and comparisons of fixed-size arrays (`to_be_bytes`, derived `Clone`/`PartialEq`/`Hash`) run over a constant length and are not listed.

| Location | Loop | Bound |
|---|---|---|
| `header.rs` `EncapsulationHeader::auto_len` | `find` over the candidate header lengths | constant: at most 3 candidates |
| `header.rs` `EncapsulationHeader::write_to` (used by `encode` and the writers) | zip of the output with the 8-octet header array, `take(n)` | ≤ 8 (header length) |
| `writer.rs` `encode` | zip of the output after the header with `data` | input length (`data`) |
| `writer.rs` `idle` | over the output after the header | output length (`total_len`, checked against `out` first) |

## Recursion

No function calls itself, directly or through another `aldis-epp` function. A scan of the call graph (2026-10-20, by `Type::function`) flagged only delegation to same-named methods of another type, checked by hand: the `EncapsulationPacket` accessors (`epi`, `is_idle`, `extension`, `user_defined`, `ccsds_defined`, `header_len`) call the `EncapsulationHeader` method of the same name (`is_idle` through `Epi::is_idle`). The internal call graph is acyclic; its longest chains have five functions, for example `EppConfig::data_unit` → `EppConfig::check` → `EppConfig::check_ids` → `EpiSet::contains` → `Epi::get`. Other calls: `EppConfig::header` → `check`, then `EncapsulationHeader::for_data`; `EncapsulationWriter::begin` → `EppConfig::check_fields` → `check_ids`; `finish` → `EppConfig::check_len`; `encode` → `EncapsulationHeader::for_data` → `auto_len`; `EncapsulationPacket::decode` and `peek_length` → `EncapsulationHeader::parse`; `EncapsulationPacket::validate` → `EppConfig::check`; `encode`, `idle`, `EncapsulationWriter::begin` and `finish`, and `EncapsulationHeader::encode` → `EncapsulationHeader::write_to` → `EncapsulationHeader::octets`.

## Blocking

There is no I/O, locking, atomics, sleep, or spin-wait. `std` and `alloc` are only behind features and in test modules. Every function is O(input length) or O(1) by the table above.
