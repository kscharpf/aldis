# Inspection: GEN-022 and GEN-023 in `aldis-uslp`

| | |
|---|---|
| Requirements | GEN-022 (every loop bounded by an input length, a compile-time capacity, or a constant; no recursion), GEN-023 (time bounded by O(input length + capacities); nothing blocks or waits) |
| Scope | `crates/aldis-uslp/src/*.rs` and `src/upid/table.rs`, library code only (test modules excluded), on branch `m2/uslp-codec` (M2 WP-5, modules 1–8, based on `main` 69b1cd4): `upid` (with the generated `upid/table.rs`), `config`, `header`, `view`, `truncated`, `tfdf`, `writer`, `anomaly`, `delimit`, `validate`, `field` (re-exports only), and `lib.rs` (module declarations and re-exports) |
| Inspected | 2026-10-23, prepared by Claude Code: first inspection. Every loop, every call between the crate's own functions, and every blocking construct checked. No unbounded loop, no recursion |
| Approved | — (pending exit review, VER-061) |
| Repeat | when `aldis-uslp` gains a loop or a call between its own functions (the C ABI of module 9 is in `aldis-ffi`), when `cargo xtask sana import` changes the generator's output shape, and before each release |

## Revisions

| Date | Change |
|---|---|
| 2026-10-23, `m2/uslp-codec` (WP-5), prepared by Claude Code, pending exit review | First inspection: modules 1–8 of WP-5 |
| 2026-10-24, `m2/rs020-check` (M2 WP-7, RS-020 check), prepared by Claude Code (agent of M2 WP-7), pending exit review | `tfdf.rs` gains `TfdzOffset`, whose `new`, `get`, and `TryFrom`/`From` impls are straight-line; `FirstHeaderPointer::from_raw` and `LastValidOctet::from_raw` now call `TfdzOffset::new`, their `raw` calls `TfdzOffset::get`, and `writer.rs` `oid_tfdf` and `view.rs` `oid_deviation` call `LastValidOctet::from_raw`. No loop added, no recursion; the call graph below is updated |
| 2026-10-24, `m2/uslp-c-abi` (M2 WP-5 module 9), prepared by Claude Code (agent of WP-5 module 9), pending exit review | `writer.rs` gains `UslpFrameWriter::resume` → `Plan::new`, `start`, and `encode_oid_galois`; `encode_oid` and `encode_oid_galois` → the new private `oid_frame`, which holds the former body of `encode_oid` and fills the TFDZ through its closure argument (`OidGenerator::fill` or `OidGeneratorGalois::fill`); `start` takes a `zero` flag and writes the TFDF Header at its offset in a second `zip`, then zero-fills the Security Header and Trailer only when `begin`, `encode`, and `oid_frame` set it (not `resume`). One loop added, the TFDF Header copy, of at most 3 octets; no recursion |
| 2026-10-24, m2/wp7-checks (WP-7), prepared by the agent, pending exit review | The Loops table gains the first column `ID`, which names each loop that `cargo xtask loops` finds (file, item, and index; GEN-022, M2 plan WP-7), or holds `—` for a row that names none; the other columns are unchanged |
| 2026-10-25, `m2/vector-review` (M2 WP-8, second review of the frame vectors), prepared by Claude Sonnet 5.5 (agent of the review's findings), pending exit review | `config.rs` `VcTable` gains the private `given_vc`, which holds the former body of the `VcLookup::vc` implementation (a `find` over at most `V` entries; the Loops table row moves from `VcTable::vc #1` to `VcTable::given_vc #1`). `VcTable::new` calls `given_vc` where it called `VcLookup::vc`. `VcLookup::vc` for `VcTable` now calls `given_vc` and, when that finds nothing for VCID 63, `PhysicalChannelConfig::is_fixed`, `has_spacecraft` (one `any` over at most `V` entries), and `VcParams::new` (straight-line): VCID 63 is valid on a fixed-length channel without an entry (732.1 Table 5-2 note 2). No new loop, no recursion, no blocking; `writer.rs` `encode_oid` and `view.rs` `decode` reach it through the `VcLookup` methods as before |

## Loops

Every loop in library code, with its bound. Copies and comparisons of fixed-size arrays (the 4- and 14-octet header arrays, the 3-octet TFDF Header, the 4-octet OCF, derived `PartialEq`/`Hash`) run over a constant length and are not listed. Calls into `aldis-core` that loop are listed with the loop they make: `crc::verify_fecf` and `crc::write_fecf` run over the frame, at most 65 536 octets; `OidGenerator::fill` runs over the slice it is given.

| ID | Location | Loop | Bound |
|---|---|---|---|
| — | `config.rs` `VcTable::new` | `[None; V]`, `[None; M]` initialisation | compile-time capacity: `V`, `M` |
| `config.rs VcTable::new #1` | `config.rs` `VcTable::new` | over the given Virtual Channels; each looks the VC up (`given_vc`) before storing it | input length, then capacity: at most `V` iterations store (one more fails with `CapacityExceeded`), each a lookup over at most `V` entries: O(V²) |
| `config.rs VcTable::new #2` | `config.rs` `VcTable::new` | over the given MAPs; each looks up its VC (`given_vc`) and the MAP (`map`) | input length, then capacity: at most `M` iterations store, each O(V + M): O(M · (V + M)) |
| `config.rs VcTable::new #3`, `config.rs VcTable::uses_sdls #1` | `config.rs` `VcTable::new` | over the stored VCs, each with `uses_sdls`, which runs `any` over the stored MAPs | capacities: O(V · M) |
| `config.rs VcTable::has_spacecraft #1`, `config.rs VcTable::given_vc #1`, `config.rs VcTable::map #1` | `config.rs` `VcLookup for VcTable` `has_spacecraft`, `map`; `VcTable::given_vc`, `vcs`, `maps` (`vc` makes the `given_vc` loop, then at most one `has_spacecraft` loop) | `iter().flatten()` with `any` / `find` | compile-time capacity: `V` or `M` |
| `header.rs read_vcf_count #1` | `header.rs` `read_vcf_count` | `fold` over the VCF Count octets | constant: at most 7 |
| `header.rs primary_octets #1` | `header.rs` `primary_octets` | `zip` of the 14-octet array with the header and the count octets | constant: 14 |
| — | `view.rs` `parse_full` (via `crc::verify_fecf`) | CRC over the frame | input length: the frame |
| `writer.rs encode #1` | `writer.rs` `encode` | copies the TFDZ into the reservation (`zip`) | input length: the TFDZ |
| `writer.rs encode_oid #1`, `writer.rs encode_oid_galois #1` | `writer.rs` `oid_frame`, for `encode_oid` and `encode_oid_galois` (via `OidGenerator::fill` or `OidGeneratorGalois::fill`) | fills the TFDZ | input length: the TFDZ, within the caller's buffer |
| `writer.rs encode_truncated #1` | `writer.rs` `encode_truncated` | writes the frame (`zip` over the chained header, zeros, TFDF Header, TFDZ, zeros) | constant: the truncated frame length, at most 32 |
| `writer.rs UslpFrameWriter::start #1 #2 #3 #4` | `writer.rs` `UslpFrameWriter::start` | writes the unfinished header and the Insert Zone (`zip` over a chain), then the TFDF Header (`zip`, at most 3); with `zero` (from `begin`, `encode`, `oid_frame`; not `resume`), `fill(0)` of the Security Header and Trailer | input length: the reserved frame in the caller's buffer; constant 3 |
| `writer.rs UslpFrameWriter::finish #1 #2 #3` | `writer.rs` `UslpFrameWriter::finish` | `copy_within` moving the Security Trailer to follow the TFDZ; the final header (`zip`, at most 14); the OCF (`zip`, 4); `crc::write_fecf` over the frame | input length: the frame; constants 14 and 4 |
| `validate.rs Reason::from_code #1` | `validate.rs` `Reason::from_code` | `ALL.into_iter().find` | constant: 17 |

`upid.rs` and the generated `upid/table.rs` have no loop: `Upid::from_bits`, `Upid::from_field`, `Upid::bits`, and `is_assigned` are `match` expressions over the 32 values. `tfdf.rs`, `truncated.rs`, and `delimit.rs` have no loop: `TfdfHeader::parse` reads one header; `view::take` splits once; `next_frame` reads one header and makes at most one VC lookup; `FrameDelimiter::next` makes one call. A caller that iterates a `FrameDelimiter` or calls `next_frame` repeatedly is bounded by the unit's length, since every item takes at least one octet (`Delimited::len`, never 0), and an `IncompleteFrame`, `NoTruncatedLength`, or `Fill` takes the rest.

## Recursion

No function calls itself, directly or through another `aldis-uslp` function. The internal call graph is acyclic and at most five functions deep, for example `UslpFrame::decode` → `parse` → `parse_full` → `check_channel` → `RawRest::kind` → `FrameKind::from_flags`. The calls between the crate's own functions:

- `UslpFrame::decode`, `validate_frame` → `parse` → `RawStart::parse`, `RawRest::parse`, `RawStart::check_version`, `length`, `fail` (→ `Rejection::new`), `unknown`, `truncated::parse`, `parse_full`.
- `parse_full` → `check_channel` (→ `RawRest::kind`, `VcConfig::vcf_count_len`, `ocf_flag_ok`, `fail`, `unknown`, `mismatch`, `invalid`, and the `VcLookup` methods), `frame_security`, `take`, `TfdfHeader::parse`, `tfdf_rejection`, `oid_deviation` (→ `LastValidOctet::from_raw`), `UslpPrimaryHeader::new`, `read_vcf_count`, `VcConfig::fixed_tfdz`, `PhysicalChannelConfig::is_fixed`, `len_u64`.
- `truncated::parse` → the `VcLookup` methods, `VcConfig::truncated_frame_len`, `TruncatedFrameLen::get`, `frame_security`, `truncated_security`, `TfdfHeader::parse`, `tfdf_rejection`, `length`, `fail`, `unknown`, `invalid`.
- `TfdfHeader::parse` → `ConstructionRule::from_bits`, `ConstructionRule::is_fixed_length` (→ `bits`), `Upid::from_field`, `TfdzRule::new` (→ `FirstHeaderPointer::from_raw`, `LastValidOctet::from_raw`, each → `TfdzOffset::new`), `invalid`; `TfdfHeader::octets`, `len`, `check_pointer` → `TfdzRule::rule`, `TfdzRule::pointer` (→ `FirstHeaderPointer::raw`, `LastValidOctet::raw`, each → `TfdzOffset::get`), `ConstructionRule::bits`, `has_pointer`, `Upid::bits`, `len_u64`.
- `UslpFrame::peek_length` → `RawStart::parse`, `RawStart::check_version`, `TruncatedFrameLen::get`, `unknown`, `invalid`, and the `VcLookup` methods.
- `encode` → `Plan::new`, `UslpFrameWriter::start`, `UslpFrameWriter::tfdz_mut`, `UslpFrameWriter::finish`; `encode_oid` and `encode_oid_galois` → `oid_frame` → `oid_tfdf` (→ `LastValidOctet::from_raw`), `Plan::new`, `start`, `tfdz_mut`, the caller's generator `fill`, `finish`; `UslpFrameWriter::begin` and `UslpFrameWriter::resume` → `Plan::new`, `start`; `Plan::new` → `FrameFields::channel`, `find_vc`, `VcConfig::vcf_count_len`, `VcfCount::new` (→ `VcfCountLen::max_count`), `frame_security`, `TfdfHeader::len`, `TfdfHeader::check_pointer`, `oid_tfdf`, `unknown`, `invalid`, `mismatch`, `len_u64`, and the `VcLookup` methods; `start` → `primary_octets` (→ `start_octets`, `FrameKind::bypass_flag`, `protocol_control_flag`), `TfdfHeader::octets`, `TfdfHeader::len`, `Plan::tfdz_start`, `security_header_mut`, `security_trailer_mut` (→ `Plan::tfdz_start`); `finish` → `TfdfHeader::check_pointer`, `primary_octets`, `Plan::tfdz_start`, `invalid`, `mismatch`, `len_u64`.
- `encode_truncated` → `find_vc`, `VcConfig::truncated_frame_len`, `frame_security`, `truncated_security`, `wrong_tfdz_len`, `start_octets`, `TfdfHeader::octets`, `invalid`, `unknown`.
- `VcTable::new` → `given_vc`, `VcLookup::map` (its own implementation, → `vcs`, `maps`), `check_room` (→ `VcConfig::longest_vcf_count`, `fixed_tfdz`), `uses_sdls`, `PhysicalChannelConfig::is_fixed`, `bad`; `VcConfig::new`, `PhysicalChannelConfig::new`, `TruncatedFrameLen::new` → `bad`.
- The `UslpFrame`, `UslpFullFrame`, `UslpTruncatedFrame`, and `UslpPrimaryHeader` accessors → each other and `start_octets`, `primary_octets`; `UnassignedUpid::new` → `is_assigned`; `Upid::from_bits` → `Upid::from_field`; the `TryFrom` impl of `TfdzOffset` → `new`.
- `FrameDelimiter::next` → `next_frame` → `declared_len` → `RawStart::parse`, `RawRest::parse`, `VcConfig::truncated_frame_len`, and `VcLookup::vc`; `Delimited::len` → `Delimited::octets`.

Calls out of the crate go to `aldis-core` (errors, identifiers, `crc`, `layout` constants, `ocf`, `oid`, `security`, `anomaly`), which has its own inspection, and to the caller's `VcLookup` implementation, whose bounds are the caller's; `VcTable`'s are listed above.

## Blocking

There is no I/O, locking, atomics, sleep, or spin-wait. `std` and `alloc` are only behind features (crate-root `extern crate` declarations) and in test modules. Every operation runs in O(input length + `V` + `M`) or, for `VcTable::new`, O((V + M)²) on capacities fixed at compile time.
