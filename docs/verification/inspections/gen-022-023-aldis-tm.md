# Inspection: GEN-022 and GEN-023 in `aldis-tm`

| | |
|---|---|
| Requirements | GEN-022 (every loop bounded by an input length, a compile-time capacity, or a constant; no recursion), GEN-023 (time bounded by O(input length + capacities); nothing blocks or waits) |
| Scope | `crates/aldis-tm/src/*.rs`, library code only (test modules excluded), on branch `m2/tm-codec` (M2 WP-2, modules 1–6, based on `main` 46bfe0f): `config.rs`, `header.rs`, `view.rs`, `anomaly.rs`, `writer.rs`, `validate.rs`, `field.rs` (re-exports only), and `lib.rs` (module declarations and re-exports) |
| Inspected | 2026-10-23, prepared by Claude Code (agent of M2 WP-2): first inspection. Every loop, every call between the crate's own functions, and every blocking construct checked. No unbounded loop, no recursion |
| Approved | Pending exit review (VER-061) |
| Repeat | when `aldis-tm` gains a loop or a call between its own functions (module 7, the C ABI, adds none to this crate), and before each release |

## Loops

Every loop in library code, with its bound. Copies and comparisons of fixed-size values (the 6-octet header array, the 4-octet OCF, the 8-slot VC array of `TmMasterChannelConfig` in derived `Clone`/`PartialEq`/`Hash`, its initializer `[const { None }; 8]`) run over a constant length and are not listed.

| Location | Loop | Bound |
|---|---|---|
| `config.rs` `TmMasterChannelConfig::vcs` | iterator over the VC slots (`iter().flatten().map`), driven by the caller | constant: 8 |
| `config.rs` `VcLookup for [TmMasterChannelConfig]` | `iter().find` for the master channel of the frame's SCID | input length: the caller's slice |
| `view.rs` `TmFrame::check` → `aldis_core::crc::verify_fecf` | CRC over the frame | input length: the frame (inspected in `aldis-core`) |
| `writer.rs` `write_at` | `iter_mut().skip(at).zip(src)` | input length: the shorter of the frame after `at` and `src` |
| `writer.rs` `TmFrameWriter::start` | `frame.fill(0)` | input length: the frame length, at most the caller's buffer |
| `writer.rs` `TmFrameWriter::begin_oid` → `aldis_core::oid::OidGenerator::fill` | one PN octet per data field octet | input length: the data field (inspected in `aldis-core`) |
| `writer.rs` `TmFrameWriter::finish` → `aldis_core::crc::crc16` | CRC over the frame less its FECF | input length: the frame |
| `writer.rs` `encode` | the four content lengths compared with the layout; the four fields written by `write_at` | constant: 4 each |
| `validate.rs` `TmFrame::validate` | the three flags (OCF, FSH, Synchronization) and the three lengths (FSH, Security Header, Security Trailer) compared | constant: 3 each |

`FrameLayout::compute` (called by `TmMasterChannelConfig::with_vc` and `TmFrame::parse`) has no loop (inspected in `aldis-core`).

## Recursion

No function calls itself, directly or through another `aldis-tm` function. The internal call graph is acyclic and at most five functions deep: `TmFrame::decode` → `decode_with` → `parse` → `fsh_id` → `FshLen::octets` → `u8_usize`. The other chains:

- decoding: `decode_with` and `decode_unchecked` → `check` → `check_len` → `mismatch`, `len_u64`; `check` → `TmPrimaryHeader::decode` → `mismatch`; `decode_with` and `decode_unchecked` → `VcLookup::security_layout`, whose `TmMasterChannelConfig` impl → `unknown_scid` → `mismatch`, and → `vc` → `entry`; the slice impl → the `TmMasterChannelConfig` impl; `NoSdls` has no call; `parse` → `fsh_id` → `FshLen::new`, `mismatch`, `len_u64`; `parse` → `PacketPointer::classify` → `mismatch`, `len_u64`; `peek_length` → `mismatch`;
- view accessors: `secondary_header_data`, `security_header`, `data_field`, `security_trailer`, `ocf`, `fecf` → `slice`;
- validation: `validate` → `len`, `scid`, `vcid`, `header`, `fsh_len`, `security_header`, `security_trailer` (→ `slice`), `unknown_scid`, `TmMasterChannelConfig::vc` (→ `entry`), `ocf_of`, `fsh_of`, `DataFieldContent::sync_flag`, `FshLen::octets`, `mismatch`, `len_u64`;
- writing: `encode` → `Plan::new` → `entry`, `DataFieldStatus::sync_flag`, `DataFieldContent::sync_flag`, `PacketPointer::to_raw`, `fsh_of`, `ocf_of`, `mismatch`, `len_u64`; `encode` → `TmFrameWriter::start` → `TmPrimaryHeader::to_octets` → `flag`, and → `FshLen::id_octet`, `write_at`; `encode` → `write_at`, `TmFrameWriter::finish` → `write_at`; `begin` → `Plan::new`, `start`; `begin_oid` → `TmFrameFields::new`, `begin`, `data_field_mut` → `field_mut`; the other `*_mut` → `field_mut`; `set_ocf` → `mismatch`, `write_at`;
- configuration: `TmPhysicalChannelConfig::new` → `min_frame_len`, `mismatch`, `len_u64`; `with_vc` → `FshLen::octets`; `vc` and `frame_layout` → `entry`; the `TryFrom` impls of `SegmentLengthId`, `FirstHeaderPointer`, and `FshLen` → their `new`, and `FshLen`'s → `mismatch`; `From<PacketPointer> for FirstHeaderPointer` → `to_raw`.

Calls out of the crate go to `aldis_core` (`Error` constructors, `FrameLayout::compute` and its accessors, `crc::verify_fecf`, `crc::crc16`, `Ocf::decode` and `to_octets`, `OidGenerator::fill`, `AnomalySet::insert`, the identifier constructors), each inspected in that crate.

## Blocking

There is no I/O, locking, atomics, sleep, or spin-wait. `std` and `alloc` are only behind features (crate-root `extern crate` declarations) and in test modules. Decoding is O(frame length) (the FECF check); writing is O(frame length) (zero-fill, copies, the FECF, and the OID fill); configuration, lookups, and validation are O(1) or O(number of master channels in the caller's slice).
