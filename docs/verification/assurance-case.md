# Assurance case

A structured argument that Aldis, as of Milestone 3 (the frame codecs of Milestone 2 and the COP-1 engines), does what its specification says and fails in ways a caller can detect, from four top claims down to the requirements and the evidence that supports them. Requirement: VER-066 ([06 §11](../06-verification.md#11-hazard-analysis-and-assurance-case)). The [hazard analysis](hazards.md) (VER-065) is its other half: each hazard names the claims its controls support, and each claim is weakened by the residuals the hazards state. `cargo xtask assurance` checks that every reference below resolves ([assurance-evidence.md](assurance-evidence.md)).

The case claims no compliance with any standard ([06](../06-verification.md), assurance target). It is written so that a later qualification can take it as its starting point.

## How to read it

- A **claim** is a section `AC-n…`, with a `**Claim.**` paragraph. Its parent is the claim whose ID is its own without the last number; the top claims' parent is `AC-0`.
- A claim with sub-claims has a `**Strategy.**` paragraph: how the sub-claims together establish it.
- A claim without sub-claims is a **leaf** and has an evidence table, `| Evidence | Shows | Requirements |`, whose first column holds references of [assurance-evidence.md](assurance-evidence.md) §2. A row whose Evidence is `TODO` is not yet filled.
- **Context** (`CTX-n`), **assumptions** (`ASM-n`), and **defeaters** (`DF-n`) are listed at the end and referenced with `asm:` and `df:`.
- Evidence that runs only at night says so by its kind (`nightly-test:`, `nightly-check:`, `job:`, `fuzz:`, `proof:`). Every other test, vector, and check runs in the merge gate of every pull request (VER-058), so citing it means it passed on every merged commit.

```mermaid
graph TD
  AC0["AC-0 Aldis M3 behaves as specified and fails detectably"]
  AC0 --> AC1["AC-1 Decodes and encodes per the Blue Books"]
  AC0 --> AC2["AC-2 Never panics"]
  AC0 --> AC3["AC-3 Terminates in bounded time, never blocks"]
  AC0 --> AC4["AC-4 Uses bounded resources"]
  AC0 --> AC5["AC-5 Reports every deviation"]
  AC0 --> AC6["AC-6 The C ABI keeps AC-1 to AC-5"]
  AC0 --> AC7["AC-7 The evidence is trustworthy"]
```

## AC-0 Aldis M3 behaves as specified and fails detectably

**Claim.** Within the context `asm:CTX-1` and `asm:CTX-2` and under the assumptions `asm:ASM-1` to `asm:ASM-18`, the library crates of Milestones 2 and 3 (`aldis-core`, `aldis-time`, `aldis-spp`, `aldis-epp`, `aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp`, `aldis-cop1`) and their C ABI (`aldis-ffi`) decode and encode as the cited Blue Books specify, run the FOP-1 and FARM-1 state machines of 232.1-B-2, never panic or block, use bounded resources, and report every deviation they can detect.

**Strategy.** Argue over the four properties of the plan's assurance-case row ([M2 plan](../milestones/m2-frame-codecs.md) §3, WP-8): conformance (AC-1), panic freedom (AC-2), termination (AC-3), bounded resources (AC-4), and deviation reporting (AC-5); then that they hold through the C ABI (AC-6); then that the evidence for all of them can be trusted (AC-7). The COP-1 engines of Milestone 3 enter AC-1 as a standard (AC-1.8) and AC-5 as the service guarantee of the sequence-controlled service (AC-5.6), and the other claims take their evidence where it exists. The hazards of [hazards.md](hazards.md) are the failure modes each claim rules out or bounds; a hazard's residual is a limit on the claim it names.

## AC-1 Aldis decodes and encodes per the cited Blue Books

**Claim.** For every requirement of the frame standards and of the codec contract in the M2 exit criteria ([M2 plan](../milestones/m2-frame-codecs.md) §4), the decoders accept exactly the data units the requirement allows and report them field by field, and the encoders produce exactly the octets the Blue Book specifies (API-020).

**Strategy.** By standard. Each requirement table cites its clauses; each requirement is traced to the tests and vectors of its Verif. column (VER-020); the vectors come from Blue Book material where it exists, and otherwise are hand-derived with a derivation, checked by an independent model written blind (TV-023), and reviewed a second time ([m2-vectors.md](reviews/m2-vectors.md)); every vector passes through the Rust API and the C ABI (for the COP-1 files see AC-1.8, `df:DF-19`, and `df:DF-20`). The M1 codecs are carried by the M1 exit. The independent cross-check of the frames is still missing (`df:DF-9`). The COP-1 engines (AC-1.8) use the same method with the tables as data (COP-003) and a closed-loop test of the two engines (COP-040), but not every scenario runs through both paths: the FOP-1 files run through the Rust API, the FARM-1 files only through the C ABI, and the C runner skips the `table_event` scenarios.

### AC-1.1 TM frames conform to 132.0-B-3

**Claim.** TM-001…011 hold.

| Evidence | Shows | Requirements |
|---|---|---|
| `vector:tm-min-dec-003`; `vector:tm-min-cfg-001`; `vector:tm-inv-len-001`; `vector:tm-inv-len-002`; `test:crates/aldis-tm/src/view.rs::length_and_fecf_are_checked_first` | The configured frame length (shortest 7 octets, no FECF) decodes; one octet short or long is LengthMismatch; length and FECF presence are checked before any field is read | TM-001 |
| `vector:tm-inv-ver-001`; `vector:tm-inv-ver-002`; `vector:tm-inv-ver-003`; `test:crates/aldis-tm/src/header.rs::header_rejects_version_and_short_input` | Version 01 and 11 are UnsupportedVersion, and peek_length rejects the version too | TM-002 |
| `vector:tm-fof-dec-004`; `vector:tm-fof-dec-005`; `vector:tm-inv-fsh-001`; `vector:tm-inv-fsh-003`; `test:crates/aldis-tm/src/view.rs::secondary_header_errors` | The FSH length is read from its ID octet (2 and 64 octets); an FSH version other than 00 and an FSH that overruns the frame are errors | TM-003 |
| `vector:tm-fof-dec-001`; `vector:tm-fof-dec-002`; `vector:tm-fof-dec-003`; `test:crates/aldis-tm/src/view.rs::secondary_header_and_ocf_follow_the_flags` | With the OCF Flag set the four OCF octets are exposed, whether they hold a CLCW, a project-specific report, or an SDLS FSR | TM-004 |
| `vector:tm-inv-crc-001`; `vector:tm-fof-dec-006`; `test:crates/aldis-tm/tests/bit_flips.rs::every_single_bit_error_is_rejected`; `test:crates/aldis-tm/tests/bit_flips.rs::every_double_bit_error_of_the_short_frames_is_rejected` | A flipped bit is CrcMismatch, found before a view is returned; decode_unchecked still decodes a corrupted frame for diagnostics; every single-bit and every double-bit error of the short frames is rejected | TM-005 |
| `vector:tm-fof-dec-001`; `vector:tm-sec-dec-006`; `vector:tm-inv-ocf-001`; `test:crates/aldis-tm/src/view.rs::security_layout_places_the_data_field` | The data field is the frame less header, FSH, security header and trailer, OCF and FECF; a layout that leaves no data octet is rejected | TM-006 |
| `vector:tm-inv-pof-001`; `vector:tm-inv-slid-001`; `vector:tm-inv-pof-002`; `vector:tm-fhp-vca-001`; `test:crates/aldis-tm/src/view.rs::sync_flag_0_fixed_fields`; `test:crates/aldis-tm/src/view.rs::sync_flag_1_fields_are_raw` | With Sync Flag 0, Packet Order Flag 1 and Segment Length ID other than 11 are errors in strict mode and anomalies in lenient mode; with Sync Flag 1 the fields are exposed raw | TM-007 |
| `vector:tm-fhp-dec-001`; `vector:tm-fhp-dec-003`; `vector:tm-fhp-dec-004`; `vector:tm-fhp-err-001`; `test:crates/aldis-tm/tests/restricted.rs::packet_pointer_classifies_every_raw_value` | The FHP is classified as PacketStartsAt, NoPacketStart (0x7FF), or OnlyIdleData (0x7FE); an offset at or past the data field length is Malformed; every raw value classifies | TM-008 |
| `vector:tm-fof-enc-001`; `vector:tm-fhp-enc-004`; `vector:tm-inv-enc-006`; `test:crates/aldis-tm/tests/properties.rs::encode_then_decode_round_trips`; `fuzz:tm_roundtrip` | The writer computes the FSH ID octet, the OCF flag and the FECF from the configuration and takes the counts, sync flag and FHP as inputs; encode then decode returns the fields | TM-009 |
| `vector:tm-oid-001`; `vector:tm-oid-003`; `vector:tm-oid-006`; `test:crates/aldis-tm/src/writer.rs::oid_frames_continue_the_sequence`; `test:crates/aldis-ffi/tests/tm.rs::encode_oid_takes_a_generator_handle` | OID frames carry FHP 0x7FE and the next octets of the generator; a VC that is not a packet VC, or that uses SDLS, is InvalidField | TM-010 |
| `vector:tm-fof-val-001`; `vector:tm-inv-val-001`; `vector:tm-inv-val-003`; `test:crates/aldis-tm/src/validate.rs::flag_and_length_mismatches` | validate checks SCID and VCID, and that the OCF Flag and the FSH presence and length match the channel configuration | TM-011 |
| `test:crates/aldis-tm/tests/vectors.rs::minimal`; `test:crates/aldis-tm/tests/vectors.rs::fsh_ocf_fecf`; `test:crates/aldis-tm/tests/vectors.rs::fhp_values`; `test:crates/aldis-tm/tests/vectors.rs::oid_sequence`; `test:crates/aldis-tm/tests/vectors.rs::security_layout`; `test:crates/aldis-tm/tests/vectors.rs::invalid` | The Rust API passes every live case of the six TM vector files, decode, encode, validate, and configuration | TM-001…011 |
| `test:crates/aldis-ffi/tests/vectors.rs::tm_minimal`; `test:crates/aldis-ffi/tests/vectors.rs::tm_fsh_ocf_fecf`; `test:crates/aldis-ffi/tests/vectors.rs::tm_fhp_values`; `test:crates/aldis-ffi/tests/vectors.rs::tm_oid_sequence`; `test:crates/aldis-ffi/tests/vectors.rs::tm_security_layout`; `test:crates/aldis-ffi/tests/vectors.rs::tm_invalid` | The same six files pass through the C ABI, including the error kinds and field codes | TM-001…011 |

### AC-1.2 TC frames and the CLCW conform to 232.0-B-4

**Claim.** TC-001…008, TC-020…023, TC-030, and TC-031 hold.

| Evidence | Shows | Requirements |
|---|---|---|
| `vector:tc-ad-dec-004`; `vector:tc-dec-err-002`; `vector:tc-max-dec-001`; `test:crates/aldis-tc/src/view.rs::decodes_an_ad_frame` | The length comes from the Frame Length field and exactly that many octets are consumed, with trailing octets left; a short input is Incomplete | TC-001 |
| `vector:tc-dec-err-003`; `vector:tc-dec-err-005`; `vector:tc-ad-peek-004`; `test:crates/aldis-tc/src/header.rs::raw_header_checks` | TFVN other than 00 and the reserved flag combination Bypass 0 / Control Command 1 are errors in both modes, and peek_length applies the same checks | TC-002 |
| `vector:tc-dec-err-007`; `vector:tc-dec-err-008`; `vector:tc-val-006`; `test:crates/aldis-tc/src/view.rs::header_errors_in_check_order` | A non-zero Reserved Spare is ReservedValue in strict mode and an anomaly in lenient mode | TC-003 |
| `vector:tc-seg-dec-001`; `vector:tc-bc-unlock-dec-003`; `vector:tc-cfg-008`; `test:crates/aldis-tc/src/view.rs::segment_header_and_sdls_slices` | The Segment Header is parsed only on a Type-D frame of a VC that has one; a Type-C frame on such a VC has none; a MAP on a VC without Segment Headers is a configuration error | TC-004 |
| `vector:tc-bc-unlock-dec-001`; `vector:tc-bc-setvr-dec-001`; `vector:tc-bc-setvr-dec-004`; `vector:tc-bc-setvr-dec-008`; `test:crates/aldis-tc/src/view.rs::control_commands` | A Type-C data field decodes as Unlock or Set V(R); any other pattern is ReservedValue in strict mode and UnknownControlCommand in lenient mode | TC-005 |
| `vector:tc-ad-dec-001`; `vector:tc-dec-err-011`; `vector:tc-val-019`; `test:crates/aldis-tc/tests/bit_flips.rs::every_single_bit_error_is_rejected` | The FECF is verified when configured and a mismatch is CrcMismatch; every single-bit error of the frames tried is rejected | TC-006 |
| `vector:tc-max-enc-001`; `vector:tc-max-enc-err-001`; `vector:tc-max-enc-err-002`; `vector:tc-max-enc-err-003`; `test:crates/aldis-tc/src/writer.rs::encode_checks` | The writer computes Frame Length and the FECF and rejects a frame above the 1024 octets or the configured maximum, and a data field over 1019 (1017 with FECF) | TC-007 |
| `vector:tc-bd-enc-002`; `vector:tc-bd-dec-002`; `test:crates/aldis-tc/src/writer.rs::encode_round_trips` | A Type-B frame is written with the caller's N(S) (0x5A) and decoded back unchanged; the codec does not force 0 | TC-008 |
| `vector:tc-delimit-002`; `vector:tc-delimit-004`; `vector:tc-delimit-007`; `test:crates/aldis-tc/tests/vectors.rs::delimit_multiple_frames_with_fill`; `test:crates/aldis-ffi/tests/tc.rs::the_stream_intake_pipeline`; `fuzz:tc_delimit` | FrameDelimiter yields the frames of a data unit in order and treats fill and a truncated tail as the procedure specifies; it is stateless across the C ABI | TC-020 |
| `vector:tc-val-003`; `vector:tc-val-004`; `vector:tc-val-009`; `vector:tc-val-014`; `test:crates/aldis-tc/tests/vectors.rs::validation_failures` | validate_frame applies checks (a) TFVN, (b) MCID, (c) header values, (d) length, and (e) FECF, each with its own reason code, in a fixed order | TC-021 |
| `vector:tc-val-008`; `vector:tc-val-015`; `test:crates/aldis-tc/src/validate.rs::reason_codes_are_stable` | An invalid VCID is discarded as InvalidVcid and an invalid MAP ID as InvalidMapId | TC-022 |
| `vector:tc-val-001`; `vector:tc-val-002`; `test:crates/aldis-tc/src/validate.rs::rejections_carry_reason_and_error` | The same checks accept a Type-A and a Type-B frame; the order against FARM-1 processing belongs to the engine of M3 | TC-023 |
| `vector:tc-clcw-dec-001`; `vector:tc-clcw-dec-004`; `vector:tc-clcw-dec-006`; `vector:tc-clcw-enc-001`; `test:crates/aldis-tc/tests/properties.rs::clcw_round_trips`; `fuzz:tc_clcw` | Every CLCW field encodes and decodes; strict decoding requires Control Word Type 0, version 00 and zero spares, and accepts any COP in Effect value | TC-030 |
| `vector:tc-clcw-dec-003`; `vector:tc-clcw-dec-014`; `vector:tc-clcw-enc-003`; `test:crates/aldis-tc/tests/restricted.rs::clcw_status_is_three_bits` | The Status Field is passed through unchanged, in all eight values | TC-031 |
| `test:crates/aldis-tc/tests/vectors.rs::ad`; `test:crates/aldis-tc/tests/vectors.rs::bd`; `test:crates/aldis-tc/tests/vectors.rs::bc_unlock`; `test:crates/aldis-tc/tests/vectors.rs::bc_setvr`; `test:crates/aldis-tc/tests/vectors.rs::segment_header`; `test:crates/aldis-tc/tests/vectors.rs::max_length`; `test:crates/aldis-tc/tests/vectors.rs::delimit_multiple_frames_with_fill`; `test:crates/aldis-tc/tests/vectors.rs::validation_failures`; `test:crates/aldis-tc/tests/vectors.rs::decode_errors`; `test:crates/aldis-tc/tests/vectors.rs::config`; `test:crates/aldis-tc/tests/vectors.rs::clcw` | The Rust API passes every live case of the eleven TC vector files | TC-001…008, TC-020…023, TC-030, TC-031 |
| `test:crates/aldis-ffi/tests/vectors.rs::tc_ad`; `test:crates/aldis-ffi/tests/vectors.rs::tc_bd`; `test:crates/aldis-ffi/tests/vectors.rs::tc_bc_unlock`; `test:crates/aldis-ffi/tests/vectors.rs::tc_bc_setvr`; `test:crates/aldis-ffi/tests/vectors.rs::tc_segment_header`; `test:crates/aldis-ffi/tests/vectors.rs::tc_max_length`; `test:crates/aldis-ffi/tests/vectors.rs::tc_delimit_multiple_frames_with_fill`; `test:crates/aldis-ffi/tests/vectors.rs::tc_validation_failures`; `test:crates/aldis-ffi/tests/vectors.rs::tc_decode_errors`; `test:crates/aldis-ffi/tests/vectors.rs::tc_config`; `test:crates/aldis-ffi/tests/vectors.rs::tc_clcw` | The same eleven files pass through the C ABI | TC-001…008, TC-020…023, TC-030, TC-031 |

### AC-1.3 AOS frames conform to 732.0-B-5 and, in `Issue4Compatible`, to 732.0-B-4

**Claim.** AOS-001…003, AOS-010…019, and GEN-062 hold.

| Evidence | Shows | Requirements |
|---|---|---|
| `vector:aos-cfg-001`; `vector:aos-i4-cfg-001`; `vector:aos-i4-dec-001`; `test:crates/aldis-aos/src/config.rs::physical_channel_minimum` | The physical channel configuration carries the edition, Issue5 by default, and Issue4Compatible selects the older layout | AOS-001 |
| `vector:aos-i4-dec-001`; `vector:aos-i4-dec-004-s`; `vector:aos-i4-dec-004-l`; `vector:aos-scid-dec-001`; `test:crates/aldis-aos/src/view.rs::issue_4_spares` | In Issue4Compatible the SCID is 8 bits and bits 42-43 are a reserved spare: ReservedValue in strict mode, Issue4SpareSet in lenient; in Issue5 they are the SCID high bits | AOS-002 |
| `vector:aos-i4-dec-002`; `vector:aos-i4-dec-003`; `vector:aos-i4-dec-005-s`; `vector:aos-i4-dec-005-l`; `test:crates/aldis-aos/src/mpdu.rs::decode_issue_4_spare` | In Issue4Compatible the M_PDU header is five spare bits and an 11-bit FHP with 0x7FF and 0x7FE reserved; a non-zero spare is ReservedValue in strict mode and Issue4SpareSet in lenient | AOS-003 |
| `vector:aos-i4-dec-007`; `vector:aos-i4-dec-008`; `vector:aos-i4-cfg-002`; `vector:aos-bpdu-cfg-005`; `test:crates/aldis-aos/src/mpdu.rs::reserved_values_per_edition` | The edition changes the wire reading: the same octets decode differently in Issue5, and the SCID width and B_PDU limit follow the edition; Issue5 is the default | GEN-062 |
| `vector:aos-min-dec-001`; `vector:aos-inv-dec-007`; `vector:aos-inv-dec-008`; `test:crates/aldis-aos/src/view.rs::unknown_channels`; `test:crates/aldis-ffi/tests/aos.rs::the_decoder_takes_the_master_channels_vcs` | Decoding is two-step, header then frame against a VC lookup; an SCID or VCID without configuration is UnknownChannel naming the field | AOS-010 |
| `vector:aos-inv-dec-004`; `vector:aos-inv-dec-005`; `vector:aos-inv-dec-006`; `test:crates/aldis-aos/src/header.rs::header_rejects_version_and_short_input` | Versions 00, 10 and 11 are UnsupportedVersion; only 01 is accepted | AOS-011 |
| `vector:aos-min-dec-008`; `vector:aos-min-dec-010-s`; `vector:aos-min-dec-010-l`; `test:crates/aldis-aos/src/view.rs::strict_and_lenient_deviations` | The cycle is reported only when the Use Flag is 1; with Use Flag 0 a non-zero cycle is InvalidField in strict mode and an anomaly in lenient | AOS-012 |
| `vector:aos-fhec-enc-003`; `vector:aos-fhec-c1-001`; `test:crates/aldis-aos/src/fhec.rs::encoder_agrees_with_the_model`; `test:crates/aldis-aos/src/fhec.rs::decoder_agrees_with_the_model_on_all_65536_syndromes`; `fuzz:aos_fhec_model`; `file:test-vectors/models/aos_fhec.py` | The FHEC encodes and decodes as the (15,11) Reed-Solomon code shortened to (40,24): vectors derived by hand and by an independent model written blind, the crate's brute-force model over all 65 536 syndromes, and a fuzz comparison | AOS-013 |
| `vector:aos-fhec-c1-001`; `vector:aos-fhec-c2-001`; `vector:aos-fhec-un-002`; `vector:aos-fhecf-dec-005`; `test:crates/aldis-aos/tests/bit_flips.rs::every_double_bit_error_of_the_short_frames_is_corrected_or_rejected` | Up to two symbol errors in the ten transmitted symbols are corrected before any field is read, the corrected-symbol count is reported, three or more are HeaderUncorrectable, and the frame count is not protected | AOS-014 |
| `vector:aos-iz-dec-001`; `vector:aos-iz-dec-003`; `vector:aos-iz-enc-002`; `test:crates/aldis-aos/src/view.rs::insert_zone_security_and_ocf` | The Insert Zone is a fixed-length slice present in every frame, an OID frame included | AOS-015 |
| `vector:aos-oid-enc-001`; `vector:aos-oid-dec-001`; `vector:aos-oid-cfg-001`; `test:crates/aldis-aos/tests/properties.rs::oid_frames_decode_as_idle_data`; `test:crates/aldis-ffi/tests/aos.rs::encode_oid_takes_a_generator_handle` | OID frames have VCID 63 and a data field from the generator; VCID 63 cannot be configured as a data VC | AOS-016 |
| `vector:aos-mpdu-dec-003`; `vector:aos-mpdu-dec-005-s`; `vector:aos-bpdu-dec-004`; `vector:aos-bpdu-dec-006-s`; `test:crates/aldis-aos/tests/restricted.rs::packet_pointer_classifies_every_raw_value`; `test:crates/aldis-aos/tests/restricted.rs::bitstream_pointer_classifies_every_raw_value` | M_PDU and B_PDU pointers classify as StartsAt, NoPacketStart, OnlyIdleData, ValidBits, AllValid, or OnlyIdle; a pointer that is neither a reserved value nor inside the zone is Malformed | AOS-017 |
| `vector:aos-bpdu-cfg-001`; `vector:aos-bpdu-cfg-002`; `vector:aos-bpdu-cfg-004`; `vector:aos-bpdu-cfg-005`; `test:crates/aldis-aos/src/config.rs::bitstream_limits` | A B_PDU VC with a Bitstream Data Zone over 2 047 octets is InvalidConfig in both editions, and exactly at the limit it is accepted | AOS-018 |
| `vector:aos-fhecf-enc-001`; `vector:aos-min-enc-001`; `vector:aos-min-enc-002`; `test:crates/aldis-aos/tests/properties.rs::two_phase_writer_matches_encode`; `fuzz:aos_roundtrip` | The writer computes the FHEC and the FECF and takes the VC frame count, cycle, replay flag and data field from the caller | AOS-019 |
| `test:crates/aldis-aos/tests/vectors.rs::fhec_encode`; `test:crates/aldis-aos/tests/vectors.rs::fhec_correct_1_symbol`; `test:crates/aldis-aos/tests/vectors.rs::fhec_correct_2_symbols`; `test:crates/aldis-aos/tests/vectors.rs::fhec_miscorrection`; `test:crates/aldis-aos/tests/vectors.rs::fhec_uncorrectable`; `test:crates/aldis-aos/tests/vectors.rs::minimal`; `test:crates/aldis-aos/tests/vectors.rs::fhec_frame`; `test:crates/aldis-aos/tests/vectors.rs::insert_zone`; `test:crates/aldis-aos/tests/vectors.rs::security_layout`; `test:crates/aldis-aos/tests/vectors.rs::mpdu_pointers`; `test:crates/aldis-aos/tests/vectors.rs::bpdu_pointers`; `test:crates/aldis-aos/tests/vectors.rs::oid`; `test:crates/aldis-aos/tests/vectors.rs::issue4_compat`; `test:crates/aldis-aos/tests/vectors.rs::scid_10bit`; `test:crates/aldis-aos/tests/vectors.rs::config_file`; `test:crates/aldis-aos/tests/vectors.rs::invalid` | The Rust API passes every live case of the sixteen AOS vector files | AOS-001…003, AOS-010…019, GEN-062 |
| `test:crates/aldis-ffi/tests/vectors.rs::aos_fhec_encode`; `test:crates/aldis-ffi/tests/vectors.rs::aos_fhec_correct_1_symbol`; `test:crates/aldis-ffi/tests/vectors.rs::aos_fhec_correct_2_symbols`; `test:crates/aldis-ffi/tests/vectors.rs::aos_fhec_miscorrection`; `test:crates/aldis-ffi/tests/vectors.rs::aos_fhec_uncorrectable`; `test:crates/aldis-ffi/tests/vectors.rs::aos_minimal`; `test:crates/aldis-ffi/tests/vectors.rs::aos_fhec_frame`; `test:crates/aldis-ffi/tests/vectors.rs::aos_insert_zone`; `test:crates/aldis-ffi/tests/vectors.rs::aos_security_layout`; `test:crates/aldis-ffi/tests/vectors.rs::aos_mpdu_pointers`; `test:crates/aldis-ffi/tests/vectors.rs::aos_bpdu_pointers`; `test:crates/aldis-ffi/tests/vectors.rs::aos_oid`; `test:crates/aldis-ffi/tests/vectors.rs::aos_issue4_compat`; `test:crates/aldis-ffi/tests/vectors.rs::aos_scid_10bit`; `test:crates/aldis-ffi/tests/vectors.rs::aos_config`; `test:crates/aldis-ffi/tests/vectors.rs::aos_invalid` | The same sixteen files pass through the C ABI | AOS-001…003, AOS-010…019, GEN-062 |

### AC-1.4 USLP frames conform to 732.1-B-3

**Claim.** USLP-001…010 and USLP-012…016 hold.

| Evidence | Shows | Requirements |
|---|---|---|
| `vector:uslp-inv-003`; `vector:uslp-inv-006`; `vector:uslp-val-006`; `vector:uslp-val-009`; `test:crates/aldis-uslp/src/header.rs::frame_kinds_follow_table_4_1` | TFVN other than 1100 and the reserved flag combination 01 are errors in both modes | USLP-001 |
| `vector:uslp-trunc-002`; `vector:uslp-trunc-004`; `vector:uslp-inv-021`; `vector:uslp-cfg-013`; `test:crates/aldis-uslp/src/truncated.rs::decodes_truncated_frames` | A truncated frame takes its length from the VC configuration (6 to 32 octets); a truncated header on a VC with no truncated length is an error | USLP-002 |
| `vector:uslp-fixed000-029`; `vector:uslp-inv-013`; `vector:uslp-val-019`; `vector:uslp-var-039`; `test:crates/aldis-uslp/src/view.rs::length_errors_on_a_variable_channel` | A fixed-length channel needs the configured length (LengthMismatch otherwise); on a variable-length channel the Frame Length field governs and a short input is Incomplete | USLP-003 |
| `vector:uslp-vcf-002`; `vector:uslp-inv-011`; `vector:uslp-val-016`; `vector:uslp-vcf-042`; `test:crates/aldis-uslp/tests/restricted.rs::vcf_count_edges_fit_their_length` | The VCF Count is decoded with the length in the header, lengths 0 to 7 round-trip, and a length that differs from the VC's configuration for the QoS is LengthMismatch | USLP-004 |
| `vector:uslp-fixed000-001`; `vector:uslp-fixedmapa-001`; `vector:uslp-var-001`; `vector:uslp-pc-001`; `test:crates/aldis-uslp/src/tfdf.rs::parse_and_octets` | The TFDF header is 3 octets with a pointer for rules 000, 001 and 010 and 1 octet for rules 011 to 111 | USLP-005 |
| `vector:uslp-fixed000-010`; `vector:uslp-fixedmapa-003`; `vector:uslp-fixed000-025`; `vector:uslp-inv-020`; `test:crates/aldis-uslp/tests/restricted.rs::tfdz_offset_is_below_all_ones` | The FHP and LVO classify as PacketStartsAt, NoPacketStartOrEnd, LastValidOctet, or SduContinues; an offset beyond the zone or 0xFFFE is Malformed | USLP-006 |
| `vector:uslp-fixed000-021`; `vector:uslp-fixed000-023`; `test:crates/aldis-uslp/src/upid.rs::every_value_round_trips`; `ci:sana` | The UPID is exposed raw with a typed reading taken from the SANA snapshot; an unassigned value decodes as Other without an anomaly, and the snapshot is checked against the registry | USLP-007 |
| `vector:uslp-cfg-009`; `vector:uslp-iz-001`; `vector:uslp-iz-002`; `vector:uslp-iz-011`; `test:crates/aldis-uslp/src/config.rs::physical_channel_limits` | An Insert Zone on a variable-length channel is InvalidConfig; on a fixed-length channel it is written and decoded | USLP-008 |
| `vector:uslp-oid-001`; `vector:uslp-cfg-020`; `vector:uslp-cfg-021`; `vector:uslp-val-034`; `test:crates/aldis-uslp/src/writer.rs::oid_frames`; `test:crates/aldis-ffi/tests/uslp.rs::encode_oid_takes_a_generator_handle` | OID frames have VCID 63, MAP ID 0, rule 001, UPID Only Idle Data and the generator's octets, only on fixed-length channels; a wrong OID header is an error in strict mode | USLP-009 |
| `vector:uslp-fixedmapa-001`; `vector:uslp-var-001`; `vector:uslp-vcf-001`; `vector:uslp-trunc-001`; `test:crates/aldis-uslp/tests/properties.rs::encode_then_decode_round_trips`; `fuzz:uslp_roundtrip` | The writer computes Frame Length, the VCF Count length, the TFDF header and the FECF, and takes the TFDZ as given | USLP-010 |
| `vector:uslp-tsdls-001`; `vector:uslp-tsdls-005`; `vector:uslp-cfg-045`; `vector:uslp-cfg-047`; `test:crates/aldis-uslp/src/truncated.rs::truncated_sdls_layout` | On truncated frames with SDLS the layout is the fixed 6-octet header and 16-octet trailer, and a truncated length under 28 is InvalidConfig | USLP-012 |
| `vector:uslp-pc-003`; `vector:uslp-pc-005`; `test:crates/aldis-uslp/tests/protocol_control.rs::cop1_commands_round_trip_through_the_tc_decoder`; `test:crates/aldis-uslp/tests/protocol_control.rs::the_cop1_vectors_carry_tc_control_commands` | The TFDZ of a protocol control frame decodes with the TC control-command decoder | USLP-013 |
| `vector:uslp-val-001`; `vector:uslp-val-006`; `vector:uslp-val-013`; `vector:uslp-val-028`; `test:crates/aldis-uslp/src/validate.rs::reason_codes_are_stable` | validate_frame applies the USLP Frame Validation Checks in a fixed order, each failure with its own reason code | USLP-014 |
| `vector:uslp-delim-001`; `vector:uslp-delim-003`; `vector:uslp-delim-009`; `test:crates/aldis-uslp/tests/vectors.rs::delimit_multiple_frames_with_fill`; `test:crates/aldis-ffi/tests/uslp.rs::the_uslp_stream_intake_pipeline` | FrameDelimiter yields frames in order, handles truncated frames, fill and an incomplete tail, and is stateless across the C ABI | USLP-015 |
| `vector:uslp-inv-019`; `vector:uslp-val-030`; `vector:uslp-val-031`; `vector:uslp-pc-015`; `test:crates/aldis-uslp/src/config.rs::vc_parameters` | The construction rule must match the VC Transfer Frame Type: rules 000 to 010 on fixed-length VCs and 011 to 111 on variable-length ones, an error in both modes | USLP-016 |
| `test:crates/aldis-uslp/tests/vectors.rs::fixed_packets_rule000`; `test:crates/aldis-uslp/tests/vectors.rs::fixed_mapa_rules001_010`; `test:crates/aldis-uslp/tests/vectors.rs::variable_rules011_111`; `test:crates/aldis-uslp/tests/vectors.rs::vcf_count_lengths_0_to_7`; `test:crates/aldis-uslp/tests/vectors.rs::truncated`; `test:crates/aldis-uslp/tests/vectors.rs::truncated_sdls_layout`; `test:crates/aldis-uslp/tests/vectors.rs::oid`; `test:crates/aldis-uslp/tests/vectors.rs::insert_zone`; `test:crates/aldis-uslp/tests/vectors.rs::protocol_control_cop1`; `test:crates/aldis-uslp/tests/vectors.rs::delimit_multiple_frames_with_fill`; `test:crates/aldis-uslp/tests/vectors.rs::validation_failures`; `test:crates/aldis-uslp/tests/vectors.rs::invalid`; `test:crates/aldis-uslp/tests/vectors.rs::config` | The Rust API passes every live case of the thirteen USLP vector files | USLP-001…010, USLP-012…016 |
| `test:crates/aldis-ffi/tests/vectors.rs::uslp_fixed_packets_rule000`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_fixed_mapa_rules001_010`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_variable_rules011_111`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_vcf_count_lengths_0_to_7`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_truncated`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_truncated_sdls_layout`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_oid`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_insert_zone`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_protocol_control_cop1`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_delimit_multiple_frames_with_fill`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_validation_failures`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_invalid`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_config` | The same thirteen files pass through the C ABI | USLP-001…010, USLP-012…016 |

### AC-1.5 The common link features conform

**Claim.** The OCF, the CLCW's opacity in `aldis-core`, the FECF, and the OID generator hold their requirements.

| Evidence | Shows | Requirements |
|---|---|---|
| `vector:crc16-check-002`; `vector:crc16-empty-002`; `test:crates/aldis-core/tests/vectors.rs::crc16_vectors`; `proof:crates/aldis-core/src/proofs.rs::crc16_matches_reference`; `fuzz:crc_variants` | crc16 matches the 132.0 generator: the check string and the empty input vectors, a proof that the table and bitwise variants agree with a reference, and a fuzz comparison | CMN-001, CMN-004 |
| `vector:crc16-check-002`; `test:crates/aldis-core/src/crc.rs::check_value_and_incremental`; `test:crates/aldis-ffi/tests/vectors.rs::core_crc16` | crc16 of "123456789" is 0x29B1, in Rust and through the C ABI | CMN-002 |
| `vector:tm-inv-crc-001`; `vector:aos-inv-dec-003`; `test:crates/aldis-core/src/crc.rs::fecf_write_verify_and_syndrome`; `test:crates/aldis-core/tests/properties.rs::both_verification_methods_agree`; `proof:crates/aldis-core/src/proofs.rs::fecf_methods_agree` | Recomputing the CRC and checking the syndrome for zero give the same verdict; a flipped bit is a CRC mismatch in the TM and AOS decoders | CMN-003 |
| `vector:ocf-classify-001`; `vector:ocf-classify-005`; `vector:ocf-classify-007`; `vector:ocf-classify-008`; `test:crates/aldis-core/tests/properties.rs::ocf_classification_matches_the_flag_bits`; `fuzz:ocf_classify` | Bits 0-1 classify the OCF as a Clcw, ProjectSpecific, or SdlsFsr report, for generated OCF words | CMN-010 |
| `vector:ocf-classify-003`; `test:crates/aldis-core/src/ocf.rs::the_clcw_stays_opaque`; `record:CMN-011` | aldis-core treats the CLCW as opaque except for bit 0 (every other bit set is still a Type-1 Report); the field decoding is in aldis-tc; reviewed in the record | CMN-011 |
| `vector:oid-start-002`; `vector:tm-oid-001`; `vector:aos-oid-enc-001`; `test:crates/aldis-core/src/oid.rs::both_forms_reproduce_annex_d`; `fuzz:oid_forms` | The generator reproduces the Annex D sequence from start-up, most significant bit first; the TM and AOS OID frames continue it | CMN-020 |
| `vector:oid-continuation-002`; `vector:tm-oid-005`; `vector:uslp-oid-003`; `test:crates/aldis-core/src/oid.rs::sequence_continues_across_fills`; `test:crates/aldis-ffi/tests/abi.rs::the_oid_generator_follows_the_handle_pattern` | The generator state persists across frames: consecutive OID frames of one generator continue the sequence, in Rust and through the C ABI | CMN-021 |
| `vector:oid-start-002`; `vector:oid-continuation-002`; `test:crates/aldis-core/src/oid.rs::forms_are_identical_over_a_long_run`; `fuzz:oid_forms` | The Galois form with seed 0b00000000001111111111111111111101 gives the same octets as the other form over a long run and on the Annex D vectors | CMN-022 |
| `vector:oid-continuation-002`; `test:crates/aldis-core/src/oid.rs::sequence_continues_across_fills`; `test:crates/aldis-ffi/tests/vectors.rs::core_oid` | fill writes the next buf.len() octets and advances by exactly that many (two fills of 10 octets continue without restart) | CMN-023 |
| `test:crates/aldis-ffi/tests/vectors.rs::core_crc16`; `test:crates/aldis-ffi/tests/vectors.rs::core_ocf`; `test:crates/aldis-ffi/tests/vectors.rs::core_oid` | The three core files with a C ABI pass through it | CMN-001…004, CMN-010, CMN-020…023 |

### AC-1.6 The codec contract holds for the frame codecs

**Claim.** The views, writers, configurations, and SDLS layouts of the four frame codecs meet the API model.

| Evidence | Shows | Requirements |
|---|---|---|
| `test:crates/aldis-tm/tests/properties.rs::decoders_never_panic`; `test:crates/aldis-aos/tests/properties.rs::decoders_never_panic`; `test:crates/aldis-tc/src/view.rs::decodes_an_ad_frame`; `test:crates/aldis-uslp/src/view.rs::decodes_a_fixed_frame` | Every accessor of a successful view returns without further checks; for TM and AOS a property over arbitrary octets checks that the layout ranges are contiguous and cover the input and the pointers lie in the data field, for TC and USLP the view tests read every accessor | API-010 |
| `vector:tc-ad-dec-004`; `vector:uslp-var-016`; `vector:uslp-trunc-004`; `test:crates/aldis-tc/src/view.rs::decodes_an_ad_frame` | TC and variable-length and truncated USLP frames decode from a longer input and report the consumed length | API-011 |
| `vector:tm-inv-len-001`; `vector:aos-inv-dec-002`; `vector:uslp-fixed000-029`; `vector:uslp-inv-013` | TM, AOS and fixed-length USLP frames of any length other than the configured one are LengthMismatch | API-012 |
| `vector:tm-min-peek-001`; `vector:aos-fhecf-peek-003`; `vector:tc-ad-peek-001`; `vector:uslp-var-036` | Each frame codec has peek_length: the configured length for TM, AOS and fixed USLP, the header-derived length for TC and variable USLP, and Incomplete or an error for a short or wrong prefix | API-013 |
| `vector:aos-min-hdr-001`; `test:crates/aldis-tm/src/header.rs::header_bits_follow_figure_4_2`; `test:crates/aldis-tc/src/header.rs::raw_header_and_octets`; `test:crates/aldis-uslp/src/header.rs::header_octets_round_trip` | Each view converts to an owned header value of plain fields with no borrow, whose fields and octets the header tests check; the header also decodes alone on the C ABI | API-014 |
| `vector:tm-min-enc-001`; `vector:tc-ad-enc-001`; `vector:aos-min-enc-001`; `vector:uslp-fixedmapa-001` | The encoders produce exactly the vector octets, which are derived by hand from the Blue Books | API-020 |
| `test:crates/aldis-tm/tests/properties.rs::encode_then_decode_round_trips`; `test:crates/aldis-aos/tests/properties.rs::encode_then_decode_round_trips`; `test:crates/aldis-tc/tests/properties.rs::encode_then_decode_round_trips`; `test:crates/aldis-uslp/tests/properties.rs::encode_then_decode_round_trips`; `fuzz:tm_roundtrip`; `fuzz:aos_roundtrip`; `fuzz:tc_roundtrip`; `fuzz:uslp_roundtrip` | decode(encode(x)) returns x for every generated valid frame of the four codecs, checked by property and by fuzz target | API-020 |
| `vector:tm-fof-enc-001`; `vector:aos-fhecf-enc-001`; `vector:tc-ad-enc-001`; `vector:uslp-var-001`; `vector:tm-inv-enc-004`; `vector:tm-inv-enc-006` | The encoders compute the length fields, FECF, FHEC, FSH ID octet and TFDF header; a caller-supplied value that would be inconsistent (wrong data length, OCF missing or given on a VC without one) is an error | API-021 |
| `vector:tm-inv-enc-010`; `vector:aos-inv-enc-009`; `test:crates/aldis-tm/tests/properties.rs::rejected_encode_leaves_the_buffer`; `test:crates/aldis-aos/tests/properties.rs::rejected_encode_leaves_the_buffer`; `test:crates/aldis-tc/tests/properties.rs::writers_agree`; `test:crates/aldis-uslp/tests/properties.rs::writers_agree` | A buffer one or more octets too small gives BufferTooSmall and the buffer is untouched, in all four codecs | API-022 |
| `test:crates/aldis-tm/src/writer.rs::unfinished_frame_is_defined`; `test:crates/aldis-aos/src/writer.rs::unfinished_frame_is_defined`; `test:crates/aldis-tc/src/writer.rs::unfinished_and_rejected_finish`; `test:crates/aldis-uslp/src/writer.rs::the_writer_reports_the_layout_of_the_reserved_frame` | A writer that is begun and never finished leaves a defined buffer: TM and AOS have the header written and the data zeroed, a TC frame decodes as UnsupportedVersion, and the USLP writer reports the layout of the reserved frame; nothing outside the supplied buffer is touched | API-023 |
| `vector:tm-inv-cfg-004`; `vector:uslp-cfg-009`; `vector:aos-oid-cfg-001`; `vector:tc-cfg-005`; `test:crates/aldis-tm/src/config.rs::master_channel_rejects_what_132_forbids`; `test:crates/aldis-aos/src/config.rs::table_rejects_what_732_forbids` | The configuration constructors reject what the Blue Books forbid: VC_FSH outside 2 to 64 octets, an Insert Zone on a variable-length USLP channel, a data VC with VCID 63, and a TC maximum below the minimum frame | API-040 |
| `record:API-041`; `test:crates/aldis-tm/tests/config_contract.rs::configurations_are_plain_data`; `test:crates/aldis-tc/tests/config_contract.rs::configurations_are_plain_data_built_in_const_context`; `test:crates/aldis-aos/tests/config_contract.rs::configurations_are_plain_data`; `test:crates/aldis-uslp/tests/config_contract.rs::configurations_are_plain_data_built_in_const_context` | The record reviews the TM, TC, AOS and USLP configuration types for references, heap, and interior mutability (none) and for the derives; each crate's test requires its types to be `Clone + PartialEq + Debug + Send + Sync + 'static` (`Copy` for all but the tables and the TM master channel), and the TC and USLP physical channels, VCs and MAPs fill a `static`. The tables are built at run time | API-041 |
| `vector:tm-sec-dec-001`; `vector:aos-sec-dec-001`; `vector:tc-ad-dec-005`; `vector:uslp-var-012`; `vector:layout-tc-sdls` | Data field lengths are computed with the Security Header and Trailer of the VC or MAP: frames with SDLS decode with the layout and the data field shrinks accordingly | API-070 |
| `vector:tm-sec-enc-001`; `vector:aos-sec-enc-001`; `vector:tc-ad-enc-005`; `vector:uslp-var-011`; `test:crates/aldis-ffi/tests/uslp.rs::the_encoder_leaves_the_sdls_fields_zero` | Views expose the Security Header and Trailer as slices and the writers as mutable slices for the hook to fill, the writer leaving them to the caller | API-072 |
| `vector:uslp-tsdls-001`; `vector:uslp-tsdls-005`; `vector:uslp-cfg-045`; `vector:uslp-cfg-047`; `vector:layout-uslp-truncated-sdls-002` | Truncated USLP frames with SDLS use the 6-octet header and 16-octet trailer, and a truncated length under 28 octets is rejected | API-075 |
| `test:crates/aldis-tm/src/view.rs::minimal_frame_accessors`; `test:crates/aldis-aos/src/view.rs::every_content`; `test:crates/aldis-tc/src/view.rs::decodes_an_ad_frame`; `test:crates/aldis-uslp/src/view.rs::decodes_a_fixed_frame` | The TM and AOS tests assert the data field is the input's own memory (pointer equality); the TC and USLP tests check the accessor contents but not pointer identity | GEN-013 |
| `vector:aos-inv-enc-009`; `test:crates/aldis-aos/src/writer.rs::two_phase_writer_fills_every_field`; `test:crates/aldis-tc/tests/properties.rs::writers_agree`; `test:crates/aldis-uslp/tests/properties.rs::writers_agree` | The writers produce a frame in a buffer of exactly its length, and write nothing past it | GEN-014 |
| `vector:aos-min-enc-001`; `vector:aos-i4-enc-001`; `test:crates/aldis-aos/src/writer.rs::issue_4_frames`; `test:crates/aldis-tc/src/writer.rs::encode_round_trips`; `test:crates/aldis-uslp/src/writer.rs::fixed_frames_round_trip` | The encoders write spare and reserved bits as 0, including the Issue 4 spares | GEN-052 |
| `vector:tc-ad-peek-005`; `vector:aos-bpdu-dec-002`; `vector:uslp-var-038`; `vector:uslp-val-021`; `test:crates/aldis-tc/src/header.rs::raw_header_and_octets`; `test:crates/aldis-aos/src/bpdu.rs::valid_bit_counts_are_1_to_0x3ffe` | The length-minus-one fields are exposed as true lengths (TC 1024 for field 1023, USLP 65536 for 0xFFFF, B_PDU valid bits 47 for pointer 46) | GEN-053 |
| `vector:tc-cfg-002`; `vector:tc-cfg-006`; `vector:aos-cfg-005`; `vector:uslp-cfg-004`; `test:crates/aldis-tc/src/config.rs::table_checks`; `test:crates/aldis-uslp/src/config.rs::table_checks`; `test:crates/aldis-aos/tests/restricted.rs::vc_tables_accept_exactly_the_configurable_channels` | Each configuration constructor validates all constraints of the Blue Book tables and returns InvalidConfig or OutOfRange naming the field | GEN-060 |
| `test:crates/aldis-tm/tests/config_contract.rs::a_configuration_cannot_change_after_construction`; `test:crates/aldis-tc/tests/config_contract.rs::a_configuration_cannot_change_after_construction`; `test:crates/aldis-aos/tests/config_contract.rs::a_configuration_cannot_change_after_construction`; `test:crates/aldis-uslp/tests/config_contract.rs::a_configuration_cannot_change_after_construction` | In each frame crate a builder or a copy taken from a configuration leaves the original equal to its clone, a different configuration is a new value from the validating constructor, and the configuration module has no method that takes `&mut self`. That the fields are private is a property of the types that no runtime test observes (no compile-fail test is available): the record of API-041 states it by review | GEN-061 |
| `test:crates/aldis-aos/src/config.rs::physical_channel_minimum`; `test:crates/aldis-aos/src/config.rs::virtual_channel_builder`; `test:crates/aldis-tm/src/config.rs::physical_channel_minimum`; `test:crates/aldis-tm/src/config.rs::virtual_channel_builder` | The TM and AOS physical channels are built in const context through validated constructors and the VC configurations through a builder, and read back through accessors | RS-012 |
| `test:crates/aldis-tc/tests/config_contract.rs::configurations_are_plain_data_built_in_const_context`; `test:crates/aldis-uslp/tests/config_contract.rs::configurations_are_plain_data_built_in_const_context`; `record:API-041` | The TC and USLP physical channel, VC and MAP are built by their validated `const` constructors into a `static`, read back through accessors, and implement `Clone + PartialEq + Debug`; the validation rejects a bad value. A VC with a truncated frame length is built there too (the truncated-frame-length constructor is a `const fn`). Not met by the letter: the tables are not `const`-constructible, and `VcParams` has public fields; the record lists the second as open | RS-012 |

### AC-1.7 The M1 codecs still conform

**Claim.** Every requirement in the M1 exit criteria keeps its artefacts.

| Evidence | Shows | Requirements |
|---|---|---|
| `ci:trace --scope m1 --strict`; `check:spec` | Every requirement of the M1 exit table is traced, on every commit; M1's own exit review established that the artefacts verify them | VER-020 |
| `test:crates/aldis-ffi/tests/vectors.rs::spp_decode_valid`; `test:crates/aldis-ffi/tests/vectors.rs::time_cuc`; `test:crates/aldis-ffi/tests/vectors.rs::epp_invalid` | The M1 vector files pass through the C ABI too (M2 plan D4), for example these three | VER-021, CABI-010, CABI-020 |

### AC-1.8 The COP-1 engines conform to 232.1-B-2

**Claim.** COP-001…006, COP-010…017, COP-030, COP-031, COP-033…036, COP-040, and COP-041 hold.

| Evidence | Shows | Requirements |
|---|---|---|
| `vector:cop1-fop1-s1-e1`; `vector:cop1-fop1-s6-e104`; `test:crates/aldis-cop1/tests/fop1_table.rs::test_exhaustive_300_cells`; `test:crates/aldis-cop1/tests/fop1_vectors.rs::fop1_scenarios`; `file:docs/verification/reviews/cop1-tables.md` | Table 5-1 is data, one vector per cell of the model written blind (S1 to S6, E1 to E46 and E101 to E104), every cell run through the engine in the Rust crate, and the cell-by-cell review against the Blue Book is kept | COP-001, COP-003, COP-005 |
| `vector:cop1-farm1-s1-e1`; `vector:cop1-farm1-s3-e11`; `test:crates/aldis-cop1/tests/farm1_table.rs::test_exhaustive_33_cells`; `test:crates/aldis-cop1/tests/farm1.rs::every_cell_of_table_6_1_through_step`; `test:crates/aldis-ffi/tests/vectors.rs::farm1_table` | Table 6-1 is data, every cell run through the step in the Rust crate and the 33 vectors through the C ABI; the cell marked "Not applicable" is `InvalidState` | COP-002, COP-003, COP-005 |
| `test:crates/aldis-cop1/tests/fop1_vectors.rs::the_action_bound_target_and_classification_at_run_time`; `test:crates/aldis-cop1/src/farm1.rs::every_cell_runs_within_the_bound_and_only_the_na_cell_fails`; `test:crates/aldis-cop1/src/farm1.rs::lookup_matches_the_table_and_the_bound_covers_every_cell`; `test:crates/aldis-ffi/tests/engines.rs::capacities_are_published_and_match_the_engines` | The step contract: the actions of a cell in its order, the action bounds derived from the table and the capacities, and the C ABI constants equal to them | COP-006, API-057 |
| `test:crates/aldis-cop1/tests/farm1_table.rs::test_notify_types`; `test:crates/aldis-cop1/tests/fop1_table.rs::test_fop1_types`; `vector:cop1-farm1-s2-e4`; `vector:cop1-fop1-s1-e14` | Each notification names the event and the prior state that caused it | COP-004 |
| `vector:cop1-fop1-ad-run`; `vector:cop1-fop1-retransmit-in-order`; `vector:cop1-fop1-initiate-unlock`; `vector:cop1-fop1-bd-service`; `test:crates/aldis-cop1/tests/fop1_uslp.rs::type_ad_and_type_bd_frames_carry_the_fdu_as_the_tfdz`; `test:crates/aldis-cop1/tests/fop1_uslp.rs::type_bc_frames_carry_the_control_commands_as_protocol_control` | Type-AD frames carry N(S) = V(S), Type-BD frames N(S) = 0, Type-BC frames Unlock or Set V(R), for the TC and the USLP target; the Sent_Queue holds byte-identical master copies, retransmitted in order | COP-010, COP-011 |
| `vector:cop1-fop1-modulo-256`; `vector:cop1-fop1-nr-invalid-wrap`; `test:crates/aldis-cop1/src/fop1.rs::a_clcw_that_matches_several_events_gets_the_highest_ranked` | N(R) is compared with NN(R) and V(S) modulo 256, and a word that matches several events gets the first by the project's ranking (OI-62) | COP-012 |
| `vector:cop1-fop1-timer-and-limit`; `vector:cop1-fop1-suspend-resume`; `vector:cop1-fop1-window-k`; `vector:cop1-fop1-directive-values`; `test:crates/aldis-cop1/src/fop1.rs::suspension_set_vs_and_alerts`; `test:crates/aldis-cop1/src/fop1.rs::queries_and_construction` | T1 starts, restarts, and cancels where the table lists it and is disarmed when it expires; Suspend_State records and restores the state; K, T1_Initial, the Transmission_Limit, and the Timeout_Type are rejected outside their ranges | COP-013, COP-014, COP-015 |
| `test:crates/aldis-cop1/src/fop1.rs::transmit_actions_carry_the_configured_repetitions`; `vector:cop1-fop1-alert-suppression`; `test:crates/aldis-cop1/src/fop1.rs::no_alert_follows_an_alert_before_an_initiate_ad` | A Transmit action carries the repetition value and the engine never duplicates a frame itself; no Alert other than Llif follows an Alert until an Initiate AD is accepted (or, if Suspend_State survived, a Resume), as a consequence of the table | COP-016, COP-017 |
| `vector:cop1-farm1-window-positive-edge`; `vector:cop1-farm1-window-negative-edge`; `vector:cop1-farm1-vr-wrap`; `vector:cop1-farm1-window-not-allowed`; `vector:cop1-farm1-bc-not-a-command`; `test:crates/aldis-cop1/tests/farm1.rs::window_classification_with_modular_sequence_numbers`; `test:crates/aldis-cop1/tests/farm1.rs::bc_frame_without_a_valid_command_is_an_invalid_frame` | N(S) is classified against V(R), PW, and NW modulo 256 for both window modes; a Type-BC frame with no valid command is an invalid frame | COP-030, COP-031 |
| `vector:cop1-farm1-buffer-wait`; `vector:cop1-farm1-farm-b-counter`; `vector:cop1-farm1-clcw-fields`; `vector:cop1-farm1-set-vr-in-lockout`; `test:crates/aldis-cop1/tests/farm1.rs::buffer_availability_is_the_callers_input`; `test:crates/aldis-cop1/tests/farm1.rs::farm_b_counter_counts_valid_type_b_frames_modulo_four`; `test:crates/aldis-cop1/tests/farm1.rs::clcw_follows_the_state_after_every_input`; `test:crates/aldis-cop1/tests/farm1.rs::lockout_is_left_only_by_unlock` | The buffer-available flag is an input, the FARM-B Counter counts modulo 4, the CLCW reflects the state after the last input, and Unlock resets FARM-1 only and is reported | COP-033, COP-034, COP-035, COP-036 |
| `test:crates/aldis-cop1/tests/closed_loop.rs::an_ideal_link_transfers_everything_without_retransmission`; `test:crates/aldis-cop1/tests/closed_loop.rs::every_impairment_at_once`; `test:crates/aldis-cop1/tests/closed_loop.rs::any_link_gives_the_service_guarantee_or_an_alert`; `test:crates/aldis-cop1/tests/closed_loop.rs::timeout_type_one_suspends_resumes_and_completes`; `test:crates/aldis-cop1/tests/closed_loop.rs::a_loss_after_a_resume_leaves_the_transfer_suspended_not_alerted` | FOP-1 and FARM-1 through a simulated link with loss, duplication, reordering, delay, and CLCW sampling: in-order, complete, duplicate-free delivery, or an Alert, and under Timeout Type 1 a suspended transfer | COP-040 |
| `test:crates/aldis-cop1/tests/event_sequences.rs::fop1_stays_within_table_5_1_under_any_event_sequence`; `test:crates/aldis-cop1/tests/event_sequences.rs::farm1_stays_within_table_6_1_under_any_event_sequence`; `test:crates/aldis-cop1/tests/farm1_properties.rs::any_input_sequence_keeps_the_invariants` | Random event sequences keep both engines inside their tables and panic-free | COP-041, GEN-020 |

## AC-2 Aldis never panics

**Claim.** No public function of the library crates or of the C ABI panics, aborts, or traps for any input value, buffer content, or call sequence (GEN-020).

**Strategy.** Three independent kinds of evidence: a proof by linking that no panic path survives in the exercised release code (AC-2.1); source rules that exclude the constructs that panic, with every exception reviewed (AC-2.2); and dynamic testing that drives arbitrary inputs through every decoder (AC-2.3). AC-2.1 is the strongest; its limits are `df:DF-6` and `df:DF-10`. Hazard: `hz:HZ-X-02`.

### AC-2.1 No panic path survives in the exercised release link

**Claim.** A release build that calls every public function, including every C ABI function, links with a panic handler that references an undefined symbol, on every gating bare-metal target.

| Evidence | Shows | Requirements |
|---|---|---|
| `record:VER-010` | What the exerciser calls (every public API of the library crates and every C ABI function), with which inputs (every live vector case), and how the canary keeps the check from passing vacuously | VER-010, GEN-020, GEN-021 |
| `check:exerciser-thumbv7em-none-eabihf`; `check:exerciser-armv7r-none-eabihf`; `check:exerciser-riscv32imac-unknown-none-elf` | The link check, the canary, and the vector run under QEMU, gating on every pull request | VER-010, GEN-021 |
| `file:exerciser/src/ffi.rs` | The C ABI group of the exerciser (M2 plan D16), so the link check covers `aldis-ffi` | VER-010 |
| `nightly-check:rust-sparc-leon3` | The same on LEON3, reported and not gating (OI-08) | VER-010 |

### AC-2.2 The source excludes the constructs that panic

**Claim.** Library code has no `unwrap`, `expect`, `panic!`, unchecked indexing, or unchecked arithmetic, and no `unsafe` outside `aldis-ffi`; every exception is a reasoned, local `#[expect]`.

| Evidence | Shows | Requirements |
|---|---|---|
| `check:clippy`; `check:clippy` | The lint set that denies those constructs, and that every crate passes it with no warning | VER-001, GEN-020 |
| `record:RS-016` | Exceptions are local `#[expect(…, reason)]` items, read by the reviewer of each pull request | RS-016 |
| `check:clippy`; `check:clippy` | `unsafe` is forbidden outside `aldis-ffi`, so safe Rust's own guarantees hold there | GEN-070, VER-002 |

### AC-2.3 Arbitrary inputs do not make a decoder panic

**Claim.** Fuzzing and property tests, over arbitrary octets and generated configurations, find no panic.

| Evidence | Shows | Requirements |
|---|---|---|
| `record:VER-022`; `fuzz:tm_decode`; `fuzz:tc_decode`; `fuzz:tc_delimit`; `fuzz:aos_decode`; `fuzz:aos_fhec_model`; `fuzz:uslp_decode`; `fuzz:uslp_truncated`; `fuzz:ffi_decode`; `job:fuzz` | Each target asserts no panic, among its other properties, every night for 600 s; before a release for at least one CPU-hour (`df:DF-3`) | VER-022, GEN-020 |
| `test:crates/aldis-tc/tests/properties.rs::decoder_never_panics`; `test:crates/aldis-tc/tests/properties.rs::delimiter_covers_any_octets`; `test:crates/aldis-uslp/tests/properties.rs::delimiter_covers_any_octets` | Property tests, run on every pull request, over arbitrary decoder and delimiter input | GEN-020, VER-023 |
| `test:crates/aldis-cop1/tests/event_sequences.rs::fop1_stays_within_table_5_1_under_any_event_sequence`; `test:crates/aldis-cop1/tests/event_sequences.rs::farm1_stays_within_table_6_1_under_any_event_sequence`; `test:crates/aldis-cop1/tests/farm1_properties.rs::any_input_sequence_keeps_the_invariants`; `file:fuzz/fuzz_targets/cop1_events.rs` | COP-1: random event sequences through both engines, every gating run; the fuzz target `cop1_events` is written and not yet registered, so the nightly job does not run it (`df:DF-17`) | GEN-020, COP-041 |

## AC-3 Every call terminates in bounded time and never blocks

**Claim.** Every call returns, in time bounded by its input length and the configured capacities, without waiting (GEN-022, GEN-023).

**Strategy.** Termination follows from two facts: every loop has a bound (AC-3.1) and nothing recurses (AC-3.2); the bound in time from the same facts, since each bound is an input length, a capacity, or a constant. Not blocking follows from the absence of anything to wait on (AC-3.3). Hazard: `hz:HZ-X-03`.

### AC-3.1 Every loop is bounded

**Claim.** Every loop of library code is bounded by an input length, a compile-time capacity, or a constant.

| Evidence | Shows | Requirements |
|---|---|---|
| `check:spec`; `record:GEN-022` | `cargo xtask loops` finds every loop of library code: one over a slice, an array, or a closed range is bounded by what it iterates; every `while`, `while let`, `loop`, and open or endless iteration carries a `// bound:` comment with its bound, and the check fails on a missing comment or one that is not above a loop | GEN-022 |
| `check:stack`; `record:VER-011` | Object-level static analysis confirming recursion freedom and entry-point stack depth | GEN-022 |
| `proof:crates/aldis-core/src/proofs.rs::error_kind_lookup_is_bounded`; `nightly-check:kani`; `file:docs/verification/formal-proofs.md` | A machine proof of one loop's bound, and of the CRC and bit-field loops up to their input lengths; nightly only (`df:DF-2`) | GEN-022 |
| `test:crates/aldis-cop1/src/farm1.rs::a_list_without_room_fails_at_every_push_and_never_drops_an_action`; `test:crates/aldis-cop1/tests/fop1_vectors.rs::the_action_bound_target_and_classification_at_run_time` | The loops of the COP-1 engines run over the Sent_Queue capacity, and the action bounds are derived from the tables at compile time, so no input does work beyond them (`hz:HZ-COP1-10`) | COP-006, GEN-022 |

### AC-3.2 Nothing recurses

**Claim.** No function of the library reaches itself through any chain of calls.

| Evidence | Shows | Requirements |
|---|---|---|
| `check:stack`; `record:VER-011` | The call graph of the shipped C ABI library and of the exerciser, built from object code on three targets, is checked complete (no indirect call outside stated exemptions) and fails on a cycle | GEN-022 |

### AC-3.3 Nothing blocks

**Claim.** Library code has no blocking construct, I/O, clock, or global mutable state to wait on.

| Evidence | Shows | Requirements |
|---|---|---|
| `record:GEN-022` | Analysis of architecture: `#![no_std]`, no heap allocation, no locks, atomics, or syscalls | GEN-023 |
| `record:CABI-004`; `check:no-std-thumbv7em-none-eabihf`; `check:no-std-armv7r-none-eabihf`; `check:no-std-riscv32imac-unknown-none-elf` | The bare-metal static library needs nothing from the platform beyond `memcpy` and `memset` equivalents, so it calls no operating system service that could block; GEN-031 and GEN-032 have no record of their own (`df:DF-13`) | CABI-004, GEN-005 |
| `check:layers` | No third-party runtime dependency brings any in | GEN-001 |

## AC-4 Aldis uses bounded resources

**Claim.** Aldis allocates no heap, writes only to the buffers it is given, and needs at most a published, budgeted stack per entry point.

**Strategy.** Memory a call can use is the heap (AC-4.1), its buffers (AC-4.3, AC-4.4), and the stack (AC-4.2). Hazards: `hz:HZ-X-01`, `hz:HZ-X-04`.

### AC-4.1 No heap

**Claim.** No library crate allocates dynamically in any configuration; every capacity is a compile-time parameter.

| Evidence | Shows | Requirements |
|---|---|---|
| `record:GEN-010` | Fixed capacities, and builds with no allocator | GEN-010, GEN-012 |
| `check:no-std-thumbv7em-none-eabihf`; `check:no-std-armv7r-none-eabihf`; `check:no-std-riscv32imac-unknown-none-elf`; `check:no-std-thumbv7em-none-eabihf` | The facade with every feature and the C ABI library build for bare metal without `alloc` | GEN-005, GEN-010 |
| `record:CABI-004` | The shipped static library imports no allocator | CABI-004 |
| `check:features`; `check:features` | `alloc` and `std` add no API and change no result | GEN-006 |

### AC-4.2 The stack of every entry point is bounded and within its budget

**Claim.** On each bare-metal target, every public Rust function and every C ABI function has a computed worst-case stack within the budget of its kind.

| Evidence | Shows | Requirements |
|---|---|---|
| `record:VER-011`; `check:stack` | The worst case per entry point from `-Z emit-stack-sizes` and the complete call graph, against the budgets, gating; channel table stack use decided per OI-45 (`df:DF-1`) | VER-011 |
| `file:xtask/stack-budgets.toml` | The budgets (4 096 bytes Rust, 10 240 bytes C ABI, every target) and the stated exemptions | VER-011 |
| `job:stack-sizes` | The per-target table published every night, and for the release | VER-011 |
| `oi:OI-45` | The channel tables' stack use decided per option (d) | VER-011, GEN-012 |

### AC-4.3 Outputs stay within their buffers and the data unit

**Claim.** No API needs more output space than the data unit it produces, and none writes outside the buffer it is given.

| Evidence | Shows | Requirements |
|---|---|---|
| `vector:tm-inv-enc-010`; `test:crates/aldis-tm/tests/properties.rs::rejected_encode_leaves_the_buffer`; `test:crates/aldis-tm/src/writer.rs::two_phase_writer_fills_every_field`; `test:crates/aldis-ffi/tests/tm.rs::encode_arguments_are_checked` | TM: a buffer one octet too small gives BufferTooSmall with the buffer untouched; into a larger buffer the writer leaves the 8 octets past the frame as they were; the C ABI checks the arguments | GEN-014, API-022 |
| `vector:aos-inv-enc-009`; `test:crates/aldis-aos/tests/properties.rs::rejected_encode_leaves_the_buffer`; `test:crates/aldis-aos/src/writer.rs::two_phase_writer_fills_every_field`; `test:crates/aldis-ffi/tests/aos.rs::encode_arguments_are_checked` | AOS: the same, including the case that a buffer of 48 octets receives a 40-octet frame and the other 8 stay as they were | GEN-014, API-022 |
| `test:crates/aldis-tc/tests/properties.rs::writers_agree`; `test:crates/aldis-tc/src/writer.rs::encode_checks`; `test:crates/aldis-ffi/tests/tc.rs::encode_arguments_are_checked` | TC: for every generated frame the writer fills a buffer of exactly the frame length, and one octet less gives BufferTooSmall with the buffer untouched | GEN-014, API-022 |
| `test:crates/aldis-uslp/tests/properties.rs::writers_agree`; `test:crates/aldis-ffi/tests/uslp.rs::content_and_fields_are_checked_as_arguments` | USLP: for generated frames and OID frames one octet short gives BufferTooSmall with the buffer untouched | GEN-014, API-022 |
| `test:crates/aldis-cop1/src/farm1.rs::an_area_shorter_than_the_fdu_fails_before_anything_is_written`; `test:crates/aldis-cop1/tests/farm1.rs::delivered_range_addresses_the_output_area`; `test:crates/aldis-cop1/src/fop1.rs::fdu_and_area_checks`; `test:crates/aldis-cop1/src/fop1.rs::decide_changes_nothing_and_commit_keeps_the_master_copies_in_queue_order`; `test:crates/aldis-ffi/tests/engines.rs::a_fop1_step_checks_every_argument`; `test:crates/aldis-ffi/tests/engines.rs::a_farm1_step_checks_every_argument` | COP-1: one frame per input at most, written from offset 0 of the caller's area; an area too short fails before any write; the store is changed only inside its positions; the C ABI rejects overlapping buffers | API-061, API-062, GEN-014 |

### AC-4.4 Memory accesses stay in bounds, whatever the values

**Claim.** Every slice access is bounds-checked, so a wrong length or offset, even one corrupted in memory, gives a wrong result or an error and never an access outside a buffer; an enum corrupted in memory is outside this claim (`asm:ASM-1`).

| Evidence | Shows | Requirements |
|---|---|---|
| `check:clippy`; `check:clippy` | Unchecked indexing is denied | GEN-020 |
| `record:GEN-015`; `file:docs/hardware-assumptions.md` | What an upset does and does not do, and the assumption that covers the rest | GEN-015 |

## AC-5 Aldis reports every deviation it can detect

**Claim.** In strict mode every checkable violation of a SHALL is an error naming its kind and field; in lenient mode the listed deviations are accepted and reported; a rejection changes nothing; errors on the link are detected within the bounds of the codes the Blue Books define; and the COP-1 engines deliver in order and without duplicates, or report.

**Strategy.** By the four parts of the API model's error contract (AC-5.1 to AC-5.4), then by the link codes (AC-5.5), whose limits are the residuals of `hz:HZ-X-06`, `hz:HZ-X-07`, `hz:HZ-TC-01`, and `hz:HZ-USLP-01`, then by the service guarantee of COP-1 (AC-5.6), whose limits are the residuals of `hz:HZ-COP1-01`, `hz:HZ-COP1-02`, `hz:HZ-COP1-09`, and `hz:HZ-COP1-18`. Hazard: `hz:HZ-X-08`.

### AC-5.1 Strict mode rejects every checkable violation

**Claim.** API-031 holds for each frame codec, with its two listed exceptions.

| Evidence | Shows | Requirements |
|---|---|---|
| `vector:tm-inv-pof-001`; `vector:tm-inv-slid-001`; `vector:tm-inv-fsh-003`; `test:crates/aldis-tm/src/view.rs::sync_flag_0_fixed_fields`; `fuzz:tm_decode` | TM: a Packet Order Flag or Segment Length ID deviation with Sync Flag 0, and an FSH that overruns the frame, are errors in strict mode; the fuzz target asserts the anomaly set is empty for every strict decode | API-030, API-031 |
| `vector:aos-min-dec-010-s`; `vector:aos-i4-dec-004-s`; `vector:aos-i4-dec-005-s`; `vector:aos-bpdu-dec-007-s`; `test:crates/aldis-aos/src/anomaly.rs::strict_reports_the_first_deviation`; `fuzz:aos_decode` | AOS: the cycle with Use Flag 0, the Issue 4 spares and the B_PDU spare are errors in strict mode; the fuzz target asserts the anomaly set is empty for every strict decode | API-030, API-031 |
| `vector:tc-dec-err-007`; `vector:tc-bc-setvr-dec-004`; `vector:tc-clcw-dec-015`; `vector:tc-clcw-dec-004`; `test:crates/aldis-tc/tests/properties.rs::decoder_never_panics`; `test:crates/aldis-tc/tests/properties.rs::clcw_decodes_any_word` | TC: a reserved spare, a reserved Control Command and CLCW spare bits are errors in strict mode, while a CLCW with any COP in Effect value is decoded (the listed exception, TC-030); the properties assert strict decodes carry no anomaly | API-030, API-031 |
| `vector:uslp-inv-006`; `vector:uslp-inv-007`; `vector:uslp-val-010`; `vector:uslp-oid-035`; `test:crates/aldis-uslp/tests/properties.rs::decoder_never_panics` | USLP: the reserved flags, Reserve Spares and an OID header deviation are errors in strict mode; the property asserts strict decodes carry no anomaly | API-030, API-031 |
| `vector:tm-oid-004`; `vector:aos-oid-dec-001`; `vector:uslp-oid-010` | The second listed exception: OID frames decode and validate by their headers, and the decode and validate calls take no generator, so the data cannot be checked against the sequence | API-031 |

### AC-5.2 Lenient mode accepts only the listed deviations, and reports each

**Claim.** API-032 and API-033 hold for each frame codec.

| Evidence | Shows | Requirements |
|---|---|---|
| `vector:tm-inv-pof-002`; `vector:tm-inv-slid-002`; `vector:tm-inv-both-001`; `vector:tm-inv-both-002`; `test:crates/aldis-tm/src/anomaly.rs::documented_bit_indices`; `file:docs/standards/tm-frame.md` | TM: the two listed deviations are accepted with PacketOrderFlagSet and SegmentLengthIdNot11; an FHP outside the data field stays Malformed in lenient mode; the flags and bit indices agree with the standard document | API-032, API-033, GEN-043 |
| `vector:aos-min-dec-010-l`; `vector:aos-i4-dec-004-l`; `vector:aos-bpdu-dec-007-l`; `vector:aos-mpdu-dec-005-l`; `test:crates/aldis-aos/src/anomaly.rs::documented_bit_indices`; `file:docs/standards/aos-frame.md` | AOS: the cycle, Issue 4 spare and B_PDU spare deviations are accepted with their flags; a pointer outside the zone stays Malformed; the flags and bit indices agree with the standard document | API-032, API-033, GEN-043 |
| `vector:tc-dec-err-008`; `vector:tc-bc-setvr-dec-008`; `vector:tc-clcw-dec-012`; `vector:tc-dec-err-004`; `test:crates/aldis-tc/src/anomaly.rs::documented_bit_indices`; `file:docs/standards/tc-frame.md` | TC: a reserved spare, an unknown Control Command and CLCW spare bits are accepted with their flags; a wrong TFVN stays an error in lenient mode; the flags agree with the standard document | API-032, API-033, GEN-043 |
| `vector:uslp-inv-008`; `vector:uslp-inv-004`; `vector:uslp-inv-014`; `vector:uslp-oid-036`; `test:crates/aldis-uslp/src/anomaly.rs::documented_bit_indices`; `file:docs/standards/uslp-frame.md` | USLP: Reserve Spares and an OID header deviation are accepted with their flags; a wrong TFVN and an over-long frame stay errors in lenient mode; the flags agree with the standard document | API-032, API-033, GEN-043 |
| `test:crates/aldis-tm/tests/properties.rs::decoders_never_panic`; `test:crates/aldis-aos/tests/properties.rs::decoders_never_panic`; `test:crates/aldis-tc/tests/properties.rs::decoder_never_panics`; `test:crates/aldis-uslp/tests/properties.rs::decoder_never_panics` | For arbitrary octets lenient accepts whatever strict accepts, with the same view, and a frame accepted only in lenient mode carries at least one anomaly flag (TM, AOS; TC and USLP assert strict is a subset of lenient with the same view) | API-032, GEN-043 |
| `fuzz:tm_decode`; `fuzz:aos_decode`; `fuzz:tc_decode`; `fuzz:uslp_decode`; `fuzz:anomaly_set_model` | The fuzz targets compare strict with lenient on arbitrary input and check the anomaly set against a model, including indices that do not fit in 32 bits | API-032, GEN-043 |
| `record:API-033` | The anomaly lists of the standard documents are reviewed against the declarations in every pull request that touches an anomaly table or declaration | API-033 |

### AC-5.3 Errors say what failed

**Claim.** Every error carries a catalogued kind, and a field code where it names a field.

| Evidence | Shows | Requirements |
|---|---|---|
| `vector:error-code-BufferTooSmall-002`; `vector:error-code-CrcMismatch-002`; `test:crates/aldis-core/src/error.rs::codes_are_dense_and_round_trip`; `test:crates/aldis-core/tests/vectors.rs::error_vectors`; `test:crates/aldis-ffi/src/error.rs::status_codes_are_the_catalogue` | Every error kind has a stable numeric code, the codes are dense and round-trip, and the C ABI status is the kind code | GEN-040 |
| `vector:tm-inv-fsh-002`; `vector:aos-inv-dec-007`; `vector:tc-seg-enc-err-001`; `vector:uslp-inv-012`; `test:crates/aldis-core/src/error.rs::detail_and_display`; `test:crates/aldis-ffi/src/error.rs::detail_carries_kind_field_and_values` | The frame vectors name the error kind and the field code of the failing field, and the detail carries the kind, field, and expected and found values without allocation, on the Rust side and in the C struct | GEN-041 |
| `ci:fields`; `test:crates/aldis-core/src/error.rs::field_codes_are_the_catalogue`; `test:crates/aldis-core/tests/restricted.rs::field_id_accepts_exactly_the_catalogue`; `test:xtask/src/vectors.rs::field_codes_need_an_error_and_the_catalogue` | The field codes are the catalogue of the API model: ci checks the catalogue, the generated code, and that a vector field code names a catalogued field of an error case | API-082 |
| `test:crates/aldis-ffi/tests/abi.rs::errors_carry_their_field_code_and_values`; `test:crates/aldis-ffi/tests/vectors.rs::tm_invalid`; `test:crates/aldis-ffi/tests/vectors.rs::tc_decode_errors`; `test:crates/aldis-ffi/tests/vectors.rs::aos_invalid`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_invalid` | The field codes cross the C ABI: the vector runs compare the kind and field code of every error case | GEN-041, API-082 |

### AC-5.4 A rejected input changes nothing

**Claim.** Decoding a malformed data unit, or a rejected encode, leaves caller-visible state as it was.

| Evidence | Shows | Requirements |
|---|---|---|
| `vector:tm-inv-enc-010`; `test:crates/aldis-tm/tests/properties.rs::rejected_encode_leaves_the_buffer`; `test:crates/aldis-tm/src/writer.rs::begin_rejects_without_writing`; `fuzz:tm_roundtrip` | TM: a rejected encode or writer begin leaves the output buffer as it was and the OID generator unchanged; the vector runner asserts the buffer is untouched after every error case | GEN-042, API-022 |
| `vector:aos-inv-enc-009`; `test:crates/aldis-aos/tests/properties.rs::rejected_encode_leaves_the_buffer`; `test:crates/aldis-aos/src/writer.rs::begin_rejects_without_writing`; `fuzz:aos_roundtrip` | AOS: the same, with the OID generator unchanged | GEN-042, API-022 |
| `test:crates/aldis-tc/tests/properties.rs::writers_agree`; `test:crates/aldis-tc/src/writer.rs::unfinished_and_rejected_finish`; `fuzz:tc_roundtrip`; `fuzz:tc_decode` | TC: a short buffer is left untouched and a rejected finish leaves the undecodable unfinished header; the decode target also checks every decoded frame against its re-encoding | GEN-042, API-022 |
| `test:crates/aldis-uslp/tests/properties.rs::writers_agree`; `test:crates/aldis-uslp/src/writer.rs::writer_errors_in_order`; `fuzz:uslp_roundtrip`; `fuzz:uslp_decode` | USLP: the same; the decode target also checks that the decoded input is unchanged | GEN-042, API-022 |
| `test:crates/aldis-cop1/src/fop1.rs::an_input_that_fails_leaves_the_engine_and_the_store_unchanged`; `test:crates/aldis-cop1/src/fop1.rs::every_failure_of_a_cell_leaves_the_state_unchanged`; `test:crates/aldis-cop1/tests/farm1.rs::errors_leave_the_state_unchanged_and_write_nothing`; `test:crates/aldis-ffi/tests/engines.rs::a_farm1_step_returns_its_actions_and_leaves_the_state_on_error` | COP-1: a rejected input (unknown channel, empty FDU, short area, a cell that is not applicable) leaves the engine, the store, and the output area as they were; the decision of FOP-1 is made apart from the state and applied by a commit that cannot fail (the pairing is the Rust caller's, `asm:ASM-14`) | API-058, COP-005, RS-022 |

### AC-5.5 Errors on the link are detected within the codes' bounds

**Claim.** With the FECF configured, every single-bit error, and every double-bit error in a frame of at most 4 095 octets, is rejected by the receiving checks, or for AOS header errors within the FHEC's reach corrected to the frame sent, and never accepted as another frame.

| Evidence | Shows | Requirements |
|---|---|---|
| `file:docs/verification/derivations/crc16-bit-flips.md` | The 4 095-octet bound of the CRC-16's double-bit guarantee | VER-064 |
| `test:crates/aldis-tm/tests/bit_flips.rs::every_single_bit_error_is_rejected`; `test:crates/aldis-tc/tests/bit_flips.rs::every_single_bit_error_is_rejected`; `test:crates/aldis-aos/tests/bit_flips.rs::every_single_bit_error_of_the_short_frames_is_corrected_or_rejected`; `test:crates/aldis-uslp/tests/bit_flips.rs::every_single_bit_error_of_the_short_frames_is_as_expected` | Every single-bit error, gated | VER-064, TM-005, TC-006, AOS-014, USLP-014 |
| `test:crates/aldis-tc/tests/bit_flips.rs::every_double_bit_error_of_the_short_frames_is_rejected`; `nightly-test:crates/aldis-tc/tests/bit_flips.rs::every_double_bit_error_of_the_longest_frame_is_rejected`; `nightly-test:crates/aldis-aos/tests/bit_flips.rs::every_double_bit_error_within_the_guarantee_is_corrected_or_rejected`; `nightly-test:crates/aldis-uslp/tests/bit_flips.rs::every_double_bit_error_within_the_guarantee_is_as_expected`; `job:bit-flips` | Every double-bit error: of the short frames gated, of the longest frames at night | VER-064 |
| `test:crates/aldis-tc/tests/bit_flips.rs::decode_may_accept_a_frame_length_error_as_a_shorter_frame` | The limit of the guarantee for an error in the Frame Length field (`hz:HZ-TC-01`) | TC-021, VER-064 |
| `test:crates/aldis-uslp/tests/bit_flips.rs::a_double_bit_error_32767_bits_apart_passes_the_fecf` | The limit of the guarantee above 4 095 octets (`hz:HZ-X-06`) | VER-064 |

### AC-5.6 The sequence-controlled service delivers in order and without duplicates, or reports

**Claim.** Over a link with loss, duplication, reordering, and delay of Type-AD frames and CLCWs, and with the Type-BC frames in order (`asm:ASM-18`, `hz:HZ-COP1-18`), the AD service of FOP-1 and FARM-1 delivers every frame once and in order, or FOP-1 raises an Alert, or (under Timeout Type 1) the transfer is suspended; a CLCW that is invalid or out of range, and a frame outside the window, are reported and never accepted silently (232.1 §2.2.1; an Alert ends the guarantee, §4.3.2).

| Evidence | Shows | Requirements |
|---|---|---|
| `test:crates/aldis-cop1/tests/closed_loop.rs::any_link_gives_the_service_guarantee_or_an_alert`; `test:crates/aldis-cop1/tests/closed_loop.rs::duplicated_frames_and_reports_deliver_once`; `test:crates/aldis-cop1/tests/closed_loop.rs::forward_reordering_is_repaired`; `test:crates/aldis-cop1/tests/closed_loop.rs::corruption_is_discarded_by_the_fecf_and_repaired`; `test:crates/aldis-cop1/tests/closed_loop.rs::a_dead_forward_link_ends_in_an_alert_at_the_transmission_limit` | The closed loop asserts in-order, complete, duplicate-free delivery or an Alert in every scenario, including the generated links; the scenarios send no Type-BC frame | COP-040 |
| `vector:cop1-fop1-clcw-pattern-type`; `vector:cop1-fop1-clcw-pattern-cop`; `vector:cop1-fop1-nr-invalid-wrap`; `vector:cop1-fop1-s1-e13`; `vector:cop1-fop1-s1-e14`; `test:crates/aldis-cop1/tests/closed_loop.rs::reordered_reports_complete_or_alert` | An invalid bit pattern, an N(R) outside NN(R) to V(S), and a Lockout in a CLCW are an Alert, not a silent acceptance; a reordered report completes or alerts | COP-012, COP-017 |
| `vector:cop1-farm1-s1-e3`; `vector:cop1-farm1-s1-e4`; `vector:cop1-farm1-s1-e5`; `vector:cop1-farm1-wait-to-lockout`; `test:crates/aldis-cop1/tests/farm1_properties.rs::window_classification_matches_the_inequalities` | A frame in the positive window, the negative window, or the lockout area is discarded and reported (a gap sets Retransmit, a lockout sets Lockout), and never delivered | COP-030, COP-004 |
| `test:crates/aldis-cop1/tests/farm1.rs::lockout_is_left_only_by_unlock`; `test:crates/aldis-cop1/tests/closed_loop.rs::a_receiver_that_holds_its_buffer_waits_and_resumes` | Wait and Lockout are visible in the CLCW and left only by the buffer release and the Unlock command | COP-033, COP-036 |

## AC-6 The C ABI keeps AC-1 to AC-5

**Claim.** A C caller gets the same results as a Rust caller, and a misuse of the C ABI within CABI-022's reach is an error, not undefined behaviour.

**Strategy.** Agreement with the Rust crates on every vector and on fuzzed input (AC-6.1); the argument checks of every function (AC-6.2); the library's freestanding build (AC-6.3). The `unsafe` code of `aldis-ffi` is outside AC-2.2's lint argument, so AC-6.2 and the review of every `// SAFETY:` comment carry it. Hazard: `hz:HZ-X-05`.

### AC-6.1 The C ABI agrees with the Rust API

**Claim.** Every live vector case passes through the C ABI, and on arbitrary input the C ABI decoders agree with the Rust decoders field by field.

| Evidence | Shows | Requirements |
|---|---|---|
| `test:crates/aldis-ffi/tests/vectors.rs::tm_invalid`; `test:crates/aldis-ffi/tests/vectors.rs::tc_validation_failures`; `test:crates/aldis-ffi/tests/vectors.rs::aos_invalid`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_validation_failures` | The vector files through the `extern "C"` functions with the same exact comparison (M2 plan D4), for example these four; `cargo xtask consistency` requires every file of the areas with a C ABI | VER-021, CABI-010, CABI-020, CABI-021 |
| `fuzz:ffi_decode`; `record:VER-022` | The C ABI decoders, validators, and delimiters agree with the Rust ones on fuzzed octets | VER-022, CABI-010 |
| `check:header`; `check:header` | The generated header matches the library and compiles as C99 and C++11 | CABI-001, CABI-002, CABI-003 |
| `record:CABI-010` | Only C-representable types cross the ABI | CABI-010, CABI-011 |
| `test:crates/aldis-ffi/tests/vectors.rs::fop1_table`; `test:crates/aldis-ffi/tests/vectors.rs::fop1_ad_service`; `test:crates/aldis-ffi/tests/vectors.rs::fop1_window_timer`; `test:crates/aldis-ffi/tests/vectors.rs::fop1_wrap_directives`; `test:crates/aldis-ffi/tests/vectors.rs::fop1_clcw`; `test:crates/aldis-ffi/tests/vectors.rs::farm1_table`; `test:crates/aldis-ffi/tests/vectors.rs::farm1_window`; `test:crates/aldis-ffi/tests/vectors.rs::farm1_flow_clcw` | The eight COP-1 scenario files run through the FOP-1 and FARM-1 handles of the C ABI, step by step, with the actions, the state, and the deadline compared; the runner skips the 52 `table_event` scenarios of `fop1_table.json` (`df:DF-20`) | VER-021, CABI-040, COP-005 |

### AC-6.2 Misuse within CABI-022's reach is an error

**Claim.** Null pointers, inconsistent lengths, wrong struct sizes, and bad helper storage are reported as errors.

| Evidence | Shows | Requirements |
|---|---|---|
| `test:crates/aldis-ffi/tests/tc.rs::encode_arguments_are_checked`; `test:crates/aldis-ffi/tests/tm.rs::encode_arguments_are_checked`; `test:crates/aldis-ffi/tests/uslp.rs::content_and_fields_are_checked_as_arguments`; `test:crates/aldis-ffi/tests/tm.rs::vc_arrays_are_c_arrays`; `test:crates/aldis-ffi/tests/uslp.rs::vc_and_map_arrays_are_c_arrays` | Pointer and length checks of the frame functions | CABI-022 |
| `check:header` | The capacities of the helper storage, generated and checked | CABI-030 |
| `test:crates/aldis-ffi/tests/engines.rs::storage_is_checked_and_tagged`; `test:crates/aldis-ffi/tests/engines.rs::configurations_are_checked_once`; `test:crates/aldis-ffi/tests/engines.rs::a_fop1_step_checks_every_argument`; `test:crates/aldis-ffi/tests/engines.rs::a_farm1_step_checks_every_argument`; `test:crates/aldis-ffi/tests/engines.rs::capacities_are_published_and_match_the_engines` | The FOP-1 and FARM-1 handles: storage that is null, misaligned, uninitialized, copied, foreign, or released is an error; the configuration is checked once at `init`; every step argument and every overlap of the buffers is checked | CABI-022, CABI-030, CABI-031, CABI-032 |

### AC-6.3 The C ABI library is freestanding

**Claim.** The static library builds for bare metal without `std`, a heap, threads, or a C runtime.

| Evidence | Shows | Requirements |
|---|---|---|
| `record:CABI-004`; `check:no-std-thumbv7em-none-eabihf` | The bare-metal archive and its imports | CABI-004 |

## AC-7 The evidence is trustworthy

**Claim.** The evidence above is what it says: produced from the commit it describes, by checks that cannot be skipped unnoticed, reviewed, and kept current.

**Strategy.** By the parts of the process that produce and judge the evidence: the merge gate and its records (AC-7.1), the review (AC-7.2), vectors and models independent of the implementation (AC-7.3), the recorded decisions (AC-7.4), the measures of test adequacy (AC-7.5), and the independent nightly run (AC-7.6).

### AC-7.1 Every merged commit carries a passing record of every gating check

**Claim.** No pull request merges without a published record, for its own tree, in which every gating check passed or was soundly skipped or reused.

| Evidence | Shows | Requirements |
|---|---|---|
| `record:VER-056`; `record:VER-057`; `record:VER-058` | The run, its pinned image, the input rule for skips and reuses and its re-derivation, the record, and the branch protection that requires it; skips rest on complete input lists (`df:DF-12`); records are unsigned (`df:DF-5`) | VER-056, VER-057, VER-058 |
| `record:VER-060` | A run on a GitHub-hosted runner meets the same rule | VER-060 |

### AC-7.2 Changes are reviewed by an agent other than their author

**Claim.** A work-package pull request is reviewed by an agent session other than the one that wrote it, against the Blue Book and the requirements, citing the code, and the maintainer decides its merge. Process and documentation pull requests may merge unreviewed, and reviewer and author share one account and one model (`df:DF-16`).

| Evidence | Shows | Requirements |
|---|---|---|
| `file:AGENTS.md`; `file:docs/process/decision-rights.md` | The review rules (what a reviewer reads, the citation rule, every suppression read) and who decides each finding | VER-062 |
| `file:docs/verification/reviews/m2-vectors.md` | The second review of the M2 frame vectors, hand-derived with worked notes, and its findings resolved | TV-020 |

### AC-7.3 Vectors and models are independent of the code they check

**Claim.** Expected values do not come from the implementation.

| Evidence | Shows | Requirements |
|---|---|---|
| `check:spec` | Every independent model a vector file names runs and agrees, and models are written blind | TV-023 |
| `record:TV-021` | Released vectors are never edited | TV-021 |
| `file:docs/verification/cross-checks.md` | The independent implementations searched and run; none yet for the frames (`df:DF-9`) | VER-040 |

### AC-7.4 Decisions are taken by the right role and recorded

**Claim.** Every reading the Blue Book does not settle, and every scope and convention decision, was the maintainer's and is in the log.

| Evidence | Shows | Requirements |
|---|---|---|
| `record:VER-062` | The decision rights, inspected when a pull request changes them or records a maintainer decision | VER-062 |

### AC-7.5 The tests are adequate by measure

**Claim.** Every requirement is traced, every region and branch of Layers 0–2 is covered, and surviving mutants are triaged.

| Evidence | Shows | Requirements |
|---|---|---|
| `check:spec`; `ci:trace --scope m2 --strict` | The trace of the M2 scope, strict at the exit | VER-020 |
| `check:coverage`; `check:coverage` | 100 % regions, functions, lines, and branches, with no waiver; MC/DC is not measured (`df:DF-14`) | VER-024 |
| `record:VER-025`; `job:mutants`; `file:docs/verification/mutants-triage.md` | Mutation testing every night and its triage | VER-025 |

### AC-7.6 An independent run repeats every check each night

**Claim.** Every check runs again each night on `main`, on GitHub's machines, and a failure opens an issue.

| Evidence | Shows | Requirements |
|---|---|---|
| `record:VER-059`; `job:verify` | The nightly run of every check through `cargo xtask verify` on both Linux architectures (`df:DF-8` closed) | VER-059 |

## Assumptions and context

| ID | Assumption | Basis | Integrator obligation |
|---|---|---|---|
| CTX-1 | The case covers Milestones 2 and 3: the library crates of AC-0 and their C ABI, including the FOP-1 and FARM-1 handles. It does not cover what the plans leave out (CMN-024; API-074 and the engines' security hooks; USLP-011 and the LNK requirements; the C ABI handles of the link engines; the USLP form of FOP-1 in the C ABI; CABI-041, CABI-050, CABI-060, CABI-070…072; VER-026 and VER-050's full matrix; synchronization and channel coding; the authentication of COP-1 control commands) | [M2 plan](../milestones/m2-frame-codecs.md) §4, "Not in M2"; [M3 plan](../milestones/m3-cop1.md) | — |
| CTX-2 | Aldis is a library. The mission's hazard analysis owns mission effects; this case argues the library's contract | [hazards.md](hazards.md) §1.1 | Take [hazards.md](hazards.md) into the mission's own analysis |
| ASM-1 | Memory that holds Aldis state between calls is protected against single-event upsets, or does not change at run time | GEN-015, [hardware assumptions](../hardware-assumptions.md) §1 | EDAC with scrubbing; configurations in `const` context and read-only memory (RS-012); an uncorrectable error as a reset ([hardware assumptions](../hardware-assumptions.md) §3). The state of a FOP-1 or FARM-1 changes on every call and lasts the mission: place it in protected memory, and restart an engine from its initial state (Initiate AD, Unlock) after a reset (`hz:HZ-COP1-15`) |
| ASM-2 | A receiver runs the receiving checks the codec provides | TC-020, TC-021, TM-011, USLP-014, USLP-015, API-011 | TC and USLP over TC Synchronization and Channel Coding: delimit each clean data unit with `FrameDelimiter` and pass every frame to `validate_frame`, given exactly the frame's octets where its boundary is known independently of the Frame Length field (`hz:HZ-TC-01`). TM: call `validate`, or decode with `decode_with` (`hz:HZ-TM-01`) |
| ASM-3 | Each context that calls Aldis has the stack the published table gives for the functions it calls | VER-011 | Add the worst case of `target/spec/stack-<target>.md` to the caller's own frames and to interrupt use |
| ASM-4 | The synchronization and channel coding sublayer below passes only data units it found free of errors, and the FECF is configured wherever a frame error that the code misses would be unacceptable | 232.0 §4.4.9.2.3; 732.1 §4.3.11.2.1; the FECF a managed parameter of every frame standard | Configure the FECF (and, for AOS, the FHEC) unless the mission's analysis accepts the residuals of `hz:HZ-TC-02`, `hz:HZ-TM-01`, `hz:HZ-AOS-01`, and `hz:HZ-USLP-03` |
| ASM-5 | Flight reception decodes in strict mode, the default, unless the mission accepts the lenient deviations; a lenient caller reads the anomaly set | API-030, API-032, GEN-043 | Decide the mode per channel; act on anomalies (`hz:HZ-X-08`) |
| ASM-6 | A C caller passes valid pointers and the true sizes of its buffers | CABI-022 | Test the integration under the platform's own tools; Aldis checks null pointers and length consistency only (`hz:HZ-X-05`) |
| ASM-7 | A USLP sender does not send non-truncated frames on a VC that has a Truncated Transfer Frame Length | 732.1 Annex D1.2 (notes; OI-48, `oi:OI-48`) | Configure truncated and non-truncated frames on different VCs; Aldis rejects a non-truncated frame on a VC with a truncated frame length, so a sender that mixes them loses those frames and is not misread (`hz:HZ-USLP-02`) |
| ASM-8 | The receiver's managed parameters equal the sender's: for every codec the edition, the frame length, the presence of the FHEC and the FECF, the Insert Zone length, per VC the Data Field Content, the presence of the OCF, the Security Header and Trailer lengths, the VCF Count Lengths (USLP), and the truncated frame lengths (USLP) | 132.0 §4.1.2.6; 232.0 §5; 732.0 §4.1.2.6.2–4, §4.1.3.4, §4.1.4.1.4, §4.1.5.2, Table 6-1; 732.1 Tables 5-1 to 5-4 (a managed parameter is static through a Mission Phase); `proposal:P-AOS-01`, `proposal:P-TC-05`, `proposal:P-USLP-09` | Take both sides from one managed-parameter database and decode a known frame of each channel before the phase starts; change them only at a phase change. No frame field says which layout a frame has, and the FECF covers any layout, so Aldis cannot detect a mismatch (`hz:HZ-AOS-02`, `hz:HZ-AOS-08`, `hz:HZ-AOS-09`, `hz:HZ-TC-07`, `hz:HZ-USLP-06`) |
| ASM-9 | The caller of a writer supplies the First Header Pointer, the frame counts, and the data of every frame correctly, keeps one `OidGenerator` per physical channel for the mission phase, fills the security fields of a frame before the FECF is written, and finishes every frame it begins | TM-009, TM-010, CMN-021, API-023, API-072, API-074; `proposal:P-TM-03`, `proposal:P-USLP-09` | Take the pointer and counts from the multiplexing engine (M3); never reset the generator except at a device start-up; use the two-phase writer, never `encode`, for an SDLS VC; prefer the one-shot `encode` where no step can be skipped (`hz:HZ-TM-09`, `hz:HZ-TM-10`, `hz:HZ-TM-12`, `hz:HZ-USLP-12`, `hz:HZ-USLP-16`) |
| ASM-10 | A consumer of a decoded CLCW other than FOP-1 checks that COP in Effect is COP-1 before it uses the Report Value or the flags; FOP-1 treats any other value as an invalid pattern (E15, `hz:HZ-COP1-02`) | TC-030, API-031 (the listed exception); 232.0 §4.2.1.5.2; `proposal:P-TC-04` | Check `cop_in_effect` against the COP-1 value of COP in Effect after the CLCW decoder, the CLCW report decoder, and `sym:aldis_clcw_decode` (`hz:HZ-TC-11`) |
| ASM-11 | A truncated USLP frame has no FECF, so the integrator protects it by other means | 732.1 Annex D1.2; `proposal:P-USLP-09` | Rely on the SDLS Security Trailer, or accept the residual of `hz:HZ-USLP-04` |
| ASM-12 | Fill is added before randomization on a link that randomizes, or a truncated frame length of 6 octets is not configured on such a link | 732.1 §4.3.11.2.4 note 2; `proposal:P-USLP-08` | Choose one of the two for each link: the sending end adds Fill Data before randomization, so that the receiver sees `01010101`, or the mission gives no VC a truncated frame length of 6 (`hz:HZ-USLP-05`) |
| ASM-13 | A C integrator compares `sym:aldis_abi_version` with the header's `ALDIS_ABI_VERSION` at start-up, and does not use a library whose version differs | CABI-060, CABI-021 | Refuse to start on a difference. A header and a library of different versions disagree on struct layouts (`sym:aldis_error_detail_t`, the configuration structs and their arrays), which no call can detect (`hz:HZ-X-05`, `hz:HZ-TC-14`) |
| ASM-14 | A Rust caller of the FOP-1 split step keeps one `Fop1Store` for each engine, zeroed when the engine is built, and commits each `Fop1Decision` once, to the engine and the store it was made for, before the next `decide` | API-058, API-060, COP-011, RS-022; `proposal:P-COP1-02`, `proposal:P-COP1-03` | Use the composed form where it exists; otherwise keep the pair (engine, store) as one object and never decide on one while a decision of it is pending. A mismatch gives wrong frames, never a panic (`hz:HZ-COP1-14`). The C ABI composes the two calls and is not exposed |
| ASM-15 | The caller routes each CLCW to the FOP-1 of its Virtual Channel and spacecraft, and each validated frame to the FARM-1 of its Virtual Channel, and the managed parameters of the two ends are compatible: K at most the PW of the peer FARM-1 (the note to Table 7-1 in 232.1 §7.2), the same window and retransmission mode (Table 7-2), T1_Initial above the loop delay, and a V(R) that matches the V(S) at start-up | TC-021, TC-023, COP-001, COP-002, COP-006, OI-58, OI-59; 232.1 §7.2, §7.3; `proposal:P-COP1-01`, `proposal:P-COP1-03` | Take both ends from one managed-parameter database. Route a CLCW by the Spacecraft ID and the VCID of the frame that carried it (the CLCW has no Spacecraft ID), and a frame by its VCID after the Frame Validation Checks; one FOP-1 and one FARM-1 per Virtual Channel. A FARM-1 does not compare the frame's VCID (`hz:HZ-COP1-01`, `hz:HZ-COP1-03`, `hz:HZ-COP1-04`) |
| ASM-16 | The caller of a FOP-1 supplies what the sans-I/O contract leaves to it: a monotonic `now`, a `Timeout` input at each reported deadline, a `TransmitDone` for every Transmit action, the CLCW of the return link, and a reaction to every Alert and Suspend notification | GEN-030, API-053, API-054, API-055, API-083, COP-013, COP-017, COP-040 | Read `next_deadline` after every step and arm a timer for it; answer each Transmit Request, accepted or rejected, before the next one of its kind (at most one is outstanding per kind); surface an Alert to the operators, and decide Resume under Timeout Type 1; choose the Transmission_Limit and T1_Initial for the mission (`hz:HZ-COP1-06`, `hz:HZ-COP1-09`) |
| ASM-17 | The caller of a FARM-1 sets `buffer_available` only when it has a buffer for the FDU, feeds `BufferReleased` when space returns, copies a delivered FDU out of the output area before the next call, purges its buffers on an `Unlocked` action, and asks for the CLCW at the reporting period | COP-033, COP-035, COP-036, API-061; 232.1 §3.3.2, §6.3.2.2 note, §6.3.2.3 | Pass the true state of the delivery buffer: FARM-1 reports a frame accepted when it was told a buffer exists. Treat a Lockout or Wait in the CLCW as a service state to be cleared by the operators (`hz:HZ-COP1-03`, `hz:HZ-COP1-07`) |
| ASM-18 | No Type-BC frame (an Unlock or a Set V(R) Control Command) of a Virtual Channel arrives at its FARM-1 after a Type-AD frame that FOP-1 sent after it; copies of a Type-BC frame made by the sending end (retransmission, systematic retransmission) that arrive before that Type-AD frame are allowed | COP-002, COP-003, COP-036; 232.0 §2.3.3; 232.1 §2.2.1, §5.1.2, §5.1.9.3 (the Blue Book assumes an order-keeping link and does not state it) | Route a Virtual Channel over one path, or resequence it before FARM-1, or drop a Type-BC frame older than the last Type-AD frame accepted; do not replay recorded Type-BC frames. FARM-1 executes every Unlock and Set V(R) it receives: a stale Set V(R) sets V(R) back, a stale Unlock emits Unlocked (the caller purges, `asm:ASM-17`) and clears Wait and Lockout (`hz:HZ-COP1-18`). Decided by the maintainer, 2026-10-31: an obligation on the integrator, with no engine guard |

## Defeaters

What weakens the case now. A defeater is `open` until what it names is done; `cargo xtask assurance` fails on an open defeater whose OI is resolved, so that the list stays honest.

| ID | Defeater | Weakens | References | Status |
|---|---|---|---|---|
| DF-1 | The channel tables' stack use is decided (OI-45 option (d)): the budgets are 4 096 bytes (Rust) and 10 240 bytes (C ABI), documented in record:VER-011; integrators bound task stacks to hold them | `ac:AC-4.2` | `oi:OI-45`; `record:VER-011` | `closed` |
| DF-2 | The Kani proofs run only at night and are not gating; the method for proven requirements is open | `ac:AC-3.1` | `oi:OI-42`; `nightly-check:kani` | `open` |
| DF-3 | The one CPU-hour per fuzz target of VER-022 ran and passed on main (run 40437244324: 34 targets, 3 600 s each, 0 failures) | `ac:AC-2.3` | `record:VER-022`; `job:fuzz` | `closed` |
| DF-4 | The GEN-022/023 inspections of all crates changed in M2 (aldis-core, aldis-time, aldis-epp, aldis-spp, aldis-tm, aldis-tc, aldis-aos, aldis-uslp, and aldis-ffi) were approved at the M2 exit review; inspections retired 2026-10-28 in favor of static stack/call-graph analysis | `ac:AC-3.1`; `ac:AC-3.3` | `record:GEN-022` | `closed` |
| DF-5 | The verification records are git notes without a signature; the signed evidence of #107 is parked | `ac:AC-7.1` | `gh:#110` | `open` |
| DF-6 | The shipped `aldis-ffi` static library is built without LTO, so the link check of VER-010, made on the exerciser's LTO build, does not cover it as shipped; a panic that reaches its handler traps (OI-49, `proposal:P-X-02`, implemented) instead of spinning, and a check of its panic paths is `proposal:P-X-01` | `ac:AC-2.1` | `gh:#114`; `hz:HZ-X-02` | `open` |
| DF-7 | No check runs `aldis-ffi` under Miri, which GEN-071 requires; it was run once by hand in WP-0c; the C harness with sanitizers is M5's | `ac:AC-6.2` | `gh:#71` | `open` |
| DF-8 | The nightly run on main passed all checks (run 40437244324 on main 01e380f: verify on x86_64 and aarch64, coverage 100%, 34 fuzz targets 3 600 s, all 8 crates mutation tested with 0 timeouts, big-endian bare-metal, bit-flips, and stack sizes; #96 closed) | `ac:AC-7.6`; `ac:AC-7.5` | `gh:#96`; `job:verify` | `closed` |
| DF-9 | No independent implementation has cross-checked any frame codec yet (M2 plan D15); frame conformance rests on Blue Book material, hand derivations, blind models, and the second review | `ac:AC-1` | `file:docs/verification/cross-checks.md`; `oi:OI-32` | `open` |
| DF-10 | The link check covers the generic instantiations the exerciser makes; an integrator's other capacities are not linked; LEON3 is not gating (OI-08) | `ac:AC-2.1` | `record:VER-010`; `nightly-check:rust-sparc-leon3` | `open` |
| DF-11 | The stack bound leaves out interrupts and the caller's own frames | `ac:AC-4.2` | `record:VER-011` | `open` |
| DF-12 | Skipping and reusing checks is sound only if every check's input list is complete; a missing path goes unchecked until the nightly run | `ac:AC-7.1` | `record:VER-056` | `open` |
| DF-13 | GEN-031 and GEN-032 (no clocks, no I/O, no global state) had no verification record and were untraced; `record:GEN-031` traces them (`proposal:P-X-03`, accepted); no check searches for a new global or platform call in library code | `ac:AC-3.3` | `hz:HZ-X-03`; `record:GEN-031` | `closed` |
| DF-14 | MC/DC is not measured | `ac:AC-7.5` | `oi:OI-09`; `check:coverage` | `open` |
| DF-15 | The hazard analysis has no TODO category in any codec file (`cargo xtask assurance --strict`), and the independent M2 exit review (VER-065) confirmed that all identified failure modes map into the 12 categories of hazards.md §1.2 with no unrepresented categories found | `ac:AC-0` | `file:docs/verification/hazards.md` | `closed` |
| DF-16 | The review of AC-7.2 is weaker than independent human review: reviews are written by agents posting under the maintainer's account, the same model as the author; branch protection requires no approval, so the review is not enforced; of the 60 pull requests merged up to #190, 29 had no review, most of them process pull requests, among them product pull requests #174, #189, and #190 | `ac:AC-7.2` | `file:docs/process/review.md`; `gh:#174` | `open` |
| DF-17 | The fuzz target `cop1_events` of the COP-1 engines is written and not registered in the fuzz workspace (process freeze of M3), and the targets of VER-022 for the engines (`fop1_events`, `farm1_events`, `cop1_closed_loop`, `fop1_clcw`) are not yet written; no night run drives the engines with arbitrary input, only the random-sequence tests of every run do | `ac:AC-2.3` | `file:fuzz/fuzz_targets/cop1_events.rs`; `file:docs/milestones/m3-cop1.md` | `open` |
| DF-18 | The mutation testing of `aldis-cop1` and the triage of its survivors (VER-025) are not recorded yet; the coverage waivers of the crate are justified in the waiver file and read at review | `ac:AC-7.5` | `file:docs/verification/mutants-triage.md`; `file:coverage-waivers.toml` | `open` |
| DF-19 | The USLP form of FOP-1 has no C ABI form and no peer reading of the counts (OI-66, resolved as the narrowest reading): it is tested in the Rust crate against the project's own reading | `ac:AC-1.8` | `file:docs/standards/cop1.md`; `test:crates/aldis-cop1/tests/fop1_uslp.rs::the_frames_are_the_octets_worked_by_hand` | `open` |
| DF-20 | The COP-1 scenarios do not all run on both paths: FARM-1 has no Rust vector runner (its files run only through the C ABI), and the C runner skips every `table_event` scenario, 52 cases of `fop1_table.json`, which only the Rust runner executes | `ac:AC-1.8`; `ac:AC-6.1` | `file:crates/aldis-ffi/tests/engine/mod.rs`; `file:crates/aldis-cop1/tests/fop1_vectors.rs` | `open` |
