# Inspection: GEN-022 and GEN-023 in `aldis-time`

| | |
|---|---|
| Requirements | GEN-022 (every loop bounded by an input length, a compile-time capacity, or a constant; no recursion), GEN-023 (time bounded by O(input length + capacities); nothing blocks or waits) |
| Scope | `crates/aldis-time/src/*.rs`, library code only (test modules excluded), on branch `m1/time` (M1 WP-1 part 1: `pfield`, `code`, `cuc`, `cds`, `ccs`, `util`), based on `main` b817d0f. `ascii.rs` is still a skeleton |
| Inspected | 2026-10-18, prepared by Claude Code |
| Approved | awaiting maintainer approval |
| Repeat | when `aldis-time` gains a loop or a call between its own functions (the ASCII codec and conversions will), and before each release |

## Loops

Every loop in library code, with its bound. `util.rs` and `anomaly.rs` contain none.

| Location | Loop | Bound |
|---|---|---|
| `pfield.rs` `PField::parse` | zip of the 7 extension slots with the input | constant 7 |
| `pfield.rs` `PField::octets` | zip of the 8-octet output with the P-field | constant 8 |
| `pfield.rs` `PField::build` | zip of the 7 slots with defined and tail octets; flag pass `take(n)` | constant 7 each |
| `pfield.rs` `PFieldTail::new`, `from_chain` | zip of the 7 slots with the given octets | constant 7 |
| `code.rs` `Agency::from_t` | zip of the 16-octet array with the T-field | constant 16 |
| `code.rs` `TimeCode::t_octets` | zip of the 17-octet output with the code's T-field | constant 17 |
| `code.rs` `TimeCode::encode` | zip of the output with P-field and T-field octets | constant 8 + 17 |
| `cuc.rs` `Cuc::t_octets` | zip of the 17-octet output with coarse and fine octets | constant 17 |
| `cuc.rs` `Cuc::from_t` | folds over the coarse and fine octets | T-field length ≤ 17 |
| `cds.rs` `Cds::t_octets` | zip of the 11-octet output with the segments | constant 11 |
| `cds.rs` `Cds::from_t` | three folds over `take(n)` of the T-field | T-field length ≤ 11 |
| `ccs.rs` `Ccs::t_octets` | over the sub-second octets, `take(n)` | constant 6 |
| `ccs.rs` `Ccs::from_t` | `for _ in 0..format.subsecond` | constant 6 (`CcsFormat` rejects more) |

## Recursion

No function calls itself, directly or through another `aldis-time` function. The internal call graph is acyclic and at most four deep, for example `decode_with_pfield` → `decode_t` → `Ccs::from_t` → `Ccs::new` → `validate`, and `TimeCode::encode` → `TimeCode::pfield` → `CucFormat::pfield` → `PField::build`.

## Blocking

There is no I/O, locking, atomics, sleep, spin-wait, clock, or time zone (TIME-004). `std` and `alloc` are only behind features and in test modules. Every function is O(1): the longest input any decoder reads is an 8-octet P-field and a 17-octet T-field.
