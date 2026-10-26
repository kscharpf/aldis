# Independent cross-checks (VER-040)

The `crosscheck/` workspace runs the vector files under `test-vectors/` through independent, existing implementations and reports every disagreement (M1 D5, WP-4; M2 D15, WP-6). Each discrepancy is resolved against the Blue Book text and recorded in §3. This file also lists, per standard, what has no independent implementation and what was searched (§4).

Two implementations are run: `spacepackets` (Rust) against the vectors only, and NASA cFE's `MSG` module (C) against the vectors and against `aldis-spp` on the same packets, including random ones.

## 1. Implementations

### `spacepackets` 0.18.0 (Rust, crates.io)

Evaluated 2026-10-19 for SPP, CUC, and CDS, and 2026-10-20 for the P-field and ASCII formatting.

| Check | Result |
|---|---|
| Independence | Developed at the University of Stuttgart's Institute of Space Systems; shares no code or authors with Aldis |
| License | Apache-2.0 (with a `NOTICE` file, which matters only on redistribution; `crosscheck/` is never shipped). All 31 crates in its dependency tree carry licenses on the `deny.toml` allow list |
| MSRV | 1.85, below the pinned 1.99.1 (RS-001) |
| `cargo deny check` (cargo-deny 0.20.2, root policy) | licenses, bans, sources: ok (bans warns about two `syn` major versions, which the policy allows). Advisories: fails on RUSTSEC-2024-0436 |
| Version | Pinned with `=0.18.0` so that a new release cannot change the results unnoticed. An upgrade re-runs this evaluation |
| ASCII formatting | `spacepackets::time::ascii` supplies the strftime patterns of codes A and B (`%FT%T%.3f` and `%Y-%jT%T%.3f`, each also with a final `Z`), and `chrono` 0.4.45 (MIT OR Apache-2.0), which `spacepackets`' default `std` feature enables, renders them. `crosscheck/` names `chrono` as a direct dependency only to build the dates these functions take; it adds no crate to the tree |

**Exception RUSTSEC-2024-0436** (maintainer decision 2026-10-19). The advisory reports that `paste` 1.0.15, a required dependency of `spacepackets`, is unmaintained. It reports no vulnerability. `paste` is a build-time macro helper, and `crosscheck/` is a dev-only workspace that is never shipped. The exception therefore lives in `crosscheck/deny.toml` only, which repeats the root policy and adds this one `ignore`. The root `deny.toml`, which governs the shipped crates, is unchanged. The `deny` check of `cargo xtask verify` (and of the nightly CI) checks `crosscheck/` with its own file.

### NASA cFE `MSG` v7.0.1 (C, GitHub `nasa/cFE`)

Evaluated and added 2026-10-21 for the Space Packet primary header and cFE's telemetry time (maintainer's choice; M2 D15, OI-32).

| Check | Result |
|---|---|
| Independence | NASA Goddard's core Flight Executive, the framework of the core Flight System. Written independently of Aldis; shares no code, authors, or ancestry with it, nor with `spacepackets` |
| License | Apache-2.0: the repository's `LICENSE`, and the header of every file used (NASA Docket No. GSC-19,200-1, "cFS Draco", copyright the United States Government as represented by the NASA Administrator). The repository has no `NOTICE` file. Running it as a test oracle is permitted and costs nothing (VER-055). Vendoring is redistribution: Apache-2.0 §4 asks for a copy of the license, which `crosscheck/vendor/cfe/LICENSE` is, and notice of changes, of which there are none |
| Version | Release "cFS - v7.0.1" of 2026-05-14, the latest (`gh release list -R nasa/cFE`); tag `v7.0.1`, a lightweight tag on commit `c5fb2b4d540bd55eb6c3707da7dd13eee679d4dd` (`git ls-remote --tags https://github.com/nasa/cFE.git`). The archive `https://github.com/nasa/cFE/archive/c5fb2b4d540bd55eb6c3707da7dd13eee679d4dd.tar.gz` had SHA-256 `8eab93d9bb09854605487665d44f1b5f446f07641ec5f7fd954d19ec3cdbb2f4` on 2026-10-21, and every vendored file matches its copy in that archive. An upgrade re-runs this evaluation |
| Files used | Unmodified, at their repository paths under `crosscheck/vendor/cfe/`, each with its SHA-256 in `SHA256SUMS` (check with `shasum -a 256 -c SHA256SUMS` in that directory, or against a checkout of the tag): `modules/msg/fsw/src/` `cfe_msg_ccsdspri.c`, `cfe_msg_init.c`, `cfe_msg_initdefaulthdr_pri.c`, `cfe_msg_msgid_v1.c`, `cfe_msg_sechdr_time.c`, `cfe_msg_priv.h`, `cfe_msg_defaults.h`; `modules/msg/fsw/inc/ccsds_hdr.h`; `modules/msg/option_inc/` `default_cfe_msg_hdr_pri.h`, `default_cfe_msg_sechdr.h`; `modules/core_api/fsw/inc/` `cfe_msg.h`, `cfe_msg_api_typedefs.h`, `cfe_error.h`. That is the selection of `modules/msg/CMakeLists.txt` without `MISSION_INCLUDE_CCSDSEXT_HEADER`, less the files the harness does not call (integrity actions, function code, checksum, the shared message-identifier helper, the object table) |
| Stubs | `crosscheck/cfe-stubs/`, Aldis's own code (MIT OR Apache-2.0), each file marked as a stub: OSAL's `common_types.h` (the `<stdint.h>` types) and `osapi.h`; the configuration wrappers `cfe_msg_hdr.h` and `cfe_msg_sechdr.h`, which cFE's build generates (`modules/msg/mission_build.cmake`) and which include the default layouts; empty `cfe_mission_cfg.h` and `cfe_platform_cfg.h`, so that `cfe_msg_defaults.h` applies its defaults (Packet Version Number 0, default APID 0); an empty `cfe_es_api_typedefs.h`; `cfe_sb_api_typedefs.h` and `cfe_sb.h` with the message identifier and the three Software Bus helpers that `CFE_MSG_Init` and `CFE_MSG_SetMsgId` call, as cFE defines them without EDS; `cfe_time_api_typedefs.h` and `cfe_time.h` with `CFE_TIME_SysTime_t`. `abi_check.c` fails the build if a type size or a status code that the Rust declarations assume differs from the vendored headers |
| Build | `crosscheck/build.rs` compiles the five sources and `abi_check.c` with the `cc` crate (1.9.0, MIT OR Apache-2.0; with its dependencies `shlex` and `find-msvc-tools` it was already in `crosscheck/Cargo.lock`, as a build dependency of `chrono`'s `iana-time-zone-haiku`, which builds only for Haiku) as C11, with `-Wall -Wextra` and warnings as errors, into a static library. The host C compiler is used: in the image of `cargo xtask verify`, `cc` is the pinned GCC 14 (`cpp/toolchains.lock`); on macOS, Apple clang |
| C code rules | Maintainer decision 2026-10-21: `abi_check.c` and the stub headers are C test code written here, so they follow SEI CERT C (VER-051). The vendored cFE files are excluded: they are third-party code kept unmodified, so a finding in them could only be logged, never fixed. Checked 2026-10-21 with clang-tidy 19.1.7 (`-checks='-*,cert-*,bugprone-*'`, header filter `cfe-stubs/`) on `abi_check.c` with the include path of `build.rs`: no finding in the stubs. Until VER-051's analyser runs in `cargo xtask verify`, that command is the check; `build.rs` also compiles with warnings as errors |
| Bindings | `crosscheck/src/cfe_msg.rs`, the only module of `crosscheck/` that may use `unsafe` (maintainer decision 2026-10-21, over a C subprocess driver; `unsafe_code` is denied for the crate and expected for that module; GEN-071, VER-002, RS-017). Each `unsafe` block states why it is sound: every call gets a buffer of at least `sizeof(CFE_MSG_TelemetryHeader_t)`, 16 octets, because cFE reads and writes a fixed number of octets without knowing the buffer's length |

**How the source reaches the gate.** The `spec` check of `cargo xtask verify` runs `crosscheck` inside the image, so the cFE source must be there without a network fetch at check time. Of the two options, the sources are vendored (option b; maintainer decision 2026-10-21, with the `cc` build dependency) rather than downloaded into the image by the `Containerfile` (option a), because:

- they are 13 C files of about 3 400 lines in all, and the license, which the pull request shows and review can read, against an archive of the whole cFE repository;
- the check is the same everywhere: in the image, in the nightly CI, which runs `crosscheck` outside the image, and by hand, with no download, no environment variable, and nothing to keep in step;
- the image is unchanged, so no new image is built and pushed for it, and the C++ track's image pins stay where they are.

The cost is third-party C code in the repository, kept unmodified under its own license. `ALDIS_CFE_SRC` may name a cFE checkout to compile from instead of `vendor/cfe/`, to try another cFE release before re-vendoring. If a file is missing, from either place, the build of `crosscheck` fails, so the gate never skips the check for want of its source.

## 2. Coverage

### `spacepackets` 0.18.0

What `spacepackets` 0.18.0 can check, by vector file. The harness (`cargo run --manifest-path crosscheck/Cargo.toml`, a step of the `spec` check of `cargo xtask verify` and of the nightly CI, after the harness's own tests) checks only the cases in scope, prints every case it skips with the reason, and fails on any disagreement. A check compares only the values `spacepackets` produces; the expected values it cannot produce are listed under "Out of scope". A key such as `format.day_bits` names the member `day_bits` of the expected object `format`.

A case on which `spacepackets` differs from the vector exactly as a discrepancy of §3 describes is reported as **known**, with the entry's identifier, and does not fail the run. Each check tests the precise condition of its entry. For D1 it tests both sides: the harness computes the value `spacepackets`' rule gives and the value the Blue Book gives, and reports the known discrepancy only when `spacepackets` produced the first and the vector holds the second. D4 is resolved (OI-40) and no longer reported, so any difference in `cuc_from_ns` is a disagreement. Any other difference is a disagreement. The harness's tests (`crosscheck/src/time.rs`) show that, under each entry, a vector with a wrong expected value still disagrees.

`spacepackets` has one decoding mode, so a lenient case is checked like a strict one where its expectation does not depend on the mode.

| Vector files | Checked | Out of scope, and why |
|---|---|---|
| `spp/decode_valid`, `spp/decode_invalid` (`decode`) | `CcsdsPacketReader`: `packet_type`, `secondary_header_flag`, `apid`, `sequence_flags`, `sequence_count`, `packet_len`, `consumed`, `data_len`, `data_hex`. A truncated packet must be rejected as too short (`Incomplete`) | `idle`, `packet_name`, `anomalies`. `UnsupportedVersion`: it does not check the Packet Version Number. The idle rule of SPP-004 |
| `peek_length` cases | `SpHeader::from_be_bytes`: `len`; fewer than 6 octets rejected | `UnsupportedVersion` |
| `spp/stream` | Packet by packet: `apid`, `sequence_count`, `consumed` of each, `stop_error` (none or `Incomplete`), `offset` | `anomalies`; streams that stop at an error it does not detect |
| `spp/encode`, `spp/octet_string` | `CcsdsPacketCreator` builds the packet from the parameters; `output_hex` must match. A data field above 65 536 octets must be rejected | `next_count`. The rejection rules it does not have: empty data field, idle APID, `max_packet_len`. Its creator underflows on an empty data field (§3), so that input is never passed to it |
| `spp/idle` | The expected octets decode to APID 2047, Secondary Header Flag 0, Sequence Flags `11`, the given count and total length (133.0 §4.1.3.3.4.4, §4.1.3.3.3.4; the Sequence Flags by SPP-010, as 133.0 sets none for Idle Packets) | The idle pattern; rejections (no Idle Packet builder) |
| `spp/segment` | Each expected packet decodes to the given type, APID, Secondary Header Flag, and count + index modulo 2^14, and its length field covers exactly its octets | Sequence Flags and the split of the SDU (no segmentation logic); `next_count`; rejections |
| `spp/reassemble`, `spp/sequence_tracker`, `spp/split` | — | No reassembler, sequence tracker, or managed secondary header layout |
| `spp/secondary_header_time` (`split_time`) | The Time Code Field. `CcsdsPacketReader` gives the Packet Data Field, the CUC or CDS reader of the configured code reads the time code at its start, and `time_code_hex` must equal the octets it read. With an implicit P-field, the P-field `spacepackets` writes for the configured format is put in front (301.0 §3.1.1 allows either form of the same T-field). A time code that does not fit the data field must be rejected as too short (`LengthMismatch`), and an explicit P-field of another code or day length as an invalid time code (`InvalidField`) | `secondary_header_hex`, `user_data_hex`, `ancillary_hex`: the split and the ancillary field, including a mismatch that lies only in the ancillary field. The Secondary Header Flag rule. CCS layouts; Level 2; 5–7 coarse octets. CDS range errors. Known: D2, D3 |
| `time/pfield` | `ccsds_time_code_from_p_field` selects the decoder by the time code ID; then as for `time/cuc` and `time/cds`. The reserved IDs `000`, `011`, `111` (301.0 §3.1.1) must be rejected (`ReservedValue`), and a lone CDS P-field octet as too short (`Incomplete`) | The agency-defined code (ID `110`, §3.6); a reserved ID in lenient mode; an encoding with P-field extension octets; an empty input. Known: D3 |
| `time/cuc` (`decode`, `decode_implicit`, `encode`) | `CucTime`: `code`, `pfield_hex`, `format.epoch`, `format.coarse_octets`, `format.fine_octets`, `value.coarse`, `value.fine_hex`, `consumed`; with an implicit P-field (put in front as above) only `value.*` and `consumed`. `CucTime::write_to_bytes`: `output_hex`, after `spacepackets`' one-octet P-field when `include_pfield` is false. A coarse value too wide for its octets must be rejected (`OutOfRange`), and a short input as too short (`Incomplete`) | `format.mission_bits`, `format.octet2`, `format.pfield_tail_hex`, `anomalies`. The agency epoch (Level 2); 5–7 coarse or 4–10 fine octets; P-field octet 2. Known: D3 |
| `time/cuc` (`cuc_to_ns`, `cuc_from_ns`) | `subsec_nanos`: `seconds`, `nanoseconds`. `fractional_part_from_subsec_ns` with `CucTime::new_generic`: `coarse`, `fine_hex`; a sub-second part of 1 s and a value beyond the coarse octets must be rejected (`OutOfRange`) | 4–10 fine octets. Known: D1 (D4 resolved, OI-40) |
| `time/cds` | `get_dyn_time_provider_from_bytes`: `code`, `pfield_hex`, `format.epoch`, `format.day_bits`, `format.subms`, `value.day`, `value.ms_of_day`, `value.subms`, `consumed`; implicit P-field as for CUC. `CdsTime`: `output_hex`, built as a 24-bit-day value and converted with `TryFrom` for a 16-bit day segment, which must reject a day count above 16 bits (`OutOfRange`). A short input must be rejected (`Incomplete`) | `format.pfield_tail_hex`, `anomalies`. The agency epoch. Range errors: `spacepackets` checks neither the millisecond of day nor the sub-millisecond segment (TIME-021; §3). Known: D2 |
| `time/ascii` | `ascii_format`: `generate_time_code_a` or `generate_time_code_b` (the `_terminated` form when `z`), with the value built as a `chrono` date and time, and second 60 as `chrono`'s leap second. `text` must equal the vector's text with its fraction written at three digits, truncated or filled with zeros: 301.0 §3.5.1.1–2 leave the number of fraction digits to the user, and `spacepackets`' patterns fix it at three | `ascii_parse`, `ascii_subset`: no parser. The fraction digits beyond the third |
| `time/ccs`, `time/convert`, `epp/*` | — | No implementation (§4) |

**Result, 2026-10-21:** 91 agree, 118 skipped, 18 known, 0 disagreements over the twelve checked files; the five superseded `time/cuc` cases are not checked. (On 2026-10-20, before OI-40: 86 agree, 118 skipped, 22 known, with D4 ×4 in `time/cuc`.)

| Vector file | Agree | Skipped | Known | Disagree |
|---|---|---|---|---|
| `spp/decode_valid` | 13 | 0 | 0 | 0 |
| `spp/decode_invalid` | 5 | 5 | 0 | 0 |
| `spp/stream` | 3 | 2 | 0 | 0 |
| `spp/encode` | 8 | 5 | 0 | 0 |
| `spp/idle` | 6 | 2 | 0 | 0 |
| `spp/octet_string` | 3 | 3 | 0 | 0 |
| `spp/segment` | 7 | 4 | 0 | 0 |
| `spp/secondary_header_time` | 9 | 7 | 2 (D2, D3) | 0 |
| `time/pfield` | 6 | 12 | 5 (D3) | 0 |
| `time/cuc` | 16 | 9 | 9 (D1 ×2, D3 ×7) | 0 |
| `time/cds` | 11 | 9 | 2 (D2) | 0 |
| `time/ascii` | 4 | 60 | 0 | 0 |

The time code skips: `spp-tsh-007` (CCS), `-010` (Level 2), `-012` (ancillary field), `-013` and `-018` (5 coarse octets), `-014` (Secondary Header Flag rule), `-015` (CDS range); `time-pf-009` (empty input), `-013` and `-014` (reserved ID in lenient mode), `-015`…`-020`, `-021-enc`, and `-021-dec` (agency-defined code), `-022` (extension octets); `time-cuc-002`, `-010-enc`, and `-010-dec` (Level 2), `-007-enc`, `-017`, `-019`, and `-033` (beyond 4 coarse or 3 fine octets), `-008-enc` and `-009-enc` (P-field octet 2); `time-cds-003`, `-014-enc`, and `-014-dec` (agency epoch), `-007`…`-010`, `-015`, and `-017` (CDS range); the 60 ASCII parse and subset cases.

The harness was confirmed to fail on a corrupted vector, in two runs. 2026-10-19: an APID changed in `spp-dec-001` and an octet changed in `spp-enc-001`. 2026-10-20: the day of `time-cds-001`, the expected nanoseconds of `time-cuc-015` (a D1 case, which shows that a known discrepancy does not hide a wrong vector), the day of year in the text of `time-ascii-030`, and one octet of `time_code_hex` in `spp-tsh-001`. Each was reported as a disagreement, with a failing exit status, and the vectors were restored. 2026-10-21, after OI-40: `time-cuc-030` given the rounded-up result (coarse 6, fine `00`) and `time-cuc-032` the fraction `FFFFFF` were each reported as a disagreement, with a failing exit status, and the vector file was restored.

### NASA cFE `MSG` v7.0.1

cFE's `MSG` module reads and writes the fields of a message through `CFE_MSG_Get*` and `CFE_MSG_Set*`, and `CFE_MSG_Init` clears a message and sets its size and default header. It is a message library for cFS applications, not a codec: it trusts its callers (see below). The harness (`crosscheck/src/cfe.rs`, `cfe_properties.rs`) runs it in three parts, after the `spacepackets` checks in the same run, and fails on any disagreement in any part.

1. **Packets.** Every well-formed packet in each `spp/` file: the case's input, its expected `output_hex`, and its expected or given `packets_hex`, each walked packet by packet as far as `aldis-spp` decodes it in lenient mode. cFE and `aldis-spp` must read the same primary header (all seven values of the first table below), and cFE must build the same octets from Aldis's fields and data field (`CFE_MSG_Init` for the total size, then each setter, then the data field copied after the header). A case with no such packet is skipped.
2. **Expected values.** The values of a case that cFE can produce, compared with the vector as for `spacepackets`; the second table lists them.
3. **Properties.** Random packets from `aldis_testkit::strategies::spp`, built by `aldis-spp`'s encoder, drawn with a fixed ChaCha seed so that each run checks the same cases: 2000 packets with up to 46 data field octets, 200 with up to 65 536, both compared field by field and octet by octet as in part 1; and 2000 telemetry packets with a user APID whose secondary header is a CUC 4+2 time code without a P-field, encoded by `aldis-time` and `aldis-spp`, which `CFE_MSG_GetMsgTime` must read as the same seconds and fraction, and `CFE_MSG_SetMsgTime` must write into a packet cFE built as the same octets.

**What cFE reads and writes**, by field (`cfe_msg_ccsdspri.c`, `cfe_msg_init.c`, `cfe_msg_sechdr_time.c`; the extended header and the command secondary header from `cfe_msg_ccsdsext.c`, `cfe_msg_sechdr_fc.c`, `cfe_msg_sechdr_checksum.c`, and the `option_inc/` variants):

| 133.0 field, or cFE item | cFE API | Compared |
|---|---|---|
| Packet Version Number (§4.1.3.2) | `Get`/`SetHeaderVersion` (3 bits) | Yes: `000` on every packet and set when building. Other values only as skips: Aldis rejects them, cFE reads them and rejects none |
| Packet Type (§4.1.3.3.2) | `Get`/`SetType` (`Tlm`, `Cmd`) | Yes |
| Secondary Header Flag (§4.1.3.3.3) | `Get`/`SetHasSecondaryHeader` | Yes |
| APID (§4.1.3.3.4) | `Get`/`SetApId` (rejects more than 11 bits) | Yes, the idle APID included |
| Sequence Flags (§4.1.3.4.2) | `Get`/`SetSegmentationFlag` (all four values) | Yes |
| Packet Sequence Count (§4.1.3.4.3) | `Get`/`SetSequenceCount` (rejects more than 14 bits); `GetNextSequenceCount` (increment modulo 2^14) | Yes; the next count against `next_count` of `spp/octet_string` and `spp/segment` |
| Packet Data Length (§4.1.3.5) and total length | `GetSize`, `SetSize` through `CFE_MSG_Init` (total 7–65 542 octets) | Yes; `SetSize`'s range against the empty data field (SPP-005) and the 65 537-octet data field |
| Packet Data Field (§4.1.4) | none: an application writes its payload into the structure | The octets of the built packet |
| Message identifier, version 1 | `Get`/`SetMsgId`: the first 16 bits as a routing key | No: a cFE Software Bus key, not a 133.0 field. Linked because `CFE_MSG_Init` calls `SetMsgId`; the harness passes `CFE_SB_INVALID_MSG_ID`, which skips it |
| CCSDS extended header: subsystem, system, EDS version, endian, playback | `cfe_msg_ccsdsext.c` with `default_cfe_msg_hdr_priext.h`: 4 octets right after the primary header | No: mission-specific, and not compiled. For 133.0 it is the start of the Packet Secondary Header, which Aldis treats as opaque octets (§4.1.4.2.1.5 b; SPP-007) |
| Telemetry time | `Get`/`SetMsgTime`: 4 octets of seconds, then the upper 16 bits of the 32-bit subseconds (units of 2^-32 s), big-endian, right after the primary header | Yes, as a CUC T-field of 4 coarse and 2 fine octets without a P-field (301.0 §3.2; 133.0 §4.1.4.2.2.4 allows the P-field to be left out when the code's characteristics are fixed), Aldis's `TimeHeaderLayout` with that format and `pfield` false: the fine octets are the binary fraction of a second, so cFE's subseconds are Aldis's left-aligned fraction shifted right by 96 bits. cFE truncates the subseconds to 16 bits when writing. The epoch is a cFE mission setting, outside the packet, so the octets do not say whether the code is Level 1 or Level 2 |
| `Spare[4]` of `CFE_MSG_TelemetryHeader_t` | none: alignment padding of the C structure (primary header only), not read or written by `MSG` | No. For Aldis, 4 octets of Ancillary Data Field after the time code (§4.1.4.2.3) |
| Command secondary header: function code, checksum | `cfe_msg_sechdr_fc.c`, `cfe_msg_sechdr_checksum.c` | No: cFE's own convention, not 133.0; for Aldis opaque secondary header octets. Not compiled |

**By vector file:**

| Vector files | Packets (part 1) | Expected values (part 2) | Out of scope, and why |
|---|---|---|---|
| `spp/decode_valid`, `spp/decode_invalid`, `spp/field_codes` (`decode`, `peek_length`) | Each input packet | `decode`: `packet_type`, `secondary_header_flag`, `apid`, `sequence_flags`, `sequence_count`, `packet_len`, `consumed`, `data_len`, `data_hex`. `peek_length`: `len` | `idle`, `packet_name`, `anomalies`. Every rejection: cFE rejects no version (`UnsupportedVersion`), does not compare the length with the buffer (`Incomplete`), and has no Idle Packet rule (SPP-004) |
| `spp/stream` | Each packet of the stream | The stream split by `CFE_MSG_GetSize`: `apid`, `sequence_count`, `consumed` of each packet, `stop_error` (none or `Incomplete`), `offset` | `anomalies`; streams that stop at an error cFE does not detect |
| `spp/encode`, `spp/octet_string` | The expected packet | Built with `CFE_MSG_Init` and the setters from the parameters: `output_hex`, and `next_count` for the Octet String; for `encode` also `aldis-spp`'s encoder on the same parts. An empty data field and one above 65 536 octets must be rejected (`CFE_MSG_BAD_ARGUMENT`) | The rejection rules cFE does not have: an Idle Packet with a secondary header (SPP-005), the idle APID for the Octet String (SPP-012), `max_packet_len` (SPP-006) |
| `spp/idle` | The expected packet, which cFE also builds from Aldis's fields and the pattern | Read by cFE: APID 2047, Secondary Header Flag 0, Sequence Flags `11`, the given count and total length. A total length below 7 must be rejected | The idle pattern; `max_packet_len` |
| `spp/segment` | Each expected packet | Read by cFE: the type, APID, Secondary Header Flag, count (advanced with `CFE_MSG_GetNextSequenceCount`), and length of each; `next_count` | The split of the SDU; rejections (no segmentation logic) |
| `spp/reassemble`, `spp/split` | Each packet | — | No reassembler; no secondary header length, which in cFE is set by the application's C structures |
| `spp/sequence_tracker` | — (no packets) | — | No sequence tracker |
| `spp/secondary_header_time` (`split_time`) | Each input packet | With cFE's layout (CUC 4+2, no P-field; any ancillary length): `CFE_MSG_GetMsgTime` gives `time_code_hex`, `CFE_MSG_SetMsgTime` writes the expected time code into the packet with that field cleared, and `aldis-spp`'s split reads the same value. Secondary Header Flag 0 must be rejected (`CFE_MSG_WRONG_MSG_TYPE`) | Other layouts: CDS, CCS, other CUC sizes, an explicit P-field. `LengthMismatch`: cFE does not compare the time with the Packet Data Length. The split into secondary header, ancillary data, and user data. A telecommand (none in the file): cFE defines the time for telemetry only |
| `time/cuc` (`decode`, `decode_implicit`, `encode`) | — | With 4 coarse and 2 fine octets: the T-field (after the expected P-field) in a telemetry packet cFE built, read by `CFE_MSG_GetMsgTime` as `value.coarse` and `value.fine_hex`; `encode` written by `CFE_MSG_SetMsgTime`, which must give the last 6 octets of `output_hex` (cFE writes no P-field) | Every other format; rejections (cFE's seconds are a `uint32` and it checks no value); `cuc_to_ns` and `cuc_from_ns` (`CFE_MSG` converts nothing; `CFE_TIME` is not examined, §4.7) |
| Properties | — | Part 3 | — |

**What cFE does not check.** It trusts its callers, so these differences are skips, never disagreements, and none is a discrepancy in a checked value:

- No function knows the length of the buffer. The getters read the 6 header octets, `CFE_MSG_GetMsgTime` and `CFE_MSG_SetMsgTime` 12 octets, and `CFE_MSG_Init` clears `Size` octets before it checks `Size`. The harness gives every call a buffer of at least 16 octets and of at least `Size`, and checks a short packet's time only where the vector expects one.
- No length check on reading: `CFE_MSG_GetSize` returns the Packet Data Length plus 7, whatever follows (SPP-002).
- No version check, except that `CFE_MSG_GetMsgTime` and `SetMsgTime` require version `000` (`CFE_MISSION_CCSDSVER`), telemetry, and the Secondary Header Flag (SPP-001).
- No Idle Packet rule: APID 2047 with a secondary header is read and built (SPP-004, SPP-005, SPP-010).
- No rule between the Secondary Header Flag and the data field: cFE builds the flag with any data field of at least one octet (SPP-005), and has no maximum packet length below 65 542 (SPP-006).

**Result, 2026-10-21:** part 1, 78 agree, 36 skipped, 0 known, 0 disagreements over twelve files; part 2, 49 agree, 73 skipped, 0 known, 0 disagreements over ten files; part 3, 4200 agree, 0 disagreements.

| Vector file | Packets: agree | skipped | Values: agree | skipped |
|---|---|---|---|---|
| `spp/decode_valid` | 10 | 3 (`peek_length` header only) | 13 | 0 |
| `spp/decode_invalid` | 1 (`spp-inv-008`, lenient) | 9 | 0 | 10 |
| `spp/field_codes` | 1 | 3 | 1 | 3 |
| `spp/stream` | 5 | 0 | 3 | 2 |
| `spp/encode` | 7 | 6 | 9 | 4 |
| `spp/idle` | 6 | 2 | 7 | 1 |
| `spp/octet_string` | 3 | 3 | 4 | 2 |
| `spp/segment` | 7 | 4 | 7 | 4 |
| `spp/reassemble` | 14 | 0 | — | — |
| `spp/sequence_tracker` | 0 | 6 | — | — |
| `spp/split` | 6 | 0 | — | — |
| `spp/secondary_header_time` | 18 | 0 | 3 (`spp-tsh-001`, `-006`, `-014`) | 15 |
| `time/cuc` | — | — | 2 (`time-cuc-006-enc`, `-006-dec`) | 32 |

The value skips: `spp-inv-001`…`-010`, `spp-field-001`…`-003` (rejections cFE does not make); `spp-str-003`, `-004` (stops at `UnsupportedVersion` and `InvalidField`); `spp-enc-005`, `-008`, `-009`, `-010`, `spp-idle-006`, `spp-oct-005`, `-006` (rules cFE does not have); `spp-seg-007`…`-010` (segmentation rules); `spp-tsh-011`, `-012` (`LengthMismatch`) and the 13 other `spp-tsh` cases whose layout is not cFE's; the 32 `time/cuc` cases of other formats, of `OutOfRange` (`time-cuc-011`), or of the nanosecond conversions. Every skip is printed with its reason.

The harness was confirmed to fail on a corrupted vector on 2026-10-21: the APID of `spp-dec-001`, the last octet of `spp-enc-001`, one octet of `time_code_hex` in `spp-tsh-001`, and `value.coarse` of `time-cuc-006-dec` were each reported as a disagreement by cFE and by `spacepackets`, with a failing exit status, and the vectors were restored. Its tests (`crosscheck/src/cfe.rs`) show the same for a wrong decoding, encoding, and time vector.

## 3. Discrepancies

Three are known differences in `spacepackets`, each resolved against the Blue Book text in favour of Aldis; the harness reports them under their identifiers (§2). The fourth, D4, was resolved by changing Aldis to agree with `spacepackets` (OI-40), and the harness no longer reports it. cFE `MSG` v7.0.1 has none: on every value it produces it agrees with the vectors and with `aldis-spp` (2026-10-21). A human review of the four readings against 301.0, made without this section's reasoning, is recorded in [reviews/human-301-discrepancies.md](reviews/human-301-discrepancies.md).

**D1. CUC fine time to nanoseconds** (`time-cuc-015`, `-016`). `spacepackets` divides the fine time counter of n octets by 2^(8n) − 1, deliberately (`fractional_res_to_div`, `convert_fractional_part_to_ns`). 301.0 §3.2.1 defines the fine time octets as the elapsed binary fraction of the basic time unit, each octet the state of 8 consecutive bits of a binary counter that rolls over at a modulo of 256, cascaded with the coarse octets. A fraction of n octets therefore counts units of 2^(−8n) s, and the divisor is 2^(8n), as TIME-011 specifies. The two readings differ by one part in 2^(8n) − 1; at the largest counter, `spacepackets` reports a sub-second part of a full 1 000 000 000 ns (`time-cuc-016`). **Resolved in favour of Aldis**; the vectors stand.

**D2. CDS sub-millisecond code `11`** (`time-cds-004`, `-005`, `spp-tsh-017`). 301.0 §3.3.2 marks code `11` of P-field bits 6–7 as reserved for future use and gives it no segment length. `spacepackets` decodes it as a code without a sub-millisecond segment (`calc_stamp_len` adds no octets for `Reserved`; the 24-bit-day reader also drops the code from the P-field it reports). That assumes a length the Blue Book does not define. Aldis rejects the code with `ReservedValue` in both modes, because it sets the length of the T-field (TIME-020, OI-15). **Resolved in favour of Aldis.**

**D3. P-field extension flag** (`time-cuc-003`, `-004`, `-005`, `-007-dec`, `-008-dec`, `-009-dec`, `-028`; `time-pf-003`…`-007`; `spp-tsh-003`). 301.0 §3.1.1 makes P-field bit 0 the extension flag: `1` means that another P-field octet follows. For CUC, §3.2.2 defines that octet 2 (additional coarse and fine octets, mission-defined bits). `CucTime::from_bytes` and the CDS readers of `spacepackets` take the T-field layout from octet 1 alone and do not read the flag, so they take octet 2 as the first T-field octet and report a one-octet P-field. Its documentation states the one-octet limit (`MAX_CUC_LEN_SMALL_PREAMBLE`), but the readers do not reject an extended P-field. Aldis reads the extension chain (TIME-001, TIME-005, TIME-010). **Resolved in favour of Aldis.**

**D4. Nanoseconds to CUC fine time** (`time-cuc-020`, `-022`, `-023`, `-025`, superseded by `time-cuc-029`…`-032`). This direction does not use the divisor of D1: `fractional_part_from_subsec_ns` computes ns × 2^(8n) / 10^9, the binary fraction of 301.0 §3.2.1, and rounds it down. TIME-011 rounded it up, so that the round trip through nanoseconds was exact for 0–3 fine octets: a fraction that rounded up to 2^(8n) carried into the coarse time (`time-cuc-022`), and a carry beyond the coarse octets was `OutOfRange` (`time-cuc-023`, where `spacepackets` returns 255 s and fine time `FF`). 301.0 defines the fine time as a binary fraction but gives no conversion from decimal seconds, so both roundings conform. **Resolved by changing Aldis** (OI-40, maintainer, 2026-10-21, from Q4 of the review): TIME-011 now rounds down as well, so no carry or overflow can occur. The four cases are superseded by `time-cuc-029`…`-032`, which hold the rounded-down results (`-031`: 255 s and fine time `FF`, as `spacepackets` gives), and `spacepackets` agrees on them; the harness reports no D4, so a difference in `cuc_from_ns` is a disagreement. An exact fraction (`time-cuc-021`, `-034`), a sub-second part of 1 s (`-024`), and a coarse overflow (`-027`) agree, as before.

The version of this section of 2026-10-19 expected D1 in `cuc_from_ns` as well, and every nanosecond case to be excluded. The check shows that `cuc_from_ns` differs only in the rounding (D4), so the harness checks both directions instead.

Observed in `spacepackets`, not discrepancies in a checked value:

- `CcsdsPacketCreator` computes the Packet Data Length as `len − 1` without checking for an empty data field. That subtraction underflows: it panics in a debug build and wraps in a release build. 133.0 §4.1.4.1.2 requires at least one octet of Packet Data Field, which Aldis enforces (`InvalidField`, `spp-enc-004`). The harness never passes that input.
- Its CDS readers and constructors check no range: a millisecond of day of 86 401 000 or more, a microsecond segment of 1000 or more, and a picosecond segment of 10^9 or more are accepted. Aldis rejects them in both modes (TIME-021). The cases are skipped as a rule `spacepackets` does not have.

Observed in cFE `MSG` v7.0.1, not discrepancies in a checked value (its unchecked inputs are listed in §2):

- `CFE_MSG_Init` clears `Size` octets of the caller's buffer before `CFE_MSG_SetSize` checks `Size`, so a size above the buffer overruns it even when the call then fails. The harness's buffers are never shorter than the size.
- `CFE_MSG_SetMsgTime` keeps only the upper 16 bits of the subseconds: it truncates a time to 2^-16 s, the resolution of 2 fine octets (301.0 §3.2.1).
- Without the extended header, `CFE_MSG_TelemetryHeader_t` has 4 spare octets after the time, so a cFE application's payload starts 16 octets into the packet; with the extended header the time follows the 4 extended header octets and there is no spare. Either way, for 133.0 these octets are Packet Secondary Header content, which `aldis-spp` describes with an Ancillary Data Field or an opaque layout.
- cFE defines its time for telemetry only; a command's secondary header holds a function code and a checksum instead. 133.0 §4.1.4.2.1.5 allows a Time Code Field in the secondary header of any packet, and §4.1.4.2.1.6 fixes the choice per managed data path, which cFE's convention conforms to.

## 4. Standards with no independent implementation

The features `spacepackets` does not cover (§2), searched 2026-10-20 for packets and time (M1) and 2026-10-26 for the four frame standards (M2 D15, WP-6). cFE `MSG` (§1, run since 2026-10-21) adds a second implementation of the primary header and of CUC 4+2 time codes without a P-field, which `spacepackets` already covers, and of the count increment modulo 2^14 (§4.6); it covers none of the features below otherwise. Search covered crates.io (the `ccsds` query and the crates it lists), PyPI, GitHub, and agency and mission tools. A candidate qualifies when it is open source under a license that allows running it as a test oracle, shares no code or authors with Aldis, and implements the feature as 132.0, 133.1, 232.0, 301.0, 732.0, or 732.1 define it. Candidates were read at source level unless the entry says otherwise.

Every candidate below is independent of Aldis. None is run yet: each that qualifies needs a JVM, Python, or a Prolog runtime in `crosscheck/`, or is a single-maintainer beta crate. Maintainer decision 2026-10-20: M1 closes with the `spacepackets` checks above, and running these candidates is [OI-32](../09-open-issues.md), targeted at M2. In M2 WP-6 the frame candidates were evaluated (D15; §4.7–§4.10); none was wired into `crosscheck/`. Until an independent oracle is run, the vectors of these features stay `hand-derived`, with the derivation written down (D5) and reviewed by an independent session (M1 §5; M2 WP-8, [m2-vectors.md](reviews/m2-vectors.md)).

### 4.1 CUC P-field octet 2 and the agency epoch (Level 2)

`time/pfield`, `time/cuc`: the extension chain, 5–7 coarse and 4–10 fine octets, mission-defined bits, Level 2.

| Candidate | License | Coverage | Verdict |
|---|---|---|---|
| Orekit `CcsdsUnsegmentedTimeCode` (Java, CS GROUP; [source](https://github.com/CS-SI/Orekit/blob/develop/src/main/java/org/orekit/time/CcsdsUnsegmentedTimeCode.java)) | Apache-2.0 | Decode only. Reads octet 2 when bit 0 is set (additional coarse and fine octets, 301.0 §3.2.2); Level 2 with an epoch the caller supplies. Its fine-time scale table has seven entries, so 8–10 fine octets are out of reach | Qualifies, for decoding with up to 7 fine octets |
| `eu.dariolucia.ccsds` `TimeUtil.toCUC`/`fromCUC` (Java; [repository](https://github.com/dariol83/ccsds), archived 2024) | Apache-2.0 | Encode and decode with octet 2 and an agency epoch, through `java.time.Instant` (nanoseconds) | Qualifies for 0–3 fine octets; beyond that its nanosecond path loses the fraction |
| `puslib` `time.py` (Python; [repository](https://github.com/pxntus/puslib)) | MIT | Encodes and decodes the P-field with octet 2, 1–7 coarse and 0–10 fine octets; an epoch selects Level 2. Its float conversion is lossy | Qualifies, on the raw coarse and fine fields only |
| `deep-time` (Rust; [repository](https://github.com/ragardner/deep-time)) | MIT OR Apache-2.0 | CUC with a one- or two-octet P-field, 1–7 coarse and 0–10 fine octets; Level 1 only. A 0.1 beta from 2026; its tests compare with `spacepackets` | Qualifies for octet 2, not Level 2. The cheapest to add (Rust), but immature |
| Logtalk `ccsds_time_fields` ([repository](https://github.com/LogtalkDotOrg/logtalk3), `library/`) | Apache-2.0 | CUC with octet 2, 1–7 coarse and 0–10 fine octets; maps the agency epoch to the Unix epoch | Qualifies for octet 2 only |
| Yamcs `CucTimeDecoder` ([repository](https://github.com/yamcs/yamcs)) | AGPL-3.0 | Its extension test `(pfield1 & 0x80) == 1` is never true, so octet 2 is ignored; no epoch; millisecond resolution | Does not qualify |
| `ccsds` crate (bmflynn), `satellite-datetime` (Rust) | MIT OR Apache-2.0 | 1–4 coarse and 0–3 fine octets, as `spacepackets` | Does not qualify (no coverage beyond §2) |
| `spacepackets` (Python) | Apache-2.0 | No CUC class; same authors as the Rust crate | Does not qualify |

### 4.2 CCS (301.0 §3.4)

`time/ccs`: BCD segments, both calendar variations, 0–6 sub-second octets, second 60.

| Candidate | License | Coverage | Verdict |
|---|---|---|---|
| Logtalk `ccsds_time_codes` (`ccsds_ccs.lgt`) | Apache-2.0 | BCD with digit checks, both variations, sub-second octets. Rejects second 60, which the UTC-based CCS needs (301.0 §3.4.1) | Qualifies, except leap-second cases |
| `deep-time` (`from_ccsds_ccs`, `to_ccsds_ccs`) | MIT OR Apache-2.0 | BCD encode and decode, both variations, 0–6 sub-second octets, second 60 | Qualifies (beta) |
| `maser-data` `maser.data.cdpp.ccsds` (Python, Observatoire de Paris) | CeCILL-B | Lists a CCS class with BCD helpers; the source could not be read (its GitLab host refused the request) | Not verified |
| Orekit `CcsdsSegmentedTimeCode` | Apache-2.0 | Reads the CCS segments as binary, not BCD | Does not qualify: contradicts 301.0 §3.4.1 |
| `eu.dariolucia.ccsds`, Yamcs, `spacepackets` (Rust and Python), `satellite-datetime` | — | No CCS | — |

### 4.3 ASCII parsing and subsets (301.0 §3.5)

`time/ascii`: `ascii_parse` and `ascii_subset`.

| Candidate | License | Coverage | Verdict |
|---|---|---|---|
| Orekit `DateTimeComponents.parseDateTime` | Apache-2.0 | Parses complete codes A (calendar) and B (ordinal), any number of fraction digits, an optional `Z`, and right-truncated forms. Also accepts forms 301.0 does not (ISO 8601 basic format, comma decimals, offsets), and has no left-truncated subsets (§3.5.1.3 c) | Qualifies, for complete codes and right truncation only |
| `deep-time` | MIT OR Apache-2.0 | Formats A and B; a generic parser, not checked for day-of-year input | Not verified for parsing |
| Yamcs `TimeEncoding.parse` | AGPL-3.0 | Not read at source level | Not verified |
| `chrono` (with the patterns of §1) | MIT OR Apache-2.0 | A general strftime parser; implements none of the CCSDS rules (optional `Z`, digit counts, subsets) | Does not qualify as a CCSDS implementation |

No implementation of the subset rules of §3.5.1.3 was found.

### 4.4 Conversions between CDS, CCS, and ASCII

`time/convert`.

| Candidate | License | Coverage | Verdict |
|---|---|---|---|
| `deep-time` | MIT OR Apache-2.0 | One date-time type with CDS and CCS decode and encode and the A and B strings, UTC with leap seconds | Qualifies (beta; its leap-second table needs checking) |
| Orekit (`AbsoluteDate`) | Apache-2.0 | CDS decode to a date, ISO strings both ways; needs its leap-second data files; no CDS encoder; CCS as in §4.2 | Qualifies for CDS to ASCII only |
| Logtalk, `eu.dariolucia.ccsds` | Apache-2.0 | Convert through Unix seconds or `Instant`, which have no leap second | Does not qualify for second 60 |
| `spacepackets` (Python) `CdsShortTimestamp` | Apache-2.0 | CDS to `datetime` | Does not qualify: same authors as the Rust crate of §1 |

### 4.5 EPP (133.1-B-3)

`epp/*`. Searched "encapsulation packet" on GitHub, PyPI, and crates.io, and in Yamcs, NASA HDTN, the Wireshark CCSDS dissector, and gr-satellites.

| Candidate | License | Coverage | Verdict |
|---|---|---|---|
| `eu.dariolucia.ccsds` `EncapsulationPacket`, `EncapsulationPacketBuilder` ([source](https://github.com/dariol83/ccsds/blob/master/eu.dariolucia.ccsds.tmtc/src/main/java/eu/dariolucia/ccsds/tmtc/transport/pdu/EncapsulationPacket.java)) | Apache-2.0 | Decode and encode of the 1-, 2-, 4-, and 8-octet headers with every field; the builder chooses the header size or takes it. Decode does not check the Packet Version Number | Qualifies; the best EPP candidate found |
| NASA HDTN `CcsdsEncap*.h` ([repository](https://github.com/nasa/HDTN)) | NOSA 1.3 | All four header sizes; the encoder picks the smallest. Decode accepts only HDTN's own values of the Protocol ID extension and CCSDS-defined fields | Partial: lengths and header sizes |
| Yamcs `PacketDecoder` | AGPL-3.0 | Header size, packet length, and idle detection for all four sizes; no other fields | Partial |
| `spacepackets` (Rust and Python), Wireshark `packet-ccsds.c`, gr-satellites, every Rust crate found | — | No EPP | — |

### 4.6 Space Packet reassembly, segmentation, sequence tracking, and secondary headers

`spp/reassemble`, `spp/segment` (sender side), `spp/sequence_tracker`, `spp/split`.

| Candidate | License | Coverage | Verdict |
|---|---|---|---|
| `space_packet_parser` (Python, LASP; [PyPI](https://pypi.org/project/space-packet-parser/)), `combine_segmented_packets` | BSD-3-Clause | Reassembles per APID, drops an orphan continuation, and drops a group with a count gap; its policy is its own, not SPP-013's | Qualifies for reassembly, where the policies coincide |
| `ccsds` crate (bmflynn) | MIT OR Apache-2.0 | Counts sequence gaps with wrap-around; groups packets without checking their flags | Partial: gap counting |
| F Prime `Svc/Ccsds/ApidManager` ([repository](https://github.com/nasa/fprime)) | Apache-2.0 | Generates and checks counts per APID | Partial |
| NASA cFE `MSG` v7.0.1 `CFE_MSG_GetNextSequenceCount` (§1) | Apache-2.0 | The next count modulo 2^14; no tracker, segmentation, or reassembly | Run (§2) for `next_count` of `spp/octet_string` and `spp/segment` |
| `ccsdspy` (NASA), `minspp` | BSD-3-Clause, GPL-3.0 | Sequence warnings and an assembler, not read at source level | Not verified |

No sender-side segmentation was found: libraries expose the Sequence Flags but do not split an SDU.

### 4.7 TM Transfer Frame (132.0-B-3)

`tm/*`: `fhp_values`, `fsh_ocf_fecf`, `invalid`, `minimal`, `oid_sequence`, `security_layout`, `config`. Evaluated 2026-10-26 (M2 D15).

| Candidate | License | Coverage | Verdict |
|---|---|---|---|
| `eu.dariolucia.ccsds` `TmTransferFrame` (Java; [repository](https://github.com/dariol83/ccsds), archived 2024) | Apache-2.0 | Decode and encode of TM transfer frames with MC/VC counters, FHP packet extraction, OCF, and FECF (CRC-16). Does not model SDLS Security Header/Trailer layout inside the frame | Qualifies for primary header and OCF decoding; not run in `crosscheck/` because it needs a JVM runtime (OI-32) |
| Yamcs `TmTransferFrame` / `TmFrameDecoder` (Java; [repository](https://github.com/yamcs/yamcs)) | AGPL-3.0 | Frame length, VCID, MC/VC counters, FHP pointer, OCF, CRC-16. Part of the Yamcs mission control server framework | Does not qualify: AGPL-3.0 copyleft, requires the full Yamcs runtime and a JVM (OI-32) |
| NASA CryptoLib `crypto_tm.c` ([repository](https://github.com/nasa/CryptoLib)) | NOSA 1.3 | Space Data Link Security (CCSDS 355.0-B-1) processing on TM frames only (SPI lookup, IV, MAC authentication, decryption). Does not decode or validate TM primary header fields (SCID, VCID, MCFC, VCFC, FHP, POF, SLID) or OCF | Does not qualify: an SDLS security processor, not a transfer frame codec |
| `spacepackets` (Rust; crates.io) | Apache-2.0 | Space Packet Protocol (133.0) and PUS packets only; no TM transfer frame (132.0) implementation | Does not qualify: no TM frame support |
| `gr-satellites` `ccsds_combined_deframer` / `ccsds_demux` (Python/C++; [repository](https://github.com/daniestevez/gr-satellites)) | GPL-3.0 | CADU synchronization, pseudo-noise derandomization, Reed-Solomon (255, 223), and demuxing packets from a telemetry stream. No standalone TM transfer frame codec or validator | Does not qualify: a streaming physical-layer deframer with GNU Radio runtime dependencies; no structured frame codec |
| Wireshark `epan/dissectors/packet-ccsds.c` (C; [GitLab](https://gitlab.com/wireshark/wireshark)) | GPL-2.0 | Dissects Space Packets (`packet-ccsds.c`) and CFDP (`packet-cfdp.c`). Upstream Wireshark has no TM transfer frame dissector | Does not qualify: no upstream TM transfer frame dissector |
| `ccsds` crate (bmflynn, Rust; crates.io) | MIT OR Apache-2.0 | CADU synchronization, derandomization, RS decoding, and packet extraction using FHP. Exposes raw CADU bytes; no TM frame primary header decoder (MC/VC counters, FSH, OCF, SLID, POF) | Does not qualify: deframer only, no TM frame header codec |
| Orekit (Java, CS GROUP) | Apache-2.0 | Navigation Data Messages (NDM) and time codes; no TM transfer frames | Does not qualify: no frame support |

### 4.8 TC Transfer Frame and CLCW (232.0-B-4)

`tc/*`: `ad`, `bd`, `bc_unlock`, `bc_setvr`, `clcw`, `config`, `decode_errors`, `delimit_multiple_frames_with_fill`, `max_length`, `segment_header`, `validation_failures`. Evaluated 2026-10-26 (M2 D15).

| Candidate | License | Coverage | Verdict |
|---|---|---|---|
| `eu.dariolucia.ccsds` `TcTransferFrame` / `Clcw` (Java; [repository](https://github.com/dariol83/ccsds), archived 2024) | Apache-2.0 | Decode and encode of variable-length TC transfer frames (bypass and sequence-controlled flags, VCID, frame length, sequence number), COP-1 control commands, Segment Header, and CLCW (including COP-1 FARM status fields). Does not model SDLS layout or the delimiting and fill data removal algorithm (TC-020) | Qualifies for TC frame header and CLCW decoding; not run in `crosscheck/` because it needs a JVM runtime (OI-32) |
| Yamcs `TcTransferFrame` / `TcFrameDecoder` (Java; [repository](https://github.com/yamcs/yamcs)) | AGPL-3.0 | TC frame encoding, multiplexing, and CLCW parsing from return link OCF. Part of the Yamcs mission control server framework | Does not qualify: AGPL-3.0 copyleft, requires the full Yamcs runtime and a JVM (OI-32) |
| NASA CryptoLib `crypto_tc.c` ([repository](https://github.com/nasa/CryptoLib)) | NOSA 1.3 | SDLS (CCSDS 355.0-B-1) processing on TC frames only (SPI lookup, IV, authentication and decryption). Does not decode TC frame headers, Segment Headers, or CLCW | Does not qualify: an SDLS security processor, not a transfer frame codec |
| `spacepackets` (Rust; crates.io) | Apache-2.0 | Space Packet Protocol (133.0) and PUS telecommands only; no TC transfer frame (232.0) or CLCW support | Does not qualify: no TC frame support |
| `gr-satellites` (Python/C++; [repository](https://github.com/daniestevez/gr-satellites)) | GPL-3.0 | Downlink telemetry reception only; no TC frame codec, delimiter, or CLCW support | Does not qualify: no TC frame support |
| Wireshark (C; [GitLab](https://gitlab.com/wireshark/wireshark)) | GPL-2.0 | Upstream Wireshark has no TC transfer frame or CLCW dissector | Does not qualify: no upstream TC frame dissector |
| `ccsds` crate (bmflynn, Rust; crates.io) | MIT OR Apache-2.0 | Telemetry framing only; no TC frame or CLCW support | Does not qualify: no TC frame support |

### 4.9 AOS Transfer Frame and FHEC (732.0-B-5 EC1)

`aos/*`: `minimal`, `fhec_frame`, `fhec_encode`, `fhec_correct_1_symbol`, `fhec_correct_2_symbols`, `fhec_uncorrectable`, `fhec_miscorrection`, `insert_zone`, `security_layout`, `mpdu_pointers`, `bpdu_pointers`, `oid`, `issue4_compat`, `scid_10bit`, `config`, `invalid`. Evaluated 2026-10-26 (M2 D15).

| Candidate | License | Coverage | Verdict |
|---|---|---|---|
| `eu.dariolucia.ccsds` `AosTransferFrame` (Java; [repository](https://github.com/dariol83/ccsds), archived 2024) | Apache-2.0 | Decode and encode of AOS Transfer Frames conforming to Issue 4 (CCSDS 732.0-B-4, 8-bit SCID). Uses Project Nayuki Reed-Solomon for check-only; explicitly lacks error correction capabilities, so it cannot correct 1 or 2 symbol errors in the FHEC (AOS-013, AOS-014). No Issue 5 (10-bit SCID) support | Partial (Issue 4 parsing only; no FHEC correction); not run in `crosscheck/` because it needs a JVM runtime (OI-32) |
| Yamcs `AosTransferFrame` / `AosFrameHeaderErrorCorr` (Java; [repository](https://github.com/yamcs/yamcs)) | AGPL-3.0 | AOS frame decoding, Insert Zone, M_PDU/B_PDU, Reed-Solomon (10,6) FHEC error detection and correction. Part of the Yamcs mission control server framework | Does not qualify: AGPL-3.0 copyleft, requires the full Yamcs runtime and a JVM (OI-32) |
| NASA CryptoLib `crypto_aos.c` ([repository](https://github.com/nasa/CryptoLib)) | NOSA 1.3 | SDLS processing on AOS frames only (SPI, IV, MAC, encryption/decryption). Does not decode AOS primary header fields, FHEC, M_PDU, or B_PDU pointers | Does not qualify: an SDLS security processor, not an AOS frame codec |
| `spacepackets` (Rust; crates.io) | Apache-2.0 | No AOS frame (732.0) or FHEC support | Does not qualify: no AOS support |
| `gr-satellites` (Python/C++; [repository](https://github.com/daniestevez/gr-satellites)) | GPL-3.0 | No AOS frame or FHEC support | Does not qualify: no AOS support |
| Wireshark (C; [GitLab](https://gitlab.com/wireshark/wireshark)) | GPL-2.0 | Upstream Wireshark has no AOS dissector | Does not qualify: no upstream AOS dissector |
| `ccsds` crate (bmflynn, Rust; crates.io) | MIT OR Apache-2.0 | No AOS frame or FHEC support | Does not qualify: no AOS support |

### 4.10 USLP Transfer Frame (732.1-B-3)

`uslp/*`: `fixed_packets_rule000`, `fixed_mapa_rules001_010`, `variable_rules011_111`, `vcf_count_lengths_0_to_7`, `truncated`, `truncated_sdls_layout`, `oid`, `insert_zone`, `protocol_control_cop1`, `delimit_multiple_frames_with_fill`, `validation_failures`, `config`, `invalid`. Evaluated 2026-10-26 (M2 D15).

| Candidate | License | Coverage | Verdict |
|---|---|---|---|
| `spacepackets` `uslp` module (`TransferFrameReader`, `TransferFrameCreator`; Rust; [crates.io](https://crates.io/crates/spacepackets)) | Apache-2.0 | Primary Header (16-bit SCID, Source/Dest, VCID, MAP ID, Frame Length, Sequence Control, Protocol Control, OCF flag, 0–7 octet VCF count) and TFDF Header (rules 000–111, FHP/LVO, closed `UslpProtocolId` enum). Rejects truncated frames (`UslpError::HeaderIsTruncated`, Annex D); assumes Insert Zone length 0; no Security Header or Trailer support; closed protocol ID enum rejects registered SANA UPIDs outside its enum (USLP-007); no frame delimiting or fill data removal (USLP-014, USLP-015); no USLP Frame Validation checks | Partial: primary header and basic TFDF decoding of fixed/variable user data frames without insert zone or security. Does not cover truncated frames, insert zone, SDLS, delimiting, or validation |
| Yamcs `org.yamcs.tctm.ccsds` USLP classes (Java; [repository](https://github.com/yamcs/yamcs)) | AGPL-3.0 | USLP uplink frame building and encoding according to CCSDS 732.1-B-3. Part of the Yamcs mission control server framework | Does not qualify: AGPL-3.0 copyleft, requires the full Yamcs runtime and a JVM (OI-32) |
| `eu.dariolucia.ccsds` (Java; [repository](https://github.com/dariol83/ccsds), archived 2024) | Apache-2.0 | No USLP (CCSDS 732.1) support | Does not qualify: no USLP support |
| NASA CryptoLib (C; [repository](https://github.com/nasa/CryptoLib)) | NOSA 1.3 | No USLP (CCSDS 732.1) support | Does not qualify: no USLP support |
| `gr-satellites` (Python/C++; [repository](https://github.com/daniestevez/gr-satellites)) | GPL-3.0 | No USLP support | Does not qualify: no USLP support |
| Wireshark (C; [GitLab](https://gitlab.com/wireshark/wireshark)) | GPL-2.0 | Upstream Wireshark has no USLP dissector | Does not qualify: no upstream USLP dissector |
| `ccsds` crate (bmflynn, Rust; crates.io) | MIT OR Apache-2.0 | No USLP support | Does not qualify: no USLP support |

### 4.11 Not available

ESA SCOS-2000, EGOS, and libgcs are not published as open source. OpenC3 COSMOS is not under an OSI license and has only day, millisecond, and microsecond helpers. `satrs` and `tmtccmd` share authors with `spacepackets`. NASA cFE's `MSG` module is run (§1, §2); its `CFE_TIME` module, which converts and formats times, is not examined. Not examined either: AMMOS MMTC, SpacePy, astropy, libCSP, and Go or JavaScript packages.
