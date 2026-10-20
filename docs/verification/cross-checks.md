# Independent cross-checks (VER-040)

The `crosscheck/` workspace runs the vector files under `test-vectors/` through independent, existing implementations and reports every disagreement (M1 D5, WP-4). Each discrepancy is resolved against the Blue Book text and recorded in §3. This file also lists, per standard, what has no independent implementation and what was searched (§4).

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

## 2. Coverage

What `spacepackets` 0.18.0 can check, by vector file. The harness (`cargo run --manifest-path crosscheck/Cargo.toml`, a step of the `spec` check of `cargo xtask verify` and of the nightly CI, after the harness's own tests) checks only the cases in scope, prints every case it skips with the reason, and fails on any disagreement. A check compares only the values `spacepackets` produces; the expected values it cannot produce are listed under "Out of scope". A key such as `format.day_bits` names the member `day_bits` of the expected object `format`.

A case on which `spacepackets` differs from the vector exactly as a discrepancy of §3 describes is reported as **known**, with the entry's identifier, and does not fail the run. Each check tests the precise condition of its entry. For D1 and D4 it tests both sides: the harness computes the value `spacepackets`' rule gives and the value the Blue Book (D1) or TIME-011 (D4) gives, and reports the known discrepancy only when `spacepackets` produced the first and the vector holds the second. Any other difference is a disagreement. The harness's tests (`crosscheck/src/time.rs`) show that, under each entry, a vector with a wrong expected value still disagrees.

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
| `time/cuc` (`cuc_to_ns`, `cuc_from_ns`) | `subsec_nanos`: `seconds`, `nanoseconds`. `fractional_part_from_subsec_ns` with `CucTime::new_generic`: `coarse`, `fine_hex`; a sub-second part of 1 s and a value beyond the coarse octets must be rejected (`OutOfRange`) | 4–10 fine octets. Known: D1, D4 |
| `time/cds` | `get_dyn_time_provider_from_bytes`: `code`, `pfield_hex`, `format.epoch`, `format.day_bits`, `format.subms`, `value.day`, `value.ms_of_day`, `value.subms`, `consumed`; implicit P-field as for CUC. `CdsTime`: `output_hex`, built as a 24-bit-day value and converted with `TryFrom` for a 16-bit day segment, which must reject a day count above 16 bits (`OutOfRange`). A short input must be rejected (`Incomplete`) | `format.pfield_tail_hex`, `anomalies`. The agency epoch. Range errors: `spacepackets` checks neither the millisecond of day nor the sub-millisecond segment (TIME-021; §3). Known: D2 |
| `time/ascii` | `ascii_format`: `generate_time_code_a` or `generate_time_code_b` (the `_terminated` form when `z`), with the value built as a `chrono` date and time, and second 60 as `chrono`'s leap second. `text` must equal the vector's text with its fraction written at three digits, truncated or filled with zeros: 301.0 §3.5.1.1–2 leave the number of fraction digits to the user, and `spacepackets`' patterns fix it at three | `ascii_parse`, `ascii_subset`: no parser. The fraction digits beyond the third |
| `time/ccs`, `time/convert`, `epp/*` | — | No implementation (§4) |

**Result, 2026-10-20:** 86 agree, 118 skipped, 22 known, 0 disagreements over the twelve checked files.

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
| `time/cuc` | 11 | 9 | 13 (D1 ×2, D3 ×7, D4 ×4) | 0 |
| `time/cds` | 11 | 9 | 2 (D2) | 0 |
| `time/ascii` | 4 | 60 | 0 | 0 |

The time code skips: `spp-tsh-007` (CCS), `-010` (Level 2), `-012` (ancillary field), `-013` and `-018` (5 coarse octets), `-014` (Secondary Header Flag rule), `-015` (CDS range); `time-pf-009` (empty input), `-013` and `-014` (reserved ID in lenient mode), `-015`…`-020`, `-021-enc`, and `-021-dec` (agency-defined code), `-022` (extension octets); `time-cuc-002`, `-010-enc`, and `-010-dec` (Level 2), `-007-enc`, `-017`, `-019`, and `-026` (beyond 4 coarse or 3 fine octets), `-008-enc` and `-009-enc` (P-field octet 2); `time-cds-003`, `-014-enc`, and `-014-dec` (agency epoch), `-007`…`-010`, `-015`, and `-017` (CDS range); the 60 ASCII parse and subset cases.

The harness was confirmed to fail on a corrupted vector, in two runs. 2026-10-19: an APID changed in `spp-dec-001` and an octet changed in `spp-enc-001`. 2026-10-20: the day of `time-cds-001`, the expected nanoseconds of `time-cuc-015` (a D1 case, which shows that a known discrepancy does not hide a wrong vector), the day of year in the text of `time-ascii-030`, and one octet of `time_code_hex` in `spp-tsh-001`. Each was reported as a disagreement, with a failing exit status, and the vectors were restored.

## 3. Discrepancies

No discrepancy is resolved against the vectors or Aldis. Four are known differences in `spacepackets`, each resolved against the Blue Book text; the harness reports them under their identifiers (§2).

**D1. CUC fine time to nanoseconds** (`time-cuc-015`, `-016`). `spacepackets` divides the fine time counter of n octets by 2^(8n) − 1, deliberately (`fractional_res_to_div`, `convert_fractional_part_to_ns`). 301.0 §3.2.1 defines the fine time octets as the elapsed binary fraction of the basic time unit, each octet the state of 8 consecutive bits of a binary counter that rolls over at a modulo of 256, cascaded with the coarse octets. A fraction of n octets therefore counts units of 2^(−8n) s, and the divisor is 2^(8n), as TIME-011 specifies. The two readings differ by one part in 2^(8n) − 1; at the largest counter, `spacepackets` reports a sub-second part of a full 1 000 000 000 ns (`time-cuc-016`). **Resolved in favour of Aldis**; the vectors stand.

**D2. CDS sub-millisecond code `11`** (`time-cds-004`, `-005`, `spp-tsh-017`). 301.0 §3.3.2 marks code `11` of P-field bits 6–7 as reserved for future use and gives it no segment length. `spacepackets` decodes it as a code without a sub-millisecond segment (`calc_stamp_len` adds no octets for `Reserved`; the 24-bit-day reader also drops the code from the P-field it reports). That assumes a length the Blue Book does not define. Aldis rejects the code with `ReservedValue` in both modes, because it sets the length of the T-field (TIME-020, OI-15). **Resolved in favour of Aldis.**

**D3. P-field extension flag** (`time-cuc-003`, `-004`, `-005`, `-007-dec`, `-008-dec`, `-009-dec`, `-028`; `time-pf-003`…`-007`; `spp-tsh-003`). 301.0 §3.1.1 makes P-field bit 0 the extension flag: `1` means that another P-field octet follows. For CUC, §3.2.2 defines that octet 2 (additional coarse and fine octets, mission-defined bits). `CucTime::from_bytes` and the CDS readers of `spacepackets` take the T-field layout from octet 1 alone and do not read the flag, so they take octet 2 as the first T-field octet and report a one-octet P-field. Its documentation states the one-octet limit (`MAX_CUC_LEN_SMALL_PREAMBLE`), but the readers do not reject an extended P-field. Aldis reads the extension chain (TIME-001, TIME-005, TIME-010). **Resolved in favour of Aldis.**

**D4. Nanoseconds to CUC fine time** (`time-cuc-020`, `-022`, `-023`, `-025`). This direction does not use the divisor of D1: `fractional_part_from_subsec_ns` computes ns × 2^(8n) / 10^9, the binary fraction of 301.0 §3.2.1, and rounds it down. TIME-011 rounds it up, so that the round trip through nanoseconds is exact for 0–3 fine octets. A fraction that rounds up to 2^(8n) carries into the coarse time (`time-cuc-022`), and a carry beyond the coarse octets is `OutOfRange` (`time-cuc-023`, where `spacepackets` returns 255 s and fine time `FF`). 301.0 defines the fine time as a binary fraction but gives no conversion from decimal seconds, so both roundings conform; the vectors encode the TIME-011 rule. **No change.** An exact fraction (`time-cuc-021`), a sub-second part of 1 s (`-024`), and a coarse overflow (`-027`) agree.

The version of this section of 2026-10-19 expected D1 in `cuc_from_ns` as well, and every nanosecond case to be excluded. The check shows that `cuc_from_ns` differs only in the rounding (D4), so the harness checks both directions instead.

Observed in `spacepackets`, not discrepancies in a checked value:

- `CcsdsPacketCreator` computes the Packet Data Length as `len − 1` without checking for an empty data field. That subtraction underflows: it panics in a debug build and wraps in a release build. 133.0 §4.1.4.1.2 requires at least one octet of Packet Data Field, which Aldis enforces (`InvalidField`, `spp-enc-004`). The harness never passes that input.
- Its CDS readers and constructors check no range: a millisecond of day of 86 401 000 or more, a microsecond segment of 1000 or more, and a picosecond segment of 10^9 or more are accepted. Aldis rejects them in both modes (TIME-021). The cases are skipped as a rule `spacepackets` does not have.

## 4. Standards with no independent implementation

The features `spacepackets` does not cover (§2), searched 2026-10-20. Search covered crates.io (the `ccsds` query and the crates it lists), PyPI, GitHub, and agency and mission tools. A candidate qualifies when it is open source under a license that allows running it as a test oracle, shares no code or authors with Aldis, and implements the feature as 301.0 or 133.1 define it. Candidates were read at source level unless the entry says otherwise.

Every candidate below is independent of Aldis. None is run yet: each that qualifies needs a JVM, Python, or a Prolog runtime in `crosscheck/`, or is a single-maintainer beta crate. Adding one is a maintainer decision, against VER-040's "where one is available". Until then the vectors of these features stay `hand-derived`, with the derivation written down (D5).

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
| `ccsdspy` (NASA), `minspp` | BSD-3-Clause, GPL-3.0 | Sequence warnings and an assembler, not read at source level | Not verified |

No sender-side segmentation was found: libraries expose the Sequence Flags but do not split an SDU.

### 4.7 Not available

ESA SCOS-2000, EGOS, and libgcs are not published as open source. OpenC3 COSMOS is not under an OSI license and has only day, millisecond, and microsecond helpers. `satrs` and `tmtccmd` share authors with `spacepackets`. Not examined: NASA cFE `CFE_TIME`, AMMOS MMTC, SpacePy, astropy, libCSP, and Go or JavaScript packages.
