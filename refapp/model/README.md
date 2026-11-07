# Blind model of `a1-nominal`

`a1_nominal.py` regenerates the `packet_out` records of the `a1-nominal` golden trace (A1 D5(c), TV-023). It was written blind, from `docs/reference-application.md` and the Blue Books 133.0-B-2, 301.0-B-4, and 132.0-B-3 (the CRC-16 of CMN-001), without sight of the refapp crates. It also reads `refapp/scenario/scenario.json` as scenario input data; that file is data, not implementation, and D5(c) accepts it as input. Python 3, standard library only; no CI step.

**Valid for `a1-nominal` only.** The model applies each delivery at the next 125 ms instant instead of at its arrival instant (x.010 s), which is exact only because no nominal delivery outputs a packet, and it has no rejected-command path. It is not meant to be widened to other cases.

    python3 refapp/model/a1_nominal.py                           # print the records
    python3 refapp/model/a1_nominal.py --compare <golden.jsonl>  # every mismatch, by record

The golden copy to compare against is written locally, for example with
`git show origin/a1/wp-5-golden:refapp/golden/a1-nominal.jsonl > refapp/model/.golden-a1-nominal.jsonl`;
`.gitignore` keeps `.golden-*.jsonl` out of git, so the copy stays local.

The result is recorded in `docs/verification/cross-checks.md` §7.
