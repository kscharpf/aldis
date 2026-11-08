#!/usr/bin/env python3
"""Blind model of the A1 cases (TV-023; maintainer decision 2026-11-07: A1 D5(c) extended to every A1 case): a1-nominal, a1-immediate,
a1-rejects and a1-downlink-faults.

Written from docs/reference-application.md (the scenario spec), docs/03-api-model.md §9
(error codes), docs/standards/space-packet.md §5.1, §5.3-5.4 (strict decode, sequence classes, reassembler),
refapp/scenario/scenario.json (scenario input data) and the CCSDS Blue Books 133.0-B-2,
301.0-B-4 and 132.0-B-3 (CRC-16), without sight of the refapp implementation. The golden
traces are read only as the comparison target.

Unlike a1_nominal.py, this model steps at the true arrival instant of every delivery
(release + one-way delay + fault delay, §6.1, §6.3), in the harness order of §6.2.

It regenerates, in trace order, the records of kinds packet_out (both nodes), packet_in
(ground) and app_event (both nodes). It does not model fault, unit_sent or unit_delivered
records.

Usage:
    python3 a1_cases.py CASE                 # print the records (JSON Lines)
    python3 a1_cases.py CASE --compare FILE  # compare with those records of a golden trace
    python3 a1_cases.py --all --golden-dir DIR   # every case, golden DIR/<case>.jsonl
"""

import argparse
import difflib
import heapq
import json
import sys
from pathlib import Path

SCENARIO = json.loads((Path(__file__).resolve().parent.parent / "scenario" / "scenario.json").read_text())

T0 = SCENARIO["time"]["t0_coarse_s"]  # §5
APID = SCENARIO["apid"]  # §3.1
SEC_US = 1_000_000
SLOT_US = SCENARIO["link"]["frame_slot_us"]  # 125 000, §6.1
DELAY_US = SCENARIO["link"]["one_way_delay_us"]  # §6.1
GEN_END_US = 30 * SEC_US  # §2.1 "while t < 30 s"
DUMP_CHUNK = 128  # §3.3.4
QUEUE_CAP = 8  # §2.3
ERR_INCOMPLETE = 2  # docs/03-api-model.md §9
ERR_LENGTH_MISMATCH = 3
ERR_UNSUPPORTED_VERSION = 4
ERR_OUT_OF_RANGE = 5
ERR_INVALID_FIELD = 6
CASE_IDS = ["a1-nominal", "a1-immediate", "a1-rejects", "a1-downlink-faults"]


class ModelError(Exception):
    """A broken precondition or self-check of the model; main() exits non-zero (not an `assert`:
    `python3 -O` removes those)."""


def require(cond, msg):
    if not cond:
        raise ModelError(msg)


def crc16(data: bytes) -> int:
    # CMN-001; 132.0 §4.1.6.2.2: G(X) = X^16 + X^12 + X^5 + 1, preset all ones, MSB first, no inversion.
    crc = 0xFFFF
    for octet in data:
        crc ^= octet << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0x1021) & 0xFFFF if crc & 0x8000 else (crc << 1) & 0xFFFF
    return crc


def u16(v): return (v & 0xFFFF).to_bytes(2, "big")  # GEN-050, §3.3 big-endian
def u32(v): return (v & 0xFFFFFFFF).to_bytes(4, "big")


def cuc_tfield(t_us: int) -> bytes:
    # §5: T0 + now; 4 coarse + 2 fractional octets (2^-16 s), exact on the 125 000 µs grid.
    coarse = T0 + t_us // SEC_US
    num = (t_us % SEC_US) * 65536
    require(num % SEC_US == 0, f"time {t_us} not exact in 2^-16 s")
    return u32(coarse) + u16(num // SEC_US)


def cuc_to_us(tfield: bytes) -> int:
    coarse = int.from_bytes(tfield[0:4], "big")
    frac = int.from_bytes(tfield[4:6], "big")
    return (coarse - T0) * SEC_US + frac * SEC_US // 65536


PFIELD = bytes([0x1E])  # §3.2


def space_packet(ptype, apid, flags, count, data_field, sec_hdr) -> bytes:
    # 133.0 §4.1.3: PVN 000, type, secondary header flag, APID; flags, count; length - 1.
    w0 = (ptype << 12) | ((1 if sec_hdr else 0) << 11) | (apid & 0x7FF)
    w1 = (flags << 14) | (count % 16384)  # §3.1
    return u16(w0) + u16(w1) + u16(len(data_field) - 1) + data_field


class Counters:
    def __init__(self):
        self.seq = {}

    def next(self, apid):
        c = self.seq.get(apid, 0)  # §3.1 / APP-021: own count per APID, from 0
        self.seq[apid] = c + 1
        return c


def table_octets(length):
    return bytes((3 * i + 1) % 256 for i in range(length))  # §3.3.6


def memory(addr, length):
    return bytes(((a ^ (a >> 8)) % 256) for a in range(addr, addr + length))  # §3.3.4


def jline(obj):
    return json.dumps(obj, separators=(",", ":"))


# --- ground side: command and table packets from ops entries (§3.3.5, §3.3.6, §6.5) -------------

FUNC = {"noop": 0x01, "dump_memory": 0x02, "activate_table": 0x03, "set_payload_mode": 0x04}
ARG_LEN = {0x01: 0, 0x02: 6, 0x03: 3, 0x04: 1}  # §3.3.5 packet length - 14


def build_uplink(ops, gseq):
    """-> list of (release_us, apid, packet octets), in array order."""
    out = []
    for op in ops:
        if op["kind"] == "command":
            call = op["call"]
            fn = call["function"]
            if fn == "unknown":
                code, args = call["code"], b""
            else:
                code = FUNC[fn]
                if fn == "dump_memory":
                    args = u32(call["address"]) + u16(call["length"])
                elif fn == "activate_table":
                    args = bytes([call["table_id"]]) + u16(call["crc"])
                elif fn == "set_payload_mode":
                    args = bytes([call["mode"]])
                else:
                    args = b""
            args += bytes(op.get("extra_octets", 0))  # §6.5
            ex = op["execution"]
            if ex["mode"] == "immediate":
                mode, tag = 0x00, bytes(6)
            else:
                mode, tag = 0x01, cuc_tfield(ex["exec_us"])
            ud = bytes([code, mode]) + tag + args
            seq = gseq.next(APID["commands"])
            out.append((op["at_us"], APID["commands"], space_packet(1, APID["commands"], 0b11, seq, ud, False)))
        elif op["kind"] == "table_upload":
            ud = bytes([op["table_id"]]) + u16(op["length"]) + table_octets(op["length"])
            seq = gseq.next(APID["table_upload"])
            out.append((op["at_us"], APID["table_upload"], space_packet(1, APID["table_upload"], 0b11, seq, ud, False)))
        # fop1_directive: ignored below A3 (APP-019)
    return out


# --- the run -------------------------------------------------------------------------------------


class Run:
    def __init__(self, case):
        self.case = case
        self.end = case["end_us"]
        self.faults = {(f["dir"], f["index"]): f["fault"] for f in case.get("faults", [])}
        require(all(f["dir"] == "down" for f in case.get("faults", [])), "uplink-unit faults are not modelled")
        self.trace = []  # json lines in trace order
        self.heap = []  # instants of interest
        self.seen = set()
        self.pending = []  # deliveries: (arrival, send_t, dirn, index, copy, octets)
        self.units = {"down": 0, "up": 0}
        self.down_units = []  # (apid, count) of each downlink unit, for self-checks
        self.fseq = Counters()
        # flight state (§3.3.1)
        self.executed = self.rejected = 0
        self.payload_on = True  # §2.1
        self.last_table_id = self.last_table_crc = 0
        self.dumps_completed = 0
        self.table = None  # (id, octets)
        self.queue = []  # (exec_us, func, seq, args)
        self.dump = None
        self.payload_n = 0
        # ground state
        self.last_count = {}
        self.asm = None  # dump reassembly: list of user-data chunks in progress, or None

    def at(self, t):
        if t not in self.seen and t < self.end:
            self.seen.add(t)
            heapq.heappush(self.heap, t)

    def rec(self, t, kind, **kw):
        self.trace.append(jline({"t_us": t, "kind": kind, **kw}))

    # -- link (§6.1, §6.3)
    def send(self, t, dirn, octets):
        idx = self.units[dirn]
        self.units[dirn] += 1
        fault = self.faults.get((dirn, idx), {"kind": None})
        arr = t + DELAY_US
        copies = [octets]
        k = fault["kind"]
        if k == "drop":
            copies = []
        elif k == "duplicate":
            copies = [octets, octets]
        elif k == "delay":
            arr += fault["delay_us"]
        elif k == "truncate":
            copies = [octets[: fault["octets"]]]
        elif k == "flip":
            raise NotImplementedError("flip is for frame levels only")
        for c, o in enumerate(copies):
            self.pending.append((arr, t, 0 if dirn == "down" else 1, idx, c, o))
        if copies:
            self.at(arr)
        return idx

    # -- flight outputs
    def tm(self, t, apid, flags, ud, explicit=False):
        sh = (PFIELD if explicit else b"") + cuc_tfield(t)  # §3.2, APP-022
        pkt = space_packet(0, apid, flags, self.fseq.next(apid), sh + ud, True)
        self.rec(t, "packet_out", node="flight", apid=apid, hex=pkt.hex().upper())
        idx = self.send(t, "down", pkt)
        self.down_units.append((apid, int.from_bytes(pkt[2:4], "big") & 0x3FFF))
        require(idx == len(self.down_units) - 1, "downlink unit index out of step")

    def event(self, t, code, p1=0, p2=0, p3=0):  # §3.3.2
        self.tm(t, APID["events"], 0b11, u16(code) + u16(p1) + u16(p2) + u16(p3), True)

    def flight_app_event(self, t, code, name, args):
        self.rec(t, "app_event", node="flight", code=code, name=name, args=args)  # §7.4

    # -- flight: uplink delivery (§2.1 step 1, §2.3)
    def flight_deliver(self, t, pkt):
        apid = int.from_bytes(pkt[0:2], "big") & 0x7FF
        d = pkt[6:]
        require(len(d) >= (3 if apid == APID["table_upload"] else 8),
                "short uplink packet: flight handling of a malformed uplink unit is not modelled (APP-131)")
        if apid == APID["table_upload"]:
            tid, ln = d[0], int.from_bytes(d[1:3], "big")
            octets = d[3:]
            require(len(octets) == ln, "table upload length field disagrees with its octets")
            self.table = (tid, octets)  # APP-017: keeps the most recent
            self.flight_app_event(t, 21, "table_received", [tid, ln])
            return
        require(apid == APID["commands"], f"unexpected uplink APID {apid}")
        func = d[0]
        seq = int.from_bytes(pkt[2:4], "big") & 0x3FFF
        self.flight_app_event(t, 20, "cmd_received", [func, seq])  # APP-129
        mode, tag, args = d[1], d[2:8], d[8:]
        if mode == 0x00 and tag == bytes(6):
            self.execute(t, func, seq, args)  # immediate: executes at delivery (§2.3)
        elif mode == 0x01:
            ex = cuc_to_us(tag)
            if ex < t:
                self.reject(t, func, seq, 2)  # time tag in the past (§2.3)
            elif len(self.queue) >= QUEUE_CAP:
                self.reject(t, func, seq, 4)  # queue full
            else:
                self.queue.append((ex, func, seq, args))  # checks happen at execution
        else:
            self.reject(t, func, seq, 3)  # no valid execution instant (APP-013)

    def reject(self, t, func, seq, reason):
        self.rejected += 1
        self.event(t, 0x0002, func, seq, reason)

    def execute(self, t, func, seq, args):
        # §2.3: checks of function code and arguments at execution
        if func not in ARG_LEN:
            return self.reject(t, func, seq, 1)
        if len(args) != ARG_LEN[func]:
            return self.reject(t, func, seq, 3)  # APP-024
        if func == 0x02:
            addr, ln = int.from_bytes(args[0:4], "big"), int.from_bytes(args[4:6], "big")
            if not 1 <= ln <= 1024:
                return self.reject(t, func, seq, 3)
            if self.dump is not None:
                return self.reject(t, func, seq, 5)  # APP-016
        if func == 0x04 and args[0] > 1:
            return self.reject(t, func, seq, 3)
        self.executed += 1
        self.event(t, 0x0001, func, seq, 0)  # CMD_EXECUTED first (§3.3.2)
        if func == 0x03:
            tid, crc = args[0], int.from_bytes(args[1:3], "big")
            if self.table is None or self.table[0] != tid:
                self.event(t, 0x0011, tid, 1, 0)  # APP-017 reason 1
            elif crc16(self.table[1]) != crc:
                self.event(t, 0x0011, tid, 2, 0)  # reason 2
            else:
                self.last_table_id, self.last_table_crc = tid, crc
                self.event(t, 0x0010, tid, len(self.table[1]), crc)
        elif func == 0x02:
            sdu = u32(addr) + u16(ln) + memory(addr, ln)
            chunks = [sdu[i:i + DUMP_CHUNK] for i in range(0, len(sdu), DUMP_CHUNK)]
            self.dump = {"chunks": chunks, "i": 0, "addr": addr, "len": ln, "due": t}
        elif func == 0x04:
            self.payload_on = args[0] == 1  # APP-015
            self.event(t, 0x0030, args[0], 0, 0)

    def flight_step(self, t, uplink):
        for pkt in uplink:  # (1)
            self.flight_deliver(t, pkt)
        due = [c for c in self.queue if c[0] <= t]  # (2) in queue order
        self.queue = [c for c in self.queue if c[0] > t]
        for _ex, func, seq, args in due:
            self.execute(t, func, seq, args)
        if self.dump is not None and self.dump["due"] <= t:  # (3) one segment per 125 000 µs from the execution instant
            dp = self.dump
            dp["due"] += SLOT_US
            i, n = dp["i"], len(dp["chunks"])
            flags = 0b11 if n == 1 else (0b01 if i == 0 else (0b10 if i == n - 1 else 0b00))  # APP-016
            self.tm(t, APID["dump"], flags, dp["chunks"][i])
            dp["i"] += 1
            if dp["i"] == n:
                self.dumps_completed += 1
                self.event(t, 0x0020, dp["addr"] >> 16, dp["addr"] & 0xFFFF, dp["len"])
                self.dump = None
        if t % SEC_US == 0 and t < GEN_END_US:  # (4) §3.3.1
            k = t // SEC_US
            ud = (u32(k) + u16(self.executed) + u16(self.rejected)
                  + bytes([len(self.queue), 1 if self.payload_on else 0, self.last_table_id, 0])
                  + u16(self.last_table_crc) + u16(self.dumps_completed)
                  + b"".join(u16(1000 * (j + 1) + 3 * k) for j in range(8)))
            require(len(ud) == 32, "HK user data is not 32 octets")
            self.tm(t, APID["housekeeping"], 0b11, ud)
        if t < GEN_END_US and t >= 500_000 and (t - 500_000) % (2 * SEC_US) == 0 and self.payload_on:  # (5)
            ud = u32(self.payload_n) + bytes((self.payload_n + i) % 256 for i in range(4, 400))
            self.payload_n += 1
            self.tm(t, APID["payload"], 0b11, ud)

    # -- ground: downlink delivery (APP-130, §7.4)
    def ground_app_event(self, t, code, name, args):
        self.rec(t, "app_event", node="ground", code=code, name=name, args=args)

    def ground_deliver(self, t, unit):
        # strict decoder: 133.0 §4.1.3; errors from docs/standards/space-packet.md §5.1
        if len(unit) < 6:
            return self.ground_app_event(t, 6, "packet_rejected", [ERR_INCOMPLETE])
        if unit[0] >> 5 != 0:
            return self.ground_app_event(t, 6, "packet_rejected", [ERR_UNSUPPORTED_VERSION])
        total = 7 + int.from_bytes(unit[4:6], "big")
        if len(unit) < total:
            return self.ground_app_event(t, 6, "packet_rejected", [ERR_INCOMPLETE])
        apid = int.from_bytes(unit[0:2], "big") & 0x7FF
        if apid == 0x7FF and (unit[0] >> 3) & 1:
            # SPP-004 strict: Idle Packet with Secondary Header Flag = 1
            return self.ground_app_event(t, 6, "packet_rejected", [ERR_INVALID_FIELD])
        if len(unit) > total:
            # The model's own reading (the spec does not settle it: API-011 strict decode accepts
            # longer input; API-019 only offers an exact decode): a link unit is exactly one packet.
            return self.ground_app_event(t, 6, "packet_rejected", [ERR_LENGTH_MISMATCH])
        pkt = unit
        self.rec(t, "packet_in", node="ground", apid=apid, hex=pkt.hex().upper())
        w1 = int.from_bytes(pkt[2:4], "big")
        flags, count = w1 >> 14, w1 & 0x3FFF
        # SPP-011 / CMN-031 classification against the last count of this APID
        cont = "first"
        if apid in self.last_count:
            dist = (count - self.last_count[apid]) % 16384
            if dist == 1:
                cont = "in_sequence"
            elif dist == 0:
                cont = "repeat"
            elif dist <= 8192:
                cont = "gap"
                self.ground_app_event(t, 1, "seq_gap", [apid, dist - 1])
            else:
                cont = "reset"
        if cont == "repeat":
            self.ground_app_event(t, 2, "seq_repeat", [apid, count])
        elif cont == "reset":
            self.ground_app_event(t, 3, "seq_reset", [apid, count])
        self.last_count[apid] = count
        if apid == APID["dump"]:
            self.reassemble(t, flags, cont, pkt[6 + 6:])  # implicit P-field header: 6 octets (§3.2)

    def reassemble(self, t, flags, cont, ud):
        # docs/standards/space-packet.md §5.4 (SPP-013)
        discard = None
        if self.asm is not None and cont not in ("in_sequence", "first"):
            discard, self.asm = sum(map(len, self.asm)), None  # Gap
        else:
            if self.asm is not None and flags in (0b01, 0b11):  # Restarted / Interrupted
                discard, self.asm = sum(map(len, self.asm)), None
        complete = None
        if flags == 0b01:
            self.asm = [ud]
        elif flags == 0b11:
            complete = ud
        elif flags in (0b00, 0b10):
            if self.asm is None:
                pass  # Orphan, dropped without event
            else:
                self.asm.append(ud)
                if flags == 0b10:
                    complete, self.asm = b"".join(self.asm), None
        if discard is not None:
            self.ground_app_event(t, 5, "dump_discarded", [APID["dump"], discard])
        if complete is not None:
            addr, ln = int.from_bytes(complete[0:4], "big"), int.from_bytes(complete[4:6], "big")
            mem = complete[6:]
            # APP-130: an SDU that does not parse as §3.3.4 raises packet_rejected after the events
            # above. The spec names no error code for it; the codes below are the model's reading.
            if len(complete) < 6:
                self.ground_app_event(t, 6, "packet_rejected", [ERR_INCOMPLETE])
            elif not 1 <= ln <= 1024:
                self.ground_app_event(t, 6, "packet_rejected", [ERR_OUT_OF_RANGE])
            elif len(mem) != ln:
                self.ground_app_event(t, 6, "packet_rejected", [ERR_LENGTH_MISMATCH])
            else:
                self.ground_app_event(t, 4, "dump_reassembled", [addr, ln, crc16(mem)])

    # -- harness loop (§6.2)
    def go(self):
        gseq = Counters()
        uplink = build_uplink(self.case.get("ops", []), gseq)
        for g in range(0, self.end, SLOT_US):
            self.at(g)
        for at, _a, _p in uplink:
            self.at(at)
        self.at(0)
        while self.heap:
            t = heapq.heappop(self.heap)
            due = sorted((d for d in self.pending if d[0] == t), key=lambda d: d[1:5])  # (send, dir, index, copy)
            self.pending = [d for d in self.pending if d[0] != t]
            # step 2: ground
            for d in due:
                if d[2] == 0:
                    self.ground_deliver(t, d[5])
            for at, apid, pkt in uplink:
                if at == t:
                    self.rec(t, "packet_out", node="ground", apid=apid, hex=pkt.hex().upper())
                    self.send(t, "up", pkt)
            # step 3: flight
            self.flight_step(t, [d[5] for d in due if d[2] == 1])
        return self.trace


def self_checks(case_id, run):
    # Consistency of the model with the unit/fault tables of §6.6 (derived independently).
    if case_id == "a1-downlink-faults":
        want = {2: (16, 1), 4: (32, 1), 5: (16, 3), 11: (32, 3), 25: (33, 2)}
        for idx, ac in want.items():
            require(run.down_units[idx] == ac, f"unit {idx}: {run.down_units[idx]} != {ac} (§6.6)")
    require(crc16(b"123456789") == 0x29B1, "CRC-16 check value (CMN-002)")
    require(crc16(table_octets(300)) == 0x0C4F and crc16(memory(0x10000, 600)) == 0x80A8, "CRC-16 of §2.4")


def model(case_id):
    case = next(c for c in SCENARIO["cases"] if c["id"] == case_id)
    run = Run(case)
    trace = run.go()
    self_checks(case_id, run)
    return trace


KINDS = {"packet_out", "packet_in", "app_event"}


def compare(case_id, golden_path):
    mine = model(case_id)
    golden = [ln for ln in Path(golden_path).read_text().splitlines() if json.loads(ln)["kind"] in KINDS]
    sm = difflib.SequenceMatcher(None, mine, golden, autojunk=False)
    bad = 0
    for tag, i1, i2, j1, j2 in sm.get_opcodes():
        if tag == "equal":
            continue
        bad += max(i2 - i1, j2 - j1)
        for i in range(i1, i2):
            print(f"  model only [{i}]: {mine[i]}")
        for j in range(j1, j2):
            print(f"  golden only [{j}]: {golden[j]}")
    counts = {}
    for ln in mine:
        k = json.loads(ln)
        key = (k["kind"], k.get("node"))
        counts[key] = counts.get(key, 0) + 1
    summary = ", ".join(f"{n} {k[1]} {k[0]}" for k, n in sorted(counts.items()))
    print(f"{case_id}: {len(mine)} model records ({summary}), {len(golden)} golden records, {bad} mismatches")
    return bad


def main(argv):
    ap = argparse.ArgumentParser(description="Blind model of the A1 cases")
    ap.add_argument("case", nargs="?", choices=CASE_IDS)
    ap.add_argument("--compare", metavar="FILE")
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--golden-dir", metavar="DIR")
    args = ap.parse_args(argv[1:])
    if args.all:
        if not args.golden_dir:
            ap.error("--all needs --golden-dir")
        bad = sum(compare(c, Path(args.golden_dir) / f"{c}.jsonl") for c in CASE_IDS)
        return 1 if bad else 0
    if args.case is None:
        ap.error("CASE or --all required")
    if args.compare:
        return 1 if compare(args.case, args.compare) else 0
    print("\n".join(model(args.case)))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv))
    except ModelError as e:
        print(f"model error: {e}", file=sys.stderr)
        sys.exit(2)
