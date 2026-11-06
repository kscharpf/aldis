# TM synchronization: conformance vectors

Vectors for the SYNC functions of [tm-sync.md](../../docs/standards/tm-sync.md): the ASM attach (CADU), the two pseudo-random sequences, and the ASM search engine (SYNC-001…SYNC-020; CCSDS 131.0-B-6 §9–§11). Every file is written by the independent model [`../models/sync.py`](../models/sync.py) (TV-023), which was written blind: from the Blue Books and `docs/` only, by an agent that had read no implementation. The model checks its own sequences against the 40-bit notes of 131.0 §10.4.3 and its search engine against a second, offline formulation.

**The JSON is never edited by hand.** `python3 -I test-vectors/models/sync.py --write` writes every file here in a fixed order and format; `--check test-vectors/sync/*.json test-vectors/sync/scenarios/*.json` replays every case and compares each file with what the generator writes (`cargo xtask vectors validate` runs it; `cargo xtask vectors generated` regenerates the directory).

No runner exists yet: `AWAITING_RUNNER` in `xtask/src/consistency.rs` waives the test requirement for `sync/` until `aldis-sync` (M5 WP-3) and its C ABI (WP-4) add one. The engine scenarios are in `scenarios/`, below the area directory, as for `cop1/`, so that the exerciser's build script does not read them as codec files.

## Codec files (`kind: "codec"`)

| File | Content | Requirements |
|---|---|---|
| `cadu_tm.json`, `cadu_aos.json`, `cadu_uslp.json` | `attach`: the CADU of frames of 1 octet up to the maximum of the protocol (TM 2048; AOS 65536; fixed-length USLP 65535 here, 65536 in `config.json`), and of frames that hold the marker, zeros, or ones | SYNC-001…SYNC-003 |
| `cadu_errors.json` | `attach` errors: `LengthMismatch` (`frame_len`, 68) for a frame one octet short or long or empty; `BufferTooSmall` with `needed` 4 + frame length for an area of 15, 4, and 0 octets; the exact area accepted | SYNC-003 |
| `randomizer_131071.json`, `randomizer_255.json` | `randomize`: the first 40 bits of the note (five zero octets), 1 to 100 zero octets, data, the involution, all ones, and the buffers that run past a period (131071: 16383 to 32768 octets and the 65536-octet unit, four periods and 4 bits; 255: 31, 32, 255, 256, 1000, 2048 octets) | SYNC-004, SYNC-005, SYNC-020 |
| `config.json` | `config`: frame lengths accepted and rejected per protocol (`InvalidConfig`, `frame_len`), the tolerance 0 to 15 and above (`asm_errors`, 250), `CapacityExceeded`, a protocol that is none of the three (`sync_protocol`, 251), and the randomizer choices that are neither sequence (`randomize` with `randomizer_sequence`, 253) | SYNC-002, SYNC-016, SYNC-020 |

Operations. `attach`: `config` `protocol` (`tm`, `aos`, `uslp`) and `frame_len`; `input_hex` the frame; optional `params.out_len` (octets of the output area; absent: large enough); `expect.cadu_hex`. `randomize`: `config.randomizer_sequence` is the length of the sequence in bits, `131071` or `255` (any other value is the C ABI input of SYNC-020); `input_hex` the buffer; `expect.output_hex`. `config`: `config` has `protocol`, `frame_len`, `asm_errors`, and optionally `frame_capacity` (the `FRAME` capacity the instance under test has); `expect` is `{"ok": true}` or the error. An error carries `field`, the code of the field catalogue (03 §9.1) where the error names one, and `needed` for `BufferTooSmall`. The check order is not pinned: no case has two faults.

## Engine scenarios (`scenarios/`, `engine: "asm-search"`)

Format of [07 §3](../../docs/07-test-vectors.md) and `schema/engine-scenario.schema.json`; a step lists the complete, ordered outputs of its input (TV-010, TV-012) and `expect_state` (`Searching` or `Collecting`, SYNC-012) after it, an error step included (the state does not change, API-058). `at_us` rises by 1000 per step: the engine has no clock and no deadline, so no step asserts one.

| File | Content | Requirements |
|---|---|---|
| `search_offsets_tm.json` | TM, 4-octet frames: every skip of 0 to 40 junk bits before the CADU (all eight bit offsets, five octet counts) | SYNC-007, SYNC-009, SYNC-011, SYNC-017 |
| `search_offsets_aos.json`, `search_offsets_uslp.json` | AOS (9 octets) and fixed-length USLP (16 octets): skips around octet and word boundaries, up to 256 bits | same |
| `search_errors.json` | Tolerance 0 to 15: a marker with one more flipped bit than the tolerance gives no unit, one with exactly as many is found with `errors` as flipped; a flip at each of the 32 positions; two flips at tolerance 1; the exact marker at tolerance 15 | SYNC-007, SYNC-011, SYNC-016 |
| `search_first_match.json` | The first position within the tolerance wins over a better one later: ones at tolerance 13 (and not at 12); a near-miss window 1, 2, 3, and 5 bits before an exact marker, and the same stream one error below, which finds the exact marker | SYNC-007, SYNC-008, SYNC-011 |
| `search_chunks.json` | An ASM and unit split into two chunks at every octet, at bit offsets 0, 1, 3, 7; one octet per step at all eight offsets; empty chunks; a marker over three chunks; state and partial-unit queries; four units in one chunk (the bound of SYNC-013); five units at different offsets | SYNC-009…SYNC-011, SYNC-013, SYNC-014, SYNC-012, SYNC-015 |
| `search_units.json` | Units back to back and with gaps (`skipped_bits`, `skipped_bits_total`); a marker inside a unit not searched; the tail of a unit not reused after it (empty window); a partial unit at the end of a chunk; Reset of a partial unit and of the window, the counters kept; 32 bits needed for a match; frame lengths 1, 2, 5, 64 | SYNC-007, SYNC-009, SYNC-011, SYNC-012, SYNC-015 |
| `search_bounds.json` | `BufferTooSmall` with the exact `needed` (the octets of the units the chunk completes), state unchanged and the same chunk accepted after; no unit needs no area | SYNC-014, SYNC-011 |
| `search_capacity.json` | `OutOfRange` (`chunk_len`) for a chunk above the `CHUNK` capacity of the instance; `config.chunk_capacity` names the capacity the instance under test must have | SYNC-014 |

## Vocabulary

Configuration: `protocol`, `frame_len`, `asm_errors` (tolerance), and for the two capacity cases `chunk_capacity`. The engine starts in `Searching` with an empty window.

| Input | Fields | Meaning |
|---|---|---|
| `chunk` | `data_hex`; optional `out_len` (octets of the output area; absent: large enough) | A chunk of octets of the stream, read most significant bit first |
| `reset` | none | The Reset input (SYNC-015): the partial unit and the window are discarded; the counters are kept |
| `query` | `counter`: `units_returned`, `partial_octets`, `skipped_bits_total` | Answered by `{"counter": {"name", "value"}}` |

| Output | Meaning |
|---|---|
| `{"unit": {"data_hex", "errors", "skipped_bits"}}` | The unit: the (randomized) frame octets as the engine returns them, shifted to an octet boundary; the differing ASM bits; the stream bits between the end of the previous unit (or the start or Reset) and the first ASM bit (SYNC-011). The streams use the CADU of the sender: the frame randomized with the 131071-bit sequence (never the marker), or, where a scenario says so, left as it is |
| `{"error": kind, "field"?: name, "needed"?: n}` | The input is refused and nothing changes: `BufferTooSmall` (`needed`), `OutOfRange` (`chunk_len`). The field is the name of the catalogue (the codes are in the codec files) |
| `{"counter": {"name", "value"}}` | The answer to `query` |

A stream is built from whole octets; where it holds a junk prefix of n bits, the CADU follows at bit n, and the last octet is padded with zero bits, which stay in the window after the last unit. Frame lengths are at most 64 octets and chunks at most 128 octets in every scenario, so an instance needs `FRAME` of 64 and `CHUNK` of 128 for the files other than `search_capacity.json`.

## Readings the model makes

1. The window and the match are as SYNC-007 and SYNC-008 state them; the unit is returned in the input that supplies its last bit, and the bits of the same chunk after it are searched with an empty window (SYNC-009, SYNC-010).
2. `skipped_bits` is the number of bits read since the search began (the stream start, a Reset, or the end of the previous unit), less 32, at the match.
3. A match with `errors` below the tolerance is reported with the number of bits that differ, as SYNC-011 says.

## Readings the model makes (decided in OI-83, 2026-11-05)

4. `skipped_bits_total` is the sum of the `skipped_bits` of the units returned; bits that a Reset discards (an unfinished search or unit) count for nothing.
5. `partial_octets` counts complete octets of the partial unit, rounded down; 0 while `Searching`.
6. With two faults at once the first is reported: `attach` checks the frame length, then the output area; a configuration checks the protocol, the frame length, the tolerance, then the capacity.

## Readings the model refuses (OI-83 d, open, target WP-4)

* The encoding of the choices that the C ABI receives (`randomizer_sequence`, `sync_protocol`): the vectors use the sequence length in bits and the name of the protocol; the C ABI harness maps them.
* How a runner builds an instance of a given `FRAME` and `CHUNK` capacity (compile-time constants, GEN-011), for `frame_capacity` and `chunk_capacity`.
