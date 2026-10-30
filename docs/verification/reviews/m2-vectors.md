# Review: hand-derived M2 frame vectors (TM, TC, AOS, USLP)

> Dated review: identifiers reflect the code at the time.

The second review the [M2 plan](../../milestones/m2-frame-codecs.md) WP-6 asks for (model: [m1-vectors.md](m1-vectors.md)): the hand-derived frame vectors that no independent implementation checks ([cross-checks.md](../cross-checks.md) §4) are read by reviewers other than the sessions that derived them, so that a misreading of the Blue Book shared by the vectors and the code can surface.

## Round 1

- **Date:** 2026-10-25
- **Reviewers:** four Claude Sonnet agents, one for each standard (the TC review names the model Claude Sonnet 5.5), working blind: none opened `crates/`, `cpp/`, `test-vectors/models/`, `xtask/`, `exerciser/`, or `fuzz/`; the TM, TC, and USLP reviewers also left the worked derivations under `docs/verification/derivations/` unread. The vectors and their models were written in Claude Code sessions by Claude Opus 5.5 agents (the `Co-Authored-By` trailers of 222232e, 5e1235e, afff003, 5bbbf45, and 0459ef0; the provenance notes now say so). The maintainer commissioned the review and took the decisions below.
- **Baseline:** `main` at `6c4966d`, as the TC and USLP reports state; the TM and AOS reports name none. The reviewers read the Blue Books from `specs/pdf/` (132.0-B-3, 232.0-B-4 Cor. 1, 732.0-B-5 EC1 and 732.0-B-4, 732.1-B-3, with 131.0-B-6 and 355.0-B-2 for single clauses) and the documents named in each section.
- **Method:** each reviewer wrote its own decoder, encoder, validator, and configuration checker from the Blue Book and the requirement text, in a scratch directory that shares nothing with the repository (those scripts were not kept, as in M1), recomputed every case, and derived cases of every file by hand.
- **Result:** **approved for all four standards; no protocol error and no arithmetic error found.** 27 numbered findings (TM 7, TC 7, AOS 6, USLP 7) and 7 TC coverage gaps. Four needed a maintainer decision (the TM OID writer on SDLS, USLP VCID 63, and two readings to keep); the rest are presentation issues, undocumented limits, project readings stated as Blue Book facts, and coverage gaps. The resolutions are in the tables below. The released-vector rule (TV-021) does not apply: only `v0.1.0-m0` and `v0.2.0-m1` are released, and no file of `tm/`, `tc/`, `aos/`, `uslp/`, `core/ocf.json` is in either, so presentation fixes were made in place by regenerating the files from their models (`--write`), and the two cases whose result changed were marked `superseded`.
- **Fix branch:** `m2/vector-review` (M2 WP-8): code, inspections, and the header `003b7bc`; vectors and models `d8adf22`; documentation `e9d11d5`; this record, the plan link, and the changelog fragment in the next commit.

## Maintainer decisions (2026-10-25)

| # | Decision | Where it is recorded | Effect |
|---|---|---|---|
| 1 | USLP VCID 63 follows 732.1-B-3 Table 5-2 note 2 ("the binary value of 'all ones' (i.e., 63) is always valid"): on a fixed-length Physical Channel, OID frames on VCID 63 are always accepted, with no configuration entry; on a variable-length one they stay rejected (Table 5-1 note 4) | `docs/standards/uslp-frame.md` §2.1 | `VcTable`'s `VcLookup::vc` answers VCID 63 of a known Spacecraft ID on a fixed-length channel with a fixed-length VC of `VcParams::new` defaults (no VCF Count, OCF, or SDLS fields); an entry still overrides them. `uslp-oid-019` superseded. Variable-length cases unchanged |
| 2 | The TM OID writer rejects a VC that uses SDLS (132.0-B-3 §6.4.6.2) | TM-010 in `docs/standards/tm-frame.md` | `TmFrameWriter::begin_oid` and `aldis_tm_oid_writer_begin` (both generator forms) return `InvalidField` naming `vc_id` after the `sync_flag` check, writing nothing. `tm-oid-002` superseded |
| 3 | A stateless decoder cannot check the OID pseudo-noise data (it depends on the generator's position across frames), and receivers discard it; documented, no behaviour change | CMN-021 and CMN-020 sources in `docs/standards/common-link-features.md`; TM-010; `aos-frame.md` (after AOS-019); `uslp-frame.md` §2.1; API-031 | none |
| 4 | Keep two readings: a CLCW with COP in Effect ≠ `01` is decoded in strict mode (TC-030), now a listed exception in API-031; the USLP VCF Count Length is per QoS (Table 5-3) despite §4.1.2.11.4, recorded as OI-46 and resolved with this reading | `docs/03-api-model.md` API-031; `docs/09-open-issues.md` OI-46 | none |

## TM (132.0-B-3), 99 cases

Scope: `test-vectors/tm/` `fhp_values` (18), `fsh_ocf_fecf` (10), `invalid` (46), `minimal` (10), `oid_sequence` (4, one of them now superseded), `security_layout` (11, one of them already superseded).

The reviewer's script (FECF by long division and by the Figure 4-5 register, giving `29B1` for `"123456789"`; the Annex D OID recurrence, reproducing the 20 printed octets; its own decoder, encoder, validator, and OID writer) reproduced 86 cases exactly, with 2 differences that were the script's (`tm-fhp-enc-err-002`: FHP `0x7FE` collides with the only-idle-data code of §4.1.2.7.6.5; `tm-inv-enc-010`: the script ignored `out_len`) and `tm-sec-val-001`, which is superseded. The 13 `config` cases were read against the Blue Book.

**Confirmed:** the Primary Header layout and all FHP codes (§4.1.2.1–§4.1.2.7); Sync = 0 and Sync = 1 handling of POF, SLID, and FHP (§4.1.2.7.4–6, TM-007, TM-008); the Secondary Header ID octet and lengths 2–64 (§4.1.3); the OCF types (§4.1.5.4–5); the FECF (§4.1.6.2.2, `29B1`); the data field length (§4.1.4.2, §6.3.5.2) in all 99 cases; the field order with SDLS (§6.3.4–§6.3.8); the OID frame (§4.1.4.6, Annex D, frame 2 starting at the 21st LFSR octet); managed parameters (§5, Tables 5-1…5-3); the Security Header limit of 64 octets (355.0-B-2 §4.1.1.1.4); every error kind and field code against `docs/03-api-model.md` §9, §9.1.

| # | Class | Finding | Resolution |
|---|---|---|---|
| 1 | spec ambiguity | `tm-oid-002` encodes OID frames on a VC with a Security Header and Trailer; 132.0-B-3 §6.4.6.2 requires the Master Channel Multiplexing Function to use a VC without SDLS, and TM-010 did not say | **Decision 2.** TM-010 states it; `crates/aldis-tm/src/writer.rs` `begin_oid` rejects (`InvalidField`, `vc_id`); `aldis-ffi` `tm.rs` Galois branch goes through `begin_oid`; tests `oid_frames_continue_the_sequence` (unit) and `the_oid_writer_takes_a_generator_handle` (C ABI); `tm-oid-002` superseded in `test-vectors/models/tm_frame.py` with its released expectation kept, `by` `tm-oid-005` |
| 2 | spec ambiguity / presentation | `tm-fhp-enc-005` and `tm-fhp-enc-err-002` use a 2 100-octet frame, above the 2 048-octet maximum of 131.0-B-6 §11; 132.0-B-3 states none | Both descriptions say it is on purpose and why; `tm-frame.md` §6 |
| 3 | spec ambiguity / presentation | "no data octet" is a project rule (CMN-041, source "—"), and `tm-inv-cfg-001` names field 68 where four other cases name 7 | The codes stay (68 for a physical channel too short for its own header and FECF with no VC; 7 when a configured field leaves none); `tm-inv-cfg-001`'s description says so; CMN-041 now says "a project rule"; `tm-frame.md` §6 |
| 4 | spec ambiguity | FECF before version (`tm-inv-crc-002`) is a project order | Description and `tm-frame.md` §6 say so |
| 5 | gap, presentation | no case combines a Security Trailer with an OCF; `minimal` and `fsh_ocf_fecf` hold no error case; the `provenance_note` is the same in all files and does not say who wrote the model (TV-023) | The case is in the follow-up list; error cases of those categories live in `invalid.json` (`tm-frame.md` §5 lists the categories); each note now ends with "This file covers …" and names the model's author (see "Provenance notes" below) |
| 6 | spec ambiguity (low) | error kinds and fields for POF with Sync = 0 (a NOTE in the Blue Book) and for a VC whose content does not fit (`tm-oid-003`, `tm-inv-enc-002`, `-003`) are project choices | `tm-oid-003`, `tm-inv-enc-002`, `-003` descriptions say so; `tm-frame.md` §6 |
| 7 | presentation in `docs/` | API-032 listed the TM POF "when Sync Flag = 1" among the lenient anomalies | Now "when Sync Flag = 0" (`docs/03-api-model.md` API-032), as TM-007 and `tm-inv-pof-002` have it |

## TC (232.0-B-4 Cor. 1), 154 cases

Scope: `test-vectors/tc/` `ad`, `bd`, `bc_unlock`, `bc_setvr`, `segment_header`, `max_length`, `clcw`, `delimit_multiple_frames_with_fill`, `validation_failures`, `decode_errors`, `config`: 11 files, no case superseded.

The reviewer's program (40-bit header integer, bitwise CRC, `29B1` asserted, CLCW, delimiter, validator) agreed with 141 of 141 non-config cases; the 13 `config` cases were checked by hand, and every FECF in an error case was checked for its role.

**Confirmed:** Primary Header bits and Frame Length (§4.1.2); Table 4-1 and the reserved combination (field 77); Reserved Spare (TC-003); Segment Header octets (Table 4-2) and its absence from Type-C; Control Commands (§4.1.3.3); FECF; SDLS field placement (§6.3.4.2, §6.3.6.2, Tables 6-2, 6-3); delimiting (§4.4.9.2) and the OI-41 item 2 rule; one validation case per check (a)…(e) with the reason codes of `tc-frame.md` §4.1; every CLCW field position and width and its field codes 82–88; the arithmetic of all 13 configurations.

| # | Class | Finding | Resolution |
|---|---|---|---|
| 1 | presentation | `tc-val-018` said "a valid VCID" for VCID 4, which `tc-val-008` shows is not configured | Description rewritten: SCID 0x3FF unexpected and VCID 4 not configured, check (b) first |
| 2 | presentation | `tc-val-019`'s description could not be read | Now "the BD frame of tc-val-002 with the first bit of its FECF flipped" |
| 3 | traceability | `tc-bd-enc-err-001`, `tc-bc-unlock-enc-err-001`, `-002` cited TC-007 and API-001, which do not cover them | New requirement **TC-009** (the writer builds a Type-C frame from a Control Command only and a Type-D frame from a Frame Data Unit only; §4.1.2.3.2.2, Table 4-1); the three cases cite it |
| 4 | spec ambiguity (tracked) | `tc-delimit-003` read as the Blue Book's own result; the `Fill` item is OI-41 item 2 | `tc-delimit-003` and `-007` descriptions cite OI-41 item 2 |
| 5 | spec ambiguity | TC-007 gave two limits and no field for the data-field one; `tc-max-enc-err-001`, `-002` pin `frame_len` | TC-007 now says the data-field limit is the 1024 limit expressed for the data field: `OutOfRange` naming `frame_len` for the one-shot writer, `user_len` for `finish` of the two-phase writer (`crates/aldis-tc/src/writer.rs`) |
| 6 | spec ambiguity | `peek_length` applies the version and flag checks of TC-002 (`tc-ad-peek-003`, `-004`), which no requirement said | TC-002 says it; pinning `needed` in `tc-ad-peek-002` is in the follow-up list |
| 7 | spec ambiguity (by design) | a CLCW with COP in Effect ≠ `01` is decoded in strict mode against §4.2.1.5.2 and API-031 | **Decision 4.** API-031 lists it as an exception; TC-030 says so |

Observation without a class: within `ad`, `bd`, `bc_*`, `segment_header`, and `max_length` each encode case is followed by its decode case, so the IDs are not in sort order; the vector validator does not require order, no change.

## AOS (732.0-B-5 EC1, 732.0-B-4), 190 cases

Scope: the 16 files of `test-vectors/aos/`: `minimal` (20), `config` (11), `invalid` (19), `insert_zone` (7), `security_layout` (7), `mpdu_pointers` (12), `bpdu_pointers` (20), `oid` (7), `issue4_compat` (18), `scid_10bit` (8), `fhec_encode` (14), `fhec_correct_1_symbol` (14), `fhec_correct_2_symbols` (8), `fhec_miscorrection` (1), `fhec_uncorrectable` (9), `fhec_frame` (15).

The reviewer's scripts (GF(16) with F = x⁴ + x + 1, g(x) built from α⁶…α⁹, encode by division, decode by exhaustive search of all 1- and 2-symbol patterns; an independent frame decoder for both editions; the Annex D recurrence) matched all 14 FHEC values, all 22 correction cases, the miscorrection and 9 uncorrectable cases, 69 decode cases, and 20 encode cases byte for byte, and recomputed every FECF.

**Confirmed:** the FHEC code and symbol mapping (§4.1.2.6.5); the SCID split across bits 2–9 and 42–43 and the Issue 4 and Issue 5 headers; the FECF (`29B1`) over the corrected header (§4.3.7.2–3, `aos-fhecf-dec-006`); M_PDU pointers in both editions and the B_PDU pointer (§4.1.4.2.2, §4.1.4.3.3); SDLS layout (§6.3); the OID frame and the continuing sequence (Annex D); every error kind and field code (67, 68, 2, 1, 7, 8, 9, 97, 100…106, 74, 93).

| # | Class | Finding | Resolution |
|---|---|---|---|
| 1 | spec ambiguity | `aos-bpdu-dec-005-s/-l` reject the pointer that names the zone's last bit in both modes; the Blue Book does not say a receiver must | AOS-017 states it as the project's reading; the description says so |
| 2 | spec ambiguity | `aos-bpdu-cfg-002` and AOS-018's 2 048-octet frame limit come from a NOTE, not a SHALL | AOS-018 and the description say it is a project choice |
| 3 | spec ambiguity | check precedence is pinned by vectors, not given by the Blue Book (pointer over spare in `aos-i4-dec-006`; version against FECF in `aos-inv-dec-004`…`006`, which avoid it); OID data not checked (`aos-iz-dec-003`, `aos-sec-dec-002`) | New paragraph "Order of the decoder's checks" in `aos-frame.md`; **decision 3** paragraph "OID data" there, in API-031, and in the two descriptions. The version-against-FECF case is in the follow-up list |
| 4 | presentation | `aos-i4-hdr-001` holds under strict mode only because `decode_header` takes no mode | Description says so (AOS-010) |
| 5 | presentation | `aos-cfg-002`, `-003`, `aos-iz-cfg-001` name field 68 though 97 exists; the arithmetic of `aos-iz-cfg-001` was loose | The codes stay: 68 names the Physical Channel parameter to change in a configuration, 97 is the error of a writer given an Insert Zone of the wrong length (`aos-iz-enc-003`); the three descriptions say so, and `aos-iz-cfg-001` now reads "the 6-octet header and the 9-octet Insert Zone fill all 15 octets, and one data octet needs 16" |
| 6 | presentation | `oid.json` cites CMN-020/021, whose source column named only 132.0 Annex D | Sources of CMN-020, 021, 022 now include 732.0 §4.1.4.1.5.2 and Annex D and 732.1 §4.1.4.1.10 and Annex H |

## USLP (732.1-B-3), 408 cases

Scope: the 13 files of `test-vectors/uslp/`: `fixed_packets_rule000`, `fixed_mapa_rules001_010`, `variable_rules011_111`, `vcf_count_lengths_0_to_7`, `truncated`, `truncated_sdls_layout`, `oid`, `insert_zone`, `protocol_control_cop1`, `delimit_multiple_frames_with_fill`, `validation_failures`, `invalid`, `config` (one case now superseded).

The reviewer's decoder, validator, writers, delimiter, `peek_length`, and configuration checker reproduced all 408 cases (186 decode and validate, 127 encode, 14 `peek_length`, 16 `delimit`, and the 65 `config` cases, whose order-of-checks cases were checked by hand); every FECF by two methods (124 frames; `29B1`); the OID stream by a Fibonacci and a Galois LFSR (equal over 5 000 octets); 252 facts in the descriptions; the UPIDs against `specs/sana/uslp_protocol_id.tsv`.

**Confirmed:** Primary Header and Table 4-1; VCF Count lengths 0–7 and their maxima (Table 4-2); Insert Zone, TFDF Header, Table 4-3; OID frames; OCF and FECF position; SDLS (§6.3); truncated frames (Annex D); validation checks (a)…(e) (§4.3.11.3.4) and delimiting (§4.3.11.2); COP-1 commands (232.0-B-4 §4.1.3.3); Table 5-3 limits.

| # | Class | Finding | Resolution |
|---|---|---|---|
| 1 | spec ambiguity (citation) | `uslp-frame.md` §2.1 cited Table 5-2 note 2 for "VCID 63 valid only when given", which note 2 says the opposite of | **Decision 1.** §2.1 rewritten; `VcTable` implements it (`crates/aldis-uslp/src/config.rs`; unit test `vcid_63_is_valid_without_an_entry_on_a_fixed_channel`); `uslp-oid-019` superseded. `uslp-val-013`, `uslp-cfg-029`, and `uslp-oid-041` give the same result as before (all three are variable-length channels: reason 5 and `InvalidConfig` from Table 5-1 note 4); their descriptions now cite that note |
| 2 | spec ambiguity | §4.1.2.11.4 "invariant for a VC" against the two per-QoS parameters of Table 5-3 | **Decision 4.** OI-46, resolved with the per-QoS reading; a paragraph in `uslp-frame.md` §2.1 |
| 3 | undocumented limit | a Physical Channel frame length below 9 is rejected even with no VC; a Security Trailer of 65 536 is rejected while API-071 limited only the Header | `uslp-frame.md` §2.1 (Physical Channel and VC rows) and API-071 state both limits; `uslp-cfg-004`, `-007`, `-034`, `-016`, `-025` descriptions say so |
| 4 | coverage gap | no case orders the "fewer than 7 octets" check against version and SCID; a protocol control frame on a fixed-length VC (USLP-016); USLP-011 | `uslp-frame.md` §3.1 says the length comes first; the cases are in the follow-up list; USLP-011 is a sending rule of the multiplexing engine, no codec case is possible |
| 5 | presentation | the 2 700-character `provenance_note` is the same in all 13 files | Each note ends with "This file covers …"; see "Provenance notes" |
| 6 | presentation | the order of the configuration checks is pinned by `uslp-cfg-048`…`055` and written nowhere | `uslp-frame.md` §2.1 "Order of the configuration checks (a project choice)"; the eight descriptions say so |
| 7 | presentation (Blue Book) | 732.1-B-3 §6.3.2.2 refers to D1.2.5 for the truncated header with SDLS; the header is D1.3 (D1.2.5 is the no-mixing rule) | Noted in `uslp-frame.md` §2.1 and in the provenance note of every USLP file |

The decisions that OI-33, OI-35, OI-41, and OI-44 recorded were applied as written and are not findings.

## Provenance notes

TV-023 asks a note to say who wrote the independent model. The git history of `test-vectors/models/` shows the author as the maintainer's account with a `Co-Authored-By: Claude Opus 5.5` trailer on every commit that added a model, and no session identifier, so the notes say that and nothing more:

| Model | Commit | What the notes now say about independence |
|---|---|---|
| `aos_fhec.py`, `core_ocf.py` | 222232e (2026-10-21; the vectors: 910c9db, 7f8ecda) | written before the Rust codec (the existing text); the history records no briefing and does not show whether any implementation was read |
| `tm_frame.py` | 5e1235e (2026-10-23) | written from 132.0-B-3 and `tm-frame.md`, after the Rust codec existed, "not from its code" (the existing text); no briefing recorded |
| `tc_frame.py` | afff003 (2026-10-23) | the commit message states it was written from 232.0-B-4 Cor. 1 before the Rust codec |
| `aos_frame.py` | 5bbbf45 (2026-10-23) | as `tm_frame.py` |
| `uslp_frame.py` | 0459ef0 (2026-10-23) | the commit message states it was written from 732.1-B-3 and `uslp-frame.md` without reading the Rust code |

Every note was regenerated with `--write`, so each file's note is the model's, and `--check` and `cargo xtask vectors validate` still pass.

## Cases changed

- **Superseded (reason in each case; `by` names the replacement of the follow-up cases):** `tm-oid-002` (decision 2, by `tm-oid-005`) and `uslp-oid-019` (decision 1, by `uslp-oid-042`). Their operation and released expectation are kept; the vector runners skip superseded cases. `tm-sec-val-001` was already superseded by `tm-sec-val-002`.
- **Not superseded although listed for review:** `uslp-val-013`, `uslp-cfg-029`, `uslp-oid-041` give the same result as before: all three are variable-length channels, where VCID 63 stays rejected (decision 1 changes the fixed-length channel only). The fixed-length cases with a configured VCID 63 (`uslp/oid.json`, `uslp-iz-*`) also pass unchanged.
- **Edited in place (unreleased):** descriptions of `tc-val-018`, `tc-val-019`, `aos-i4-hdr-001`, `aos-iz-cfg-001`, `aos-cfg-002`, `aos-cfg-003`, `tm-inv-cfg-001`, `tm-fhp-enc-005`, `tm-fhp-enc-err-002`, and the others named above; requirement citations of `tc-bd-enc-err-001`, `tc-bc-unlock-enc-err-001`, `-002`; every `provenance_note`. No field code, input, or expected value changed except through the two superseded cases.

## Follow-up cases

Written on 2026-10-25 by a second, blind Claude Sonnet 5.5 agent (TV-023: it read the Blue Books, `docs/`, and the vectors, never the implementation) by extending the four models, and generated with their `--write`. The Rust and C ABI vector runners pass all of them; no discrepancy between model and implementation was found. Each item with the cases that cover it:

TM (`test-vectors/models/tm_frame.py`):
1. Replacement of `tm-oid-002` (decision 2): `tm-oid-005` (three OID frames with an FSH on VC 2, no SDLS; `tm-oid-002` now has `by: tm-oid-005`), `tm-oid-006` (VC 0 uses SDLS: `InvalidField` `vc_id`), `tm-oid-007` (a VC that is both `vca_sdu` and SDLS: `sync_flag` first), `tm-oid-008` and `tm-oid-009` (a Security Trailer alone, a Security Header alone: still SDLS, `vc_id`). A rejected request writes nothing: a case holds the whole `frames` list, so "nothing written" is the absence of `frames_hex` in the error expectation.
2. FSH, Security Header, data, Security Trailer, OCF, and FECF together: `tm-sec-dec-006` (decode), `tm-sec-enc-003` (encode), `tm-sec-val-003` (validate).
2a. Finding 1 of the review of #117 (every TM writer applies the SDLS check to FHP `0x7FE`, TM-010, 132.0-B-3 §6.4.6.2), added 2026-10-25 by a blind Claude Sonnet 5.5 agent: `tm-sec-enc-004` (Security Header and Trailer), `tm-sec-enc-005` (Header only), `tm-sec-enc-006` (Trailer only): `InvalidField` `vc_id`; `tm-sec-enc-007` (no SDLS: accepted). The vector format has no two-phase writer operation for TM (operations: `encode`, `encode_oid`, `decode`, ...), so `begin`/`resume` have no vector case.

TC (`test-vectors/models/tc_frame.py`):
3. Unknown MAP ID with a wrong FECF, reason 9: `tc-val-021`; the decoder's `CrcMismatch`: `tc-dec-err-014`.
4. Order of `FrameTooLong`/`FrameTooShort` against `LengthMismatch`: `tc-val-022` and `-023` (header above the VC maximum, fewer and more octets present: reason 6), `tc-val-024` and `-025` (header below the minimum, more and fewer octets present: reason 7).
5. Type-C frame on VC 3: `tc-bc-unlock-enc-007`/`-dec-007` and `tc-bc-setvr-enc-012`/`-dec-012` (8 octets, no Security fields).
6. Field extremes: `tc-ad-enc-006`/`-dec-006` (SCID 0x3FF, VCID 63), `tc-bd-enc-004`/`-dec-004`, `tc-seg-enc-007`/`-dec-007` (SCID 0x200, VCID 32, MAP 63, Sequence Flags 11), `tc-seg-enc-008`/`-dec-008` (Sequence Flags 10, MAP 0); out of range on encode, `OutOfRange` before any lookup (API-001): `tc-ad-enc-err-001` (SCID 1024), `tc-ad-enc-err-002` (VCID 64), `tc-seg-enc-err-005` (MAP ID 64). The two extreme configurations each hold one Spacecraft ID, because the C ABI runner builds one master channel per case.
7. Decode against the Physical Channel maximum: `tc-max-dec-004` (100 octets accepted), `tc-max-dec-err-002` (101 octets: `OutOfRange` `frame_len`).
8. A VC maximum of 6 or 7 with FECF and no SDLS: `tc-cfg-014`, `tc-cfg-015` (`InvalidConfig` `frame_data_field`), with `tc-cfg-016` (8 accepted) and `tc-cfg-017` (6 without FECF accepted).
9. Both CLCW spares in strict mode: `tc-clcw-dec-015` (`ReservedValue`, field 86: bits 14-15 come first, a project order by field order); lenient counterpart, bit 23 alone: `tc-clcw-dec-016` (`ClcwSpareSet`); both set in lenient mode was `tc-clcw-dec-013`.
10. **Skipped: `tc-ad-peek-002` pinning `needed`.** The vector format has no `needed` key in an expected error (docs/07-test-vectors.md, `core/fields.json`; an error expectation carries `error` and `field` only), so no case can pin it without a new format.

AOS (`test-vectors/models/aos_frame.py`):
11. Wrong version and wrong FECF: `aos-inv-dec-009` (`CrcMismatch`).

USLP (`test-vectors/models/uslp_frame.py`; the model now resolves VCID 63 of a known Spacecraft ID on a fixed-length channel to a fixed-length VC with no VCF Count, OCF, Security fields, or truncated frames):
12. Replacement of `uslp-oid-019` (decision 1; it now has `by: uslp-oid-042`, and keeps its released expectation): `uslp-oid-042`/`-043` (encode, decode), `-044` (validate), `-045` (`peek_length`), `-046` (`delimit`), `-047`/`-048` (two Spacecraft IDs), `-049` to `-051` (unknown Spacecraft ID: encode, decode, validate reason 2), `-052`, `-053` (OCF and VCF Count refused by the defaults), `-054`/`-055` (an entry with 2-octet counts overrides), `-056`, `-057` (the two directions of the count-length mismatch), `-058`/`-059` (an entry with OCF Required), `-060` to `-062` (the OCF mismatch in both directions, with validate reason 9), `-063` (a truncated header on the default VCID 63: `InvalidField` `end_of_frame_primary_header_flag`; this follows from "a fixed-length VC" and Annex D1.2.1, and is not stated for VCID 63 in the Blue Book or in `uslp-frame.md`).
13. Five octets with TFVN 0000 or an unknown SCID: `uslp-inv-045`, `uslp-inv-046` (decode: `Incomplete`), `uslp-val-038` to `uslp-val-040` (validate: reason 13 before reasons 1 and 2).
14. Protocol control frame on a fixed-length VC: `uslp-pc-014` (encode, `InvalidField` `tfdz_construction_rule`), `uslp-pc-015` (decode strict), `uslp-pc-016` (lenient), `uslp-pc-017` (validate, reason 15). The Blue Book puts Control Commands in rule 111, which is variable-length (§4.1.4.2.2.2.8 d), and USLP-016 rejects rule 111 on a fixed-length VC. A frame with flags 11 and a fixed-length rule (000 to 010) is not rejected by any requirement and is not covered.

## Cases after the hazard analysis

Written on 2026-10-25 (`m2/hazard-controls`) by a blind Claude Sonnet 5.5 agent (TV-023: the Blue Books, `docs/`, and the vectors, never the implementation), by extending the USLP, TM, and AOS models and generating the files with their `--write`. Each item with the cases that cover it:

USLP (`test-vectors/models/uslp_frame.py`), OI-48 (a VC with a truncated frame length carries only truncated frames; validation reason 18 `NonTruncatedFrameNotAllowed`, delimiter item `non_truncated_not_allowed`):
- Superseded: `uslp-tsdls-003` and `uslp-tsdls-004` (a non-truncated frame on VC 6, truncated length 32), with their released expectations kept. Replaced by `uslp-tsdls-017`/`-018` (the same frame on VC 12, which has the layout 8/8 and no truncated length: encode, decode) and the refusals on VC 6: `uslp-tsdls-019` (encode), `-020` (decode, strict), `-021` (decode, lenient).
- Encode, decode, `peek_length`: `uslp-trunc-039` to `-055` (writer refusal on VC 5 and VC 8, its place in the writer order against the MAP check and the rule check; decoder refusal in both modes, before the length check, after reasons 3, 4, and 6, before reason 8; `peek_length` with 4 and 6 octets, and the control on VC 1).
- Validate: `uslp-val-041` to `-053` (reason 18 in both modes; reason 6 before it; it before reasons 8, 9, 11, 13, 14, and 15; reason 4 before it in strict mode; the control on VC 1).
- Delimit: `uslp-delim-017` to `-022` (`non_truncated_not_allowed` with the rest; the Frame Length of the header not trusted, so not `incomplete`; fewer than 7 octets, and a Frame Length below the minimum, are `fill`; a non-truncated header on an unconfigured VC is a `frame`).
- P-USLP-07 (`hz:HZ-USLP-05`): `uslp-delim-023` (a frame and six octets of non-`0x55` fill that read as the truncated header of VC 8, truncated length 6: a second `frame`), `uslp-delim-026` (`validate` accepts those six octets), `uslp-delim-024` (the same octets where no VC has length 6, the other half of `asm:ASM-12`: `no_truncated_length`), `uslp-delim-025` (OI-48 interaction: six fill octets that read as a non-truncated header on VC 8 are `fill`, since a non-truncated header needs 7).

TM (`test-vectors/models/tm_frame.py`), OI-51 (frame length at most 4 095 octets, TM-001): `tm-inv-cfg-010` to `-015` (4 095 accepted and 4 096 refused, with and without FECF, on a plain physical channel and a master channel; the Physical Channel is checked before the Master Channel). `tm-fhp-enc-005` and `tm-fhp-enc-err-002` (2 100 octets) are unchanged and still pass the model.

TM (`test-vectors/models/tm_frame.py`), OI-54 (an Only Idle Data frame on a VC that uses SDLS; TM-010, TM-011; 132.0-B-3 §6.4.6.2), added 2026-10-25 by a blind Claude Sonnet 5.5 agent in `test-vectors/tm/security_layout.json`: no existing vector changed its expectation, so none is superseded. No case for `decode_unchecked`: the vector operation of that name takes no channel table, so the rule cannot apply to it (TM-010 names `decode_unchecked` with the rule).
- `decode_with`, strict, `InvalidField` `vc_id`: `tm-sec-dec-007` (Security Header and Trailer), `-008` (Header only), `-009` (Trailer only).
- `decode_with`, lenient, accepted with `OnlyIdleDataOnSdlsVc`: `tm-sec-dec-010`, `-011`, `-012` (the same three VCs); `tm-sec-dec-013` (with `PacketOrderFlagSet` and `SegmentLengthIdNot11`).
- `decode` without the table, accepted and no anomaly: `tm-sec-dec-014` (strict), `-015` (lenient).
- Check order: `tm-sec-dec-016` (Packet Order Flag: `ReservedValue`), `-017` (Segment Length ID: `InvalidField` `segment_length_id`), `-018` (wrong FECF: `CrcMismatch`).
- Not triggered: `tm-sec-dec-019` (Sync Flag 1, FHP `0x7FE`), `-020` (Sync Flag 1, FHP `0x7FF`), `-021` (Sync Flag 0, FHP `0x7FF`), `-022` (Only Idle Data on a VC without SDLS).
- `validate`: `tm-sec-val-004` (strict) and `-005` (lenient) `InvalidField` `vc_id`; `-006` (lenient, Trailer only); `-007` (no SDLS: accepted); `-008` (lenient, a VCA_SDU VC with SDLS: `sync_flag` first, the SDLS rule is validate's last check).

AOS (`test-vectors/models/aos_frame.py`), P-AOS-04 (`hz:HZ-AOS-09`): `aos-i4-dec-009` and `aos-i4-dec-010` (the octets of an Issue 4 frame with a 2 052-octet Packet Zone and the pointer `0x7FF`, then `0x7FE`, read with the `issue5` edition in the configuration: `StartsAt(2047)` and `StartsAt(2046)`, no anomaly). The vector format expresses the receiver's edition in `config`, so no case was skipped.

## Not verified

- The independent models were not read by the reviewers, by design; their independence rests on the notes above.
- Values that rest only on a project decision (check orders, reason codes, the delimiter's `Fill` rule, the writers' error order, capacities) were checked for consistency with the documents and OIs, not against the Blue Book, which does not decide them.
- The C ABI, the C++ implementation, and the Rust codecs were not read or run by the reviewers.
