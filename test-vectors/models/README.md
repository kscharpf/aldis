# Independent vector models

The expected values of a `hand-derived` vector file come from an **independent model** of the Blue Book clauses, written before the code it checks and sharing nothing with it. When that model is a program, it is committed here and named in the file's `provenance_note` (TV-023, [docs/07-test-vectors.md](../../docs/07-test-vectors.md) §4).

| Model | Vector files | Covers |
|---|---|---|
| [`aos_fhec.py`](aos_fhec.py) | `aos/fhec_{encode,correct_1_symbol,correct_2_symbols,miscorrection,uncorrectable}.json` | AOS Frame Header Error Control, 732.0-B-5 EC1 §4.1.2.6.5 (AOS-013, AOS-014): GF(2^4) by carry-less multiplication, the check symbols by GF(2) linear algebra on the syndromes, and decoding by a table over every pattern of at most two symbol errors |
| [`aos_frame.py`](aos_frame.py) | `aos/{minimal,fhec_frame,insert_zone,security_layout,mpdu_pointers,bpdu_pointers,oid,issue4_compat,scid_10bit,config,invalid}.json` | AOS Transfer Frame in both editions, 732.0-B-5 EC1 §4.1, §4.3.7, §5, §6.3, Annex D, and 732.0-B-4 §4.1.2, §4.1.4.2 (AOS-001…AOS-019, GEN-062): frames assembled and taken apart as bit strings from the figure widths, the FECF by polynomial division over the corrected header, the OID data by the Annex D recurrence, the FHEC by `aos_fhec.py`, loaded from its path |
| [`tm_frame.py`](tm_frame.py) | `tm/*.json` | TM Transfer Frame, 132.0-B-3 §4.1, §5, §6.3, Annex D (TM-001…TM-011): frames assembled and taken apart as bit strings from the figure widths, the FECF by polynomial division of integers, the OID data by the Annex D recurrence |
| [`core_ocf.py`](core_ocf.py) | `core/ocf.json` | Operational Control Field classification, 132.0-B-3 / 732.0-B-5 §4.1.5.4–5 and 732.1-B-3 §4.1.5.2.1–2 (CMN-010, CMN-011), with the CLCW layout of 232.0-B-4 §4.2.1 for one input |
| [`tc_frame.py`](tc_frame.py) | `tc/*.json` | TC Transfer Frame, CLCW, Frame Delimiting and Fill Removal, and the Frame Validation Checks, 232.0-B-4 Cor. 1 §4.1, §4.2.1, §4.4.9, §5, §6.3, §6.6 (TC-001…TC-008, TC-020…TC-023, TC-030, TC-031): the header as one 40-bit integer, the CRC bit by bit from G(X), the check order and reason codes of tc-frame.md §4.1 |

## Running them

From the repository root, with Python 3 and no packages beyond the standard library:

```sh
python3 -I test-vectors/models/aos_fhec.py --check test-vectors/aos/fhec_*.json
python3 -I test-vectors/models/aos_frame.py --check test-vectors/aos/{minimal,fhec_frame,insert_zone,security_layout,mpdu_pointers,bpdu_pointers,oid,issue4_compat,scid_10bit,config,invalid}.json
python3 -I test-vectors/models/core_ocf.py --check test-vectors/core/ocf.json
python3 -I test-vectors/models/tm_frame.py --check test-vectors/tm/*.json
python3 -I test-vectors/models/tc_frame.py --check test-vectors/tc/*.json
```

`--check` recomputes the expected outcome of every live case from its operation and input and compares it with the file, and compares the cases the model's generator writes with the file's; it exits 1 on any difference. `cargo xtask vectors validate` (part of `cargo xtask ci` and the verify check `spec`) runs every model on the files whose notes name it, and fails on a named model that is missing or a model that no file names. `--write` regenerates a file from the model, cases and provenance note; it records how the inputs were chosen. Use it only for a file that is not released yet: a released file changes only by new cases and `superseded` marks (TV-021).

## Rules

- **Independent.** A model is written from the Blue Book, never from the Rust or C++ code, and never changed to agree with them. Its method should differ from the implementation's (the FHEC model uses no division by g(x) and no algebraic decoder), so that a shared mistake is unlikely. When a model and an implementation disagree, the Blue Book decides which is wrong.
- **No Blue Book text.** A model holds parameters and code, with the clause each comes from; never copy text from a Blue Book into it.
- **Standard library only**, runnable with `python3 -I` (no site packages, no import from the current directory).
- A new model gets a row in the table above, and its vector file names it in `provenance_note`.

## Models not kept

The M1 vectors (`time/`, `spp/`, `epp/`) were also computed by independent Python models, written before the Rust codecs, but those programs lived only in scratch directories and were not kept; they cannot be recovered. Their provenance notes stand as written, the files are released (`v0.2.0-m1`) and immutable (TV-021), and their agreement with the Blue Book also rests on the M1 cross-checks against `spacepackets` ([cross-checks.md](../../docs/verification/cross-checks.md)) and the second review of the hand-derived vectors ([m1-vectors.md](../../docs/verification/reviews/m1-vectors.md)).
