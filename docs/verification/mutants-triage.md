# Mutation testing triage (VER-025)

Surviving mutants from `cargo mutants` (`-p aldis-core -p aldis-time -p aldis-epp -p aldis-spp -p aldis-aos -p aldis-tm -p aldis-tc -p aldis-uslp --all-features`) (cargo-mutants 27.1.0), triaged per VER-025. The nightly job `mutants` publishes the full report. Update this file when a new survivor appears.

## 2026-10-25, branch `m2/vector-review` (M2 WP-8, second review of the frame vectors)

`TmFrameWriter::begin_oid` rejects a VC that uses SDLS (`aldis-tm`), and `VcTable` answers VCID 63 without an entry on a fixed-length channel (`aldis-uslp`, `config.rs` `given_vc` and `VcLookup::vc`). The mutants of the changed lines, `cargo mutants -p aldis-tm -p aldis-uslp --all-features --in-diff <diff against main>` (with `PROPTEST_CASES=32`): 12 mutants: 9 caught, 3 unviable, 0 missed, 0 timeouts. No survivor to triage. The `aldis-ffi` change (the Galois branch of `aldis_tm_oid_writer_begin` calls `begin_oid`) is outside the crates this file lists.

After the review of #117 the check moved from `begin_oid` into `Plan::new` (`crates/aldis-tm/src/writer.rs`), so every writer applies it. `cargo mutants -p aldis-tm --all-features --in-diff <diff against cf5e035>`: 1 mutant, unviable (`begin_oid` replaced by `Ok(Default::default())`). cargo-mutants makes no mutant of a function named `new`, so the condition in `Plan::new` has none. Unit test `oid_frames_continue_the_sequence` covers it in both directions: `begin`, `resume`, and `encode` reject Only Idle Data on the SDLS VC 0, and `begin_oid` accepts VC 5 without SDLS. Vectors `tm-sec-enc-004`…`007` cover it too.

## 2026-10-25, branch `m2/wp7-tests` (M2 WP-7, review of #116)

`aldis_aos::PacketPointer::classify` rejects raw values wider than the Issue 4 pointer, `to_raw` and `header_octets` return `Option`, and `writer.rs` `mpdu_header` takes the edition check from them. The mutants of the changed lines of `mpdu.rs` and `writer.rs`, `cargo mutants -p aldis-aos --all-features --in-diff <diff against 87a48ce>` (cargo-mutants 27.1.0, with `PROPTEST_CASES=32`): 22 mutants: 21 caught, 1 unviable. None missed.

## 2026-10-24, branch `m2/rs020-check` (M2 WP-7, RS-020 check)

The check gave the M_PDU offset its own type, `PacketOffset` (`aldis-aos`), and the USLP pointer offsets theirs, `TfdzOffset` (`aldis-uslp`). The mutants of the changed lines, `cargo mutants --in-diff <diff against main>` (with `PROPTEST_CASES=32`): `-p aldis-aos --all-features`, 21 mutants: 19 caught, 2 unviable; `-p aldis-uslp --all-features`, 15 mutants: 10 caught, 5 unviable. None missed. The TM packet offset followed (`aldis_tm::PacketOffset`, with the C ABI's `Fields::checked` in `aldis-ffi`), against the previous commit: `-p aldis-tm --all-features`, 12 mutants: 9 caught, 3 unviable; `-p aldis-ffi --all-features`, 7 mutants: 5 caught, 2 unviable. None missed.

## 2026-10-23, branch `m2/uslp-codec` (M2 WP-5, modules 1–8)

`cargo mutants -p aldis-uslp --all-features` (with `PROPTEST_CASES=32`): 450 mutants: 283 caught, 155 unviable, 12 missed, 0 timeouts. Two survivors were a test gap, fixed and re-run: `header.rs` `len_u64` replaced by 0 and by 1, which only error details use; `view.rs` `length_errors_on_a_variable_channel` now checks the `OutOfRange` detail of an over-long frame (2 mutants, 2 caught). The other ten are equivalent and are expected in every report.

| Mutant | Verdict | Action |
|---|---|---|
| `header.rs` `start_octets` `\|` → `^`, five places (TFVN `<< 28`, SCID `<< 12`, Source-or-Destination `<< 11`, VCID `<< 5`, MAP ID `<< 1`, End of Frame Primary Header Flag) | Equivalent: every field is masked to its own bits of the 32-bit word, which do not overlap | None possible |
| `header.rs` `primary_octets` `\|` → `^`, three places (the two flags `<< 7` and `<< 6`, the OCF Flag `<< 3`, the VCF Count Length `& 0b111`) | Equivalent: disjoint bits of octet 6 | None possible |
| `header.rs` `read_vcf_count` `\|` → `^` (the count shifted left by 8 and the next octet) | Equivalent: the shift leaves the low eight bits zero, so OR and XOR agree | None possible |
| `tfdf.rs` `TfdfHeader::octets` `\|` → `^` (rule `<< 5` and UPID `& 0x1F`) | Equivalent: disjoint bits | None possible |

## 2026-10-23, branch `m2/aos-codec` (M2 WP-4, modules 2–7)

`cargo mutants -p aldis-aos --all-features` (cargo-mutants 27.1.0, with `PROPTEST_CASES=32`), the whole crate, the FHEC included: 405 mutants: 273 caught, 124 unviable, 8 missed, 0 timeouts.

| Mutant | Verdict | Action |
|---|---|---|
| `header.rs` `AosPrimaryHeader::parse` `\|` → `^`, two places (the 6 SCID bits of octet 0 `<< 2` and the 2 of octet 1; bits 42–43 `<< 8` and the 8-bit LSBs) | Equivalent: each joins fields shifted into disjoint bits, so OR and XOR agree | None possible |
| `header.rs` `AosPrimaryHeader::to_octets` `\|` → `^`, five places (octet 5: the two flags, bits 42–43 `<< 4`, and the cycle `& 0x0F`; octet 0: version `<< 6` and the SCID LSBs `>> 2`; octet 1: the SCID LSBs `<< 6` and the VCID `& 0x3F`) | Equivalent: every field is masked or bounded to its own bits, which do not overlap | None possible |
| `fhec.rs` `Gf::octet` `\|` → `^` | Equivalent, as triaged for module 1 below | None possible |

The eight survivors are equivalent and are expected in every report.

Re-run 2026-10-24 after the review of #97 gave the B_PDU count its own type, `ValidBitCount` (RS-020): `cargo mutants -p aldis-aos --all-features --file crates/aldis-aos/src/bpdu.rs --file crates/aldis-aos/src/writer.rs` (with `PROPTEST_CASES=32`): 93 mutants: 57 caught, 33 unviable, 3 missed. One was a test gap, fixed and re-run on `bpdu.rs` (43 mutants: 30 caught, 11 unviable, 2 missed); the other two are equivalent.

| Mutant | Verdict | Action |
|---|---|---|
| `bpdu.rs` `From<ValidBitCount> for u16` → `1` | Test gap: `u16::from` was checked only on the count 1 | `valid_bit_counts_are_1_to_0x3ffe` checks it on `0x3FFE`; caught |
| `bpdu.rs` `TryFrom<u16> for ValidBitCount` `<` → `<=` (the bound reported for a rejected count) | Equivalent: the closure runs only for a count `new` rejects, so never for 1, the one value where `<` and `<=` differ | None possible |
| `bpdu.rs` `BitstreamPointer::classify` `index < last_bit` → `<=` | Equivalent: an index equal to the last bit has already returned `InvalidField` | None possible |

## 2026-10-23, branch `m2/tm-codec` (M2 WP-2, modules 1-6)

`cargo mutants -p aldis-tm --all-features` (cargo-mutants 27.1.0), the whole crate: 256 mutants: 159 caught, 80 unviable, 17 missed, 0 timeouts. Two survivors were test gaps, fixed and re-run (`config.rs`, `TmMasterChannelConfig::fsh` and `::ocf`: 12 mutants, 10 caught, 2 unviable); the other 15 are equivalent and are expected in every report.

| Mutant | Verdict | Action |
|---|---|---|
| `config.rs` `TmMasterChannelConfig::fsh` → `None` | Test gap: `fsh()` was checked only on a master channel without an MC_FSH | `master_channel_rejects_what_132_forbids` checks it on one with an MC_FSH; caught |
| `config.rs` `TmMasterChannelConfig::ocf` → `true` | Test gap: `ocf()` was checked only on a master channel with an MC_OCF | The same test checks it on one without; caught |
| `header.rs` `TmPrimaryHeader::to_octets` `\|` → `^` (8 mutants), `TmPrimaryHeader::decode` `\|` → `^` (2), `FshLen::id_octet` `\|` → `^` (1) | Equivalent: each joins fields shifted into disjoint bits (version, SCID, VCID, OCF Flag; the four flags, the Segment Length Identifier, and the FHP bits; SCID and FHP halves; FSH version and length), so OR and XOR agree | None possible |
| `header.rs` `TmPrimaryHeader::to_octets` `<<` → `>>` on `VERSION`, `FshLen::id_octet` `<<` → `>>` on `FSH_VERSION` | Equivalent: both versions are `00`, and 0 shifted either way is 0 | None possible |
| `header.rs` `TryFrom<u8> for FshLen` `<` → `<=` (the bound reported for a rejected length) | Equivalent: the closure runs only for a length `new` rejects, so never for 2, the one value where `<` and `<=` differ | None possible |
| `config.rs` `VcLookup for NoSdls` `security_layout` → `Ok(Default::default())` | Equivalent: `SecurityLayout::default()` is `SecurityLayout::NONE` | None possible |

## 2026-10-23, branch `m2/tc-codec` (M2 WP-3, modules 1–7)

`cargo mutants -p aldis-tc --all-features` (with `PROPTEST_CASES=32`): 298 mutants: 189 caught, 98 unviable, 11 missed, 0 timeouts. The first run also missed `len_u64` replaced by 0 and by 1, which only error details use; a unit test of the `OutOfRange` detail of an over-long frame (`writer.rs` `encode_checks`) now catches both.

| Mutant | Verdict | Action |
|---|---|---|
| `header.rs` `SegmentHeader::octet` `\|` → `^` (Sequence Flags `<< 6` and MAP ID `& 0x3F`) | Equivalent: the two fields occupy disjoint bits, so OR and XOR agree | None possible |
| `header.rs` `primary_octets` `\|` → `^`, four places (the two flags `<< 5` and `<< 4`; version `<< 6`, flags, and the SCID's top two bits; VCID `<< 2` and the Frame Length's top two bits) | Equivalent: every field is masked or bounded to its own bits (VCID ≤ 63), which do not overlap | None possible |
| `clcw.rs` `Clcw::to_octets` `\|` → `^`, six places (Status `<< 2` and COP in Effect; the five flags and FARM-B Counter `<< 1`) | Equivalent: disjoint bits, each field bounded by its type | None possible |

The eleven survivors are equivalent and are expected in every report.

## 2026-10-20, branch `m2/aos-fhec` (M2 WP-4, module 1)

`cargo mutants -p aldis-aos --all-features` (the FHEC, the crate's only code): 39 mutants: 29 caught, 9 unviable, 1 missed, 0 timeouts.

| Mutant | Verdict | Action |
|---|---|---|
| `fhec.rs` `Gf::octet` `\|` → `^` (`high.0 << 4 \| low.0`) | Equivalent: a symbol is below 16 by construction, so the shifted upper symbol and the lower symbol occupy disjoint bits and OR and XOR agree | None possible |

The one survivor is equivalent and is expected in every report.

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
