# Independent cross-checks (VER-040)

The `crosscheck/` workspace runs the vector files under `test-vectors/` through independent, existing implementations and reports every disagreement (M1 D5, WP-4). Each discrepancy is resolved against the Blue Book text and recorded in §3. This file also lists, per standard, what has no independent implementation and what was searched (§4).

## 1. Implementations

### `spacepackets` 0.18.0 (Rust, crates.io)

Evaluated 2026-10-19 for SPP, CUC, and CDS.

| Check | Result |
|---|---|
| Independence | Developed at the University of Stuttgart's Institute of Space Systems; shares no code or authors with Aldis |
| License | Apache-2.0 (with a `NOTICE` file, which matters only on redistribution; `crosscheck/` is never shipped). All 31 crates in its dependency tree carry licenses on the `deny.toml` allow list |
| MSRV | 1.85, below the pinned 1.99.1 (RS-001) |
| `cargo deny check` (cargo-deny 0.20.2, root policy) | licenses, bans, sources: ok (bans warns about two `syn` major versions, which the policy allows). Advisories: fails on RUSTSEC-2024-0436 |
| Version | Pinned with `=0.18.0` so that a new release cannot change the results unnoticed. An upgrade re-runs this evaluation |

**Exception RUSTSEC-2024-0436** (maintainer decision 2026-10-19). The advisory reports that `paste` 1.0.15, a required dependency of `spacepackets`, is unmaintained. It reports no vulnerability. `paste` is a build-time macro helper, and `crosscheck/` is a dev-only workspace that is never shipped. The exception therefore lives in `crosscheck/deny.toml` only, which repeats the root policy and adds this one `ignore`. The root `deny.toml`, which governs the shipped crates, is unchanged. The CI `deny` job checks `crosscheck/` with its own file.

## 2. Coverage

What `spacepackets` 0.18.0 can check, by vector file. The harness (`cargo run --manifest-path crosscheck/Cargo.toml`, a step of the CI `spec` job) checks only the cases in scope, prints every case it skips with the reason, and fails on any disagreement. A check compares only the values `spacepackets` produces; the expected values it cannot produce are listed under "Out of scope".

| Vector files | Checked | Out of scope, and why |
|---|---|---|
| `spp/decode_valid`, `spp/decode_invalid` (`decode`) | `CcsdsPacketReader`: `packet_type`, `secondary_header_flag`, `apid`, `sequence_flags`, `sequence_count`, `packet_len`, `consumed`, `data_len`, `data_hex`. A truncated packet must be rejected as too short (`Incomplete`) | `idle`, `packet_name`, `anomalies`. `UnsupportedVersion`: it does not check the Packet Version Number. The idle rule of SPP-004 |
| `peek_length` cases | `SpHeader::from_be_bytes`: `len`; fewer than 6 octets rejected | `UnsupportedVersion` |
| `spp/stream` | Packet by packet: `apid`, `sequence_count`, `consumed` of each, `stop_error` (none or `Incomplete`), `offset` | `anomalies`; streams that stop at an error it does not detect |
| `spp/encode`, `spp/octet_string` | `CcsdsPacketCreator` builds the packet from the parameters; `output_hex` must match. A data field above 65 536 octets must be rejected | `next_count`. The rejection rules it does not have: empty data field, idle APID, `max_packet_len`. Its creator underflows on an empty data field (§3), so that input is never passed to it |
| `spp/idle` | The expected octets decode to APID 2047, Secondary Header Flag 0, Sequence Flags `11`, the given count and total length (133.0 §4.1.3.3.4.4, §4.1.4.3.4) | The idle pattern; rejections (no Idle Packet builder) |
| `spp/segment` | Each expected packet decodes to the given type, APID, Secondary Header Flag, and count + index modulo 2^14, and its length field covers exactly its octets | Sequence Flags and the split of the SDU (no segmentation logic); `next_count`; rejections |
| `spp/reassemble`, `spp/sequence_tracker`, `spp/split` | — | No reassembler, sequence tracker, or managed secondary header layout |

**Result, 2026-10-19:** 45 agree, 21 skipped, 0 disagreements over the seven checked `spp/` files. The harness was confirmed to fail on a corrupted vector: an APID changed in `spp-dec-001` and an octet changed in `spp-enc-001` were each reported as a disagreement, with a failing exit status.
| `spp/secondary_header_time` | The Time Code Field, where its format is in scope for CUC or CDS below | The split and the ancillary field |
| `time/cuc` | Decode and encode with 1–4 coarse and 0–3 fine octets, a one-octet P-field, and the CCSDS epoch | 5–7 coarse or 4–10 fine octets, P-field extensions (its decoder ignores the extension bit, 301.0 §3.2.2), the agency epoch. `cuc_to_ns`, `cuc_from_ns`: see §3 |
| `time/cds` | Decode and encode with 16- and 24-bit day segments, no, µs, or ps sub-millisecond field, and the CCSDS epoch | The agency epoch; range errors (it does not check the millisecond of day) |
| `time/ascii` | Formatting of codes A and B at the fixed precision `chrono` produces | Parsing, subsets, second 60 |
| `time/pfield`, `time/ccs`, `time/convert`, `epp/*` | — | No implementation (§4) |

## 3. Discrepancies

None between the `spp/` vectors and `spacepackets`.

Observed in `spacepackets`, not exercised by the harness: `CcsdsPacketCreator` computes the Packet Data Length as `len − 1` without checking for an empty data field. That subtraction underflows: it panics in a debug build and wraps in a release build. 133.0 §4.1.4.1.2 requires at least one octet of Packet Data Field, which Aldis enforces (`InvalidField`, `spp-enc-004`).

One discrepancy is expected for the time codes and will be confirmed by their checks:

- **CUC fine time to nanoseconds.** `spacepackets` divides the fine time counter by 2^(8n) − 1, deliberately (`fractional_res_to_div`). 301.0 §3.2.1 defines the fine time octets as the elapsed binary fraction of the second, each octet a stage of a binary counter that rolls over at 256, so the divisor is 2^(8n), as TIME-011 specifies. If confirmed, the resolution is in favour of Aldis, and the `cuc_to_ns` and `cuc_from_ns` cases are excluded from the `spacepackets` check with this entry as the reason.

## 4. Standards with no independent implementation

To be completed in WP-4 with what was searched for each: P-field extensions, CCS, ASCII parsing, CDS/CCS/ASCII conversions, and EPP. Their vectors stay `hand-derived` with the derivation written down (D5).
