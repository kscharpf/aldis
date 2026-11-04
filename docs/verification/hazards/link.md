# Hazards: link engines (`aldis-link`)

Failure modes of the sending and receiving engines of `aldis-link` (the PacketGenerator, AccessGenerator, BitstreamGenerator, ChannelMux, FrameReceiver, PacketExtractor, MapSegmenter, MapReassembler, and CltuPacker) and of their C ABI handles, in the format of [hazards.md](../hazards.md) §1.7. Standards: the protocol procedures of 132.0-B-3 (TM), 232.0-B-4 (TC), 732.0-B-5 (AOS), and 732.1-B-3 (USLP), with the packets of 133.0-B-2 and 133.1-B-3; the requirements are the LNK rows of [04](../../04-engines.md). The failure modes common to all crates (panics, loops, stack, the C ABI, single-event upsets) are in [hazards.md](../hazards.md) §3; a hazard here repeats one only where the engines add a cause of their own. The codec hazards of [tm.md](tm.md), [tc.md](tc.md), [aos.md](aos.md), and [uslp.md](uslp.md) cover the frames the engines write and read; the COP-1 engines are in [cop1.md](cop1.md).

The engines are sans-I/O (GEN-030): they read no clock, send no frame, and hold no output queue (API-059). The sending engines take packets, SDUs, and bits into fixed queues, cut them into data fields and frames, and write the octets into the caller's output area; the receiving engines take a decoded frame (`Plain`, RS-023) and return its parts. Everything around them is the caller's: the choice of when to release a field or build a frame, the clock, the order of the engines, the delivery of the actions, the SDLS hook, the coding sublayer, and the routing of a field from a generator to the mux or from the receiver to an extractor. The assumptions are `asm:ASM-8`, `asm:ASM-9`, and `asm:ASM-19` of the [assurance case](../assurance-case.md#assumptions-and-context).

## Coverage

| Category | Hazards | State |
|---|---|---|
| `corrupt-accepted` | HZ-LINK-01, HZ-LINK-02 | done |
| `valid-rejected` | HZ-LINK-03, HZ-LINK-04 | done |
| `misrouting` | HZ-LINK-05 | done |
| `truncation` | HZ-LINK-06 | done |
| `wrong-value` | HZ-LINK-07 | done |
| `nonconformant-output` | HZ-LINK-08 | done |
| `unbounded-time` | HZ-LINK-09 | done |
| `memory` | HZ-LINK-10 | done |
| `panic` | HZ-LINK-11 | done |
| `state-corruption` | HZ-LINK-12, HZ-LINK-13 | done |
| `abi-misuse` | HZ-LINK-14 | done |
| `hidden-deviation` | HZ-LINK-15 | done |

## Hazards

### HZ-LINK-01 An extractor or reassembler delivers a packet or SDU that was not sent

| | |
|---|---|
| Category | `corrupt-accepted` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The PacketExtractor delivers a packet stitched from the tail of one packet and the head of another, or the MapReassembler delivers an SDU assembled from segments of two SDUs, after a lost frame, a wrong First Header Pointer, or a segment out of sequence. The user then reads a packet or SDU that no sender sent |
| Cause | A defect in the resynchronisation (the extractor discards the partial packet after a gap and continues at the next pointer, LNK-061; the reassembler discards the SDUs in progress of the VC at a gap and on a sequence violation, LNK-070); a wrong pointer that the channel changed past the frame check; a length field that the channel changed in a packet |
| Mission effect | A telemetry packet or a telecommand segment that the ground or the spacecraft acts on, although it is not what was sent |
| Requirements | LNK-060, LNK-061, LNK-062, LNK-063, LNK-064, LNK-070, LNK-071, LNK-072, LNK-073, LNK-077 |
| Evidence | `vector:link-recv-extract-tm-packet-spans-fields`; `vector:link-recv-extract-tm-header-split`; `vector:link-recv-extract-aos-packet-ends-at-field-end`; `vector:link-recv-extract-uslp-no-packet-starts`; `vector:link-recv-reassemble-tc-sequence-violations`; `vector:link-recv-reassemble-tc-maps-interleaved`; `vector:link-recv-reassemble-uslp-fixed-violations`; `vector:link-recv-reassemble-uslp-gap`; `test:crates/aldis-link/tests/packet_roundtrip.rs::tm_round_trip`; `test:crates/aldis-link/tests/packet_roundtrip.rs::aos_round_trip`; `test:crates/aldis-link/tests/packet_roundtrip.rs::uslp_round_trip`; `test:crates/aldis-link/tests/map_roundtrip.rs::tc_round_trip`; `test:crates/aldis-link/tests/map_roundtrip.rs::uslp_round_trip`; `test:crates/aldis-link/tests/engine_housekeeping.rs::the_extractor_resynchronises_in_the_cases_the_vectors_leave_open`; `test:crates/aldis-link/tests/map_reassembler_engine.rs::a_gap_discards_the_sdus_in_progress_of_every_map_of_the_vc`; `test:crates/aldis-link/tests/map_reassembler_engine.rs::a_reassembled_packet_must_agree_with_its_length_field`; `fuzz:link_packet_roundtrip`; `fuzz:link_map_roundtrip`; `fuzz:link_map_inputs` |
| Residual risk | After a frame is lost, the extractor discards the partial packet and starts at the next pointer, and the reassembler discards the SDUs in progress: what is delivered next is a packet or SDU that began after the loss. A loss that the frame counts do not show (a gap of a multiple of the count's range, 256 frames for the 8-bit TM and AOS-less counts) is not seen by the receiver, and the engines then stitch the packet across the loss; the packet header's length field detects some of the cases, and the packet-level sequence count of a Space Packet the rest, but only if the caller checks it. The `deliver_incomplete` option (LNK-061) is off by default and, when on, hands a partial packet to the user marked as incomplete. An error of the channel that changes a pointer or length field and that the frame's FECF misses (`hz:HZ-X-06`) produces a wrong split that the engines cannot see. Vectors for these cases were written by the blind model of WP-2b (TV-023), and the round trips are properties of the engines among themselves, not against a second implementation |
| Assumptions | `asm:ASM-4`; `asm:ASM-8`; `asm:ASM-19` |
| Proposals | — |
| Claims | `ac:AC-1.9`; `ac:AC-5.5` |

### HZ-LINK-02 A frame that failed a check reaches the FrameReceiver as a good one

| | |
|---|---|
| Category | `corrupt-accepted` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The FrameReceiver delivers the data field, the OCF, or the Insert Zone of a frame whose FECF did not match, whose AOS FHEC was uncorrectable, or whose SDLS processing failed |
| Cause | A caller that feeds raw octets where the decoded frame is required; a defect that lets a frame reach the engine without the codec's checks; the raw-frame option (LNK-079) |
| Mission effect | Corrupt data delivered as service data |
| Requirements | LNK-050, LNK-079, LNK-080, RS-023, API-017 |
| Evidence | `test:crates/aldis-link/tests/frame_receiver.rs::a_frame_of_an_unconfigured_channel_is_decoded_and_counted`; `test:crates/aldis-link/tests/frame_receiver.rs::a_raw_frame_is_refused_when_the_option_is_off_or_the_length_is_wrong`; `test:crates/aldis-link/tests/frame_receiver.rs::a_frame_of_another_length_is_refused`; `test:crates/aldis-link/tests/frame_receiver.rs::the_reserved_flag_combination_of_uslp_has_no_plain_frame`; `vector:link-recv-aos-caller-discard`; `vector:link-recv-tc-caller-discard`; `test:crates/aldis-link/tests/frame_receiver.rs::no_frame_makes_a_receiver_fail_internally`; `fuzz:link_frame_receiver`; `check:clippy` |
| Residual risk | The type of the input is the control: the engine takes the `Plain` frame that the codec's decoder and the no-security step produce (RS-023), and nothing else, so a frame that failed the FECF check has no value of that type. A caller who builds a `Plain` frame without the check, or who feeds the raw-frame input (an option, off by default), takes the data of an unchecked frame from the engine: the raw input returns the Insert Zone only (LNK-079), which is an AOS or USLP value that a mission uses for time or housekeeping. The caller reports a frame it discarded with the CallerDiscard input so that the counters show it (LNK-080); an unreported discard is a frame the engine counts as lost. The residual of the check itself is `hz:HZ-X-06` |
| Assumptions | `asm:ASM-2`; `asm:ASM-4`; `asm:ASM-19` |
| Proposals | — |
| Claims | `ac:AC-1.9`; `ac:AC-5.5` |

### HZ-LINK-03 An engine refuses a valid input or configuration

| | |
|---|---|
| Category | `valid-rejected` |
| Status | `residual` |
| Severity | S3 |
| Failure mode | A generator, the mux, or a segmenter refuses a packet, SDU, offer, or configuration that the Blue Book allows, or a receiver refuses a frame for a channel it is configured for |
| Cause | A configuration constructor that is stricter than the standard (LNK-078); a capacity that is too small for the data (the queue, the slot, the assembly buffer); a defect in a length or a flag check |
| Mission effect | A packet or frame is lost, always with an error the caller sees; the data is not delivered |
| Requirements | LNK-010, LNK-015, LNK-020, LNK-040, LNK-073, LNK-076, LNK-078, API-052 |
| Evidence | `test:crates/aldis-link/tests/engine_housekeeping.rs::the_generator_refuses_what_the_blue_book_forbids`; `test:crates/aldis-link/tests/channel_mux_engine.rs::a_configuration_the_blue_books_forbid_is_refused`; `test:crates/aldis-link/tests/channel_mux_engine.rs::an_offer_the_vc_cannot_have_is_refused_and_changes_nothing`; `test:crates/aldis-link/tests/map_segmenter_engine.rs::configurations_are_checked`; `test:crates/aldis-link/tests/map_reassembler_engine.rs::configurations_are_checked`; `test:crates/aldis-link/tests/frame_receiver.rs::a_configuration_is_checked_where_it_is_built`; `vector:link-send-pg-tm-lnk-010-submit-errors`; `vector:link-send-pg-aos-lnk-010-submit-errors`; `vector:link-send-seg-tc-lnk-040-refused`; `vector:link-send-seg-uslp-lnk-042-refused`; `test:crates/aldis-link/tests/packet_roundtrip.rs::tm_round_trip` |
| Residual risk | A refusal is an error with the field named (API-040) and no change of state (API-058), so the loss is reported. The capacity of a queue or slot is a build constant of the integrator (CABI-030); one that is smaller than the longest packet or SDU of a mission refuses that data every time. `WouldBlock` is the normal answer of a full queue (LNK-015, LNK-076): a caller who treats it as a failure and drops the packet loses it |
| Assumptions | `asm:ASM-19` |
| Proposals | — |
| Claims | `ac:AC-1.9`; `ac:AC-5.3` |

### HZ-LINK-04 A policy starves a Virtual Channel, or a field waits forever

| | |
|---|---|
| Category | `valid-rejected` |
| Status | `residual` |
| Severity | S3 |
| Failure mode | The multiplexing policy never selects an offered field of a VC, or the generator never releases a started field, so that the data of the VC does not leave |
| Cause | Fixed priority gives an always-offered VC of higher priority every opportunity (the intended behaviour of the policy, LNK-031); a field below the release deadline that the caller never completes or releases; a `Timeout` the caller does not send |
| Mission effect | Data of a lower-priority VC (for example housekeeping beside a high-rate payload) is delayed without bound |
| Requirements | LNK-012, LNK-014, LNK-030, LNK-031, LNK-076 |
| Evidence | `vector:link-send-mux-tm-lnk-031-fixed-priority`; `vector:link-send-mux-tm-lnk-031-round-robin`; `test:crates/aldis-link/tests/channel_mux_engine.rs::no_sequence_of_inputs_breaks_the_engine`; `test:crates/aldis-link/tests/engine_housekeeping.rs::timeouts_are_harmless_until_the_deadline`; `test:crates/aldis-link/tests/engine_housekeeping.rs::a_packet_waits_in_the_queue_behind_a_marked_field` |
| Residual risk | CCSDS gives no multiplexing algorithm (LNK-031), and the library offers fixed priority, round robin, and weighted round robin; only the round-robin policies bound the wait of a VC. The choice of policy and priorities is the mission's. The release deadline is only a deadline the engine reports: the caller sends the `Timeout` input at it (`asm:ASM-19`). The OID frame fills an opportunity at which no VC offers a field (LNK-030), so an idle channel is not starved of frames, only of data |
| Assumptions | `asm:ASM-19` |
| Proposals | — |
| Claims | `ac:AC-1.9` |

### HZ-LINK-05 A frame or field is built for, or delivered to, the wrong channel

| | |
|---|---|
| Category | `misrouting` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The mux writes a field of one VC into the frame of another, or into a frame with another MAP, or gives it the count of another channel; the FrameReceiver delivers a part of a frame to the service of another VC or MAP; the MapReassembler joins segments of different MAPs; the caller routes a data field of a TM VC to the extractor of another |
| Cause | A defect in the offer slot lookup, the count slot lookup, or the MAP bookkeeping; a frame whose header names a channel that the channel's error changed to another configured one (the frame check passed, `hz:HZ-X-06`); a caller that wires the engines wrongly |
| Mission effect | Data of one source is read as data of another, or a telecommand is applied to the wrong MAP |
| Requirements | LNK-030, LNK-032, LNK-041, LNK-050, LNK-051, LNK-052, LNK-070, LNK-076, LNK-081 |
| Evidence | `vector:link-recv-tm-routing`; `vector:link-recv-aos-routing`; `vector:link-recv-uslp-routing`; `vector:link-recv-tm-unknown-vc`; `vector:link-recv-aos-unknown-vc`; `vector:link-recv-uslp-unknown-vc-and-map`; `vector:link-recv-tc-unknown-vc`; `vector:link-recv-reassemble-tc-maps-interleaved`; `vector:link-recv-reassemble-uslp-unknown-map`; `vector:link-send-mux-uslp-lnk-032-map-id`; `vector:link-send-seg-uslp-lnk-041-complete-before-another-map`; `test:crates/aldis-link/tests/channel_mux_engine.rs::a_uslp_offer_is_checked_against_the_vc_and_its_cop`; `test:crates/aldis-link/tests/frame_receiver.rs::a_tm_frame_that_contradicts_its_vc_is_refused`; `test:crates/aldis-link/tests/frame_receiver.rs::a_frame_of_an_unconfigured_channel_is_decoded_and_counted` |
| Residual risk | A frame of an unconfigured channel is discarded and counted (`UnknownChannel`), not delivered. A channel error that turns the VCID or MAP ID of a frame into another configured one passes the engine's checks when the FECF misses it (`hz:HZ-X-06`); with no FECF configured, any such error passes. One engine serves one protocol and one VC or channel (LNK-001): that the caller hands a field to the right engine is the caller's wiring |
| Assumptions | `asm:ASM-4`; `asm:ASM-8`; `asm:ASM-19` |
| Proposals | — |
| Claims | `ac:AC-1.9`; `ac:AC-5.5` |

### HZ-LINK-06 Octets of a packet, SDU, or field are dropped or added

| | |
|---|---|
| Category | `truncation` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A packet or SDU leaves the sending engines shortened or with extra octets: the split of a packet over data fields loses or repeats an octet, the bit queue of the BitstreamGenerator loses a bit, the Last Valid Octet Pointer or the B_PDU pointer names the wrong octet, or a reconfiguration or a gap discards octets without saying so |
| Cause | A defect in the placement of the First Header Pointer, the split and the residue of an idle packet, the pointer of a B_PDU, or the discard accounting; a `Reconfigure` input that drops the queued data |
| Mission effect | A packet that the receiver cannot parse, or one that parses as another; a bitstream with a missing bit |
| Requirements | LNK-011, LNK-013, LNK-020, LNK-021, LNK-003, LNK-064, LNK-072, LNK-077, LNK-002 |
| Evidence | `vector:link-send-pg-tm-lnk-011-contiguous-exact-fit`; `vector:link-send-pg-tm-lnk-011-spanning-no-start`; `vector:link-send-pg-tm-lnk-011-oi69-tail-ends-at-zone-end`; `vector:link-send-pg-aos-lnk-011-start-after-tail`; `vector:link-send-pg-uslp-lnk-011-spanning-no-start`; `vector:link-send-ag-uslp-lnk-020-sdu-spans-frames`; `vector:link-send-ag-uslp-lnk-020-sdu-spans-exact`; `vector:link-send-bg-aos-lnk-021-full-and-partial`; `vector:link-send-bg-aos-lnk-021-bit-granularity`; `vector:link-send-ag-uslp-lnk-003-reconfigure`; `vector:link-send-bg-aos-lnk-003-reconfigure`; `test:crates/aldis-link/tests/packet_roundtrip.rs::tm_round_trip`; `test:crates/aldis-link/tests/packet_roundtrip.rs::uslp_round_trip`; `test:crates/aldis-link/tests/engine_housekeeping.rs::an_idle_field_never_splits_a_packet`; `test:crates/aldis-link/tests/engine_housekeeping.rs::reconfigure_discards_and_keeps_the_counters`; `test:crates/aldis-link/tests/generators_engine.rs::the_queue_frees_what_a_release_takes`; `test:crates/aldis-link/tests/frame_receiver.rs::the_valid_bits_of_a_bpdu_are_delivered_and_the_idle_bits_dropped`; `fuzz:link_packet_roundtrip`; `fuzz:link_map_roundtrip` |
| Residual risk | The placement of the pointer is a vector of the blind model for every protocol (the cases of the M4 plan §5: a header split over two fields, a packet ending exactly at the end of a field, idle fill shorter than a packet header) and a round trip against the extractor over arbitrary lengths. `Reconfigure` discards the queued and partly built data by design (LNK-003) and reports the octets in a `Discarded` action and the counter; a caller that ignores the action loses them silently. The octets the receiving engines drop (idle packets, the idle bits of a B_PDU, the octets after a Last Valid Octet Pointer, a packet over the maximum length) are counted, not delivered |
| Assumptions | `asm:ASM-9`; `asm:ASM-19` |
| Proposals | — |
| Claims | `ac:AC-1.9` |

### HZ-LINK-07 A count, a pointer, or a gap is wrong

| | |
|---|---|
| Category | `wrong-value` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The mux numbers frames with a count that is not the next one of its channel (the Master Channel or Virtual Channel Frame Count, the USLP count of a QoS and MAP), the FrameReceiver reports a gap of the wrong size or none, or a pointer value (the First Header Pointer, the Bitstream Data Pointer) is wrong |
| Cause | A defect in the assignment of counts at the selection of a frame (LNK-032), the modular arithmetic of the gap (LNK-051), or the choice of counter by protocol (TM VC, AOS 24-bit, USLP 1 to 7 octets, an AOS cycle) |
| Mission effect | A false gap hides or invents a frame loss; the receiving end of COP-1 or the ground's sequence check draws a wrong conclusion |
| Requirements | LNK-032, LNK-036, LNK-051, LNK-052, LNK-053, LNK-054, LNK-082, COP-040 |
| Evidence | `vector:link-send-mux-tm-lnk-032-counts-independent`; `vector:link-send-mux-tm-lnk-032-counts-wrap`; `vector:link-send-mux-aos-lnk-032-vc-counts`; `vector:link-send-mux-uslp-lnk-032-qos-counts`; `vector:link-send-mux-uslp-lnk-032-vcf-wrap`; `vector:link-recv-aos-vc-gap`; `vector:link-recv-aos-count-discontinuity`; `vector:link-recv-uslp-vcf-gap`; `vector:link-recv-uslp-count-wrap`; `vector:link-recv-uslp-no-count-no-gap`; `test:crates/aldis-link/tests/channel_mux_engine.rs::the_aos_vc_frame_count_counts_up_by_one_per_frame`; `test:crates/aldis-link/tests/channel_mux_engine.rs::a_reconfiguration_keeps_the_counts_and_the_oid_generator_and_starts_the_walk_afresh`; `test:crates/aldis-link/tests/frame_receiver.rs::a_change_of_the_cycle_use_flag_is_a_discontinuity`; `test:crates/aldis-link/tests/frame_receiver.rs::a_gap_and_a_discontinuity_never_come_together_for_one_counter`; `test:crates/aldis-link/tests/frame_receiver.rs::counters_count_and_persist_across_a_reconfiguration`; `test:crates/aldis-link/tests/frame_receiver.rs::frame_loss_changes_no_count` |
| Residual risk | The counts are one vector per protocol and rule of the blind model. A gap is the difference of the count and the expected one modulo the range of the count (LNK-051), so a loss of exactly the range (or a multiple) is not seen, and the first frame after start-up or reconfiguration sets the expectation and reports no gap. A frame that was lost before the receiver (the caller's `FrameLoss` input, LNK-053) is a flag the caller raises. Which counts a VC has (a TM VC with no counter of its own, a USLP VC with a VCF Count Length of zero) is the managed parameter of the channel and must equal the sender's (`asm:ASM-8`) |
| Assumptions | `asm:ASM-8`; `asm:ASM-19` |
| Proposals | — |
| Claims | `ac:AC-1.9` |

### HZ-LINK-08 The mux or a generator writes a field or frame that does not conform

| | |
|---|---|
| Category | `nonconformant-output` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A data field, a frame, or an FDU that an engine produces does not conform to the Blue Book: a wrong First Header Pointer or rule for the protocol, a wrong idle fill, an OID frame with the wrong marks, a wrong Segment Header, TFDF rule, or sequence flags, an Insert Zone of the wrong length, a CLTU unit that mixes frames the standard keeps apart |
| Cause | A defect in the header writers of the engines (they call the codec writers' layouts, `file:crates/aldis-link/src/mux.rs`), a configuration the engine accepts but the Blue Book forbids, or a reading of a place where the standard is silent |
| Mission effect | A frame the receiver or the ground segment cannot parse or that parses as something else |
| Requirements | LNK-013, LNK-014, LNK-020, LNK-021, LNK-030, LNK-033, LNK-034, LNK-035, LNK-040, LNK-042, LNK-044, LNK-082 |
| Evidence | `vector:link-send-mux-tm-lnk-030-oid-frames`; `vector:link-send-mux-aos-lnk-030-oid-frames`; `vector:link-send-mux-uslp-lnk-030-oid-frames`; `vector:link-send-mux-aos-lnk-035-fhec-reserved`; `vector:link-send-mux-aos-lnk-034-insert-zone`; `vector:link-send-mux-uslp-lnk-033-mc-ocf`; `vector:link-send-seg-tc-lnk-040-segmented-sdu`; `vector:link-send-seg-uslp-lnk-042-rule-111`; `vector:link-send-seg-uslp-lnk-042-segments`; `vector:link-send-pg-aos4-lnk-011-fhp-11-bit`; `vector:link-send-pg-aos-lnk-011-oi69-tail-ends-at-zone-end`; `test:crates/aldis-link/tests/channel_mux_engine.rs::an_issue_4_aos_frame_has_an_8_bit_scid_and_zero_spare_bits`; `test:crates/aldis-link/tests/channel_mux_engine.rs::a_reconfiguration_puts_the_slots_back_to_the_configured_values`; `test:crates/aldis-link/tests/cltu_packer.rs::truncated_and_non_truncated_usl_frames_do_not_share_a_unit`; `test:crates/aldis-link/tests/cltu_packer.rs::a_unit_with_a_type_bd_frame_asks_for_no_repetition`; `test:crates/aldis-link/tests/segmenter_fop1.rs::tc_fdus_are_the_requests_of_fop1`; `ci:vectors validate` |
| Residual risk | The expected octets of the send vectors come from a Python model written blind from the Blue Books and `docs/` (TV-023), and the engines' frames are decoded again by the codecs in the round trips, which share the codec's reading of a field with the engine. The places where the standards are silent were decided by the maintainer (OI-73 to OI-78) and are not checked against a peer; they are in `hz:HZ-LINK-15`. The frame the mux writes has the Security Header and Trailer, the FHEC, and the FECF as zero octets (LNK-035): the caller completes them, and a frame sent without that step is not a conforming frame (`asm:ASM-9`) |
| Assumptions | `asm:ASM-9`; `asm:ASM-19` |
| Proposals | — |
| Claims | `ac:AC-1.9` |

### HZ-LINK-09 One input takes unbounded work, or an action list overflows

| | |
|---|---|
| Category | `unbounded-time` |
| Status | `residual` |
| Severity | S2 |
| Failure mode | A step loops without bound over a queue, a pointer chain, or a list of VCs, or returns more actions than the engine's compile-time bound, so that a push fails after the state changed |
| Cause | A loop bound by a value read from a frame or a packet header (a packet length, a pointer), a queue that is longer than the capacity, a list of actions longer than the bound of LNK-074 |
| Mission effect | The calling task hangs until a watchdog acts |
| Requirements | LNK-074, API-057, GEN-022, GEN-023, GEN-031 |
| Evidence | `record:GEN-022`; `check:spec`; `check:stack`; `test:crates/aldis-link/tests/frame_receiver.rs::the_worst_case_of_each_protocol_returns_exactly_the_bound`; `test:crates/aldis-link/tests/channel_mux_engine.rs::the_worst_case_of_an_input_is_the_bound`; `test:crates/aldis-link/tests/engine_housekeeping.rs::the_worst_case_of_the_extractor_has_four_actions`; `test:crates/aldis-link/tests/map_reassembler_engine.rs::no_input_exceeds_two_actions`; `test:crates/aldis-link/tests/map_segmenter_engine.rs::no_action_exceeds_the_bound`; `test:crates/aldis-link/tests/packet_roundtrip.rs::extractor_actions_stay_within_the_bound`; `test:crates/aldis-link/tests/map_roundtrip.rs::the_reassembler_never_reports_an_internal_error`; `fuzz:link_map_inputs`; `fuzz:link_mux`; `fuzz:link_packet_roundtrip` |
| Residual risk | Every loop of the engines is over a slice, an array, or a closed range, or has a `bound:` comment checked by `cargo xtask loops` (`record:GEN-022`); no recursion (`check:stack`). The action bound of each input is a compile-time constant of the type and is driven to its worst case by a test for each engine. The work of one step grows with the capacities the integrator builds (a queue of 8 192 octets is shifted octet by octet), so the time of a call is bounded but not small; the engines read no clock and cannot measure it |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-3.1`; `ac:AC-3.2`; `ac:AC-1.9` |

### HZ-LINK-10 A step overruns its stack, or writes outside the output area or its own storage

| | |
|---|---|
| Category | `memory` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | An engine needs more stack than its task has, writes more octets than the output area holds, or reads or writes outside its own queue and slot arrays |
| Cause | A large engine built on the stack (the `[link]` capacities of the C ABI, `file:crates/aldis-ffi/capacities.toml`, are 8 192 octets of queue and 2 048 of data field, and the ChannelMux and receivers carry arrays of VC slots); an area shorter than the documented bound; a defect in an index into a queue or slot |
| Mission effect | An overrun of the stack or of a caller's buffer silently overwrites other memory (S1) |
| Requirements | LNK-075, API-061, API-062, GEN-012, GEN-023, VER-011, CABI-030, CABI-031 |
| Evidence | `record:VER-011`; `check:stack`; `job:stack-sizes`; `file:xtask/stack-budgets.toml`; `test:crates/aldis-link/tests/engine_housekeeping.rs::an_area_too_short_fails_with_no_change`; `test:crates/aldis-link/tests/frame_receiver.rs::an_area_shorter_than_the_bound_fails_before_any_change`; `test:crates/aldis-link/tests/map_reassembler_engine.rs::an_area_that_is_too_short_fails_the_input_and_changes_nothing`; `test:crates/aldis-link/tests/map_segmenter_engine.rs::next_fdu_needs_the_whole_area_and_changes_nothing_without_it`; `test:crates/aldis-link/tests/channel_mux_engine.rs::an_opportunity_needs_the_whole_frame_and_changes_nothing_without`; `test:crates/aldis-ffi/tests/link_gen.rs::buffers_must_not_overlap_the_storage`; `test:crates/aldis-ffi/tests/link_mux.rs::the_step_checks_its_pointers`; `fuzz:link_frame_receiver`; `fuzz:link_mux`; `check:exerciser-thumbv7em-none-eabihf`; `check:exerciser-armv7r-none-eabihf`; `check:exerciser-riscv32imac-unknown-none-elf` |
| Residual risk | An input that writes to the output area fails with `BufferTooSmall` and the needed size when the area is shorter than the documented bound (LNK-075), before it changes any state; indexes into the engines' arrays are checked (GEN-020), and a fuzz run found no write outside an area. The engines of the C ABI are built in place in the caller's storage (the head and the octet buffers, #240), so the stack of a C ABI call does not grow with the capacities: the largest link entry points measured by `cargo xtask stack` are `sym:aldis_channel_mux_init` (8 152 bytes at the budget of 10 240) and `sym:aldis_channel_mux_step` (7 048 bytes), within the budgets of every target. The Rust engines built by value (`new`) on the stack of the integrator's task are larger by their capacities: the exerciser's TM ChannelMux of 4 VCs and 48-octet frames needs 3 792 bytes to be constructed, and a larger capacity needs more, which only the integrator can bound. The MapSegmenter, the MapReassembler, and the CltuPacker have C ABI handles built at the same sizes (M4 WP-7d, #260), so `cargo xtask stack` measures them too: on thumbv7em `sym:aldis_map_segmenter_step` needs 4 464 bytes, `sym:aldis_map_reassembler_step` 1 680, and `sym:aldis_cltu_packer_step` 640, and the initialisation `sym:aldis_map_segmenter_init` 5 024 (`record:VER-011`, the report `target/spec/stack-<target>.md` of the run), all within the budget of 10 240 |
| Assumptions | `asm:ASM-3`; `asm:ASM-19` |
| Proposals | `proposal:P-LINK-01` |
| Claims | `ac:AC-4.2`; `ac:AC-4.3`; `ac:AC-4.4` |

### HZ-LINK-11 A step panics

| | |
|---|---|
| Category | `panic` |
| Status | `residual` |
| Severity | S2 |
| Failure mode | An engine panics on some input, configuration, or call sequence |
| Cause | An unchecked index, arithmetic, or `unwrap` in the engines, the queues, or the header writers; an internal invariant that an input breaks |
| Mission effect | The calling task stops (`hz:HZ-X-02`) |
| Requirements | GEN-020, GEN-021, VER-010, VER-022, API-057 |
| Evidence | `record:VER-010`; `check:exerciser-thumbv7em-none-eabihf`; `check:exerciser-armv7r-none-eabihf`; `check:exerciser-riscv32imac-unknown-none-elf`; `nightly-check:rust-sparc-leon3`; `check:clippy`; `record:VER-022`; `fuzz:link_packet_roundtrip`; `fuzz:link_map_roundtrip`; `fuzz:link_map_inputs`; `fuzz:link_mux`; `fuzz:link_frame_receiver`; `job:fuzz`; `test:crates/aldis-link/tests/frame_receiver.rs::no_frame_makes_a_receiver_fail_internally`; `test:crates/aldis-link/tests/channel_mux_engine.rs::no_sequence_of_inputs_breaks_the_engine`; `test:crates/aldis-link/tests/map_roundtrip.rs::the_segmenter_never_reports_an_internal_error`; `test:crates/aldis-link/tests/map_roundtrip.rs::the_packer_never_reports_an_internal_error`; `check:coverage` |
| Residual risk | The link check (VER-010) calls every engine of the library at the capacities of the exerciser and every C ABI function; a panic path that survives optimisation fails the link. The engines map an internal inconsistency to an `InternalError` return (never a panic, GEN-020); the lines that return it are the waived misses of `check:coverage`, each one an error return that no input reaches, and the fuzz targets and the property tests assert that no input produces one. The residual of `hz:HZ-X-02` applies: other capacities than the exerciser's are not linked |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-2.1`; `ac:AC-2.2`; `ac:AC-2.3` |

### HZ-LINK-12 A rejected input changes the state

| | |
|---|---|
| Category | `state-corruption` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A step that returns an error leaves the engine changed (a half-written queue, a started field, a slot taken, a count advanced), so that the next call works on a state that is not the one the caller believes |
| Cause | A check that runs after the first change of state; an action push that fails after the state changed |
| Mission effect | A packet half-queued or a count skipped, found only when the receiver reports a gap or a malformed packet |
| Requirements | API-052, API-057, API-058, GEN-042, LNK-003, LNK-075 |
| Evidence | `test:crates/aldis-link/tests/channel_mux_engine.rs::an_offer_the_vc_cannot_have_is_refused_and_changes_nothing`; `test:crates/aldis-link/tests/channel_mux_engine.rs::a_slot_update_the_configuration_cannot_take_is_refused`; `test:crates/aldis-link/tests/generators_engine.rs::an_sdu_the_generator_cannot_take_is_refused_and_changes_nothing`; `test:crates/aldis-link/tests/generators_engine.rs::bits_the_generator_cannot_take_are_refused_and_change_nothing`; `test:crates/aldis-link/tests/map_reassembler_engine.rs::a_blocked_fdu_whose_packets_do_not_fit_is_refused_and_changes_nothing`; `test:crates/aldis-link/tests/frame_receiver.rs::a_bitstream_pointer_beyond_the_zone_is_refused_without_a_change`; `test:crates/aldis-link/tests/frame_receiver.rs::no_frame_makes_a_receiver_fail_internally`; `test:crates/aldis-link/tests/cltu_packer.rs::frames_and_configurations_are_checked`; `test:crates/aldis-ffi/tests/link_gen.rs::a_failed_step_changes_nothing_and_writes_no_actions`; `fuzz:link_frame_receiver` |
| Residual risk | The exceptions are the counters of LNK-002 (a refused input is counted as refused, an unknown channel is counted) and the state changes that the CCSDS procedure itself defines for a rejected frame (a counted gap before a discarded frame, LNK-051). The tests above check each refusal with an unchanged copy of the engine (or, for the receiver, a twin that refused the frame for an area of no octet); the fuzz targets check that no refusal is an `InternalError`, not that the state is equal, because the counters change |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-5.4`; `ac:AC-1.9` |

### HZ-LINK-13 A single-event upset changes the state of an engine

| | |
|---|---|
| Category | `state-corruption` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A bit of an engine's state (the queue, a slot, a count, a pointer, the OID generator, a configuration) changes between two calls, and the engine then writes a wrong frame, a wrong count, or a packet with a changed octet |
| Cause | A single-event upset in memory the platform does not protect |
| Mission effect | A frame or packet with a wrong octet or count; the frame check of the receiver still passes, because the sender computes the FECF after the change |
| Requirements | GEN-015, RS-012, RS-020, GEN-020 |
| Evidence | `record:GEN-015`; `file:docs/hardware-assumptions.md` |
| Residual risk | The engines hold more state between calls than the codecs, and all of it is plain data in the engine's own storage (a queue of octets, slots, counts, a configuration). They do not re-check it: a corrupted length or index gives a wrong result or an error, not an access outside a buffer (GEN-020), but a corrupted octet in a queue is sent as it is. Only the platform's memory protection closes this (`asm:ASM-1`, `hz:HZ-X-01`); the C ABI handle's tag detects a foreign or deinitialised handle, not a changed bit of the engine |
| Assumptions | `asm:ASM-1` |
| Proposals | — |
| Claims | `ac:AC-4.4` |

### HZ-LINK-14 A C caller misuses an engine handle

| | |
|---|---|
| Category | `abi-misuse` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A C caller passes a null, misaligned, uninitialised, deinitialised, or foreign handle, storage that overlaps its buffers, an input struct whose tag or length contradicts its pointer, or a configuration the engine's constructor refuses, and the library does anything but return an error |
| Cause | A defect in the caller |
| Mission effect | Undefined behaviour in the caller's process (`hz:HZ-X-05`) |
| Requirements | CABI-022, CABI-030, CABI-031, CABI-032, CABI-040, GEN-071 |
| Evidence | `test:crates/aldis-ffi/tests/link_gen.rs::storage_is_checked_and_tagged`; `test:crates/aldis-ffi/tests/link_gen.rs::buffers_must_not_overlap_the_storage`; `test:crates/aldis-ffi/tests/link_gen.rs::the_protocol_is_chosen_in_the_configuration`; `test:crates/aldis-ffi/tests/link_gen.rs::every_protocol_takes_a_release_deadline_and_refuses_a_bad_configuration`; `test:crates/aldis-ffi/tests/link_rx.rs::the_extractor_storage_is_checked_and_tagged`; `test:crates/aldis-ffi/tests/link_rx.rs::the_receiver_storage_is_checked_and_tagged`; `test:crates/aldis-ffi/tests/link_rx.rs::the_receiver_configuration_is_checked`; `test:crates/aldis-ffi/tests/link_rx.rs::every_protocol_refuses_a_bad_extractor_configuration_and_serves_loss_and_discard`; `test:crates/aldis-ffi/tests/link_mux.rs::the_storage_is_checked_and_tagged`; `test:crates/aldis-ffi/tests/link_mux.rs::the_step_checks_its_pointers`; `test:crates/aldis-ffi/tests/link_mux.rs::every_member_of_the_configuration_is_read_and_checked`; `test:crates/aldis-ffi/tests/link_mux.rs::every_protocol_refuses_a_bad_member_and_reports_its_status`; `check:header`; `test:crates/aldis-ffi/tests/link_map.rs::the_segmenter_storage_is_checked_and_tagged`; `test:crates/aldis-ffi/tests/link_map.rs::the_segmenter_protocol_is_chosen_in_the_configuration`; `test:crates/aldis-ffi/tests/link_map.rs::the_reassembler_storage_is_checked_and_tagged`; `test:crates/aldis-ffi/tests/link_cltu.rs::the_storage_is_checked_and_tagged`; `test:crates/aldis-ffi/tests/link_cltu.rs::the_protocol_is_chosen_in_the_configuration` |
| Residual risk | As `hz:HZ-X-05`: a null pointer, a misaligned address, a wrong tag, an input of another handle's kind, overlapping buffers, and an inconsistent length are errors; a wrong non-null pointer or an overstated length is undefined behaviour the library cannot detect. The storage is the caller's: the engine is built in it by `init` and runs there, so a caller that moves or copies the storage while the engine lives breaks the engine (CABI-031). |
| Assumptions | `asm:ASM-6`; `asm:ASM-13` |
| Proposals | — |
| Claims | `ac:AC-6.2`; `ac:AC-6.1` |

### HZ-LINK-15 A reading of the Blue Book's silence differs from the peer's

| | |
|---|---|
| Category | `hidden-deviation` |
| Status | `residual` |
| Severity | S4 |
| Failure mode | Where the Blue Books are silent or leave a choice, the engine does something a peer implementation does differently, and the difference shows only against that peer: for example the content of a Space Packet Idle Packet that does not fit (LNK-013), the First Header Pointer of a field that begins with the tail of a packet (OI-69), the handling of an unknown channel's counts (OI-77), the action of a gap before a discarded frame, the choice of a COP-1 hand-off |
| Cause | A decision the standard does not settle, taken by the maintainer (OI-73 to OI-78) and implemented by the engines |
| Mission effect | Frames or packets that a conforming peer reads differently; no data is lost on the library's side |
| Requirements | LNK-013, LNK-051, LNK-061, LNK-062, LNK-078, LNK-080, LNK-081, LNK-082 |
| Evidence | `oi:OI-73`; `oi:OI-74`; `oi:OI-75`; `oi:OI-76`; `oi:OI-77`; `file:docs/09-open-issues.md`; `vector:link-send-pg-tm-lnk-011-oi69-tail-ends-at-zone-end`; `vector:link-recv-tm-unknown-vc-counts-mc`; `test:crates/aldis-link/tests/engine_housekeeping.rs::judgement_calls_of_oi_78_for_the_extractor` |
| Residual risk | Each open reading is an OI with the maintainer's decision, and the engine's behaviour is a vector or a test named for it. No independent implementation has cross-checked the engines (`df:DF-9` for the codecs, and M5 is the cross-check); a mission that exchanges frames with a peer reads the OIs above as part of its interface agreement (`asm:ASM-8`) |
| Assumptions | `asm:ASM-8` |
| Proposals | — |
| Claims | `ac:AC-1.9`; `ac:AC-5.2` |

## Proposals

| ID | Kind | Proposal | Status |
|---|---|---|---|
| P-LINK-01 | `tooling` | When the handles of the MapSegmenter, the MapReassembler, and the CltuPacker are built (M4 WP-7d), build them in `aldis-ffi` at the `[link]` sizes as the other engines are, so that `cargo xtask stack` measures their entry points at those sizes, and record their worst case in `record:VER-011` | `accepted`: written in #260 (M4 WP-7d), measured in `record:VER-011` |
