# Inspection: GEN-022 and GEN-023 in `aldis-spp`

| | |
|---|---|
| Requirements | GEN-022 (every loop bounded by an input length, a compile-time capacity, or a constant; no recursion), GEN-023 (time bounded by O(input length + capacities); nothing blocks or waits) |
| Scope | `crates/aldis-spp/src/*.rs`, library code only (test modules excluded), at `main` 5f40e8b: the whole crate as released in `v0.2.0-m1`, including segmentation, reassembly, and the time secondary header (PR #42, and PR #45 from its review: 03d6120) and the documentation pass of M1 WP-6 (990a4cf, comments only) |
| Inspected | 2026-10-20, prepared by Claude Code: pre-release re-inspection for `v0.2.0-m1` (M1 exit). Every loop, every call between the crate's own functions, and every blocking construct re-checked; the table and the call graph below are confirmed and made more complete. No unbounded loop, no recursion |
| Approved | pending maintainer review |
| Repeat | when `aldis-spp` gains a loop or a call between its own functions, and before each release |
| History | First inspected 2026-10-19 on branch `m1/spp` (M1 WP-2 part 1, based on `main` a8e9ad8), approved 2026-10-19 maintainer; revised since as below |
| Revised | 2026-10-19, M1 WP-2 part 2 (branch `m1/spp-part2`): `segment.rs` adds one loop, the copy in `Reassembler::append` (below); `Segmenter::next_packet` writes one packet per call through `encode`, so the caller drives the segment loop. `secondary.rs` adds no loop: the time layout calls the `aldis-time` decoders, whose loops the [`aldis-time` inspection](gen-022-023-aldis-time.md) bounds (a P-field of at most 8 octets, a T-field of at most 17). No recursion. Confirmed 2026-10-19, maintainer |

## Loops

Every loop in library code, with its bound. `header.rs`, `view.rs`, `secondary.rs`, `config.rs`, `octet.rs`, and `anomaly.rs` contain none, and `Segmenter` loops only through its caller; the `anomaly_flags!` expansion contains none at run time. Copies and comparisons of fixed-size arrays (`to_be_bytes`, derived `Clone`/`PartialEq`/`Hash`) run over a constant length and are listed only where the length is a capacity.

| Location | Loop | Bound |
|---|---|---|
| `writer.rs` `write_packet` (used by `encode` and `OctetStringEncoder::encode`) | zip of the output with the 6 header octets chained with the two parts | total packet length ≤ 65 542 (`SppConfig` checks it before the call) |
| `writer.rs` `SpacePacketWriter::begin`, `finish` | zip of the buffer with the 6 header octets | constant 6 |
| `idle.rs` `IdlePacket::encode` | zip of the output with the 6 header octets chained with the pattern, `cycle().take(n)` | total packet length ≤ 65 542 (`IdlePacket::new` checks it); `take` bounds the cycle |
| `sequence.rs` `SequenceTracker::track`, `last` | `find` over the tracked APIDs | capacity `N` (const generic) |
| `sequence.rs` `SequenceTracker::forget` | `position` over the tracked APIDs | capacity `N` |
| `sequence.rs` `SequenceTracker::new`, derived `PartialEq` | the `aldis-core` `FixedVec` initialiser and slice comparison | capacity `N` |
| `segment.rs` `Segmenter::next_packet` | none in the call: one packet per call through `encode` (`write_packet` above); draining an SDU takes ⌈SDU length / `max_user_len`⌉ calls, and `max_user_len` ≥ 1 (`Segmenter::new` checks it) | — |
| `segment.rs` `Reassembler::append` | zip of the buffer slice with one packet's user data | user data length ≤ 65 536 and the caller's buffer length (the slice is taken only if it fits) |
| `secondary.rs` `TimeHeaderLayout::decode` (feature `time`) | none in `aldis-spp`: calls the `aldis-time` decoders and `TimeFormat::pfield`, whose loops the [`aldis-time` inspection](gen-022-023-aldis-time.md) bounds (constant 7, 8, or 17) | — |

## Recursion

No function calls itself, directly or through another `aldis-spp` function. A scan of the call graph (2026-10-20, by `Type::function`) flagged only delegation to same-named methods of other types, each checked by hand: the `SpacePacket` accessors (`apid`, `packet_type`, `sequence_count`, …, `is_idle`) call the `SpacePacketHeader` method of the same name; `SpacePacketHeader::is_idle` calls the `aldis-core` `Apid::is_idle`; `SequenceTracker::len` and `is_empty` call `FixedVec`; `SpacePacket::split` calls `SecondaryHeaderLayout::split`. The internal call graph is acyclic; its longest chain has five functions, `Segmenter::next_packet` → `encode` → `write_packet` → `SpacePacketHeader::octets` → `octets_with_version`. The calls between the crate's functions:
- `SpacePacket::decode` and `peek_length` → `SpacePacketHeader::parse`.
- `SpacePacket::split` → `SecondaryHeaderLayout::split` → `invalid`.
- `SppConfig::packet` → `check_parts` and `SppConfig::check_len`.
- `encode` → `write_packet` → `SpacePacketHeader::octets` → `octets_with_version`.
- `OctetStringEncoder::encode` → `check_parts`, `check_len`, and `write_packet`; then `SequenceCounter::advance`.
- `SequenceTracker::track` → the `aldis-core` `SeqCount::classify` and `FixedVec::push`.
- `Segmenter::next_packet` → `SppConfig::packet` and `encode`, then `SequenceCounter::advance`.
- `Reassembler::push` → `SeqCount::classify` and `Reassembler::append`.
- `SpacePacket::split` → `SecondaryHeaderLayout::split` → `TimeHeaderLayout::decode` (and `too_short`) → the `aldis-time` `PField::parse`, `TimeFormat::pfield`, `decode_with_pfield`, or `decode_implicit`, which do not call back into `aldis-spp`.

## Blocking

There is no I/O, locking, atomics, sleep, spin-wait, or clock. `std` and `alloc` are only behind features and in test modules. Every function is O(packet length), O(N), or O(1) by the table above.
