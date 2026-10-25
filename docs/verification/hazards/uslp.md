# Hazards: USLP Transfer Frame

Failure modes of `aldis-uslp` and its C ABI (`aldis_uslp_*`), in the format of [hazards.md](../hazards.md) §1.7. Standard: [uslp-frame.md](../../standards/uslp-frame.md), 732.1-B-3. The failure modes common to all codecs (panics, loops, stack, the C ABI, the CRC-16) are in [hazards.md](../hazards.md) §3 and are not repeated here.

## Coverage

| Category | Hazards | State |
|---|---|---|
| `corrupt-accepted` | HZ-USLP-02 | TODO |
| `valid-rejected` | — | TODO |
| `misrouting` | HZ-USLP-03 | done |
| `truncation` | HZ-USLP-01 | done |
| `wrong-value` | — | TODO |
| `nonconformant-output` | — | TODO |
| `unbounded-time` | — | TODO |
| `memory` | — | TODO |
| `panic` | — | TODO |
| `state-corruption` | — | TODO |
| `abi-misuse` | — | TODO |
| `hidden-deviation` | — | TODO |

`corrupt-accepted` stays TODO although HZ-USLP-02 is in it: the other ways a corrupt USLP frame can be accepted are not yet analysed.

## Hazards

### HZ-USLP-01 A bit error in the Frame Length field makes the decoder accept a shorter frame

| | |
|---|---|
| Category | `truncation` |
| Status | `gap` |
| Severity | S1 |
| Failure mode | On a variable-length Physical Channel, the Frame Length field (bits 32–47, 732.1 §4.1.2.7) declares a length L′ shorter than the frame's length L. `UslpFrame::decode` takes the first L′ octets as the frame, reports `len()` = L′, and leaves the rest unread (USLP-003, API-011). On a fixed-length channel the same error is rejected |
| Cause | A channel error in the Frame Length field that the coding sublayer below did not detect |
| Mission effect | A shortened TFDZ passed up as valid: a packet, MAPA_SDU, or octet stream missing its end, and an OCF or the FECF read from data octets |
| Requirements | USLP-003, USLP-014, USLP-015, API-011, API-012, VER-064 |
| Evidence | `vector:uslp-val-019`; `vector:uslp-inv-013`; `vector:uslp-val-025`; `vector:uslp-val-026`; `vector:uslp-inv-016`; `vector:uslp-delim-003`; `test:crates/aldis-uslp/src/view.rs::length_errors_on_a_variable_channel`; `test:crates/aldis-uslp/tests/vectors.rs::validation_failures`; `test:crates/aldis-uslp/tests/vectors.rs::delimit_multiple_frames_with_fill`; `test:crates/aldis-uslp/tests/bit_flips.rs::every_single_bit_error_of_the_short_frames_is_as_expected`; `test:crates/aldis-uslp/tests/bit_flips.rs::every_double_bit_error_of_the_short_frames_is_as_expected`; `nightly-test:crates/aldis-uslp/tests/bit_flips.rs::every_double_bit_error_within_the_guarantee_is_as_expected`; `test:crates/aldis-ffi/tests/uslp.rs::validation_reports_a_stable_reason`; `test:crates/aldis-ffi/tests/vectors.rs::uslp_validation_failures`; `fuzz:uslp_decode`; `job:bit-flips` |
| Residual risk | As for TC (`hz:HZ-TC-01`), on a variable-length channel: `validate_frame` (USLP-014 check (d)) rejects the frame only when its octets are bounded by something other than the Frame Length field, and frames from the delimiter (USLP-015) are bounded by that same field. The FECF is then the only control, and the CRC's guarantee does not cover an error that moves the end of the codeword. With no FECF, a shortened frame from the delimiter is accepted. On a fixed-length channel, `decode` and `validate_frame` require the configured length (USLP-003, API-012; reason 10) and reject every such error |
| Assumptions | `asm:ASM-2`; `asm:ASM-4` |
| Proposals | `proposal:P-USLP-01`; `proposal:P-USLP-02`; `proposal:P-USLP-03` |
| Claims | `ac:AC-1.4`; `ac:AC-5.5` |

**Analysis.** The reasoning of `hz:HZ-TC-01` applies, with these differences.

1. **Fixed-length channels are not exposed.** On a fixed-length Physical Channel the frame's length is a managed parameter, and `decode` and `validate_frame` both compare the Frame Length field with it: `FixedLengthMismatch`, reason 10 (`uslp-val-019`; `uslp-inv-013` when decoding). The bit-flip suite's 64-octet fixed-length frame shows every single-bit and double-bit error rejected by both ([module docs](../../../crates/aldis-uslp/tests/bit_flips.rs), lines 13–14).
2. **Variable-length channels behave as TC.** `decode` accepts octets after the frame (API-011), so a shorter L′ is read as the frame; the bit-flip suite asserts for errors in the field only that `decode` never returns a frame of the input's whole length (lines 15–19 of the same file). `validate_frame` given exactly the frame's octets rejects with reason 13, `LengthMismatch` (`uslp-val-025`, `uslp-val-026`). Unlike TC, no test builds a shortened frame that `decode` accepts; P-USLP-02 adds one.
3. **The delimiter reads the same field** (732.1 §4.3.11.2.2–4, USLP-015), so check (d) agrees with the shortened frame. The octets left of the real frame then start a valid header ([uslp-frame.md](../../standards/uslp-frame.md) §3.2: four octets with TFVN `1100` and either the End of Frame Primary Header Flag `1`, or seven octets and a Frame Length + 1 of at least 9 plus the VCF Count Length), giving an `IncompleteFrame` (`uslp-delim-003`), a `NoTruncatedLength`, or another `Frame`; or they are `Fill`, discarded (§4.3.11.2.4 a–b). NOTE 2 of §4.3.11.2.4 bounds the fill by the code below: up to six octets for the BCH code, seven for the (128,64) LDPC code, and 31 for the (512,256) LDPC code. The same note names a case in which even real fill can pass the validation checks: the BCH code with randomization, when the fill is added after randomization at the sending end.
4. **Frames are longer.** A variable-length USLP frame may be 65 536 octets (§4.1.2.7), so a shortening can remove far more data than in TC, and in a frame above 4 095 octets even the CRC's double-bit guarantee is gone (`hz:HZ-X-06`).

Integrator obligations: as for TC (`asm:ASM-2`, `asm:ASM-4`).

### HZ-USLP-02 A bit error in the End of Frame Primary Header Flag turns a frame into a truncated frame without an FECF

| | |
|---|---|
| Category | `corrupt-accepted` |
| Status | `gap` |
| Severity | S1 |
| Failure mode | The End of Frame Primary Header Flag (bit 31) of a non-truncated frame reads `1`, on a VC that has a Truncated Transfer Frame Length. `decode`, `validate_frame`, and the delimiter then treat the first 6–32 octets as a truncated frame (USLP-002, USLP-015): the length comes from the VC, the octet after the 4-octet header is read as the TFDF Header, and no FECF is checked, since a truncated frame has none (Annex D1.2.2 note 1). The truncated frame's TFDZ is made of the real frame's Frame Length, flags, VCF Count, and first data octets |
| Cause | A single-bit channel error in the flag that the coding sublayer did not detect, on a VC on which non-truncated frames are also sent: Aldis's writer `encode` accepts a VC that has a truncated frame length (`crates/aldis-uslp/src/writer.rs` checks the truncated length only in `encode_truncated`, at `:372`–`375`) |
| Mission effect | Header octets of a frame passed up as the user data of a short frame, on the same VC and MAP; the delimiter then continues inside the real frame, whose remaining octets may be read as further frames or discarded as fill |
| Requirements | USLP-002, USLP-014, USLP-015, USLP-016 |
| Evidence | `vector:uslp-val-015`; `vector:uslp-trunc-020`; `vector:uslp-inv-021`; `vector:uslp-trunc-016`; `vector:uslp-trunc-017`; `vector:uslp-trunc-018`; `vector:uslp-val-032`; `test:crates/aldis-uslp/src/truncated.rs::truncated_errors`; `test:crates/aldis-uslp/tests/vectors.rs::truncated`; `fuzz:uslp_truncated` |
| Residual risk | On a VC with no truncated frame length the flag error is rejected: `InvalidField` (`end_of_frame_primary_header_flag`), reason 7 (`uslp-val-015`, `uslp-trunc-020`). On a VC with one, the only check left is USLP-016's: the rule must be `011`–`111`. Without SDLS on that VC, the rule is read from the top three bits of the real frame's Frame Length field, which are `011` or more only for a Frame Length of at least `0x6000`, a frame of at least 24 577 octets: shorter frames are rejected with reason 15 (`uslp-trunc-016`…`018`, `uslp-val-032`), and longer ones are accepted. With SDLS on that VC, the TFDF Header is the eleventh octet (after the 6-octet Security Header, Annex D1.2.3), a VCF Count or data octet of the real frame, whose rule bits pass for five of their eight values; SDLS authentication, outside Aldis, is then the remaining control. The bit-flip suite does not show this: its only VC has no truncated frame length (`crates/aldis-uslp/tests/bit_flips.rs:80`–`101`, `VcParams::new` at `crates/aldis-uslp/src/config.rs:195`–`204`) |
| Assumptions | `asm:ASM-7` |
| Proposals | `proposal:P-USLP-04`; `proposal:P-USLP-05` |
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

## Proposals

| ID | Kind | Proposal | Status |
|---|---|---|---|
| P-USLP-01 | `oi` | Draft OI, "USLP fill longer than the coding can produce": as P-TC-01, for USLP-015. 732.1 §4.3.11.2.4 NOTE 2 bounds the fill by the code: six octets (BCH), seven ((128,64) LDPC), 31 ((512,256) LDPC). Options: (a) `next_frame` takes the bound as a parameter and reports a longer rest as a distinct item, still discarded, with its own C ABI code; (b) keep USLP-015 and state the residual in [uslp-frame.md](../../standards/uslp-frame.md) §3.2. Recommendation (a), decided together with P-TC-01 so that TC and USLP stay alike (M2 plan D17). Maintainer: a NOTE, and a C ABI value. `hz:HZ-USLP-01` | `proposed` |
| P-USLP-02 | `test` | A test in `crates/aldis-uslp/tests/bit_flips.rs`, the twin of `decode_may_accept_a_frame_length_error_as_a_shorter_frame` in `crates/aldis-tc/tests/bit_flips.rs`: a variable-length frame whose data hold the CRC of a shorter prefix, so that a Frame Length error makes `decode` accept the prefix and `validate_frame` reject it with reason 13. An agent's decision | `proposed` |
| P-USLP-03 | `doc` | State in [uslp-frame.md](../../standards/uslp-frame.md) §3.1 and §3.2, as P-TC-02 for TC, that check (d) cannot see a Frame Length error in a frame the delimiter yields, and that with no FECF a shortened frame is accepted. The coordinator's | `proposed` |
| P-USLP-04 | `oi` | Draft OI, "Does a VC with a Truncated Transfer Frame Length carry only truncated frames?" 732.1 Annex D1.2.1 allows truncated frames on a variable-length VC, and its NOTE says that all truncated frames of a VC have the same length; the NOTE to D1.2.4 recommends a different Security Association for the VCs of truncated frames than for the VCs of non-truncated ones, which reads as if each VC carries one kind. No normative text forbids both kinds on one VC. If a VC carries both, a single-bit error in the End of Frame Primary Header Flag can be accepted as a truncated frame without an FECF (`hz:HZ-USLP-02`). Options: (a) a VC with a truncated frame length carries only truncated frames: `encode`, `UslpFrameWriter`, and their C ABI calls reject it with `InvalidField` (`end_of_frame_primary_header_flag`), and `decode` and `validate_frame` reject a non-truncated frame on it with a new reason; (b) keep both, and state the residual and the integrator obligation (`asm:ASM-7`) in [uslp-frame.md](../../standards/uslp-frame.md). Recommendation (a): with it the error must also change the VCID to be accepted. Maintainer: the reading rests on notes ([decision rights](../../process/decision-rights.md) §2.1) and changes behaviour and the C ABI's reasons (§2.4) | `proposed` |
| P-USLP-05 | `test` | Add to `crates/aldis-uslp/tests/bit_flips.rs` a variable-length VC that has a truncated frame length, so that the End of Frame Primary Header Flag error of `hz:HZ-USLP-02` is exercised: rejected with reason 15 for frames below 24 577 octets without SDLS, and the outcome above that bound and with SDLS pinned. An agent's decision; its expected outcome changes if P-USLP-04 is accepted | `proposed` |
