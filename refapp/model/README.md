# Blind model of `a1-nominal`

`a1_nominal.py` regenerates the `packet_out` records of the `a1-nominal` golden trace (A1 D5(c), TV-023). It was written blind, from `docs/reference-application.md` and the Blue Books 133.0-B-2, 301.0-B-4, and 132.0-B-3 (the CRC-16 of CMN-001), without sight of the refapp crates. It also reads `refapp/scenario/scenario.json` as scenario input data; that file is data, not implementation, and D5(c) accepts it as input. Python 3, standard library only; no CI step.

**`a1_nominal.py` is valid for `a1-nominal` only.** The model applies each delivery at the next 125 ms instant instead of at its arrival instant (x.010 s), which is exact only because no nominal delivery outputs a packet, and it has no rejected-command path. It is not meant to be widened to other cases.

    python3 refapp/model/a1_nominal.py                           # print the records
    python3 refapp/model/a1_nominal.py --compare <golden.jsonl>  # every mismatch, by record

The golden copy to compare against is written locally, for example with
`git show origin/a1/wp-5-golden:refapp/golden/a1-nominal.jsonl > refapp/model/.golden-a1-nominal.jsonl`;
`.gitignore` keeps `.golden-*.jsonl` out of git, so the copy stays local.

The result is recorded in `docs/verification/cross-checks.md` §7.

## `a1_cases.py`: the four A1 cases

Maintainer decision (2026-11-07): D5(c) extended to every A1 case. `a1_cases.py` models `a1-nominal`, `a1-immediate`, `a1-rejects`, and `a1-downlink-faults`. It was written the same way (blind: `docs/reference-application.md`, `docs/03-api-model.md` §9 and API-019, `docs/standards/space-packet.md` §5.1 and §5.3-5.4, the Blue Books, and `refapp/scenario/scenario.json`; the golden traces only as the comparison target). It steps at the true arrival instant of every delivery (release + 10 000 µs, §6.1, plus the fault delay, §6.3, in the harness order of §6.2), so it has none of the simplification of `a1_nominal.py`. It models the command rejections of §3.3.2, immediate execution, the downlink faults, and the ground node's strict packet decoding (Incomplete, version, SPP-004 idle check, trailing octets), sequence tracking, and dump reassembly.

It regenerates the records of kinds `packet_out` (both nodes), `packet_in` (ground), and `app_event` (both nodes), in trace order.

**Not covered:** `fault`, `unit_sent`, and `unit_delivered` records; the reassembler `Overflow` path; APP-131 (the flight node's `packet_rejected`, and the flight handling of a malformed uplink unit, which `flight_deliver` assumes well formed); uplink-unit faults; frame levels (A2 on); and the error code of an SDU that does not parse as §3.3.4 (APP-130), for which the spec names none (the model emits `packet_rejected` with a reading of its own, unexercised by any case). Two orderings the spec does not settle are chosen and not tested by any case: ground deliveries before ops outputs within a step, and the queue-full check before the due commands leave the queue.

    python3 refapp/model/a1_cases.py <case>                               # print the records
    python3 refapp/model/a1_cases.py <case> --compare <golden.jsonl>      # every difference
    python3 refapp/model/a1_cases.py --all --golden-dir refapp/golden     # all four cases

`a1_nominal.py` stays as the first model (the A1 D5(c) result); `a1_cases.py` also reproduces `a1-nominal`.
