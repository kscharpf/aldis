# Inspection: GEN-022 and GEN-023 in `aldis-tc`

| | |
|---|---|
| Requirements | GEN-022 (every loop bounded by an input length, a compile-time capacity, or a constant; no recursion), GEN-023 (time bounded by O(input length + capacities); nothing blocks or waits) |
| Scope | `crates/aldis-tc/src/*.rs`, library code only (test modules excluded), on branch `m2/tc-codec` (M2 WP-3, modules 1–7, based on `main` 46bfe0f): `config`, `header`, `view`, `writer`, `control`, `anomaly`, `clcw`, `delimit`, `validate`, `field` (re-exports only), and `lib.rs` (module declarations and re-exports) |
| Inspected | 2026-10-23, prepared by Claude Code: first inspection. Every loop, every call between the crate's own functions, and every blocking construct checked. No unbounded loop, no recursion |
| Revised | 2026-10-25, `m2/hc-finish` (OI-50), prepared by Claude Sonnet 5.5 (agent of OI-50), pending exit review: `writer.rs` `TcFrameWriter::start` takes a `write` flag; with it (from `begin`, `begin_oid`, `encode`) it zero-fills and writes the headers as before, without it (from `resume`) it writes nothing and compares the headers with an `all` over a `zip`, at most 6 octets. The unfinished header now holds the reserved frame length: `primary_octets` is called with `plan.total`, no new call. No recursion; the Loops table is updated |
| Approved | — (pending exit review, VER-061) |
| Repeat | when `aldis-tc` gains a loop or a call between its own functions (the C ABI of module 8 is in `aldis-ffi`), and before each release |

## Revisions

| Date | Change |
|---|---|
| 2026-10-23, `m2/tc-codec` (WP-3), prepared by Claude Code, pending exit review | First inspection: modules 1–7 of WP-3 |
| 2026-10-24, `m2/tc-c-abi` (WP-3 module 8), prepared by Claude Code (agent of WP-3 module 8), pending exit review | For the C ABI: `view.rs` adds `TcFrame::peek_header` → `RawHeader::parse`, `check_version`, `frame_type`, `TcFrameHeader::new`, and `peek_length` now → `peek_header`, `TcFrameHeader::frame_len`; `writer.rs` adds `TcFrameWriter::resume` → `Plan::new`, `TcFrameWriter::start`, and `start` takes a `write` flag: the unfinished header, the Segment Header, and the zeros of the Security Header and Trailer are written only for `begin` and `encode`. No loop added (the existing ones in `start` are now conditional), no recursion |
| 2026-10-24, m2/wp7-checks (WP-7), prepared by the agent, pending exit review | The Loops table gains the first column `ID`, which names each loop that `cargo xtask loops` finds (file, item, and index; GEN-022, M2 plan WP-7), or holds `—` for a row that names none; the other columns are unchanged |

## Loops

Every loop in library code, with its bound. Copies and comparisons of fixed-size arrays (the five header octets, the four CLCW octets, the three Control Command octets, derived `PartialEq`/`Hash`) run over a constant length and are not listed. Calls into `aldis-core` that loop are listed with the loop they make: `crc::verify_fecf` and `crc::write_fecf` run over the frame, at most 1024 octets; `FrameLayout::compute` has no loop.

| ID | Location | Loop | Bound |
|---|---|---|---|
| — | `config.rs` `VcTable::new` | `[None; V]`, `[None; M]` initialisation | compile-time capacity: `V`, `M` |
| `config.rs VcTable::new #1` | `config.rs` `VcTable::new` | over the given Virtual Channels; each looks the VC up (`vc`) before storing it | input length, then capacity: at most `V` iterations store (one more fails with `CapacityExceeded`), each a lookup over at most `V` entries: O(V²) |
| `config.rs VcTable::new #2` | `config.rs` `VcTable::new` | over the given MAPs; each looks up its VC (`vc`) and the MAP (`map`) | input length, then capacity: at most `M` iterations store, each O(V + M): O(M · (V + M)) |
| `config.rs VcTable::has_spacecraft #1`, `config.rs VcTable::vc #1`, `config.rs VcTable::map #1` | `config.rs` `VcLookup for VcTable` `has_spacecraft`, `vc`, `map`; `VcTable::vcs`, `maps` | `iter().flatten()` with `any` / `find` | compile-time capacity: `V` or `M` |
| — | `view.rs` `parse` (via `crc::verify_fecf`) | CRC over the frame | input length: the frame, at most 1024 octets |
| `writer.rs encode #1` | `writer.rs` `encode` | copies the User Data or Control Command into the reservation (`zip`) | input length: the payload |
| `writer.rs TcFrameWriter::start #1` | `writer.rs` `TcFrameWriter::start` | writes the unfinished header and the Segment Header (`zip` over `header.chain(segment)`), when called by `begin` or `encode` (not by `resume`) | constant: at most 6 |
| `writer.rs TcFrameWriter::start #2` | `writer.rs` `TcFrameWriter::start` | when called by `resume`: `all` over the unfinished header and the Segment Header zipped with the expected octets | constant: at most 6 |
| `writer.rs TcFrameWriter::start #3 #4` | `writer.rs` `TcFrameWriter::start` | `fill(0)` of the Security Header and Trailer, when called by `begin` or `encode` (not by `resume`) | input length: the reserved frame in the caller's buffer, at most 1024 octets |
| `writer.rs TcFrameWriter::finish #1` | `writer.rs` `TcFrameWriter::finish` | `copy_within` moving the Security Trailer to follow the User Data | input length: the unused User Data and the trailer, within the frame |
| `writer.rs TcFrameWriter::finish #2` | `writer.rs` `TcFrameWriter::finish` | writes the final header (`zip` over 5 octets); `crc::write_fecf` over the frame | constant: 5; input length: the frame |
| `clcw.rs Clcw::decode #1` | `clcw.rs` `Clcw::decode` | over the two Reserved Spare fields | constant: 2 |
| `validate.rs Reason::from_code #1` | `validate.rs` `Reason::from_code` | `ALL.into_iter().find` | constant: 11 |

`delimit.rs` has no loop: `next_frame` reads one header; `FrameDelimiter::next` makes one call. A caller that iterates a `FrameDelimiter` or calls `next_frame` repeatedly is bounded by the unit's length, since every item takes at least one octet (`Delimited::len`, never 0), and a `TruncatedFrame` or `Fill` takes the rest.

## Recursion

No function calls itself, directly or through another `aldis-tc` function. The internal call graph is acyclic and at most five functions deep, for example `TcFrame::decode` → `parse` → `check_head` → `RawHeader::frame_type` → `FrameType::from_flags`. The calls between the crate's own functions:

- `TcFrame::decode`, `validate_frame` → `parse` → `check_head`, `length`, `fail` (→ `Rejection::new`), `too_short`, `control_command` (→ `ControlCommand::parse`), `SegmentHeader::from_octet` (→ `SequenceFlags::from_bits`), `VcConfig::security`, `MapConfig::security`, `TcFrameHeader::new`, `unknown`, `reserved`, `invalid`; `check_head` → `RawHeader::parse`, `RawHeader::check_version`, `RawHeader::frame_type` (→ `FrameType::from_flags`, `reserved`), `VcConfig::has_segment_header`, `VcConfig::max_frame_len`, `PhysicalChannelConfig::max_frame_len`, `fecf`, `len_u64`, and the `VcLookup` methods.
- `TcFrame::peek_length` → `TcFrame::peek_header`, `TcFrameHeader::frame_len`; `peek_header` → `RawHeader::parse`, `RawHeader::check_version`, `RawHeader::frame_type`, `TcFrameHeader::new`.
- `encode` → `ControlCommand::to_octets`, `ControlOctets::as_slice`, `Plan::new`, `TcFrameWriter::start`, `TcFrameWriter::user_data_mut`, `TcFrameWriter::finish`; `TcFrameWriter::begin` and `TcFrameWriter::resume` → `Plan::new`, `TcFrameWriter::start`; `Plan::new` → `FrameType::control_command_flag`, `VcConfig::has_segment_header`, `VcConfig::security`, `MapConfig::security`, `unknown`, `invalid`, `len_u64`, and the `VcLookup` methods; `TcFrameWriter::start` → `primary_octets` (→ `FrameType::bypass_flag`, `control_command_flag`), `SegmentHeader::octet` (→ `SequenceFlags::bits`), `security_header_mut`, `security_trailer_mut`; `TcFrameWriter::finish` → `primary_octets`, `invalid`, `len_u64`.
- `VcTable::new` → `VcLookup::vc`, `VcLookup::map` (its own implementation, → `vcs`, `maps`), `VcConfig::has_segment_header`, `VcConfig::security`, `check_room`, `bad_max_len` (through `PhysicalChannelConfig::new`, `VcConfig::new`).
- `TcFrameHeader::primary_header_octets` → `primary_octets`, `TcFrameHeader::frame_len`; the `TcFrame` accessors → the `TcFrameHeader` accessors.
- `Clcw::decode` → `fixed`, `reserved`, `ClcwStatus::masked`, `CopInEffect::masked`, `FarmBCounter::masked`; `decode_report` → `decode`; `to_word`, `to_ocf` → `to_octets` → `flag`; the `TryFrom` implementations → `new`.
- `FrameDelimiter::next` → `next_frame` → `RawHeader::parse`; `Delimited::len` → `Delimited::octets`.

Calls out of the crate go to `aldis-core` (errors, identifiers, `crc`, `layout`, `ocf`, `anomaly`), which has its own inspection, and to the caller's `VcLookup` implementation, whose bounds are the caller's; `VcTable`'s are listed above.

## Blocking

There is no I/O, locking, atomics, sleep, or spin-wait. `std` and `alloc` are only behind features (crate-root `extern crate` declarations) and in test modules. Every operation runs in O(input length + `V` + `M`) or, for `VcTable::new`, O((V + M)²) on capacities fixed at compile time.
