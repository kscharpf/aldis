# Review: hand-derived M1 vectors (EPP, CCS, ASCII)

> Dated review: identifiers reflect the code at the time.

The second review the [M1 plan](../../milestones/m1-packets-time.md) §5 asks for: the hand-derived vectors that no independent implementation checks ([cross-checks.md](../cross-checks.md) §4, OI-32) are read by a reviewer other than the one who derived them, so that a misreading of the Blue Book shared by the vectors and the code can surface.

## Round 1

- **Date:** 2026-10-20
- **Reviewer:** Gemini, an AI model, which did not take part in deriving the vectors or writing the codecs. These were derived and written in Claude Code sessions. The maintainer commissioned the review and accepted its result.
- **Baseline:** `main` after PR #50
- **Scope:** the 8 EPP vector files (`test-vectors/epp/header_1.json`, `header_2.json`, `header_4.json`, `header_8.json`, `idle.json`, `auto_size_boundaries.json`, `config.json`, `invalid.json`), `test-vectors/time/ccs.json`, and `test-vectors/time/ascii.json`, with their provenance notes
- **Reviewed against:** CCSDS 133.1-B-3, CCSDS 301.0-B-4 §3.4, §3.5, and Annex A, [encapsulation-packet.md](../../standards/encapsulation-packet.md), [time-codes.md](../../standards/time-codes.md), and the open issues the vectors pin (OI-15, OI-16, OI-17, OI-18)
- **Result:** approved; no protocol or arithmetic error found

### Findings on the protocol

| Area | What the reviewer confirmed |
|---|---|
| EPP (133.1-B-3) | Total-length semantics; the header-size thresholds at 255/256 and 65 535/65 536 octets; EPI `110` only with an extension field (OI-17, EPP-012); the empty data field (OI-16, EPP-005) |
| CCS (301.0-B-4 §3.4, Annex A) | BCD segment checks; Gregorian leap years (rules of 4, 100, and 400); second 60; the DOY high-nibble anomaly (TIME-034); resolution code `111` rejected in both strict and lenient modes (OI-15, OI-18) |
| ASCII (301.0-B-4 §3.5, Annex A) | The worked examples for codes A and B; 0–12 fraction digits with optional `Z`; all five subset rules of §3.5.1.3 a–e, including the separators kept by left and right truncation and the handling around `T` |

The reviewer also ran `cargo xtask vectors validate` and `cargo test --workspace --all-features`; both pass.

### Observations and resolutions

Neither file had been released (the only release is `v0.1.0-m0`), so TV-021 does not yet apply and both were corrected in place on `m1/exit`. No case changed.

| # | Observation | Resolution |
|---|---|---|
| 1 | The `provenance_note` of `time/ccs.json` ended with "CUC fine time is given as its raw octets.", copied from `time/cuc.json`. CCS has no CUC fine time; its sub-second digits are decimal | Sentence removed |
| 2 | In `epp/invalid.json`, cases `epp-inv-022`…`026` sat between `014` and `015` | Cases sorted by ID; a script confirmed the set of cases, and each case's content, is unchanged |
