# Inspection: GEN-022 and GEN-023 in `aldis-spp`

| | |
|---|---|
| Requirements | GEN-022 (every loop bounded by an input length, a compile-time capacity, or a constant; no recursion), GEN-023 (time bounded by O(input length + capacities); nothing blocks or waits) |
| Scope | `crates/aldis-spp/src/*.rs`, library code only (test modules excluded), on branch `m1/spp` (M1 WP-2 part 1: `header`, `view`, `secondary`, `writer`, `config`, `idle`, `sequence`, `octet`), based on `main` a8e9ad8. `segment.rs` is still a skeleton, and the time secondary header is not yet implemented |
| Inspected | 2026-10-19, prepared by Claude Code |
| Approved | 2026-10-19, maintainer |
| Repeat | when `aldis-spp` gains a loop or a call between its own functions, and before each release |
| Revised | 2026-10-19, M1 WP-2 part 2 (branch `m1/spp-part2`): `segment.rs` adds one loop, the copy in `Reassembler::append` (below); `Segmenter::next_packet` writes one packet per call through `encode`, so the caller drives the segment loop. `secondary.rs` adds no loop: the time layout calls the `aldis-time` decoders, whose loops the [`aldis-time` inspection](gen-022-023-aldis-time.md) bounds (a P-field of at most 8 octets, a T-field of at most 17). No recursion. Confirmed 2026-10-19, maintainer |

## Loops

Every loop in library code, with its bound. `header.rs`, `view.rs`, `secondary.rs`, `config.rs`, `octet.rs`, and `anomaly.rs` contain none, and `Segmenter` loops only through its caller; the `anomaly_flags!` expansion contains none at run time.

| Location | Loop | Bound |
|---|---|---|
| `writer.rs` `write_packet` (used by `encode` and `OctetStringEncoder::encode`) | zip of the output with the 6 header octets chained with the two parts | total packet length ≤ 65 542 (`SppConfig` checks it before the call) |
| `writer.rs` `SpacePacketWriter::begin`, `finish` | zip of the buffer with the 6 header octets | constant 6 |
| `idle.rs` `IdlePacket::encode` | zip of the output with the 6 header octets chained with the pattern, `cycle().take(n)` | total packet length ≤ 65 542 (`IdlePacket::new` checks it); `take` bounds the cycle |
| `sequence.rs` `SequenceTracker::track`, `last` | `find` over the tracked APIDs | capacity `N` (const generic) |
| `sequence.rs` `SequenceTracker::forget` | `position` over the tracked APIDs | capacity `N` |
| `segment.rs` `Reassembler::append` | zip of the buffer slice with one packet's user data | user data length ≤ 65 536 and the caller's buffer length |

## Recursion

No function calls itself, directly or through another `aldis-spp` function. The internal call graph is acyclic and at most three deep:
- `SpacePacket::decode` and `peek_length` → `SpacePacketHeader::parse`.
- `SpacePacket::split` → `SecondaryHeaderLayout::split` → `invalid`.
- `SppConfig::packet` → `check_parts` and `SppConfig::check_len`.
- `encode` → `write_packet` → `SpacePacketHeader::octets` → `octets_with_version`.
- `OctetStringEncoder::encode` → `check_parts`, `check_len`, and `write_packet`; then `SequenceCounter::advance`.
- `SequenceTracker::track` → the `aldis-core` `SeqCount::classify` and `FixedVec::push`.
- `Segmenter::next_packet` → `SppConfig::packet` and `encode`, then `SequenceCounter::advance`.
- `Reassembler::push` → `SeqCount::classify` and `Reassembler::append`.
- `SecondaryHeaderLayout::split` → `TimeHeaderLayout::decode` → the `aldis-time` `PField::parse`, `decode_with_pfield`, or `decode_implicit`, which do not call back into `aldis-spp`.

## Blocking

There is no I/O, locking, atomics, sleep, spin-wait, or clock. `std` and `alloc` are only behind features and in test modules. Every function is O(packet length), O(N), or O(1) by the table above.
