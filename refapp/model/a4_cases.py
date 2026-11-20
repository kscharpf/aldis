#!/usr/bin/env python3
"""Blind model of the cases `a4-aos-nominal` and `a4-uslp-nominal` (A4 D9, TV-023).

Regenerates every `unit_sent` record of both cases, in trace order: the AOS or
USLP downlink frames and the TC or USLP uplink frames.  Written blind: from
docs/reference-application.md (section 2.4, 4.1 to 4.6, 6, 7), the decisions of
docs/milestones/a4-variants.md section 2, docs/standards/ and the glossary, and
the Blue Books 732.0-B-5, 732.1-B-3, 232.0, 232.1, 133.0, 133.1-B-3, 301.0.  It
reads no scenario file: every value is taken from the document tables.  Python 3,
standard library only; no CI step.

Scope.  Only the FOP-1 and FARM-1 cells these two cases reach are implemented
(FOP-1: Initiate AD without CLCW check, request, transmit, and a CLCW that
acknowledges nothing or every outstanding frame; FARM-1: a Type-AD frame in
sequence).  Anything else raises NotModelled.  The model covers `unit_sent`
records only (maintainer, 2026-11-20; A3 D8(c) precedent).

    python3 -I refapp/model/a4_cases.py <case>                       # print records
    python3 -I refapp/model/a4_cases.py --self-check                 # section 4.6 figures
    python3 -I refapp/model/a4_cases.py --compare <golden.jsonl> <case>
"""

import json
import sys
from collections import defaultdict, deque


class NotModelled(Exception):
    """A cell or path that the two cases never reach."""


def fail(msg):
    raise AssertionError(msg)


def check(cond, msg):
    if not cond:
        fail(msg)


# ---------------------------------------------------------------------------
# Scenario constants (document tables)
# ---------------------------------------------------------------------------

SCID = 0x155                  # 3.4
T0 = 2_100_000_000            # 5
DOWN_SLOT = 125_000           # 6.1
UP_SLOT = 250_000
DELAY = 10_000
END = 35_000_000              # 2.4 (exclusive)
GEN_END = 30_000_000
STEP = 5_000                  # extra wake-ups change no output (APP-074)

DEADLINE = {0: 250_000, 1: 1_000_000}   # 4.1, 4.3.1, 4.3.2
IDLE_APID = 0x7FF

K_WINDOW = 4                  # 4.2
T1_INITIAL = 1_000_000
FARM_W = 8

CASES = ("a4-aos-nominal", "a4-uslp-nominal")
VARIANT = {"a4-aos-nominal": "aos", "a4-uslp-nominal": "uslp"}

# AOS downlink, 4.3.1
AOS_LEN = 223
AOS_ZONE = 207
AOS_MIN_IDLE = 7
# USLP downlink, 4.3.2
USLP_LEN = 223
USLP_ZONE = 206
# USLP uplink, 4.3.3
USLP_UP_MAX = 512
USLP_UP_MAX_PACKET = 501
# TC uplink, 4.2
TC_MAX_FDU = 121

# USLP TFDF Header first octets, 4.6.1 (UPID bits from specs/sana/uslp_protocol_id.tsv)
UPID_PACKETS = 0b00000
UPID_COP1 = 0b00001
UPID_IDLE = 0b11111


# ---------------------------------------------------------------------------
# Common algorithms
# ---------------------------------------------------------------------------

def crc16(data, init=0xFFFF):
    """FECF: G = x^16+x^12+x^5+1, preset all ones, MSB first, no final XOR (CMN-001)."""
    crc = init
    for b in data:
        crc ^= b << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0x1021) & 0xFFFF if crc & 0x8000 else (crc << 1) & 0xFFFF
    return crc


class OidGenerator:
    """x[n] = x[n-1]^x[n-2]^x[n-22]^x[n-32], x[0..31] = 1; MSB first; never restarted (CMN-020, CMN-021)."""

    def __init__(self):
        self.bits = [1] * 32
        self.pos = 0   # octets already taken

    def take(self, n):
        out = bytearray()
        for _ in range(n):
            while len(self.bits) < (self.pos + 1) * 8:
                i = len(self.bits)
                self.bits.append(self.bits[i - 1] ^ self.bits[i - 2] ^ self.bits[i - 22] ^ self.bits[i - 32])
            v = 0
            for b in self.bits[self.pos * 8:(self.pos + 1) * 8]:
                v = (v << 1) | b
            out.append(v)
            self.pos += 1
        return bytes(out)


# GF(16), field polynomial x^4 + x + 1; RS (15,11) with g(x) = (x+a^6)(x+a^7)(x+a^8)(x+a^9) (AOS-013)
_EXP = [0] * 30
_LOG = [0] * 16
_a = 1
for _i in range(15):
    _EXP[_i] = _EXP[_i + 15] = _a
    _LOG[_a] = _i
    _a <<= 1
    if _a & 0x10:
        _a ^= 0b10011


def _gmul(x, y):
    if x == 0 or y == 0:
        return 0
    return _EXP[_LOG[x] + _LOG[y]]


def _rs_generator():
    g = [1]
    for root in (6, 7, 8, 9):
        r = _EXP[root]
        new = [0] * (len(g) + 1)
        for j, c in enumerate(g):
            new[j] ^= c
            new[j + 1] ^= _gmul(c, r)
        g = new
    return g


_G = _rs_generator()


def aos_fhec(octets_0_1_5):
    """FHEC over header octets 0, 1, 5: five virtual zero symbols, six data symbols, four parity symbols."""
    syms = [0] * 5
    for b in octets_0_1_5:
        syms += [b >> 4, b & 15]
    r = [0, 0, 0, 0]
    for s in syms:
        fb = s ^ r[0]
        r = [r[1] ^ _gmul(fb, _G[1]), r[2] ^ _gmul(fb, _G[2]), r[3] ^ _gmul(fb, _G[3]), _gmul(fb, _G[4])]
    return bytes([(r[0] << 4) | r[1], (r[2] << 4) | r[3]])


# ---------------------------------------------------------------------------
# Packets (133.0, 133.1, 301.0; sections 3, 5 of the document)
# ---------------------------------------------------------------------------

def tfield(t_us):
    """CUC T-field: 4 coarse and 2 fractional octets of T0 + t (5)."""
    coarse = T0 + t_us // 1_000_000
    rem = t_us % 1_000_000
    if (rem * 65536) % 1_000_000:
        raise NotModelled("time not on the 2^-16 s grid")
    frac = rem * 65536 // 1_000_000
    return coarse.to_bytes(4, "big") + frac.to_bytes(2, "big")


def spp(apid, ptype, sec, flags, count, data):
    b0 = (ptype << 4) | (sec << 3) | (apid >> 8)
    return bytes([b0, apid & 0xFF, (flags << 6) | (count >> 8), count & 0xFF]) + (len(data) - 1).to_bytes(2, "big") + data


def spp_idle(n):
    """Space Packet Idle Packet of n octets in all: APID 2047, flags 11, count 0, zero data (SPP-010, OI-90)."""
    if n < 7:
        raise NotModelled("idle packet below 7 octets")
    return spp(IDLE_APID, 0, 0, 3, 0, bytes(n - 6))


def encap_idle(n):
    """Encapsulation Idle Packet of n octets in all (133.1 4.1.2.2-4, Table 4-2)."""
    if n == 1:
        return b"\xE0"
    if 2 <= n <= 255:
        return bytes([0xE1, n]) + bytes(n - 2)
    raise NotModelled("encapsulation idle length")


def split_packets(data):
    """Split a run of complete Space Packets by their length fields."""
    out = []
    i = 0
    while i < len(data):
        if len(data) - i < 6:
            raise NotModelled("truncated packet header")
        n = int.from_bytes(data[i + 4:i + 6], "big") + 7
        if i + n > len(data):
            raise NotModelled("truncated packet")
        out.append(data[i:i + n])
        i += n
    return out


def split_zone(data):
    """Split a stream of Space Packets and Encapsulation Packets (version in the top three bits)."""
    out = []
    i = 0
    while i < len(data):
        if data[i] >> 5 == 0b111:
            if data[i] == 0xE0:
                n = 1
            elif data[i] == 0xE1:
                n = data[i + 1]
            else:
                raise NotModelled("encapsulation form")
        else:
            n = int.from_bytes(data[i + 4:i + 6], "big") + 7
        out.append(data[i:i + n])
        i += n
    return out


def apid_of(pkt):
    return ((pkt[0] & 7) << 8) | pkt[1]


# ---------------------------------------------------------------------------
# Frames
# ---------------------------------------------------------------------------

def aos_frame(vc, count, fhp, zone, ocf):
    """AOS frame, Issue 5, FHEC and FECF, M_PDU, VC_OCF (4.3.1)."""
    if len(zone) != AOS_ZONE:
        fail("AOS zone length")
    scid8 = SCID & 0xFF
    b0 = (0b01 << 6) | (scid8 >> 2)
    b1 = ((scid8 & 3) << 6) | vc
    b5 = (((SCID >> 8) & 3) << 4)           # replay 0, cycle use 0, SCID MSB, cycle 0000
    head = bytes([b0, b1]) + count.to_bytes(3, "big") + bytes([b5])
    fhec = aos_fhec(bytes([b0, b1, b5]))
    body = head + fhec + fhp.to_bytes(2, "big") + zone + ocf
    frame = body + crc16(body).to_bytes(2, "big")
    if len(frame) != AOS_LEN:
        fail("AOS frame length")
    return frame


def uslp_header(sd, vcid, total, bypass, pcc, ocf_flag, cnt_len, map_id=0):
    b0 = 0xC0 | (SCID >> 12)
    b1 = (SCID >> 4) & 0xFF
    b2 = ((SCID & 0xF) << 4) | (sd << 3) | (vcid >> 3)
    b3 = ((vcid & 7) << 5) | (map_id << 1)      # End of Frame Primary Header Flag 0
    b6 = (bypass << 7) | (pcc << 6) | (ocf_flag << 3) | cnt_len
    return bytes([b0, b1, b2, b3]) + (total - 1).to_bytes(2, "big") + bytes([b6])


def uslp_down_data(vc, count, fhp, zone, ocf):
    if len(zone) != USLP_ZONE:
        fail("USLP zone length")
    hdr = uslp_header(0, vc, USLP_LEN, 1, 0, 1, 1)
    body = hdr + bytes([count]) + bytes([(0b000 << 5) | UPID_PACKETS]) + fhp.to_bytes(2, "big") + zone + ocf
    frame = body + crc16(body).to_bytes(2, "big")
    if len(frame) != USLP_LEN:
        fail("USLP frame length")
    return frame


def uslp_down_oid(zone, ocf):
    if len(zone) != USLP_ZONE + 1:
        fail("OID zone length")
    hdr = uslp_header(0, 63, USLP_LEN, 1, 0, 1, 0)
    lvop = len(zone) - 1
    body = hdr + bytes([(0b001 << 5) | UPID_IDLE]) + lvop.to_bytes(2, "big") + zone + ocf
    frame = body + crc16(body).to_bytes(2, "big")
    if len(frame) != USLP_LEN:
        fail("OID frame length")
    return frame


def tc_frame(kind, ns, seg_hdr, data):
    """TC frame (232.0 4.1): Type-AD only here."""
    if kind != "AD":
        raise NotModelled("TC frame kind " + kind)
    total = 5 + 1 + len(data) + 2
    b0 = (SCID >> 8) & 3       # version 00, Bypass 0, Control Command 0
    b1 = SCID & 0xFF
    b2 = (0 << 2) | ((total - 1) >> 8)     # VCID 0
    b3 = (total - 1) & 0xFF
    body = bytes([b0, b1, b2, b3, ns, seg_hdr]) + data
    return body + crc16(body).to_bytes(2, "big")


def uslp_up_frame(kind, ns, tfdz):
    if kind != "AD":
        raise NotModelled("USLP frame kind " + kind)
    total = 7 + 1 + 1 + len(tfdz) + 2
    if total > USLP_UP_MAX:
        raise NotModelled("USLP uplink frame beyond 512 octets")
    body = uslp_header(1, 0, total, 0, 0, 0, 1) + bytes([ns]) + bytes([(0b111 << 5) | UPID_PACKETS]) + tfdz
    return body + crc16(body).to_bytes(2, "big")


# --- decoders used by the receiving nodes ---

def tc_decode(frame):
    if len(frame) < 8:
        raise NotModelled("short TC frame")
    if crc16(frame) != 0:
        raise NotModelled("TC FECF")
    if frame[0] >> 6 != 0 or (((frame[0] & 3) << 8) | frame[1]) != SCID:
        raise NotModelled("TC version or SCID")
    bypass, ctl = (frame[0] >> 5) & 1, (frame[0] >> 4) & 1
    if (bypass, ctl) != (0, 0):
        raise NotModelled("TC frame other than Type-AD")
    if frame[2] >> 2 != 0:
        raise NotModelled("TC VCID")
    if (((frame[2] & 3) << 8) | frame[3]) != len(frame) - 1:
        raise NotModelled("TC length field")
    return {"ns": frame[4], "flags": frame[5] >> 6, "map": frame[5] & 0x3F, "data": frame[6:-2]}


def uslp_up_decode(frame):
    if len(frame) < 12:
        raise NotModelled("short USLP frame")
    if crc16(frame) != 0:
        raise NotModelled("USLP FECF")
    if frame[0] >> 4 != 0b1100 or ((((frame[0] & 15) << 12) | (frame[1] << 4) | (frame[2] >> 4))) != SCID:
        raise NotModelled("USLP version or SCID")
    sd = (frame[2] >> 3) & 1
    vcid = ((frame[2] & 7) << 3) | (frame[3] >> 5)
    map_id = (frame[3] >> 1) & 15
    if sd != 1 or vcid != 0 or map_id != 0 or frame[3] & 1:
        raise NotModelled("USLP header fields")
    if int.from_bytes(frame[4:6], "big") != len(frame) - 1:
        raise NotModelled("USLP length field")
    if frame[6] != 0x01:
        raise NotModelled("USLP flags other than Type-AD without OCF")
    if frame[8] != (0b111 << 5) | UPID_PACKETS:
        raise NotModelled("USLP TFDF header")
    return {"ns": frame[7], "tfdz": frame[9:-2]}


def ocf_of_down(variant, frame):
    """The OCF of a downlink frame, after the strict checks of the receiver."""
    if len(frame) != 223 or crc16(frame) != 0:
        raise NotModelled("downlink FECF or length")
    if variant == "aos":
        scid8 = SCID & 0xFF
        b5 = ((SCID >> 8) & 3) << 4
        if frame[:2] != bytes([0x40 | (scid8 >> 2), ((scid8 & 3) << 6) | (frame[1] & 63)]) or frame[5] != b5:
            raise NotModelled("AOS header")
        if frame[6:8] != aos_fhec(bytes([frame[0], frame[1], frame[5]])):
            raise NotModelled("AOS FHEC")
    return frame[-6:-2]


# ---------------------------------------------------------------------------
# Packet generator (LNK-010 to LNK-014): fields, deadlines, idle fill
# ---------------------------------------------------------------------------

class Field:
    def __init__(self):
        self.data = bytearray()
        self.starts = []
        self.t_start = None
        self.timed_out = False


class Generator:
    def __init__(self, zone, deadline, min_idle, idle_fn):
        self.zone = zone
        self.deadline = deadline
        self.min_idle = min_idle
        self.idle_fn = idle_fn
        self.cur = Field()
        self.overflow = deque()    # [bytes, starts_a_packet]

    def _fill(self, now):
        f = self.cur
        while self.overflow and len(f.data) < self.zone:
            data, start = self.overflow[0]
            room = self.zone - len(f.data)
            piece = data[:room]
            if not f.data:
                f.t_start = now
            if start:
                f.starts.append(len(f.data))
            f.data += piece
            if len(piece) == len(data):
                self.overflow.popleft()
            else:
                self.overflow[0] = [data[room:], False]

    def full(self):
        return len(self.cur.data) >= self.zone

    def submit(self, pkt, now):
        if self.cur.timed_out:
            raise NotModelled("packet into a timed-out field")
        self.overflow.append([bytes(pkt), True])
        self._fill(now)

    def timeout(self, now):
        f = self.cur
        if f.data and not self.full() and now >= f.t_start + self.deadline:
            f.timed_out = True

    def ready(self):
        return self.full() or self.cur.timed_out

    def release(self, now):
        f = self.cur
        if len(f.data) < self.zone:
            rem = self.zone - len(f.data)
            f.starts.append(len(f.data))
            if rem >= self.min_idle:
                f.data += self.idle_fn(rem)
            else:
                idle = self.idle_fn(self.min_idle)
                f.data += idle[:rem]
                self.overflow.appendleft([idle[rem:], False])
        self.cur = Field()
        self._fill(now)
        return bytes(f.data), (f.starts[0] if f.starts else None)


# ---------------------------------------------------------------------------
# FARM-1 and FOP-1 (232.1): only the cells these cases reach
# ---------------------------------------------------------------------------

class Farm1:
    def __init__(self):
        self.vr = 0

    def accept_ad(self, ns):
        """E1: Type-AD, N(S) = V(R), buffer available (state S1 Open).  Any other event is not reached."""
        if ns != self.vr:
            raise NotModelled("FARM-1 E3/E4/E5: N(S) %d with V(R) %d" % (ns, self.vr))
        self.vr = (self.vr + 1) & 0xFF

    def clcw(self):
        """232.0 4.2.1; 4.5.3 of the document: type 0, version 00, status 000, COP 01, VCID 0, flags 0, N(R) = V(R)."""
        return (0x01000000 + self.vr).to_bytes(4, "big")


class Fop1:
    def __init__(self):
        self.state = "S6"
        self.vs = 0
        self.nnr = 0
        self.sent = []          # N(S) of unacknowledged frames
        self.t1_deadline = None

    def initiate_ad_without_clcw(self):
        """E23: S6 only; Accept, Initialize, Confirm; to S1."""
        if self.state != "S6":
            raise NotModelled("directive outside S6")
        self.state = "S1"

    def check_timer(self, now):
        if self.t1_deadline is not None and now >= self.t1_deadline:
            raise NotModelled("T1 expiry (E16/E17)")

    def can_accept(self):
        if self.state != "S1":
            raise NotModelled("FOP-1 request outside S1")
        if (self.vs - self.nnr) & 0xFF >= K_WINDOW:
            raise NotModelled("window full: Wait_Queue")
        return True

    def transmit(self, now):
        """A new Type-AD frame: N(S) = V(S); V(S) advances; T1 started or restarted (232.1 5.1.9.1)."""
        self.can_accept()
        ns = self.vs
        self.vs = (self.vs + 1) & 0xFF
        self.sent.append(ns)
        self.t1_deadline = now + T1_INITIAL
        return ns

    def on_clcw(self, now, word):
        if self.state != "S1":
            raise NotModelled("CLCW outside S1")
        if word & 0xFFFFFF00 != 0x01000000:
            raise NotModelled("CLCW with a flag, status, or FARM-B Counter set (E3, E4, E7, E8, E13 to E15)")
        nr = word & 0xFF
        if ((nr - self.nnr) & 0xFF) > ((self.vs - self.nnr) & 0xFF):
            raise NotModelled("CLCW N(R) outside NN(R)..V(S): E13")
        if nr == self.nnr:
            return                      # E1 (nothing outstanding) or E5 (not yet reported): ignore
        if nr == self.vs:               # E2: every outstanding frame acknowledged
            self.sent = []
            self.t1_deadline = None
            self.nnr = nr
            return
        raise NotModelled("CLCW acknowledges some frames: E6")


# ---------------------------------------------------------------------------
# Flight node
# ---------------------------------------------------------------------------

def memory_octet(a):
    return (a ^ (a >> 8)) & 0xFF


class Flight:
    def __init__(self, variant):
        self.variant = variant
        self.farm = Farm1()
        if variant == "aos":
            mk = lambda d: Generator(AOS_ZONE, d, AOS_MIN_IDLE, spp_idle)
        else:
            mk = lambda d: Generator(USLP_ZONE, d, 1, encap_idle)
        self.gens = {0: mk(DEADLINE[0]), 1: mk(DEADLINE[1])}
        self.mux = {0: None, 1: None}
        self.counts = defaultdict(int)     # per-VC frame counts
        self.oid = OidGenerator()
        self.seq = defaultdict(int)
        self.queue = []                    # (exec_us, order, func, seq, args)
        self.order = 0
        self.tables = {}
        self.executed = 0
        self.rejected = 0
        self.payload_mode = 1
        self.last_table = (0, 0)
        self.dumps_completed = 0
        self.dump = None
        self.hk_k = 0
        self.payload_n = 0
        self.reasm = {}
        self.packets = []                  # (t, apid, bytes) in generation order
        self.cmd_arrival = {}              # command sequence -> instant
        self.table_arrival = None
        self.down_meta = []
        self.stream_vc = {0: [], 1: []}

    # -- packet output --
    def _emit(self, t, vc, apid, pkt):
        self.packets.append((t, apid, pkt))
        self.gens[vc].submit(pkt, t)

    def _next_seq(self, apid):
        c = self.seq[apid]
        self.seq[apid] = (c + 1) & 0x3FFF
        return c

    def event(self, t, code, p1=0, p2=0, p3=0):
        data = b"\x1E" + tfield(t) + b"".join(x.to_bytes(2, "big") for x in (code, p1, p2, p3))
        self._emit(t, 0, 0x011, spp(0x011, 0, 1, 3, self._next_seq(0x011), data))

    # -- delivery --
    def deliver(self, t, frame):
        packets = []
        if self.variant == "aos":
            d = tc_decode(frame)
            self.farm.accept_ad(d["ns"])
            m = d["map"]
            if d["flags"] == 3:
                packets = split_packets(d["data"])
            elif d["flags"] == 1:
                self.reasm[m] = bytes(d["data"])
            elif d["flags"] == 0:
                if m not in self.reasm:
                    raise NotModelled("continuation without start")
                self.reasm[m] += d["data"]
            else:
                if m not in self.reasm:
                    raise NotModelled("end without start")
                whole = self.reasm.pop(m) + d["data"]
                packets = split_packets(whole)
                if len(packets) != 1:
                    raise NotModelled("segmented SDU is not one packet")
        else:
            d = uslp_up_decode(frame)
            self.farm.accept_ad(d["ns"])
            packets = split_packets(d["tfdz"])
        for p in packets:
            self._on_packet(t, p)

    def _on_packet(self, t, p):
        apid = apid_of(p)
        if apid == 0x080:
            func, mode = p[6], p[7]
            tt = p[8:14]
            args = p[14:]
            seq = ((p[2] & 0x3F) << 8) | p[3]
            if mode != 1:
                raise NotModelled("immediate command")
            coarse = int.from_bytes(tt[:4], "big") - T0
            frac = int.from_bytes(tt[4:], "big")
            if (frac * 1_000_000) % 65536:
                raise NotModelled("inexact time tag")
            exec_us = coarse * 1_000_000 + frac * 1_000_000 // 65536
            sizes = {1: 0, 2: 6, 3: 3, 4: 1}
            if func not in sizes or len(args) != sizes[func]:
                raise NotModelled("command rejection path")
            if exec_us < t or len(self.queue) >= 8:
                raise NotModelled("rejection at delivery")
            self.order += 1
            self.queue.append((exec_us, self.order, func, seq, bytes(args)))
            self.cmd_arrival[seq] = t
        elif apid == 0x081:
            tid = p[6]
            length = int.from_bytes(p[7:9], "big")
            body = p[9:]
            if len(body) != length:
                raise NotModelled("table length")
            self.tables = {tid: bytes(body)}
            self.table_arrival = t
        else:
            raise NotModelled("uplink APID")

    # -- applications --
    def _execute(self, t):
        due = sorted(q for q in self.queue if q[0] <= t)
        for q in due:
            self.queue.remove(q)
            _, _, func, seq, args = q
            self.executed += 1
            self.event(t, 0x0001, func, seq, 0)
            if func == 1:
                pass
            elif func == 2:
                if self.dump is not None:
                    raise NotModelled("reject: dump in progress")
                addr = int.from_bytes(args[:4], "big")
                length = int.from_bytes(args[4:6], "big")
                if not 1 <= length <= 1024:
                    raise NotModelled("dump length")
                sdu = args[:4] + args[4:6] + bytes(memory_octet(addr + i) for i in range(length))
                chunks = [sdu[i:i + 128] for i in range(0, len(sdu), 128)]
                self.dump = {"chunks": chunks, "next": 0, "due": t, "addr": addr, "len": length}
            elif func == 3:
                tid = args[0]
                crc = int.from_bytes(args[1:3], "big")
                if tid not in self.tables or crc16(self.tables[tid]) != crc:
                    raise NotModelled("TABLE_REJECTED")
                self.last_table = (tid, crc)
                self.event(t, 0x0010, tid, len(self.tables[tid]), crc)
            elif func == 4:
                if args[0] not in (0, 1):
                    raise NotModelled("payload mode argument")
                self.payload_mode = args[0]
                self.event(t, 0x0030, args[0], 0, 0)

    def _dump_step(self, t):
        d = self.dump
        if d is None or t < d["due"]:
            return
        n = len(d["chunks"])
        i = d["next"]
        flags = 3 if n == 1 else (1 if i == 0 else (2 if i == n - 1 else 0))
        data = tfield(t) + d["chunks"][i]
        self._emit(t, 1, 0x021, spp(0x021, 0, 1, flags, self._next_seq(0x021), data))
        d["next"] += 1
        d["due"] += DOWN_SLOT
        if d["next"] == n:
            self.event(t, 0x0020, d["addr"] >> 16, d["addr"] & 0xFFFF, d["len"])
            self.dumps_completed = (self.dumps_completed + 1) & 0xFFFF
            self.dump = None

    def _hk(self, t):
        k = self.hk_k
        self.hk_k += 1
        d = bytearray()
        d += k.to_bytes(4, "big")
        d += (self.executed & 0xFFFF).to_bytes(2, "big")
        d += (self.rejected & 0xFFFF).to_bytes(2, "big")
        d += bytes([len(self.queue), self.payload_mode, self.last_table[0], 0])
        d += self.last_table[1].to_bytes(2, "big")
        d += self.dumps_completed.to_bytes(2, "big")
        for j in range(8):
            d += ((1000 * (j + 1) + 3 * k) & 0xFFFF).to_bytes(2, "big")
        self._emit(t, 0, 0x010, spp(0x010, 0, 1, 3, self._next_seq(0x010), tfield(t) + bytes(d)))

    def _payload(self, t):
        n = self.payload_n
        self.payload_n += 1
        d = bytearray(n.to_bytes(4, "big"))
        for i in range(4, 400):
            d.append((n + i) & 0xFF)
        self._emit(t, 1, 0x020, spp(0x020, 0, 1, 3, self._next_seq(0x020), tfield(t) + bytes(d)))

    # -- one step --
    def step(self, t):
        """Steps 2 to 6 of 2.1 (step 1, the deliveries, is made by the caller).  Returns a frame or None."""
        self._execute(t)
        self._dump_step(t)
        if t < GEN_END and t % 1_000_000 == 0:
            self._hk(t)
        if t < GEN_END and t % 2_000_000 == 500_000 and self.payload_mode == 1:
            self._payload(t)
        if t % DOWN_SLOT != 0:
            return None
        return self._slot(t)

    def _slot(self, t):
        for vc in (0, 1):
            g = self.gens[vc]
            g.timeout(t)
            if self.mux[vc] is None and g.ready():
                self.mux[vc] = g.release(t)
        ocf = self.farm.clcw()
        vc = next((v for v in (0, 1) if self.mux[v] is not None), None)
        if self.variant == "aos":
            if vc is None:
                vc, zone, fhp = 7, bytes(AOS_ZONE), 0xFFFE
            else:
                zone, fhp = self.mux[vc]
                self.mux[vc] = None
                fhp = 0xFFFF if fhp is None else fhp
            count = self.counts[vc]
            self.counts[vc] = (count + 1) & 0xFFFFFF
            frame = aos_frame(vc, count, fhp, zone, ocf)
        else:
            if vc is None:
                zone = self.oid.take(USLP_ZONE + 1)
                vc, fhp, count = 63, None, None
                frame = uslp_down_oid(zone, ocf)
            else:
                zone, fhp = self.mux[vc]
                self.mux[vc] = None
                fhp = 0xFFFF if fhp is None else fhp
                count = self.counts[vc]
                self.counts[vc] = (count + 1) & 0xFF
                frame = uslp_down_data(vc, count, fhp, zone, ocf)
        self.down_meta.append({"vc": vc, "count": count, "fhp": fhp, "zone": zone, "ocf": ocf, "t": t})
        return frame


# ---------------------------------------------------------------------------
# Ground node
# ---------------------------------------------------------------------------

def command_packet(seq, func, exec_s, args):
    data = bytes([func, 0x01]) + tfield(exec_s * 1_000_000) + args
    return spp(0x080, 1, 0, 3, seq, data)


def table_octets(length):
    return bytes((3 * i + 1) & 0xFF for i in range(length))


def ops_script():
    """The ops script of 2.4: (release_us, kind, payload)."""
    table = table_octets(300)
    table_pkt = spp(0x081, 1, 0, 3, 0, bytes([1]) + (300).to_bytes(2, "big") + table)
    crc = crc16(table)
    return [
        (0, "directive", "initiate_ad_without_clcw"),
        (1_000_000, "packet", (0, command_packet(0, 0x01, 6, b""))),
        (2_000_000, "packet", (1, table_pkt)),
        (3_000_000, "packet", (0, command_packet(1, 0x03, 10, bytes([1]) + crc.to_bytes(2, "big")))),
        (4_000_000, "packet", (0, command_packet(2, 0x02, 12, (0x00010000).to_bytes(4, "big") + (600).to_bytes(2, "big")))),
        (5_000_000, "packet", (0, command_packet(3, 0x04, 20, bytes([0])))),
        (5_000_000, "packet", (0, command_packet(4, 0x01, 20, b""))),
        (6_000_000, "packet", (0, command_packet(5, 0x04, 24, bytes([1])))),
    ]


class Ground:
    def __init__(self, variant):
        self.variant = variant
        self.fop = Fop1()
        self.script = deque(ops_script())
        self.q0 = deque()
        self.q1 = deque()
        self.m1 = None          # (packet, offset) of a MAP 1 packet in progress
        self.up_meta = []
        self.clcw_log = []      # (t, N(R)) per downlink frame delivered

    def deliver(self, t, frame):
        ocf = ocf_of_down(self.variant, frame)
        word = int.from_bytes(ocf, "big")
        self.clcw_log.append((t, word))
        self.fop.on_clcw(t, word)

    def _next_fdu_tc(self):
        if self.m1 is None and self.q0:
            data = b""
            while self.q0 and len(data) + len(self.q0[0]) <= TC_MAX_FDU - 1:
                data += self.q0.popleft()
            return 3, 0, data
        if self.m1 is None and self.q1:
            self.m1 = [self.q1.popleft(), 0]
            first = True
        elif self.m1 is not None:
            first = False
        else:
            return None
        pkt, off = self.m1
        chunk = pkt[off:off + TC_MAX_FDU - 1]
        last = off + len(chunk) >= len(pkt)
        if first and last:
            flags = 3
        elif first:
            flags = 1
        elif last:
            flags = 2
        else:
            flags = 0
        self.m1[1] = off + len(chunk)
        if last:
            self.m1 = None
        return flags, 1, chunk

    def _next_tfdz_uslp(self):
        if not self.q0:
            return None
        data = b""
        while self.q0 and len(data) + len(self.q0[0]) <= USLP_UP_MAX_PACKET:
            data += self.q0.popleft()
        return data

    def step(self, t):
        self.fop.check_timer(t)
        while self.script and self.script[0][0] <= t:
            _, kind, payload = self.script.popleft()
            if kind == "directive":
                self.fop.initiate_ad_without_clcw()
            else:
                mp, pkt = payload
                if self.variant == "aos":
                    (self.q0 if mp == 0 else self.q1).append(pkt)
                else:
                    if len(pkt) > USLP_UP_MAX_PACKET:   # one MAP: every packet goes to MAP 0 (4.3.3)
                        raise NotModelled("USLP uplink: packet beyond 501 octets")
                    self.q0.append(pkt)
        if t % UP_SLOT != 0:
            return None
        if self.variant == "aos":
            fdu = self._next_fdu_tc()
            if fdu is None:
                return None
            flags, mp, data = fdu
            ns = self.fop.transmit(t)
            frame = tc_frame("AD", ns, (flags << 6) | mp, data)
            self.up_meta.append({"t": t, "ns": ns, "map": mp, "flags": flags, "data": data})
        else:
            tfdz = self._next_tfdz_uslp()
            if tfdz is None:
                return None
            ns = self.fop.transmit(t)
            frame = uslp_up_frame("AD", ns, tfdz)
            self.up_meta.append({"t": t, "ns": ns, "tfdz": tfdz})
        return frame


# ---------------------------------------------------------------------------
# Harness (6.1, 6.2)
# ---------------------------------------------------------------------------

class Run:
    pass


def run_case(case):
    variant = VARIANT[case]
    ground, flight = Ground(variant), Flight(variant)
    pending = []        # (arrive, send, dirrank, index, frame)
    records = []
    index = {"down": 0, "up": 0}
    sent_down, sent_up = [], []
    t = 0
    while t < END:
        due = sorted(p for p in pending if p[0] == t)
        pending = [p for p in pending if p[0] != t]
        # ground step (step 2 of 6.2): deliveries of down frames, then ops and the uplink slot
        for arrive, send, rank, idx, frame in due:
            if rank == 0:
                ground.deliver(t, frame)
        up = ground.step(t)
        if up is not None:
            i = index["up"]
            index["up"] += 1
            records.append({"t_us": t, "kind": "unit_sent", "dir": "up", "index": i, "hex": up.hex().upper()})
            sent_up.append(up)
            pending.append((t + DELAY, t, 1, i, up))
        # flight step (step 3)
        for arrive, send, rank, idx, frame in due:
            if rank == 1:
                flight.deliver(t, frame)
        down = flight.step(t)
        if down is not None:
            i = index["down"]
            index["down"] += 1
            records.append({"t_us": t, "kind": "unit_sent", "dir": "down", "index": i, "hex": down.hex().upper()})
            sent_down.append(down)
            pending.append((t + DELAY, t, 0, i, down))
        t += STEP
    r = Run()
    r.case, r.variant = case, variant
    r.records, r.ground, r.flight = records, ground, flight
    r.down, r.up = sent_down, sent_up
    return r


def to_line(rec):
    return json.dumps(rec, separators=(",", ":"))


# ---------------------------------------------------------------------------
# Compare
# ---------------------------------------------------------------------------

def compare(run, path):
    golden = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            rec = json.loads(line)
            if rec.get("kind") == "unit_sent":
                golden.append(rec)
    mism = 0
    n = max(len(golden), len(run.records))
    for i in range(n):
        m = run.records[i] if i < len(run.records) else None
        g = golden[i] if i < len(golden) else None
        if m is None or g is None:
            print("record %d: model %s, golden %s" % (i, "absent" if m is None else "present", "absent" if g is None else "present"))
            mism += 1
            continue
        if m != g:
            mism += 1
            why = []
            for k in ("t_us", "dir", "index"):
                if m[k] != g[k]:
                    why.append("%s model %r golden %r" % (k, m[k], g[k]))
            if m["hex"] != g["hex"]:
                a, b = m["hex"], g["hex"]
                off = next((j for j in range(min(len(a), len(b))) if a[j] != b[j]), min(len(a), len(b)))
                why.append("hex differs at octet %d (model %d octets, golden %d)" % (off // 2, len(a) // 2, len(b) // 2))
            print("record %d (%s %d, t=%d): %s" % (i, m["dir"], m["index"], m["t_us"], "; ".join(why)))
    print("%s: %d model records, %d golden unit_sent records, %d mismatches" % (run.case, len(run.records), len(golden), mism))
    return mism


# ---------------------------------------------------------------------------
# Self-check: the figures of 4.6.2, 4.6.3, 4.6.4 (before any golden trace)
# ---------------------------------------------------------------------------

def hx(s):
    return bytes.fromhex(s.replace(" ", ""))


def pk(run, apid, n):
    """The n-th packet of an APID, in generation order."""
    return [p for (_, a, p) in run.flight.packets if a == apid][n]


def reassemble_vc(run, vc):
    """Concatenate the zones of a VC and parse the packets: the packet projection of the downlink."""
    stream = b"".join(m["zone"] for m in run.flight.down_meta if m["vc"] == vc)
    parts = split_zone(stream)
    return [p for p in parts if not (p[0] >> 5 == 0 and apid_of(p) == IDLE_APID) and p[0] != 0xE0 and p[0] != 0xE1]


def self_check_common(run):
    f = run.flight
    # 2.4: derived values
    check(crc16(b"123456789") == 0x29B1, "CRC-16 of 123456789")
    check(crc16(table_octets(300)) == 0x0C4F, "CRC-16 of the 300 table octets")
    check(crc16(bytes(memory_octet(0x00010000 + i) for i in range(600))) == 0x80A8, "CRC-16 of the 600 dumped octets")
    check(OidGenerator().take(20) == hx("FF FF FF FF 6D B6 D8 61 45 1F 11 F1 97 16 72 3C BE 7E 00 B1"), "OID pattern")
    # packets of 2.4
    by = defaultdict(list)
    for _, a, p in f.packets:
        by[a].append(p)
    check(len(by[0x010]) == 30 and all(len(p) == 44 for p in by[0x010]), "30 HK packets of 44")
    check(len(by[0x011]) == 10 and all(len(p) == 21 for p in by[0x011]), "10 event packets of 21")
    check(len(by[0x020]) == 13 and all(len(p) == 412 for p in by[0x020]), "13 payload packets of 412")
    check([len(p) for p in by[0x021]] == [140, 140, 140, 140, 106], "5 dump packets")
    check(len(by[0x080]) == 0, "no command in the flight packets")
    check(run.ground.script == deque(), "ops script consumed")
    # the packet projection of the downlink: every packet arrives in order on its VC
    check(reassemble_vc(run, 0) == [p for (_, a, p) in f.packets if a in (0x010, 0x011)], "VC 0 packets")
    check(reassemble_vc(run, 1) == [p for (_, a, p) in f.packets if a in (0x020, 0x021)], "VC 1 packets")
    # events of 3.3.2 in 2.4 order (counts by instant)
    ev_times = [t for (t, a, p) in f.packets if a == 0x011]
    check(ev_times == [6_000_000, 10_000_000, 10_000_000, 12_000_000, 12_500_000, 20_000_000, 20_000_000, 20_000_000, 24_000_000, 24_000_000], "event instants")
    # arrival of the commands (4.6.4)
    arr = f.cmd_arrival
    exp = {"aos": [1.010, 3.010, 4.010, 5.010, 5.010, 6.010], "uslp": [1.010, 3.010, 4.010, 5.010, 5.010, 6.010]}[run.variant]
    check([arr[s] / 1e6 for s in (0, 1, 2, 3, 4, 5)] == exp, "command arrival instants")
    check(f.table_arrival == {"aos": 2_510_000, "uslp": 2_010_000}[run.variant], "table-upload arrival")
    for s, ex in zip(range(6), (6.0, 10.0, 12.0, 20.0, 20.0, 24.0)):
        check(ex - arr[s] / 1e6 >= 4.990 - 1e-9, "margin of command %d" % s)
    # HK content at the end of the run: state 3.3.1
    hk29 = by[0x010][29][12:]
    check(hk29[0:4] == (29).to_bytes(4, "big"), "HK number")
    check(int.from_bytes(hk29[4:6], "big") == 6 and hk29[8] == 0 and hk29[9] == 1 and hk29[10] == 1, "HK state at 29 s")
    check(hk29[12:14] == (0x0C4F).to_bytes(2, "big") and int.from_bytes(hk29[14:16], "big") == 1, "HK table CRC and dumps")
    # the downlink frames
    check(len(run.down) == 280 and len(run.records) == 280 + len(run.up), "280 downlink frames")
    check(all(len(fr) == 223 and crc16(fr) == 0 for fr in run.down), "frame lengths and FECF")
    check([r["t_us"] for r in run.records if r["dir"] == "down"] == [i * 125_000 for i in range(280)], "downlink instants")
    # the records are in trace order: time, the ground unit before the flight unit at one instant
    keys = [(r["t_us"], 0 if r["dir"] == "up" else 1) for r in run.records]
    check(keys == sorted(keys), "trace order")
    # CLCW: N(R) = number of frames accepted, FARM-B Counter 0
    for m in f.down_meta:
        w = int.from_bytes(m["ocf"], "big")
        check(w & 0xFFFFFF00 == 0x01000000, "OCF flags")
    # T1 margin: every acknowledgement 135 000 us after its frame
    ack = {}
    for t, w in run.ground.clcw_log:
        ack.setdefault(w & 0xFF, t)
    for u, nr in zip(run.ground.up_meta, range(1, 99)):
        check(ack[nr] - u["t"] == 135_000, "acknowledgement 135 000 us after frame %d" % (nr - 1))


def clcw_changes(run):
    out = []
    prev = 0x01000000
    for i, m in enumerate(run.flight.down_meta):
        w = int.from_bytes(m["ocf"], "big")
        if w != prev:
            out.append((i, w))
            prev = w
    return out


def self_check_aos():
    run = run_case("a4-aos-nominal")
    self_check_common(run)
    f, m = run.flight, run.flight.down_meta
    d = run.down
    # FHEC of 4.6.2, independent of the VC counts
    check(aos_fhec(hx("55 40 10")) == hx("A5 79"), "FHEC VC 0")
    check(aos_fhec(hx("55 41 10")) == hx("87 67"), "FHEC VC 1")
    check(aos_fhec(hx("55 47 10")) == hx("4B 05"), "FHEC VC 7")
    # heads
    check(d[0][:10] == hx("55 47 00 00 00 10 4B 05 FF FE"), "frame 0 head")
    check(d[2][:10] == hx("55 40 00 00 00 10 A5 79 00 00"), "frame 2 head")
    check(d[4][:10] == hx("55 41 00 00 00 10 87 67 00 00"), "frame 4 head")
    # totals
    tot = {v: sum(1 for x in m if x["vc"] == v) for v in (0, 1, 7)}
    check(tot == {0: 31, 1: 37, 7: 212}, "frames per VC %r" % tot)
    last = {v: max(x["count"] for x in m if x["vc"] == v) for v in (0, 1, 7)}
    check(last == {0: 30, 1: 36, 7: 211}, "final counts %r" % last)
    check(all(x["fhp"] == 0xFFFE for x in m if x["vc"] == 7), "VC 7 pointer")
    check(all(x["zone"] == bytes(207) for x in m if x["vc"] == 7), "VC 7 zone")
    check([x["count"] for x in m if x["vc"] == 7] == list(range(212)), "VC 7 counts")
    for v in (0, 1, 7):
        check([x["count"] for x in m if x["vc"] == v] == list(range(tot[v])), "dense VC %d counts" % v)
    # VC 0 frames
    ev_at = {6: 1, 12: 1, 10: 2, 24: 2, 20: 3}
    idle_tail = {163: "9C", 142: "87", 121: "72", 100: "5D", 186: "B3"}
    vc0 = [i for i, x in enumerate(m) if x["vc"] == 0]
    check(vc0 == sorted([8 * k + 2 for k in range(30)] + [102]), "VC 0 frame indices")
    for i in vc0:
        z = m[i]["zone"]
        check(m[i]["fhp"] == 0, "VC 0 FHP, frame %d" % i)
        if i == 102:
            n_idle = 186
        else:
            k = (i - 2) // 8
            n_idle = 207 - 44 - 21 * ev_at.get(k, 0)
        check(n_idle in idle_tail, "idle length %d" % n_idle)
        head = z[207 - n_idle:207 - n_idle + 6]
        check(head == hx("07 FF C0 00 00 " + idle_tail[n_idle]), "idle head, frame %d" % i)
        check(z[207 - n_idle + 6:] == bytes(n_idle - 6), "idle data, frame %d" % i)
    # VC 0 counts
    for k in range(30):
        i = 8 * k + 2
        want = k if k <= 12 else k + 1
        check(m[i]["count"] == want, "VC 0 count in frame %d" % i)
    check(m[102]["count"] == 13, "VC 0 count in frame 102")
    # VC 1 payload table of 4.6.2
    rows = {
        0: [(4, 0), (12, 1)], 1: [(20, 2), (21, 3), (29, 4)], 2: [(36, 5), (44, 6)],
        3: [(52, 7), (53, 8), (61, 9)], 4: [(68, 10), (76, 11)], 5: [(84, 12), (85, 13), (93, 14)],
        6: [(101, 18), (103, 19), (110, 20)], 7: [(116, 21), (124, 22)], 8: [(132, 23), (133, 24), (141, 25)],
        9: [(148, 26), (156, 27), (164, 28)], 10: [(196, 29), (204, 30)], 11: [(212, 31), (213, 32), (221, 33)],
        12: [(228, 34), (236, 35), (244, 36)],
    }
    kind = {0: "E", 1: "O", 2: "E", 3: "O", 4: "E", 5: "O", 7: "E", 8: "O", 9: "E", 10: "E", 11: "O", 12: "E"}
    vc1 = [i for i, x in enumerate(m) if x["vc"] == 1]
    in_rows = sorted(i for fr in rows.values() for i, _ in fr)
    window_extra = [97, 99, 100]
    check(vc1 == sorted(in_rows + window_extra), "VC 1 frame indices")
    for n, fr in rows.items():
        for i, c in fr:
            check(m[i]["vc"] == 1 and m[i]["count"] == c, "payload %d: frame %d count" % (n, i))
        if n == 6:
            continue
        p = pk(run, 0x020, n)
        z = [m[i]["zone"] for i, _ in fr]
        fh = [m[i]["fhp"] for i, _ in fr]
        if kind[n] == "E":
            check(z[0] == p[:207] and fh[0] == 0, "payload %d frame 1" % n)
            check(z[1] == p[207:] + hx("07 FF") and fh[1] == 205, "payload %d frame 2" % n)
            if len(fr) == 3:      # the 5 octets that wait for the next packet, released by their deadline
                check(z[2] == hx("C0 00 00 00 00") + hx("07 FF C0 00 00 C3") + bytes(196) and fh[2] == 5, "idle of 202, frame %d" % fr[2][0])
        else:
            five = hx("C0 00 00 00 00")
            check(z[0] == five + p[:202] and fh[0] == 5, "payload %d frame 1 (O)" % n)
            check(z[1] == p[202:409] and fh[1] == 0xFFFF, "payload %d frame 2 (O)" % n)
            check(z[2] == p[409:] + hx("07 FF C0 00 00 C5") + bytes(198) and fh[2] == 3, "payload %d frame 3 (O)" % n)
    check(rows[9][2][0] == 164 and rows[12][2][0] == 244, "frames of the 5-octet idle")
    # the window of 4.6.2
    dmp = [pk(run, 0x021, i) for i in range(5)]
    p6 = pk(run, 0x020, 6)
    check(m[97]["zone"] == dmp[0] + dmp[1][:67] and m[97]["fhp"] == 0 and m[97]["count"] == 15, "frame 97")
    check(m[99]["zone"] == dmp[1][67:] + dmp[2][:134] and m[99]["fhp"] == 73 and m[99]["count"] == 16, "frame 99")
    check(m[100]["zone"] == dmp[2][134:] + dmp[3] + dmp[4][:61] and m[100]["fhp"] == 6 and m[100]["count"] == 17, "frame 100")
    check(m[101]["zone"] == dmp[4][61:] + p6[:162] and m[101]["fhp"] == 45 and m[101]["count"] == 18, "frame 101")
    check(m[103]["zone"] == p6[162:369] and m[103]["fhp"] == 0xFFFF and m[103]["count"] == 19, "frame 103")
    check(m[110]["zone"] == p6[369:] + spp_idle(164) and m[110]["fhp"] == 43 and m[110]["count"] == 20, "frame 110")
    ev = [p for (_, a, p) in f.packets if a == 0x011]
    hk12 = pk(run, 0x010, 12)
    check(m[98]["zone"] == ev[3] + hk12 + spp_idle(142) and m[98]["count"] == 12, "frame 98")
    check(m[102]["zone"] == ev[4] + spp_idle(186) and m[102]["count"] == 13, "frame 102")
    # CLCW changes
    ch = clcw_changes(run)
    check(ch == [(9, 0x01000001), (17, 0x01000002), (19, 0x01000003), (21, 0x01000004), (25, 0x01000005),
                 (33, 0x01000006), (41, 0x01000007), (49, 0x01000008)], "CLCW changes %r" % ch)
    check(m[21]["vc"] == 1, "frame 21 is a data frame on VC 1")
    for i in (9, 17, 19, 21, 25, 33, 41, 49):
        check(d[i][-6:-2] == m[i]["ocf"], "OCF position")
    # uplink: eight Type-AD frames of 4.5.5, with the lengths of 4.4.4
    u = run.up
    want = [
        (1_000_000, 0, 22, "01 55 00 15 00", "C0"),
        (2_000_000, 1, 128, "01 55 00 7F 01", "41"),
        (2_250_000, 2, 128, "01 55 00 7F 02", "01"),
        (2_500_000, 3, 77, "01 55 00 4C 03", "81"),
        (3_000_000, 4, 25, "01 55 00 18 04", "C0"),
        (4_000_000, 5, 28, "01 55 00 1B 05", "C0"),
        (5_000_000, 6, 37, "01 55 00 24 06", "C0"),
        (6_000_000, 7, 23, "01 55 00 16 07", "C0"),
    ]
    check(len(u) == 8, "eight uplink frames")
    for idx, (t, ns, ln, head, seg) in enumerate(want):
        r = [x for x in run.records if x["dir"] == "up" and x["index"] == idx][0]
        check(r["t_us"] == t and len(u[idx]) == ln, "uplink %d instant and length" % idx)
        check(u[idx][:5] == hx(head) and u[idx][5:6] == hx(seg), "uplink %d head" % idx)
        check(crc16(u[idx]) == 0, "uplink %d FECF" % idx)
    # the two commands of 5.0 s share one FDU of 30 octets
    check(len(run.ground.up_meta[6]["data"]) == 29, "blocked commands")
    check(run.ground.up_meta[1]["data"] + run.ground.up_meta[2]["data"] + run.ground.up_meta[3]["data"] == pk_up_table(),
          "table upload segments")
    return run


def pk_up_table():
    t = table_octets(300)
    return spp(0x081, 1, 0, 3, 0, bytes([1]) + (300).to_bytes(2, "big") + t)


def self_check_uslp():
    run = run_case("a4-uslp-nominal")
    self_check_common(run)
    f, m, d = run.flight, run.flight.down_meta, run.down
    # heads of 4.6.3
    check(d[0][:20] == hx("C0 15 57 E0 00 DE 88 3F 00 CE FF FF FF FF 6D B6 D8 61 45 1F"), "frame 0 (OID) head")
    check(d[2][:11] == hx("C0 15 50 00 00 DE 89 00 00 00 00"), "frame 2 head")
    check(d[4][:11] == hx("C0 15 50 20 00 DE 89 00 00 00 00"), "frame 4 head")
    check(d[5][:11] == hx("C0 15 50 20 00 DE 89 01 00 FF FF"), "frame 5 head")
    n_oid = sum(1 for x in m if x["vc"] == 63)
    check(n_oid == 219 and sum(1 for x in m if x["vc"] != 63) == 61, "61 data frames and 219 OID frames")
    check(sum(1 for x in m if x["vc"] == 0) == 31 and sum(1 for x in m if x["vc"] == 1) == 30, "31 and 30 data frames")
    check(max(x["count"] for x in m if x["vc"] == 0) == 30 and max(x["count"] for x in m if x["vc"] == 1) == 29, "final VCF counts")
    # OID frames: the m-th carries octets 207m..207m+206 of the sequence, never restarted
    seq = OidGenerator().take(219 * 207)
    oids = [x for x in m if x["vc"] == 63]
    for j, x in enumerate(oids):
        check(x["zone"] == seq[207 * j:207 * j + 207], "OID zone %d" % j)
    check([i for i, x in enumerate(m) if x["vc"] == 63][:2] == [0, 1], "frames 0 and 1 are OID")
    for i in (0, 77, 279):
        fr = d[i]
        if m[i]["vc"] == 63:
            check(fr[7:10] == hx("3F 00 CE") and fr[6] == 0x88, "OID TFDF header, frame %d" % i)
    # VC 0
    ev_at = {6: 1, 12: 1, 10: 2, 24: 2, 20: 3}
    idle_len = {162: 0xA2, 141: 0x8D, 120: 0x78, 99: 0x63, 185: 0xB9}
    vc0 = [i for i, x in enumerate(m) if x["vc"] == 0]
    check(vc0 == sorted([8 * k + 2 for k in range(30)] + [102]), "VC 0 frame indices")
    for i in vc0:
        z = m[i]["zone"]
        check(m[i]["fhp"] == 0, "VC 0 FHP")
        if i == 102:
            n_idle = 185
        else:
            k = (i - 2) // 8
            n_idle = 206 - 44 - 21 * ev_at.get(k, 0)
        check(n_idle in idle_len, "idle length %d" % n_idle)
        check(z[206 - n_idle:206 - n_idle + 2] == bytes([0xE1, idle_len[n_idle]]) and z[206 - n_idle + 2:] == bytes(n_idle - 2), "idle packet, frame %d" % i)
    for k in range(30):
        i = 8 * k + 2
        check(m[i]["count"] == (k if k <= 12 else k + 1), "VC 0 count, frame %d" % i)
    check(m[102]["count"] == 13, "VC 0 count in frame 102")
    # VC 1 payload table of 4.6.3
    rows = {0: (4, 0), 1: (20, 2), 2: (36, 4), 3: (52, 6), 4: (68, 8), 5: (84, 10), 7: (116, 18), 8: (132, 20),
            9: (148, 22), 10: (196, 24), 11: (212, 26), 12: (228, 28)}
    for n, (i0, c0) in rows.items():
        p = pk(run, 0x020, n)
        check(m[i0]["vc"] == 1 and m[i0]["count"] == c0 and m[i0]["zone"] == p[:206] and m[i0]["fhp"] == 0, "payload %d first" % n)
        check(m[i0 + 1]["vc"] == 1 and m[i0 + 1]["count"] == c0 + 1 and m[i0 + 1]["zone"] == p[206:] and m[i0 + 1]["fhp"] == 0xFFFF, "payload %d second" % n)
    dmp = [pk(run, 0x021, i) for i in range(5)]
    p6 = pk(run, 0x020, 6)
    check(m[97]["zone"] == dmp[0] + dmp[1][:66] and m[97]["fhp"] == 0 and m[97]["count"] == 12, "frame 97")
    check(m[99]["zone"] == dmp[1][66:] + dmp[2][:132] and m[99]["fhp"] == 74 and m[99]["count"] == 13, "frame 99")
    check(m[100]["zone"] == dmp[2][132:] + dmp[3] + dmp[4][:58] and m[100]["fhp"] == 8 and m[100]["count"] == 14, "frame 100")
    check(m[101]["zone"] == dmp[4][58:] + p6[:158] and m[101]["fhp"] == 48 and m[101]["count"] == 15, "frame 101")
    check(m[103]["zone"] == p6[158:364] and m[103]["fhp"] == 0xFFFF and m[103]["count"] == 16, "frame 103")
    check(m[110]["zone"] == p6[364:] + encap_idle(158) and m[110]["fhp"] == 48 and m[110]["count"] == 17, "frame 110")
    ev = [p for (_, a, p) in f.packets if a == 0x011]
    check(m[98]["zone"] == ev[3] + pk(run, 0x010, 12) + encap_idle(141) and m[98]["count"] == 12, "frame 98")
    check(m[102]["zone"] == ev[4] + encap_idle(185) and m[102]["count"] == 13, "frame 102")
    check([i for i, x in enumerate(m) if x["vc"] == 1] == sorted(
        [r for n, (a, _) in rows.items() for r in (a, a + 1)] + [97, 99, 100, 101, 103, 110]), "VC 1 frame indices")
    # CLCW
    ch = clcw_changes(run)
    check(ch == [(9, 0x01000001), (17, 0x01000002), (25, 0x01000003), (33, 0x01000004), (41, 0x01000005), (49, 0x01000006)], "CLCW changes %r" % ch)
    for i in (9, 17, 25, 33, 41, 49):
        check(d[i][-6:-2] == m[i]["ocf"], "OCF position")
    # uplink frames of 4.6.3
    u = run.up
    want = [
        (1_000_000, 0, 25, "C0 15 58 00 00 18 01 00 E0"),
        (2_000_000, 1, 320, "C0 15 58 00 01 3F 01 01 E0"),
        (3_000_000, 2, 28, "C0 15 58 00 00 1B 01 02 E0"),
        (4_000_000, 3, 31, "C0 15 58 00 00 1E 01 03 E0"),
        (5_000_000, 4, 40, "C0 15 58 00 00 27 01 04 E0"),
        (6_000_000, 5, 26, "C0 15 58 00 00 19 01 05 E0"),
    ]
    check(len(u) == 6, "six uplink frames")
    for idx, (t, ns, ln, head) in enumerate(want):
        r = [x for x in run.records if x["dir"] == "up" and x["index"] == idx][0]
        check(r["t_us"] == t and len(u[idx]) == ln, "uplink %d instant and length" % idx)
        check(u[idx][:9] == hx(head), "uplink %d head" % idx)
        check(crc16(u[idx]) == 0, "uplink %d FECF" % idx)
    check(run.ground.up_meta[1]["tfdz"] == pk_up_table(), "the table upload is one TFDZ")
    return run


def self_check():
    self_check_aos()
    self_check_uslp()
    print("self-check ok: a4-aos-nominal (288 records) and a4-uslp-nominal (286 records)")


# ---------------------------------------------------------------------------

def main(argv):
    if "--self-check" in argv:
        self_check()
        return 0
    args = [a for a in argv if not a.startswith("--")]
    case = next((a for a in args if a in CASES), None)
    if "--compare" in argv:
        path = argv[argv.index("--compare") + 1]
        if case is None:
            print("usage: a4_cases.py --compare <golden.jsonl> <case>", file=sys.stderr)
            return 2
        return 1 if compare(run_case(case), path) else 0
    if case is None:
        print("usage: a4_cases.py <case> | --self-check | --compare <golden.jsonl> <case>", file=sys.stderr)
        return 2
    for rec in run_case(case).records:
        print(to_line(rec))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
