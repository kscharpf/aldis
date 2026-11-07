# Hazards: TM synchronization (`aldis-sync`)

Failure modes of `aldis-sync` (the ASM attach, the pseudo-randomizer, and the ASM search engine), of its C ABI (the two codec functions and the search handle), and of the C++ boundary of the handle (`aldis.hpp`), in the format of [hazards.md](../hazards.md) §1.7. Standard: [tm-sync.md](../../standards/tm-sync.md), CCSDS 131.0-B-6 §9 and §10, for the TM, AOS, and fixed-length USLP frames of one physical channel with no coding or with convolutional coding alone. The failure modes common to all crates (panics, loops, stack, the C ABI, single-event upsets) are in [hazards.md](../hazards.md) §3; a hazard here repeats one only where the sync layer adds a cause of its own. The hazards of the frame codecs ([tm.md](tm.md), [aos.md](aos.md), [uslp.md](uslp.md)) cover what happens to a unit once the engine has returned it.

The engine is sans-I/O (GEN-030): it reads the stream in chunks the caller supplies, keeps the last 31 bits and the partial unit in its own storage, and writes each completed unit into the caller's output area. It does not verify the spacing of the units, hold a lock, derandomize, or look for the complemented marker (SYNC-006, SYNC-019): the caller does, from the `skipped_bits` of each unit. Everything around it is the caller's: the choice of the tolerance, the check of the spacing, the order randomize then attach (and its reverse), the code that finds a bit error in the unit, and the size and copying of the output area. The assumptions are `asm:ASM-4`, `asm:ASM-6`, `asm:ASM-8`, `asm:ASM-13`, and `asm:ASM-20` of the [assurance case](../assurance-case.md#assumptions-and-context).

## Coverage

| Category | Hazards | State |
|---|---|---|
| `corrupt-accepted` | HZ-SYNC-01 | done |
| `valid-rejected` | HZ-SYNC-02 | done |
| `misrouting` | — | n/a: a CADU carries no spacecraft or channel identity, since the marker is the same for every frame, and routing starts in the frame decoders (`hz:HZ-TM-01`, `hz:HZ-AOS-01`, `hz:HZ-USLP-03`); a configuration of another protocol or frame length than the sender's is `asm:ASM-8` |
| `truncation` | HZ-SYNC-03 | done |
| `wrong-value` | HZ-SYNC-04 | done |
| `nonconformant-output` | HZ-SYNC-05, HZ-SYNC-06 | done |
| `unbounded-time` | HZ-SYNC-07 | done |
| `memory` | HZ-SYNC-08 | done |
| `panic` | HZ-SYNC-09 | done |
| `state-corruption` | HZ-SYNC-10 | done |
| `abi-misuse` | HZ-SYNC-11, HZ-SYNC-12 | done |
| `hidden-deviation` | HZ-SYNC-13 | done |

## Hazards

### HZ-SYNC-01 A pattern in the noise is taken for the ASM, and the unit behind it is returned (false lock)

| | |
|---|---|
| Category | `corrupt-accepted` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The search engine matches the last 32 stream bits against the ASM at a bit position where the stream holds no ASM, collects the next frame-length octets as a unit, and returns it as a frame that no sender sent. The octets it swallows may hide a real ASM, so one false lock can also cost the following frame (`hz:HZ-SYNC-02`) |
| Cause | Channel errors or fill that put at most the configured number of bit errors (0 to 15, SYNC-007) against the marker at some bit offset. With a tolerance of e, the fraction of random 32-bit windows within e flips is the sum of C(32, k)/2^32 for k up to e: 2.3·10⁻¹⁰ at 0, 7.7·10⁻⁹ at 1, 1.3·10⁻⁶ at 3, 1.1·10⁻³ at 7, and 0.43 at 15. The first match in stream order wins, so a near miss that comes before a real marker takes the real marker's place (SYNC-008) |
| Mission effect | A corrupted frame reaches the frame decoder: with an FECF configured it is rejected (`hz:HZ-X-06` limits that); without one a wrong frame is delivered, and the frame counts of the receiving engine show a gap or a duplicate |
| Requirements | SYNC-007, SYNC-008, SYNC-009, SYNC-011, SYNC-016, SYNC-017, SYNC-019 |
| Evidence | `vector:sync-search-tol-00`; `vector:sync-search-tol-01`; `vector:sync-search-tolerance-counts-differing-bits`; `vector:sync-search-exact-marker-high-tolerance`; `vector:sync-first-ones-tolerance-13`; `vector:sync-first-ones-tolerance-12`; `vector:sync-first-near-miss-shift1-v0`; `vector:sync-units-marker-needs-32-bits`; `vector:sync-units-asm-inside-unit`; `vector:sync-units-complemented-marker`; `test:crates/aldis-sync/src/config.rs::tolerance_is_0_to_15`; `test:crates/aldis-sync/tests/properties.rs::the_engine_agrees_with_a_naive_search_for_any_chunking`; `fuzz:sync_search` |
| Residual risk | The engine cannot tell noise from a marker, and 131.0 gives it no lock rule (§9.2 note; `oi:OI-81`, decided: no spacing check, no lock). It reports `errors` and `skipped_bits` of every unit (SYNC-011) and nothing else: a caller that accepts a unit whose `skipped_bits` is not 0 after the first unit, or that sets a tolerance above 0 on a clean channel, accepts false locks at the rates above. The unit has no integrity check at this layer: a bit error inside it reaches the frame decoder, which finds it only with the FECF (`asm:ASM-4`; `hz:HZ-X-06` for what the CRC misses). A tolerance of 15 is within the range the maintainer decided (`oi:OI-80`) and matches nearly half of all windows, so the engine returns a unit at almost every position; the tolerance is the integrator's choice, and there is no default |
| Assumptions | `asm:ASM-4`; `asm:ASM-20` |
| Proposals | — |
| Claims | `ac:AC-1.10`; `ac:AC-5.5` |

### HZ-SYNC-02 A frame that was sent is not returned (missed ASM)

| | |
|---|---|
| Category | `valid-rejected` |
| Status | `residual` |
| Severity | S3 |
| Failure mode | The search engine returns no unit for a CADU that is in the stream: the frame is lost, and only the `skipped_bits` of the next unit, the frame counts of the receiving engine, or the counters of the status show it |
| Cause | More bit errors in the ASM than the tolerance; a false lock whose unit covers the real ASM (`hz:HZ-SYNC-01`); a defect in the window across a chunk boundary or at a bit offset; the caller's Reset between the ASM and the end of its unit, which discards the partial unit (SYNC-015) |
| Mission effect | A frame of telemetry is missing. For a counted service the frame count shows a gap and the receiving engine reports it (`hz:HZ-LINK-07`); for a stream without counts the loss is silent |
| Requirements | SYNC-007, SYNC-009, SYNC-010, SYNC-011, SYNC-015, SYNC-017 |
| Evidence | `vector:sync-search-single-error-every-position`; `vector:sync-search-tol-15`; `vector:sync-search-tm-skip-003`; `vector:sync-search-aos-skip-001`; `vector:sync-search-uslp-skip-007`; `vector:sync-chunk-o3-k05`; `vector:sync-chunk-bytewise-o5`; `vector:sync-chunk-split-asm-three-chunks`; `vector:sync-units-back-to-back`; `vector:sync-units-gaps`; `vector:sync-units-tail-of-unit-not-reused`; `test:crates/aldis-sync/tests/properties.rs::every_bit_offset_of_a_marker_is_found`; `test:crates/aldis-sync/tests/properties.rs::a_stream_of_cadus_gives_every_frame_at_any_chunking`; `test:crates/aldis-sync/tests/properties.rs::attach_randomize_derandomize_search_round_trips`; `fuzz:sync_search` |
| Residual risk | A marker with more differing bits than the tolerance is not found: that is the tolerance's meaning, and 131.0 states none (`oi:OI-80`). At tolerance 0 one bit error in the 32 bits of the ASM loses the frame; a higher tolerance raises the false-lock rate of `hz:HZ-SYNC-01`. A false lock swallows the frame length of octets without searching them (SYNC-009), so a real ASM inside is missed; the engine resumes with an empty window after the unit. The loss shows in `skipped_bits` and in the frame counts only if the caller reads them (`asm:ASM-20`) |
| Assumptions | `asm:ASM-20` |
| Proposals | — |
| Claims | `ac:AC-1.10` |

### HZ-SYNC-03 A unit is shorter or longer than the frame, or is cut where the stream is divided

| | |
|---|---|
| Category | `truncation` |
| Status | `controlled` |
| Severity | S1 |
| Failure mode | The search engine returns a unit of other than the configured frame length, with its bits shifted wrongly against the octet boundary, or loses or repeats bits where an ASM or a unit crosses the boundary between two chunks |
| Cause | A defect in the bit-by-bit window, in the shift of the unit to an octet boundary, or in the carrying of the partial unit from one chunk to the next |
| Mission effect | A frame with missing, extra, or shifted octets reaches the frame decoder: the FECF rejects most of them, and a shifted frame has the wrong headers |
| Requirements | SYNC-009, SYNC-010, SYNC-011, SYNC-014, SYNC-017 |
| Evidence | `vector:sync-units-length-1`; `vector:sync-units-length-2`; `vector:sync-units-length-5`; `vector:sync-units-length-64`; `vector:sync-units-partial-at-end`; `vector:sync-chunk-o0-k01`; `vector:sync-chunk-o7-k09`; `vector:sync-chunk-many-units-misaligned`; `test:crates/aldis-sync/tests/properties.rs::a_stream_of_cadus_gives_every_frame_at_any_chunking`; `test:crates/aldis-sync/tests/properties.rs::attach_randomize_derandomize_search_round_trips`; `test:crates/aldis-sync/tests/engine.rs::the_worst_case_reaches_the_action_and_area_bounds`; `fuzz:sync_search` |
| Residual risk | — |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-1.10`; `ac:AC-4.3` |

### HZ-SYNC-04 A skipped-bit count, an error count, or a counter is wrong

| | |
|---|---|
| Category | `wrong-value` |
| Status | `controlled` |
| Severity | S1 |
| Failure mode | A unit is returned with a `skipped_bits` or `errors` that differs from the stream, or the status reports a wrong number of units, skipped bits, or octets of the partial unit. A caller that checks the spacing of the units from `skipped_bits` then accepts a false lock or rejects a true one |
| Cause | A defect in the count of the bits read since the search began, in the 32-bit wait after a Reset or a returned unit, or in the saturation of a counter |
| Mission effect | The caller's own lock or spacing logic decides on a wrong number (`hz:HZ-SYNC-01`, `hz:HZ-SYNC-02`) |
| Requirements | SYNC-011, SYNC-015, SYNC-019 |
| Evidence | `vector:sync-units-gaps`; `vector:sync-units-reset-keeps-counters`; `vector:sync-units-total-after-reset`; `vector:sync-units-partial-octets-rounds-down`; `vector:sync-units-reset-window`; `vector:sync-search-tol-05`; `vector:sync-chunk-unit-state-queries`; `test:crates/aldis-sync/src/search.rs::the_read_count_and_the_skipped_bits_saturate`; `test:crates/aldis-sync/tests/engine.rs::reset_keeps_the_counters_and_takes_a_new_configuration`; `test:crates/aldis-sync/tests/properties.rs::the_engine_agrees_with_a_naive_search_for_any_chunking`; `fuzz:sync_search` |
| Residual risk | — |
| Assumptions | `asm:ASM-20` |
| Proposals | — |
| Claims | `ac:AC-1.10` |

### HZ-SYNC-05 The ASM attach writes a CADU that does not conform

| | |
|---|---|
| Category | `nonconformant-output` |
| Status | `controlled` |
| Severity | S1 |
| Failure mode | The attach function writes a marker other than `1ACFFC1D`, writes it after the frame or in another bit order, changes the frame, or writes into a part of the output area outside the CADU |
| Cause | A defect in the marker constant or in the copy; a caller that attaches to a frame of another length than the configured one (refused, SYNC-003) |
| Mission effect | Every frame of the link is lost to a receiver that searches for the right marker |
| Requirements | SYNC-001, SYNC-002, SYNC-003, SYNC-006 |
| Evidence | `vector:sync-cadu-tm-1`; `vector:sync-cadu-tm-2048`; `vector:sync-cadu-aos-65536`; `vector:sync-cadu-uslp-65535`; `vector:sync-cadu-tm-frame-holds-asm`; `vector:sync-cadu-tm-short-frame`; `vector:sync-cadu-out-one-short`; `vector:sync-cadu-both-faults`; `test:crates/aldis-sync/src/asm.rs::the_marker_is_1acffc1d`; `test:crates/aldis-sync/src/asm.rs::attach_writes_the_marker_then_the_frame`; `test:crates/aldis-sync/src/asm.rs::attach_errors_write_nothing`; `test:crates/aldis-sync/tests/codec_vectors.rs::cadu_tm`; `test:crates/aldis-sync/tests/codec_vectors.rs::cadu_aos`; `test:crates/aldis-sync/tests/codec_vectors.rs::cadu_uslp`; `test:crates/aldis-sync/tests/codec_vectors.rs::cadu_errors` |
| Residual risk | — |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-1.10`; `ac:AC-4.3` |

### HZ-SYNC-06 The randomizer's sequence is wrong, or a unit is randomized or derandomized out of alignment

| | |
|---|---|
| Category | `nonconformant-output` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | The randomizer exclusive-ORs a buffer with bits that are not those of the selected sequence: a wrong polynomial or seed, a sequence that does not restart at the first octet of a call, a period that does not wrap at 131071 (or 255) bits inside a long buffer, or the legacy sequence where the standard one was meant. A frame is then randomized by one sequence and derandomized by another, or at another position |
| Cause | A defect in the register or the recurrence; the caller's choice of the sequence; the caller's order of operations: randomizing a CADU that includes the ASM, attaching the ASM before randomizing, derandomizing a buffer that does not start at the first octet of the unit, or randomizing a frame in pieces without restarting |
| Mission effect | The receiver derandomizes into octets that are not the frame: the FECF rejects it, or, with no FECF, a frame of wrong content reaches the decoder, which checks only its fields |
| Requirements | SYNC-004, SYNC-005, SYNC-006, SYNC-009, SYNC-020 |
| Evidence | `vector:sync-rand-131071-first-40-bits`; `vector:sync-rand-255-first-40-bits`; `vector:sync-rand-131071-zeros-16385`; `vector:sync-rand-131071-zeros-65536`; `vector:sync-rand-255-zeros-256`; `vector:sync-rand-131071-involution`; `vector:sync-rand-131071-ones`; `vector:sync-rand-sequence-invalid-0`; `test:crates/aldis-sync/tests/randomizer.rs::the_sequences_repeat_after_their_period_and_not_before`; `test:crates/aldis-sync/tests/randomizer.rs::a_buffer_of_four_periods_wraps_at_a_bit_that_is_not_an_octet_boundary`; `test:crates/aldis-sync/src/randomizer.rs::applying_it_twice_restores_the_buffer`; `test:crates/aldis-sync/src/randomizer.rs::a_call_depends_on_its_buffer_alone`; `test:crates/aldis-sync/src/randomizer.rs::the_choice_is_a_period_in_bits`; `test:crates/aldis-sync/tests/properties.rs::attach_randomize_derandomize_search_round_trips`; `fuzz:sync_search`; `fuzz:sync_ffi` |
| Residual risk | The sequence is checked against the 40 bits the Blue Book prints and against the blind model over a whole period and a wrap (`df:DF-25` for the missing second implementation). The codec does not combine the randomizer with the attach (SYNC-006), so the order randomize-then-attach on sending and find-then-derandomize on receiving is the caller's, and a caller that gets it wrong is not told: the call cannot know which octets it was given. The randomizer fails for no input and cannot flag a buffer that is not a unit |
| Assumptions | `asm:ASM-20`; `asm:ASM-8` |
| Proposals | — |
| Claims | `ac:AC-1.10` |

### HZ-SYNC-07 A sync call takes unbounded time

| | |
|---|---|
| Category | `unbounded-time` |
| Status | `controlled` |
| Severity | S2 |
| Failure mode | A search step, an attach, or a randomize call runs for longer than its input and capacities allow, or an action list overflows its bound |
| Cause | A loop whose bound the stream can change; a chunk larger than the capacity the instance was built for; a unit count above the action bound |
| Mission effect | The calling task misses its deadline or the watchdog acts |
| Requirements | SYNC-013, SYNC-014, GEN-022, GEN-023 |
| Evidence | `ci:loops`; `test:crates/aldis-sync/src/search.rs::the_bounds_follow_the_requirements`; `test:crates/aldis-sync/tests/engine.rs::the_worst_case_reaches_the_action_and_area_bounds`; `vector:sync-chunk-many-units-bound`; `vector:sync-capacity-chunk-too-large`; `fuzz:sync_search`; `record:GEN-022` |
| Residual risk | — |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-3.1` |

### HZ-SYNC-08 A sync call writes or reads outside its buffers, or overruns the stack

| | |
|---|---|
| Category | `memory` |
| Status | `controlled` |
| Severity | S1 |
| Failure mode | A search step writes a unit outside the output area or the partial-unit storage, reads outside the chunk, or a handle built at the full frame capacity overruns the caller's stack |
| Cause | A defect in the unit range or the storage index; an output area smaller than the units the chunk completes; a frame length above the capacity of the instance |
| Mission effect | Memory outside the buffers is overwritten, or the task stops (`hz:HZ-X-04` for the stack) |
| Requirements | SYNC-002, SYNC-013, SYNC-014, GEN-011, GEN-014, GEN-023, CABI-030, CABI-040 |
| Evidence | `vector:sync-bound-buffer-too-small`; `vector:sync-bound-needed-counts-completed-units`; `vector:sync-bound-error-then-same-chunk`; `vector:sync-capacity-chunk-too-large`; `vector:sync-config-capacity-exceeded`; `test:crates/aldis-sync/tests/engine.rs::a_refused_input_changes_nothing`; `test:crates/aldis-sync/tests/engine.rs::the_instance_refuses_a_frame_length_above_its_capacity`; `test:crates/aldis-ffi/tests/sync.rs::a_frame_of_the_largest_length_goes_through_attach_randomize_and_search`; `test:crates/aldis-ffi/tests/sync.rs::a_step_checks_its_arguments_and_the_chunk_capacity`; `check:stack`; `record:VER-011`; `sym:aldis_sync_search_init`; `sym:aldis_sync_search_step`; `file:crates/aldis-ffi/capacities.toml`; `fuzz:sync_search`; `fuzz:sync_ffi` |
| Residual risk | — |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-4.2`; `ac:AC-4.3`; `ac:AC-4.4` |

### HZ-SYNC-09 A sync call panics

| | |
|---|---|
| Category | `panic` |
| Status | `residual` |
| Severity | S2 |
| Failure mode | A public function of `aldis-sync` or of its C ABI panics on some configuration, stream, chunk division, or output area |
| Cause | An unchecked index, shift, or arithmetic in the window or the unit store; an error return that the code treats as impossible and that an input can reach |
| Mission effect | The calling task stops (`hz:HZ-X-02`) |
| Requirements | SYNC-018, GEN-020, GEN-021, VER-010, VER-022 |
| Evidence | `record:VER-010`; `record:VER-022`; `check:exerciser-thumbv7em-none-eabihf`; `check:exerciser-armv7r-none-eabihf`; `check:exerciser-riscv32imac-unknown-none-elf`; `check:exerciser-riscv64gc-unknown-none-elf`; `file:exerciser/src/sync.rs`; `check:clippy`; `check:coverage`; `file:coverage-waivers.toml`; `fuzz:sync_search`; `fuzz:sync_ffi` |
| Residual risk | As `hz:HZ-X-02`: the link check covers the exerciser's instantiations (the 65536-octet frame and 256-octet chunks of the C ABI, small capacities through Rust), not an integrator's other capacities. The coverage waivers of the crate (4 regions, 3 lines, and 3 branches, all in the search step and its unit store) are explicit error returns that no input reaches: the unit range of an output area that was checked against the units the chunk completes, the action list whose bound is the engine's own, and a store index below the frame length. Each returns an error and not a default; a defect there would be an internal error, and the fuzz targets assert there is none |
| Assumptions | — |
| Proposals | — |
| Claims | `ac:AC-2.1`; `ac:AC-2.3` |

### HZ-SYNC-10 A rejected input, or a reset, leaves the engine in a state it should not hold

| | |
|---|---|
| Category | `state-corruption` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A refused chunk (an output area that is too small, a chunk above the capacity) changes the window, the partial unit, or the counters, so that the next accepted chunk gives units that depend on the refused one; or a Reset keeps a stale window or loses the counters; or a single-event upset changes the state held between calls |
| Cause | A step that updates the state before it has checked the output area; a Reset that clears the wrong fields; an upset in memory the platform does not protect |
| Mission effect | Frames lost or invented after an error the caller thought harmless |
| Requirements | SYNC-012, SYNC-014, SYNC-015, API-058, GEN-042, GEN-015 |
| Evidence | `vector:sync-bound-error-then-same-chunk`; `vector:sync-units-reset-partial`; `vector:sync-units-reset-window`; `vector:sync-capacity-chunk-too-large`; `test:crates/aldis-sync/tests/engine.rs::a_refused_input_changes_nothing`; `test:crates/aldis-sync/tests/engine.rs::reset_keeps_the_counters_and_takes_a_new_configuration`; `fuzz:sync_search`; `fuzz:sync_ffi`; `hz:HZ-X-01` |
| Residual risk | A refused input changes nothing, and the fuzz targets compare the engine before and after every refusal. An upset of the 31-bit window, the count of bits read, or the partial unit is not detected by the engine; it gives a wrong unit or a lost frame, which the frame check and the counts of the receiving engine show, as in `hz:HZ-X-01`. For the C handle the state is in the caller's storage (`hz:HZ-SYNC-11`) |
| Assumptions | `asm:ASM-1` |
| Proposals | — |
| Claims | `ac:AC-5.4`; `ac:AC-4.4` |

### HZ-SYNC-11 A C caller misuses the sync functions or the search handle

| | |
|---|---|
| Category | `abi-misuse` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A C caller passes a null or misaligned pointer, storage that was never initialised or that is a copy of live storage, a buffer that overlaps the storage or the output area, a choice of frame protocol or sequence that is none of the constants, a configuration the engine refuses, or a length that contradicts its pointer, and the library does anything but return an error |
| Cause | A defect in the caller |
| Mission effect | Undefined behaviour in the caller's process (`hz:HZ-X-05`) |
| Requirements | CABI-022, CABI-031, CABI-032, CABI-040, SYNC-002, SYNC-016, SYNC-020, GEN-071 |
| Evidence | `test:crates/aldis-ffi/tests/sync.rs::the_storage_is_checked_and_tagged`; `test:crates/aldis-ffi/tests/sync.rs::the_choices_and_the_configuration_are_checked_in_order`; `test:crates/aldis-ffi/tests/sync.rs::the_codec_functions_check_their_pointers`; `test:crates/aldis-ffi/tests/sync.rs::a_step_checks_its_arguments_and_the_chunk_capacity`; `test:crates/aldis-ffi/tests/sync.rs::capacities_are_published_and_match_the_engine`; `vector:sync-config-protocol-invalid`; `vector:sync-rand-sequence-invalid-255`; `vector:sync-config-two-faults-protocol-and-frame`; `check:header`; `check:c-sanitizers`; `check:ffi-miri`; `fuzz:sync_ffi`; `sym:aldis_sync_attach`; `sym:aldis_sync_randomize`; `sym:aldis_sync_search_step` |
| Residual risk | As `hz:HZ-X-05` and `hz:HZ-LINK-14`: a null pointer, a misaligned address, a wrong tag, overlapping buffers, and an inconsistent length are errors; a wrong non-null pointer or an overstated length is undefined behaviour the library cannot detect. The storage is the caller's: the engine is built in it and runs there, so a caller that moves or copies the storage while the engine lives breaks the engine, and the tag, which holds the storage's own address, makes a copy a foreign handle. The Miri run is the gating check `ffi-miri`, when its inputs change |
| Assumptions | `asm:ASM-6`; `asm:ASM-13` |
| Proposals | — |
| Claims | `ac:AC-6.1`; `ac:AC-6.2` |

### HZ-SYNC-12 The C++ wrapper gives the library storage of the wrong size or alignment

| | |
|---|---|
| Category | `abi-misuse` |
| Status | `residual` |
| Severity | S1 |
| Failure mode | A C++ caller of the `aldis.hpp` search handle builds the engine in storage that is smaller than the library's or not aligned for it, or with a frame or chunk capacity that differs from the library's, so that the library writes past the storage or reads a partial unit of another size than it has |
| Cause | A handle type instantiated with capacities other than those the library was built with; a header and a library of different versions; a copy or move of a live handle; a wrapper that restates a size by hand and lets it drift |
| Mission effect | Memory of the caller's process overwritten by the engine, or an engine that rejects or misreads every call |
| Requirements | CABI-072, CABI-030, CABI-031, CABI-060, CABI-021 |
| Evidence | `check:header`; `file:include/aldis/aldis.hpp`; `file:crates/aldis-ffi/tests/cpp/capacity_mismatch.cpp`; `file:crates/aldis-ffi/tests/cpp/engines.cpp`; `file:crates/aldis-ffi/tests/cpp/sync.hpp`; `test:crates/aldis-ffi/tests/sync.rs::capacities_are_published_and_match_the_engine`; `test:crates/aldis-ffi/tests/sync.rs::the_storage_is_checked_and_tagged`; `sym:aldis_abi_version`; `sym:aldis_sync_search_init` |
| Residual risk | The wrapper adds no size of its own: its storage member is the C struct of the header, so size and alignment come from the header, a test shows that the struct has the library's size and the alignment of 8 octets, and the C++ test of the sync handle asserts that the wrapper holds at least one such struct. A capacity template argument that differs from the library's constant does not compile, and the `header` check compiles a file that must fail for that reason and only that. Left: a header from another version of the library than the one linked is not detected at build time (CABI-060; `asm:ASM-13` obliges the integrator to compare the versions at start-up), and an object compiled against a different header than the library's build has another layout that no call can detect; the library checks the alignment of the pointer and the tag, not the size of the storage behind it, which only the type fixes. A handle cannot be copied or moved in C++, and a bitwise copy by other means is refused by the tag |
| Assumptions | `asm:ASM-13`; `asm:ASM-6` |
| Proposals | — |
| Claims | `ac:AC-6.1`; `ac:AC-6.2` |

### HZ-SYNC-13 A reading of the Blue Book's silence differs from the peer's

| | |
|---|---|
| Category | `hidden-deviation` |
| Status | `residual` |
| Severity | S4 |
| Failure mode | Where 131.0 is silent or leaves a choice, the sync layer does something a peer does differently: the tolerance, the first-match rule, no spacing check or lock, no search for the complemented marker, the second (legacy) randomizer sequence, and the counting rules of `skipped_bits` and of the partial unit |
| Cause | Decisions the standard does not settle, taken by the maintainer (`oi:OI-80`, `oi:OI-81`, `oi:OI-82`, `oi:OI-83`) and implemented by the engine |
| Mission effect | A receiver that expects a lock, a different tolerance, or a search for the complemented marker loses frames or accepts others than the engine does; no data is lost on the library's side |
| Requirements | SYNC-007, SYNC-008, SYNC-011, SYNC-015, SYNC-019, SYNC-020 |
| Evidence | `oi:OI-80`; `oi:OI-81`; `oi:OI-82`; `oi:OI-83`; `file:docs/standards/tm-sync.md`; `vector:sync-units-complemented-marker`; `vector:sync-first-ones-tolerance-13`; `vector:sync-units-partial-octets-rounds-down`; `vector:sync-units-total-after-reset` |
| Residual risk | Each open reading is an OI with the maintainer's decision, and the engine's behaviour is a vector named for it. No independent implementation has cross-checked the sync layer (`df:DF-25`); a mission that exchanges CADUs with a peer reads the OIs as part of its interface agreement (`asm:ASM-8`) |
| Assumptions | `asm:ASM-8` |
| Proposals | — |
| Claims | `ac:AC-1.10` |

## Proposals

| ID | Kind | Proposal | Status |
|---|---|---|---|
