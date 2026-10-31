# Hazards: COP-1 (FOP-1 and FARM-1)

Failure modes of `aldis-cop1` (the FOP-1 and FARM-1 engines) and of its C ABI (the FOP-1 and FARM-1 handles), in the format of [hazards.md](../hazards.md) §1.7. Standard: [cop1.md](../../standards/cop1.md), 232.1-B-2 with Cor. 1, with the TC frame and the CLCW of 232.0-B-4. The failure modes common to all crates (panics, loops, stack, the C ABI, single-event upsets) are in [hazards.md](../hazards.md) §3; a hazard here repeats one only where the engines add a cause of their own.

The engines are sans-I/O (GEN-030): they read no clock, send no frame, and hold no delivery buffer. Everything around them is the caller's: the link, the timer, the choice of which engine gets which CLCW or frame, the delivery buffer, and the reaction to an Alert. The hazards below are the failure modes of the engines' contract and the obligations on the caller that the contract leaves open; the assumptions are `asm:ASM-14`, `asm:ASM-15`, `asm:ASM-16`, and `asm:ASM-17` of the [assurance case](../assurance-case.md#assumptions-and-context). The Blue Book guarantees (232.1 §2.2.1, §4.3.2 for the end of the guarantee by an Alert, §5.1.9.3, §5.2.17) hold for a transfer over a link in the way COP-040 states: in-order, complete, duplicate-free delivery, or an Alert; under Timeout Type 1, a suspended transfer is a third outcome.

## Coverage

| Category | Hazards | State |
|---|---|---|
| `corrupt-accepted` | HZ-COP1-01, HZ-COP1-02, HZ-COP1-18 | done |
| `valid-rejected` | HZ-COP1-03 | done |
| `misrouting` | HZ-COP1-04 | done |
| `truncation` | HZ-COP1-05 | done |
| `wrong-value` | HZ-COP1-06, HZ-COP1-07 | done |
| `nonconformant-output` | HZ-COP1-08 | done |
| `unbounded-time` | HZ-COP1-09, HZ-COP1-10 | done |
| `memory` | HZ-COP1-11 | done |
| `panic` | HZ-COP1-12 | done |
| `state-corruption` | HZ-COP1-13, HZ-COP1-14, HZ-COP1-15 | done |
| `abi-misuse` | HZ-COP1-16 | done |
| `hidden-deviation` | HZ-COP1-17 | done |

## Hazards

### HZ-COP1-01 FARM-1 accepts a Type-AD frame that it should discard

| | |
|---|---|
| Category | `corrupt-accepted` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | FARM-1 delivers the FDU of a valid Type-AD frame whose N(S) is not V(R): a frame already accepted (duplicate), one that skips a lost frame (gap), or one in the lockout area. The higher procedures then see an FDU twice, out of order, or with a hole, although the CLCW tells FOP-1 otherwise |
| Cause | A defect in the classification of N(S) against V(R), PW, and NW modulo 256 (232.1 §6.1.8.3), a wrong cell of Table 6-1, or an input that skips the table |
| Mission effect | A telecommand executed twice, out of sequence, or with a command missing between two that depend on each other: the loss of the service guarantee of Type-AD |
| Requirements | COP-002, COP-003, COP-004, COP-005, COP-030, COP-040, COP-041 |
| Evidence | `test:crates/aldis-cop1/tests/farm1_table.rs::test_exhaustive_33_cells`; `test:crates/aldis-cop1/tests/farm1.rs::every_cell_of_table_6_1_through_step`; `test:crates/aldis-cop1/tests/farm1.rs::window_classification_with_modular_sequence_numbers`; `test:crates/aldis-cop1/tests/farm1.rs::window_classification_with_an_asymmetric_window`; `test:crates/aldis-cop1/tests/farm1.rs::window_constructors_enforce_table_7_2`; `test:crates/aldis-cop1/tests/farm1_properties.rs::window_classification_matches_the_inequalities`; `test:crates/aldis-cop1/tests/farm1_properties.rs::any_input_sequence_keeps_the_invariants`; `test:crates/aldis-cop1/tests/event_sequences.rs::farm1_stays_within_table_6_1_under_any_event_sequence`; `test:crates/aldis-cop1/tests/closed_loop.rs::duplicated_frames_and_reports_deliver_once`; `test:crates/aldis-cop1/tests/closed_loop.rs::forward_reordering_is_repaired`; `test:crates/aldis-cop1/tests/closed_loop.rs::any_link_gives_the_service_guarantee_or_an_alert`; `vector:cop1-farm1-s1-e1`; `vector:cop1-farm1-s1-e3`; `vector:cop1-farm1-s1-e4`; `vector:cop1-farm1-s1-e5`; `vector:cop1-farm1-s3-e1`; `vector:cop1-farm1-window-positive-edge`; `vector:cop1-farm1-window-negative-edge`; `vector:cop1-farm1-vr-wrap`; `vector:cop1-farm1-window-not-allowed`; `test:crates/aldis-ffi/tests/vectors.rs::farm1_table`; `test:crates/aldis-ffi/tests/vectors.rs::farm1_window`; `test:crates/aldis-ffi/tests/vectors.rs::farm1_flow_clcw`; `test:crates/aldis-cop1/src/farm1.rs::flag_actions_of_every_cell_agree_with_the_next_state` |
| Residual risk | The 33 cells of Table 6-1 are data, each with a vector from the model written blind (TV-023, COP-003), and the window edges, the wrap of V(R), and the asymmetric window of 232.1 §6.1.8.3.2 are vectors of their own. The constructor `from_tc` of `Farm1Frame` takes a `ValidatedTcFrame`, but `Farm1Frame` is a public enum, and the README, the tests, and the C input build one from raw fields (`file:crates/aldis-cop1/src/farm1.rs`, `file:crates/aldis-cop1/README.md`). Running the Frame Validation Checks (TC-021, TC-023) before FARM-1 is therefore the caller's duty in both APIs (`asm:ASM-2`), not a control of the engine: a frame that skipped them, with an error the FECF would have caught, is accepted. What the engine does not see: a Type-BD frame has no sequence number and is delivered each time it is received (232.1 Table 6-1 E6), so a duplicate Type-BD frame is delivered twice; a valid Type-AD frame whose sequence number the channel changed to V(R) (an error the coding sublayer and the FECF missed, `hz:HZ-TC-03`, `hz:HZ-X-06`) is accepted as the expected frame; and the Unlock and Set V(R) commands, which a Type-BC frame carries, are not authenticated by COP-1 (232.0 security is outside the engines). The engines do not check the Virtual Channel of the frame (`hz:HZ-COP1-04`) |
| Assumptions | `asm:ASM-2`; `asm:ASM-4`; `asm:ASM-15` |
| Proposals | — |
| Claims | `ac:AC-1.8`; `ac:AC-5.6` |

### HZ-COP1-02 FOP-1 accepts a CLCW with an error, or a stale CLCW, as a report

| | |
|---|---|
| Category | `corrupt-accepted` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A CLCW that the channel changed (or an old report that arrives late) acknowledges Type-AD frames the receiver did not accept, or clears a Retransmit, Wait, or Lockout condition that holds, and FOP-1 purges master copies it should keep, with a Positive Confirm for each |
| Cause | A channel error in the OCF of the frame that carries the CLCW (232.0 §4.2.1) that the coding sublayer and the FECF of that frame missed; a report overtaken by a later one; a word of another COP or of a Type-1 report taken for a COP-1 report |
| Mission effect | A frame lost on the way is reported delivered (a Positive Confirm), and is never retransmitted: a telecommand missing at the spacecraft that the ground believes was received |
| Requirements | COP-001, COP-012, COP-017, TC-030, API-083, VER-064 |
| Evidence | `vector:cop1-fop1-clcw-pattern-type`; `vector:cop1-fop1-clcw-pattern-cop`; `vector:cop1-fop1-alert-suppression`; `vector:cop1-fop1-nr-invalid-wrap`; `vector:cop1-fop1-s1-e13`; `vector:cop1-fop1-s2-e13`; `vector:cop1-fop1-s1-e14`; `vector:cop1-fop1-s2-e14`; `vector:cop1-fop1-s1-e15`; `vector:cop1-fop1-s6-e15`; `test:crates/aldis-cop1/src/fop1.rs::a_clcw_that_matches_several_events_gets_the_highest_ranked`; `test:crates/aldis-cop1/src/fop1.rs::classification_of_directives_and_responses`; `test:crates/aldis-cop1/src/fop1.rs::no_alert_follows_an_alert_before_an_initiate_ad`; `test:crates/aldis-cop1/tests/closed_loop.rs::reordered_reports_complete_or_alert`; `test:crates/aldis-cop1/tests/closed_loop.rs::return_loss_is_repaired_by_the_next_report_or_the_timer`; `test:crates/aldis-tc/tests/properties.rs::clcw_decodes_any_word`; `test:crates/aldis-ffi/tests/vectors.rs::fop1_clcw`; `test:crates/aldis-ffi/tests/vectors.rs::fop1_table` |
| Residual risk | FOP-1 classifies a word before it acts: a Control Word Type of 1, or a COP in Effect other than `01`, is the invalid pattern (E15, an Alert[CLCW]); a CLCW with the Lockout Flag is E14 (an Alert[Lockout]); an N(R) outside NN(R) to V(S) modulo 256 is E13 (an Alert[NN(R)]); the ranking is OI-62 (`oi:OI-62`). A report overtaken by a later one usually has an N(R) below NN(R), which is E13 and an Alert; `reordered_reports_complete_or_alert` shows only that some reordering seed raises an Alert, not that no stale report is accepted. A stale report is accepted when its N(R) is in the current NN(R) to V(S): after a Set V(S) in S6 (E35 sets V(S) and NN(R) to V*(S)) a report still in flight from the earlier session can have such an N(R), and a late report with N(R) = NN(R) passes the classification and FOP-1 acts on its flags. An N(R) that the channel moved to another value in NN(R) to V(S) is a valid report; the CLCW has no check of its own, so only the FECF of the TM, AOS, or USLP frame that carries it detects the error (`hz:HZ-X-06`, `asm:ASM-4`), and a bit error that changes N(R) to a larger value inside the window acknowledges frames that were not received. COP-1 recovers nothing from it (232.1 §2.2.1 gives the guarantee only with a high probability of success, and the project reads this as resting on a return link whose reports the lower layers protect) |
| Assumptions | `asm:ASM-4`; `asm:ASM-10` |
| Proposals | — |
| Claims | `ac:AC-1.8`; `ac:AC-5.5`; `ac:AC-5.6` |

### HZ-COP1-03 FARM-1 discards valid frames and stays closed to the sender

| | |
|---|---|
| Category | `valid-rejected` |
| Status | `residual` |
| Severity | S3 |
| Failure mode | FARM-1 discards the frames of a healthy sender: in Wait (S2) while the caller has no delivery buffer, and in Lockout (S3) after one frame outside the sliding window, where every Type-AD frame is discarded until an Unlock command arrives (232.1 Table 6-1, §6.3.2.2) |
| Cause | A `buffer_available` of false that does not match the caller's delivery buffer, or a `BufferReleased` input that never comes; a sender whose K is above the PW of the receiver, or whose window is not the receiver's, so that frames fall in the lockout area; a V(R) that does not match the sender's V(S) at start-up (OI-58) |
| Mission effect | Telecommands are discarded, reported by the Lockout Flag of the CLCW, and the sender raises an Alert and stops; commanding resumes only after the operators Unlock FARM-1 (a Type-BC command) and Initiate AD. The loss is reported, not silent |
| Requirements | COP-002, COP-030, COP-033, COP-036, COP-035 |
| Evidence | `vector:cop1-farm1-buffer-wait`; `vector:cop1-farm1-wait-to-lockout`; `vector:cop1-farm1-set-vr-in-lockout`; `vector:cop1-farm1-s2-e1`; `vector:cop1-farm1-s3-e7`; `test:crates/aldis-cop1/tests/farm1.rs::buffer_availability_is_the_callers_input`; `test:crates/aldis-cop1/tests/farm1.rs::lockout_is_left_only_by_unlock`; `test:crates/aldis-cop1/tests/closed_loop.rs::a_receiver_that_holds_its_buffer_waits_and_resumes`; `test:crates/aldis-cop1/tests/closed_loop.rs::a_dead_forward_link_ends_in_an_alert_at_the_transmission_limit`; `test:crates/aldis-cop1/tests/farm1.rs::clcw_follows_the_state_after_every_input`; `test:crates/aldis-ffi/tests/vectors.rs::farm1_flow_clcw` |
| Residual risk | Every discard is an action (`FrameDiscarded` with the event and the prior state, COP-004) and appears in the CLCW as the Wait, Retransmit, or Lockout Flag (COP-035), so the loss is reported to the caller and to the sender. Whether a lockout happens depends on managed parameters of both ends that no call can compare (K at most PW of the peer FARM, a note to Table 7-1 in 232.1 §7.2; `Farm1Window` against the sender's, §7.3, Table 7-2): the types allow only the pairs Table 7-2 allows, and the relation between the two ends is `asm:ASM-15`. The engine does not decide whether a buffer exists: `buffer_available` is the caller's input (COP-033, `asm:ASM-17`). After an Unlock the engine returns an `Unlocked` action so that the higher procedures can purge their buffers (232.1 §6.3.2.2 note, COP-036); a caller that ignores it keeps a partial transfer across the reset |
| Assumptions | `asm:ASM-15`; `asm:ASM-17` |
| Proposals | — |
| Claims | `ac:AC-1.8`; `ac:AC-5.6` |

### HZ-COP1-04 A CLCW or a frame reaches the engine of another Virtual Channel or spacecraft

| | |
|---|---|
| Category | `misrouting` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A CLCW of one Virtual Channel is applied by the FOP-1 of another, or a frame of one Virtual Channel is accepted by the FARM-1 of another, so that the sequence state of one channel is driven by the traffic of a second |
| Cause | A caller that routes by a wrong key; a CLCW that carries the Virtual Channel Identifier but no Spacecraft ID (232.0 §4.2.1.6), so the same identifier on two spacecraft, reached through the same lower layer, looks alike |
| Mission effect | Frames of one channel acknowledged by the report of another, or accepted into the wrong channel's sequence: a telecommand lost or executed on the wrong virtual channel |
| Requirements | COP-001, COP-002, COP-006 |
| Evidence | `vector:cop1-fop1-other-vcid`; `test:crates/aldis-cop1/tests/fop1_uslp.rs::a_clcw_of_another_virtual_channel_is_refused`; `test:crates/aldis-cop1/src/fop1.rs::every_failure_of_a_cell_leaves_the_state_unchanged`; `test:crates/aldis-cop1/tests/farm1_properties.rs::tc_frames_map_to_farm1_frames`; `test:crates/aldis-ffi/tests/vectors.rs::fop1_clcw`; `test:crates/aldis-ffi/tests/engines.rs::a_fop1_run_from_initial_to_a_retransmission` |
| Residual risk | FOP-1 compares the Virtual Channel of each CLCW with its own and refuses another with `UnknownChannel` and no state change; the Spacecraft ID is not in the CLCW, so the caller routes by the Spacecraft ID of the carrying frame (`asm:ASM-15`). The Frame Validation Checks (TC-021, TC-023) check the Spacecraft ID and the VCID against the channel table, but running them is the caller's duty (`asm:ASM-2`), not a control of the engine; and the engine holds the VCID only for the CLCW it builds, and neither the C input nor the Rust input carries the frame's VCID, so it cannot compare. A caller that gives one FARM-1 the frames of two channels gets one sequence for both and a CLCW that reports one of them. The check would need the frame's VCID in the input (`proposal:P-COP1-01`) |
| Assumptions | `asm:ASM-2`; `asm:ASM-15` |
| Proposals | `proposal:P-COP1-01` |
| Claims | `ac:AC-1.8`; `ac:AC-6.2` |

### HZ-COP1-05 An FDU or a frame is stored, delivered, or transmitted shortened

| | |
|---|---|
| Category | `truncation` |
| Status | `controlled` |
| Severity | S1 |
| Failure mode | A Type-AD or Type-BD FDU longer than the store or the frame holds is copied in part, or delivered in a range shorter than the data field, or an output area that is too short receives the first octets of a frame, and the action reports success |
| Cause | A request with an FDU above the Maximum Transfer Frame Length of the channel; a caller's output area below it; a length computation that clamps |
| Mission effect | A telecommand segment or packet that ends early, accepted and acknowledged as sent (S1), or a delivered FDU missing its tail |
| Requirements | COP-006, COP-010, COP-011, COP-033, API-061, API-062, GEN-014, GEN-020 |
| Evidence | `test:crates/aldis-cop1/src/fop1.rs::fdu_and_area_checks`; `test:crates/aldis-cop1/src/fop1.rs::queries_and_construction`; `test:crates/aldis-ffi/tests/engines.rs::configurations_are_checked_once`; `test:crates/aldis-cop1/src/fop1.rs::an_input_that_fails_leaves_the_engine_and_the_store_unchanged`; `test:crates/aldis-cop1/src/farm1.rs::an_area_shorter_than_the_fdu_fails_before_anything_is_written`; `test:crates/aldis-cop1/tests/farm1.rs::delivered_range_addresses_the_output_area`; `test:crates/aldis-cop1/tests/farm1.rs::errors_leave_the_state_unchanged_and_write_nothing`; `test:crates/aldis-cop1/src/actions.rs::range_slices_the_area`; `test:crates/aldis-ffi/tests/engines.rs::a_fop1_step_checks_every_argument`; `test:crates/aldis-ffi/tests/engines.rs::a_farm1_step_checks_every_argument`; `vector:cop1-fop1-directive-values`; `vector:cop1-fop1-wait-queue` |
| Residual risk | An empty FDU is `InvalidField`, one above the target's limit is `OutOfRange` (`frame_len`), and an area shorter than the frame or the data field is `BufferTooSmall` with the needed size; each check runs before the engine writes an octet and leaves the state unchanged (API-058, API-061). No path clamps a length. The constructor of `Fop1` and `sym:aldis_fop1_init` reject a target whose frames do not fit the `F` octets of the store (`InvalidConfig`, `max_frame_len`), so a frame that the store cannot hold does not exist |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-4.3`; `ac:AC-5.4` |

### HZ-COP1-06 FOP-1 gets the sequence arithmetic wrong and acknowledges frames that were not received

| | |
|---|---|
| Category | `wrong-value` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The comparison of N(R) with NN(R) and V(S) modulo 256 (232.1 §5.3.2.1, §5.1.8) is wrong at the wrap, or V(S), NN(R), or the Transmission_Count takes a wrong value after a directive (Set V(S), Initiate AD with Set V(R), a purge), so FOP-1 removes a frame the receiver has not accepted or retransmits an acknowledged one, or numbers a new frame with a duplicate N(S) |
| Cause | A defect in the modular comparison, in the removal of the acknowledged frames, or in a directive's effect on V(S) and NN(R); a reading of the Blue Book that the engine takes differently from the peer (OI-60) |
| Mission effect | A Positive Confirm for a frame that the receiver never accepted; a frame numbered like another and discarded as a duplicate by FARM-1 |
| Requirements | COP-001, COP-005, COP-011, COP-012, COP-013, COP-015, COP-040, COP-041 |
| Evidence | `test:crates/aldis-cop1/tests/fop1_table.rs::test_exhaustive_300_cells`; `test:crates/aldis-cop1/tests/fop1_vectors.rs::fop1_scenarios`; `test:crates/aldis-cop1/tests/event_sequences.rs::fop1_stays_within_table_5_1_under_any_event_sequence`; `test:crates/aldis-cop1/tests/closed_loop.rs::an_ideal_link_transfers_everything_without_retransmission`; `test:crates/aldis-cop1/tests/closed_loop.rs::forward_loss_is_repaired_by_retransmission`; `test:crates/aldis-cop1/tests/closed_loop.rs::every_impairment_at_once`; `test:crates/aldis-cop1/tests/closed_loop.rs::any_link_gives_the_service_guarantee_or_an_alert`; `vector:cop1-fop1-modulo-256`; `vector:cop1-fop1-nr-invalid-wrap`; `vector:cop1-fop1-s1-e5`; `vector:cop1-fop1-s1-e6`; `vector:cop1-fop1-s1-e13`; `vector:cop1-fop1-ad-run`; `vector:cop1-fop1-initiate-set-vr`; `vector:cop1-fop1-directive-values`; `vector:cop1-fop1-terminate-purge`; `test:crates/aldis-ffi/tests/vectors.rs::fop1_table`; `test:crates/aldis-ffi/tests/vectors.rs::fop1_wrap_directives` |
| Residual risk | The 300 cells of Table 5-1 (S1 to S6, E1 to E46 and E101 to E104) are data with a vector each, from the model written blind, and the wrap of the frame number, the values of the directives at their edges, and the purge are vectors of their own (the cell-by-cell review is [cop1-tables.md](../reviews/cop1-tables.md), COP-003). The property tests drive random event sequences and the closed loop drives both engines through a link with loss, duplication, reordering, and delay. The ranking of a word that matches several events (E15, E14, E13) is the project's reading (OI-62), and the effect of a purge on NN(R) is OI-60; both are resolved and written into the vectors, and neither is a statement of the Blue Book (`hz:HZ-COP1-17`) |
| Assumptions | `asm:ASM-16` |
| Proposals | — |
| Claims | `ac:AC-1.8`; `ac:AC-5.6` |

### HZ-COP1-07 FARM-1 reports a state that is not its own, or acknowledges a frame the caller did not take

| | |
|---|---|
| Category | `wrong-value` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The CLCW that FARM-1 builds carries a V(R), a flag, or a FARM-B Counter that does not match the state; or FARM-1 accepts a frame (E1) and moves V(R) on, and the higher procedures never receive the FDU |
| Cause | A defect in the CLCW from the state (E11), in the FARM-B Counter (modulo 4), or in the flag a cell sets; a caller that sets `buffer_available` to true without a buffer, or drops the Deliver range before it has copied it |
| Mission effect | FOP-1 receives an N(R) past a frame that the spacecraft's software never saw: a Positive Confirm for a telecommand that did not execute |
| Requirements | COP-033, COP-034, COP-035, COP-002, COP-031, COP-036, TC-030, TC-031 |
| Evidence | `test:crates/aldis-cop1/tests/farm1.rs::clcw_follows_the_state_after_every_input`; `test:crates/aldis-cop1/tests/farm1.rs::event_e11_reports_the_state_without_changing_it`; `test:crates/aldis-cop1/tests/farm1.rs::farm_b_counter_counts_valid_type_b_frames_modulo_four`; `test:crates/aldis-cop1/tests/farm1.rs::physical_flags_reach_the_clcw_and_nothing_else`; `test:crates/aldis-cop1/tests/farm1.rs::bc_frame_without_a_valid_command_is_an_invalid_frame`; `test:crates/aldis-cop1/tests/farm1.rs::set_vr_sets_the_expected_sequence_number`; `test:crates/aldis-cop1/tests/farm1.rs::delivered_range_addresses_the_output_area`; `test:crates/aldis-cop1/src/farm1.rs::report_event_changes_nothing`; `vector:cop1-farm1-farm-b-counter`; `vector:cop1-farm1-clcw-fields`; `vector:cop1-farm1-physical-flags`; `vector:cop1-farm1-bc-not-a-command`; `vector:cop1-farm1-s1-e11`; `vector:cop1-farm1-s3-e11`; `test:crates/aldis-ffi/tests/engines.rs::a_farm1_step_returns_its_actions_and_leaves_the_state_on_error`; `test:crates/aldis-tc/tests/properties.rs::clcw_round_trips` |
| Residual risk | The CLCW is built from the state after the most recent input, with COP in Effect `01`, the configured VCID and Status Field, and the flags the cells set (COP-035), and a Type-BC frame that is neither command is an invalid frame that does not count (COP-031, 232.1 §6.3.2.1). What the engine cannot know is whether the caller kept its promise: `buffer_available` is an input, and the FDU of a Deliver action is in the output area until the next call (COP-033, API-061). A caller that sets it without a buffer is the cause of the failure, and the engine's CLCW is then correct about the engine and wrong about the mission (`asm:ASM-17`) |
| Assumptions | `asm:ASM-17` |
| Proposals | — |
| Claims | `ac:AC-1.8`; `ac:AC-5.6` |

### HZ-COP1-08 FOP-1 builds a frame that does not conform, or retransmits a frame that differs from the original

| | |
|---|---|
| Category | `nonconformant-output` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A Type-AD frame has an N(S) other than V(S), a Type-BD frame an N(S) other than 0, a Type-BC frame a data field other than the Unlock (`00`) or Set V(R) (`82 00 vv`) command, or a USLP frame the wrong Bypass/Sequence Control, Protocol Control Command Flag, UPID, or count; or a retransmission differs in any octet from the frame first transmitted |
| Cause | A defect in the frame construction for the target, in the master copy, or in the choice of the copy; a store that does not hold what this engine put there (`hz:HZ-COP1-14`) |
| Mission effect | A frame the receiver discards (a loss that the protocol then repairs or reports), or, for a changed N(S) or command, a frame it accepts as another (S1) |
| Requirements | COP-010, COP-011, COP-016, TC-005, TC-008, USLP-013, API-061 |
| Evidence | `test:crates/aldis-cop1/src/fop1.rs::decide_changes_nothing_and_commit_keeps_the_master_copies_in_queue_order`; `test:crates/aldis-cop1/src/fop1.rs::a_frame_a_decision_added_is_emitted_again_by_encoding_it`; `test:crates/aldis-cop1/src/fop1.rs::transmit_actions_carry_the_configured_repetitions`; `test:crates/aldis-cop1/src/fop1.rs::the_uslp_target_builds_its_frames_and_refuses_what_it_cannot_fill`; `test:crates/aldis-cop1/tests/fop1_uslp.rs::type_ad_and_type_bd_frames_carry_the_fdu_as_the_tfdz`; `test:crates/aldis-cop1/tests/fop1_uslp.rs::type_bc_frames_carry_the_control_commands_as_protocol_control`; `test:crates/aldis-cop1/tests/fop1_uslp.rs::the_frames_are_the_octets_worked_by_hand`; `test:crates/aldis-cop1/tests/fop1_uslp.rs::fop1_and_farm1_deliver_over_uslp_frames_through_a_loss`; `vector:cop1-fop1-retransmit-in-order`; `vector:cop1-fop1-initiate-unlock`; `vector:cop1-fop1-initiate-set-vr`; `vector:cop1-fop1-bd-service`; `test:crates/aldis-ffi/tests/vectors.rs::fop1_ad_service` |
| Residual risk | Every frame is built by the TC or USLP encoder of the target, so the header, the Frame Length, and the FECF are those of `hz:HZ-TC-16` and the codec files; the scenarios compare the whole frame, octet by octet, with the model, a retransmission is compared with the first transmission (`retransmit-in-order`), and the closed loop decodes every frame with the receiving codec. The USLP target is confined to the one reading of OI-66 (a one-octet sequence-controlled count, no expedited count) and has no C ABI form; a peer with another reading of the counts does not interoperate, and the engine refuses a channel it cannot fill once, at construction (`oi:OI-66`). Systematic retransmission (a repetition count) is the lower layer's: the engine only annotates the Transmit action (COP-016) |
| Assumptions | `asm:ASM-8` |
| Proposals | — |
| Claims | `ac:AC-1.8`; `ac:AC-6.1` |

### HZ-COP1-09 A lost frame is neither retransmitted nor reported

| | |
|---|---|
| Category | `unbounded-time` |
| Status | `residual` |
| Severity | S2 |
| Failure mode | A transfer stalls with no retransmission and no Alert: the timer T1 is not armed where the table arms it, restarts too early or late, or a Transmit Request is never answered, so that an Out flag stays Not Ready; or FOP-1 suspends (Timeout Type 1) and nobody resumes it |
| Cause | A defect in the start, restart, or cancel of T1 (232.1 §5.1.9.1); a caller that does not feed `Timeout` at the deadline, does not feed `TransmitDone` for a Transmit action, or does not read `next_deadline`; a monotonic clock that goes back; a T1_Initial below the loop delay or a Transmission_Limit above the mission's tolerance (232.1 §5.1.9.2, §5.1.10.2) |
| Mission effect | A telecommand that is in the Sent_Queue and never reaches the spacecraft, with no Alert to the operators; or a transfer that waits for a Resume. The service guarantee of the Blue Book holds only while the timer runs |
| Requirements | COP-013, COP-014, COP-015, COP-016, COP-017, COP-040, API-053, API-054, API-055, API-083 |
| Evidence | `vector:cop1-fop1-timer-and-limit`; `vector:cop1-fop1-suspend-resume`; `vector:cop1-fop1-window-k`; `vector:cop1-fop1-wait-queue`; `vector:cop1-fop1-s1-e16`; `vector:cop1-fop1-s1-e17`; `vector:cop1-fop1-s1-e18`; `vector:cop1-fop1-s1-e104`; `vector:cop1-fop1-s2-e16`; `vector:cop1-fop1-s1-e41`; `vector:cop1-fop1-s1-e42`; `test:crates/aldis-cop1/src/fop1.rs::suspension_set_vs_and_alerts`; `test:crates/aldis-cop1/src/fop1.rs::queries_and_construction`; `test:crates/aldis-cop1/tests/closed_loop.rs::a_dead_forward_link_ends_in_an_alert_at_the_transmission_limit`; `test:crates/aldis-cop1/tests/closed_loop.rs::timeout_type_one_suspends_resumes_and_completes`; `test:crates/aldis-cop1/tests/closed_loop.rs::a_loss_after_a_resume_leaves_the_transfer_suspended_not_alerted`; `test:crates/aldis-cop1/tests/closed_loop.rs::loss_on_both_links_with_delay`; `test:crates/aldis-cop1/tests/closed_loop.rs::a_slow_clcw_sampling_rate_still_completes`; `test:crates/aldis-ffi/tests/vectors.rs::fop1_window_timer` |
| Residual risk | The engine keeps T1 as state and reports its deadline; a `Timeout` before the deadline changes nothing (API-053); the cells restart T1 only where Table 5-1 lists it, BD frames never start it, and every Alert a cell lists is returned (API-083). The closed-loop scenarios end in complete, duplicate-free delivery, in an Alert, or, under Timeout Type 1, in a suspended transfer (COP-040): the third outcome is one that no Alert reports, because Resume starts the timer and clears Suspend_State but does not reset the Transmission_Count (232.1 §5.2.17), and the test `a_loss_after_a_resume_leaves_the_transfer_suspended_not_alerted` pins it. The engine owns no clock: the caller calls `Timeout`, answers every Transmit Request, and reads a `Suspend` or `Alert` notification (`asm:ASM-16`) |
| Assumptions | `asm:ASM-16` |
| Proposals | `proposal:P-COP1-03` |
| Claims | `ac:AC-1.8`; `ac:AC-5.6` |

### HZ-COP1-10 One input takes unbounded work, or an action list overflows

| | |
|---|---|
| Category | `unbounded-time` |
| Status | `controlled` |
| Severity | S2 |
| Failure mode | An input loops over the Sent_Queue without a bound, or produces more actions than the list holds and drops one, so that a Negative Confirm, an Alert, or a Transmit is lost without a trace |
| Cause | A purge or a removal of acknowledged frames over a queue whose length is not the capacity; an action bound written by hand that a change of the table or of the capacity outgrows |
| Mission effect | The calling task hangs until a watchdog acts (S2), or an Alert that the table lists does not reach the operators |
| Requirements | COP-006, API-057, API-083, GEN-010, GEN-022, GEN-023 |
| Evidence | `test:crates/aldis-cop1/src/farm1.rs::every_cell_runs_within_the_bound_and_only_the_na_cell_fails`; `test:crates/aldis-cop1/src/farm1.rs::lookup_matches_the_table_and_the_bound_covers_every_cell`; `test:crates/aldis-cop1/src/farm1.rs::a_list_without_room_fails_at_every_push_and_never_drops_an_action`; `test:crates/aldis-cop1/src/actions.rs::list_keeps_order_and_refuses_overflow`; `test:crates/aldis-cop1/tests/fop1_vectors.rs::the_action_bound_target_and_classification_at_run_time`; `test:crates/aldis-ffi/tests/engines.rs::capacities_are_published_and_match_the_engines`; `check:spec`; `check:stack`; `record:GEN-022`; `record:VER-011` |
| Residual risk | `fop1_max_actions` and the FARM-1 bound are computed from the table data by a `const` function, and `Fop1` fails to build when its `N` is below that value, so a table or capacity change that raises the bound fails the build, not a run (COP-006). Every loop is over a slice or a closed range or carries a bound comment (`check:spec` runs `cargo xtask loops`), and `check:stack` fails on a call cycle. The action list refuses a push beyond its capacity with an error and never drops an action (`a_list_without_room_fails_at_every_push_and_never_drops_an_action`); the sweep over every cell shows the bound holds for the table. The worst stack of the C ABI entry points of the engines is part of the published table (`hz:HZ-X-04`) |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-3.1`; `ac:AC-4.3` |

### HZ-COP1-11 A step writes outside the output area, the store, or the caller's storage

| | |
|---|---|
| Category | `memory` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A step writes a frame, an FDU, or an action list beyond the output area, beyond the `Fop1Store`, or over the caller's buffers, or reads a master copy from outside the store |
| Cause | A defect in the copy of a frame into the store or the output area (`commit`, `Transmit`, `Deliver`); through the C ABI, an output area, an input buffer, an action list, or the engine's own storage that overlap |
| Mission effect | Corruption of memory the engine was not given. In the Rust crates it is excluded by `unsafe` being forbidden (GEN-070) and by checked accesses; in the C ABI it is the `unsafe` code of the handle and the in-place write of the action list |
| Requirements | API-061, API-062, CABI-022, CABI-040, GEN-014, GEN-020, GEN-070, GEN-071 |
| Evidence | `check:clippy`; `test:crates/aldis-cop1/src/fop1.rs::decide_changes_nothing_and_commit_keeps_the_master_copies_in_queue_order`; `test:crates/aldis-cop1/src/fop1.rs::an_input_that_fails_leaves_the_engine_and_the_store_unchanged`; `test:crates/aldis-cop1/tests/farm1.rs::delivered_range_addresses_the_output_area`; `test:crates/aldis-cop1/src/farm1.rs::an_area_shorter_than_the_fdu_fails_before_anything_is_written`; `test:crates/aldis-ffi/tests/engines.rs::a_fop1_step_checks_every_argument`; `test:crates/aldis-ffi/tests/engines.rs::a_farm1_step_checks_every_argument`; `file:exerciser/src/fop1.rs`; `file:exerciser/src/farm1.rs`; `check:exerciser-thumbv7em-none-eabihf`; `check:stack`; `record:VER-011`; `hz:HZ-X-04` |
| Residual risk | `aldis-cop1` forbids `unsafe`, and every access to the store and the output area is a checked `get`, `get_mut`, or `split_at_mut`; the waivers of `file:coverage-waivers.toml` for the crate name the unreachable arms, among them a position below `Q` by the loop bound, and are justified there. A frame goes to the output area from offset 0 and at most one frame per input (API-062); an area that is too short fails before any write. The C ABI writes the engine and the store in place in the caller's storage (CABI-040): `sym:aldis_fop1_step` and `sym:aldis_farm1_step` check that the input data, the output area, and the action list do not overlap the storage or each other (CABI-022) and the tests above show each rejection; the `unsafe` blocks carry their `// SAFETY:` comments (GEN-071) and are read at review; no check runs them under Miri (`df:DF-7`). The store of the Rust API is `Q × F + F` octets, which the caller places (OI-45) |
| Assumptions | `asm:ASM-3`; `asm:ASM-6` |
| Proposals | — |
| Claims | `ac:AC-4.3`; `ac:AC-4.4`; `ac:AC-6.2` |

### HZ-COP1-12 A step panics

| | |
|---|---|
| Category | `panic` |
| Status | `residual` |
| Severity | S2 |
| Failure mode | A step of either engine, a classification, or a table lookup panics on some state, input, or event number |
| Cause | An unchecked index into the state tables or the queues, an overflow in the modulo-256 or window arithmetic, or an `unwrap` |
| Mission effect | The calling task stops; the link service of the channel is lost until a restart, which COP-1 survives by an Initiate AD directive (`hardware-assumptions.md` §3) |
| Requirements | COP-003, COP-041, GEN-020, GEN-021, VER-010, VER-001 |
| Evidence | `check:clippy`; `test:crates/aldis-cop1/tests/event_sequences.rs::fop1_stays_within_table_5_1_under_any_event_sequence`; `test:crates/aldis-cop1/tests/event_sequences.rs::farm1_stays_within_table_6_1_under_any_event_sequence`; `test:crates/aldis-cop1/tests/farm1_properties.rs::any_input_sequence_keeps_the_invariants`; `test:crates/aldis-cop1/src/fop1.rs::every_failure_of_a_cell_leaves_the_state_unchanged`; `test:crates/aldis-cop1/src/farm1.rs::every_cell_runs_within_the_bound_and_only_the_na_cell_fails`; `file:exerciser/src/fop1.rs`; `file:exerciser/src/farm1.rs`; `check:exerciser-thumbv7em-none-eabihf`; `check:exerciser-armv7r-none-eabihf`; `check:exerciser-riscv32imac-unknown-none-elf`; `record:VER-010`; `hz:HZ-X-02` |
| Residual risk | The tables are read by an exhaustive `match` with no wildcard arm (RS-021), the lookups bind one name per state and event so that no index is needed, and the lints deny unchecked indexing and arithmetic (`check:clippy`); both engines run in the exerciser's release link with the canary that fails the link on a reachable panic path (`record:VER-010`). The sweeps over every state and event, and the random sequences, assert no panic. The fuzz targets for the engines are not yet registered (`df:DF-17`), so the nightly fuzz job does not drive them; the exerciser covers the instantiations it makes, not an integrator's other capacities (`df:DF-10`), and the shipped C ABI library has the limits of `hz:HZ-X-02` |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-2.1`; `ac:AC-2.2`; `ac:AC-2.3` |

### HZ-COP1-13 A rejected input changes the state

| | |
|---|---|
| Category | `state-corruption` |
| Status | `controlled` |
| Severity | S1 |
| Failure mode | An input that fails (an unknown channel, an empty FDU, an area that is too short, a cell Table 5-1 or 6-1 marks "Not applicable") leaves the engine, the store, or the output area partly changed, so that the next input starts from a state that no sequence of inputs reaches |
| Cause | A state written before the check that fails; a cell applied to the engine in place |
| Mission effect | A V(S), NN(R), queue, or flag that disagrees with what was sent: frames numbered twice or acknowledged that were not, after a rejected request |
| Requirements | API-058, COP-005, COP-006, RS-022 |
| Evidence | `test:crates/aldis-cop1/src/fop1.rs::an_input_that_fails_leaves_the_engine_and_the_store_unchanged`; `test:crates/aldis-cop1/src/fop1.rs::every_failure_of_a_cell_leaves_the_state_unchanged`; `test:crates/aldis-cop1/tests/farm1.rs::errors_leave_the_state_unchanged_and_write_nothing`; `test:crates/aldis-cop1/src/farm1.rs::report_event_changes_nothing`; `test:crates/aldis-ffi/tests/engines.rs::a_farm1_step_returns_its_actions_and_leaves_the_state_on_error`; `test:crates/aldis-ffi/tests/engines.rs::a_fop1_step_checks_every_argument`; `vector:cop1-fop1-other-vcid`; `vector:cop1-farm1-s2-e1`; `test:crates/aldis-cop1/tests/fop1_table.rs::test_exhaustive_300_cells` |
| Residual risk | FOP-1 splits the step in two: `decide` reads the engine and the store and builds the next small state apart from them, and `commit` applies it and cannot fail (API-058, RS-022), so an error of `decide` drops the decision and nothing else has changed. FARM-1's `step` returns the next state and leaves the engine as it was on an error. A sweep drives every failure of every cell and compares the engine and the store before and after. The C ABI composes `decide` and `commit` in one call on one storage (`file:crates/aldis-ffi/src/fop1.rs`), so a C caller cannot separate them (`hz:HZ-COP1-14`) |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-5.4` |

### HZ-COP1-14 A decision is committed to another engine, another store, or a changed state

| | |
|---|---|
| Category | `state-corruption` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A Rust caller commits a `Fop1Decision` to an engine or a store other than the one that made it, commits it after the engine has taken another input, or decides with a `Fop1Store` that is not the engine's (another engine's, one that missed a commit, one not zeroed at start). The next retransmission transmits zeros or the frame of another transfer, and the state tracks octets that no input produced |
| Cause | A defect in the caller; the types do not tie the decision to the engine and the store (the decision borrows only the FDU of its input, so the borrow checker does not stop a commit after another input) |
| Mission effect | A retransmission that is not the frame first sent: the receiver discards it (a loss), or accepts a frame whose octets differ from the original (S1) |
| Requirements | API-058, API-060, COP-011, COP-006, RS-022, GEN-020 |
| Evidence | `test:crates/aldis-cop1/src/fop1.rs::a_store_of_another_engine_gives_wrong_frames_and_no_failure`; `test:crates/aldis-cop1/src/fop1.rs::a_new_store_is_zero`; `test:crates/aldis-cop1/src/fop1.rs::decide_changes_nothing_and_commit_keeps_the_master_copies_in_queue_order`; `test:crates/aldis-ffi/tests/engines.rs::a_fop1_run_from_initial_to_a_retransmission`; `test:crates/aldis-ffi/tests/engines.rs::storage_is_checked_and_tagged` |
| Residual risk | The consequence is bounded and documented, not prevented: a mismatched store gives wrong frames and never a panic, an access outside a buffer, or `unsafe` in this crate (`a_store_of_another_engine_gives_wrong_frames_and_no_failure` shows the zero frame and the absence of a failure). A `Fop1Decision` is not `Clone` and `commit` takes it by value, so it is applied at most once; that it is applied to the engine and the store it was made for, in the state it was made for, is the Rust caller's obligation (`asm:ASM-14`). The C ABI is not exposed: one call decides and commits on the engine and the store that sit in one caller storage, which the tag ties to the handle (CABI-032). Making the Rust API refuse a mismatch needs a change of its shape (`proposal:P-COP1-02`) |
| Assumptions | `asm:ASM-14` |
| Proposals | `proposal:P-COP1-02`; `proposal:P-COP1-03` |
| Claims | `ac:AC-5.4`; `ac:AC-6.1` |

### HZ-COP1-15 A single-event upset changes the state of an engine

| | |
|---|---|
| Category | `state-corruption` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A bit of V(S), NN(R), V(R), the Transmission_Count, a flag, the Sent_Queue entries, a master copy, or the state of the protocol changes between two calls, so that the engine acknowledges, retransmits, accepts, or discards on a corrupted value; or an enum takes a discriminant that names no variant, which is undefined behaviour |
| Cause | A single-event upset in memory that the platform does not protect. The state of an engine is the first long-lived state of Aldis, held for the whole mission and rewritten on every call (`file:docs/hardware-assumptions.md` §2) |
| Mission effect | Any: a wrong N(S) or V(R) is a lost or duplicated telecommand; a corrupted discriminant is undefined behaviour (`hz:HZ-X-01`) |
| Requirements | GEN-015, RS-012, RS-020, GEN-020, COP-003 |
| Evidence | `record:GEN-015`; `file:docs/hardware-assumptions.md`; `test:crates/aldis-cop1/tests/event_sequences.rs::fop1_stays_within_table_5_1_under_any_event_sequence`; `test:crates/aldis-cop1/tests/event_sequences.rs::farm1_stays_within_table_6_1_under_any_event_sequence` |
| Residual risk | The decision of the plan (D8, accepted) is to rely on platform memory protection, as for every Aldis state: EDAC with scrubbing, an uncorrectable error as a reset, and the engines restartable from their initial state by an Initiate AD directive and an Unlock (`asm:ASM-1`). The engines hold no checksum and no redundant copy of their state, so a value changed in unprotected memory is used as it is; the protocol may detect some of its effects at the other end (a wrong V(R) can put FARM-1 in Lockout, a wrong V(S) can make the next N(R) invalid, Alert[NN(R)]), and may not, and the table tests show that a state outside the tables is not reached by inputs, not that it is not reached by an upset. Bounds checks hold (GEN-020): a corrupted index gives a wrong result, not an access outside a buffer. The maintainer accepted D8, option (a), on 2026-10-31 (it may be revisited) |
| Assumptions | `asm:ASM-1` |
| Proposals | — |
| Claims | `ac:AC-4.4` |

### HZ-COP1-16 A C caller misuses a FOP-1 or FARM-1 handle

| | |
|---|---|
| Category | `abi-misuse` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A C caller calls a step on storage that was never initialized, was deinitialized, was copied, or belongs to the other engine; passes a null pointer, a misaligned pointer, or buffers that overlap the engine or each other; passes an `out_cap` that is not the true size; steps one handle from two threads; or initializes over a live engine; and the library does anything but return an error |
| Cause | A defect in the caller, or a header and a library of different versions (CABI-060) |
| Mission effect | Undefined behaviour in the caller's process, or an engine silently replaced (a COP-1 session lost without a notification) |
| Requirements | CABI-021, CABI-022, CABI-030, CABI-031, CABI-032, CABI-040, CABI-050, CABI-060, GEN-071 |
| Evidence | `test:crates/aldis-ffi/tests/engines.rs::storage_is_checked_and_tagged`; `test:crates/aldis-ffi/tests/engines.rs::configurations_are_checked_once`; `test:crates/aldis-ffi/tests/engines.rs::a_fop1_step_checks_every_argument`; `test:crates/aldis-ffi/tests/engines.rs::a_farm1_step_checks_every_argument`; `test:crates/aldis-ffi/tests/engines.rs::capacities_are_published_and_match_the_engines`; `sym:aldis_fop1_init`; `sym:aldis_fop1_step`; `sym:aldis_farm1_init`; `sym:aldis_farm1_step`; `check:header`; `file:exerciser/src/fop1.rs`; `file:exerciser/src/farm1.rs`; `record:CABI-010`; `hz:HZ-X-05` |
| Residual risk | Every call checks the tag of the storage (the magic of the engine, the generation, and the storage's own address) and answers `ALDIS_ERR_INVALID_HANDLE` for uninitialized, deinitialized, copied, or foreign storage, and `ALDIS_ERR_INVALID_FIELD` for a null or misaligned pointer, a flag that is not 0 or 1, an input kind with no constant, or buffers that overlap (`file:crates/aldis-ffi/src/handle.rs`). The size and alignment of the storage come from the C type, so no length crosses the ABI (CABI-031). What no call can see is as in `hz:HZ-X-05`: a wrong non-null pointer or an overstated length, the header and the library of different versions (`asm:ASM-13`), and two threads on one handle (CABI-050, the caller serializes). `init` replaces whatever the storage holds, live or not, because the tag cannot tell a live engine from garbage (CABI-031); a caller that initializes a running engine loses its session. The C ABI has no USLP form of FOP-1 (`file:crates/aldis-ffi/src/fop1.rs`) |
| Assumptions | `asm:ASM-6`; `asm:ASM-13` |
| Proposals | — |
| Claims | `ac:AC-6.2` |

### HZ-COP1-17 A reading of the Blue Book's silence differs from the peer's

| | |
|---|---|
| Category | `hidden-deviation` |
| Status | `residual` |
| Severity | S3 |
| Failure mode | Aldis resolves a point on which 232.1 is silent or ambiguous one way and the peer (a ground system, a flight implementation) another: the start-up values, the effect of a purge on NN(R), the rank of an invalid CLCW pattern, the second Accept of a BD request, Wait_Flag on entering Lockout, Suspend_State after an Alert, the counts of a USLP target. The sessions then differ on a corner and the transfer stalls, locks out, or alerts |
| Cause | The Blue Book's text does not settle the point; each is an open issue resolved by the maintainer |
| Mission effect | A link that does not come up, or a corner case that ends in an Alert or a Lockout where the peer would carry on (S3); not a wrong result on a conforming exchange |
| Requirements | COP-001, COP-002, COP-005, COP-012, COP-014, COP-017, COP-035, API-031 |
| Evidence | `oi:OI-58`; `oi:OI-59`; `oi:OI-60`; `oi:OI-61`; `oi:OI-62`; `oi:OI-64`; `oi:OI-65`; `oi:OI-66`; `file:docs/verification/reviews/cop1-tables.md`; `file:test-vectors/cop1/README.md`; `vector:cop1-fop1-alert-suppression`; `vector:cop1-fop1-s6-e5`; `test:crates/aldis-cop1/src/fop1.rs::no_alert_follows_an_alert_before_an_initiate_ad`; `test:crates/aldis-cop1/src/fop1.rs::a_clcw_that_matches_several_events_gets_the_highest_ranked` |
| Residual risk | Each reading is an open issue with the maintainer's decision, is written where it applies (the module documentation, [cop1.md](../../standards/cop1.md), and the vector README), and is exercised by a vector that the model written blind produces from the text. They are not deviations the engines can detect or report, because the Blue Book does not rule on them; and the question to CCSDS about the E45 Accept of OI-61 stays open. An integrator who meets a peer with another reading changes the engine, not a flag |
| Assumptions | `asm:ASM-8` |
| Proposals | — |
| Claims | `ac:AC-1.8`; `ac:AC-5.1` |

### HZ-COP1-18 A late Set V(R) directive resets V(R) after frames were delivered

| | |
|---|---|
| Category | `corrupt-accepted` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A Type-BC frame with the Set V(R) Control Command (a retransmission or a duplicate of one already executed, or the first copy delayed past its retransmission) reaches FARM-1 in the Open or Wait state after later Type-AD frames were accepted. FARM-1 executes it again and sets V(R) back to V*(R) (232.1 Table 6-1, E8). The Type-AD frames between V*(R) and the old V(R) fall into the negative window, the next CLCW reports an N(R) that moved backwards, and a retransmission of FDUs already delivered is accepted a second time (observed by the fuzz target `cop1_closed_loop`: delivery of N(S) 1 at position 3 with V(R) 2, after V(R) was 3) |
| Cause | A Type-BC frame carries no Frame Sequence Number, so FARM-1 cannot tell a stale Set V(R) from a current one (232.1 §6.3.2.2 describes only the execution), and the Blue Book names no check on a repeated command. COP-1 relies on the link to keep the order of the frames of a Virtual Channel: 232.0 §2.4.1 describes the loss that COP-1 repairs as "errors or discontinuities" of a link that, if perfect, would deliver "the exact duplicate of the series" of data units, and neither 232.0 nor 232.1 mentions reordering or the creation of a frame by the link. The only duplicates 232.1 admits are those the sending end makes, by a FOP-1 retransmission or by the systematic retransmission of 232.1 §5.1.9.3 (note 2 to that clause, which says FARM-1 rejects a duplicate Type-AD frame and names no rule for a Type-BC frame). On an in-order link such a copy is adjacent to its original: FOP-1 sends no Type-AD frame after the directive until a CLCW acknowledges it (232.1 §5.2, Transmit Type-BC Frame), so every copy of the directive is on the link before the first Type-AD frame |
| Mission effect | Telecommands executed twice, or a gap of telecommands after a re-accepted retransmission, with a CLCW that tells FOP-1 the opposite of what FARM-1 delivered: the loss of the Sequence-Controlled Service guarantee without an Alert, on a link that reorders (a relay with several paths, a ground network that routes the two directions differently, a replay) |
| Requirements | COP-002, COP-003, COP-005, COP-040, COP-041 |
| Evidence | `test:crates/aldis-cop1/tests/closed_loop.rs::any_link_gives_the_service_guarantee_or_an_alert`; `test:crates/aldis-cop1/tests/closed_loop.rs::duplicated_frames_and_reports_deliver_once`; `test:crates/aldis-cop1/tests/farm1_table.rs::test_exhaustive_33_cells` |
| Residual risk | The engine executes Table 6-1 as written and cannot do otherwise: a check that refused a repeated Set V(R) would deviate from E8, and the Blue Book gives FARM-1 nothing to tell a repeat from a new command (the V*(R) is the only datum, and a legitimate re-initialization may repeat it). The reading is that the standard assumes a link that keeps the order of the frames of a Virtual Channel and creates no frames (an inference from the quoted text; the Blue Book does not state it, and does not address a stale directive), so the Type-AD guarantee holds under a link that loses, corrupts, and (by the sender) repeats frames, and the Set V(R) directive is outside it on a link that reorders. The Type-AD sequence number still repairs the reordering and duplication of Type-AD frames (`hz:HZ-COP1-01`); only the Type-BC command is exposed. The closed-loop tests and the fuzz target therefore leave the Set V(R) directive out of a re-initiation over a link that reorders, and the Unlock directive stays (its late execution leaves V(R) as it is and clears Lockout and Retransmit). The obligation is on the integrator (`asm:ASM-18`): reordering and duplication by the link are excluded or confined to Type-AD frames. The question to CCSDS, whether a FARM-1 should guard a repeated Set V(R), is not an open issue of Aldis, which has no reading to choose between: the table is unambiguous |
| Assumptions | `asm:ASM-18` |
| Proposals | — |
| Claims | `ac:AC-5.6` |

## Proposals

| ID | Kind | Proposal | Status |
|---|---|---|---|
| P-COP1-01 | `oi` | Draft OI, "The Virtual Channel of a frame that FARM-1 takes": FOP-1 refuses a CLCW of another Virtual Channel with `UnknownChannel` (cop1.md §2.2), but a FARM-1 instance takes frames from any channel, because its input carries no VCID and the engine holds the VCID only for its CLCW. Options: (a) keep, and state the routing duty (`asm:ASM-15`, this branch); (b) add the frame's VCID to `Farm1Frame` (it is in the `ValidatedTcFrame` that `from_tc` reads) and to the C input struct, and refuse a frame of another channel with `UnknownChannel` and no state change, as FOP-1 does for a CLCW. Recommendation (b): the check costs one comparison, it makes the two engines symmetric, and the C ABI may still change before release. A C ABI input shape and an engine API, so the maintainer's (decision-rights §2.4) | `accepted` (maintainer, 2026-10-31), option (b), pending its own pull request that adds the VCID to the FARM-1 input; the residual of `hz:HZ-COP1-04` stays until it lands |
| P-COP1-02 | `oi` | Draft OI, "A decision tied to its engine and store": the `commit` of `Fop1` cannot fail and trusts that the `Fop1Decision` is of this engine in this state and that the `Fop1Store` is its own (`hz:HZ-COP1-14`). Options: (a) keep and document (`asm:ASM-14`, this branch); (b) make the decision borrow the engine and the store shared until `commit`, so that the borrow checker rejects another input between `decide` and `commit` (the signature of `commit` then takes the engine by value or through a token); (c) a generation counter in the engine and the store that `commit` compares, returning an error, which gives up "cannot fail" (API-058). Recommendation (a) until the first integrator uses the split API directly: the C ABI does not expose it, and (b) and (c) change the API of RS-022. A public API convention, so the maintainer's (decision-rights §2.4) | `accepted` (maintainer, 2026-10-31), option (a): kept as the caller obligation of `asm:ASM-14` |
| P-COP1-03 | `doc` | Write the obligations of a COP-1 integrator as `ASM-14` to `ASM-17` of the [assurance case](../assurance-case.md#assumptions-and-context), and extend `ASM-1` to the engine state, so that each is named by a hazard of this file. Wording that changes no behaviour (decision-rights §2.2, the coordinator's) | `accepted` this branch, assurance-case.md ASM-14 to ASM-17 |
