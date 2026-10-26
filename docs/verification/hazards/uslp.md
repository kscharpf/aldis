# Hazards: USLP Transfer Frame

Failure modes of `aldis-uslp` and its C ABI (`aldis_uslp_*`), in the format of [hazards.md](../hazards.md) §1.7. Standard: [uslp-frame.md](../../standards/uslp-frame.md), 732.1-B-3. The failure modes common to all codecs (panics, loops, stack, the C ABI, the CRC-16) are in [hazards.md](../hazards.md) §3 and are not repeated here.

## Coverage

| Category | Hazards | State |
|---|---|---|
| `corrupt-accepted` | HZ-USLP-02, HZ-USLP-04, HZ-USLP-05 | done |
| `valid-rejected` | HZ-USLP-06, HZ-USLP-07 | done |
| `misrouting` | HZ-USLP-03, HZ-USLP-08 | done |
| `truncation` | HZ-USLP-01 | done |
| `wrong-value` | HZ-USLP-09, HZ-USLP-10, HZ-USLP-11 | done |
| `nonconformant-output` | HZ-USLP-12, HZ-USLP-13 | done |
| `unbounded-time` | — | n/a: `aldis-uslp` adds no failure mode of its own to `hz:HZ-X-03`. Every loop of the crate is bounded by an input length, a capacity, or a constant (the loop table of `gen-022-023-aldis-uslp.md`), and `next_frame` (`crates/aldis-uslp/src/delimit.rs:93`–`112`) returns `None` only for an empty rest and otherwise an item of at least 6 octets or the whole rest, so a caller that skips each item's length terminates |
| `memory` | HZ-USLP-14 | done |
| `panic` | — | n/a: no USLP-specific panic path beyond `hz:HZ-X-02`. The crate's arithmetic on wire lengths is saturating or checked (for example `crates/aldis-uslp/src/view.rs:194`–`201`), and the property `decoder_never_panics` and the fuzz targets `uslp_decode`, `uslp_truncated`, and `uslp_roundtrip` drive every decoder, writer, and delimiter entry point, which `hz:HZ-X-02` lists as evidence |
| `state-corruption` | HZ-USLP-15 | done |
| `abi-misuse` | HZ-USLP-16, HZ-USLP-17 | done |
| `hidden-deviation` | HZ-USLP-18 | done |

## Hazards

### HZ-USLP-01 A bit error in the Frame Length field makes the decoder accept a shorter frame

| | |
|---|---|
| Category | `truncation` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | On a variable-length Physical Channel, the Frame Length field (bits 32–47, 732.1 §4.1.2.7) declares a length L′ shorter than the frame's length L. `UslpFrame::decode` takes the first L′ octets as the frame, reports `len()` = L′, and leaves the rest unread (USLP-003, API-011). On a fixed-length channel the same error is rejected |
| Cause | A channel error in the Frame Length field that the coding sublayer below did not detect |
| Mission effect | A shortened TFDZ passed up as valid: a packet, MAPA_SDU, or octet stream missing its end, and an OCF or the FECF read from data octets |
| Requirements | USLP-003, USLP-014, USLP-015, API-011, API-012, VER-064 |
| Evidence | `vector:uslp-val-019`; `vector:uslp-inv-013`; `vector:uslp-val-025`; `vector:uslp-val-026`; `vector:uslp-inv-016`; `vector:uslp-delim-003`; `test:crates/aldis-uslp/src/view.rs::length_errors_on_a_variable_channel`; `test:crates/aldis-uslp/tests/vectors.rs::validation_failures`; `test:crates/aldis-uslp/tests/vectors.rs::delimit_multiple_frames_with_fill`; `test:crates/aldis-uslp/tests/bit_flips.rs::every_single_bit_error_of_the_short_frames_is_as_expected`; `test:crates/aldis-uslp/tests/bit_flips.rs::every_double_bit_error_of_the_short_frames_is_as_expected`; `nightly-test:crates/aldis-uslp/tests/bit_flips.rs::every_double_bit_error_within_the_guarantee_is_as_expected`; `test:crates/aldis-ffi/tests/uslp.rs::validation_reports_a_stable_reason`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_validation_failures`; `fuzz:uslp_decode`; `job:bit-flips`; `test:crates/aldis-uslp/tests/bit_flips.rs::decode_may_accept_a_frame_length_error_as_a_shorter_frame` |
| Residual risk | As for TC (`hz:HZ-TC-01`), on a variable-length channel: `validate_frame` (USLP-014 check (d)) rejects the frame only when its octets are bounded by something other than the Frame Length field, and frames from the delimiter (USLP-015) are bounded by that same field. The FECF is then the only control, and the CRC's guarantee does not cover an error that moves the end of the codeword. With no FECF, a shortened frame from the delimiter is accepted. On a fixed-length channel, `decode` and `validate_frame` require the configured length (USLP-003, API-012; reason 10) and reject every such error. Until OI-47 is implemented (M3), a caller-set bound on the fill does not report a leftover above it |
| Assumptions | `asm:ASM-2`; `asm:ASM-4` |
| Proposals | `proposal:P-USLP-01`; `proposal:P-USLP-02`; `proposal:P-USLP-03` |
| Claims | `ac:AC-1.4`; `ac:AC-5.5` |

**Analysis.** The reasoning of `hz:HZ-TC-01` applies, with these differences.

1. **Fixed-length channels are not exposed.** On a fixed-length Physical Channel the frame's length is a managed parameter, and `decode` and `validate_frame` both compare the Frame Length field with it: `FixedLengthMismatch`, reason 10 (`uslp-val-019`; `uslp-inv-013` when decoding). The bit-flip suite's 64-octet fixed-length frame shows every single-bit and double-bit error rejected by both ([module docs](../../../crates/aldis-uslp/tests/bit_flips.rs), lines 13–14).
2. **Variable-length channels behave as TC.** `decode` accepts octets after the frame (API-011), so a shorter L′ is read as the frame; the bit-flip suite asserts for errors in the field only that `decode` never returns a frame of the input's whole length (lines 15–19 of the same file). `validate_frame` given exactly the frame's octets rejects with reason 13, `LengthMismatch` (`uslp-val-025`, `uslp-val-026`). Like the TC test, `decode_may_accept_a_frame_length_error_as_a_shorter_frame` builds a shortened frame that `decode` and the delimiter yield and `validate_frame` accepts, and that `validate_frame` rejects given the 64 octets it was sent as (P-USLP-02).
3. **The delimiter reads the same field** (732.1 §4.3.11.2.2–4, USLP-015), so check (d) agrees with the shortened frame. The octets left of the real frame then start a valid header ([uslp-frame.md](../../standards/uslp-frame.md) §3.2: four octets with TFVN `1100` and either the End of Frame Primary Header Flag `1`, or seven octets and a Frame Length + 1 of at least 9 plus the VCF Count Length), giving an `IncompleteFrame` (`uslp-delim-003`), a `NoTruncatedLength`, or another `Frame`; or they are `Fill`, discarded (§4.3.11.2.4 a–b). NOTE 2 of §4.3.11.2.4 bounds the fill by the code below: up to six octets for the BCH code, seven for the (128,64) LDPC code, and 31 for the (512,256) LDPC code. The same note names a case in which even real fill can pass the validation checks: the BCH code with randomization, when the fill is added after randomization at the sending end.
4. **Frames are longer.** A variable-length USLP frame may be 65 536 octets (§4.1.2.7), so a shortening can remove far more data than in TC, and in a frame above 4 095 octets even the CRC's double-bit guarantee is gone (`hz:HZ-X-06`).

Integrator obligations: as for TC (`asm:ASM-2`, `asm:ASM-4`).

### HZ-USLP-02 A bit error in the End of Frame Primary Header Flag turns a frame into a truncated frame without an FECF

| | |
|---|---|
| Category | `corrupt-accepted` |
| Status | `controlled` |
| Severity | S1 |
| Failure mode | The End of Frame Primary Header Flag (bit 31) of a non-truncated frame reads `1`, on a VC that has a Truncated Transfer Frame Length. `decode`, `validate_frame`, and the delimiter then treat the first 6–32 octets as a truncated frame (USLP-002, USLP-015): the length comes from the VC, the octet after the 4-octet header is read as the TFDF Header, and no FECF is checked, since a truncated frame has none (Annex D1.2.2 note 1). The truncated frame's TFDZ is made of the real frame's Frame Length, flags, VCF Count, and first data octets |
| Cause | A single-bit channel error in the flag that the coding sublayer did not detect, on a VC on which non-truncated frames are also sent. Before OI-48 Aldis accepted that: `encode` accepted a VC that has a truncated frame length, and `decode`, `validate_frame`, and the delimiter accepted a non-truncated frame on it. Since OI-48 a VC with a truncated frame length carries only truncated frames (732.1 Annex D1.2.1, project reading; USLP-002), so no non-truncated frame is sent on it and a flag error makes no frame of such a VC look truncated |
| Mission effect | Header octets of a frame passed up as the user data of a short frame, on the same VC and MAP; the delimiter then continues inside the real frame, whose remaining octets may be read as further frames or discarded as fill |
| Requirements | USLP-002, USLP-011, USLP-014, USLP-015, USLP-016, API-031 |
| Evidence | `vector:uslp-val-015`; `vector:uslp-trunc-020`; `vector:uslp-inv-021`; `vector:uslp-trunc-016`; `vector:uslp-trunc-017`; `vector:uslp-trunc-018`; `vector:uslp-val-032`; `test:crates/aldis-uslp/src/truncated.rs::truncated_errors`; `test:crates/aldis-uslp/tests/vectors.rs::truncated`; `fuzz:uslp_truncated`; `test:crates/aldis-uslp/tests/bit_flips.rs::a_flipped_end_of_frame_flag_of_a_truncated_frame_is_rejected_whatever_follows`; `test:crates/aldis-uslp/tests/bit_flips.rs::a_flipped_end_of_frame_flag_of_a_non_truncated_frame_is_rejected`; `vector:uslp-val-041`; `vector:uslp-val-042`; `vector:uslp-val-043`; `vector:uslp-val-044`; `vector:uslp-val-045`; `vector:uslp-val-046`; `vector:uslp-val-047`; `vector:uslp-val-048`; `vector:uslp-val-049`; `vector:uslp-val-050`; `vector:uslp-val-051`; `vector:uslp-val-052`; `vector:uslp-val-053`; `vector:uslp-trunc-039`; `vector:uslp-trunc-040`; `vector:uslp-trunc-041`; `vector:uslp-trunc-042`; `vector:uslp-trunc-043`; `vector:uslp-trunc-044`; `vector:uslp-trunc-045`; `vector:uslp-trunc-046`; `vector:uslp-trunc-047`; `vector:uslp-trunc-048`; `vector:uslp-trunc-049`; `vector:uslp-trunc-050`; `vector:uslp-trunc-051`; `vector:uslp-trunc-052`; `vector:uslp-trunc-053`; `vector:uslp-trunc-054`; `vector:uslp-trunc-055`; `vector:uslp-tsdls-017`; `vector:uslp-tsdls-018`; `vector:uslp-tsdls-019`; `vector:uslp-tsdls-020`; `vector:uslp-tsdls-021`; `vector:uslp-delim-017`; `vector:uslp-delim-018`; `vector:uslp-delim-019`; `vector:uslp-delim-020`; `vector:uslp-delim-021`; `vector:uslp-delim-022`; `test:crates/aldis-uslp/src/view.rs::a_vc_with_a_truncated_length_refuses_non_truncated_frames`; `test:crates/aldis-uslp/src/writer.rs::a_vc_with_a_truncated_length_refuses_non_truncated_frames`; `test:crates/aldis-uslp/src/delimit.rs::a_vc_with_a_truncated_length_carries_only_truncated_frames`; `test:crates/aldis-ffi/tests/uslp.rs::a_vc_with_a_truncated_length_refuses_non_truncated_frames` |
| Residual risk | On a VC with a truncated frame length, `decode` and `validate_frame` reject a non-truncated frame, whatever its other fields, with `InvalidField` (`end_of_frame_primary_header_flag`), reason 18 (`check_channel`, `crates/aldis-uslp/src/view.rs:147`–`152`, after the checks of the flags, the Reserve Spares, the VCID, and the MAP ID, before the VCF Count Length, the OCF Flag, and the length; `uslp-val-041`…`053`, `uslp-trunc-043`…`052`); `peek_length` rejects the header at once (`uslp-trunc-053`, `-054`); the writers reject one (`crates/aldis-uslp/src/writer.rs:157`–`159`, `uslp-trunc-039`…`042`); and the delimiter discards the rest of the data unit as `NonTruncatedNotAllowed`, without trusting its Frame Length (`crates/aldis-uslp/src/delimit.rs:75`–`131`, `uslp-delim-017`…`019`). So the End of Frame Primary Header Flag error of this hazard is rejected whichever way it flips: on a VC without a truncated frame length, reason 7 (`uslp-val-015`, `uslp-trunc-020`); on a VC with one, reason 18 for a truncated frame read as non-truncated (`a_flipped_end_of_frame_flag_of_a_truncated_frame_is_rejected_whatever_follows`), and no non-truncated frame exists there to be read as truncated (`a_flipped_end_of_frame_flag_of_a_non_truncated_frame_is_rejected`). The failure mode needs two errors, the flag and the VCID. The reading of Annex D1.2.1 and D1.2.4 is the project's, a maintainer decision (OI-48): a sender that sends both kinds on one VC is rejected, not misread (a valid frame rejected, with a reported error). Proposals P-USLP-04 and P-USLP-05 were accepted as OI-48 and the bit-flip tests, and their rows are removed. The residual of the truncated frames themselves, which have no FECF, is `hz:HZ-USLP-04` |
| Assumptions | `asm:ASM-7` |
| Proposals | — |
| Claims | `ac:AC-1.4` |

### HZ-USLP-03 A frame is delivered on another spacecraft, virtual channel, or MAP

| | |
|---|---|
| Category | `misrouting` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | `decode` or `validate_frame` accepts a frame and reports a Spacecraft ID, VCID, or MAP ID other than the one it was sent with |
| Cause | A channel error in the Spacecraft ID (bits 4–19), the VCID (bits 21–26), or the MAP ID (bits 27–30) that names another configured channel; a frame of a channel that is not configured is rejected |
| Mission effect | User data delivered to the wrong virtual channel's or MAP's consumer, or taken for another spacecraft's |
| Requirements | USLP-014, USLP-002, VER-064 |
| Evidence | `vector:uslp-val-008`; `vector:uslp-val-012`; `vector:uslp-val-013`; `vector:uslp-val-014`; `vector:uslp-inv-009`; `vector:uslp-inv-010`; `vector:uslp-trunc-022`; `vector:uslp-trunc-023`; `vector:uslp-val-028`; `vector:uslp-val-029`; `test:crates/aldis-uslp/src/view.rs::header_errors_in_check_order`; `test:crates/aldis-uslp/tests/vectors.rs::validation_failures`; `test:crates/aldis-uslp/tests/vectors.rs::invalid`; `test:crates/aldis-uslp/tests/bit_flips.rs::every_single_bit_error_of_the_short_frames_is_as_expected`; `test:crates/aldis-ffi/tests/uslp.rs::validation_reports_a_stable_reason`; `fuzz:uslp_decode` |
| Residual risk | The decoder and the validator look the channel up and reject an SCID, VCID, or MAP ID that is not configured (`UnknownChannel`; reasons 2, 5, and 6; VCID 63 only on a fixed-length channel, Table 5-1 note 4). An error that turns one configured channel into another passes the lookup; only the FECF detects it, checked before the TFDF Header (`uslp-val-029`). With the FECF, every single-bit error and every double-bit error within 4 095 octets is rejected (VER-064). Without it (the FECF is a managed parameter of the Physical Channel, Table 5-1), and always for truncated frames, which have no FECF (Annex D1.2.2 note 1), such an error is not detected by Aldis |
| Assumptions | `asm:ASM-4` |
| Proposals | — |
| Claims | `ac:AC-1.4`; `ac:AC-5.5` |

### HZ-USLP-04 A bit error in a truncated frame is accepted: it has no FECF

| | |
|---|---|
| Category | `corrupt-accepted` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A truncated frame (End of Frame Primary Header Flag `1`) with bit errors passes `decode`, `validate_frame`, and the delimiter. It has no FECF (Annex D1.2.2 note 1), so the only checks are those of its four-octet header and of the rule: `truncated::parse` (`crates/aldis-uslp/src/truncated.rs:45`–`89`) looks the SCID, VCID, and MAP ID up, takes the length from the VC, and passes the TFDF Header to the rule check of USLP-016. Nothing detects an error in the UPID, the TFDZ, the Security Header, or the Security Trailer; an error in the rule that leaves a rule of `011`–`111`; an error in the Source-or-Destination Identifier, which no check reads; or an error that turns the SCID, VCID, or MAP ID into another configured one (for another VC, of the same truncated length when `validate_frame` is used, since a different length is reason 13) |
| Cause | A channel error in the data unit that the coding sublayer below did not detect (`asm:ASM-4`) |
| Mission effect | The user data of a short frame, which Annex D1.3.1 note 3 has processed as expedited user data (a short telecommand, such as a direct-from-Earth command, Annex D1.1), changed in any octet, or delivered on another VC or MAP, or as the other source or destination |
| Requirements | USLP-002, USLP-012, USLP-014, USLP-015, USLP-016 |
| Evidence | `vector:uslp-trunc-002`; `vector:uslp-trunc-016`; `vector:uslp-trunc-017`; `vector:uslp-trunc-018`; `vector:uslp-trunc-020`; `vector:uslp-trunc-022`; `vector:uslp-trunc-023`; `vector:uslp-val-003`; `vector:uslp-val-015`; `vector:uslp-val-032`; `vector:uslp-tsdls-009`; `vector:uslp-tsdls-010`; `test:crates/aldis-uslp/src/truncated.rs::decodes_truncated_frames`; `test:crates/aldis-uslp/src/truncated.rs::validating_truncated_frames`; `test:crates/aldis-uslp/src/truncated.rs::truncated_errors`; `test:crates/aldis-uslp/tests/vectors.rs::truncated`; `test:crates/aldis-uslp/tests/vectors.rs::truncated_sdls_layout`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_truncated`; `fuzz:uslp_truncated`; `test:crates/aldis-uslp/tests/bit_flips.rs::every_single_bit_error_of_a_truncated_frame_is_as_pinned`; `vector:uslp-val-041`; `vector:uslp-trunc-043` |
| Residual risk | The Blue Book's own: a truncated frame has no error control field, so its protection is the coding sublayer's code and, with SDLS, the 16-octet Security Trailer of Annex D1.2.3, which Aldis does not verify (the SDLS hooks are outside M2, `asm:CTX-1`). The evidence above shows which errors the structural checks reject (a TFVN, an SCID, VCID, or MAP ID that is not configured, a rule `000`–`010`, a VC without a truncated length) and not that the rest is accepted; `every_single_bit_error_of_a_truncated_frame_is_as_pinned` (P-USLP-06) flips every bit of truncated frames of 6, 12, 32, and 28 octets (with SDLS) and pins the outcome of each in both modes against a model written from Annex D1.3 and the order of checks; the module documentation of `bit_flips.rs` still names the FECF suite only. The opposite flag error, a truncated frame read as non-truncated, is rejected with reason 18, since the VC carries only truncated frames (OI-48; `hz:HZ-USLP-02`), before any length check |
| Assumptions | `asm:ASM-4`; `asm:CTX-1`; `asm:ASM-11` |
| Proposals | `proposal:P-USLP-06`; `proposal:P-USLP-09` |
| Claims | `ac:AC-1.4`; `ac:AC-5.5` |

### HZ-USLP-05 Fill Data added after randomization is delimited as a truncated frame

| | |
|---|---|
| Category | `corrupt-accepted` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The octets after the last frame of a data unit are read as a valid truncated header: `next_frame` yields them as a `Frame`, and `validate_frame` accepts it. The delimiter takes the rest of a unit for `Fill` unless it starts with a valid header (`crates/aldis-uslp/src/delimit.rs:70`–`86`, OI-44 item 3): at least four octets with TFVN `1100`, and either the End of Frame Primary Header Flag `1` or seven octets that declare a plausible frame. The Fill Data pattern `01010101` has TFVN `0101` and never qualifies; pseudo-random fill can. The BCH code gives at most six fill octets, too few for a non-truncated header, so only a truncated header can be formed, and only for a VC whose truncated frame length is 6, the shortest (Annex D1.3.2 note 2). A scratch test run by this analysis (not kept), with VC 1 of SCID 0x155 and a truncated frame length of 6, gave `Frame([C0 15 50 21 EB 3C])` from `next_frame` and `Ok` from `validate_frame`. For a longer truncated length the six octets are an `IncompleteFrame` event, discarded |
| Cause | The BCH code with randomization, with the fill added at the sending end after randomization: the receiver derandomizes the fill without it having been randomized, and the Blue Book says nothing then guarantees that the result fails the validation checks (732.1 §4.3.11.2.4 note 2) |
| Mission effect | Up to six random octets delivered as the user data of a truncated frame on a VC and MAP chosen by chance. For one SCID, one VC, and one MAP the chance is about 3 in 10^10 per six-octet fill (TFVN 1/16, SCID 1/65 536, VCID 1/64, MAP ID 1/16, flag 1/2, rule 5/8), so small but not zero, and nothing in Aldis detects it |
| Requirements | USLP-002, USLP-014, USLP-015 |
| Evidence | `vector:uslp-delim-004`; `vector:uslp-delim-008`; `vector:uslp-delim-009`; `vector:uslp-delim-010`; `vector:uslp-delim-011`; `vector:uslp-delim-012`; `vector:uslp-delim-013`; `test:crates/aldis-uslp/src/delimit.rs::frames_incomplete_and_fill`; `test:crates/aldis-uslp/src/delimit.rs::what_is_fill_and_what_has_no_length`; `test:crates/aldis-uslp/tests/properties.rs::delimiter_returns_the_frames`; `test:crates/aldis-uslp/tests/vectors.rs::delimit_multiple_frames_with_fill`; `fuzz:uslp_truncated`; `vector:uslp-delim-023`; `vector:uslp-delim-024`; `vector:uslp-delim-025`; `vector:uslp-delim-026` |
| Residual risk | Aldis cannot tell such fill from a truncated frame: a truncated frame has no FECF, and six octets leave nothing else to check. The Blue Book concedes the case (the note above), and the seven-octet rule narrows it, since six octets cannot make a non-truncated frame. The evidence covers fill of `0x55` and short or unmatched headers, and `uslp-delim-023` builds the case above: a frame and six octets of fill, not `0x55`, that read as the truncated header of VC 8 (truncated frame length 6), yielded as a second `Frame` that `uslp-delim-026` shows `validate` accepting; `uslp-delim-024` shows the same octets where no VC has that length (`no_truncated_length`), and `uslp-delim-025` six octets that read as the start of a non-truncated header (fill, since fewer than seven). P-USLP-01 (fill longer than the code can give) does not reach it, six octets being within the BCH bound. The integrator can close it at the sending end, by adding fill before randomization, or by giving no VC a truncated length of 6 on a BCH link with randomization (P-USLP-08). The integrator closes it with `asm:ASM-12`; the vectors above pin it |
| Assumptions | `asm:ASM-4`; `asm:ASM-12` |
| Proposals | `proposal:P-USLP-07`; `proposal:P-USLP-08` |
| Claims | `ac:AC-1.4`; `ac:AC-5.5` |

### HZ-USLP-06 A valid frame is rejected because the receiver's parameters differ from the sender's

| | |
|---|---|
| Category | `valid-rejected` |
| Status | `residual` |
| Severity | S3 |
| Failure mode | `decode`, `validate_frame`, or the delimiter rejects a frame that the sender built correctly for its own managed parameters. Every case is reported with a stable reason: (a) the VCF Count Length for the frame's QoS is not the receiver's (reason 8, `LengthMismatch` naming `vcf_count_len`, in both modes, USLP-004, OI-46); (b) on a fixed-length channel VCID 63 is valid without an entry, with the defaults of no VCF Count, no OCF, and no SDLS (`crates/aldis-uslp/src/config.rs:582`–`595`), so the OID frames of a sender that counts them or carries an OCF are rejected, with reasons 8 or 9, until the receiver gives an entry for VCID 63; on a variable-length channel VCID 63 is never valid (reason 5); (c) a truncated header on a VC the receiver gives no truncated length (reason 7); (d) the OCF Flag against the VC's OCF use (reason 9), the rule against the VC's frame type (reason 15), and the frame length against the Physical Channel's (reasons 10 and 11); (e) `decode` on a fixed-length channel takes exactly one frame, so a frame in a longer buffer is rejected (API-012, reason 13 in `validate_frame`) |
| Cause | Managed parameters (Tables 5-1 to 5-4) set differently at the receiver than at the sender: a stale or mistyped configuration, a sender that reads §4.1.2.11.4 ("invariant" per VC) differently from Table 5-3 (OI-46, resolved per QoS), or a caller that hands `decode` a buffer longer than the fixed-length frame |
| Mission effect | The frames are lost, and the loss is reported with a reason code (S3). For OID frames, which carry only idle data, the loss costs nothing at the receiver except the reports |
| Requirements | USLP-004, USLP-009, USLP-014, USLP-016, API-012, GEN-060 |
| Evidence | `vector:uslp-vcf-042`; `vector:uslp-vcf-043`; `vector:uslp-vcf-044`; `vector:uslp-val-016`; `vector:uslp-val-017`; `vector:uslp-val-018`; `vector:uslp-val-019`; `vector:uslp-val-020`; `vector:uslp-val-030`; `vector:uslp-val-031`; `vector:uslp-val-037`; `vector:uslp-val-013`; `vector:uslp-val-015`; `vector:uslp-pc-010`; `vector:uslp-oid-042`; `vector:uslp-oid-054`; `vector:uslp-oid-056`; `vector:uslp-oid-057`; `vector:uslp-oid-060`; `vector:uslp-oid-062`; `vector:uslp-fixed000-029`; `vector:uslp-fixed000-030`; `test:crates/aldis-uslp/src/view.rs::header_errors_in_check_order`; `test:crates/aldis-uslp/src/config.rs::vcid_63_is_valid_without_an_entry_on_a_fixed_channel`; `test:crates/aldis-uslp/tests/restricted.rs::vc_tables_imply_vcid_63_only_on_fixed_channels`; `test:crates/aldis-uslp/tests/vectors.rs::validation_failures`; `test:crates/aldis-uslp/tests/vectors.rs::oid`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_validation_failures`; `test:crates/aldis-ffi/tests/uslp.rs::validation_reports_a_stable_reason` |
| Residual risk | Aldis checks a frame against the configuration it is given and cannot know the sender's. A receiver configured differently loses frames, each with its reason. No row of the assurance case names the obligation that the configuration equal the sender's (P-USLP-09) |
| Assumptions | `asm:ASM-8` |
| Proposals | `proposal:P-USLP-09` |
| Claims | `ac:AC-1.4`; `ac:AC-5.3` |

### HZ-USLP-07 The delimiter drops the rest of a data unit

| | |
|---|---|
| Category | `valid-rejected` |
| Status | `residual` |
| Severity | S3 |
| Failure mode | `next_frame` returns one item that takes all the octets not yet delimited (`IncompleteFrame`, `NoTruncatedLength`, or `Fill`; `crates/aldis-uslp/src/delimit.rs:93`–`112`), so every valid frame behind it in the same data unit is not delivered. Three ways to get there: (a) a truncated header whose SCID and VCID name no configured VC, or a VC without a truncated frame length (`delimit.rs:105`), because a truncated frame has no length field and nothing else tells where it ends (USLP-015, OI-35 item 8); (b) a header that declares more octets than remain (the discard of §4.3.11.2.3); (c) a Frame Length that is wrong, which `declared_len` (`delimit.rs:70`–`86`) reads on every channel, a fixed-length one included, so the frames after it are read from the wrong place (`hz:HZ-USLP-01`) |
| Cause | A truncated length configured differently at the receiver than at the sender, a VC missing at the receiver, or an error in a Frame Length field that the coding sublayer did not detect |
| Mission effect | Valid frames of the same data unit lost. Each loss is reported as an item with its octets, and the frames before it were delivered |
| Requirements | USLP-015, USLP-014 |
| Evidence | `vector:uslp-delim-003`; `vector:uslp-delim-006`; `vector:uslp-delim-007`; `vector:uslp-delim-008`; `test:crates/aldis-uslp/src/delimit.rs::frames_incomplete_and_fill`; `test:crates/aldis-uslp/src/delimit.rs::what_is_fill_and_what_has_no_length`; `test:crates/aldis-uslp/tests/properties.rs::delimiter_returns_the_frames`; `test:crates/aldis-uslp/tests/properties.rs::delimiter_covers_any_octets`; `test:crates/aldis-uslp/tests/vectors.rs::delimit_multiple_frames_with_fill`; `test:crates/aldis-ffi/tests/uslp.rs::the_delimiter_is_a_stateless_call`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_delimit_multiple_frames_with_fill`; `fuzz:uslp_truncated` |
| Residual risk | The procedure's own: it delimits by the Frame Length and, for truncated frames, by the configured length, and discards what it cannot place (§4.3.11.2.3–4). The delimiter takes the unit's whole remainder because it cannot find the next frame; a receiver that wants the frames behind a lost one needs a lower layer that delimits them. The data units of a lower layer that carries its own frame length (one frame per unit) do not need the delimiter |
| Assumptions | `asm:ASM-4` |
| Proposals | — |
| Claims | `ac:AC-1.4` |

### HZ-USLP-08 A frame is taken for an OID frame, or an OID frame for a frame of a data VC

| | |
|---|---|
| Category | `misrouting` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A bit error in the VCID turns a data VC into 63, or 63 into a data VC, and the frame is accepted as the other. On a fixed-length Physical Channel VCID 63 is valid without an entry (Table 5-2 note 2; `VcTable::vc`, `crates/aldis-uslp/src/config.rs:582`–`595`), so the lookup that rejects other unconfigured VCIDs accepts it. VCIDs 31, 47, 55, 59, 61, and 62 are one bit from 63. Data frame read as OID: strict mode rejects it by the OID header check (reason 17, `crates/aldis-uslp/src/view.rs:230`–`235`) because its MAP ID, rule, UPID, or Last Valid Octet Pointer differ, and by reasons 8 and 9 when its VC has a VCF Count or an OCF; lenient mode accepts it with the anomaly `OidHeaderMismatch`, and a consumer that discards VCID 63 as idle drops the data. OID read as a data frame: a VC with MAP 0 configured and a fixed-length TFDZ accepts the frame, which has rule `001` and UPID "Only Idle Data", and delivers the pseudo-noise data on that VC (a scratch test run by this analysis, not kept, accepted it in both modes with no anomaly) |
| Cause | A single-bit channel error in the VCID that the coding sublayer did not detect, on a fixed-length Physical Channel whose VC is one bit from 63 |
| Mission effect | User data silently discarded as idle data, or idle pseudo-noise data delivered to the consumer of a data VC |
| Requirements | USLP-009, USLP-014, USLP-016, API-031, API-032 |
| Evidence | `vector:uslp-oid-021`; `vector:uslp-oid-023`; `vector:uslp-oid-025`; `vector:uslp-oid-027`; `vector:uslp-oid-029`; `vector:uslp-oid-031`; `vector:uslp-oid-022`; `vector:uslp-oid-024`; `vector:uslp-oid-035`; `vector:uslp-oid-036`; `vector:uslp-oid-037`; `vector:uslp-oid-042`; `vector:uslp-oid-056`; `vector:uslp-oid-060`; `vector:uslp-oid-063`; `vector:uslp-val-013`; `vector:uslp-val-034`; `vector:uslp-val-035`; `test:crates/aldis-uslp/src/view.rs::oid_header_checks`; `test:crates/aldis-uslp/src/config.rs::vcid_63_is_valid_without_an_entry_on_a_fixed_channel`; `test:crates/aldis-uslp/tests/restricted.rs::vc_tables_imply_vcid_63_only_on_fixed_channels`; `test:crates/aldis-uslp/tests/bit_flips.rs::every_single_bit_error_of_the_short_frames_is_as_expected`; `test:crates/aldis-ffi/tests/uslp.rs::the_oid_writer_takes_a_generator_handle`; `job:bit-flips`; `test:crates/aldis-uslp/tests/bit_flips.rs::without_the_fecf_a_vcid_error_between_a_data_frame_and_an_oid_frame_is_pinned` |
| Residual risk | With the FECF every single-bit error and every double-bit error within 4 095 octets is rejected before the TFDF Header is read (VER-064), which includes these. Without it: the data-to-OID direction is rejected in strict mode and reported in lenient mode (`asm:ASM-5`); the OID-to-data direction is accepted in both, with the UPID "Only Idle Data" visible to a consumer that reads it. A truncated header cannot reach VCID 63, which has no truncated frame length (`uslp-oid-063`). `without_the_fecf_a_vcid_error_between_a_data_frame_and_an_oid_frame_is_pinned` (P-USLP-10) shows the no-FECF outcome on a fixed-length channel with VC 31: all six VCID errors of a data frame and of an OID frame, in both modes |
| Assumptions | `asm:ASM-4`; `asm:ASM-5` |
| Proposals | `proposal:P-USLP-10` |
| Claims | `ac:AC-1.4`; `ac:AC-5.2`; `ac:AC-5.5` |

### HZ-USLP-09 A flag error changes how the rest of the frame is read but not whether it passes

| | |
|---|---|
| Category | `wrong-value` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A bit error in the Source-or-Destination Identifier (bit 20), the Bypass/Sequence Control Flag (48), the Protocol Control Command Flag (49), or the OCF Flag (52) of a non-truncated frame gives a frame the decoder accepts with another meaning. A scratch test run by this analysis (not kept), on a variable-length channel without FECF, accepted all four from an expedited frame: the identifier read as the destination; the QoS as sequence-controlled (when both count lengths are equal); the frame as protocol control (rule `111` fits a variable-length VC); and, on a VC that allows an OCF, the last four TFDZ octets as an OCF (a TFDZ of 4 octets for 8). The checks that do catch flag errors: `01` flags (reason 3, from a flip of bit 48 of a protocol control frame or of bit 49 of a sequence-controlled one), a Bypass flip between QoS whose count lengths differ (reason 8), and an OCF Flag against a VC with no OCF or a required one (reason 9). The Protocol Control Command Flag also removes the Security Header and Trailer from the layout (`crates/aldis-uslp/src/view.rs:189`–`193`), so on an SDLS VC its flip moves the end of the TFDZ |
| Cause | A single-bit channel error in the flags that the coding sublayer did not detect, on a channel without FECF |
| Mission effect | User data processed as COP-1 control commands, or the reverse (`hz:HZ-USLP-10`); an expedited frame counted as sequence-controlled, so that the receiver's FARM applies its acceptance checks to it (§4.1.2.8.1.2) and its VCF Count is read against the wrong counter; four data octets read as an OCF, and so as a CLCW or a report; a frame read for the other end as source or destination |
| Requirements | USLP-001, USLP-004, USLP-013, USLP-014, VER-064 |
| Evidence | `vector:uslp-inv-006`; `vector:uslp-inv-011`; `vector:uslp-inv-012`; `vector:uslp-val-009`; `vector:uslp-val-016`; `vector:uslp-val-017`; `vector:uslp-val-018`; `vector:uslp-val-037`; `vector:uslp-pc-011`; `vector:uslp-pc-012`; `test:crates/aldis-uslp/src/view.rs::header_errors_in_check_order`; `test:crates/aldis-uslp/tests/bit_flips.rs::every_single_bit_error_of_the_short_frames_is_as_expected`; `test:crates/aldis-uslp/tests/bit_flips.rs::every_double_bit_error_of_the_short_frames_is_as_expected`; `nightly-test:crates/aldis-uslp/tests/bit_flips.rs::every_double_bit_error_within_the_guarantee_is_as_expected`; `job:bit-flips`; `test:crates/aldis-uslp/tests/bit_flips.rs::without_the_fecf_most_flag_errors_are_accepted` |
| Residual risk | With the FECF every single-bit error and every double-bit error within 4 095 octets is rejected (VER-064). Without it, or in a frame above 4 095 octets whose two errors fall 32 767 bits apart (`hz:HZ-X-06`), these errors are accepted. The bit-flip suite uses a channel with the FECF, with count lengths 2 and 0 for the two QoS, so a Bypass flip is also caught there by reason 8; `without_the_fecf_most_flag_errors_are_accepted` (P-USLP-10) pins the no-FECF outcome in both modes on a channel with equal count lengths: bits 20, 48, 52 accepted with the new meaning, bit 49 accepted for an expedited frame and reason 3 for a sequence-controlled one, and bit 48 of a protocol control frame reason 3. A truncated frame has none of these flags and is always expedited user data (Annex D1.3.1 note 3) |
| Assumptions | `asm:ASM-4` |
| Proposals | `proposal:P-USLP-10` |
| Claims | `ac:AC-1.4`; `ac:AC-5.5` |

### HZ-USLP-10 A frame is routed as COP-1 control commands whose TFDZ is not, or the commands are read wrongly

| | |
|---|---|
| Category | `wrong-value` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The TFDZ of a protocol control frame (flags `11`) is decoded with the TC control-command decoder (USLP-013, TC-005) although it holds another protocol's data, or a COP-1 command is read as another. `UslpFrame::decode` accepts any UPID on a protocol control frame and exposes it (the flags give the kind, `crates/aldis-uslp/src/view.rs:124`, and `TfdfHeader::parse` reads the UPID); it does not require the COP-1 UPID `00001` that the SANA registry assigns to COP-1 Control Commands, and the frame kinds do not name a protocol. The TC decoder (`ControlCommand::parse`) reads an Unlock or a Set V(R) from exact octets and gives `None` for anything else, but it cannot tell COP-1 octets from other octets with the same pattern |
| Cause | A caller that decodes every protocol control TFDZ as COP-1 without reading the UPID (COP-P control commands and Proximity-1 SPDUs are carried under the same flag, and have their own UPIDs), or a channel error in the UPID that the FECF did not catch |
| Mission effect | A directive taken for a COP-1 Unlock or Set V(R) (clearing a lockout, or setting the receiver's expected sequence number) from data that is not one. In M2 no FARM-1 exists to act on it (COP-1 is outside the case, `asm:CTX-1`, and arrives with `aldis-cop1` in M3) |
| Requirements | USLP-013, USLP-007, TC-005 |
| Evidence | `test:crates/aldis-uslp/tests/protocol_control.rs::cop1_commands_round_trip_through_the_tc_decoder`; `test:crates/aldis-uslp/tests/protocol_control.rs::the_cop1_vectors_carry_tc_control_commands`; `test:crates/aldis-uslp/tests/vectors.rs::protocol_control_cop1`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_protocol_control_cop1`; `vector:uslp-pc-001`; `vector:uslp-pc-002`; `vector:uslp-pc-003`; `vector:uslp-pc-004`; `vector:uslp-pc-009`; `vector:uslp-pc-013` |
| Residual risk | Aldis exposes the flags, the UPID, and the octets; deciding that a protocol control TFDZ is COP-1 is the engine's, and in M2 the caller's. An M3 engine should act on a protocol control frame as COP-1 only when its UPID is COP-1; no requirement of [cop1.md](../../standards/cop1.md) or [04-engines.md](../../04-engines.md) names the UPID. USLP-013 shows only that valid COP-1 octets decode, not that other protocols' octets are kept from the decoder |
| Assumptions | `asm:CTX-1` |
| Proposals | — |
| Claims | `ac:AC-1.4` |

### HZ-USLP-11 A decoder reads a field from the wrong octets

| | |
|---|---|
| Category | `wrong-value` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A decoder accepts a valid frame and reports a field, a pointer class, or a slice that differs from what the sender wrote. After the 7-octet Primary Header every offset depends on flags and parameters at once: the VCF Count Length (0–7 octets, per QoS), the Insert Zone (fixed-length channels), the Security Header and Trailer (per VC or per MAP, none for protocol control frames, fixed 6 and 16 octets for a truncated frame), the 1- or 3-octet TFDF Header (by the rule), the OCF Flag, and the FECF |
| Cause | A defect in offset arithmetic, in the pointer classification (`PacketStartsAt`, `LastValidOctet`, and `0xFFFF`, USLP-006), or in a combination of parameters that no test reached |
| Mission effect | A packet, an SDU, an OCF, or a count read from the wrong octets, with a valid FECF (the CRC covers the frame, not Aldis's reading of it) |
| Requirements | USLP-003, USLP-004, USLP-005, USLP-006, USLP-007, API-010, API-020, GEN-053, VER-021 |
| Evidence | `test:crates/aldis-uslp/tests/vectors.rs::fixed_packets_rule000`; `test:crates/aldis-uslp/tests/vectors.rs::fixed_mapa_rules001_010`; `test:crates/aldis-uslp/tests/vectors.rs::variable_rules011_111`; `test:crates/aldis-uslp/tests/vectors.rs::vcf_count_lengths_0_to_7`; `test:crates/aldis-uslp/tests/vectors.rs::insert_zone`; `test:crates/aldis-uslp/tests/vectors.rs::truncated_sdls_layout`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_fixed_packets_rule000`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_variable_rules011_111`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_vcf_count_lengths_0_to_7`; `test:crates/aldis-uslp/tests/properties.rs::encode_then_decode_round_trips`; `test:crates/aldis-uslp/tests/properties.rs::decoder_never_panics`; `fuzz:uslp_roundtrip`; `fuzz:uslp_decode`; `file:test-vectors/models/uslp_frame.py`; `record:TV-023`; `file:docs/verification/derivations/uslp-frame.md`; `file:docs/verification/reviews/m2-vectors.md`; `vector:uslp-fixed000-001`; `vector:uslp-vcf-035`; `vector:uslp-var-011`; `vector:uslp-fixed000-011` |
| Residual risk | The vectors' expected values come from an independent model written without the Rust code, and a second review, by agents that did not open the code, read the clauses again (7 USLP findings, each with its resolution in the record). Both follow the project's readings of what the Blue Book leaves open (OI-35, OI-44, OI-46), which are decisions, not clause text, and a misreading shared by the model, the code, and both reviewers would pass. The model and its author are recorded in the vectors' provenance (TV-023); the differential test against the native C++ implementation (VER-053) belongs to track C, not M2 |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-1.4`; `ac:AC-1.6`; `ac:AC-7.3` |

### HZ-USLP-12 A writer's frame carries zeroed Security Header and Trailer

| | |
|---|---|
| Category | `nonconformant-output` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | For a VC or MAP with SDLS, `encode`, `UslpFrameWriter::begin`, and `encode_truncated` write the Security Header and Trailer as zeros for the SDLS layer to fill (API-072; `start` with `zero`, `crates/aldis-uslp/src/writer.rs:471`–`510`). The frame is well-formed, with a correct FECF over the zeros, so every Aldis decoder accepts it, but it carries no protection until the fields are filled. `encode` returns the finished frame at once, with its FECF computed over the zeros: a caller that then patches the fields in `out` leaves a FECF that no longer matches, and the receiver rejects the frame. Only the two-phase writer lets the caller fill the fields before the FECF is written |
| Cause | A caller that does not apply SDLS between `begin` and `finish`, that uses the one-shot `encode` for an SDLS VC, or that sends frames before an SDLS layer exists (the engines' security hooks are outside M2, API-074, `asm:CTX-1`) |
| Mission effect | A frame with zero authentication data is sent: a receiver that does not verify SDLS accepts it as if it were protected, which bypasses the security the VC was configured for; a receiver that verifies it rejects it, and the data is lost |
| Requirements | API-070, API-072, API-075, USLP-010, USLP-012 |
| Evidence | `vector:uslp-var-011`; `vector:uslp-var-012`; `vector:uslp-fixed000-011`; `vector:uslp-fixed000-013`; `vector:uslp-tsdls-001`; `vector:uslp-tsdls-005`; `vector:uslp-pc-001`; `test:crates/aldis-uslp/src/writer.rs::variable_frames_with_sdls_shrink`; `test:crates/aldis-uslp/src/writer.rs::resume_keeps_what_the_caller_filled`; `test:crates/aldis-uslp/tests/properties.rs::encode_then_decode_round_trips`; `test:crates/aldis-ffi/tests/uslp.rs::the_writer_leaves_the_sdls_fields_to_the_caller` |
| Residual risk | The tests show the placeholders are zeros and the two-phase writer leaves the caller's octets in place. They cannot show that a caller filled them: a zero MAC is a legal value to the writers, and Aldis M2 has no SDLS hook to call. No row of the assurance case says that the caller must fill the fields (P-USLP-09) |
| Assumptions | `asm:ASM-9` |
| Proposals | `proposal:P-USLP-09` |
| Claims | `ac:AC-1.6` |

### HZ-USLP-13 An encoder writes octets that the Blue Book does not specify

| | |
|---|---|
| Category | `nonconformant-output` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | An encoder returns success with a frame other than the Blue Book specifies for its arguments: a wrong Frame Length (length minus one), VCF Count Length or count, OCF Flag, TFDF Header (1 or 3 octets, pointer), Reserve Spares, FECF, Insert Zone or Security placement, truncated header, or OID header (rule `001`, UPID "Only Idle Data", Last Valid Octet Pointer at the last octet); or an OID frame whose pseudo-noise octets are not the next ones of the sequence |
| Cause | A defect in a derived field the writer computes itself (API-021), or in the order of the fields |
| Mission effect | A frame the receiver rejects (a wrong FECF or length), or reads as other data (a wrong header), or OID data that departs from the mandatory sequence |
| Requirements | USLP-009, USLP-010, USLP-012, USLP-016, API-020, API-021, GEN-052, CMN-020, CMN-022 |
| Evidence | `test:crates/aldis-uslp/src/writer.rs::fixed_frames_round_trip`; `test:crates/aldis-uslp/src/writer.rs::frames_without_fecf`; `test:crates/aldis-uslp/src/writer.rs::truncated_frames`; `test:crates/aldis-uslp/src/writer.rs::oid_frames`; `test:crates/aldis-uslp/src/writer.rs::oid_frames_from_the_galois_form`; `test:crates/aldis-uslp/tests/properties.rs::encode_then_decode_round_trips`; `test:crates/aldis-uslp/tests/properties.rs::writers_agree`; `test:crates/aldis-uslp/tests/vectors.rs::oid`; `test:crates/aldis-uslp/tests/vectors.rs::truncated`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_oid`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_truncated`; `test:crates/aldis-ffi/tests/uslp.rs::the_oid_writer_takes_a_generator_handle`; `vector:uslp-fixed000-001`; `vector:uslp-oid-001`; `vector:uslp-oid-003`; `vector:uslp-trunc-001`; `fuzz:uslp_roundtrip`; `fuzz:oid_forms`; `file:test-vectors/models/uslp_frame.py`; `record:TV-023`; `file:docs/verification/derivations/uslp-frame.md` |
| Residual risk | As for `hz:HZ-USLP-11`: the expected octets come from a model written without the Rust code and from one frame worked by hand with its FECF (`uslp-fixed000-001`), and share the project's readings of what the Blue Book leaves open. The OID writer takes whatever generator the caller gives it; the Blue Book requires one generator that is initialized at start-up and never restarted (§4.1.4.1.10.1–2), and two generators, or a restart, repeat the sequence without any Aldis check seeing it (`hz:HZ-USLP-15`) |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-1.4`; `ac:AC-1.6`; `ac:AC-7.3` |

### HZ-USLP-14 A writer writes outside its frame

| | |
|---|---|
| Category | `memory` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | `encode`, `UslpFrameWriter::begin`, `resume`, and `finish`, `encode_truncated`, or `encode_oid` write outside `out`, or outside the frame they were given room for. The offsets come from lengths that reach 65 536 octets for a frame, 65 535 for a Security Trailer, and 7 octets for a VCF Count, and `finish` moves the Security Trailer to follow a TFDZ shorter than the reservation (`crates/aldis-uslp/src/writer.rs:573`) |
| Cause | A defect in the offset arithmetic; an `out` shorter than the reservation; in the C ABI, an `out` that overlaps an input buffer |
| Mission effect | Memory outside the frame overwritten, or a frame left half written |
| Requirements | API-022, GEN-014, GEN-020, CABI-022, USLP-010 |
| Evidence | `test:crates/aldis-uslp/src/writer.rs::writer_length_errors`; `test:crates/aldis-uslp/src/writer.rs::writer_errors_in_order`; `test:crates/aldis-uslp/src/writer.rs::resume_keeps_what_the_caller_filled`; `test:crates/aldis-uslp/src/writer.rs::variable_frames_with_sdls_shrink`; `test:crates/aldis-uslp/tests/properties.rs::writers_agree`; `vector:uslp-inv-040`; `vector:uslp-var-019`; `vector:uslp-var-021`; `vector:uslp-var-024`; `test:crates/aldis-ffi/tests/uslp.rs::content_and_fields_are_checked_as_arguments`; `fuzz:uslp_roundtrip`; `record:VER-001`; `check:clippy`; `check:exerciser-thumbv7em-none-eabihf`; `test:crates/aldis-uslp/tests/writer_bounds.rs::encode_writes_only_the_frame`; `test:crates/aldis-uslp/tests/writer_bounds.rs::begin_and_finish_write_only_the_reservation`; `test:crates/aldis-uslp/tests/writer_bounds.rs::a_failed_begin_or_finish_changes_nothing_of_the_frame` |
| Residual risk | Every write goes through a slice taken with `get_mut`, and `start` takes `out.get_mut(..plan.total)` and returns `BufferTooSmall` before it writes anything (`writer.rs:477`–`479`), so a short `out` is left as it was (API-022); unchecked indexing is denied (`record:VER-001`). The C ABI rejects an `out` that overlaps an input (`crates/aldis-ffi/tests/uslp.rs:1122`–`1132`). What stays: `finish` leaves the octets after the frame unspecified, among them the Security Trailer as it stood before the move (`writer.rs:540`–`542`), so a caller that sends the reservation or `cap` octets instead of the returned length sends them; and the C ABI under Miri has not been run as a check (`df:DF-7`) |
| Assumptions | `asm:ASM-6` |
| Proposals | — |
| Claims | `ac:AC-4.3`; `ac:AC-4.4`; `ac:AC-6.2` |

### HZ-USLP-15 State kept across writer calls becomes inconsistent

| | |
|---|---|
| Category | `state-corruption` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | (a) A rejected OID encode advances the pseudo-noise generator, so the sequence of later OID frames skips octets (the sequence continues across frames, never restarted, §4.1.4.1.10.1–2; CMN-021). (b) A frame that was begun and never finished, or whose `finish` failed, is mistaken for a finished frame |
| Cause | A write that fills the TFDZ from the generator before every check has passed; a caller that sends the buffer of an unfinished writer |
| Mission effect | (a) An OID stream that breaks the mandatory sequence, which no receiver checks. (b) A frame with a Primary Header that is not final and no FECF accepted as valid |
| Requirements | USLP-009, API-022, API-023, GEN-042, CMN-021, CMN-023 |
| Evidence | `test:crates/aldis-uslp/src/writer.rs::oid_frames`; `test:crates/aldis-uslp/src/writer.rs::oid_frames_from_the_galois_form`; `test:crates/aldis-uslp/src/writer.rs::variable_frames_with_sdls_shrink`; `test:crates/aldis-uslp/tests/properties.rs::writers_agree`; `test:crates/aldis-ffi/tests/uslp.rs::the_oid_writer_takes_a_generator_handle`; `test:crates/aldis-uslp/src/delimit.rs::what_is_fill_and_what_has_no_length`; `vector:uslp-inv-004`; `vector:uslp-oid-003` |
| Residual risk | (a) `oid_frame` fills the TFDZ only after `Plan::new` and `start` have passed every check (`crates/aldis-uslp/src/writer.rs:328`–`346`), and `finish` has no error left for such a plan, whose pointer is the last octet (`oid_tfdf`); the tests compare the generator before and after each error. Continuity across frames needs one generator for the physical channel, which the caller owns (CMN-021): two generators, or a new one after a restart, repeat the sequence, and a receiver discards OID data. An upset in a generator's register changes the sequence silently (`hz:HZ-X-01`). (b) The unfinished header has TFVN `1111`, which every decoder rejects with `UnsupportedVersion` and the delimiter takes for fill (`crates/aldis-uslp/src/delimit.rs:71`) |
| Assumptions | `asm:ASM-1` |
| Proposals | — |
| Claims | `ac:AC-5.4`; `ac:AC-1.6` |

### HZ-USLP-16 A stateless finish is given other arguments than its begin

| | |
|---|---|
| Category | `abi-misuse` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | `aldis_uslp_writer_finish` keeps no state from `aldis_uslp_writer_begin` (c-abi.md §11), so it calls `UslpFrameWriter::resume` with the arguments it is given and trusts that they are the ones of `begin` (`crates/aldis-ffi/src/uslp.rs:1195`–`1226`; `crates/aldis-uslp/src/writer.rs:455`–`466`). `resume` derives the layout again, then `finish` writes the header, the OCF, and a new FECF around the Security Header, TFDZ, and Security Trailer that the caller filled at `begin`'s offsets. When an argument changes the layout (the MAP ID of a MAP with other SDLS lengths, the Protocol Control flag, the QoS and so the VCF Count Length, the OCF, the Insert Zone, or the rule, so 1 or 3 octets of TFDF Header), the filled octets are read at other offsets and the frame is still well-formed. A scratch test run by this analysis (not kept) began on a MAP with SDLS 6/16, filled the Security Header with `51`, the Trailer with `52`, and the TFDZ with `07 08 09`, and finished on a MAP with no SDLS: `finish` returned a 13-octet frame with a valid FECF whose TFDZ was `51 51 51`. The Rust `resume` is public and has the same property; `begin` followed by `finish` on the same writer does not, since the writer holds the plan |
| Cause | A caller that changes an argument between the two calls. Since OI-50 `resume` compares the unfinished Primary Header (with the reserved total in the Frame Length field), the Insert Zone, and the TFDF Header with those `begin` writes for the arguments, and fails with `InvalidState` (`UslpFrameWriter::resume`, `start` with `write` false, `crates/aldis-uslp/src/writer.rs:467`–`530`) |
| Mission effect | A frame with a valid FECF and other content than the caller placed, sent as valid: data in the place of the security fields, or security octets in the place of data |
| Requirements | API-022, API-023, API-072, CABI-022, USLP-010 |
| Evidence | `test:crates/aldis-uslp/src/writer.rs::resume_keeps_what_the_caller_filled`; `test:crates/aldis-ffi/tests/uslp.rs::the_writer_leaves_the_sdls_fields_to_the_caller`; `test:crates/aldis-ffi/tests/uslp.rs::content_and_fields_are_checked_as_arguments`; `test:crates/aldis-uslp/src/writer.rs::resume_rejects_another_frame_than_begun`; `test:crates/aldis-uslp/src/writer.rs::resume_rejects_a_buffer_never_begun`; `test:crates/aldis-ffi/tests/uslp.rs::finish_checks_the_buffer_was_begun_with_these_arguments`; `test:crates/aldis-uslp/tests/writer_bounds.rs::begin_and_finish_write_only_the_reservation`; `test:crates/aldis-uslp/tests/writer_bounds.rs::a_failed_begin_or_finish_changes_nothing_of_the_frame` |
| Residual risk | `resume` and `aldis_uslp_writer_finish` return `InvalidState`, leaving the buffer unchanged, for a buffer whose Primary Header (SCID, VCID, MAP ID, flags, OCF Flag, VCF Count Length, VCF Count, and the reserved Frame Length), Insert Zone, or TFDF Header differs from what `begin` writes for the arguments, and for a buffer never begun or already finished (`resume_rejects_another_frame_than_begun`, `resume_rejects_a_buffer_never_begun`; the C call in `finish_checks_the_buffer_was_begun_with_these_arguments`). Another MAP with other SDLS lengths, the QoS, the OCF, the Insert Zone, the rule, and a different `max_user_len` (the reserved length) change the compared octets and are caught. Not compared: the Security Header and Trailer and the TFDZ, which the caller fills, and a changed security layout whose header, Insert Zone, and TFDF Header are identical, which gives a frame with a valid FECF whose fields lie at the other layout's offsets (OI-50 states this residual). The caller finishes with the arguments it began with (`asm:ASM-9`) |
| Assumptions | `asm:ASM-9` |
| Proposals | `proposal:P-USLP-09`; `proposal:P-USLP-11` |
| Claims | `ac:AC-6.2` |

### HZ-USLP-17 A C caller reads a truncated frame as a sequence-controlled one

| | |
|---|---|
| Category | `abi-misuse` |
| Status | `controlled` |
| Severity | S1 |
| Failure mode | In the C view `aldis_uslp_frame_t` the members that only a full frame has are 0 for a truncated frame, and 0 is a valid value of each: `vcf_count_len` is 0, and `ocf_flag` is false. Before OI-52 `kind` was `ALDIS_USLP_KIND_SEQUENCE_CONTROLLED` too, so a C caller that read `kind` without testing `truncated` took a truncated frame for sequence-controlled user data, where Annex D1.3.1 note 3 and USLP-002 make it expedited user data, for which the FARM's acceptance checks are bypassed. The Rust API has no such alias: `UslpFrame::Truncated` is a variant of its own and has no `kind()` |
| Cause | A C caller that reads `kind` and the other full-frame members without `truncated` |
| Mission effect | A short telecommand handled by the sequence-controlled path of the receiving procedure, which expects a VCF Count the frame does not have: it is discarded or counted out of sequence instead of delivered |
| Requirements | USLP-002, CABI-012 |
| Evidence | `test:crates/aldis-ffi/tests/uslp.rs::full_and_truncated_frames_cross_as_one_tagged_view`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_truncated`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_truncated_sdls_layout`; `vector:uslp-trunc-002`; `vector:uslp-trunc-056`; `fuzz:ffi_decode` |
| Residual risk | `view` sets `kind` to `ALDIS_USLP_KIND_EXPEDITED` for a truncated frame (`crates/aldis-ffi/src/uslp.rs:808`; OI-52), so a caller that ignores `truncated` still routes the frame as the Blue Book says; `vcf_count_len` and `ocf_flag` are 0 and false, which is true of a frame with no count and no OCF. `truncated` is the first member and the header documents the others. Proposal P-USLP-12 was accepted as OI-52 and its row is removed |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-6.1` |

### HZ-USLP-18 A deviation is accepted in strict mode and reported nowhere

| | |
|---|---|
| Category | `hidden-deviation` |
| Status | `residual` |
| Severity | S4 |
| Failure mode | A deviation from a SHALL that the octets and the configuration let a strict decoder see is accepted with no error and no anomaly: (a) a UPID that the SANA registry does not assign decodes as `Upid::Other(n)` in both modes, in full and truncated frames (USLP-007, OI-33 item 4), although 732.1 §4.1.4.2.3.3 requires a registered UPID; the C view's `upid_assigned` is the only sign (`crates/aldis-ffi/src/uslp.rs:778`); (b) truncated and non-truncated frames in one data unit, which Annex D1.2.5 forbids within one CLTU, are delimited without comment (`uslp-delim-005`; USLP-011 is a sending rule and not checked on reception, OI-35 item 8). API-031 names two exceptions to strict rejection (a CLCW's COP in Effect, and OID data), and neither is (a) or (b), so API-031 and USLP-007 disagree. Lenient mode adds only `ReserveSparesSet` and `OidHeaderMismatch` (`crates/aldis-uslp/src/anomaly.rs`); a truncated frame never reports an anomaly (`crates/aldis-uslp/src/truncated.rs:149`–`152`), and the rule `111` and UPID "Mission Specific Information-1" that Annex D1.4.1.2 gives in notes are not checked (OI-35 item 1) |
| Cause | The maintainer's decisions of OI-33 item 4 (a registry that grows after the snapshot would make a strict decoder reject UPIDs assigned since) and of OI-35 items 1 and 8, which API-031's list does not record |
| Mission effect | A frame with an unregistered UPID treated as ordinary data of its MAP; a data unit that mixes the two frame kinds accepted, where the Blue Book says none should exist. The result is the one the decisions chose, and the deviation is unreported |
| Requirements | USLP-007, USLP-011, USLP-015, API-031, API-032 |
| Evidence | `vector:uslp-fixed000-021`; `vector:uslp-fixed000-022`; `vector:uslp-fixed000-023`; `vector:uslp-trunc-013`; `vector:uslp-trunc-014`; `vector:uslp-trunc-015`; `vector:uslp-delim-005`; `test:crates/aldis-uslp/tests/restricted.rs::unassigned_upid_is_what_sana_leaves`; `test:crates/aldis-uslp/src/upid.rs::the_values_the_code_names`; `test:crates/aldis-uslp/src/anomaly.rs::documented_bit_indices`; `test:crates/aldis-ffi/tests/uslp.rs::content_and_fields_are_checked_as_arguments`; `ci:sana`; `job:sana-registries`; `vector:uslp-delim-017`; `test:crates/aldis-uslp/src/delimit.rs::a_vc_with_a_truncated_length_carries_only_truncated_frames` |
| Residual risk | A caller sees (a) through `Upid::Other` and `upid_assigned`, and (b) not at all, since the delimiter reports no mix; a mission that must reject either has to check it. The nightly job compares the snapshot with the live registry (M2 plan D13), so a UPID assigned later shows as a difference, but until the snapshot is imported again it decodes as `Other`. The two deviations are listed exceptions of API-031 (this branch); the mix on one VC is not accepted: a VC with a truncated frame length carries only truncated frames, and the delimiter discards the rest of the unit at a non-truncated header on it (OI-48, `hz:HZ-USLP-02`, `uslp-delim-017`), so only a mix on different VCs is delimited without comment |
| Assumptions | `asm:ASM-5` |
| Proposals | `proposal:P-USLP-13` |
| Claims | `ac:AC-5.1`; `ac:AC-5.2` |

## Proposals

| ID | Kind | Proposal | Status |
|---|---|---|---|
| P-USLP-01 | `oi` | Draft OI, "USLP fill longer than the coding can produce": as P-TC-01, for USLP-015. 732.1 §4.3.11.2.4 NOTE 2 bounds the fill by the code: six octets (BCH), seven ((128,64) LDPC), 31 ((512,256) LDPC). Options: (a) `next_frame` takes the bound as a parameter and reports a longer rest as a distinct item, still discarded, with its own C ABI code; (b) keep USLP-015 and state the residual in [uslp-frame.md](../../standards/uslp-frame.md) §3.2. Recommendation (a), decided together with P-TC-01 so that TC and USLP stay alike (M2 plan D17). Maintainer: a NOTE, and a C ABI value. `hz:HZ-USLP-01` | `accepted` OI-47, implemented in M3 |
| P-USLP-02 | `test` | A test in `crates/aldis-uslp/tests/bit_flips.rs`, the twin of `decode_may_accept_a_frame_length_error_as_a_shorter_frame` in `crates/aldis-tc/tests/bit_flips.rs`: a variable-length frame whose data hold the CRC of a shorter prefix, so that a Frame Length error makes `decode` accept the prefix and `validate_frame` reject it with reason 13. An agent's decision | `accepted` `m2/hazard-controls` |
| P-USLP-03 | `doc` | State in [uslp-frame.md](../../standards/uslp-frame.md) §3.1 and §3.2, as P-TC-02 for TC, that check (d) cannot see a Frame Length error in a frame the delimiter yields, and that with no FECF a shortened frame is accepted. The coordinator's | `accepted` this branch, uslp-frame.md §3.1, §3.2 |
| P-USLP-06 | `test` | A test in `crates/aldis-uslp/tests/bit_flips.rs` for the truncated frames that the suite leaves out (module docs, lines 6–7): a variable-length VC with a truncated frame length (6, 12, 32, and 28 with SDLS), every single-bit error of the frame through `validate_frame` and `decode`, with the outcome of each bit pinned: rejected for a TFVN error, an SCID, VCID, or MAP ID that names no configured channel, and rule bits that give `000`–`010`; accepted for the Source-or-Destination Identifier, the UPID, the TFDZ, and the security fields. It records the residual of `hz:HZ-USLP-04` as the Blue Book states it, and would show if a later change made a check looser. An agent's decision | `accepted` `m2/hazard-controls` |
| P-USLP-07 | `test` | A vector in `test-vectors/uslp/delimit_multiple_frames_with_fill.json`: a data unit of one frame followed by six octets of non-`0x55` fill that form the truncated header of a VC whose truncated frame length is 6, expected as a `Frame` item that `validate` accepts, so that `hz:HZ-USLP-05` is pinned and a later change to the delimiter's valid-header rule cannot remove or widen it unseen. The expected values come from the model, written blind (TV-023). New vector cases are an agent's decision | `accepted` `m2/hazard-controls` |
| P-USLP-08 | `doc` | State the case of `hz:HZ-USLP-05` in [uslp-frame.md](../../standards/uslp-frame.md) §3.2 (after the sentence on the Fill Data pattern), and extend the obligation of ASM-4 in the assurance case: on a BCH link with randomization the sending end adds Fill Data before randomization, so that the receiver sees `01010101`, or the mission gives no VC a truncated frame length of 6. The note is the coordinator's; the choice between the two obligations is the maintainer's if it limits how VCs are configured | `accepted` this branch, ASM-12 and uslp-frame.md §3.2 |
| P-USLP-09 | `doc` | Draft rows for the assumptions table of [assurance-case.md](../assurance-case.md) (that file's, not this one's): ASM-8, "The receiver's managed parameters equal the sender's" (Tables 5-1 to 5-4: VCF Count Length per QoS, OCF use, truncated frame lengths, the entry for VCID 63, the frame lengths), with the obligation to derive both configurations from one mission database (`hz:HZ-USLP-06`; one row would serve TM, TC, and AOS too); ASM-9, "The caller fills the security fields of a writer's frame before the FECF is written, and finishes a stateless writer with the arguments it began with" (API-072, API-074), with the obligation to use the two-phase writer and never `encode` for an SDLS VC (`hz:HZ-USLP-12`, `hz:HZ-USLP-16`); and, in ASM-4, that a truncated frame has no FECF and the mission either relies on the SDLS Security Trailer or accepts the residual of `hz:HZ-USLP-04`. The coordinator's | `accepted` this branch, ASM-8, ASM-9, ASM-11 |
| P-USLP-10 | `test` | A test in `crates/aldis-uslp/tests/bit_flips.rs` or `properties.rs` for a channel without FECF, with equal VCF Count Lengths for the two QoS: the single-bit errors of the Source-or-Destination Identifier, the Bypass/Sequence Control Flag, the Protocol Control Command Flag, and the OCF Flag of a frame on a VC that allows an OCF (`hz:HZ-USLP-09`), and of the VCID of a frame of VC 31 and of an OID frame on a fixed-length channel (`hz:HZ-USLP-08`), in both modes, with the outcomes pinned (observed by this analysis in scratch tests: all accepted but the VCID of the data frame, which strict mode rejects with reason 17 and lenient mode accepts with `OidHeaderMismatch`). An agent's decision | `accepted` `m2/hazard-controls` |
| P-USLP-11 | `requirement` | Draft requirement for [uslp-frame.md](../../standards/uslp-frame.md) §3.3 and c-abi.md §11: "`UslpFrameWriter::resume` SHALL compare the Primary Header octets, the Insert Zone, and the TFDF Header that its arguments give with those of the unfinished frame in `out`, except the VCF Count and the pointer that `finish` sets, and SHALL return `InvalidField` and leave `out` unchanged if they differ." For the reserved TFDZ length to be covered, `begin` writes the reserved frame length in the unfinished header's Frame Length field (now 1, `crates/aldis-uslp/src/writer.rs:480`–`487`). Options: (a) the check, with a test that changes each layout argument between `begin` and `finish`; (b) keep the behaviour, state in c-abi.md §11 that the arguments must be identical, and add the obligation to the assurance case (P-USLP-09). Recommendation (a): the evidence is in the buffer, the cost is a few comparisons, and it turns a frame that is valid and wrong into an error. A change of C ABI behaviour and of the unfinished header, so the maintainer's (decision rights §2.4) | `accepted` OI-50, `m2/hazard-controls` |
| P-USLP-13 | `requirement` | Draft amendment of API-031 in [03-api-model.md](../../03-api-model.md): add to its listed exceptions "a USLP UPID that the SANA registry does not assign (USLP-007, OI-33 item 4)" and "truncated and non-truncated USLP frames in one data unit (USLP-011, OI-35 item 8)", which the codec accepts in strict mode by decisions already taken (`hz:HZ-USLP-18`); HZ-X-08 would then list them too. Alternative: report the mixed unit as an event of the delimiter. Recommendation: the list, since only the list is behind the decisions. Maintainer: API-031 is a requirement | `accepted` this branch, API-031 |
