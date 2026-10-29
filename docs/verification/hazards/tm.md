# Hazards: TM Transfer Frame

Failure modes of `aldis-tm` and its C ABI (the TM functions), in the format of [hazards.md](../hazards.md) §1.7. Standard: [tm-frame.md](../../standards/tm-frame.md), 132.0-B-3. The failure modes common to all codecs (panics, loops, stack, the C ABI, the CRC-16) are in [hazards.md](../hazards.md) §3 and are not repeated here.

## Coverage

| Category | Hazards | State |
|---|---|---|
| `corrupt-accepted` | HZ-TM-02, HZ-TM-03 | done |
| `valid-rejected` | HZ-TM-05 | done |
| `misrouting` | HZ-TM-01 | done |
| `truncation` | HZ-TM-06 | done |
| `wrong-value` | HZ-TM-07 | done |
| `nonconformant-output` | HZ-TM-08, HZ-TM-09, HZ-TM-10 | done |
| `unbounded-time` | — | n/a: every loop of `aldis-tm` is bounded by the frame length, the eight VC slots, or a constant (`record:GEN-022`), and no call waits; TM adds no failure mode to `hz:HZ-X-03` |
| `memory` | HZ-TM-11 | done |
| `panic` | — | n/a: TM adds no panic path to `hz:HZ-X-02`; its decoders and writers are covered by the same link check, the fuzz targets `tm_decode` and `tm_roundtrip`, and the property tests, cited there and in `hz:HZ-TM-11` |
| `state-corruption` | HZ-TM-12 | done |
| `abi-misuse` | HZ-TM-13 | done |
| `hidden-deviation` | HZ-TM-04 | done |

## Hazards

### HZ-TM-01 A frame is delivered on another spacecraft or virtual channel

| | |
|---|---|
| Category | `misrouting` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A TM frame is accepted, and its view reports a Spacecraft ID or VCID other than the one it was sent with, or one the Master Channel does not configure |
| Cause | (1) A channel error in the Spacecraft ID (bits 2–11) or the VCID (bits 12–14) that the coding sublayer did not detect. (2) A caller that decodes with the TM frame decoder, which looks no channel up (`file:crates/aldis-tm/src/view.rs`: its errors are those of `decode_with` "except `UnknownChannel`"), and does not call `validate` (TM-011) |
| Mission effect | Telemetry of one virtual channel, or of another spacecraft, delivered to the consumer of another |
| Requirements | TM-005, TM-011, API-070, VER-064 |
| Evidence | `vector:tm-inv-val-001`; `vector:tm-inv-val-002`; `vector:tm-sec-dec-004`; `vector:tm-sec-dec-005`; `vector:tm-inv-crc-002`; `test:crates/aldis-tm/src/validate.rs::channel_and_length_mismatches`; `test:crates/aldis-tm/tests/vectors.rs::invalid`; `test:crates/aldis-tm/tests/vectors.rs::security_layout`; `test:crates/aldis-tm/tests/bit_flips.rs::every_single_bit_error_is_rejected`; `test:crates/aldis-tm/tests/bit_flips.rs::every_double_bit_error_of_the_short_frames_is_rejected`; `test:crates/aldis-tm/tests/bit_flips.rs::sampled_double_bit_errors_of_the_long_frames_are_rejected`; `nightly-test:crates/aldis-tm/tests/bit_flips.rs::every_double_bit_error_of_the_longest_frame_is_rejected`; `job:bit-flips`; `fuzz:tm_decode` |
| Residual risk | `validate` checks the SCID and the VCID against the Master Channel (TM-011; `tm-inv-val-001`, `-002`), and `decode_with` and `decode_unchecked` look the channel up (`UnknownChannel`; `tm-sec-dec-004`, `-005`). `decode` alone checks neither, so the check is the caller's to make. An error that turns one configured channel into another passes both; only the FECF detects it, and it is checked before any field is read (TM-005; `tm-inv-crc-002`). With the FECF, every single-bit error of a TM frame, and every double-bit error of one of at most 4 095 octets, is rejected (VER-064); a TM channel longer than 4 095 octets is refused when configured (OI-51, `hz:HZ-TM-04`). Without it (an optional managed parameter of the Physical Channel, TM-001, `hz:HZ-TM-03`), such an error is not detected by Aldis; only the coding sublayer protects the frame |
| Assumptions | `asm:ASM-2`; `asm:ASM-4` |
| Proposals | — |
| Claims | `ac:AC-1.1`; `ac:AC-5.5` |

### HZ-TM-02 A frame with a wrong FECF is accepted by the diagnostic decoder

| | |
|---|---|
| Category | `corrupt-accepted` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The unchecked TM decoder, or `sym:aldis_tm_decode_unchecked` and `sym:aldis_tm_decode_unchecked_with` through the C ABI, return a view of a frame whose FECF does not match, and the view carries no mark that the check did not run |
| Cause | A receiving program decodes operational frames with the function TM-005 provides "for diagnostics of frames that failed" the check (`file:crates/aldis-tm/src/view.rs`), or falls back to it after a `CrcMismatch`. The view has the same type as one from `decode_with`: its fields are the frame's octets, header, layout, status, and anomalies (`file:crates/aldis-tm/src/view.rs`), and `fecf()` returns the received value, not a verdict |
| Mission effect | Corrupt telemetry, with a possibly wrong channel, counts, or data, passed up as valid |
| Requirements | TM-005, CMN-003 |
| Evidence | `vector:tm-fof-dec-006`; `vector:tm-inv-crc-001`; `vector:tm-inv-crc-002`; `test:crates/aldis-tm/src/view.rs::length_and_fecf_are_checked_first`; `test:crates/aldis-tm/tests/vectors.rs::fsh_ocf_fecf`; `test:crates/aldis-tm/tests/vectors.rs::invalid`; `test:crates/aldis-ffi/tests/tm.rs::the_with_functions_take_the_vc_security_layout` |
| Residual risk | `decode_unchecked` is a separate function that the requirement keeps out of the default path (TM-005): `decode` and `decode_with` check the FECF first, before any field is read (`tm-inv-crc-001`, `-002`), and `tm-fof-dec-006` pins that only the unchecked decoder accepts the corrupted frame. Aldis cannot stop a caller from choosing the unchecked function for operations. The frame still passes the other decode checks (length, version, Secondary Header, First Header Pointer), which catch few channel errors. Receivers use `decode` or `decode_with`, never `decode_unchecked`, for flight data |
| Assumptions | `asm:ASM-2` |
| Proposals | — |
| Claims | `ac:AC-1.1`; `ac:AC-5.5` |

### HZ-TM-03 A frame is accepted unchecked because no FECF is configured, or the configuration is wrong

| | |
|---|---|
| Category | `corrupt-accepted` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A frame with bit errors is accepted because the Physical Channel is configured without the FECF, or is configured without it while the sender appends one |
| Cause | (1) The mission chooses no FECF: it is optional and set by management (132.0 §4.1.6.1.2), and 132.0 §4.1.6.1 note 2 says whether to use it depends on the mission's data-quality needs and the channel coding. (2) The receiver's FECF setting differs from the sender's. With the FECF set where the sender has none, nearly every frame fails `CrcMismatch` (an availability loss, reported). With it unset where the sender appends one, nothing is checked, and the fields that end the frame are placed two octets off (the data field or the OCF includes the FECF) |
| Mission effect | Corrupt telemetry accepted as valid; with a wrong setting, two octets of CRC read as data |
| Requirements | TM-001, TM-005, API-012 |
| Evidence | `vector:tm-min-dec-001`; `vector:tm-min-dec-003`; `vector:tm-min-cfg-001`; `vector:tm-fof-dec-001`; `vector:tm-inv-crc-001`; `test:crates/aldis-tm/src/config.rs::physical_channel_minimum`; `test:crates/aldis-tm/tests/restricted.rs::physical_channel_frame_length_has_its_minimum`; `test:crates/aldis-tm/tests/bit_flips.rs::every_single_bit_error_is_rejected`; `test:crates/aldis-tm/tests/vectors.rs::minimal` |
| Residual risk | The decoder applies the configured presence exactly (TM-001, API-012): the FECF is verified when configured, before any field is read (`tm-inv-crc-001`), and not looked for otherwise (`tm-min-dec-001`). It cannot tell from the octets which setting is right. Without the FECF, Aldis can check only the length, the version, the Secondary Header, the First Header Pointer, and, in `validate`, the flags and the channel; an error in the data field, the OCF, a frame count, or a field that turns one configured value into another passes. The Blue Book leaves the choice to management (132.0 §4.1.6.1.2), so this is its residual, not Aldis's to remove |
| Assumptions | `asm:ASM-4` |
| Proposals | — |
| Claims | `ac:AC-1.1`; `ac:AC-5.5` |

### HZ-TM-04 A frame longer than the TM maximum of 2 048 octets is accepted without a report

| | |
|---|---|
| Category | `hidden-deviation` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The TM physical channel constructor accepts a frame length from the minimum up to 4 095 octets (`MAX_FRAME_LEN`, `file:crates/aldis-tm/src/config.rs`; OI-51) and rejects a longer one with `InvalidConfig` (`frame_len`). 132.0-B-3 states no maximum, so Aldis decodes and encodes TM frames longer than 2 048 octets, the maximum of 131.0-B-6 §11.1 and §11.3, with no error and no anomaly flag. Before OI-51 any length was accepted, and above 4 095 octets the CRC-16 does not detect every double-bit error, so the guarantee of VER-064 that `hz:HZ-TM-01` and `ac:AC-5.5` rest on did not hold for such a channel; it now holds for every accepted TM channel |
| Cause | A configuration with a frame length above the maximum of 131.0-B-6 §11.1 and §11.3 for the TM Space Data Link Protocol (the 11-bit First Header Pointer supports 2 048 octets); the project accepts them, up to 4 095 octets, so that the First Header Pointer offsets 2045 and 2046 are testable (`docs/standards/tm-frame.md` §6; `tm-fhp-enc-005`) |
| Mission effect | In a frame of 2 049 to 4 095 octets, a first Packet at offset 2046 or above cannot be signalled by the pointer, and the writer rejects it (`tm-fhp-enc-err-002`). A mission that configures such a channel sends frames that 131.0 does not allow below the frame layer |
| Requirements | TM-001, TM-005, API-012, VER-064 |
| Evidence | `file:docs/verification/derivations/crc16-bit-flips.md`; `test:crates/aldis-aos/tests/bit_flips.rs::a_double_bit_error_32767_bits_apart_passes_the_fecf`; `test:crates/aldis-tm/tests/bit_flips.rs::every_single_bit_error_is_rejected`; `test:crates/aldis-tm/tests/restricted.rs::physical_channel_frame_length_has_its_minimum`; `vector:tm-fhp-enc-005`; `vector:tm-fhp-enc-err-002`; `vector:tm-inv-cfg-010`; `vector:tm-inv-cfg-011`; `vector:tm-inv-cfg-012`; `vector:tm-inv-cfg-013`; `vector:tm-inv-cfg-014`; `vector:tm-inv-cfg-015`; `test:crates/aldis-ffi/tests/tm.rs::config_check_refuses_a_frame_length_above_4095` |
| Residual risk | The TM bit-flip suite covers frames up to 2 048 octets only (`file:crates/aldis-tm/tests/bit_flips.rs`, `LONGEST`, and the compile-time assertion that it lies within the 4 095 octets of the CRC bound); frames of 2 049 to 4 095 octets rest on the derivation of the bound (`file:docs/verification/derivations/crc16-bit-flips.md`). OI-51 (resolved, maintainer, 2026-10-25) caps the configured length at 4 095 octets, not at the 2 048 of 131.0, so a frame between the two is accepted knowingly: it is not a violation of 132.0, whose Blue Book states no maximum, and VER-064 holds for it. The failure mode that remains is the Blue Book's gap and the maintainer's reading: a mission that configures a TM channel above 2 048 octets takes the longer-frame risks of `hz:HZ-X-06` for TM, and a mission that wants 131.0's maximum enforced has to configure it. `hz:HZ-TM-01` is corrected to say so |
| Assumptions | `asm:ASM-4` |
| Proposals | `proposal:P-TM-01` |
| Claims | `ac:AC-1.1`; `ac:AC-5.5` |

### HZ-TM-05 A conformant frame is rejected by a project rule

| | |
|---|---|
| Category | `valid-rejected` |
| Status | `residual` |
| Severity | S3 |
| Failure mode | A frame that a sender built with managed parameters of 132.0-B-3 is rejected with an error, or a channel configuration it needs is refused |
| Cause | (1) A frame with no Transfer Frame Data Field octet (a Secondary Header, Security Fields, OCF, and FECF that fill the frame) is rejected: a project rule (CMN-041; `docs/standards/tm-frame.md` §6: the Blue Book states no minimum). Decode fails `Malformed` (`frame_data_field`), and the configuration `InvalidConfig`. (2) With Synchronization Flag `0`, a First Header Pointer past the data field that is neither `0x7FE` nor `0x7FF` is `Malformed` in both modes (TM-008, `oi:OI-33`): 132.0 §4.1.2.7.6.3 numbers the octets of the Data Field, so a pointer outside it names no octet. (3) A Secondary Header Length field of 0, a header of the ID octet alone, is rejected `InvalidField` (`fsh_len`): 132.0 §4.1.3.1.3 b requires 1 to 63 data octets, but the text of TM-003 does not list this rejection. (4) In strict mode, Packet Order Flag `1` or a Segment Length ID other than `11` with Synchronization Flag `0` is rejected (TM-007): 132.0 §4.1.2.7.5.2 sets the identifier, and §4.1.2.7.4 reserves the order flag in a note; lenient mode accepts both and flags them |
| Mission effect | The frame, and the telemetry in it, is lost with a reported error |
| Requirements | TM-003, TM-007, TM-008, CMN-041 |
| Evidence | `vector:tm-inv-fsh-002`; `vector:tm-inv-fsh-004`; `vector:tm-inv-ocf-001`; `vector:tm-inv-cfg-001`; `vector:tm-inv-cfg-008`; `vector:tm-min-dec-003`; `vector:tm-fhp-err-001`; `vector:tm-fhp-err-003`; `vector:tm-inv-pof-001`; `vector:tm-inv-pof-002`; `test:crates/aldis-tm/tests/restricted.rs::packet_pointer_classifies_every_raw_value`; `test:crates/aldis-tm/tests/restricted.rs::fsh_len_is_two_to_sixty_four`; `test:crates/aldis-tm/src/view.rs::ocf_flag_can_leave_no_data_field`; `test:crates/aldis-tm/src/view.rs::secondary_header_errors`; `test:crates/aldis-core/src/layout.rs::rejects_empty_data_field`; `test:crates/aldis-tm/tests/vectors.rs::invalid`; `test:crates/aldis-tm/tests/vectors.rs::fhp_values`; `fuzz:tm_decode` |
| Residual risk | Rejections (2) to (4) follow the Blue Book's text, and each is pinned by a vector. Rejection (1) is the project's: a mission whose frames leave no data octet cannot use Aldis, and no such mission is known. Rejection (3) is right, but TM-003 does not say it (`proposal:P-TM-02`); the vector `tm-inv-fsh-002` and the FSH length constructor (2 to 64 octets) already pin it. All are reported as errors with the field named, so the loss is visible (S3) |
| Assumptions | — |
| Proposals | `proposal:P-TM-02` |
| Claims | `ac:AC-1.1`; `ac:AC-5.1` |

### HZ-TM-06 A frame is decoded from octets that are not exactly one frame

| | |
|---|---|
| Category | `truncation` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The decoder accepts a frame cut at the wrong place: a frame whose octets are shifted against the real frame boundary, or the front of a longer input; or an encoder writes a frame with a field shorter than its configured length |
| Cause | (1) The coding sublayer loses or gains frame synchronisation, so the octets handed up are a slice of the stream that is the right length but not a frame. A TM frame has no length field to resynchronise on: its length is a managed parameter (132.0 §4.1.1.2). (2) A caller splits a stream with `peek_length`, which returns the configured length for any prefix (API-013, M2 plan D14) and so cannot detect a lost frame. (3) A caller passes `encode` a field of another length |
| Mission effect | A frame of the right length with the wrong content, or the loss of the tail of a frame, delivered as telemetry |
| Requirements | TM-001, API-012, API-013, API-022, TM-009 |
| Evidence | `vector:tm-inv-len-001`; `vector:tm-inv-len-002`; `vector:tm-inv-ver-003`; `vector:tm-min-peek-001`; `vector:tm-min-peek-002`; `vector:tm-inv-enc-004`; `vector:tm-inv-enc-005`; `vector:tm-inv-enc-008`; `vector:tm-inv-enc-010`; `test:crates/aldis-tm/src/view.rs::length_and_fecf_are_checked_first`; `test:crates/aldis-tm/src/view.rs::peek_length_returns_the_configured_length`; `test:crates/aldis-tm/src/writer.rs::encode_checks_every_length`; `test:crates/aldis-tm/tests/properties.rs::rejected_encode_leaves_the_buffer`; `test:crates/aldis-ffi/tests/tm.rs::encode_arguments_are_checked`; `fuzz:tm_decode` |
| Residual risk | The decoder accepts exactly the configured length and rejects any other with `LengthMismatch` (`tm-inv-len-001`, `-002`); the encoder requires each field at its configured length and writes nothing when the buffer is too small (`tm-inv-enc-004`, `-005`, `-010`). It cannot tell a frame from a shifted slice of the stream. A shifted slice of the right length is caught only by the FECF (`hz:HZ-X-06`, `hz:HZ-TM-04`), and without it not at all (`hz:HZ-TM-03`). Frame synchronisation is the coding sublayer's (131.0, outside Aldis, roadmap §7) |
| Assumptions | `asm:ASM-4` |
| Proposals | — |
| Claims | `ac:AC-1.1`; `ac:AC-4.3`; `ac:AC-5.5` |

### HZ-TM-07 A decoded view places the fields by the frame's own flags, not by the configuration

| | |
|---|---|
| Category | `wrong-value` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The view reports a Secondary Header, OCF, Security Header or Trailer, or Data Field that is not the one the sender put there, or reads a Packets frame as a `VCA_SDU` one, and the caller passes it up |
| Cause | (1) The view places the Secondary Header and the OCF by the frame's own flags, and compares nothing with the configuration; `validate` does that (TM-011; `file:crates/aldis-tm/src/view.rs`). A bit error in the OCF Flag (bit 15), the Secondary Header Flag (bit 32), or the Synchronization Flag (bit 33) that the coding sublayer did not detect shifts the fields. (2) the TM frame decoder uses no security layout, so on a VC that uses SDLS the Data Field includes the Security Header and Trailer, and the First Header Pointer is checked against that longer field (`tm-sec-dec-002`: a 32-octet data field where `tm-sec-dec-001` has 18). For the same reason it cannot apply the rule of OI-54 (an Only Idle Data frame on an SDLS VC is rejected in strict mode and flagged in lenient mode); only `decode_with` can. (3) `decode_with` is given the lookup of another configuration |
| Mission effect | The data field, the OCF (the CLCW, and so the state of the command link), or the Secondary Header is read from the wrong octets |
| Requirements | TM-003, TM-004, TM-006, TM-011, API-070, CMN-040 |
| Evidence | `vector:tm-sec-dec-001`; `vector:tm-sec-dec-002`; `vector:tm-sec-dec-003`; `vector:tm-sec-dec-006`; `vector:tm-sec-val-002`; `vector:tm-sec-val-003`; `vector:tm-inv-val-003`; `vector:tm-inv-val-004`; `vector:tm-inv-val-005`; `vector:tm-inv-val-006`; `vector:tm-inv-val-007`; `vector:tm-inv-val-008`; `vector:tm-fof-val-001`; `test:crates/aldis-tm/src/validate.rs::matching_frames_pass`; `test:crates/aldis-tm/src/validate.rs::flag_and_length_mismatches`; `test:crates/aldis-tm/src/validate.rs::trailer_mismatch`; `test:crates/aldis-tm/src/view.rs::secondary_header_and_ocf_follow_the_flags`; `test:crates/aldis-tm/src/view.rs::security_layout_places_the_data_field`; `test:crates/aldis-tm/tests/vectors.rs::security_layout`; `test:crates/aldis-ffi/tests/tm.rs::the_with_functions_take_the_vc_security_layout`; `fuzz:tm_decode`; `test:crates/aldis-tm/src/view.rs::only_idle_data_on_an_sdls_vc_is_refused_or_flagged`; `test:crates/aldis-tm/src/validate.rs::only_idle_data_on_an_sdls_vc_is_refused` |
| Residual risk | `validate` rejects a frame whose OCF Flag, Secondary Header Flag and length, or Synchronization Flag differs from the VC's, and a frame decoded with another security layout than the VC's (`LengthMismatch`, `security_header_len`, `security_trailer_len`; TM-011, API-070); `decode_with` and its lookup place the SDLS fields. A caller that uses `decode` on an SDLS channel, or skips `validate`, gets the view as the frame's own bits describe it, and Aldis cannot detect that. The FECF catches the bit errors of (1) when configured (`hz:HZ-TM-03`) |
| Assumptions | `asm:ASM-2`; `asm:ASM-4` |
| Proposals | — |
| Claims | `ac:AC-1.1`; `ac:AC-1.6`; `ac:AC-5.1` |

### HZ-TM-08 A writer builds an Only Idle Data frame on a virtual channel that uses SDLS

| | |
|---|---|
| Category | `nonconformant-output` |
| Status | `controlled` |
| Severity | S1 |
| Failure mode | A writer emits a frame with First Header Pointer `0x7FE`, an OID frame, on a VCID with a Security Header or Trailer, or on a `VCA_SDU` VC |
| Cause | A caller of `encode` or `begin` (or the C ABI encode call) with the status `Packets(OnlyIdleData)` on an SDLS VC; or `begin_oid` on one. Before the review of #117 only `begin_oid` checked it (`92374c4`). 132.0 §6.4.6.2 requires the multiplexing function that creates an OID frame to give it the identifier of a VC that does not use SDLS, and §4.1.4.6.3 requires a VC that transfers Packets |
| Mission effect | A non-conformant frame on the link: an OID frame whose identifier names a VC that uses SDLS |
| Requirements | TM-010, TM-009, TM-011, API-020, API-031, API-032 |
| Evidence | `vector:tm-oid-003`; `vector:tm-oid-006`; `vector:tm-oid-007`; `vector:tm-oid-008`; `vector:tm-oid-009`; `vector:tm-sec-enc-004`; `vector:tm-sec-enc-005`; `vector:tm-sec-enc-006`; `vector:tm-sec-enc-007`; `test:crates/aldis-tm/src/writer.rs::oid_frames_continue_the_sequence`; `test:crates/aldis-tm/tests/vectors.rs::oid_sequence`; `test:crates/aldis-tm/tests/vectors.rs::security_layout`; `test:crates/aldis-ffi/tests/vectors.rs::tm_oid_sequence`; `test:crates/aldis-ffi/tests/vectors.rs::tm_security_layout`; `test:crates/aldis-ffi/tests/tm.rs::encode_oid_takes_a_generator_handle`; `fuzz:tm_roundtrip`; `test:crates/aldis-tm/src/view.rs::only_idle_data_on_an_sdls_vc_is_refused_or_flagged`; `test:crates/aldis-tm/src/validate.rs::only_idle_data_on_an_sdls_vc_is_refused`; `test:crates/aldis-ffi/tests/tm.rs::an_only_idle_data_frame_on_an_sdls_vc_is_refused_or_flagged`; `test:crates/aldis-tm/tests/properties.rs::encode_then_decode_round_trips` |
| Residual risk | The writer plan constructor applies the rule to the status, whichever writer calls it (`file:crates/aldis-tm/src/writer.rs`), and `begin_oid` names the two checks in order: `sync_flag`, then `vc_id`. Nothing is written on error (`tm-oid-006`, `tm-sec-enc-005`). The decoder applies the rule too (OI-54): the TM decoder with a configuration and `decode_unchecked` return `InvalidField` naming `vc_id` in strict mode, and in lenient mode accept the frame with the anomaly `OnlyIdleDataOnSdlsVc` (bit 2; `file:crates/aldis-tm/src/view.rs`, last check of `parse`), and `validate` rejects the frame in both modes. The strict re-encode property holds for every frame the decoder accepts. The TM frame decoder has no channel table, reads every VC as one without SDLS (`hz:HZ-TM-07`), and cannot check; a caller with an SDLS VC uses `decode_with`. A lenient caller that ignores the anomaly accepts the frame (`hz:HZ-X-08`) |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-1.1`; `ac:AC-1.6` |

### HZ-TM-09 The data field of an OID frame does not continue the pseudo-noise sequence

| | |
|---|---|
| Category | `nonconformant-output` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The Idle Data of an Only Idle Data frame is not the next octets of the mandatory pseudo-noise sequence, which 132.0 §4.1.4.6.2.1 initialises at device start-up and does not restart |
| Cause | (1) The caller creates a new `OidGenerator`, or one per virtual channel, instead of one per physical channel for the mission phase (CMN-021). (2) The caller fills an OID frame built with `encode` or `begin` and `Packets(OnlyIdleData)` with data of its own: only `begin_oid` and `sym:aldis_tm_encode_oid` fill the field from a generator. |
| Mission effect | A frame that does not conform to 132.0 §4.1.4.6.2. Receivers discard the Idle Data (the note of CMN-021), so no telemetry is lost; a receiver that checks the sequence sees a restart or a break |
| Requirements | TM-010, CMN-020, CMN-021, CMN-022 |
| Evidence | `vector:tm-oid-001`; `vector:tm-oid-005`; `vector:oid-start-002`; `vector:oid-continuation-002`; `test:crates/aldis-tm/src/writer.rs::oid_frames_continue_the_sequence`; `test:crates/aldis-core/src/oid.rs::sequence_continues_across_fills`; `test:crates/aldis-core/src/oid.rs::both_forms_reproduce_annex_d`; `test:crates/aldis-ffi/tests/tm.rs::encode_oid_takes_a_generator_handle`; `fuzz:tm_roundtrip` |
| Residual risk | `begin_oid` advances the generator by exactly the data field, or not at all when it fails (`fuzz:tm_roundtrip`'s `oid_frame` check; GEN-042). The generator is the caller's, and the decoder is stateless, so it does not check the data (TM-010, CMN-021). Whether one generator serves the whole physical channel is the caller's, and the data of an OID frame built without `begin_oid` is never checked. No assumption row of the assurance case states these obligations (`proposal:P-TM-03`) |
| Assumptions | `asm:ASM-9` |
| Proposals | `proposal:P-TM-03` |
| Claims | `ac:AC-1.1` |

### HZ-TM-10 A writer is given a First Header Pointer, counts, or data that disagree with the packets in the frame

| | |
|---|---|
| Category | `nonconformant-output` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The frame is well-formed, and the FECF is right, but its First Header Pointer does not name the first Packet header that starts in the data field, or its Master or Virtual Channel Frame Count does not continue the sequence |
| Cause | The pointer and the counts are inputs of the writer (TM-009: counts are owned by the engines, [04](../../04-engines.md)). The writer checks the pointer against the data field (an offset past it is `OutOfRange`, and the reserved values `0x7FE` and `0x7FF` are not offsets) and against the VC's content, but does not read the packets in the data field, which are opaque octets |
| Mission effect | The receiver delimits the Packets at the wrong octet and loses or garbles them, until the next frame with a pointer it can use; a count gap is read as lost frames |
| Requirements | TM-008, TM-009, API-020, API-021 |
| Evidence | `vector:tm-fhp-enc-001`; `vector:tm-fhp-enc-002`; `vector:tm-fhp-enc-003`; `vector:tm-fhp-enc-004`; `vector:tm-fhp-enc-err-001`; `vector:tm-fhp-enc-err-002`; `vector:tm-fhp-enc-005`; `test:crates/aldis-tm/tests/restricted.rs::packet_offset_is_below_the_reserved_pointers`; `test:crates/aldis-tm/tests/restricted.rs::first_header_pointer_is_eleven_bits`; `test:crates/aldis-tm/src/writer.rs::offsets_stop_below_the_reserved_pointer_values`; `test:crates/aldis-tm/src/writer.rs::vca_sdu_fields_are_written_raw`; `test:crates/aldis-ffi/tests/tm.rs::packet_offsets_above_0x7fd_fail_as_the_encoder_checks_them`; `test:crates/aldis-tm/tests/properties.rs::encode_then_decode_round_trips`; `fuzz:tm_roundtrip` |
| Residual risk | Aldis writes exactly the fields it is given (API-020), derives the others (API-021), and rejects an impossible pointer. That the pointer and the counts are right is the caller's, and the M3 engines' where they own them; the codec has no packet parser to check it against. No assumption row of the assurance case states the obligation (`proposal:P-TM-03`) |
| Assumptions | `asm:ASM-9` |
| Proposals | `proposal:P-TM-03` |
| Claims | `ac:AC-1.1`; `ac:AC-1.6` |

### HZ-TM-11 An accessor or a writer reaches outside the frame, or the fields overlap

| | |
|---|---|
| Category | `memory` |
| Status | `controlled` |
| Severity | S1 |
| Failure mode | A view returns a sub-slice outside the input octets, or fields that overlap or leave gaps; or a writer's mutable accessor or `encode` writes outside the frame it was given |
| Cause | Lengths taken from untrusted octets (the Secondary Header Length of the ID octet) or from a configuration (security header and trailer, frame length), whose sum leaves no room, or overflows |
| Mission effect | A read or write of memory the call was not given (S1); in the C ABI, in the caller's process |
| Requirements | CMN-040, API-010, GEN-014, GEN-020, TM-006 |
| Evidence | `test:crates/aldis-core/src/layout.rs::tm_frame_with_everything`; `test:crates/aldis-core/src/layout.rs::rejects_empty_data_field`; `test:crates/aldis-tm/src/view.rs::security_layout_places_the_data_field`; `test:crates/aldis-tm/src/view.rs::secondary_header_errors`; `test:crates/aldis-tm/src/view.rs::ocf_flag_can_leave_no_data_field`; `test:crates/aldis-tm/tests/properties.rs::decoders_never_panic`; `test:crates/aldis-tm/tests/properties.rs::arbitrary_octets_never_panic`; `test:crates/aldis-tm/tests/properties.rs::two_phase_writer_matches_encode`; `test:crates/aldis-tm/tests/properties.rs::rejected_encode_leaves_the_buffer`; `vector:tm-inv-fsh-003`; `vector:tm-sec-dec-006`; `fuzz:tm_decode`; `fuzz:tm_roundtrip` |
| Residual risk | The frame layout computation is checked arithmetic and fails (`Malformed`, `frame_data_field`, `file:crates/aldis-tm/src/view.rs`) when the fields do not fit; the ranges it returns are contiguous and cover the frame exactly (CMN-040), which the `properties` suite and `tm_decode` assert for every frame the decoders accept. The accessors and the writer's `*_mut` functions slice with `get`, which gives an empty slice, not a panic, for a range outside the frame (`file:crates/aldis-tm/src/view.rs`, `file:crates/aldis-tm/src/writer.rs`); that cannot happen for a checked layout, and the properties above would show it. None beyond `hz:HZ-X-01`: a layout corrupted in memory gives a wrong result or an error, not an access outside the buffer |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-1.6`; `ac:AC-4.3`; `ac:AC-4.4` |

### HZ-TM-12 A frame that was begun and never finished is sent

| | |
|---|---|
| Category | `state-corruption` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The output buffer of a two-phase writer holds a frame that `finish` did not complete, and the caller sends it |
| Cause | The caller returns early after `begin`, or an error path skips `finish`. The buffer is then defined (API-023) but incomplete: `begin` writes the headers and zero-fills the rest, so the data field is zero and the FECF, if configured, is zero (`unfinished_frame_is_defined`) |
| Mission effect | A frame of zero data, with correct headers, sent as telemetry |
| Requirements | API-023, API-022, TM-009 |
| Evidence | `test:crates/aldis-tm/src/writer.rs::unfinished_frame_is_defined`; `test:crates/aldis-tm/src/writer.rs::two_phase_writer_fills_every_field`; `test:crates/aldis-tm/src/writer.rs::begin_rejects_without_writing`; `test:crates/aldis-tm/tests/properties.rs::two_phase_writer_matches_encode`; `test:crates/aldis-ffi/tests/tm.rs::encode_arguments_are_checked`; `vector:tm-inv-enc-010` |
| Residual risk | In Rust, `finish(self)` consumes the writer, so a frame cannot be finished twice, and `begin` leaves the buffer unchanged on error (API-022). With the FECF configured, a receiver rejects the unfinished frame unless the CRC of the octets before the zero FECF happens to be zero. With no FECF the unfinished frame is indistinguishable from a valid frame of zero data (`hz:HZ-TM-03`). Whether every begun frame is finished is the caller's; the one-shot `encode` has no such step |
| Assumptions | `asm:ASM-9` |
| Proposals | `proposal:P-TM-03` |
| Claims | `ac:AC-1.6`; `ac:AC-5.4` |

### HZ-TM-13 The stateless finish of the C ABI completes a frame that was begun with other arguments

| | |
|---|---|
| Category | `abi-misuse` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | Removed by OI-57: the C ABI has no two-phase writers and the Rust writers have no resume, so no call finishes a frame that another call began, with other arguments or on a buffer never begun. A frame is built in one call, or in Rust by `TmFrameWriter`, whose `finish(self)` consumes the writer that `begin` returned and so holds the layout `begin` computed |
| Cause | Before OI-57, the stateless finish of the C ABI (OI-50) took the arguments again and trusted them; no such call exists |
| Mission effect | A frame with a valid FECF whose header names a VC and layout that the body does not follow: data read from the wrong octets, or an OCF and Secondary Header misplaced, accepted as telemetry |
| Requirements | API-022, API-023, TM-009, CABI-022 |
| Evidence | `test:crates/aldis-tm/src/writer.rs::two_phase_writer_fills_every_field`; `test:crates/aldis-tm/src/writer.rs::unfinished_frame_is_defined`; `test:crates/aldis-ffi/tests/tm.rs::encode_arguments_are_checked`; `test:crates/aldis-tm/tests/writer_bounds.rs::begin_and_finish_write_only_the_frame` |
| Residual risk | The hazard is removed with the stateless finish (OI-57), so `proposal:P-TM-04`, which hardened it under OI-50, is superseded and no residual of OI-50 remains. What is left is the Rust typestate: `finish(self)` completes the frame its own `begin` started, and a writer that is dropped leaves the defined buffer of API-023 |
| Assumptions | `asm:ASM-6` |
| Proposals | `proposal:P-TM-04` |
| Claims | `ac:AC-6.2`; `ac:AC-1.6` |

## Proposals

| ID | Kind | Proposal | Status |
|---|---|---|---|
| P-TM-01 | `oi` | Draft OI: "What is the longest TM frame that the TM physical channel constructor accepts? 132.0-B-3 states no maximum; 131.0-B-6 §11.1 and §11.3 give 2 048 octets for TM (the 11-bit First Header Pointer). Aldis accepts any length from the minimum (`file:crates/aldis-tm/src/config.rs`; `docs/standards/tm-frame.md` §6), so that the offsets 2045 and 2046 are testable (`tm-fhp-enc-005`, `tm-fhp-enc-err-002`, 2 100-octet frames). Above 4 095 octets the CRC-16 misses double-bit errors (VER-064), which the TM bit-flip suite does not cover. Options: (a) cap at 2 048 (`InvalidConfig`, `frame_len`), which follows 131.0 and needs the two vector cases replaced (new cases, the old two superseded); (b) cap at 4 095, which keeps VER-064 true for every accepted TM frame and the vectors; (c) keep the rule and state in `hz:HZ-X-06` and AC-5.5 that TM above 4 095 octets is outside VER-064." Recommendation (b), or (a) if the maintainer wants 131.0's maximum enforced: either closes the gap in AC-5.5 with one comparison. A scope decision and a reading the Blue Books do not settle, so the maintainer's | `accepted` OI-51 (cap 4095), `m2/hazard-controls` |
| P-TM-02 | `requirement` | Add to TM-003: "It SHALL reject a Secondary Header Length field of 0, a Secondary Header with no data octet (132.0 §4.1.3.1.3 b requires 1 to 63), with `InvalidField` (`fsh_len`)." Text only: `tm-inv-fsh-002` and the FSH length constructor (2 to 64 octets) already pin the behaviour, which TM-003 omits | `accepted` this branch, TM-003 |
| P-TM-03 | `doc` | Add a row to the assumptions of the assurance case: "A writer's caller supplies the First Header Pointer, the frame counts, and the data of every frame, keeps one `OidGenerator` per physical channel for the mission phase, and finishes every frame it begins." Basis: TM-009, TM-010, CMN-021, API-023, `hz:HZ-TM-09`, `hz:HZ-TM-10`, `hz:HZ-TM-12`. Obligation: take the pointer and counts from the multiplexing engine (M3), never reset the generator except at a device start-up, and prefer the one-shot `encode` where no step can be skipped. The three hazards then name the new row in place of `—` | `accepted` this branch, ASM-9 |
| P-TM-04 | `requirement` | Draft requirement withdrawn with OI-57, which removed the two-phase `resume` and the C finish: a call that takes up a buffer would compare the unfinished frame with its arguments and fail, leaving the buffer unchanged, if they differ. | `accepted` OI-50, `m2/hazard-controls` |
