# Hazards: AOS Transfer Frame

Failure modes of `aldis-aos` and its C ABI (`aldis_aos_*`), in the format of [hazards.md](../hazards.md) §1.7. Standard: [aos-frame.md](../../standards/aos-frame.md), 732.0-B-5 with EC1, and Issue 4 (732.0-B-4) for `Issue4Compatible`. The failure modes common to all codecs (panics, loops, stack, the C ABI, the CRC-16, and the FHEC's miscorrection, `hz:HZ-X-07`) are in [hazards.md](../hazards.md) §3 and are not repeated here. Clause numbers are those of 732.0-B-5 EC1 unless a document is named.

## Coverage

| Category | Hazards | State |
|---|---|---|
| `corrupt-accepted` | HZ-AOS-03 | done |
| `valid-rejected` | HZ-AOS-04, HZ-AOS-05, HZ-AOS-06 | done |
| `misrouting` | HZ-AOS-01, HZ-AOS-02 | done |
| `truncation` | HZ-AOS-07 | done |
| `wrong-value` | HZ-AOS-08, HZ-AOS-09, HZ-AOS-10 | done |
| `nonconformant-output` | HZ-AOS-11, HZ-AOS-12, HZ-AOS-13 | done |
| `unbounded-time` | — | n/a: AOS adds no loop, wait, or call pattern of its own to HZ-X-03; every loop is listed in the inspection record gen-022-023-aldis-aos.md, a constant (the FHEC), the caller's input length, or the capacity N of a VcTable |
| `memory` | HZ-AOS-14 | done |
| `panic` | — | n/a: AOS adds no panic path to HZ-X-02; its decoder and writer evidence (the aos_decode and aos_roundtrip fuzz targets, the decoders_never_panic property) backs the claims of HZ-X-02 and is cited there |
| `state-corruption` | HZ-AOS-15 | done |
| `abi-misuse` | HZ-AOS-16 | done |
| `hidden-deviation` | HZ-AOS-17 | done |

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
| Evidence | `vector:aos-inv-dec-007`; `vector:aos-inv-dec-008`; `vector:aos-oid-dec-002`; `vector:aos-scid-dec-004`; `vector:aos-fhec-c2-002`; `vector:aos-fhecf-dec-002-s`; `vector:aos-fhecf-dec-006`; `vector:aos-fhecf-dec-007`; `vector:aos-fhec-mis-001`; `test:crates/aldis-aos/tests/vectors.rs::invalid`; `test:crates/aldis-aos/tests/vectors.rs::fhec_frame`; `test:crates/aldis-aos/tests/vectors.rs::fhec_correct_2_symbols`; `test:crates/aldis-aos/tests/properties.rs::two_symbol_errors_are_corrected`; `test:crates/aldis-aos/tests/bit_flips.rs::every_single_bit_error_of_the_short_frames_is_corrected_or_rejected`; `test:crates/aldis-aos/tests/bit_flips.rs::every_double_bit_error_of_the_short_frames_is_corrected_or_rejected`; `fuzz:aos_fhec_model`; `fuzz:aos_decode` |
| Residual risk | As in TC and USLP, and unlike TM's `decode` (`hz:HZ-TM-01`), decoding always takes the channel table, so an SCID or VCID that is not configured is `UnknownChannel` (AOS-010; `aos-inv-dec-007`, `-008`). With the FHEC configured, up to two symbol errors in the SCID and VCID are corrected before the lookup (AOS-014; `aos-fhec-c2-002` corrects both symbols of octet 1, the SCID and VCID bits); three or more can be miscorrected into another configured channel (`aos-fhec-mis-001`). The FECF, checked over the corrected header, then rejects the frame (AOS-014; `aos-fhecf-dec-006`) unless the CRC also misses the difference. On a Physical Channel with neither the FHEC nor the FECF (both managed parameters, [aos-frame.md](../../standards/aos-frame.md) §3), an error that names another configured channel is not detected by Aldis; `aos-fhecf-dec-007` shows the miscorrected frame accepted as SCID 0x2EA, VC 37 when no FECF is configured |
| Assumptions | `asm:ASM-4` |
| Proposals | — |
| Claims | `ac:AC-1.3`; `ac:AC-5.5` |

### HZ-AOS-02 A frame of the other edition is delivered under a truncated Spacecraft ID

| | |
|---|---|
| Category | `misrouting` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A channel configured `Issue4Compatible` decodes, in lenient mode, a frame sent in `Issue5` whose Spacecraft ID is above 255, and reports the SCID without its two most significant bits: a frame of SCID 0x2A5 is delivered as SCID 0xA5 |
| Cause | The receiver's edition is not the sender's (a configuration error), and the caller decodes in lenient mode. In `Issue4Compatible` the header reads bits 42–43 as a reserved spare, not as SCID bits (`crates/aldis-aos/src/header.rs:278`; 732.0 §4.1.2.2.3.2 and its note 1 give the two editions). The FHEC is the same code in both editions (AOS-013), so it does not flag the mismatch |
| Mission effect | Telemetry of one spacecraft delivered to the consumer of another, when the mission configures both SCIDs with the same low eight bits and the VCID exists on both |
| Requirements | AOS-001, AOS-002, API-031, API-032 |
| Evidence | `vector:aos-i4-dec-004-s`; `vector:aos-i4-dec-004-l`; `vector:aos-i4-dec-007`; `vector:aos-i4-cfg-001`; `vector:aos-i4-cfg-002`; `vector:aos-scid-dec-001`; `vector:aos-scid-dec-004`; `test:crates/aldis-aos/src/view.rs::issue_4_spares`; `test:crates/aldis-aos/src/view.rs::strict_and_lenient_deviations`; `test:crates/aldis-aos/src/config.rs::edition_defaults_to_issue_5`; `test:crates/aldis-aos/src/config.rs::table_rejects_what_732_forbids`; `test:crates/aldis-aos/tests/vectors.rs::issue4_compat`; `fuzz:aos_decode` |
| Residual risk | Strict mode, the default (API-030), rejects the frame with `ReservedValue` (`aos_header_spare`, `aos-i4-dec-004-s`). Lenient mode accepts it, keeps the SCID's low eight bits, and sets `Issue4SpareSet`, flag bit 2 (`aos-i4-dec-004-l`; the same octets read in `Issue5` give SCID 0x2A5, `aos-i4-dec-007`); the caller must read the anomaly set. A frame whose SCID is below 256 has `00` in bits 42–43 and reads the same in both editions. The truncated SCID reaches a channel only if the mission configures it. The edition itself is a managed parameter the integrator sets (`proposal:P-AOS-01`) |
| Assumptions | `asm:ASM-5`; `asm:ASM-8` |
| Proposals | `proposal:P-AOS-01` |
| Claims | `ac:AC-1.3`; `ac:AC-5.2` |

### HZ-AOS-03 An error in the Virtual Channel Frame Count is accepted

| | |
|---|---|
| Category | `corrupt-accepted` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | `AosFrame::decode` accepts a frame whose VC Frame Count (bits 16–39) differs from the one sent, and `vc_frame_count()` reports the received value |
| Cause | A channel error in octets 2–4 of the Primary Header. The FHEC covers header bits 0–15 and 40–47 and not bits 16–39 (732.0 §4.1.2.6.1 note 2); its decoder neither sees nor alters an error there (AOS-014, last sentence), so a channel that has the FHEC and no FECF has a header that looks protected and is not protected in this field |
| Mission effect | A false gap or duplicate in the sequence of a virtual channel: the count is the individual accountability of each VC and the basis of packet extraction (732.0 §4.1.2.4 note). A consumer may drop good frames as duplicates, or report a loss and abandon a packet in progress |
| Requirements | AOS-013, AOS-014, VER-064 |
| Evidence | `vector:aos-fhec-c1-012`; `vector:aos-fhec-c1-013`; `vector:aos-fhec-enc-004`; `vector:aos-fhecf-dec-004`; `test:crates/aldis-aos/src/fhec.rs::errors_in_the_frame_count_are_neither_seen_nor_changed`; `test:crates/aldis-aos/tests/bit_flips.rs::every_single_bit_error_of_the_short_frames_is_corrected_or_rejected`; `test:crates/aldis-aos/tests/bit_flips.rs::every_double_bit_error_of_the_short_frames_is_corrected_or_rejected`; `fuzz:aos_fhec_model`; `fuzz:aos_decode` |
| Residual risk | With the FECF configured the CRC detects the error: every single-bit error, and every double-bit error in a frame of at most 4 095 octets (VER-064; `aos-fhecf-dec-004` is `CrcMismatch`), with the limits of `hz:HZ-X-06` above that. Without the FECF nothing detects it, and Aldis passes the received count on unaltered (`aos-fhec-c1-013`: no correction, octets passed on; `aos-fhec-c1-012`: a symbol corrected beside it, the count untouched); the same holds for the data field, which no FHEC covers. The Blue Book leaves the FECF optional (§4.1.6.1.2) |
| Assumptions | `asm:ASM-4` |
| Proposals | — |
| Claims | `ac:AC-1.3`; `ac:AC-5.5` |

### HZ-AOS-04 The Insert Zone of a frame the decoder rejects is not delivered

| | |
|---|---|
| Category | `valid-rejected` |
| Status | `gap` |
| Severity | S3 |
| Failure mode | `AosFrame::decode` returns an error and no view for a frame on a channel the receiver does not configure, or for a frame that fails the FHEC or the FECF, so the IN_SDU in its Insert Zone is not delivered. 732.0 §4.3.7.4 has the All Frames Reception Function extract the IN_SDUs of the incoming frames regardless of their GVCID, and allows it before the FECF is checked when the IN_SDU needs no error protection |
| Cause | The decoder rejects at the length, FHEC, FECF, version, and channel checks (`crates/aldis-aos/src/view.rs:112`–`122` and `:153`–`166`) before it builds a view. AOS-015 exposes the Insert Zone of a decoded frame only. The zone lies at a fixed place (after the Primary Header, for the configured length), so a caller can slice it from the input, but no function, requirement, or test says so |
| Mission effect | The periodic data of the Insert Service (an IN_SDU) is lost for every frame on a virtual channel or spacecraft that the receiver does not configure, for OID frames of another SCID, and for damaged frames |
| Requirements | AOS-015, AOS-010 |
| Evidence | `vector:aos-inv-dec-007`; `vector:aos-inv-dec-008`; `vector:aos-oid-dec-002`; `vector:aos-iz-dec-001`; `vector:aos-iz-dec-002`; `test:crates/aldis-aos/src/view.rs::unknown_channels`; `test:crates/aldis-aos/src/view.rs::insert_zone_security_and_ocf` |
| Residual risk | The cases above show the rejections and the Insert Zone of accepted frames; no evidence shows the Insert Service of 732.0 §4.3.7.4 for a rejected frame. The Insert Service belongs to the link engine of M4, which the case leaves out (`asm:CTX-1`), so no M2 requirement is violated; but the decoder's contract is where the engine's access to the zone must come from, and today the contract is silent |
| Assumptions | `asm:CTX-1` |
| Proposals | `proposal:P-AOS-02` |
| Claims | `ac:AC-1.3` |

### HZ-AOS-05 A B_PDU whose pointer names the last bit of its zone is rejected

| | |
|---|---|
| Category | `valid-rejected` |
| Status | `residual` |
| Severity | S3 |
| Failure mode | `AosFrame::decode`, `Bpdu::decode`, and the C ABI return `InvalidField` (`bitstream_data_pointer`), in both modes, for a Bitstream Data Pointer equal to the index of the last bit of the zone (`crates/aldis-aos/src/bpdu.rs:136`–`142`), where a receiver could read "every bit is valid" |
| Cause | A sender that writes the index of the last valid bit when the zone holds no idle data. 732.0 §4.1.4.3.3.2 defines the pointer as the index of the last valid user data bit; §4.1.4.3.3.3 requires all ones when the zone has no idle data. The Blue Book does not say that a receiver rejects the former. AOS-017 does, as the project's reading, which API-031 makes a strict error and API-032 does not let lenient mode accept (second review, [m2-vectors.md](../reviews/m2-vectors.md), AOS finding 1) |
| Mission effect | Every frame of such a sender is rejected, with a reported error: bitstream data is lost, not corrupted |
| Requirements | AOS-017, API-031, API-032 |
| Evidence | `vector:aos-bpdu-dec-005-s`; `vector:aos-bpdu-dec-005-l`; `vector:aos-bpdu-dec-002`; `vector:aos-bpdu-dec-003`; `vector:aos-bpdu-enc-004`; `test:crates/aldis-aos/src/bpdu.rs::classification`; `test:crates/aldis-aos/tests/restricted.rs::bitstream_pointer_classifies_every_raw_value`; `test:crates/aldis-aos/tests/vectors.rs::bpdu_pointers`; `file:docs/verification/reviews/m2-vectors.md` |
| Residual risk | The rejection is consistent with the SHALL of §4.1.4.3.3.3 and with API-031, and the writer cannot produce the pointer (`aos-bpdu-enc-004`). But the reading is the project's, found by the second review; the maintainer took it on 2026-10-25 (OI-53, resolved: keep the rejection, no behaviour change). A sender that follows the other reading cannot interoperate |
| Assumptions | — |
| Proposals | `proposal:P-AOS-03` |
| Claims | `ac:AC-1.3`; `ac:AC-5.1` |

### HZ-AOS-06 A bitstream channel the Blue Book allows is rejected as a configuration

| | |
|---|---|
| Category | `valid-rejected` |
| Status | `residual` |
| Severity | S3 |
| Failure mode | `VcTable::new` returns `InvalidConfig` for a virtual channel that carries B_PDUs on an `Issue5` physical channel of frames longer than 2 048 octets (`frame_len`), or whose Bitstream Data Zone is longer than 2 047 octets in either edition (`frame_data_field`) (`crates/aldis-aos/src/config.rs:455`–`470`) |
| Cause | AOS-018. The zone limit follows from the 14-bit pointer: it reserves all ones and all ones minus one, so a zone of 2 048 octets has a bit index, 16 382, that reads as "only idle data" (OI-35 item 7). The frame limit is the project's choice: 732.0 gives it in a NOTE to §4.1.4.3.3.4, not in a SHALL (second review, AOS finding 2) |
| Mission effect | A mission cannot configure a bitstream channel with larger frames; the error is returned when the table is built, not in flight |
| Requirements | AOS-018, AOS-017 |
| Evidence | `vector:aos-bpdu-cfg-001`; `vector:aos-bpdu-cfg-002`; `vector:aos-bpdu-cfg-003`; `vector:aos-bpdu-cfg-004`; `vector:aos-bpdu-cfg-005`; `test:crates/aldis-aos/src/config.rs::bitstream_limits`; `test:crates/aldis-aos/tests/restricted.rs::vc_tables_accept_exactly_the_configurable_channels`; `test:crates/aldis-aos/tests/vectors.rs::config_file`; `oi:OI-35` |
| Residual risk | The maintainer decided both limits (decision log, 2026-10-20, #61, `file:docs/process/decisions.md`), so the rejection is a recorded choice, not a defect. The residual is a mission whose B_PDU channel needs a longer frame or zone: it must carry the data in M_PDUs or VCA_SDUs, or use another implementation |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-1.3` |

### HZ-AOS-07 A frame is accepted at a length other than the configured one

| | |
|---|---|
| Category | `truncation` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | `AosFrame::decode` accepts input that is shorter or longer than the configured frame, or a stream is cut into frames at the wrong octets, so that a frame is delivered with part of its neighbour |
| Cause | The frame length is a managed parameter, constant throughout a Mission Phase (732.0 §4.1.1.2); the header has no length field. A channel error therefore cannot change the length the way it can in TC (`hz:HZ-TC-01`) or USLP. A lost or inserted octet in the stream below the frames shifts every later frame |
| Mission effect | Fields of two frames mixed in one view; a data unit, a count, or an FECF from the wrong octets |
| Requirements | API-012, API-013, AOS-010 |
| Evidence | `vector:aos-inv-dec-001`; `vector:aos-inv-dec-002`; `vector:aos-min-hdr-002`; `vector:aos-min-peek-001`; `vector:aos-fhecf-hdr-002`; `test:crates/aldis-aos/src/view.rs::length_fhec_and_fecf_come_first`; `test:crates/aldis-aos/src/view.rs::peek_length_checks_the_header_as_decode_does`; `test:crates/aldis-aos/tests/vectors.rs::invalid`; `fuzz:aos_decode` |
| Residual risk | `decode` compares the input length with the configured frame length before any other check and returns `LengthMismatch` (`frame_len`) for any other length (`crates/aldis-aos/src/view.rs:154`–`161`); `peek_length` returns the configured length for any prefix, so it never disagrees with `decode`. What Aldis cannot see is a slip below it: a stream cut one octet late hands `decode` a whole frame of the right length with the wrong octets. The FHEC, the FECF, the version, and the channel lookup reject nearly all such frames; with neither FHEC nor FECF only the version and the channel lookup do. Keeping frame boundaries is the synchronization and channel coding sublayer's (`asm:ASM-4`) |
| Assumptions | `asm:ASM-4` |
| Proposals | — |
| Claims | `ac:AC-1.3`; `ac:AC-5.1` |

### HZ-AOS-08 Fields are read at the wrong offsets because the configuration is not the sender's

| | |
|---|---|
| Category | `wrong-value` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | `AosFrame::decode` accepts a frame and reports an Insert Zone, security fields, data unit, or OCF cut from the wrong octets, or an FHEC read from data octets |
| Cause | A managed parameter of the receiver differs from the sender's: whether the physical channel has the FHEC (732.0 §4.1.2.6.2–4), the Insert Zone and its length (§4.1.3.2, §4.1.3.4.1), the FECF (§4.1.6.1.2); and per virtual channel the Data Field Content (§4.1.4.1.4), whether it has an OCF (§4.1.5.2; there is no OCF flag in the header, [aos-frame.md](../../standards/aos-frame.md) §3), and the Security Header and Trailer lengths (Table 6-1). The frame length is also managed, but a wrong one is `LengthMismatch` |
| Mission effect | Four octets of data read as an OCF, which the COP-1 sender acts on once FOP-1 exists (M3), or an OCF read as data; packets taken from the wrong octets; an Insert Zone with data octets in it |
| Requirements | AOS-010, AOS-013, AOS-015, AOS-017, API-070 |
| Evidence | `vector:aos-min-dec-007`; `vector:aos-iz-dec-001`; `vector:aos-iz-dec-002`; `vector:aos-sec-dec-001`; `vector:aos-fhecf-dec-001`; `test:crates/aldis-aos/src/view.rs::insert_zone_security_and_ocf`; `test:crates/aldis-aos/src/view.rs::channels_that_do_not_fit_the_physical_channel`; `test:crates/aldis-aos/src/config.rs::lookups_and_layouts`; `test:crates/aldis-ffi/tests/aos.rs::the_decoder_takes_the_master_channels_vcs` |
| Residual risk | Nothing in the frame says which layout it has, and the FECF covers the whole frame whatever its layout, so a frame with misplaced fields passes it. Only chance catches some: an M_PDU or B_PDU pointer outside its zone (AOS-017), a zone with no room (`Malformed`, `frame_data_field`). A receiver with the FHEC configured and a sender without one reads data octets 6 and 7 plus the header as a codeword: of the 65 536 syndromes, 1 + 10·15 + 45·15² = 10 276 are explained by at most two symbol errors (the code's distance is 5, AOS-014), so about one frame in six is "corrected" into a header and the rest are `HeaderUncorrectable`. A receiver without the FHEC and a sender with it reads the FHEC as the first two data octets. The integrator owes that Aldis is configured with the sender's managed parameters, for the Mission Phase; `asm:ASM-8` says so |
| Assumptions | `asm:ASM-8` |
| Proposals | `proposal:P-AOS-01` |
| Claims | `ac:AC-1.3` |

### HZ-AOS-09 An Issue 4 pointer is read with the Issue 5 layout, or the reverse

| | |
|---|---|
| Category | `wrong-value` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | `AosFrame::decode` accepts an M_PDU and reports a First Header Pointer that is not the one sent, so packets are taken from the wrong octet |
| Cause | The edition of the receiver is not the sender's. (a) An Issue 4 frame read as `Issue5`: the 11-bit pointer with five zero spare bits reads as the same number, but its reserved values `0x7FF` (no packet starts) and `0x7FE` (only idle data) are offsets 2 047 and 2 046 in `Issue5`; `PacketPointer::classify` accepts them as `StartsAt` when the Packet Zone is longer than the offset (`crates/aldis-aos/src/mpdu.rs:126`), which needs a Packet Zone of more than 2 047 octets for `0x7FF` and more than 2 046 for `0x7FE`; in a shorter zone they are `Malformed` (`aos-i4-dec-008`). (b) An Issue 5 frame read as `Issue4Compatible`: a 16-bit pointer of `0x0800` or more has a non-zero spare, taken from octet 0 of the M_PDU Header (`crates/aldis-aos/src/mpdu.rs:216`) |
| Mission effect | A packet reassembled from the wrong octets, or a zone that holds a packet reported as holding none |
| Requirements | AOS-001, AOS-003, AOS-017, GEN-062 |
| Evidence | `vector:aos-i4-dec-002`; `vector:aos-i4-dec-003`; `vector:aos-i4-dec-005-s`; `vector:aos-i4-dec-005-l`; `vector:aos-i4-dec-006-s`; `vector:aos-i4-dec-008`; `vector:aos-i4-enc-003`; `vector:aos-i4-enc-004`; `test:crates/aldis-aos/src/mpdu.rs::reserved_values_per_edition`; `test:crates/aldis-aos/src/mpdu.rs::issue4_rejects_raw_values_wider_than_its_pointer`; `test:crates/aldis-aos/src/mpdu.rs::decode_issue_4_spare`; `test:crates/aldis-aos/src/view.rs::issue_4_spares`; `test:crates/aldis-aos/tests/vectors.rs::issue4_compat` |
| Residual risk | Direction (b) is caught in strict mode (`ReservedValue`, `mpdu_spare`, `aos-i4-dec-005-s`) and reported in lenient mode as `Issue4SpareSet` with the pointer cut to its low 11 bits (`aos-i4-dec-005-l`). Direction (a) with a Packet Zone of more than 2 047 octets is silent in both modes, and no wire value can tell the two editions apart there: the pointers are valid numbers in both. No vector pins that silent case (`aos-i4-dec-008` uses a 14-octet frame, where the pointer is out of the zone); `proposal:P-AOS-04` adds it. The edition is a managed parameter, an integrator obligation (`asm:ASM-8`) |
| Assumptions | `asm:ASM-5`; `asm:ASM-8` |
| Proposals | `proposal:P-AOS-01`; `proposal:P-AOS-04` |
| Claims | `ac:AC-1.3`; `ac:AC-5.2` |

### HZ-AOS-10 A caller reads the header from the received octets instead of the corrected header

| | |
|---|---|
| Category | `wrong-value` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A header field read by the caller from the input octets differs from the one `AosFrame` reports, after the FHEC corrected one or two symbols |
| Cause | `as_bytes()` returns the whole frame as received, header octets uncorrected (`crates/aldis-aos/src/view.rs:333`–`337`); `header()`, `scid()`, `vcid()`, and the other accessors report the corrected values. A caller that routes on a VCID it peeked from the input, or recomputes a CRC over `as_bytes()`, uses the uncorrected header |
| Mission effect | A frame routed on a symbol the FHEC had corrected: the channel of the damaged header, not of the frame sent |
| Requirements | AOS-014, API-014 |
| Evidence | `vector:aos-fhec-c1-011`; `vector:aos-fhec-c2-002`; `vector:aos-fhecf-dec-002-s`; `vector:aos-fhecf-hdr-001`; `test:crates/aldis-aos/tests/properties.rs::two_symbol_errors_are_corrected`; `test:crates/aldis-aos/src/view.rs::length_fhec_and_fecf_come_first`; `test:crates/aldis-aos/src/header.rs::fhec_corrects_before_fields_are_read`; `test:crates/aldis-ffi/tests/aos.rs::the_header_decodes_alone`; `test:crates/aldis-ffi/tests/aos.rs::the_fhec_takes_whole_headers` |
| Residual risk | The tests show the corrected header equal to the one sent while `as_bytes()` still holds the damaged octets (`two_symbol_errors_are_corrected`, `length_fhec_and_fecf_come_first`), and the accessor's documentation says the octets may differ. The C ABI returns the corrected octets in `aldis_aos_header_t.octets`. Nothing stops a caller from reading the input; that use is outside the library |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-1.3`; `ac:AC-5.5` |

### HZ-AOS-11 The FHEC computed by the writer differs from 732.0

| | |
|---|---|
| Category | `nonconformant-output` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | `AosFrameWriter`, `encode`, and `aldis_aos_fhec_encode` write an FHEC that a conformant receiver does not accept, or the decoder corrects a conformant header to another (the encoder and decoder share the field tables and the generator) |
| Cause | A defect in the reading of §4.1.2.6.5 a–h: the field and generator polynomials, the symbol-to-bit mapping (the left bit of a symbol first), the five fill symbols, or which header bits are the six systematic symbols (bits 0–15 and 40–47) |
| Mission effect | Every frame with an FHEC is "corrected" into another header or dropped by a conformant receiver, and every frame of a conformant sender is mis-decoded by Aldis: loss of the channel, or misrouting |
| Requirements | AOS-013, AOS-019 |
| Evidence | `vector:aos-fhec-enc-001`; `vector:aos-fhec-enc-002`; `vector:aos-fhec-enc-003`; `vector:aos-fhec-enc-006`; `vector:aos-fhec-enc-009`; `vector:aos-fhec-enc-014`; `vector:aos-fhecf-enc-001`; `vector:aos-iz-enc-002`; `vector:aos-scid-enc-002`; `test:crates/aldis-aos/src/fhec.rs::tables_regenerate_from_the_field_polynomial`; `test:crates/aldis-aos/src/fhec.rs::generator_is_the_product_of_its_four_roots`; `test:crates/aldis-aos/src/fhec.rs::encoder_agrees_with_the_model`; `test:crates/aldis-aos/src/fhec.rs::decoder_agrees_with_the_model_on_all_65536_syndromes`; `test:crates/aldis-aos/src/header.rs::writer_header_computes_the_fhec`; `test:crates/aldis-aos/tests/vectors.rs::fhec_encode`; `test:crates/aldis-ffi/tests/vectors.rs::aos_fhec_encode`; `fuzz:aos_fhec_model`; `file:docs/verification/derivations/aos-fhec.md`; `file:docs/verification/reviews/m2-vectors.md`; `record:TV-023` |
| Residual risk | 732.0 prints no worked example of the code (the derivation's introduction), so conformance rests on a hand derivation of one encoding and one correction from §4.1.2.6.5, an independent model that generated the other values, and the reviewers' own scripts, which matched all 14 FHEC values and all 22 correction cases (second review, AOS section). The polynomial order is not stated by the Blue Book and does not matter, since g(x) is its own reciprocal (`crates/aldis-aos/src/fhec.rs` module documentation). No independent implementation of the AOS codec has cross-checked the values (`df:DF-9`). 732.0-B-4 §4.1.2.6.5 gives the same code (OI-04, resolved) |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-1.3`; `ac:AC-7.3` |

### HZ-AOS-12 A frame is released before it is complete or consistent

| | |
|---|---|
| Category | `nonconformant-output` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The octets of a two-phase write are sent before `finish`, or a data unit header points at a place where no packet or bit boundary is |
| Cause | A caller that drops `AosFrameWriter` without `finish` (or never calls `aldis_aos_writer_finish`): `begin` writes the headers and zero-fills the rest, so until `finish` the FECF, if configured, and every field the caller has not filled are zero (API-023; `crates/aldis-aos/src/writer.rs:387`–`400`). The writer also takes the First Header Pointer and the valid-bit count on trust: it checks that they lie inside the zone (AOS-017, AOS-019), not that a packet starts there or that the bits before the pointer are the valid ones, since it does not read the zone |
| Mission effect | A frame with FECF zero that the receiver rejects, or, with no FECF configured, a frame with zero Insert Zone, OCF, or Security Header that the receiver accepts; a data unit whose pointer misleads the packet extraction |
| Requirements | AOS-017, AOS-019, API-021, API-023 |
| Evidence | `vector:aos-min-enc-003`; `vector:aos-fhecf-enc-001`; `vector:aos-mpdu-enc-003`; `vector:aos-bpdu-enc-004`; `test:crates/aldis-aos/src/writer.rs::unfinished_frame_is_defined`; `test:crates/aldis-aos/src/writer.rs::two_phase_writer_fills_every_field`; `test:crates/aldis-aos/src/writer.rs::resume_keeps_what_the_caller_filled`; `test:crates/aldis-aos/tests/properties.rs::two_phase_writer_matches_encode`; `fuzz:aos_roundtrip` |
| Residual risk | `finish(self)` consumes the writer (`crates/aldis-aos/src/writer.rs:465`), but nothing forces the call: a dropped writer leaves the buffer in the defined state `unfinished_frame_is_defined` shows. A zero FECF is correct for a frame only with probability 2⁻¹⁶. The agreement of pointer and zone content is the engine's (the multiplexing of M4) and is outside the codec; the one-shot `encode` has the same limit |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-1.3` |

### HZ-AOS-13 The OID data does not continue the pseudo-noise sequence

| | |
|---|---|
| Category | `nonconformant-output` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The data fields of successive OID frames of a physical channel restart or repeat the pseudo-noise sequence instead of continuing it |
| Cause | A caller that gives `begin_oid` (or `aldis_aos_oid_writer_begin`) a new `OidGenerator` per frame, or one generator per VC or per master channel, where 732.0 §4.1.4.1.5.2.1–2 has the register initialised at device start-up and never restarted |
| Mission effect | OID frames with the same data field, and so insufficient randomization for the receiver's frame synchronization, which the sequence exists to avoid (§4.1.4.1.5 note 3) |
| Requirements | AOS-016, CMN-020, CMN-021, GEN-042 |
| Evidence | `vector:aos-oid-enc-001`; `vector:aos-oid-enc-002`; `vector:aos-oid-dec-001`; `test:crates/aldis-aos/src/writer.rs::oid_frames_continue_the_sequence`; `test:crates/aldis-aos/tests/properties.rs::oid_frames_decode_as_idle_data`; `test:crates/aldis-aos/tests/vectors.rs::oid`; `test:crates/aldis-ffi/tests/aos.rs::the_oid_writer_takes_a_generator_handle`; `test:crates/aldis-ffi/tests/vectors.rs::aos_oid`; `record:CABI-030` |
| Residual risk | The generator advances once per data octet written and not at all when the call fails (`oid_frames_continue_the_sequence`: rejected calls leave it unchanged). Its owner is the caller: CMN-021 makes it the physical channel multiplexer's, one per physical channel, never reset by reconfiguration, and the decoder cannot check it (`hz:HZ-AOS-17`). Whether a caller keeps one generator for the life of the channel is outside the library; the C ABI generator handle is valid storage the caller must keep (`hz:HZ-X-05`) |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-1.3` |

### HZ-AOS-14 A writer writes outside its buffer or outside its field

| | |
|---|---|
| Category | `memory` |
| Status | `controlled` |
| Severity | S1 |
| Failure mode | `encode`, `AosFrameWriter::begin` and its field accessors, or `aldis_aos_encode` and `aldis_aos_writer_begin`, write more octets than the output buffer holds, or a field longer or shorter than its place in the frame |
| Cause | A buffer shorter than the frame; a content slice whose length is not the configured one; an OCF given where the VC has none |
| Mission effect | A write outside the caller's buffer, or one field overwriting its neighbour |
| Requirements | GEN-014, API-022, GEN-042, GEN-020 |
| Evidence | `vector:aos-inv-enc-008`; `vector:aos-inv-enc-009`; `vector:aos-inv-enc-006`; `vector:aos-inv-enc-007`; `vector:aos-iz-enc-003`; `vector:aos-sec-enc-002`; `vector:aos-sec-enc-003`; `test:crates/aldis-aos/src/writer.rs::encode_checks_every_length`; `test:crates/aldis-aos/src/writer.rs::begin_rejects_without_writing`; `test:crates/aldis-aos/src/writer.rs::set_ocf_without_an_ocf`; `test:crates/aldis-aos/tests/properties.rs::rejected_encode_leaves_the_buffer`; `test:crates/aldis-aos/tests/properties.rs::encode_then_decode_round_trips`; `test:crates/aldis-ffi/tests/aos.rs::writer_arguments_are_checked`; `fuzz:aos_roundtrip`; `check:clippy` |
| Residual risk | `encode` checks the length of the four variable fields against the layout before it writes anything (`crates/aldis-aos/src/writer.rs:496`–`524`), `start` takes the first frame-length octets of `out` with `get_mut` and returns `BufferTooSmall` when there are not enough (`writer.rs:387`–`392`), and the field accessors return ranges the layout computed (`writer.rs:415`–`417`). A rejected call leaves `out` as it was (API-022). Through the C ABI the caller's `cap` is trusted (`hz:HZ-X-05`, `asm:ASM-6`) |
| Assumptions | `asm:ASM-6` |
| Proposals | — |
| Claims | `ac:AC-4.3`; `ac:AC-4.4` |

### HZ-AOS-15 The physical channel and the channel table passed to a call do not belong together

| | |
|---|---|
| Category | `state-corruption` |
| Status | `gap` |
| Severity | S1 |
| Failure mode | `AosFrame::decode` or a writer is given a `VcLookup` built for another physical channel than the `pc` of the call, and the checks that `VcTable::new` made against the first (AOS-018, AOS-002, the room for data) do not apply to the second |
| Cause | The Rust API takes the physical channel and the table as two arguments (AOS-010), and `VcLookup` has no way to name the physical channel it was built for (`crates/aldis-aos/src/config.rs:309`). The decoder's layout comes from `pc` and the channel parameters from the table; only the case where the table's VC leaves no data octet in the new layout is re-checked (`Malformed` in `decode`, `InvalidConfig` in the writers) |
| Mission effect | A B_PDU zone above 2 047 octets accepted, where the pointer value `0x3FFE` means both "only idle data" and the index of a bit (OI-35 item 7), so the valid-bit count read is wrong; a VC of an SCID above 255 that no frame of an `Issue4Compatible` channel can reach |
| Requirements | AOS-010, AOS-018, AOS-002, API-040 |
| Evidence | `test:crates/aldis-aos/src/view.rs::channels_that_do_not_fit_the_physical_channel`; `test:crates/aldis-aos/src/config.rs::bitstream_limits`; `test:crates/aldis-aos/src/config.rs::table_rejects_what_732_forbids`; `test:crates/aldis-ffi/tests/aos.rs::the_decoder_takes_the_master_channels_vcs`; `vector:aos-bpdu-cfg-005` |
| Residual risk | The C ABI cannot make the mistake: `master_channel` builds the table from the master channel's own physical channel on every call (`crates/aldis-ffi/src/aos.rs:480`–`500`). In Rust the documentation of `begin` says a table "built for `pc`" never leaves a VC without a data octet, but no check compares the two, and the tests show only the no-room case. The fuzz target builds the pair together, so it cannot find a mismatch |
| Assumptions | — |
| Proposals | `proposal:P-AOS-05` |
| Claims | `ac:AC-1.3` |

### HZ-AOS-16 A C caller finishes a frame with other arguments than it began it with

| | |
|---|---|
| Category | `abi-misuse` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | `aldis_aos_writer_finish` returns success and a valid FECF for a frame whose fields the caller filled at the offsets of one configuration, while the headers, the layout, and the FECF now follow another |
| Cause | The C writer keeps no state (API-023): `aldis_aos_writer_finish` takes the same arguments as `aldis_aos_writer_begin` and rewrites the headers with them (`AosFrameWriter::resume`, `crates/aldis-aos/src/writer.rs:348`), recomputing the layout. A different VC, OCF flag, or security length in the second call moves every field after the Primary Header, and the FECF is computed over whatever lies in the buffer |
| Mission effect | A frame that passes every check of a conformant receiver, with the Insert Zone, the Security Header, the data, and the OCF at places the receiver does not expect |
| Requirements | AOS-019, API-023, CABI-022 |
| Evidence | `test:crates/aldis-ffi/tests/aos.rs::writer_arguments_are_checked`; `test:crates/aldis-ffi/tests/aos.rs::the_oid_writer_takes_a_generator_handle`; `test:crates/aldis-aos/src/writer.rs::resume_keeps_what_the_caller_filled` |
| Residual risk | The tests show `finish` after `begin` with the same arguments, and the errors for bad arguments; none shows `finish` given other arguments, which succeeds. The library cannot tell that the buffer was begun with other arguments, unless `finish` compared the headers already in `out` with the ones it computes, which it does not. The documentation requires "the same arguments" (`crates/aldis-ffi/src/aos.rs:1117`–`1119`); `asm:ASM-6` covers pointers and sizes, not this. Until OI-50 is implemented (`m2/hazard-controls`), `finish` given other arguments succeeds |
| Assumptions | `asm:ASM-6` |
| Proposals | `proposal:P-AOS-06` |
| Claims | `ac:AC-6.2` |

### HZ-AOS-17 A frame on VCID 63 is accepted with any data, and real data on it is discarded

| | |
|---|---|
| Category | `hidden-deviation` |
| Status | `residual` |
| Severity | S4 |
| Failure mode | `AosFrame::decode` accepts a frame with VCID 63 whose data field is not the pseudo-noise sequence of §4.1.4.1.5.2, delivers it as `DataUnit::IdleData`, and reports no anomaly, in either mode |
| Cause | A sender fault; a mission that sends data on VCID 63 against §4.1.4.1.5.1; or a channel error that turns the VCID of a data frame into 63 and gets past the FHEC and FECF. The decoder is stateless and so cannot check the sequence, which depends on the generator's position across frames (CMN-021); the maintainer accepted that (second review, decision 3; [aos-frame.md](../../standards/aos-frame.md) §4, "OID data"). It also accepts any SCID that has a configured VC, and applies no VC parameters to an OID frame (OI-43 item 1) |
| Mission effect | Data on VCID 63 is handled as idle by every consumer and discarded; a sender that breaks the sequence is not reported |
| Requirements | AOS-016, API-031, CMN-021 |
| Evidence | `vector:aos-iz-dec-003`; `vector:aos-sec-dec-002`; `vector:aos-oid-dec-001`; `vector:aos-oid-dec-002`; `vector:aos-oid-cfg-001`; `test:crates/aldis-aos/tests/properties.rs::oid_frames_decode_as_idle_data`; `test:crates/aldis-aos/src/view.rs::unknown_channels`; `file:docs/verification/reviews/m2-vectors.md`; `oi:OI-43` |
| Residual risk | A listed exception of API-031 (`hz:HZ-X-08`), a recorded decision, not a defect. The residual for AOS is that the VCID is inside the FHEC (AOS-013), so a damaged VCID is corrected or caught by the FECF as for any channel (`hz:HZ-AOS-01`); with neither configured, a frame turned into VCID 63 by an error is silently idle. Whether a receiver should count OID frames whose data is not the sequence is the link engine's (M4) |
| Assumptions | `asm:ASM-5` |
| Proposals | — |
| Claims | `ac:AC-5.1`; `ac:AC-1.3` |

## Proposals

| ID | Kind | Proposal | Status |
|---|---|---|---|
| P-AOS-01 | `doc` | Add an assumption to the case ([assurance-case.md](../assurance-case.md), after ASM-7), draft: "Aldis is configured with the sender's managed parameters for the Mission Phase: for AOS the edition, the frame length, the presence of the FHEC and the FECF, the Insert Zone length, and per virtual channel the Data Field Content, the presence of the OCF, and the Security Header and Trailer lengths (732.0 §4.1.2.6.2–4, §4.1.3.4, §4.1.4.1.4, §4.1.5.2, Table 6-1); the other codecs have the same parameters." Basis: those clauses (a managed parameter is static through a Mission Phase); obligation: take both sides from the same managed-parameter database and decode a known frame of each channel before the phase starts; change them only at a phase change. No frame field says which layout a frame has, and the FECF covers any layout, so Aldis cannot detect the mismatch (`hz:HZ-AOS-08`, `hz:HZ-AOS-09`, `hz:HZ-AOS-02`). Written as a case change, which the coordinator makes; the hazards then name the new row | `accepted` this branch, ASM-8 |
| P-AOS-02 | `oi` | Draft OI, "The Insert Service of AOS frames the decoder rejects": 732.0 §4.3.7.4 has the receiving function extract the IN_SDUs of incoming frames regardless of their GVCID, and allows it before the FECF is checked when the IN_SDU needs no error protection; `AosFrame::decode` gives no view of a frame it rejects. Options: (a) state in [aos-frame.md](../../standards/aos-frame.md) §4 that the zone lies at octets `primary_header_len` to `primary_header_len + insert_zone_len` of every frame of the physical channel, so the M4 link engine can read it from the input of a rejected frame, and leave the extraction to M4 (LNK-053); (b) add a function that returns the Insert Zone of a frame after the length check only, with its own C ABI call; (c) keep the contract silent. Recommendation (a): it records the reading at no cost to the codec and leaves the API decision to the milestone that owns the service. Whether the service is M2's or M4's, and a new C ABI call, are the maintainer's ([decision rights](../../process/decision-rights.md) §2.2, §2.4) | `proposed` |
| P-AOS-03 | `oi` | Draft OI, "B_PDU pointer naming the zone's last bit": 732.0 §4.1.4.3.3.2 defines the Bitstream Data Pointer as the index of the last valid bit and §4.1.4.3.3.3 requires all ones when the zone holds no idle data; it does not say whether a receiver rejects the index of the last bit. AOS-017 rejects it in both modes as the project's reading (second review, AOS finding 1). Options: (a) keep the rejection and record it as resolved, with the reading; (b) accept it as `AllValid` in lenient mode with a new anomaly flag (the bit index is permanent once released, [aos-frame.md](../../standards/aos-frame.md) §5); (c) accept it as `ValidBits` of the whole zone. Recommendation (a): API-031 makes a violation of a SHALL a strict error, and (b) adds a flag to a catalogue the C ABI shares. A reading the Blue Book does not settle is the maintainer's ([decision rights](../../process/decision-rights.md) §2.1) | `accepted` OI-53 |
| P-AOS-04 | `test` | Two vector cases in `test-vectors/aos/issue4_compat.json`: the octets of an Issue 4 frame with a Packet Zone of more than 2 047 octets and the pointer `0x7FF`, read in `Issue5` (accepted as `StartsAt(2047)`, the silent misreading of `hz:HZ-AOS-09`), and an Issue 5 frame with pointer `0x0800` read in `Issue4Compatible` in lenient mode (pointer 0, `Issue4SpareSet`). They pin the residual so that a change to it shows. The cases follow `aos-i4-dec-007` and `-008`; an agent's decision (new vector cases, [decision rights](../../process/decision-rights.md) §2.5) | `accepted` `m2/hazard-controls` |
| P-AOS-05 | `requirement` | Draft requirement, AOS-020: "The decoder and the writers SHALL compare the physical channel they are given with the one the channel table was built for, and fail with `InvalidConfig` (`frame_len`) if they differ." It needs `VcLookup` to name its physical channel (a method or an associated value), for `VcTable` and for any other implementation. Source: project rule (RS-020, a relation between two values is checked where they meet). Verification: T, with a table built for one physical channel and a call with another. Recommendation: accept; it closes `hz:HZ-AOS-15` with one comparison per call. A public API change, so the maintainer's | `proposed` |
| P-AOS-06 | `oi` | Draft OI, "`aldis_aos_writer_finish` called with other arguments than `_begin`": the stateless two-phase writer (API-023) cannot tell. Options: (a) document that the arguments must be the same, and say in ASM-6 that a mismatch gives a valid-looking frame, and add a test that pins it (a C ABI test of `finish` with another VC); (b) `finish` fails with `INVALID_FIELD` when the header octets and the `M_PDU` or `B_PDU` Header already in `out` are not those it would write, so a different VC, SCID, or pointer is an error (the frame count and flags are the caller's to leave as `begin` wrote them); (c) keep it as it is. Recommendation (b), with (a)'s test: a mismatch gives a frame that a conformant receiver accepts, the worst failure the C ABI has, and the comparison is of at most ten octets. A C ABI convention, so the maintainer's ([decision rights](../../process/decision-rights.md) §2.4) | `accepted` OI-50, `m2/hazard-controls` |
