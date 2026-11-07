#!/usr/bin/env python3
"""Blind model of the `packet_out` records of the `a1-nominal` case (TV-023, A1 D5(c)).

Written from docs/reference-application.md, refapp/scenario/scenario.json and the
CCSDS Blue Books 133.0-B-2, 301.0-B-4 and 132.0-B-3 only, without sight of the
refapp implementation.

Usage:
    python3 a1_nominal.py                 # print the packet_out records (JSON Lines)
    python3 a1_nominal.py --compare FILE  # compare with the packet_out records of a golden trace
"""

import json
import sys
from pathlib import Path

SCENARIO = json.loads((Path(__file__).resolve().parent.parent / "scenario" / "scenario.json").read_text())

T0 = SCENARIO["time"]["t0_coarse_s"]  # source: §5 T0, scenario.json time.t0_coarse_s
APID = SCENARIO["apid"]  # source: §3.1, scenario.json apid
SEC_US = 1_000_000
END_US = 35 * SEC_US  # source: §6.4 a1-nominal "end 35 s"
GEN_END_US = 30 * SEC_US  # source: §2.1 "while t < 30 s"
DUMP_SPACING_US = 125_000  # source: §2.1 memory dump, APP-016
DUMP_CHUNK = 128  # source: §3.3.4, APP-016
DELAY_US = SCENARIO["link"]["one_way_delay_us"]  # source: §6.1


def crc16(data: bytes) -> int:
    # source: CMN-001; 132.0 §4.1.6.2.2: G(X) = X^16 + X^12 + X^5 + 1, preset all ones (L(X)),
    # MSB first, no final inversion.
    crc = 0xFFFF
    for octet in data:
        crc ^= octet << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0x1021) & 0xFFFF if crc & 0x8000 else (crc << 1) & 0xFFFF
    return crc


def u16(v): return (v & 0xFFFF).to_bytes(2, "big")  # source: GEN-050, §3.3 big-endian
def u32(v): return (v & 0xFFFFFFFF).to_bytes(4, "big")


def cuc_tfield(t_us: int) -> bytes:
    # source: §5 on-board time = T0 + now; 4 coarse + 2 fractional octets (301.0 §3.2.2);
    # fraction in units of 2^-16 s, exact on the 125 000 µs grid (§5, APP-061).
    coarse = T0 + t_us // SEC_US
    frac_us = t_us % SEC_US
    assert (frac_us * 65536) % SEC_US == 0
    return u32(coarse) + u16(frac_us * 65536 // SEC_US)


PFIELD = bytes([0b0_001_11_10])  # source: 301.0 §3.2.2 (ext 0, ID 001, 4-1=11, 2=10) = 0x1E, §3.2


def space_packet(ptype: int, apid: int, flags: int, count: int, data_field: bytes, sec_hdr: bool) -> bytes:
    # source: 133.0 §4.1.3: PVN '000' (§4.1.3.2.2) bits 0-2, Packet Type bit 3 (§4.1.3.3.2.3),
    # Secondary Header Flag bit 4 (§4.1.3.3.3.2), APID bits 5-15 (§4.1.3.3.4.1),
    # Sequence Flags bits 16-17 (§4.1.3.4.2.2), Sequence Count bits 18-31 (§4.1.3.4.3.1),
    # Packet Data Length = octets - 1 (§4.1.3.5.3).
    w0 = (0 << 13) | (ptype << 12) | ((1 if sec_hdr else 0) << 11) | (apid & 0x7FF)
    w1 = (flags << 14) | (count % 16384)  # source: §3.1 modulo 16384, 133.0 §4.1.3.4.3.4
    return u16(w0) + u16(w1) + u16(len(data_field) - 1) + data_field


class Counters:
    def __init__(self):
        self.seq = {}

    def next(self, apid):
        # source: §3.1 / APP-021: own count per APID, from 0
        c = self.seq.get(apid, 0)
        self.seq[apid] = c + 1
        return c


def table_octets(length):
    return bytes((3 * i + 1) % 256 for i in range(length))  # source: §3.3.6


def memory(addr, length):
    return bytes(((a ^ (a >> 8)) % 256) for a in range(addr, addr + length))  # source: §3.3.4


# Ops script of a1-nominal: §2.4, entries marked A1, in release order (§6.6).
# (release_us, kind, params); commands are time-tagged with exec_us after T0.
OPS = [
    (1_000_000, "command", {"func": 0x01, "exec_us": 6_000_000, "args": b""}),
    (2_000_000, "table_upload", {"table_id": 1, "length": 300}),
    (3_000_000, "command", {"func": 0x03, "exec_us": 10_000_000, "args": bytes([1]) + u16(0x0C4F)}),
    (4_000_000, "command", {"func": 0x02, "exec_us": 12_000_000, "args": u32(0x00010000) + u16(600)}),
    (5_000_000, "command", {"func": 0x04, "exec_us": 20_000_000, "args": bytes([0])}),
    (5_000_000, "command", {"func": 0x01, "exec_us": 20_000_000, "args": b""}),
    (6_000_000, "command", {"func": 0x04, "exec_us": 24_000_000, "args": bytes([1])}),
]
# source: §3.3.5 function codes and argument layouts; §2.4 arguments.


def model():
    out = []  # (t_us, node, apid, octets) in trace order
    gseq, fseq = Counters(), Counters()

    # Ground node: packet_out at each release instant (§2.2 ops script, §6.5).
    ground = {}
    queue_arrivals = []  # (arrival_us, exec_us, func, seq, args) delivered to flight
    cmd_seq = 0
    for at, kind, p in OPS:
        if kind == "command":
            # source: §3.3.5: function ‖ mode 0x01 time-tagged ‖ exec time CUC T-field ‖ args
            ud = bytes([p["func"], 0x01]) + cuc_tfield(p["exec_us"]) + p["args"]
            seq = gseq.next(APID["commands"])
            # source: §3.1: Packet Type 1, no secondary header, flags '11'
            pkt = space_packet(1, APID["commands"], 0b11, seq, ud, False)
            queue_arrivals.append((at + DELAY_US, p["exec_us"], p["func"], seq, p["args"]))
            assert seq == cmd_seq
            cmd_seq += 1
        else:
            # source: §3.3.6: table ID ‖ length ‖ octets; §3.1 type 1, no sec header, '11'
            ud = bytes([p["table_id"]]) + u16(p["length"]) + table_octets(p["length"])
            pkt = space_packet(1, APID["table_upload"], 0b11, gseq.next(APID["table_upload"]), ud, False)
        ground.setdefault(at, []).append((APID["commands"] if kind == "command" else APID["table_upload"], pkt))

    # Flight state (§3.3.1 fields).
    executed = rejected = 0
    payload_on = True  # source: §2.1 mode on at t = 0
    last_table_id, last_table_crc = 0, 0
    dumps_completed = 0
    table = None  # stored (id, octets); arrives 2.010 s (§6.1 delay)
    table_arrival = 2_000_000 + DELAY_US
    queue = []
    dump = None  # list of remaining chunks, with index
    payload_n = 0

    def tm(apid, flags, t_us, ud, explicit=False):
        sh = (PFIELD if explicit else b"") + cuc_tfield(t_us)  # source: §3.2, APP-022
        return space_packet(0, apid, flags, fseq.next(apid), sh + ud, True)  # source: §3.1 type 0

    def event(t_us, code, p1=0, p2=0, p3=0):
        # source: §3.3.2 code ‖ p1 ‖ p2 ‖ p3, explicit P-field (§3.1)
        return (t_us, "flight", APID["events"],
                tm(APID["events"], 0b11, t_us, u16(code) + u16(p1) + u16(p2) + u16(p3), True))

    instants = range(0, END_US, 125_000)  # every instant of interest is on this grid (APP-061)
    for t in instants:
        # §6.2 step 2: ground first
        for apid, pkt in ground.get(t, []):
            out.append((t, "ground", apid, pkt))
        # §6.2 step 3, flight in the order of §2.1
        # (1) deliveries: time-tagged commands queue (§2.3); table stored (§3.3.6). No packets.
        # Arrivals are at x.010 s (§6.1, release + one-way delay), off the 125 ms grid; they emit
        # no packet, so the model applies each one at the first grid instant after it.
        for arr, ex, func, seq, args in queue_arrivals:
            if t - 125_000 < arr <= t:
                queue.append((ex, func, seq, args))
        if t - 125_000 < table_arrival <= t:
            table = (1, table_octets(300))
        # (2) time-tagged commands due, in queue order (§2.3)
        due = [c for c in queue if c[0] <= t]
        queue = [c for c in queue if c[0] > t]
        for ex, func, seq, args in due:
            executed += 1
            out.append(event(t, 0x0001, func, seq, 0))  # CMD_EXECUTED first (§3.3.2)
            if func == 0x03:
                tid, crc = args[0], int.from_bytes(args[1:3], "big")
                if table and table[0] == tid and crc16(table[1]) == crc:  # source: APP-017
                    last_table_id, last_table_crc = tid, crc
                    out.append(event(t, 0x0010, tid, len(table[1]), crc))
                else:
                    out.append(event(t, 0x0011, tid, 1 if not table or table[0] != tid else 2, 0))
            elif func == 0x02:
                addr, ln = int.from_bytes(args[0:4], "big"), int.from_bytes(args[4:6], "big")
                sdu = u32(addr) + u16(ln) + memory(addr, ln)  # source: §3.3.4
                chunks = [sdu[i:i + DUMP_CHUNK] for i in range(0, len(sdu), DUMP_CHUNK)]
                dump = {"chunks": chunks, "i": 0, "addr": addr, "len": ln}
            elif func == 0x04:
                payload_on = args[0] == 1  # source: APP-015
                out.append(event(t, 0x0030, args[0], 0, 0))
        # (3) a due dump segment, one per 125 ms from the execution instant (APP-016)
        if dump is not None:
            i, n = dump["i"], len(dump["chunks"])
            # source: APP-016, 133.0 §4.1.3.4.2.2: 01 first, 00 continuation, 10 last, 11 single
            flags = 0b11 if n == 1 else (0b01 if i == 0 else (0b10 if i == n - 1 else 0b00))
            out.append((t, "flight", APID["dump"], tm(APID["dump"], flags, t, dump["chunks"][i])))
            dump["i"] += 1
            if dump["i"] == n:
                dumps_completed += 1
                out.append(event(t, 0x0020, dump["addr"] >> 16, dump["addr"] & 0xFFFF, dump["len"]))
                dump = None
        # (4) housekeeping at t = k s, t < 30 s (§2.1, §3.3.1)
        if t % SEC_US == 0 and t < GEN_END_US:
            k = t // SEC_US
            ud = (u32(k) + u16(executed) + u16(rejected) + bytes([len(queue), 1 if payload_on else 0,
                  last_table_id, 0]) + u16(last_table_crc) + u16(dumps_completed)
                  + b"".join(u16(1000 * (j + 1) + 3 * k) for j in range(8)))
            assert len(ud) == 32
            out.append((t, "flight", APID["housekeeping"], tm(APID["housekeeping"], 0b11, t, ud)))
        # (5) payload at t = 0.5 + 2k s while on (§2.1, §3.3.3)
        if t < GEN_END_US and (t - 500_000) % (2 * SEC_US) == 0 and t >= 500_000 and payload_on:
            ud = u32(payload_n) + bytes((payload_n + i) % 256 for i in range(4, 400))
            payload_n += 1
            out.append((t, "flight", APID["payload"], tm(APID["payload"], 0b11, t, ud)))
    return out


def record(t, node, apid, octets):
    # source: §7.1, §7.3 key order t_us, kind, node, apid, hex; uppercase hex; no whitespace
    return json.dumps({"t_us": t, "kind": "packet_out", "node": node, "apid": apid,
                       "hex": octets.hex().upper()}, separators=(",", ":"))


def main(argv):
    import argparse
    ap = argparse.ArgumentParser(description="Blind model of the a1-nominal packet_out records")
    ap.add_argument("--compare", metavar="FILE", help="golden trace to compare against")
    args = ap.parse_args(argv[1:])  # exits 2 with a usage error on bad arguments
    assert crc16(b"123456789") == 0x29B1  # CMN-002 check value
    assert crc16(table_octets(300)) == 0x0C4F and crc16(memory(0x10000, 600)) == 0x80A8  # §2.4
    mine = [record(*r) for r in model()]
    if args.compare is not None:
        golden = [ln for ln in Path(args.compare).read_text().splitlines()
                  if json.loads(ln).get("kind") == "packet_out"]
        bad = 0
        for i in range(max(len(mine), len(golden))):
            m = mine[i] if i < len(mine) else None
            g = golden[i] if i < len(golden) else None
            if m != g:
                bad += 1
                print(f"record {i}:\n  model : {m}\n  golden: {g}")
        print(f"{len(mine)} model records, {len(golden)} golden packet_out records, {bad} mismatches")
        return 1 if bad else 0
    print("\n".join(mine))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
