# Mutation testing triage (VER-025)

Surviving mutants from `cargo mutants` (`-p aldis-core -p aldis-time -p aldis-epp -p aldis-spp --all-features`) (cargo-mutants 27.1.0), triaged per VER-025. The nightly job `mutants` publishes the full report. Update this file when a new survivor appears.

## 2026-10-21, branch `m2/wp1-ocf` (M2 WP-1, Operational Control Field)

`cargo mutants -p aldis-core --all-features` on the changed files `ocf.rs` and `field.rs`: 32 mutants: 23 caught, 9 unviable, 0 missed, 0 timeouts. `field.rs` holds only `const` items and yields no mutant. cargo-mutants generated no mutant for the three `new` constructors (none for any function named `new`, as in `ids.rs`), so every report type is classified through `Ocf::decode`, the only reader of the flag bits: the caught mutants include `bit` → `0` or `1`, its `&` → `|` or `^` and `==` → `!=`, the deletion of each explicit match arm of `decode`, and every `to_octets` and `From<…> for [u8; 4]` → `[0; 4]` or `[1; 4]`. The unviable ones replace a value of a type without `Default` (`Ocf`, the report types, `Error`). No survivor. A first run, before the constructors went through `decode`, had 34 mutants: 25 caught, 9 unviable, 0 missed.

## 2026-10-20, branch `m2/wp0a-field-codes` (M2 WP-0a, field codes)

`cargo mutants -p aldis-core --all-features` on the changed files `error.rs`, `field.rs`, `bits.rs`, `container.rs`, `counter.rs`, `ids.rs`, `layout.rs`, and `security.rs`: 339 mutants: 200 caught, 136 unviable, 3 missed, 0 timeouts. The 3 missed are the equivalent disjoint-bit `|` → `^` mutants of `bits.rs` `get` and `set` already listed below; no new survivor. The new `FieldId` code is covered: `PartialEq::eq` → `true`, → `false`, and `==` → `!=`, `Hash::hash` → `()`, and `FieldId::name` → `""` or `"xyzzy"` are all caught; `FieldId::catalogued` → `Default::default()` and `FieldId::code` → `1` are unviable. `field.rs` holds only `const` items and yields no mutant. The code mapping inside `catalogued` is an inline `const` block, which cargo-mutants does not mutate; `error::tests::field_codes_are_the_catalogue` checks every constant's code against its position in `field::ALL`.

## 2026-10-20, Nightly run 39442459267 on `main` 5f40e8b (M1 release triage)

The full report of the `mutants` job ([run 39442459267](https://github.com/kscharpf/aldis/actions/runs/39442459267), the M1 pre-release run) over all four crates: 1553 mutants: 1096 caught, 424 unviable, 33 missed, 0 timeouts. 28 of the missed are the equivalent mutants already listed below: the disjoint-bit `|` → `^` mutants (`aldis-core` `bits.rs` and `oid.rs`; `aldis-epp` `EncapsulationHeader::octets`; `aldis-spp` `SpacePacketHeader::octets_with_version`; `aldis-time` `ccs.rs`, `cds.rs`, `code.rs`, `cuc.rs`) and `aldis-time` `util.rs` `out_of_bounds` `<` → `<=`. The other 5 are new, all in `aldis-core`, whose own tests checked these items only with values the mutant also returns:

| Mutant | Verdict | Action |
|---|---|---|
| `container.rs` `FixedVec::eq` → `true` | Test gap: `fixed_vec` compared only equal vectors | `fixed_vec` checks that vectors with different contents are unequal; caught |
| `ids.rs` `SeqCount::get` → `1` | Test gap: `get` was checked only on the count 1 | `packet_identifiers` checks `get` on `SeqCount::MAX`; caught |
| `ids.rs` `From<u8> for FrameSeqNum` → `Default::default()` | Test gap: the conversion was checked only from 0 | `packet_identifiers` converts 200; caught |
| `ids.rs` `From<FrameSeqNum> for u8` → `0` | Test gap: the conversion was checked only on 0 | `packet_identifiers` converts `FrameSeqNum::MAX`; caught |
| `ids.rs` `PacketVersion::get` → `0` | Test gap: `get` was checked only on `SPACE_PACKET` (0) | `packet_identifiers` checks `ENCAPSULATION.get()` is 7; caught |

After the fixes, a re-run on `ids.rs` and `container.rs` (106 mutants: 91 caught, 15 unviable) found no survivors. With them, every surviving mutant of the four crates is equivalent.

## 2026-10-19, branch `m1/spp-part2` (M1 WP-2 part 2)

`cargo mutants -p aldis-spp --all-features` on `segment.rs`, `secondary.rs`, and `view.rs`: 81 mutants: 46 caught, 34 unviable, 1 missed.

| Mutant | Verdict | Action |
|---|---|---|
| `secondary.rs` `TimeHeaderLayout::has_pfield` → `true` | Test gap: `has_pfield` was checked only on a layout with a P-field | `layouts_hold_only_cuc_or_cds` checks a layout without one; caught |

After the fix, a re-run on `secondary.rs` (24 mutants: 10 caught, 14 unviable) found no survivors.

## 2026-10-19, branch `m1/spp` (M1 WP-2 part 1)

`cargo mutants -p aldis-spp --all-features`: 157 mutants: 99 caught, 52 unviable, 6 missed, 0 timeouts.

| Mutant | Verdict | Action |
|---|---|---|
| `sequence.rs` `SequenceTracker::is_empty` → `true` | Test gap: `is_empty` was checked only on an empty tracker | `tracker_paths` checks it after tracking an APID; caught |
| `writer.rs` `SpacePacketWriter::begin` `!=` → `==` (the unfinished header's Secondary Header Flag) | Test gap: the unfinished-state test checked only the version and length octets | `unfinished_writer_leaves_an_invalid_packet` checks all 6 octets; caught |
| `header.rs` `SpacePacketHeader::octets_with_version` `\|` → `^` (4): the Packet Identification and Packet Sequence Control words | Equivalent: version (bits 13–15), type (12), flag (11), and APID (0–10) are disjoint, as are the flags (14–15) and the count (0–13), so OR and XOR agree | None possible |

After the fixes, a re-run on `writer.rs` and `sequence.rs` (35 mutants: 25 caught, 10 unviable) found no survivors. The four equivalent mutants remain and are expected in every report.

## 2026-10-19, branch `m1/time-ascii` (M1 WP-1 part 2)

`cargo mutants -p aldis-time --all-features` on `ascii.rs`, `convert.rs`, `calendar.rs`, `util.rs`: 313 mutants: 270 caught, 42 unviable, 1 missed. The one missed is the equivalent `util.rs` `out_of_bounds` `<` → `<=` already listed below; no new survivor.

## 2026-10-19, branch `fix/rs-020` (issue #22)

`cargo mutants --all-features` on the changed files: `aldis-core` `security.rs`, `layout.rs`, `anomaly.rs` (180 mutants: 58 caught, 122 unviable, 0 missed); `aldis-epp` `config.rs`, `header.rs`, `writer.rs` (187: 150 caught, 34 unviable, 3 missed); `aldis-time` `pfield.rs`, `cuc.rs`, `code.rs` (215: 150 caught, 58 unviable, 7 missed). All 10 missed are the disjoint-bit `|` → `^` equivalents already listed for these files (`EncapsulationHeader::octets`; `CucFormat::pfield`, `AgencyFormat::pfield`, `Cuc::from_t`); no new survivor.

## 2026-10-19, branch `m1/time-types` (parse, don't validate)

`cargo mutants -p aldis-time --all-features` on `calendar.rs`, `ccs.rs`, `cds.rs`, `util.rs`: 152 mutants: 118 caught, 26 unviable, 8 missed.

| Mutant | Verdict | Action |
|---|---|---|
| `util.rs` `out_of_bounds` `<` → `<=` (`if value < min`) | Equivalent: the helper is called only for out-of-range values, so `value == min` never occurs and both comparisons choose the same bound | None possible |
| `\|` → `^` (7) in `ccs.rs` and `cds.rs` | The equivalent disjoint-bit mutants already listed below | None possible |

## 2026-10-18, branch `m1/time` (M1 WP-1 part 1)

`cargo mutants -p aldis-time --all-features`: 365 mutants: 275 caught, 74 unviable, 16 missed, 0 timeouts.

| Mutant | Verdict | Action |
|---|---|---|
| `ccs.rs` `CcsFormat::with_tail`: delete field `tail` | Test gap: the round-trip test compared two formats that both lost the tail | `formats` checks `with_tail(t).tail() == t`; caught |
| `cuc.rs` `CucFormat::with_tail` `>` → `>=` | Test gap: no test accepted the largest tail, 6 octets | `format_limits_and_accessors` accepts a 6-octet tail; caught |
| `\|` → `^` (14): P-field octet 1 of CUC, CDS, CCS, and agency codes (`*::pfield`); CUC octet 2; `ccs.rs` `to_bcd`; the big-endian folds in `Cuc::from_t` and `Cds::from_t` | Equivalent: each combines fields in disjoint bits (for example CUC octet 2 puts its three fields in bits 5–6, 2–4, and 0–1), so OR and XOR agree | None possible |

After the fixes, a re-run on `ccs.rs` and `cuc.rs` (187 mutants: 158 caught, 20 unviable) left only the 9 equivalent mutants in those files. The 14 equivalent mutants are expected in every report.

## 2026-10-18, branch `m1/epp` (M1 WP-3)

`cargo mutants -p aldis-epp --all-features`: 199 mutants: 156 caught, 37 unviable, 6 missed, 0 timeouts.

| Mutant | Verdict | Action |
|---|---|---|
| `config.rs` `EpiSet::with`, `ExtendedEpiSet::with` `\|` → `^` | Test gap: no test added a member that was already present, which `^` would remove | `sets_hold_members` adds a member twice; caught |
| `config.rs` `EppConfig::min_data_len` → `1` | Test gap: the test configuration used a minimum of 1 | `config_is_const_constructible_and_validated` uses a minimum of 3; caught |
| `header.rs` `EncapsulationHeader::octets` `\|` → `^` (three: octet 0 version/EPI/Length of Length, octet 1 User Defined/Extension) | Equivalent: the combined fields occupy disjoint bits, so OR and XOR agree | None possible |

After the fixes, a re-run on `config.rs` found no survivors (45 mutants: 40 caught, 5 unviable). The three equivalent mutants remain and are expected in every report.

## 2026-10-17, branch `m1/core` (M1 WP-0)

`cargo mutants -p aldis-core --all-features --file crates/aldis-core/src/anomaly.rs --file crates/aldis-core/src/mode.rs`

First run (`3b51e87`): 41 mutants: 19 caught, 20 unviable, 0 missed, 2 timeouts. Both timeouts were in `Iter::next` (`while self.bits != 0` → `==`, and `&=` → `|=` when clearing the lowest bit). Each one turned the loop into an infinite one, so the tests hung instead of failing.

[Review round 1](reviews/m1-core.md) finding 7 asked for the loop to be bounded by its structure. `Iter` now keeps a position that only rises over `0..CAPACITY`, so no mutant can make an iteration endless. Re-run after the fix: 53 mutants: 32 caught, 21 unviable, 0 missed, 0 timeouts.

## 2026-10-16, branch `m0/gates`

288 mutants: 198 caught, 81 unviable (do not compile), 9 missed, 0 timeouts.

| Mutant | Verdict | Action |
|---|---|---|
| `container.rs:56` `FixedQueue::is_full` → `true` | Test gap: `is_full` was only asserted on a zero-capacity queue | `fixed_queue` asserts `is_full` before and after filling; caught |
| `container.rs:320` `FixedVec::is_empty` → `false` | Test gap: `is_empty` was only asserted on a non-empty vector | `fixed_vec` asserts `is_empty` when new and after `clear`; caught |
| `security.rs:72` `&&` → `\|\|` in `SecurityLayout::is_none` | Test gap: no case had exactly one of header and trailer empty | `layout_limits` checks header-only and trailer-only layouts; caught |
| `bits.rs:69`, `bits.rs:90` `\|` → `^` in `get`/`set` (`(acc << 8) \| octet`) | Equivalent: the shifted accumulator's low 8 bits are zero, so OR and XOR agree | None possible |
| `bits.rs:92` `\|` → `^` in `set` (`(acc & !field_mask) \| (value << shift)`) | Equivalent: the field bits are cleared first, so OR and XOR agree | None possible |
| `oid.rs:29` `\|` → `^` in `next_bit` (`(r << 1) \| new`) | Equivalent: bit 0 of `r << 1` is zero | None possible |
| `oid.rs:35`, `oid.rs:85` `\|` → `^` in `next_octet` (`(octet << 1) \| bit`) | Equivalent: bit 0 of `octet << 1` is zero | None possible |

After the fixes, a re-run on `container.rs` and `security.rs` found no survivors (89 mutants: 71 caught, 18 unviable). The six equivalent mutants remain and are expected in every report.

These test gaps survived 100 % line, region, and branch coverage: the code was executed, but the result was never checked in the distinguishing state.
