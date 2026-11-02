# Independent vector models

The expected values of a `hand-derived` vector file come from an **independent model** of the Blue Book clauses, written before the code it checks and sharing nothing with it. When that model is a program, it is committed here and named in the file's `provenance_note` (TV-023, [docs/07-test-vectors.md](../../docs/07-test-vectors.md) §4).

| Model | Vector files | Covers |
|---|---|---|
| [`aos_fhec.py`](aos_fhec.py) | `aos/fhec_{encode,correct_1_symbol,correct_2_symbols,miscorrection,uncorrectable}.json` | AOS Frame Header Error Control, 732.0-B-5 EC1 §4.1.2.6.5 (AOS-013, AOS-014): GF(2^4) by carry-less multiplication, the check symbols by GF(2) linear algebra on the syndromes, and decoding by a table over every pattern of at most two symbol errors |
| [`aos_frame.py`](aos_frame.py) | `aos/{minimal,fhec_frame,insert_zone,security_layout,mpdu_pointers,bpdu_pointers,oid,issue4_compat,scid_10bit,config,invalid}.json` | AOS Transfer Frame in both editions, 732.0-B-5 EC1 §4.1, §4.3.7, §5, §6.3, Annex D, and 732.0-B-4 §4.1.2, §4.1.4.2 (AOS-001…AOS-019, GEN-062): frames assembled and taken apart as bit strings from the figure widths, the FECF by polynomial division over the corrected header, the OID data by the Annex D recurrence, the FHEC by `aos_fhec.py`, loaded from its path |
| [`tm_frame.py`](tm_frame.py) | `tm/*.json` | TM Transfer Frame, 132.0-B-3 §4.1, §5, §6.3, Annex D (TM-001…TM-011): frames assembled and taken apart as bit strings from the figure widths, the FECF by polynomial division of integers, the OID data by the Annex D recurrence |
| [`core_ocf.py`](core_ocf.py) | `core/ocf.json` | Operational Control Field classification, 132.0-B-3 / 732.0-B-5 §4.1.5.4–5 and 732.1-B-3 §4.1.5.2.1–2 (CMN-010, CMN-011), with the CLCW layout of 232.0-B-4 §4.2.1 for one input |
| [`tc_frame.py`](tc_frame.py) | `tc/*.json` | TC Transfer Frame, CLCW, Frame Delimiting and Fill Removal, and the Frame Validation Checks, 232.0-B-4 Cor. 1 §4.1, §4.2.1, §4.4.9, §5, §6.3, §6.6 (TC-001…TC-009, TC-020…TC-023, TC-030, TC-031): the header as one 40-bit integer, the CRC bit by bit from G(X), the check order and reason codes of tc-frame.md §4.1 |
| [`cop1.py`](cop1.py) | `cop1/scenarios/*.json` | FOP-1 and FARM-1, 232.1-B-2 Cor. 1 §4–§7 and Tables 5-1 and 6-1 (COP-001, COP-002, COP-005, COP-010…COP-017, COP-030…COP-036), with the TC frame and CLCW of 232.0-B-4 §4.1, §4.2.1: both tables transcribed as data, the engines as pure steps over plain variables, and the setup of each cell found by breadth-first search over the inputs, so that every cell is reached by a sequence the model checks; written blind (no implementation under test read; for the layout change only, also the test infrastructure `crates/aldis-testkit`) |
| [`uslp_frame.py`](uslp_frame.py) | `uslp/*.json` | USLP Transfer Frame, truncated frame, OID frame, Frame Delimiting and Fill Data Removal, and the USLP Frame Validation Checks, 732.1-B-3 §4.1, §4.3.11.2–3, §5, §6.3, Annex B, Annex D, Annex H (USLP-001…USLP-016): headers as bit strings from the widths of Figures 4-2 and D-2, the FECF as the Annex B remainder by GF(2) long division checked by the B1.2.1 syndrome, the OID data from the Galois form of Figure H-2, the UPIDs read from the SANA snapshot, the check order and reason codes of uslp-frame.md §3.1 |
| [`link_send.py`](link_send.py) | `link/send/*.json` | The sending-side link engines, 132.0-B-3, 732.0-B-5 EC1, 732.1-B-3, 232.0-B-4 Cor. 1, 133.0-B-2, 133.1-B-3 and docs/04-engines.md §2, §4 (LNK-003, LNK-010…LNK-015, LNK-020, LNK-021, LNK-030…LNK-036, LNK-040…LNK-042, LNK-076): PacketGenerator, AccessGenerator, BitstreamGenerator, ChannelMux with its policies, and MapSegmenter as pure steps over plain variables, the OID pattern in the Fibonacci form checked against the Galois form and Annex D, frames assembled from the figure widths; written blind (no implementation read) |
| [`link_recv.py`](link_recv.py) | `link/recv/*.json` | The receiving-side link engines, 04-engines.md §3 (LNK-050…054, LNK-060…064, LNK-070…072, LNK-077, LNK-079) with 132.0-B-3 §4.1, §4.3; 732.0-B-5 §4.1, §4.3; 732.1-B-3 §4.1, §4.3; 232.0-B-4 §4.1, §4.4; 133.0-B-2 §4.1.3; 133.1-B-3 §4.1.2: the FrameReceiver, PacketExtractor, and MapReassembler as pure steps over plain variables, frame builders for TM, AOS, USLP, and TC written from the figures, the FECF by bitwise division, the OID data by the Annex D recurrence; written blind (no implementation read, none existed); refuses (`Unspecified`) every case the documents leave open |

## Running them

From the repository root, with Python 3 and no packages beyond the standard library:

```sh
python3 -I test-vectors/models/aos_fhec.py --check test-vectors/aos/fhec_*.json
python3 -I test-vectors/models/aos_frame.py --check test-vectors/aos/{minimal,fhec_frame,insert_zone,security_layout,mpdu_pointers,bpdu_pointers,oid,issue4_compat,scid_10bit,config,invalid}.json
python3 -I test-vectors/models/core_ocf.py --check test-vectors/core/ocf.json
python3 -I test-vectors/models/tm_frame.py --check test-vectors/tm/*.json
python3 -I test-vectors/models/tc_frame.py --check test-vectors/tc/*.json
python3 -I test-vectors/models/uslp_frame.py --check test-vectors/uslp/*.json
python3 -I test-vectors/models/cop1.py --check test-vectors/cop1/scenarios/*.json
python3 -I test-vectors/models/link_send.py --check test-vectors/link/send/*.json
python3 -I test-vectors/models/link_recv.py --check test-vectors/link/recv/*.json
```

`--check` recomputes the expected outcome of every live case from its operation and input and compares it with the file, and compares the cases the model's generator writes with the file's; it exits 1 on any difference. `cargo xtask vectors validate` (part of `cargo xtask ci` and the verify check `spec`) runs every model on the files whose notes name it, and fails on a named model that is missing or a model that no file names. `--write` regenerates a file from the model (`cop1.py --write [dir]` regenerates all of `test-vectors/cop1/`, and `cargo xtask vectors generated` fails when the committed files differ from it; `cop1.py --audit` searches the reachable states for an input that reaches a "Not applicable" cell), cases and provenance note; it records how the inputs were chosen. Use it only for a file that is not released yet: a released file changes only by new cases and `superseded` marks (TV-021).

## Rules

- **Independent.** A model is written from the Blue Book, never from the Rust or C++ code, and never changed to agree with them. Its method should differ from the implementation's (the FHEC model uses no division by g(x) and no algebraic decoder), so that a shared mistake is unlikely. When a model and an implementation disagree, the Blue Book decides which is wrong.
- **No Blue Book text.** A model holds parameters and code, with the clause each comes from; never copy text from a Blue Book into it.
- **Standard library only**, runnable with `python3 -I` (no site packages, no import from the current directory).
- A new model gets a row in the table above, and its vector file names it in `provenance_note`.

## Models not kept

The M1 vectors (`time/`, `spp/`, `epp/`) were also computed by independent Python models, written before the Rust codecs, but those programs lived only in scratch directories and were not kept; they cannot be recovered. Their provenance notes stand as written, the files are released (`v0.2.0-m1`) and immutable (TV-021), and their agreement with the Blue Book also rests on the M1 cross-checks against `spacepackets` ([cross-checks.md](../../docs/verification/cross-checks.md)) and the second review of the hand-derived vectors ([m1-vectors.md](../../docs/verification/reviews/m1-vectors.md)).
