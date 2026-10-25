# Hazards: AOS Transfer Frame

Failure modes of `aldis-aos` and its C ABI (`aldis_aos_*`), in the format of [hazards.md](../hazards.md) §1.7. Standard: [aos-frame.md](../../standards/aos-frame.md), 732.0-B-5 with EC1, and Issue 4 (732.0-B-4) for `Issue4Compatible`. The failure modes common to all codecs (panics, loops, stack, the C ABI, the CRC-16, and the FHEC's miscorrection, `hz:HZ-X-07`) are in [hazards.md](../hazards.md) §3 and are not repeated here.

## Coverage

| Category | Hazards | State |
|---|---|---|
| `corrupt-accepted` | — | TODO |
| `valid-rejected` | — | TODO |
| `misrouting` | HZ-AOS-01 | done |
| `truncation` | — | TODO |
| `wrong-value` | — | TODO |
| `nonconformant-output` | — | TODO |
| `unbounded-time` | — | TODO |
| `memory` | — | TODO |
| `panic` | — | TODO |
| `state-corruption` | — | TODO |
| `abi-misuse` | — | TODO |
| `hidden-deviation` | — | TODO |

## Hazards

### HZ-AOS-01 A frame is delivered on another spacecraft or virtual channel

| | |
|---|---|
| Category | `misrouting` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | `AosFrame::decode` accepts a frame and reports a Spacecraft ID or VCID other than the one it was sent with |
| Cause | Channel errors in the Spacecraft ID (bits 2–9, and 42–43 in `Issue5`) or the VCID (bits 10–15), all within the FHEC's symbols (AOS-013), that the FHEC does not correct, or miscorrects (three or more symbol errors, `hz:HZ-X-07`), into a channel that is configured; a channel that is not configured is rejected |
| Mission effect | Telemetry of one virtual channel, or of another spacecraft, delivered to the consumer of another |
| Requirements | AOS-010, AOS-013, AOS-014, VER-064 |
| Evidence | `vector:aos-inv-dec-007`; `vector:aos-inv-dec-008`; `vector:aos-oid-dec-002`; `vector:aos-scid-dec-004`; `vector:aos-fhec-c2-002`; `vector:aos-fhecf-dec-002-s`; `vector:aos-fhec-mis-001`; `test:crates/aldis-aos/tests/vectors.rs::invalid`; `test:crates/aldis-aos/tests/vectors.rs::fhec_frame`; `test:crates/aldis-aos/tests/vectors.rs::fhec_correct_2_symbols`; `test:crates/aldis-aos/tests/bit_flips.rs::every_single_bit_error_of_the_short_frames_is_corrected_or_rejected`; `test:crates/aldis-aos/tests/bit_flips.rs::every_double_bit_error_of_the_short_frames_is_corrected_or_rejected`; `fuzz:aos_fhec_model`; `fuzz:aos_decode` |
| Residual risk | As in TC and USLP, and unlike TM's `decode` (`hz:HZ-TM-01`), decoding always takes the channel table, so an SCID or VCID that is not configured is `UnknownChannel` (AOS-010; `aos-inv-dec-007`, `-008`). With the FHEC configured, up to two symbol errors in the SCID and VCID are corrected before the lookup (AOS-014; `aos-fhec-c2-002` corrects both symbols of octet 1, the SCID and VCID bits); three or more can be miscorrected into another configured channel (`aos-fhec-mis-001`). The FECF, checked over the corrected header, then rejects the frame (AOS-014) unless the CRC also misses the difference. On a Physical Channel with neither the FHEC nor the FECF (both managed parameters, [aos-frame.md](../../standards/aos-frame.md) §3), an error that names another configured channel is not detected by Aldis |
| Assumptions | `asm:ASM-4` |
| Proposals | — |
| Claims | `ac:AC-1.3`; `ac:AC-5.5` |

## Proposals

| ID | Kind | Proposal | Status |
|---|---|---|---|
