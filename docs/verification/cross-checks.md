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

What `spacepackets` 0.18.0 can check, by vector file. The harness runs only the cases in scope and lists every case it skips, with the reason.

| Vector files | Checked | Out of scope, and why |
|---|---|---|
| `spp/decode_valid`, `spp/stream`, `peek_length` cases | Primary header fields, packet length, Packet Data Field | — |
| `spp/decode_invalid` | Truncated packets | Errors `spacepackets` does not detect: it does not check the Packet Version Number |
| `spp/encode`, `spp/idle`, `spp/octet_string`, `spp/segment` | The expected octets decode to the expected header fields | The encoder's own rules (idle pattern, Sequence Flags, rejections): `spacepackets` has no idle, Octet String, or segmentation logic |
| `spp/reassemble`, `spp/sequence_tracker`, `spp/split` | — | No reassembler, sequence tracker, or managed secondary header layout |
| `spp/secondary_header_time` | The Time Code Field, where its format is in scope for CUC or CDS below | The split and the ancillary field |
| `time/cuc` | Decode and encode with 1–4 coarse and 0–3 fine octets, a one-octet P-field, and the CCSDS epoch | 5–7 coarse or 4–10 fine octets, P-field extensions (its decoder ignores the extension bit, 301.0 §3.2.2), the agency epoch. `cuc_to_ns`, `cuc_from_ns`: see §3 |
| `time/cds` | Decode and encode with 16- and 24-bit day segments, no, µs, or ps sub-millisecond field, and the CCSDS epoch | The agency epoch; range errors (it does not check the millisecond of day) |
| `time/ascii` | Formatting of codes A and B at the fixed precision `chrono` produces | Parsing, subsets, second 60 |
| `time/pfield`, `time/ccs`, `time/convert`, `epp/*` | — | No implementation (§4) |

## 3. Discrepancies

None recorded yet. One is expected from the evaluation and will be confirmed by the harness:

- **CUC fine time to nanoseconds.** `spacepackets` divides the fine time counter by 2^(8n) − 1, deliberately (`fractional_res_to_div`). 301.0 §3.2.1 defines the fine time octets as the elapsed binary fraction of the second, each octet a stage of a binary counter that rolls over at 256, so the divisor is 2^(8n), as TIME-011 specifies. If confirmed, the resolution is in favour of Aldis, and the `cuc_to_ns` and `cuc_from_ns` cases are excluded from the `spacepackets` check with this entry as the reason.

## 4. Standards with no independent implementation

To be completed in WP-4 with what was searched for each: P-field extensions, CCS, ASCII parsing, CDS/CCS/ASCII conversions, and EPP. Their vectors stay `hand-derived` with the derivation written down (D5).
