"""Judges a run against YAMCS by the pass criteria of docs/reference-application.md section 11.5
(APP-164, APP-165), from the driver's run log, the ops script's log, and the golden trace of
`a3-nominal` (AINT WP-4). It prints the observed facts, one line each, and a verdict per criterion;
it decides nothing the facts do not: a person reads the output (the records of plan D9 are by inspection).

Usage: python3 -I judge.py <driver-run-log.jsonl> <ops-log.jsonl> <golden-a3-nominal.jsonl> [yamcs.log]
  yamcs.log is the output of `podman logs aldis-yamcs` for the run (optional): its warnings are listed.
Standard library only.
"""

import collections
import json
import sys


def load(path):
    with open(path, encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


def clcw(word):
    """The CLCW fields of 232.1 section 4.2 / 4.2.1, from the 32-bit word."""
    return {
        "version": (word >> 29) & 3,
        "cop": (word >> 24) & 3,
        "lockout": (word >> 13) & 1,
        "wait": (word >> 12) & 1,
        "retransmit": (word >> 11) & 1,
        "farm_b": (word >> 9) & 3,
        "nr": word & 0xFF,
    }


def fmt(t_us):
    return f"{t_us / 1e6:.3f}s"


def main():
    if len(sys.argv) < 4:
        print(__doc__, file=sys.stderr)
        return 2
    run, ops, golden = load(sys.argv[1]), load(sys.argv[2]), load(sys.argv[3])
    ylog = open(sys.argv[4], encoding="utf-8").read().splitlines() if len(sys.argv) > 4 else None
    ok = True

    def fact(text):
        print(" ", text)

    def verdict(name, passed):
        nonlocal ok
        ok &= passed
        print(f"=> {name}: {'met' if passed else 'NOT met'}")

    header = run[0]
    print("run:", {k: v for k, v in header.items() if k != "loss_script"})
    print("loss script:", header["loss_script"])
    up = [r for r in run if r["kind"] == "datagram" and r["dir"] == "up"]
    down = [r for r in run if r["kind"] == "datagram" and r["dir"] == "down" and r["event"] == "sent"]
    faults = [r for r in run if r["kind"] == "fault"]
    flight = [r for r in run if r["kind"] == "app_event" and r["node"] == "flight"]

    print("--- uplink units, in order (index, instant, N(S), type, MAP/flags, packets, event)")
    seen_ns = collections.Counter()
    for r in up:
        b = bytes.fromhex(r["hex"])
        ctrl = (b[0] >> 4) & 3  # bypass flag, control command flag
        ns = b[4]
        seg = b[5]
        pk = []
        i = 6
        while i + 6 <= len(b) - 2:
            ln = int.from_bytes(b[i + 4:i + 6], "big") + 7
            pk.append((f"{((b[i] & 7) << 8 | b[i + 1]):#05x}", int.from_bytes(b[i + 2:i + 4], "big") & 0x3FFF, b[i + 6] if ln > 6 else None))
            i += ln
        seen_ns[(ns, r["hex"])] += 1
        print(f"  {r['index']:2d} {fmt(r['t_us'])} wall {fmt(r['wall_us'])} {r['event']:8s} N(S)={ns} ctrl={ctrl} seg={seg:#04x} len={len(b)} pk={pk}")
    for f in faults:
        print(f"  fault {f['dir']} {f['index']} at {fmt(f['t_us'])}: {f['fault']}")

    gold_fields = {}
    for e in golden:
        if e["kind"] == "unit_sent" and e["dir"] == "up":
            b = bytes.fromhex(e["hex"])
            gold_fields[b[5:-2]] = b[4]
    same = [r["index"] for r in up if bytes.fromhex(r["hex"])[5:-2] in gold_fields]
    print(f"--- uplink frames whose Segment Header and data field equal a frame of the golden trace: units {same}")
    print("--- flight node events")
    for e in flight:
        print(f"  {fmt(e['t_us'])} {e['name']} {e['args']}")

    print("--- CLCWs (changes only)")
    last = None
    clcws = []
    for r in down:
        b = bytes.fromhex(r["hex"])
        w = int.from_bytes(b[217:221], "big")
        clcws.append((r["index"], r["t_us"], w))
        if w != last:
            c = clcw(w)
            print(f"  down {r['index']:3d} {fmt(r['t_us'])} {w:#010x} N(R)={c['nr']} retransmit={c['retransmit']} lockout={c['lockout']} wait={c['wait']} farm_b={c['farm_b']}")
            last = w
    dropped_down = {f["index"] for f in faults if f["dir"] == "down"}

    # What the ground system sees: the CLCWs not dropped.
    seen = [(i, t, clcw(w)) for i, t, w in clcws if i not in dropped_down]
    flags = [(i, c) for i, t, c in seen if c["lockout"]]
    print("--- criteria")
    print("APP-164 (a3-nominal) / APP-165 (lossy link)")
    verdict("no Lockout in any CLCW", not flags)

    # N(R) between the previous N(R) and V(S), modulo 256. V(S) is taken from the frames the flight node
    # received, which is stricter than APP-164 (the sender's own V(S) may be ahead of them).
    prev = 0
    bad = []
    ns_by_time = sorted((r["t_us"], bytes.fromhex(r["hex"])[4]) for r in up if r["event"] == "received")
    for i, t, c in seen:
        vs_t = max([n for tt, n in ns_by_time if tt <= t] + [-1]) + 1
        if not ((c["nr"] - prev) % 256 <= (vs_t - prev) % 256):
            bad.append((i, c["nr"], prev, vs_t))
        prev = c["nr"]
    fact(f"CLCW N(R) vs V(S): {len(bad)} CLCWs outside [previous N(R), V(S)] (V(S) taken from the frames the flight node received)")
    verdict("N(R) advances within [previous N(R), V(S)] modulo 256", not bad)
    final = seen[-1][2]["nr"] if seen else None
    accepted = sorted({bytes.fromhex(r["hex"])[4] for r in up if r["event"] == "received"})
    fact(f"last CLCW N(R) = {final}; highest N(S) received = {accepted[-1] if accepted else None}")
    verdict("N(R) after the last command equals V(S)", final == (accepted[-1] + 1) % 256 if accepted else False)
    retr = [(i, t) for i, t, c in seen if c["retransmit"]]
    fact(f"CLCWs with Retransmit that the ground system received: {[(i, fmt(t)) for i, t in retr]}")

    # Projection of the commands: the flight node's accepted-command events against the golden's.
    gold = [(e["name"], e["args"]) for e in golden if e["kind"] == "app_event" and e["node"] == "flight"
            and e["name"] in ("cmd_received", "table_received")]
    got = [(e["name"], e["args"]) for e in flight if e["name"] in ("cmd_received", "table_received")]
    fact(f"flight accepted commands (cmd_received, table_received): {len(got)}; a3-nominal: {len(gold)}")
    verdict("the projection of the commands equals a3-nominal's", got == gold)
    if got != gold:
        fact(f"  differs: golden-only {[x for x in gold if x not in got]}, run-only {[x for x in got if x not in gold]}")
        same_but_table = [(n, a if n != "table_received" else a[:1]) for n, a in got] == [(n, a if n != "table_received" else a[:1]) for n, a in gold]
        fact(f"  equal to a3-nominal's except the length of the table: {same_but_table}")
    slow = [r for r in run if r["kind"] == "slots_missed"]
    if slow:
        fact(f"driver slots_missed records (late frames, none skipped): {[(fmt(r['t_us']), r['count']) for r in slow]}")
    gold_pk = collections.Counter(bytes.fromhex(e["hex"]) for e in golden if e["kind"] == "packet_in" and e["node"] == "ground")
    got_pk = collections.Counter(bytes.fromhex(e["hex"]) for e in run if e["kind"] == "packet_out")
    lost_down = sum(1 for f in faults if f["dir"] == "down")
    fact(f"flight packets out: {sum(got_pk.values())}; a3-nominal packets in at the ground: {sum(gold_pk.values())} "
         f"(equal as multisets: {got_pk == gold_pk}); {lost_down} downlink frames dropped by the script")
    if got_pk != gold_pk:
        fact(f"  golden-only APIDs {[hex(((p[0] & 7) << 8) | p[1]) for p in (gold_pk - got_pk).elements()]}, "
             f"run-only {[hex(((p[0] & 7) << 8) | p[1]) for p in (got_pk - gold_pk).elements()]}")

    # YAMCS's own account.
    cmdhist = next((r for r in ops if r["kind"] == "command_history"), None)
    acks = []
    if cmdhist and isinstance(cmdhist["response"], dict):
        for e in cmdhist["response"].get("entry", []):
            attrs = {a["name"]: next((v for k, v in a["value"].items() if k != "type"), None) for a in e["attr"]}
            acks.append((e["commandId"]["commandName"], attrs.get("Acknowledge_Sent_Status"), attrs.get("CommandComplete_Status"),
                         attrs.get("Acknowledge_Sent_Message")))
    for a in acks:
        fact(f"YAMCS command history: {a}")
    verdict("YAMCS reports every command acknowledged (Sent OK, none NOK)", bool(acks) and all(a[1] == "OK" and a[2] in (None, "OK") for a in acks))
    states = collections.Counter(r["status"].get("state") for r in ops if r["kind"] == "cop1_status" and isinstance(r["status"], dict))
    fact(f"COP-1 states seen in the polls: {dict(states)}")
    last_status = next((r["status"] for r in reversed(ops) if r["kind"] == "cop1_status" and isinstance(r["status"], dict)), {})
    fact(f"COP-1 status at the end: {last_status}")
    maxtx = max((r["status"].get("txCount", 0) for r in ops if r["kind"] == "cop1_status" and isinstance(r["status"], dict)), default=0)
    fact(f"largest txCount seen: {maxtx}")
    if ylog is not None:
        lossy_down = bool(dropped_down)
        warn = [line for line in ylog if "[0;33m" in line and "HttpRequestHandler" not in line and "RouteHandler" not in line
                and "Security is disabled" not in line]
        err = [line for line in ylog if "[0;31m" in line]
        # A packet cut by a frame the script dropped is YAMCS's own, correct warning (TM side).
        expected = [line for line in warn if lossy_down and "because of frame loss" in line]
        warn = [line for line in warn if line not in expected]
        for line in warn + err:
            fact("YAMCS log: " + line.strip())
        fact(f"YAMCS log: {len(expected)} warning(s) 'Incomplete packet dropped because of frame loss' (the dropped downlink frames)")
        verdict("no YAMCS warning or error in its log, except those of the dropped downlink frames", not warn and not err)
    seq = [r["status"].get("state") for r in ops if r["kind"] == "cop1_status" and isinstance(r["status"], dict)]
    first_active = seq.index("ACTIVE") if "ACTIVE" in seq else None
    after = [] if first_active is None else [s for s in seq[first_active:] if s in ("UNINITIALIZED", "SUSPENDED")]
    fact(f"first ACTIVE at poll {first_active}; UNINITIALIZED or SUSPENDED after it: {dict(collections.Counter(after))}")
    verdict("no UNINITIALIZED or SUSPENDED state after the first ACTIVE (no alert, no limit reached)",
            first_active is not None and not after)

    # Retransmissions on the wire.
    rep = collections.defaultdict(list)
    for r in up:
        b = bytes.fromhex(r["hex"])
        rep[r["hex"]].append((r["index"], r["t_us"], r["event"]))
    print("--- frames that appear more than once on the uplink (YAMCS retransmissions)")
    for h, v in rep.items():
        if len(v) > 1:
            print(f"  N(S)={bytes.fromhex(h)[4]}: " + ", ".join(f"unit {i} {fmt(t)} {ev}" for i, t, ev in v))
    discards = [(fmt(e["t_us"]), e["args"]) for e in flight if e["name"] == "frame_discarded"]
    fact(f"frame_discarded events of the flight node (layer, FARM-1 event): {discards}")
    print("verdict of this program:", "all facts above hold" if ok else "at least one criterion line says NOT met")
    return 0


if __name__ == "__main__":
    sys.exit(main())
