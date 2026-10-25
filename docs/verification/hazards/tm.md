# Hazards: TM Transfer Frame

Failure modes of `aldis-tm` and its C ABI (`aldis_tm_*`), in the format of [hazards.md](../hazards.md) §1.7. Standard: [tm-frame.md](../../standards/tm-frame.md), 132.0-B-3. The failure modes common to all codecs (panics, loops, stack, the C ABI, the CRC-16) are in [hazards.md](../hazards.md) §3 and are not repeated here.

## Coverage

| Category | Hazards | State |
|---|---|---|
| `corrupt-accepted` | — | TODO |
| `valid-rejected` | — | TODO |
| `misrouting` | HZ-TM-01 | done |
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

### HZ-TM-01 A frame is delivered on another spacecraft or virtual channel

| | |
|---|---|
| Category | `misrouting` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A TM frame is accepted, and its view reports a Spacecraft ID or VCID other than the one it was sent with, or one the Master Channel does not configure |
| Cause | (1) A channel error in the Spacecraft ID (bits 2–11) or the VCID (bits 12–14) that the coding sublayer did not detect. (2) A caller that decodes with `TmFrame::decode`, which looks no channel up (`crates/aldis-tm/src/view.rs:37`–`50`: its errors are those of `decode_with` "except `UnknownChannel`"), and does not call `validate` (TM-011) |
| Mission effect | Telemetry of one virtual channel, or of another spacecraft, delivered to the consumer of another |
| Requirements | TM-005, TM-011, API-070, VER-064 |
| Evidence | `vector:tm-inv-val-001`; `vector:tm-inv-val-002`; `vector:tm-sec-dec-004`; `vector:tm-sec-dec-005`; `vector:tm-inv-crc-002`; `test:crates/aldis-tm/src/validate.rs::channel_and_length_mismatches`; `test:crates/aldis-tm/tests/vectors.rs::invalid`; `test:crates/aldis-tm/tests/vectors.rs::security_layout`; `test:crates/aldis-tm/tests/bit_flips.rs::every_single_bit_error_is_rejected`; `test:crates/aldis-tm/tests/bit_flips.rs::every_double_bit_error_of_the_short_frames_is_rejected`; `fuzz:tm_decode` |
| Residual risk | `validate` checks the SCID and the VCID against the Master Channel (TM-011; `tm-inv-val-001`, `-002`), and `decode_with` and `decode_unchecked` look the channel up (`UnknownChannel`; `tm-sec-dec-004`, `-005`). `decode` alone checks neither, so the check is the caller's to make. An error that turns one configured channel into another passes both; only the FECF detects it, and it is checked before any field is read (TM-005; `tm-inv-crc-002`). With the FECF, every single-bit and double-bit error of a TM frame is rejected (VER-064). Without it (an optional managed parameter of the Physical Channel, TM-001), such an error is not detected by Aldis; only the coding sublayer protects the frame |
| Assumptions | `asm:ASM-2`; `asm:ASM-4` |
| Proposals | — |
| Claims | `ac:AC-1.1`; `ac:AC-5.5` |

## Proposals

| ID | Kind | Proposal | Status |
|---|---|---|---|
