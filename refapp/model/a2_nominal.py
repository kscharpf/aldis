#!/usr/bin/env python3
"""Blind model of the `unit_sent` records of the `a2-nominal` golden trace.

A2 D8(c), TV-023.  Written from docs/reference-application.md (sections 2, 3,
4, 6, 7), docs/milestones/a2-frames.md D8 and docs/standards/; protocol values
came from those documents, not from the Blue Book text (132.0-B-3, 232.0-B-4,
133.0-B-2, 301.0-B-4 are the standards they cite); `refapp/scenario/scenario.json`
is read as input data only.  No implementation file was read.  Python 3, standard library only.

It simulates the flight node's applications and its per-virtual-channel field
generators and multiplexer, and the ground node's segmenter, then prints (or
compares) one record per link unit sent, in trace order:

    {"t_us":..,"kind":"unit_sent","dir":"down"|"up","index":..,"hex":".."}

Valid for `a2-nominal` only: no faults, no rejected command, no immediate
command (APP-141), no COP-1 (Type-BD, A2).
"""
import json
import sys

# Parameters, ops, and the end instant come from scenario.json (the file is input data).

CASE = "a2-nominal"


def load(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


# ------------------------------------------------------------------- CRC-16 --
def crc16(data):
    """CRC-16, polynomial 0x1021, preset 0xFFFF, no reflection, no final xor
    (132.0 4.1.6; 232.0 4.1.4; CMN-001)."""
    crc = 0xFFFF
    for b in data:
        crc ^= b << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0x1021) if crc & 0x8000 else (crc << 1)
            crc &= 0xFFFF
    return crc


assert crc16(b"123456789") == 0x29B1  # CMN-002


# --------------------------------------------------------------------- OID --
class OidLfsr:
    """Pseudo-noise fill of OID frames (132.0 4.1.4.6.2; CMN-020): 32 cells,
    all ones, x[n] = x[n-1] ^ x[n-2] ^ x[n-22] ^ x[n-32], MSB first.  Started
    once, never restarted (CMN-021)."""

    def __init__(self):
        self.bits = [1] * 32
        self.pos = 0

    def octets(self, count):
        out = bytearray()
        for _ in range(count):
            v = 0
            for _ in range(8):
                while len(self.bits) <= self.pos:
                    n = len(self.bits)
                    b = self.bits
                    self.bits.append(b[n - 1] ^ b[n - 2] ^ b[n - 22] ^ b[n - 32])
                v = (v << 1) | self.bits[self.pos]
                self.pos += 1
            out.append(v)
        return bytes(out)


# The expected octets are the 20-octet OID sequence printed in
# docs/verification/derivations/link-send.md (frame at line 34), written there
# before this model; the model's own output is not their source.
_chk = OidLfsr().octets(20)
assert _chk.hex().upper() == "FFFFFFFF6DB6D861451F11F19716723CBE7E00B1", _chk.hex()


# ------------------------------------------------------------ Space Packet --
def space_packet(apid, ptype, sec_hdr, flags, count, data):
    """133.0 4.1.3: version 000, type, secondary header flag, APID, sequence
    flags, count (14 bits), length = octets of the data field - 1."""
    assert 0 <= apid < 2048 and 0 <= count < 16384 and 1 <= len(data) <= 65536
    w0 = (0 << 13) | (ptype << 12) | (sec_hdr << 11) | apid
    w1 = (flags << 14) | count
    return bytes([w0 >> 8, w0 & 0xFF, w1 >> 8, w1 & 0xFF]) + \
        (len(data) - 1).to_bytes(2, "big") + bytes(data)


def idle_packet(total):
    """Space Packet Idle Packet, APID 2047, flags 11, count 0 (OI-90), idle
    data 0x00 (SPP-010).  `total` octets including the 6-octet header."""
    assert total >= 7
    return space_packet(2047, 0, 0, 3, 0, bytes(total - 6))


# --------------------------------------------------------------------- time --
class Clock:
    def __init__(self, sc):
        self.t0 = sc["time"]["t0_coarse_s"]

    def cuc(self, us_after_t0):
        """4 coarse + 2 fractional octets (301.0 3.2.2); 2^-16 s units."""
        sec, rem = divmod(us_after_t0, 1_000_000)
        frac = (rem * 65536) // 1_000_000
        assert (rem * 65536) % 1_000_000 == 0  # APP-061: exact
        return (self.t0 + sec).to_bytes(4, "big") + frac.to_bytes(2, "big")


# ----------------------------------------------------------- uplink (ground) --
class Uplink:
    """Ground node: ops entries -> packets -> MAP segmenter -> TC frames, one
    frame per uplink slot (4.2, 4.4.4, 2.2)."""

    def __init__(self, sc, ops, end_us):
        self.sc, self.ops, self.end_us = sc, ops, end_us
        self.clock = Clock(sc)
        tc = sc["tc"]
        self.scid = sc["spacecraft_id"]
        self.max_fdu = tc["max_fdu_length"]  # includes the Segment Header
        self.map_cmd, self.map_tab = tc["map_commands"], tc["map_table_upload"]
        self.vcid = tc["vcid"]
        self.cmd_seq = 0
        self.tab_seq = 0

    def command_packet(self, e):
        call = e["call"]
        assert e["execution"]["mode"] == "time_tagged", "APP-141: no immediate"
        fn = call["function"]
        if fn == "noop":
            code, args = 0x01, b""
        elif fn == "dump_memory":
            code, args = 0x02, call["address"].to_bytes(4, "big") + \
                call["length"].to_bytes(2, "big")
        elif fn == "activate_table":
            code, args = 0x03, bytes([call["table_id"]]) + call["crc"].to_bytes(2, "big")
        elif fn == "set_payload_mode":
            code, args = 0x04, bytes([call["mode"]])
        else:
            raise NotImplementedError(fn)
        assert not e.get("extra_octets")
        data = bytes([code, 0x01]) + self.clock.cuc(e["execution"]["exec_us"]) + args
        pkt = space_packet(self.sc["apid"]["commands"], 1, 0, 3, self.cmd_seq, data)
        seq, self.cmd_seq = self.cmd_seq, self.cmd_seq + 1
        return pkt, seq

    def table_packet(self, e):
        n = e["length"]
        data = bytes([e["table_id"]]) + n.to_bytes(2, "big") + \
            bytes((3 * i + 1) % 256 for i in range(n))
        pkt = space_packet(self.sc["apid"]["table_upload"], 1, 0, 3, self.tab_seq, data)
        self.tab_seq += 1
        return pkt

    def tc_frame(self, map_id, flags, fdu_data):
        """Type-BD frame, N(S) = 0 (232.0 4.1.2, 4.1.3.2.2)."""
        body = bytes([(flags << 6) | map_id]) + fdu_data  # Segment Header + FDU data
        total = 5 + len(body) + 2
        assert total <= self.sc["tc"]["max_frame_length"]
        w0 = (0 << 14) | (1 << 13) | (0 << 12) | self.scid  # version, bypass, ctrl, spare 00
        hdr = bytes([w0 >> 8, w0 & 0xFF,
                     (self.vcid << 2) | ((total - 1) >> 8), (total - 1) & 0xFF, 0])
        f = hdr + body
        return f + crc16(f).to_bytes(2, "big")

    def run(self):
        """Returns frames: dict(t, bytes, deliveries) with deliveries the
        entries (dict) that the flight node receives when the frame arrives."""
        slot = self.sc["link"]["uplink_slot_us"]
        delay = self.sc["link"]["one_way_delay_us"]
        queue0 = []  # MAP 0 packets: (packet, entry)
        queue1 = []  # MAP 1: packets
        seg = None  # MAP 1 in progress: (packet, offset, entry)
        pending_ops = [e for e in self.ops if e["kind"] in ("command", "table_upload")]
        frames = []
        t = 0
        while t < self.end_us:
            while pending_ops and pending_ops[0]["at_us"] <= t:
                e = pending_ops.pop(0)
                if e["kind"] == "command":
                    pkt, seq = self.command_packet(e)
                    queue0.append((pkt, dict(e, seq=seq)))
                else:
                    queue1.append((self.table_packet(e), dict(e)))
            frame = None
            max_user = self.max_fdu - 1
            if seg is None and queue0:
                fdu, ents = b"", []
                while queue0 and len(fdu) + len(queue0[0][0]) <= max_user:
                    p, e = queue0.pop(0)
                    fdu += p
                    ents.append(e)
                assert fdu, "a packet longer than one FDU on MAP 0"
                frame = (self.tc_frame(self.map_cmd, 3, fdu), ents)
            elif seg is not None or queue1:
                if seg is None:
                    p, e = queue1.pop(0)
                    seg = [p, 0, e]
                p, off, e = seg
                chunk = p[off:off + max_user]
                last = off + len(chunk) >= len(p)
                if off == 0:
                    flags = 3 if last else 1
                else:
                    flags = 2 if last else 0
                frame = (self.tc_frame(self.map_tab, flags, chunk), [e] if last else [])
                seg = None if last else [p, off + len(chunk), e]
            if frame is not None:
                frames.append({"t": t, "bytes": frame[0], "arrive": t + delay,
                               "delivers": frame[1]})
            t += slot
        return frames


# ---------------------------------------------------------------- downlink --
class FieldGen:
    """Per-VC packet generator: fills data fields of `size` octets, releases a
    full field, or one past its deadline after padding with an Idle Packet
    (LNK-011..013; 132.0 4.2.2.5).  A field is started when its first octet is
    written (4.4.2)."""

    def __init__(self, size, deadline_us):
        self.size, self.deadline = size, deadline_us
        self.cur = None  # [bytearray, starts, start_us]
        self.pending = []  # [packet, consumed]

    def _fill(self):
        while self.cur is not None and len(self.cur[0]) < self.size and self.pending:
            p, used = self.pending[0]
            room = self.size - len(self.cur[0])
            take = p[used:used + room]
            if used == 0:
                self.cur[1].append(len(self.cur[0]))
            self.cur[0] += take
            if used + len(take) >= len(p):
                self.pending.pop(0)
            else:
                self.pending[0][1] = used + len(take)

    def submit(self, pkt, now):
        self.pending.append([pkt, 0])
        if self.cur is None:
            self.cur = [bytearray(), [], now]
        self._fill()

    def timeout(self, now):
        if self.cur is not None and len(self.cur[0]) < self.size \
                and now >= self.cur[2] + self.deadline:
            rem = self.size - len(self.cur[0])
            assert rem >= 7, "idle remainder below 7 octets: not modelled"
            self.cur[1].append(len(self.cur[0]))
            self.cur[0] += idle_packet(rem)

    def full(self):
        return self.cur is not None and len(self.cur[0]) == self.size

    def release(self, now):
        data, starts, _ = self.cur
        self.cur = None
        if self.pending:
            self.cur = [bytearray(), [], now]
            self._fill()
        return bytes(data), (starts[0] if starts else 0x7FF)


class Flight:
    def __init__(self, sc, end_us, uplink_frames):
        self.sc, self.end_us = sc, end_us
        self.clock = Clock(sc)
        tm = sc["tm"]
        self.scid = sc["spacecraft_id"]
        self.flen = tm["frame_length"]
        self.dsize = self.flen - 6 - 4 - 2  # header, OCF, FECF (CMN-040)
        assert self.dsize == 211
        self.vc = {0: FieldGen(self.dsize, tm["release_deadline_realtime_us"]),
                   1: FieldGen(self.dsize, tm["release_deadline_payload_us"])}
        self.held = {0: None, 1: None}
        self.oid = OidLfsr()
        self.vc_count = {0: 0, 1: 0, 7: 0}
        self.apid_count = {}
        self.ul = uplink_frames
        self.out = []
        # application state
        self.queue = []  # (exec_us, order, entry)
        self.table = None  # (id, length, crc)
        self.last_table = (0, 0)
        self.executed = self.rejected = 0
        self.mode = 1
        self.dumps = 0
        self.dump = None  # [sdu_chunks, next_index, next_time, address, length]
        self.hk_n = 0
        self.pl_n = 0
        self.order = 0

    # -- packets
    def emit(self, apid, flags, data_with_sec, vc):
        c = self.apid_count.get(apid, 0)
        self.apid_count[apid] = (c + 1) % 16384
        pkt = space_packet(apid, 0, 1, flags, c, data_with_sec)
        self.vc[vc].submit(pkt, self.now)
        return pkt

    def event(self, code, p1=0, p2=0, p3=0):
        d = bytes([0x1E]) + self.clock.cuc(self.now) + b"".join(
            v.to_bytes(2, "big") for v in (code, p1, p2, p3))
        self.emit(self.sc["apid"]["events"], 3, d, 0)

    # -- steps
    def deliver(self, t, prev):
        for f in self.ul:
            if prev < f["arrive"] <= t:
                for e in f["delivers"]:
                    if e["kind"] == "table_upload":
                        n = e["length"]
                        octets = bytes((3 * i + 1) % 256 for i in range(n))
                        self.table = (e["table_id"], n, crc16(octets))
                    else:
                        assert e["execution"]["mode"] == "time_tagged"
                        assert e["execution"]["exec_us"] >= t  # not rejected (APP-141)
                        assert len(self.queue) < 8
                        self.queue.append((e["execution"]["exec_us"], self.order, e))
                        self.order += 1

    def execute_due(self, t):
        due = sorted(x for x in self.queue if x[0] <= t)
        self.queue = [x for x in self.queue if x[0] > t]
        for _, _, e in due:
            call, seq = e["call"], e["seq"]
            code = {"noop": 1, "dump_memory": 2, "activate_table": 3,
                    "set_payload_mode": 4}[call["function"]]
            self.executed += 1
            self.event(0x0001, code, seq, 0)
            if call["function"] == "activate_table":
                tab = self.table
                if tab is None or tab[0] != call["table_id"]:
                    self.event(0x0011, call["table_id"], 1, 0)
                elif tab[2] != call["crc"]:
                    self.event(0x0011, call["table_id"], 2, 0)
                else:
                    self.last_table = (tab[0], tab[2])
                    self.event(0x0010, tab[0], tab[1], tab[2])
            elif call["function"] == "set_payload_mode":
                self.mode = call["mode"]
                self.event(0x0030, call["mode"], 0, 0)
            elif call["function"] == "dump_memory":
                assert self.dump is None  # rejection (reason 5) not modelled
                a, n = call["address"], call["length"]
                mem = bytes(((a + i) ^ ((a + i) >> 8)) & 0xFF for i in range(n))
                sdu = a.to_bytes(4, "big") + n.to_bytes(2, "big") + mem
                chunks = [sdu[i:i + 128] for i in range(0, len(sdu), 128)]
                self.dump = [chunks, 0, t, a, n]

    def dump_step(self, t):
        d = self.dump
        if d is None or t < d[2]:
            return
        chunks, i = d[0], d[1]
        flags = 3 if len(chunks) == 1 else (1 if i == 0 else (2 if i == len(chunks) - 1 else 0))
        self.emit(self.sc["apid"]["dump"], flags, self.clock.cuc(t) + chunks[i], 1)
        d[1], d[2] = i + 1, d[2] + self.sc["link"]["frame_slot_us"]
        if d[1] == len(chunks):
            self.dumps += 1
            a, n = d[3], d[4]
            self.event(0x0020, a >> 16, a & 0xFFFF, n)
            self.dump = None

    def housekeeping(self, t):
        k = self.hk_n
        d = k.to_bytes(4, "big") + (self.executed & 0xFFFF).to_bytes(2, "big") + \
            (self.rejected & 0xFFFF).to_bytes(2, "big") + bytes(
                [len(self.queue), self.mode, self.last_table[0], 0]) + \
            self.last_table[1].to_bytes(2, "big") + (self.dumps & 0xFFFF).to_bytes(2, "big") + \
            b"".join(((1000 * (j + 1) + 3 * k) & 0xFFFF).to_bytes(2, "big") for j in range(8))
        assert len(d) == 32
        self.emit(self.sc["apid"]["housekeeping"], 3, self.clock.cuc(t) + d, 0)
        self.hk_n += 1

    def payload(self, t):
        n = self.pl_n
        d = n.to_bytes(4, "big") + bytes((n + i) % 256 for i in range(4, 400))
        self.emit(self.sc["apid"]["payload"], 3, self.clock.cuc(t) + d, 1)
        self.pl_n += 1

    def clcw(self, t):
        """FARM-B counter of the Type-BD frames arrived at or before t
        (4.4.1), in a CLCW with COP in Effect 01 (232.0 4.2.1)."""
        counter = sum(1 for f in self.ul if f["arrive"] <= t) % 4
        initial = self.sc["tm"]["ocf_initial"]
        assert initial & (3 << 9) == 0, "ocf_initial must have CLCW bits 21-22 clear"
        word = initial | (counter << 9)
        return word.to_bytes(4, "big")

    def frame(self, t, mc, vcid, fhp, data):
        w0 = (0 << 14) | (self.scid << 4) | (vcid << 1) | 1  # OCF flag 1
        hdr = bytes([w0 >> 8, w0 & 0xFF, mc & 0xFF, self.vc_count[vcid] & 0xFF,
                     0x18 | (fhp >> 8), fhp & 0xFF])
        self.vc_count[vcid] += 1
        f = hdr + data + self.clcw(t)
        assert len(f) == self.flen - 2
        return f + crc16(f).to_bytes(2, "big")

    def run(self):
        slot = self.sc["link"]["frame_slot_us"]
        prev = -1
        i = 0
        t = 0
        sec = 1_000_000
        while t < self.end_us:
            self.now = t
            self.deliver(t, prev)  # 1
            self.execute_due(t)  # 2
            self.dump_step(t)  # 3
            if t % sec == 0 and t < 30 * sec:
                self.housekeeping(t)  # 4
            if t % (2 * sec) == sec // 2 and t < 30 * sec and self.mode == 1:
                self.payload(t)  # 5
            for vc in (0, 1):  # 6
                self.vc[vc].timeout(t)
                if self.held[vc] is None and self.vc[vc].full():
                    self.held[vc] = self.vc[vc].release(t)
            for vc in (0, 1):
                if self.held[vc] is not None:
                    data, fhp = self.held[vc]
                    self.held[vc] = None
                    f = self.frame(t, i, vc, fhp, data)
                    break
            else:
                f = self.frame(t, i, 7, 0x7FE, self.oid.octets(self.dsize))
            self.out.append({"t": t, "dir": "down", "index": i, "hex": f})
            prev = t
            t += slot
            i += 1
        return self.out


# ------------------------------------------------------------------ driver --
def model(scenario_path):
    sc = load(scenario_path)
    case = next((c for c in sc["cases"] if c["id"] == CASE), None)
    if case is None:
        sys.exit("no a2-nominal case in the scenario")
    ops = [e for e in case["ops"] if e["kind"] != "fop1_directive"]
    end = case["end_us"]
    ul = Uplink(sc, ops, end).run()
    down = Flight(sc, end, ul).run()
    recs = [{"t": f["t"], "dir": "up", "index": n, "hex": f["bytes"]}
            for n, f in enumerate(ul)]
    recs += down
    # trace order (6.2): per instant the ground node (up) before the flight node
    recs.sort(key=lambda r: (r["t"], 0 if r["dir"] == "up" else 1, r["index"]))
    return recs, ul, down


def line(r):
    return '{"t_us":%d,"kind":"unit_sent","dir":"%s","index":%d,"hex":"%s"}' % (
        r["t"], r["dir"], r["index"], r["hex"].hex().upper())


def self_check(ul, down):
    """The hand-derived schedule of reference-application.md 4.4.4."""
    kinds = [(f["hex"][0] << 8 | f["hex"][1]) >> 1 & 7 for f in down]
    assert len(down) == 280
    assert kinds.count(7) == 219 and kinds.count(0) == 31 and kinds.count(1) == 30
    assert [down[i]["hex"][:6].hex().upper() for i in (0, 2, 4)] == \
        ["155F00001FFE", "155102001800", "155304001800"]
    # Payload data (reference-application.md 4.3, 3.3.3): the first VC 1 frame
    # holds packet number n = 0 from its start: 6 octets of frame header, 6 of
    # packet header, 6 of CUC time, then data octets 0-3 = n and octet i =
    # (n + i) mod 256 from i = 4.
    first_pl = next(f["hex"] for f in down if (f["hex"][0] << 8 | f["hex"][1]) >> 1 & 7 == 1)
    assert first_pl[18:22] == bytes(4) and first_pl[22:26] == bytes([4, 5, 6, 7])
    last = down[-1]["hex"]
    assert last[2] == 23
    vc = {0: [], 1: [], 7: []}
    for f in down:
        vc[(f["hex"][0] << 8 | f["hex"][1]) >> 1 & 7].append(f["hex"][3])
    assert vc[0][-1] == 30 and vc[1][-1] == 29 and vc[7][-1] == 218
    expect = {97: (1, 12, 0), 99: (1, 13, 69), 100: (1, 14, 138), 101: (1, 15, 33),
              103: (1, 16, 0x7FF), 110: (1, 17, 23)}
    # frames 98 and 102 are VC 0 (4.4.4); at 102 VC 0 goes before VC 1 (LNK-031)
    for i in (98, 102):
        assert (down[i]["hex"][0] << 8 | down[i]["hex"][1]) >> 1 & 7 == 0, i
    for i, (v, c, fhp) in expect.items():
        h = down[i]["hex"]
        assert (h[0] << 8 | h[1]) >> 1 & 7 == v and h[3] == c, i
        assert ((h[4] & 7) << 8 | h[5]) == fhp, (i, h[4:6].hex())
    # OCF: changes in exactly these frames
    ocf = [f["hex"][-6:-2].hex().upper() for f in down]
    chg = [i for i in range(280) if ocf[i] != (ocf[i - 1] if i else "01000000")]
    assert chg == [9, 17, 19, 21, 25, 33, 41, 49], chg
    assert ocf[9] == "01000200" and ocf[19] == "01000600" and ocf[49] == "01000000"
    tc = [f["bytes"][:6].hex().upper() + "/" + str(len(f["bytes"])) for f in ul]
    assert tc == ["2155001500C0/22", "2155007F0041/128",
                  "2155007F0001/128", "2155004C0081/77", "2155001800C0/25",
                  "2155001B00C0/28", "2155002400C0/37", "2155001600C0/23"], tc
    assert [f["t"] for f in ul] == [1000000, 2000000, 2250000, 2500000, 3000000,
                                    4000000, 5000000, 6000000]


def compare(recs, golden_path):
    gold = []
    with open(golden_path, encoding="utf-8") as f:
        for ln in f:
            r = json.loads(ln)
            if r.get("kind") == "unit_sent":
                gold.append(r)
    bad = 0
    n = max(len(gold), len(recs))
    for k in range(n):
        m = recs[k] if k < len(recs) else None
        g = gold[k] if k < len(gold) else None
        mm = None if m is None else {"t_us": m["t"], "kind": "unit_sent", "dir": m["dir"],
                                     "index": m["index"], "hex": m["hex"].hex().upper()}
        if mm != g:
            bad += 1
            print("record %d differs" % k)
            print("  model :", None if mm is None else line(m))
            print("  golden:", None if g is None else json.dumps(g, separators=(",", ":")))
            if mm and g and mm["hex"] != g["hex"]:
                a, b = mm["hex"], g["hex"]
                off = next((x for x in range(0, min(len(a), len(b)), 2) if a[x:x + 2] != b[x:x + 2]),
                           min(len(a), len(b)))
                print("  first differing octet:", off // 2)
    print("unit_sent records: model %d, golden %d, differing %d" % (len(recs), len(gold), bad))
    return bad == 0


def main(argv):
    scen = "refapp/scenario/scenario.json"
    args = argv[1:]
    gold = None
    check = False
    while args:
        a = args.pop(0)
        if a == "--compare":
            if not args:
                print(__doc__)
                return 2
            gold = args.pop(0)
        elif a == "--scenario":
            if not args:
                print(__doc__)
                return 2
            scen = args.pop(0)
        elif a == "--self-check":
            check = True
        else:
            print(__doc__)
            return 2
    recs, ul, down = model(scen)
    if check:
        self_check(ul, down)
        print("self-check against the schedule of reference-application.md 4.4.4: ok")
    if gold:
        return 0 if compare(recs, gold) else 1
    if not check:
        for r in recs:
            print(line(r))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
