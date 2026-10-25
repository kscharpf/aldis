# Inspection: GEN-022 and GEN-023 in `aldis-time`

| | |
|---|---|
| Requirements | GEN-022 (every loop bounded by an input length, a compile-time capacity, or a constant; no recursion), GEN-023 (time bounded by O(input length + capacities); nothing blocks or waits) |
| Scope | `crates/aldis-time/src/*.rs`, library code only (test modules excluded), at `main` 5f40e8b: the whole crate as released in `v0.2.0-m1`, including the ASCII codes and conversions (PR #30) and the documentation pass of M1 WP-6 (990a4cf, comments only) |
| Inspected | 2026-10-20, prepared by Claude Code: pre-release re-inspection for `v0.2.0-m1` (M1 exit). Every loop, every call between the crate's own functions, and every blocking construct re-checked; the table and the call graph below are confirmed. No unbounded loop, no recursion |
| Approved | 2026-10-20, maintainer |
| Repeat | when `aldis-time` gains a loop or a call between its own functions, and before each release |
| History | First inspected 2026-10-18 on branch `m1/time` (M1 WP-1 part 1, based on `main` b817d0f), approved 2026-10-19 maintainer; revised since as below |
| Revised | 2026-10-19, value types (parse, don't validate): new module `calendar.rs` (range-checked `Year`, `Month`, `Hour`, `Minute`, `Second`, `CalendarDate`; the leap-year rules moved here from `util.rs`) and the `bounded!` macro in `util.rs` add no loop and no recursion. `ccs.rs` `validate` is removed; `Ccs::from_t` and `Cds::from_t` call the field constructors directly. Loops unchanged. Confirmed 2026-10-19, maintainer |
| Revised | 2026-10-19, issue #22 (RS-020): `ReservedPField::new` calls `PField::id` → `TimeCodeId::is_reserved`; `Cuc::from_seconds_nanos` (moved from `CucValue`) calls `check_coarse`; `TimeCodeId` matches are exhaustive without loops. No loop and no recursion added. Confirmed 2026-10-19, maintainer |
| Revised | 2026-10-19, M1 WP-1 part 2 (branch `m1/time-ascii`): `ascii.rs` (ASCII codes A and B and subsets) adds the loops listed below, each bounded by the input length or a constant; `convert.rs` (`UtcInstant`) and the closed-form day functions in `calendar.rs` (`day_number`, `civil`; M1 D3) add none; `util.rs` `bounded!` gains a `Default` impl, no loop. No recursion. Confirmed 2026-10-19, maintainer |
| Revised | 2026-10-21, `m2/cuc-round-down` (OI-40), prepared by Claude Code, pending exit review: `Cuc::from_seconds_nanos` rounds the fraction down; the carry branch and its `range` call for `coarse` are removed, and it calls `check_coarse` on `seconds` before computing the fraction. No loop and no recursion added |
| Revised | 2026-10-24, m2/wp7-checks (WP-7), prepared by the agent, pending exit review: the Loops table gains the first column `ID`, which names each loop that `cargo xtask loops` finds (file, item, and index; GEN-022, M2 plan WP-7), or holds `—` for a row that names none; the other columns are unchanged |

## Loops

Every loop in library code, with its bound. `util.rs`, `calendar.rs`, `convert.rs`, and `anomaly.rs` contain none (`pow10` in `ascii.rs` is a `match`). Copies and comparisons of fixed-size arrays (`to_be_bytes`, derived `Clone`/`PartialEq`/`Hash`) run over a constant length and are not listed.

| ID | Location | Loop | Bound |
|---|---|---|---|
| `pfield.rs PField::parse #1` | `pfield.rs` `PField::parse` | zip of the 7 extension slots with the input | constant 7 |
| `pfield.rs PField::octets #1` | `pfield.rs` `PField::octets` | zip of the 8-octet output with the P-field | constant 8 |
| `pfield.rs PField::build #1 #2` | `pfield.rs` `PField::build` | zip of the 7 slots with defined and tail octets; flag pass `take(n)` | constant 7 each |
| `pfield.rs PFieldTail::new #1`, `pfield.rs PFieldTail::from_chain #1` | `pfield.rs` `PFieldTail::new`, `from_chain` | zip of the 7 slots with the given octets | constant 7 |
| `code.rs Agency::from_t #1` | `code.rs` `Agency::from_t` | zip of the 16-octet array with the T-field | constant 16 |
| `code.rs TimeCode::t_octets #1` | `code.rs` `TimeCode::t_octets` | zip of the 17-octet output with the code's T-field | constant 17 |
| `code.rs TimeCode::encode #1` | `code.rs` `TimeCode::encode` | zip of the output with P-field and T-field octets | constant 8 + 17 |
| `cuc.rs Cuc::t_octets #1` | `cuc.rs` `Cuc::t_octets` | zip of the 17-octet output with coarse and fine octets | constant 17 |
| `cuc.rs Cuc::from_t #1 #2` | `cuc.rs` `Cuc::from_t` | folds over the coarse and fine octets | T-field length ≤ 17 |
| `cds.rs Cds::t_octets #1` | `cds.rs` `Cds::t_octets` | zip of the 11-octet output with the segments | constant 11 |
| `cds.rs Cds::from_t #1` | `cds.rs` `Cds::from_t` | three folds over `take(n)` of the T-field | T-field length ≤ 11 |
| `ccs.rs Ccs::t_octets #1` | `ccs.rs` `Ccs::t_octets` | over the sub-second octets, `take(n)` | constant 6 |
| `ccs.rs Ccs::from_t #1` | `ccs.rs` `Ccs::from_t` | `for _ in 0..format.subsecond` | constant 6 (`CcsFormat` rejects more) |
| `ascii.rs number #1` | `ascii.rs` `number` | `try_fold` over a subfield's digits | at most 4 (callers pass fixed-width subfields: arrays in `AsciiTime::parse`, shape-table ranges in `Raw::read`) |
| `ascii.rs fraction_digits #1 #2` | `ascii.rs` `fraction_digits` | `contains`, `all`, then `fold` over the digits | input length; the `fold` runs only after the check against `AsciiOptions::max_fraction_digits` (≤ 12), so at most 12 |
| — | `ascii.rs` `dec3`, `dec4` | `array::map` over 3 or 4 digits | constant 3, 4 |
| `ascii.rs dec12 #1` | `ascii.rs` `dec12` | `for` over the 12 output digits | constant 12 |
| `ascii.rs AsciiTime::encode #1` | `ascii.rs` `AsciiTime::encode` | zip of the output with the chained date, clock, fraction, and `Z` octets | constant 33 (`MAX_ASCII_LEN`) |
| `ascii.rs shape_of #1 #2` | `ascii.rs` `shape_of` | `find` over the shape table; `zip`/`all` over a pattern | 8 shapes × 10 octets |
| `ascii.rs Raw::read #1` | `ascii.rs` `Raw::read` | `for` over a shape's subfields | constant 3 |
| `ascii.rs AsciiSubset::parse #1` | `ascii.rs` `AsciiSubset::parse` | `filter`/`nth(1)` for a second `T`; `splitn(2, …)` at `T` and at `.`; at most three `shape_of` calls | input length |

## Recursion

No function calls itself, directly or through another `aldis-time` function. A scan of the call graph (2026-10-20, by `Type::function`) flagged only delegation to same-named methods of other types, each checked by hand: `Cuc::to_seconds_nanos` calls `CucValue::to_seconds_nanos`; `TimeCode::format` calls `format` on the code (`Cuc`, `Cds`, `Ccs`, `Agency`); `TimeCode::pfield`, `TimeFormat::pfield`, and `TimeFormat::t_len` call the method of the same name on the code's format (`CucFormat`, `CdsFormat`, `CcsFormat`, `AgencyFormat`) or on `ReservedPField`; none of these calls back into `TimeCode` or `TimeFormat`; `SubMillis::get` calls the `get` of `Micros` or `Picos`. The internal call graph is acyclic; its longest chains have six functions, for example `decode_with_pfield` → `decode_t` → `Ccs::from_t` → `CalendarDate::from_month_day` → `days_in_month` → `is_leap`; shorter ones include `TimeCode::encode` → `TimeCode::pfield` → `CucFormat::pfield` → `PField::build`. The ASCII and conversion paths add `AsciiSubset::parse` → `Raw::read` → `number` and `UtcInstant::to_ascii` → `UtcInstant::date` → `civil` and `day_number`.

## Blocking

There is no I/O, locking, atomics, sleep, spin-wait, clock, or time zone (TIME-004). `std` and `alloc` are only behind features and in test modules. Every binary-code function is O(1): the longest input any decoder reads is an 8-octet P-field and a 17-octet T-field. The ASCII parsers are O(input length), with a single pass over the input to reject it before any per-subfield work; the conversions are O(1) (closed-form day arithmetic, M1 D3).
