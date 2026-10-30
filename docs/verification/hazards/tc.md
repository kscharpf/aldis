# Hazards: TC Transfer Frame and CLCW

Failure modes of `aldis-tc` and its C ABI (the TC and CLCW functions), in the format of [hazards.md](../hazards.md) §1.7. Standard: [tc-frame.md](../../standards/tc-frame.md), 232.0-B-4 with Cor. 1. The failure modes common to all codecs (panics, loops, stack, the C ABI, the CRC-16) are in [hazards.md](../hazards.md) §3 and are not repeated here.

## Coverage

| Category | Hazards | State |
|---|---|---|
| `corrupt-accepted` | HZ-TC-03 | done |
| `valid-rejected` | HZ-TC-05, HZ-TC-06 | done |
| `misrouting` | HZ-TC-02, HZ-TC-08 | done |
| `truncation` | HZ-TC-01, HZ-TC-04 | done |
| `wrong-value` | HZ-TC-07, HZ-TC-09 | done |
| `nonconformant-output` | HZ-TC-10 | done |
| `unbounded-time` | HZ-TC-15 | done |
| `memory` | HZ-TC-16 | done |
| `panic` | — | n/a: `aldis-tc` has no panic cause of its own; the controls and the evidence for every codec are in `hz:HZ-X-02`, and the TC fuzz targets `tc_decode`, `tc_delimit`, `tc_clcw`, and `tc_roundtrip` run in the nightly `fuzz` job |
| `state-corruption` | HZ-TC-13 | done |
| `abi-misuse` | HZ-TC-14 | done |
| `hidden-deviation` | HZ-TC-11, HZ-TC-12 | done |

## Hazards

### HZ-TC-01 A bit error in the Frame Length field makes the decoder accept a shorter frame

| | |
|---|---|
| Category | `truncation` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The Frame Length field (bits 22–31, 232.0 §4.1.2.7) declares a length L′ shorter than the frame's length L. The TC frame decoder takes the first L′ octets as the frame, reports `len()` = L′, and leaves the rest of the input unread (TC-001, API-011). The view is a valid frame whose data field lost its last L − L′ octets, and perhaps its FECF |
| Cause | A channel error in the Frame Length field that the coding sublayer below did not detect, so that the octets reach Aldis as "clean" (232.0 §4.4.9.2.3) |
| Mission effect | A shortened Frame Data Unit passed up as valid: a telecommand segment or packet missing its end, or a Control Command read from the wrong octets |
| Requirements | TC-001, TC-006, TC-020, TC-021, API-011, VER-064 |
| Evidence | `test:crates/aldis-tc/tests/bit_flips.rs::every_single_bit_error_is_rejected`; `test:crates/aldis-tc/tests/bit_flips.rs::every_double_bit_error_of_the_short_frames_is_rejected`; `nightly-test:crates/aldis-tc/tests/bit_flips.rs::every_double_bit_error_of_the_longest_frame_is_rejected`; `test:crates/aldis-tc/tests/bit_flips.rs::decode_may_accept_a_frame_length_error_as_a_shorter_frame`; `vector:tc-val-011`; `vector:tc-val-012`; `vector:tc-ad-dec-004`; `vector:tc-delimit-003`; `vector:tc-delimit-004`; `test:crates/aldis-tc/tests/vectors.rs::validation_failures`; `test:crates/aldis-tc/tests/vectors.rs::delimit_multiple_frames_with_fill`; `test:crates/aldis-ffi/tests/tc.rs::stream_reports_the_discard_reason`; `test:crates/aldis-ffi/tests/vectors.rs::tc_validation_failures`; `fuzz:tc_decode`; `fuzz:tc_delimit`; `job:bit-flips`; `test:crates/aldis-tc/tests/bit_flips.rs::without_the_fecf_a_shortened_frame_is_delimited_and_accepted`; `test:crates/aldis-tc/tests/bit_flips.rs::with_the_fecf_a_shortened_frame_is_delimited_and_rejected` |
| Residual risk | See the analysis below. In short: `validate_frame` (TC-021 check (d)) rejects the frame only when the octets it is given are bounded by something other than the Frame Length field. Frames from the delimiter (TC-020) are bounded by that same field, so check (d) passes them; the FECF (TC-006) is then the only control, and it is not a CRC guarantee here, because the error moves the end of the codeword. With no FECF configured, a shortened frame from the delimiter is accepted, and its tail is discarded as fill or read as the next frame. Until OI-47 is implemented (M3), a caller-set bound on the fill does not report a leftover above it; the limit of check (d) is stated in tc-frame.md §4.1 |
| Assumptions | `asm:ASM-2`; `asm:ASM-4` |
| Proposals | `proposal:P-TC-01`; `proposal:P-TC-02`; `proposal:P-TC-03` |
| Claims | `ac:AC-1.2`; `ac:AC-5.5` |

**Analysis.** The plan states this hazard as "a bit error in the TC Frame Length field makes `decode` accept a shorter frame (API-011), controlled by `validate_frame` (TC-021)" ([M2 plan](../../milestones/m2-frame-codecs.md) §3, WP-8). That holds under a condition the plan does not state. Where the frame's octets come from decides what each control sees.

1. **`decode` on a longer input** (API-011). `decode` must accept octets after a self-delimiting frame, so the frame ends where the corrupted field says. `tc-ad-dec-004` pins the behaviour for a correct field: a frame followed by three octets, of which exactly the frame is consumed.
   - With the FECF configured, `decode` checks the CRC over the first L′ − 2 octets against octets L′ − 2 and L′ − 1, which are data, not the FECF. The CRC's guarantee of VER-064 covers errors within a codeword of fixed length, and this error changes the length, so it does not apply. The prefix passes exactly when those two data octets equal the CRC of the octets before them. `decode_may_accept_a_frame_length_error_as_a_shorter_frame` builds such a frame and shows `decode` accepting it as 32 octets of a 64-octet frame (`file:crates/aldis-tc/tests/bit_flips.rs`; it also shows `validate_frame` rejecting the same octets with reason 8).
   - With no FECF, `decode` accepts the shortened frame for any L′ the other header checks allow (at least the header, a Segment Header when the VC has one, and one data octet).
   - The bit-flip suite therefore asserts, for errors in the Frame Length field, only that `decode` never returns a frame of the input's whole length (`file:crates/aldis-tc/tests/bit_flips.rs`, `rejected`).
2. **`validate_frame` on the octets of exactly one frame** (TC-021 (d), [tc-frame.md](../../standards/tc-frame.md) §4.1: "`validate_frame` takes the octets of exactly one frame"). When the caller knows the frame's length independently of the field, check (d) compares it with the field and fails with reason 8, `LengthMismatch` (`tc-val-011`, `tc-val-012`). The bit-flip suite requires this rejection for every single-bit error and every double-bit error of its frames, including the longest at night.
3. **`FrameDelimiter` or `next_frame`, then `validate_frame`** (TC-020, the procedure of 232.0 §4.4.9.2 for which TC-021 is the next step). The delimiter reads the same Frame Length field (§4.4.9.2.3 note, §4.4.9.2.4), so its first item is the L′-octet frame, and check (d) agrees with it. What happens next depends on the L − L′ octets left of the real frame, followed by the real fill:
   - they start a valid header (at least five octets, TFVN `00`, a declared length of at least six, [tc-frame.md](../../standards/tc-frame.md) §4.2): the delimiter yields a `TruncatedFrame` if that length runs past the data unit (§4.4.9.2.5, `tc-delimit-004`), discarded with an event; or a `Frame`, which `validate_frame` checks like any other and, without an FECF, may accept;
   - they do not: the delimiter yields `Fill`, discarded (§4.4.9.2.6 a–b, `tc-delimit-003`). Nothing then tells the shortened frame from a good frame followed by fill, although NOTE 2 of §4.4.9.2.6 says that no more than six octets of fill can occur with the present codeblock structure.

   So for frames from the delimiter the FECF (TC-006) is the only control in Aldis. With an FECF, the shortened frame passes only as in item 1. Without one, it passes.

The Blue Book's own procedure has the same property: §4.4.9.2.6 tells the receiver to treat any excess octets as fill and pass the frame on. The protection 232.0 relies on is the coding sublayer's decoding (§4.4.9.2.3: Aldis receives only octets the coding sublayer found free of detected errors) and the optional FECF. Aldis's residual is therefore the Blue Book's, except for what P-TC-01 would add.

Integrator obligations, in `asm:ASM-2` and `asm:ASM-4`: delimit with `FrameDelimiter` and pass every frame to `validate_frame`; where the frame boundary is known independently (one frame per data unit of a lower layer that carries its own length), pass `validate_frame` exactly those octets, so that check (d) applies; configure the FECF wherever an undetected shortened telecommand is not acceptable.

### HZ-TC-02 A frame is delivered on another spacecraft, virtual channel, or MAP

| | |
|---|---|
| Category | `misrouting` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | `decode` or `validate_frame` accepts a frame and reports a Spacecraft ID, VCID, or MAP ID other than the one it was sent with |
| Cause | A channel error in the Spacecraft ID (bits 6–15), the VCID (bits 16–21), or the MAP ID of the Segment Header that names another configured channel; a frame of a channel that is not configured is rejected |
| Mission effect | A telecommand executed by the wrong virtual channel's or MAP's consumer, or for another spacecraft |
| Requirements | TC-006, TC-021, TC-022, VER-064 |
| Evidence | `vector:tc-dec-err-009`; `vector:tc-dec-err-010`; `vector:tc-seg-dec-err-001`; `vector:tc-val-004`; `vector:tc-val-008`; `vector:tc-val-015`; `vector:tc-val-018`; `vector:tc-val-021`; `vector:tc-dec-err-014`; `test:crates/aldis-tc/src/view.rs::header_errors_in_check_order`; `test:crates/aldis-tc/tests/vectors.rs::validation_failures`; `test:crates/aldis-tc/tests/vectors.rs::decode_errors`; `test:crates/aldis-tc/tests/vectors.rs::segment_header`; `test:crates/aldis-tc/tests/bit_flips.rs::every_single_bit_error_is_rejected`; `test:crates/aldis-ffi/tests/tc.rs::stream_reports_the_discard_reason`; `fuzz:tc_decode` |
| Residual risk | The decoder and the validator look the channel up in the `VcLookup` and reject an SCID, VCID, or MAP ID that is not configured (`UnknownChannel`; reasons 2, 5, and 10). An error that turns one configured channel into another passes the lookup; only the FECF detects it, and it is checked before the Segment Header is read, so a MAP ID error is reason 9, not 10 ([tc-frame.md](../../standards/tc-frame.md) §4.1; `tc-val-021`). With the FECF configured, every single-bit and double-bit error is rejected (VER-064). Without it ("Presence of Frame Error Control", 232.0 Table 5-1), such an error is not detected by Aldis; only the coding sublayer protects the frame |
| Assumptions | `asm:ASM-4` |
| Proposals | — |
| Claims | `ac:AC-1.2`; `ac:AC-5.5` |

### HZ-TC-03 A bit error changes the frame type or a Control Command, and the frame is accepted

| | |
|---|---|
| Category | `corrupt-accepted` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A frame with a channel error in the Bypass Flag (bit 2), the Control Command Flag (bit 3), or an octet of a Control Command is accepted as another valid frame: a Type-AD frame becomes Type-BD (FARM-1 then skips its sequence check), a Type-BD becomes Type-AD, a Type-BD frame whose data field is exactly `00` or `82 00 vv` becomes a Type-BC frame, or the V(R) octet of a Set V(R) command takes another value (232.0 §4.1.2.3, Table 4-1, §4.1.3.3) |
| Cause | A channel error that the coding sublayer did not detect, in a frame on a Physical Channel without the FECF |
| Mission effect | A telecommand executed out of sequence or twice because it bypassed FARM-1's sequence check, or FARM-1's V(R) set to a wrong value, after which valid Type-A frames are refused or old ones accepted |
| Requirements | TC-002, TC-005, TC-006, TC-021, VER-064 |
| Evidence | `test:crates/aldis-tc/tests/bit_flips.rs::every_single_bit_error_is_rejected`; `test:crates/aldis-tc/tests/bit_flips.rs::every_double_bit_error_of_the_short_frames_is_rejected`; `nightly-test:crates/aldis-tc/tests/bit_flips.rs::every_double_bit_error_of_the_longest_frame_is_rejected`; `job:bit-flips`; `vector:tc-val-014`; `vector:tc-val-019`; `vector:tc-val-005`; `vector:tc-val-016`; `vector:tc-bc-unlock-dec-004`; `vector:tc-bc-unlock-dec-006`; `vector:tc-bc-setvr-dec-004`; `vector:tc-bc-setvr-dec-005`; `vector:tc-bc-setvr-dec-006`; `vector:tc-bc-setvr-dec-007`; `test:crates/aldis-tc/src/control.rs::unlock_and_set_vr` |
| Residual risk | With the FECF configured, every single-bit and every double-bit error in the frames of the suite is rejected, including those in the two flags and in the command octets (VER-064). The suite's short frames are a Type-BD frame, a Type-BC Unlock frame, and a Type-AD frame with a Segment Header (`file:crates/aldis-tc/tests/bit_flips.rs`); no Set V(R) frame is among them, and the CRC argument is the same for every octet of a frame. Without the FECF ("Presence of Frame Error Control", 232.0 §4.1.4.1.1, Table 5-1) the decoder rejects only the patterns the Blue Book reserves: flags `01` (reason 3) and a Control Command that is neither Unlock nor Set V(R) (reason 11, strict mode; the control-command parser matches `[0]` and `[0x82, 0, vr]` only, `file:crates/aldis-tc/src/control.rs`). A single-bit error that yields another defined pattern is accepted: the Bypass Flag (AD to BD and back), the Control Command Flag of a BD frame with such a data field, and any bit of the V(R) octet. Detecting those belongs to the coding sublayer and to FARM-1 (M4), not to the codec. For errors in the Frame Length field see `hz:HZ-TC-01` and `hz:HZ-TC-04` |
| Assumptions | `asm:ASM-4` |
| Proposals | — |
| Claims | `ac:AC-5.5`; `ac:AC-5.1` |

### HZ-TC-04 A bit error in the Frame Length field makes the delimiter extend a frame over the octets after it

| | |
|---|---|
| Category | `truncation` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The Frame Length field declares a length L′ longer than the frame's length L. When the data unit holds at least L′ octets from the frame's start (the next frame, or fill), `next_frame` yields a `Frame` of L′ octets that holds the frame and the start of what follows (`file:crates/aldis-tc/src/delimit.rs`: the item is `rest.split_at_checked(h.frame_len)`). `validate_frame` of those octets agrees with the field (check (d), TC-021), so only the FECF can reject it. The data field then carries octets that were not sent with it. When the unit cannot complete the declared length, the item is a `TruncatedFrame`, which is the loss of `hz:HZ-TC-05` |
| Cause | A channel error in the Frame Length field that the coding sublayer did not detect (232.0 §4.4.9.2.3) |
| Mission effect | A telecommand segment or packet accepted with foreign octets appended: a longer packet than was sent, or two commands joined. A Type-C frame so extended is no longer exactly Unlock or Set V(R), so strict decoding rejects it (reason 11) |
| Requirements | TC-001, TC-006, TC-020, TC-021, API-011, VER-064 |
| Evidence | `vector:tc-delimit-002`; `vector:tc-delimit-004`; `vector:tc-val-011`; `vector:tc-val-012`; `test:crates/aldis-tc/tests/properties.rs::delimiter_covers_any_octets`; `test:crates/aldis-tc/tests/bit_flips.rs::every_single_bit_error_is_rejected`; `fuzz:tc_delimit`; `fuzz:tc_decode`; `test:crates/aldis-tc/tests/bit_flips.rs::without_the_fecf_a_lengthened_frame_is_delimited_and_accepted`; `test:crates/aldis-tc/tests/bit_flips.rs::with_the_fecf_a_lengthened_frame_is_delimited_and_rejected` |
| Residual risk | The same as `hz:HZ-TC-01`, in the other direction. The error changes the length of the codeword, so the guarantee of VER-064 does not apply; the FECF check passes when the last two octets that the field now selects equal the CRC of the octets before them (a chance of the order of 2⁻¹⁶ for octets unrelated to the CRC; no test builds one for this direction). Without the FECF the extended frame is accepted. The Blue Book's procedure has the same property (§4.4.9.2.6 treats the octets that follow a valid length as fill or as the next frame). `without_the_fecf_a_lengthened_frame_is_delimited_and_accepted` delimits a unit of two frames whose first frame has a raised length that fits in the unit (`proposal:P-TC-07`), and shows the extended frame yielded and accepted without the FECF; with the FECF, `with_the_fecf_a_lengthened_frame_is_delimited_and_rejected` shows each such case of that unit rejected; `proposal:P-TC-02` states both directions |
| Assumptions | `asm:ASM-2`; `asm:ASM-4` |
| Proposals | `proposal:P-TC-02`; `proposal:P-TC-07` |
| Claims | `ac:AC-1.2`; `ac:AC-5.5` |

### HZ-TC-05 A Frame Length error discards the valid frames that share its data unit

| | |
|---|---|
| Category | `valid-rejected` |
| Status | `residual` |
| Severity | S3 |
| Failure mode | A data unit holds several frames, or a frame and fill. An error in the Frame Length field of one frame makes the delimiter read the octets after it at the wrong place. If the error raises the length past the end of the unit, `next_frame` yields a `TruncatedFrame` with all the remaining octets, and every frame after the bad one is discarded (`file:crates/aldis-tc/src/delimit.rs`). If the length stays within the unit, the frames after it are cut at the wrong place and fail the Frame Validation Checks, or are not found |
| Cause | A channel error in a Frame Length field that the coding sublayer did not detect; the procedure of 232.0 §4.4.9.2.4–6 |
| Mission effect | Valid telecommands in the same CLTU are lost together with the corrupted one. The loss is reported, and the commands must be sent again: Type-A frames by COP-1's retransmission, Type-B frames by the sender |
| Requirements | TC-020, TC-021, TC-006 |
| Evidence | `vector:tc-delimit-002`; `vector:tc-delimit-004`; `vector:tc-delimit-008`; `test:crates/aldis-tc/src/delimit.rs::frames_truncation_and_fill`; `test:crates/aldis-tc/tests/vectors.rs::delimit_multiple_frames_with_fill`; `test:crates/aldis-ffi/tests/vectors.rs::tc_delimit_multiple_frames_with_fill`; `test:crates/aldis-ffi/tests/tc.rs::the_stream_intake_pipeline`; `fuzz:tc_delimit` |
| Residual risk | Inherent in the Blue Book: §4.4.9.2.5 has the receiver discard all the octets when fewer arrive than a valid Frame Length declares, and no clause lets a receiver resynchronise inside a data unit. The loss is never silent: the `TruncatedFrame` item carries the discarded octets and the declared length, and a frame cut at the wrong place is rejected with a reason of 1 to 9. The case where the wrongly cut frame is accepted is in `hz:HZ-TC-01` and `hz:HZ-TC-04` |
| Assumptions | `asm:ASM-2`; `asm:ASM-4` |
| Proposals | — |
| Claims | `ac:AC-1.2`; `ac:AC-5.5` |

### HZ-TC-06 A frame that the Blue Book does not forbid is rejected because of the project's minimum data field

| | |
|---|---|
| Category | `valid-rejected` |
| Status | `residual` |
| Severity | S3 |
| Failure mode | A frame whose Frame Data Unit is only a Segment Header, with no User Data, or whose Segment Header, Security Header, Security Trailer, and FECF leave no data octet, is rejected by `decode` and `validate_frame` with `InvalidField` (`frame_data_field`), reason 7 `FrameTooShort` (`file:crates/aldis-tc/src/view.rs`, and the layout error); the writers refuse to build one, and the channel table constructor refuses a channel whose longest frame has no room (CMN-041) |
| Cause | A project rule: 232.0 §4.1.3.2.1.1 b gives a Segment Header "followed by an integral number of User Data octets" and no clause states a minimum, so a sender that reads the number as possibly zero sends frames Aldis refuses (CMN-041; [OI-41](../../09-open-issues.md) item 1, resolved by the maintainer on 2026-10-23) |
| Mission effect | Availability only: a frame from a sender that allows an empty Frame Data Unit is refused with a distinct reason |
| Requirements | CMN-041, TC-007, TC-021 |
| Evidence | `oi:OI-41`; `vector:tc-dec-err-012`; `vector:tc-val-010`; `vector:tc-val-020`; `vector:tc-seg-dec-err-002`; `vector:tc-seg-dec-err-003`; `vector:tc-seg-enc-err-004`; `vector:tc-cfg-011`; `vector:tc-cfg-014`; `vector:tc-cfg-015`; `test:crates/aldis-tc/src/view.rs::length_limits_and_fecf`; `test:crates/aldis-tc/src/writer.rs::encode_checks` |
| Residual risk | The reading is the maintainer's, not the Blue Book's: strict and lenient mode both reject (API-032 lists no such deviation). If a sender that emits an empty Frame Data Unit exists on a mission, its frames are rejected on every decode and the only remedy is the decision of OI-41 item 1 |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-1.2`; `ac:AC-5.3` |

### HZ-TC-07 The data field and the Security Header and Trailer are cut at octets other than the sender's

| | |
|---|---|
| Category | `wrong-value` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The decoder returns `data_field()`, `security_header()`, and `security_trailer()` cut at other octets than the sender's, with a valid frame. Whether a Segment Header is present, the lengths of the Security Header and Trailer, and whether the FECF is present are not on the wire: the decoder takes them from the receiver's `PhysicalChannelConfig`, `VcConfig`, and `MapConfig` (`file:crates/aldis-tc/src/view.rs`; [tc-frame.md](../../standards/tc-frame.md) §2). A frame sent without the FECF to a receiver configured with one is rejected (reason 9); one sent with the FECF to a receiver without it is accepted with the two CRC octets as the last data octets |
| Cause | The receiver's managed parameters differ from the sender's (232.0 §5, §6.6), by a configuration or version error |
| Mission effect | A command whose data is shifted by the Security Header's length, or that includes the CRC, is passed up as valid: a packet that does not parse, or an SDLS check run on the wrong octets |
| Requirements | TC-004, TC-021, TC-022, API-070, API-072 |
| Evidence | `vector:tc-ad-dec-005`; `vector:tc-seg-dec-005`; `vector:tc-bc-unlock-dec-003`; `vector:tc-bc-unlock-dec-007`; `vector:tc-bc-setvr-dec-012`; `vector:tc-dec-err-009`; `vector:tc-dec-err-010`; `vector:tc-seg-dec-err-001`; `vector:tc-val-009`; `test:crates/aldis-tc/src/view.rs::segment_header_and_sdls_slices`; `test:crates/aldis-ffi/tests/tc.rs::security_layouts_come_from_the_vc_or_its_map`; `test:crates/aldis-ffi/tests/tc.rs::channel_arrays_are_c_arrays` |
| Residual risk | The tests show that the decoder applies the configuration it is given; nothing can show that the configuration is the sender's, because the FECF covers all octets of the frame and so cannot see a difference in how they are partitioned. The decoder does reject an unconfigured channel and a frame above the channel's maximum. The obligation of the integrator that the receiver's managed parameters are the sender's is `asm:ASM-8` |
| Assumptions | `asm:ASM-8` |
| Proposals | `proposal:P-TC-05` |
| Claims | `ac:AC-1.2` |

### HZ-TC-08 A caller routes a frame on header fields that were not checked

| | |
|---|---|
| Category | `misrouting` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The TC header peek (and `sym:aldis_tc_header_decode`) returns the frame type, Spacecraft ID, VCID, and N(S) of the first five octets, and `peek_length` the Frame Length, with no channel lookup and no FECF check (`file:crates/aldis-tc/src/view.rs`). A caller that selects the Master Channel's receiver, or a Virtual Channel's consumer, from these fields acts on unverified values |
| Cause | A channel error in the Spacecraft ID or VCID of a frame whose coding sublayer did not detect it, on a call chain that routes before it decodes |
| Mission effect | A telecommand handled by the receiver of another spacecraft or virtual channel |
| Requirements | API-013, API-014, TC-001, TC-002, TC-021 |
| Evidence | `test:crates/aldis-tc/src/view.rs::the_header_peeks_alone`; `test:crates/aldis-ffi/tests/tc.rs::the_header_and_length_peek_alone`; `vector:tc-ad-peek-001`; `vector:tc-ad-peek-003`; `vector:tc-ad-peek-004`; `vector:tc-ad-peek-005`; `vector:tc-dec-err-009`; `vector:tc-val-014` |
| Residual risk | The routing call is followed by `decode` or `validate_frame` with the chosen channel's configuration, which looks the peeked fields up again and, with the FECF configured, rejects an error in them: the peeked values equal the frame's own, so the Spacecraft ID check passes, and the FECF is what fails (VER-064; `hz:HZ-TC-02`). Without the FECF the frame is accepted on the wrong channel, as in `hz:HZ-TC-02`. The doc comment of `peek_header` says that no channel is looked up; a caller that uses the peeked fields and never decodes gets no protection |
| Assumptions | `asm:ASM-2`; `asm:ASM-4` |
| Proposals | — |
| Claims | `ac:AC-1.2`; `ac:AC-5.5` |

### HZ-TC-09 A decoded field differs from the one on the wire

| | |
|---|---|
| Category | `wrong-value` |
| Status | `controlled` |
| Severity | S1 |
| Failure mode | A field of the Primary Header, the Segment Header, a Control Command, or the CLCW is read from the wrong bits, shifted, or masked: for example the Frame Length without the +1 of 232.0 §4.1.2.7.2, a CLCW flag bit, the Status Field or COP in Effect, or the Report Value N(R) |
| Cause | A defect in the decoder |
| Mission effect | A wrong spacecraft, virtual channel, MAP, command, or report value passed up with a frame or CLCW that is otherwise valid; for the CLCW, a FARM-1 report read by the sender with a wrong N(R) or flag |
| Requirements | TC-001, TC-004, TC-005, TC-008, TC-030, TC-031, API-020, GEN-053 |
| Evidence | `vector:tc-ad-dec-002`; `vector:tc-ad-dec-006`; `vector:tc-bd-dec-002`; `vector:tc-bd-dec-004`; `vector:tc-max-dec-001`; `vector:tc-seg-dec-007`; `vector:tc-seg-dec-008`; `vector:tc-clcw-dec-001`; `vector:tc-clcw-dec-002`; `vector:tc-clcw-dec-003`; `vector:tc-clcw-dec-014`; `test:crates/aldis-tc/src/view.rs::decodes_an_ad_frame`; `test:crates/aldis-tc/src/clcw.rs::encode_and_decode`; `test:crates/aldis-tc/tests/properties.rs::encode_then_decode_round_trips`; `test:crates/aldis-tc/tests/properties.rs::clcw_round_trips`; `test:crates/aldis-tc/tests/properties.rs::clcw_decodes_any_word`; `test:crates/aldis-ffi/tests/tc.rs::the_clcw_crosses_as_a_word`; `test:crates/aldis-ffi/tests/vectors.rs::tc_ad`; `test:crates/aldis-ffi/tests/vectors.rs::tc_clcw`; `fuzz:tc_clcw`; `fuzz:tc_decode`; `file:test-vectors/models/tc_frame.py`; `record:TV-023`; `file:docs/verification/derivations/tc-frame.md` |
| Residual risk | — (the vectors are derived by hand and checked by a model written blind, and `tc_clcw` checks every CLCW field against its bit position from an independent model; no independent implementation has cross-checked the frame codecs: `df:DF-9`) |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-1.2`; `ac:AC-6.1` |

### HZ-TC-10 The writer produces a frame or CLCW that does not conform to 232.0

| | |
|---|---|
| Category | `nonconformant-output` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | `encode`, `TcFrameWriter`, or the CLCW-to-OCF conversion produce octets that the Blue Book does not allow: a wrong Frame Length or FECF, a Reserved Spare or CLCW spare that is not zero, a Segment Header on a Virtual Channel without one, a Type-C frame with a Frame Data Unit or a Type-D frame with a Control Command (TC-009), or a frame above the channel's maximum or 1024 octets (TC-007) |
| Cause | A defect in the writer, or a caller's fields that the writer should have refused |
| Mission effect | A frame the spacecraft rejects, or one it accepts as another command |
| Requirements | TC-004, TC-005, TC-006, TC-007, TC-008, TC-009, TC-030, TC-031, API-020, API-021, API-022, API-023, GEN-052 |
| Evidence | `vector:tc-ad-enc-001`; `vector:tc-ad-enc-002`; `vector:tc-ad-enc-006`; `vector:tc-bd-enc-002`; `vector:tc-bc-unlock-enc-001`; `vector:tc-bc-setvr-enc-001`; `vector:tc-seg-enc-001`; `vector:tc-seg-enc-005`; `vector:tc-max-enc-001`; `vector:tc-max-enc-002`; `vector:tc-bd-enc-err-001`; `vector:tc-bc-unlock-enc-err-001`; `vector:tc-bc-unlock-enc-err-002`; `vector:tc-max-enc-err-001`; `vector:tc-max-enc-err-002`; `vector:tc-max-enc-err-003`; `vector:tc-seg-enc-err-001`; `vector:tc-seg-enc-err-002`; `vector:tc-clcw-enc-001`; `vector:tc-clcw-enc-002`; `vector:tc-clcw-enc-004`; `test:crates/aldis-tc/src/writer.rs::encode_round_trips`; `test:crates/aldis-tc/src/writer.rs::encode_checks`; `test:crates/aldis-tc/src/writer.rs::two_phase_writer_with_sdls`; `test:crates/aldis-tc/tests/properties.rs::encode_then_decode_round_trips`; `test:crates/aldis-tc/tests/properties.rs::writers_agree`; `test:crates/aldis-ffi/tests/tc.rs::encode_arguments_are_checked`; `fuzz:tc_roundtrip`; `fuzz:tc_clcw`; `file:test-vectors/models/tc_frame.py`; `record:TV-023` |
| Residual risk | Three outputs are conformant but not what a mission may expect. (1) TC-008: the writer writes the caller's N(S) in a Type-B frame; 232.0 §4.1.2.8 note 3 says COP sets it to zero, in a note, and the codec does not enforce it. (2) A channel with a Security Header or Trailer gets them written as zeros (API-072, `file:crates/aldis-tc/src/writer.rs`); the FECF is computed over those zeros at `finish`, so the one-shot `encode` of an SDLS channel yields a frame with a valid FECF and no security, and a caller that changes those octets after `finish` leaves a wrong FECF. The security hooks are outside M2 (`asm:CTX-1`). (3) The CLCW encoder accepts any COP in Effect (TC-030). TC-009 is covered by vector cases and by assertions in `encode_checks` and `writer_arguments_are_checked` that do not name it in their `// req:` lines, so the trace lists it with vectors only |
| Assumptions | `asm:CTX-1` |
| Proposals | — |
| Claims | `ac:AC-1.2` |

### HZ-TC-11 A CLCW of another COP is accepted as a COP-1 report

| | |
|---|---|
| Category | `hidden-deviation` |
| Status | `residual` |
| Severity | S4 |
| Failure mode | The CLCW decoder returns Ok, in strict mode too, for a CLCW whose COP in Effect is `00`, `10`, or `11`, with no anomaly and no error; the value is in the field `cop_in_effect` (`file:crates/aldis-tc/src/clcw.rs`). 232.0 §4.2.1.5.2 says the field is `01` for COP-1. A caller that reads the report value N(R) and the flags without reading the field treats the report as COP-1's |
| Cause | A listed exception of API-031 (maintainer decision of 2026-10-25): FOP-1 is to reject COP ≠ `01` itself, and no FOP-1 exists in M2 (TC-030); or a spacecraft that sends another COP |
| Mission effect | A ground system reads a report of another procedure as the state of COP-1's FARM, and retransmits or releases frames on wrong grounds |
| Requirements | TC-030, API-031 |
| Evidence | `vector:tc-clcw-dec-004`; `vector:tc-clcw-dec-005`; `test:crates/aldis-tc/src/clcw.rs::decode_errors_and_anomalies`; `test:crates/aldis-ffi/tests/tc.rs::the_clcw_crosses_as_a_word`; `fuzz:tc_clcw` |
| Residual risk | The value is exposed and tested; rejecting it is FOP-1's job, and FOP-1 is outside M2 (`asm:CTX-1`). Until then no component of Aldis rejects a CLCW of another COP, the anomaly set has no flag for it ([tc-frame.md](../../standards/tc-frame.md) §6), and checking `cop_in_effect` is the caller's obligation until FOP-1 exists (`asm:ASM-10`) |
| Assumptions | `asm:ASM-10` |
| Proposals | `proposal:P-TC-04` |
| Claims | `ac:AC-5.1`; `ac:AC-5.2` |

### HZ-TC-12 Lenient mode accepts a reserved Control Command or a set spare bit, and the caller does not read the anomaly

| | |
|---|---|
| Category | `hidden-deviation` |
| Status | `residual` |
| Severity | S4 |
| Failure mode | In lenient mode, a frame with a Reserved Spare other than `00`, a Type-C frame whose data field is neither Unlock nor Set V(R), or a CLCW with a set spare, is accepted. For the Type-C frame, `control_command()` is `None` and the octets are in `data_field()` (`file:crates/aldis-tc/src/view.rs`); a caller that tests `control_command()` and ignores the anomaly set sees a Type-C frame with no command |
| Cause | A caller that decodes in lenient mode (API-032) and ignores the anomaly set |
| Mission effect | A frame with set spare bits, or an unknown command, treated as an ordinary frame or silently dropped; a future Control Command that CCSDS defines later is not seen as one |
| Requirements | TC-003, TC-005, TC-030, API-030, API-032, API-033, GEN-043 |
| Evidence | `vector:tc-dec-err-008`; `vector:tc-dec-err-013`; `vector:tc-val-007`; `vector:tc-val-017`; `vector:tc-bc-unlock-dec-005`; `vector:tc-bc-setvr-dec-008`; `vector:tc-bc-setvr-dec-011`; `vector:tc-clcw-dec-012`; `vector:tc-clcw-dec-013`; `vector:tc-clcw-dec-016`; `test:crates/aldis-tc/src/view.rs::control_commands`; `test:crates/aldis-tc/tests/properties.rs::decoder_never_panics`; `fuzz:tc_decode`; `fuzz:tc_clcw`; `record:API-033` |
| Residual risk | Lenient mode accepts only the three deviations of [tc-frame.md](../../standards/tc-frame.md) §6, and each raises its flag; the decoder returns no `Ok` without the flag (`decoder_never_panics` asserts that a Type-C frame has a command unless `UnknownControlCommand` is set; `tc_decode` asserts that strict implies lenient with an empty anomaly set). Strict mode is the default. Whether the anomaly is acted on is the caller's; this is `hz:HZ-X-08` for the TC decoder |
| Assumptions | `asm:ASM-5` |
| Proposals | — |
| Claims | `ac:AC-5.2` |

### HZ-TC-13 A frame is finished with other arguments than it was begun with

| | |
|---|---|
| Category | `state-corruption` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | Removed by OI-57: the C ABI has no two-phase writers and the Rust writers have no resume, so no call finishes a frame that another call began, with other arguments or on a buffer never begun. A frame is built in one call, or in Rust by `TcFrameWriter`, whose `finish(self)` consumes the writer that `begin` returned and so holds the layout `begin` computed |
| Cause | Before OI-57, the stateless finish of the C ABI (OI-50) took the arguments again and trusted them; no such call exists |
| Mission effect | A frame sent to another MAP than the one the caller addressed, or with garbage in place of the Frame Data Unit, with a correct FECF so that no receiver check can tell |
| Requirements | API-022, API-023, TC-004, TC-007, CABI-022 |
| Evidence | `test:crates/aldis-tc/src/writer.rs::unfinished_and_rejected_finish`; `test:crates/aldis-tc/src/writer.rs::two_phase_writer_with_sdls`; `test:crates/aldis-ffi/tests/tc.rs::encode_arguments_are_checked`; `test:crates/aldis-tc/tests/writer_bounds.rs::begin_and_finish_write_only_the_reservation` |
| Residual risk | The hazard is removed with the stateless finish (OI-57), so `proposal:P-TC-08`, which hardened it under OI-50, is superseded and no residual of OI-50 remains. What is left is the Rust typestate: `finish(self)` completes the frame its own `begin` started, and a writer that is dropped leaves the defined buffer of API-023 |
| Assumptions | `asm:ASM-6` |
| Proposals | `proposal:P-TC-08` |
| Claims | `ac:AC-1.2`; `ac:AC-6.2` |

### HZ-TC-14 A C caller passes a TC value outside its set

| | |
|---|---|
| Category | `abi-misuse` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A C caller passes a frame type, decode mode, flag, Control Command, Sequence Flags value, or an offset into the data unit that is outside what the function accepts, and gets a frame or a CLCW instead of an error: for example a flag of 2 read as true, or a frame type of 3 read as BC |
| Cause | A defect in the C caller, or a header and library of different versions (CABI-060) |
| Mission effect | A frame built or decoded under values the caller did not mean |
| Requirements | CABI-021, CABI-022, CABI-060, RS-020 |
| Evidence | `test:crates/aldis-ffi/tests/tc.rs::the_clcw_crosses_as_a_word`; `test:crates/aldis-ffi/tests/tc.rs::control_commands`; `test:crates/aldis-ffi/tests/tc.rs::encode_arguments_are_checked`; `test:crates/aldis-ffi/tests/tc.rs::channel_arrays_are_c_arrays`; `test:crates/aldis-ffi/tests/tc.rs::the_stream_intake_pipeline`; `test:crates/aldis-ffi/src/abi.rs::modes_and_flags_take_only_their_constants`; `fuzz:ffi_decode` |
| Residual risk | `fields` parses a frame type, a flag, and the Sequence Flags into their types and answers anything else with `InvalidField` (`file:crates/aldis-ffi/src/tc.rs`), `command` does the same for the Control Command, and the C ABI parsers of the mode and the flag accept only their constants. The tests above show each rejection. What no check can see is a wrong non-null pointer or an overstated length, or a header and a library of different versions (the structs carry no size, so a configuration array is stepped with the wrong stride; the caller compares `sym:aldis_abi_version` with `ALDIS_ABI_VERSION`, `asm:ASM-13`), which are `hz:HZ-X-05`; a stale buffer between `begin` and `finish` is `hz:HZ-TC-13` |
| Assumptions | `asm:ASM-6`, `asm:ASM-13` |
| Proposals | — |
| Claims | `ac:AC-6.2` |

### HZ-TC-15 A loop over the data unit does not advance

| | |
|---|---|
| Category | `unbounded-time` |
| Status | `residual` |
| Severity | S2 |
| Failure mode | A loop that delimits a data unit with `next_frame`, `FrameDelimiter`, or `sym:aldis_tc_stream_next`, or that walks a stream with `peek_length` and `decode`, takes an item of zero length and never ends |
| Cause | An item or a length of zero octets from the library, or a caller that does not add the item's length to its offset |
| Mission effect | The calling task hangs until a watchdog acts |
| Requirements | TC-020, GEN-022, GEN-023, GEN-053 |
| Evidence | `test:crates/aldis-tc/tests/properties.rs::delimiter_covers_any_octets`; `test:crates/aldis-tc/tests/properties.rs::delimiter_returns_the_frames`; `test:crates/aldis-tc/src/delimit.rs::frames_truncation_and_fill`; `test:crates/aldis-tc/src/delimit.rs::what_is_fill`; `test:crates/aldis-ffi/tests/tc.rs::the_stream_intake_pipeline`; `fuzz:tc_delimit`; `record:GEN-022` |
| Residual risk | Every item takes at least one octet: `next_frame` returns `None` for an empty rest and otherwise a frame of at least six octets or the whole rest (`file:crates/aldis-tc/src/delimit.rs`), the length of a delimited item is never 0, and `peek_length` returns the Frame Length field plus one, at least 1. `tc_delimit` checks that the items cover the unit exactly. The loop is the caller's: the C ABI keeps no state, and a caller that passes the same offset again gets the same item. The bounds of the library's own loops rest on `record:GEN-022` and `check:stack` (`hz:HZ-X-03`) |
| Assumptions | `asm:CTX-2` |
| Proposals | — |
| Claims | `ac:AC-3.1` |

### HZ-TC-16 A writer changes octets of the caller's buffer outside the frame it reserved

| | |
|---|---|
| Category | `memory` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | `encode`, the TC writer's begin, or `finish` write octets of the caller's buffer beyond the frame, or beyond the reservation of `begin`, or report a length that does not match what was written |
| Cause | A defect in the length arithmetic of the writer plan constructor, `start`, or `finish` |
| Mission effect | Corruption of the data the caller keeps after the frame in the same buffer, for example the next frame of a CLTU being assembled. Writing outside the buffer itself is not possible: the crate forbids `unsafe` (GEN-070) and every access is a checked `get` or `get_mut` |
| Requirements | API-022, API-023, GEN-014, GEN-020, GEN-070 |
| Evidence | `test:crates/aldis-tc/src/writer.rs::encode_checks`; `test:crates/aldis-tc/src/writer.rs::two_phase_writer_with_sdls`; `test:crates/aldis-tc/tests/properties.rs::writers_agree`; `test:crates/aldis-ffi/tests/tc.rs::encode_arguments_are_checked`; `fuzz:tc_roundtrip`; `record:GEN-070`; `record:VER-010`; `check:clippy`; `test:crates/aldis-tc/tests/writer_bounds.rs::encode_writes_only_the_frame`; `test:crates/aldis-tc/tests/writer_bounds.rs::a_rejected_encode_changes_nothing`; `test:crates/aldis-tc/tests/writer_bounds.rs::begin_and_finish_write_only_the_reservation`; `test:crates/aldis-tc/tests/writer_bounds.rs::a_failed_begin_or_finish_changes_nothing_of_the_frame` |
| Residual risk | The writes are bounded by construction: `start` takes `out[..plan.total]` and fails with `BufferTooSmall` otherwise (`file:crates/aldis-tc/src/writer.rs`), and `finish` works on `out[..total]` with `total` at most `plan.total`. A rejected encode leaves the buffer unchanged, which `encode_checks`, `writers_agree`, and `tc_roundtrip` check. The tests of `file:crates/aldis-tc/tests/writer_bounds.rs` (`proposal:P-TC-06`) fill the buffer with a pattern and show that the octets after a successfully written frame, and after the reservation of `begin`, are untouched, including for `finish` below the reservation (whose reserved octets after the frame are "unspecified" by the doc comment, and are not looked at); and that a rejected `encode`, a failed `begin`, and a failed `finish` change no octet of the frame or the reservation. The stack use of the channel table is `hz:HZ-X-04`. |
| Assumptions | — |
| Proposals | `proposal:P-TC-06` |
| Claims | `ac:AC-4.3` |

## Proposals

| ID | Kind | Proposal | Status |
|---|---|---|---|
| P-TC-01 | `oi` | Draft OI, "TC fill longer than the coding can produce": 232.0 §4.4.9.2.6 a–b treats every octet after the last delimited frame as fill, and TC-020 does the same; NOTE 2 of §4.4.9.2.6 says that at most six octets of fill occur with the present codeblock. Options: (a) `next_frame` takes the longest fill the coding sublayer appends (a caller parameter: six for the BCH codeblock of 231.0) and reports a longer rest as a distinct item, still discarded, with its own C ABI item code; (b) keep TC-020 and state the residual in [tc-frame.md](../../standards/tc-frame.md) §4.2. Recommendation (a): it reports a frame shortened by a Frame Length error whose leftover octets do not start a valid header, whenever those octets and the real fill exceed the bound: every shortening by seven octets or more, so every single-bit error in bits 22–28 of the field that lowers the length, at no cost to a valid data unit; a shortening by one, two, or four octets (bits 29–31) is reported only when the real fill makes up the difference. Maintainer: the bound is in a NOTE ([decision rights](../../process/decision-rights.md) §2.1), and the item is a new C ABI value (§2.4). `hz:HZ-TC-01` | `accepted` OI-47, implemented in M3 |
| P-TC-02 | `doc` | State in [tc-frame.md](../../standards/tc-frame.md) §4.1 and §4.2 that check (d) cannot see a Frame Length error in a frame the delimiter yields, because the delimiter reads the same field, so that the FECF (TC-006) is then the only control, and that with no FECF a shortened frame is accepted. Wording that changes no behaviour (§2.2, the coordinator's) | `accepted` this branch, tc-frame.md §4.1, §4.2 |
| P-TC-03 | `test` | A test in `file:crates/aldis-tc/tests/bit_flips.rs` that delimits a data unit holding a frame with a Frame Length error, on a channel without the FECF, and shows `next_frame` yielding the shortened frame and `validate_frame` accepting it, so that the residual of `hz:HZ-TC-01` is pinned and a change to it (P-TC-01) shows. An agent's decision | `accepted` `m2/hazard-controls` |
| P-TC-04 | `doc` | State the obligation of the consumer of a decoded CLCW: until FOP-1 exists (M3), Aldis accepts every COP in Effect (TC-030, the listed exception of API-031), so a caller that uses the Report Value or the flags SHALL check `cop_in_effect` against the COP-1 value of COP in Effect. Write it as an `ASM` row of the [assurance case](../assurance-case.md#assumptions-and-context) (or an extension of ASM-5) and in the docs of the CLCW decoder, the CLCW report decoder, and `sym:aldis_clcw_decode`. It reopens nothing of the decision of 2026-10-25 and changes no behaviour (decision rights §2.2, the coordinator's). `hz:HZ-TC-11` | `accepted` this branch, ASM-10 |
| P-TC-05 | `doc` | State in [tc-frame.md](../../standards/tc-frame.md) §2 and as an `ASM` row of the assurance case that the receiver's managed parameters (the Spacecraft IDs, VCIDs, and MAP IDs, the maximum frame lengths, the presence of the FECF and of the Segment Header, and the Security Header and Trailer lengths of each VC and MAP) are those of the sender, because none of them is on the wire and the FECF cannot detect a difference in how the octets are partitioned. Wording that changes no behaviour (§2.2, the coordinator's). The same sentence is true of the other frame codecs, so the coordinator may prefer one `ASM` row for all. `hz:HZ-TC-07` | `accepted` this branch, ASM-8 |
| P-TC-06 | `test` | A test in `crates/aldis-tc/tests/` that writes frames with `encode` and with the TC writer's begin and `finish` (also with `user_len` below the reservation) into a buffer prefilled with a pattern and longer than the frame, and asserts that the octets from the end of the reservation on are unchanged, and that an `encode` rejected for any reason leaves every octet unchanged. It turns the inspection of the writer into evidence for `hz:HZ-TC-16`. An agent's decision | `accepted` `m2/hazard-controls` |
| P-TC-07 | `test` | A test in `file:crates/aldis-tc/tests/bit_flips.rs`, beside the one of P-TC-03: a data unit of two frames, with and without the FECF, in which each single-bit error of the first frame's Frame Length that raises the length to a value within the unit is applied; it shows `next_frame` yielding the extended frame and `validate_frame` accepting it without the FECF and rejecting each such case with it (listing any that pass). It pins the residual of `hz:HZ-TC-04` so that a change to it, such as P-TC-01, shows. An agent's decision | `accepted` `m2/hazard-controls` |
| P-TC-08 | `requirement` | Draft requirement withdrawn with OI-57, which removed the two-phase `resume` and the C finish: a call that takes up a buffer would compare the unfinished frame with its arguments and fail, leaving the buffer unchanged, if they differ. | `accepted` OI-50, `m2/hazard-controls` |
