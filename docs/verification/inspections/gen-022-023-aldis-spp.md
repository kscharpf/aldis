# Inspection: GEN-022 and GEN-023 in `aldis-spp`

| | |
|---|---|
| Requirements | GEN-022 (every loop bounded by an input length, a compile-time capacity, or a constant; no recursion), GEN-023 (time bounded by O(input length + capacities); nothing blocks or waits) |
| Scope | `crates/aldis-spp/src/*.rs`, library code only (test modules excluded), on branch `m1/spp` (M1 WP-2 part 1: `header`, `view`, `secondary`, `writer`, `config`, `idle`, `sequence`, `octet`), based on `main` a8e9ad8. `segment.rs` is still a skeleton, and the time secondary header is not yet implemented |
| Inspected | 2026-10-19, prepared by Claude Code |
| Approved | 2026-10-19, maintainer |
| Repeat | when `aldis-spp` gains a loop or a call between its own functions (segmentation, reassembly, and the time secondary header will), and before each release |

## Loops

Every loop in library code, with its bound. `header.rs`, `view.rs`, `secondary.rs`, `config.rs`, `octet.rs`, and `anomaly.rs` contain none; the `anomaly_flags!` expansion contains none at run time.

| Location | Loop | Bound |
|---|---|---|
| `writer.rs` `write_packet` (used by `encode` and `OctetStringEncoder::encode`) | zip of the output with the 6 header octets chained with the two parts | total packet length ≤ 65 542 (`SppConfig` checks it before the call) |
| `writer.rs` `SpacePacketWriter::begin`, `finish` | zip of the buffer with the 6 header octets | constant 6 |
| `idle.rs` `IdlePacket::encode` | zip of the output with the 6 header octets chained with the pattern, `cycle().take(n)` | total packet length ≤ 65 542 (`IdlePacket::new` checks it); `take` bounds the cycle |
| `sequence.rs` `SequenceTracker::track`, `last` | `find` over the tracked APIDs | capacity `N` (const generic) |
| `sequence.rs` `SequenceTracker::forget` | `position` over the tracked APIDs | capacity `N` |

## Recursion

No function calls itself, directly or through another `aldis-spp` function. The internal call graph is acyclic and at most three deep:
- `SpacePacket::decode` and `peek_length` → `SpacePacketHeader::parse`.
- `SpacePacket::split` → `SecondaryHeaderLayout::split` → `invalid`.
- `SppConfig::packet` → `check_parts` and `SppConfig::check_len`.
- `encode` → `write_packet` → `SpacePacketHeader::octets` → `octets_with_version`.
- `OctetStringEncoder::encode` → `check_parts`, `check_len`, and `write_packet`; then `SequenceCounter::advance`.
- `SequenceTracker::track` → the `aldis-core` `SeqCount::classify` and `FixedVec::push`.

## Blocking

There is no I/O, locking, atomics, sleep, spin-wait, or clock. `std` and `alloc` are only behind features and in test modules. Every function is O(packet length), O(N), or O(1) by the table above.
