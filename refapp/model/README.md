# Blind model of `a1-nominal`

`a1_nominal.py` regenerates the `packet_out` records of the `a1-nominal` golden trace (A1 D5(c), TV-023). It was written blind, from `docs/reference-application.md`, `refapp/scenario/scenario.json`, and the Blue Books 133.0-B-2, 301.0-B-4, and 132.0-B-3 (the CRC-16 of CMN-001), without sight of the refapp crates. Python 3, standard library only; no CI step.

    python3 refapp/model/a1_nominal.py                        # print the records
    python3 refapp/model/a1_nominal.py --compare <golden.jsonl>  # every mismatch, by record

The result is recorded in `docs/verification/cross-checks.md` §7.
