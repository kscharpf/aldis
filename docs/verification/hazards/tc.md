# Hazards: TC Transfer Frame and CLCW

Failure modes of `aldis-tc` and its C ABI (`aldis_tc_*`, `aldis_clcw_*`), in the format of [hazards.md](../hazards.md) §1.7. Standard: [tc-frame.md](../../standards/tc-frame.md), 232.0-B-4 with Cor. 1. The failure modes common to all codecs (panics, loops, stack, the C ABI, the CRC-16) are in [hazards.md](../hazards.md) §3 and are not repeated here.

## Coverage

| Category | Hazards | State |
|---|---|---|
| `corrupt-accepted` | — | TODO |
| `valid-rejected` | — | TODO |
| `misrouting` | HZ-TC-02 | done |
| `truncation` | HZ-TC-01 | done |
| `wrong-value` | — | TODO |
| `nonconformant-output` | — | TODO |
| `unbounded-time` | — | TODO |
| `memory` | — | TODO |
| `panic` | — | TODO |
| `state-corruption` | — | TODO |
| `abi-misuse` | — | TODO |
| `hidden-deviation` | — | TODO |

## Hazards

### HZ-TC-01 A bit error in the Frame Length field makes the decoder accept a shorter frame

| | |
|---|---|
| Category | `truncation` |
| Status | `gap` |
| Severity | S1 |
| Failure mode | The Frame Length field (bits 22–31, 232.0 §4.1.2.7) declares a length L′ shorter than the frame's length L. `TcFrame::decode` takes the first L′ octets as the frame, reports `len()` = L′, and leaves the rest of the input unread (TC-001, API-011). The view is a valid frame whose data field lost its last L − L′ octets, and perhaps its FECF |
| Cause | A channel error in the Frame Length field that the coding sublayer below did not detect, so that the octets reach Aldis as "clean" (232.0 §4.4.9.2.3) |
| Mission effect | A shortened Frame Data Unit passed up as valid: a telecommand segment or packet missing its end, or a Control Command read from the wrong octets |
| Requirements | TC-001, TC-006, TC-020, TC-021, API-011, VER-064 |
| Evidence | `test:crates/aldis-tc/tests/bit_flips.rs::every_single_bit_error_is_rejected`; `test:crates/aldis-tc/tests/bit_flips.rs::every_double_bit_error_of_the_short_frames_is_rejected`; `nightly-test:crates/aldis-tc/tests/bit_flips.rs::every_double_bit_error_of_the_longest_frame_is_rejected`; `test:crates/aldis-tc/tests/bit_flips.rs::decode_may_accept_a_frame_length_error_as_a_shorter_frame`; `vector:tc-val-011`; `vector:tc-val-012`; `vector:tc-ad-dec-004`; `vector:tc-delimit-003`; `vector:tc-delimit-004`; `test:crates/aldis-tc/tests/vectors.rs::validation_failures`; `test:crates/aldis-tc/tests/vectors.rs::delimit_multiple_frames_with_fill`; `test:crates/aldis-ffi/tests/tc.rs::validation_reports_the_reason`; `test:crates/aldis-ffi/tests/vectors.rs::tc_validation_failures`; `fuzz:tc_decode`; `fuzz:tc_delimit`; `job:bit-flips` |
| Residual risk | See the analysis below. In short: `validate_frame` (TC-021 check (d)) rejects the frame only when the octets it is given are bounded by something other than the Frame Length field. Frames from the delimiter (TC-020) are bounded by that same field, so check (d) passes them; the FECF (TC-006) is then the only control, and it is not a CRC guarantee here, because the error moves the end of the codeword. With no FECF configured, a shortened frame from the delimiter is accepted, and its tail is discarded as fill or read as the next frame |
| Assumptions | `asm:ASM-2`; `asm:ASM-4` |
| Proposals | `proposal:P-TC-01`; `proposal:P-TC-02`; `proposal:P-TC-03` |
| Claims | `ac:AC-1.2`; `ac:AC-5.5` |

**Analysis.** The plan states this hazard as "a bit error in the TC Frame Length field makes `decode` accept a shorter frame (API-011), controlled by `validate_frame` (TC-021)" ([M2 plan](../../milestones/m2-frame-codecs.md) §3, WP-8). That holds under a condition the plan does not state. Where the frame's octets come from decides what each control sees.

1. **`decode` on a longer input** (API-011). `decode` must accept octets after a self-delimiting frame, so the frame ends where the corrupted field says. `tc-ad-dec-004` pins the behaviour for a correct field: a frame followed by three octets, of which exactly the frame is consumed.
   - With the FECF configured, `decode` checks the CRC over the first L′ − 2 octets against octets L′ − 2 and L′ − 1, which are data, not the FECF. The CRC's guarantee of VER-064 covers errors within a codeword of fixed length, and this error changes the length, so it does not apply. The prefix passes exactly when those two data octets equal the CRC of the octets before them. `decode_may_accept_a_frame_length_error_as_a_shorter_frame` builds such a frame and shows `decode` accepting it as 32 octets of a 64-octet frame (`crates/aldis-tc/tests/bit_flips.rs:209`–`240`; it also shows `validate_frame` rejecting the same octets with reason 8).
   - With no FECF, `decode` accepts the shortened frame for any L′ the other header checks allow (at least the header, a Segment Header when the VC has one, and one data octet).
   - The bit-flip suite therefore asserts, for errors in the Frame Length field, only that `decode` never returns a frame of the input's whole length (`crates/aldis-tc/tests/bit_flips.rs:15`–`23`, `rejected` at `:155`).
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
| Evidence | `vector:tc-dec-err-009`; `vector:tc-dec-err-010`; `vector:tc-seg-dec-err-001`; `vector:tc-val-004`; `vector:tc-val-008`; `vector:tc-val-015`; `vector:tc-val-018`; `vector:tc-val-021`; `vector:tc-dec-err-014`; `test:crates/aldis-tc/src/view.rs::header_errors_in_check_order`; `test:crates/aldis-tc/tests/vectors.rs::validation_failures`; `test:crates/aldis-tc/tests/vectors.rs::decode_errors`; `test:crates/aldis-tc/tests/vectors.rs::segment_header`; `test:crates/aldis-tc/tests/bit_flips.rs::every_single_bit_error_is_rejected`; `test:crates/aldis-ffi/tests/tc.rs::validation_reports_the_reason`; `fuzz:tc_decode` |
| Residual risk | The decoder and the validator look the channel up in the `VcLookup` and reject an SCID, VCID, or MAP ID that is not configured (`UnknownChannel`; reasons 2, 5, and 10). An error that turns one configured channel into another passes the lookup; only the FECF detects it, and it is checked before the Segment Header is read, so a MAP ID error is reason 9, not 10 ([tc-frame.md](../../standards/tc-frame.md) §4.1; `tc-val-021`). With the FECF configured, every single-bit and double-bit error is rejected (VER-064). Without it ("Presence of Frame Error Control", 232.0 Table 5-1), such an error is not detected by Aldis; only the coding sublayer protects the frame |
| Assumptions | `asm:ASM-4` |
| Proposals | — |
| Claims | `ac:AC-1.2`; `ac:AC-5.5` |

## Proposals

| ID | Kind | Proposal | Status |
|---|---|---|---|
| P-TC-01 | `oi` | Draft OI, "TC fill longer than the coding can produce": 232.0 §4.4.9.2.6 a–b treats every octet after the last delimited frame as fill, and TC-020 does the same; NOTE 2 of §4.4.9.2.6 says that at most six octets of fill occur with the present codeblock. Options: (a) `next_frame` takes the longest fill the coding sublayer appends (a caller parameter: six for the BCH codeblock of 231.0) and reports a longer rest as a distinct item, still discarded, with its own C ABI item code; (b) keep TC-020 and state the residual in [tc-frame.md](../../standards/tc-frame.md) §4.2. Recommendation (a): it reports a frame shortened by a Frame Length error whose leftover octets do not start a valid header, whenever those octets and the real fill exceed the bound: every shortening by seven octets or more, so every single-bit error in bits 22–28 of the field that lowers the length, at no cost to a valid data unit; a shortening by one, two, or four octets (bits 29–31) is reported only when the real fill makes up the difference. Maintainer: the bound is in a NOTE ([decision rights](../../process/decision-rights.md) §2.1), and the item is a new C ABI value (§2.4). `hz:HZ-TC-01` | `proposed` |
| P-TC-02 | `doc` | State in [tc-frame.md](../../standards/tc-frame.md) §4.1 and §4.2 that check (d) cannot see a Frame Length error in a frame the delimiter yields, because the delimiter reads the same field, so that the FECF (TC-006) is then the only control, and that with no FECF a shortened frame is accepted. Wording that changes no behaviour (§2.2, the coordinator's) | `proposed` |
| P-TC-03 | `test` | A test in `crates/aldis-tc/tests/bit_flips.rs` that delimits a data unit holding a frame with a Frame Length error, on a channel without the FECF, and shows `next_frame` yielding the shortened frame and `validate_frame` accepting it, so that the residual of `hz:HZ-TC-01` is pinned and a change to it (P-TC-01) shows. An agent's decision | `proposed` |
