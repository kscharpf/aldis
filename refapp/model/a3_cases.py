#!/usr/bin/env python3
"""Independent model of A3 decision D8(c) (TV-023): `a3-nominal` and `a3-uplink-loss`.

Regenerates every `unit_sent` record (instant, direction, index, octets) of the two
cases: the TM downlink frames (packet generators, fixed-priority multiplexer, OID
frames, the CLCW in the OCF, FECF) and the TC uplink frames (Type-AD, Segment Header,
FECF) with the FOP-1 behaviour of CCSDS 232.1-B-2 Table 5-1 and the FARM-1 behaviour
of Table 6-1.

Written blind, from `docs/reference-application.md` (sections 2 to 7), the milestone
plan `docs/milestones/a3-cop1.md` (D5, D8), and the Blue Books 232.0-B-4, 232.1-B-2,
132.0-B-3, 133.0-B-2, and 301.0-B-4, without sight of any crate, refapp node, existing
model, or golden trace. The scenario file `refapp/scenario/scenario.json` is not read:
every value is taken from the document tables and written down here.

Usage (Python 3, standard library only, no CI step):

    python3 -I refapp/model/a3_cases.py --self-check
    python3 -I refapp/model/a3_cases.py --emit a3-uplink-loss
    python3 -I refapp/model/a3_cases.py --compare refapp/golden/a3-nominal.jsonl a3-nominal

Only the cells the two cases reach are modelled. A cell outside them (an Alert, Lockout,
a Type-BC frame, Wait, a rejected frame) raises `NotModelled` and stops the run, so the
model never guesses.
"""

import argparse
import heapq
import json
import sys

# ----------------------------------------------------------------------------
# Values from docs/reference-application.md (section references in comments)
# ----------------------------------------------------------------------------

SCID = 0x155  # 3.4
T0_COARSE = 2_100_000_000  # 5
END_US = 35_000_000  # 6.8: end instant 35 s
DOWN_SLOT_US = 125_000  # 6.1
UP_SLOT_US = 250_000  # 6.1
DELAY_US = 10_000  # 6.1

TM_FRAME_LEN = 223  # 4.1
TM_DATA_LEN = 211  # 4.1: 223 - 6 - 4 - 2
VC_DEADLINE_US = {0: 250_000, 1: 1_000_000}  # 4.1

TC_MAX_FDU = 121  # 4.2: Segment Header included
FOP_K = 4  # 4.2
FOP_LIMIT = 3  # 4.2: Transmission_Limit
FOP_T1_US = 1_000_000  # 4.2: T1_Initial
FARM_PW = 4  # 4.2: W = 8, PW = NW = 4
FARM_NW = 4

CLCW_BASE = 0x01000000  # 4.4.1, 4.5.3


class NotModelled(Exception):
    """A cell or path the two modelled cases do not reach."""


# ----------------------------------------------------------------------------
# CRC-16 (132.0 4.1.6.2.2, 232.0 4.1.4): poly 0x1021, initial 0xFFFF, no reflection
# ----------------------------------------------------------------------------


def crc16(data):
    reg = 0xFFFF
    for byte in data:
        reg ^= byte << 8
        for _ in range(8):
            reg = ((reg << 1) ^ 0x1021) & 0xFFFF if reg & 0x8000 else (reg << 1) & 0xFFFF
    return reg


# ----------------------------------------------------------------------------
# OID data: 32-cell LFSR, polynomial D0 + D1 + D2 + D22 + D32, Fibonacci form,
# all-ones seed, never restarted (132.0 4.1.4.6.2, Annex D). Bit n of the stream
# is s[n] = s[n-32] ^ s[n-22] ^ s[n-2] ^ s[n-1]; the first 32 bits are the seed.
# ----------------------------------------------------------------------------


class OidGenerator:
    def __init__(self):
        self.window = [1] * 32
        self.pos = 0  # bits of the seed still to be output

    def next_bit(self):
        if self.pos < 32:
            bit = self.window[self.pos]
            self.pos += 1
            return bit
        w = self.window
        bit = w[-32] ^ w[-22] ^ w[-2] ^ w[-1]
        w.append(bit)
        del w[0]
        return bit

    def next_octets(self, n):
        out = bytearray()
        for _ in range(n):
            v = 0
            for _ in range(8):
                v = (v << 1) | self.next_bit()
            out.append(v)
        return bytes(out)


# ----------------------------------------------------------------------------
# Space Packets and time (133.0 4.1.3; 301.0 3.2; reference-application 3, 5)
# ----------------------------------------------------------------------------


def cuc_tfield(us_after_t0):
    coarse = T0_COARSE + us_after_t0 // 1_000_000
    frac = ((us_after_t0 % 1_000_000) * 65536) // 1_000_000
    if ((us_after_t0 % 1_000_000) * 65536) % 1_000_000 != 0:
        raise NotModelled("time off the 125 ms grid")
    return coarse.to_bytes(4, "big") + frac.to_bytes(2, "big")


def space_packet(apid, ptype, sec_hdr, seq_flags, count, data):
    word0 = (0 << 13) | (ptype << 12) | (sec_hdr << 11) | apid
    word1 = (seq_flags << 14) | (count & 0x3FFF)
    return (
        word0.to_bytes(2, "big")
        + word1.to_bytes(2, "big")
        + (len(data) - 1).to_bytes(2, "big")
        + data
    )


def idle_packet(total_len):
    # APID 2047, Sequence Flags 11, count 0, idle data 0x00 (4.4.4, 4.1 table)
    if total_len < 7:
        raise NotModelled("idle packet shorter than 7 octets")
    return space_packet(0x7FF, 0, 0, 3, 0, bytes(total_len - 6))


def table_octets(length):
    return bytes((3 * i + 1) % 256 for i in range(length))  # 3.3.6


# ----------------------------------------------------------------------------
# Flight node applications (2.1, 2.3, 3.3)
# ----------------------------------------------------------------------------

FN_NOOP, FN_DUMP, FN_ACTIVATE, FN_SETPL = 1, 2, 3, 4


class Flight:
    def __init__(self):
        self.pkt_counts = {}
        self.queue = []  # (exec_us, function, args, seq, delivery order)
        self.order = 0
        self.executed = 0
        self.rejected = 0
        self.payload_on = True
        self.table = None  # (id, octets)
        self.active_table_id = 0
        self.active_table_crc = 0
        self.dumps_completed = 0
        self.dump = None  # dict(next_us, chunks, index)
        self.hk_k = 0
        self.payload_n = 0
        # downlink link layer
        self.gens = {0: VcGenerator(0), 1: VcGenerator(1)}
        self.mux = {0: None, 1: None}
        self.mc_count = 0
        self.vc_count = {0: 0, 1: 0, 7: 0}
        self.oid = OidGenerator()
        # FARM-1 (232.1 6)
        self.vr = 0
        self.retransmit = 0
        self.lockout = 0
        self.farm_b = 0
        self.reasm_map1 = None  # bytearray of the packet in progress
        self.ocf = CLCW_BASE.to_bytes(4, "big")

    # --- packets -----------------------------------------------------------
    def count_for(self, apid):
        c = self.pkt_counts.get(apid, 0)
        self.pkt_counts[apid] = (c + 1) % 16384
        return c

    def tm_packet(self, now, apid, explicit, seq_flags, user):
        sec = (b"\x1e" if explicit else b"") + cuc_tfield(now)
        return space_packet(apid, 0, 1, seq_flags, self.count_for(apid), sec + user)

    def event(self, now, code, p1=0, p2=0, p3=0):
        user = b"".join(x.to_bytes(2, "big") for x in (code, p1, p2, p3))
        self.gens[0].submit(now, self.tm_packet(now, 0x011, True, 3, user))

    # --- uplink delivery -> commands (2.3, 3.3.5, 3.3.6) -----------------------
    def accept_fdu(self, now, fdu):
        """FDU = Segment Header octet + data. Reassemble per 232.0 4.3 and 4.4."""
        seg, data = fdu[0], fdu[1:]
        flags, map_id = seg >> 6, seg & 0x3F
        if map_id == 0:
            if flags != 3:
                raise NotModelled("segmented MAP 0")
            pos = 0
            while pos < len(data):
                plen = int.from_bytes(data[pos + 4 : pos + 6], "big") + 1 + 6
                self.handle_packet(now, data[pos : pos + plen])
                pos += plen
        elif map_id == 1:
            if flags == 1:
                if self.reasm_map1 is not None:
                    raise NotModelled("sequence violation")
                self.reasm_map1 = bytearray(data)
            elif flags in (0, 2):
                if self.reasm_map1 is None:
                    raise NotModelled("sequence violation")
                self.reasm_map1 += data
                if flags == 2:
                    pkt, self.reasm_map1 = bytes(self.reasm_map1), None
                    plen = int.from_bytes(pkt[4:6], "big") + 7
                    if plen != len(pkt):
                        raise NotModelled("length mismatch")
                    self.handle_packet(now, pkt)
            else:
                raise NotModelled("MAP 1 single segment")
        else:
            raise NotModelled("unknown MAP")

    def handle_packet(self, now, pkt):
        apid = ((pkt[0] & 0x07) << 8) | pkt[1]
        user = pkt[6:]
        if apid == 0x081:
            tid = user[0]
            length = int.from_bytes(user[1:3], "big")
            octets = user[3 : 3 + length]
            if len(octets) != length:
                raise NotModelled("short table")
            self.table = (tid, octets)
        elif apid == 0x080:
            seq = int.from_bytes(pkt[2:4], "big") & 0x3FFF
            func, mode = user[0], user[1]
            if mode != 1:
                raise NotModelled("immediate command")
            exec_us = (
                int.from_bytes(user[2:6], "big") - T0_COARSE
            ) * 1_000_000 + (int.from_bytes(user[6:8], "big") * 1_000_000) // 65536
            if exec_us < now:
                raise NotModelled("time tag in the past")
            if len(self.queue) >= 8:
                raise NotModelled("queue full")
            self.queue.append((exec_us, func, user[8:], seq, self.order))
            self.order += 1
        else:
            raise NotModelled("unknown APID")

    # --- FARM-1 (232.1 6.1.8.3.1, Table 6-1; 232.0 4.1.2, 4.4.9) --------------
    def receive_tc_frame(self, now, frame):
        if len(frame) < 8 or crc16(frame) != 0 or (frame[2] & 0x03) << 8 | frame[3]:
            pass
        if crc16(frame) != 0:
            raise NotModelled("FECF failure")
        flen = (((frame[2] & 0x03) << 8) | frame[3]) + 1
        if flen != len(frame):
            raise NotModelled("length mismatch")
        bypass = (frame[0] >> 5) & 1
        ctrl = (frame[0] >> 4) & 1
        scid = ((frame[0] & 0x03) << 8) | frame[1]
        vcid = frame[2] >> 2
        if scid != SCID or vcid != 0 or (frame[0] >> 6) != 0:
            raise NotModelled("frame for another channel")
        if bypass or ctrl:
            raise NotModelled("Type-B frame")
        ns = frame[4]
        if self.lockout:
            return  # E1..E5 in Lockout: discard
        diff = (ns - self.vr) % 256
        if diff == 0:  # E1 (a buffer is always available, 4.5.3)
            self.vr = (self.vr + 1) % 256
            self.retransmit = 0
            self.accept_fdu(now, frame[5:-2])
        elif 1 <= diff <= FARM_PW - 1:  # E3
            self.retransmit = 1
        elif 1 <= (self.vr - ns) % 256 <= FARM_NW:  # E4
            pass
        else:  # E5
            self.lockout = 1
            raise NotModelled("Lockout (E5)")

    def clcw_word(self):
        return (
            CLCW_BASE
            + 0x2000 * self.lockout
            + 0x0800 * self.retransmit
            + 0x0200 * (self.farm_b % 4)
            + self.vr
        )

    # --- the flight step (2.1, APP-132) ------------------------------------
    def step(self, now, tc_frames, slot):
        for fr in tc_frames:  # (1) deliveries
            self.receive_tc_frame(now, fr)
        # (2) time-tagged commands due, in queue (delivery) order
        due = [c for c in self.queue if c[0] <= now]
        self.queue = [c for c in self.queue if c[0] > now]
        for _exec, func, args, seq, _o in due:
            self.execute(now, func, args, seq)
        # (3) dump segment, then DUMP_COMPLETED
        if self.dump is not None and self.dump["next_us"] <= now:
            d = self.dump
            idx = d["index"]
            last = idx == len(d["chunks"]) - 1
            first = idx == 0
            flags = 3 if (first and last) else 1 if first else 2 if last else 0
            self.gens[1].submit(
                now, self.tm_packet(now, 0x021, False, flags, d["chunks"][idx])
            )
            d["index"] += 1
            d["next_us"] += 125_000
            if last:
                addr, length = d["addr"], d["length"]
                self.dump = None
                self.dumps_completed += 1
                self.event(now, 0x0020, addr >> 16, addr & 0xFFFF, length)
        # (4) housekeeping at t = k s, t < 30 s
        if now % 1_000_000 == 0 and now < 30_000_000:
            self.gens[0].submit(now, self.hk_packet(now))
            self.hk_k += 1
        # (5) payload at t = 0.5 + 2k s, t < 30 s, mode on
        if now % 2_000_000 == 500_000 and now < 30_000_000 and self.payload_on:
            n = self.payload_n
            self.payload_n += 1
            user = n.to_bytes(4, "big") + bytes((n + i) % 256 for i in range(4, 400))
            self.gens[1].submit(now, self.tm_packet(now, 0x020, False, 3, user))
        if slot:
            return self.slot_frame(now)
        return None

    def execute(self, now, func, args, seq):
        if func == FN_NOOP:
            self.executed += 1
            self.event(now, 0x0001, func, seq, 0)
        elif func == FN_ACTIVATE:
            tid, crc = args[0], int.from_bytes(args[1:3], "big")
            self.executed += 1
            self.event(now, 0x0001, func, seq, 0)
            if self.table is None or self.table[0] != tid:
                raise NotModelled("TABLE_REJECTED reason 1")
            real = crc16(self.table[1])
            if real != crc:
                raise NotModelled("TABLE_REJECTED reason 2")
            self.active_table_id, self.active_table_crc = tid, real
            self.event(now, 0x0010, tid, len(self.table[1]), real)
        elif func == FN_DUMP:
            addr, length = int.from_bytes(args[0:4], "big"), int.from_bytes(args[4:6], "big")
            if self.dump is not None:
                raise NotModelled("dump in progress")
            self.executed += 1
            self.event(now, 0x0001, func, seq, 0)
            mem = bytes(((a ^ (a >> 8)) & 0xFF) for a in range(addr, addr + length))
            sdu = addr.to_bytes(4, "big") + length.to_bytes(2, "big") + mem
            chunks = [sdu[i : i + 128] for i in range(0, len(sdu), 128)]
            self.dump = dict(next_us=now, chunks=chunks, index=0, addr=addr, length=length)
        elif func == FN_SETPL:
            self.executed += 1
            self.event(now, 0x0001, func, seq, 0)
            self.payload_on = bool(args[0])
            self.event(now, 0x0030, args[0], 0, 0)
        else:
            raise NotModelled("unknown function")

    def hk_packet(self, now):
        k = self.hk_k
        user = (
            k.to_bytes(4, "big")
            + (self.executed % 65536).to_bytes(2, "big")
            + (self.rejected % 65536).to_bytes(2, "big")
            + bytes([len(self.queue), 1 if self.payload_on else 0, self.active_table_id, 0])
            + self.active_table_crc.to_bytes(2, "big")
            + self.dumps_completed.to_bytes(2, "big")
            + b"".join(((1000 * (j + 1) + 3 * k) % 65536).to_bytes(2, "big") for j in range(8))
        )
        return self.tm_packet(now, 0x010, False, 3, user)

    # --- downlink link layer: generators, multiplexer, frame (4.1, 4.4) -----
    def slot_frame(self, now):
        for vc in (0, 1):
            if self.mux[vc] is None:
                field = self.gens[vc].release(now)
                if field is not None:
                    self.mux[vc] = field
        self.ocf = self.clcw_word().to_bytes(4, "big")
        if self.mux[0] is not None:
            vc, (data, fhp) = 0, self.mux[0]
            self.mux[0] = None
        elif self.mux[1] is not None:
            vc, (data, fhp) = 1, self.mux[1]
            self.mux[1] = None
        else:
            vc, data, fhp = 7, self.oid.next_octets(TM_DATA_LEN), 0x7FE
        w0 = (0 << 14) | (SCID << 4) | (vc << 1) | 1
        header = (
            w0.to_bytes(2, "big")
            + bytes([self.mc_count % 256, self.vc_count[vc] % 256])
            + (0x1800 | fhp).to_bytes(2, "big")
        )
        self.mc_count += 1
        self.vc_count[vc] += 1
        body = header + data + self.ocf
        assert len(body) == TM_FRAME_LEN - 2
        return body + crc16(body).to_bytes(2, "big")


class VcGenerator:
    """Packet-to-field generator of one VC (4.4.2): a field starts when its first
    octet is written and is releasable when full or when its deadline has passed."""

    def __init__(self, vc):
        self.vc = vc
        self.buf = bytearray()
        self.starts = []  # offsets in buf at which a packet starts
        self.start_us = None

    def submit(self, now, pkt):
        if not self.buf:
            self.start_us = now
        self.starts.append(len(self.buf))
        self.buf += pkt

    def release(self, now):
        if not self.buf:
            return None
        full = len(self.buf) >= TM_DATA_LEN
        timed_out = now >= self.start_us + VC_DEADLINE_US[self.vc]
        if not (full or timed_out):
            return None
        if full:
            data = bytes(self.buf[:TM_DATA_LEN])
            starts_in = [s for s in self.starts if s < TM_DATA_LEN]
            rest = self.buf[TM_DATA_LEN:]
            self.starts = [s - TM_DATA_LEN for s in self.starts if s >= TM_DATA_LEN]
            self.buf = bytearray(rest)
            self.start_us = now if self.buf else None
        else:
            fill = TM_DATA_LEN - len(self.buf)
            if fill < 7:
                raise NotModelled("remainder shorter than an idle packet")
            starts_in = list(self.starts) + [len(self.buf)]
            data = bytes(self.buf) + idle_packet(fill)
            self.buf = bytearray()
            self.starts = []
            self.start_us = None
        fhp = starts_in[0] if starts_in else 0x7FF
        return data, fhp


# ----------------------------------------------------------------------------
# Ground node: ops script, segmenter, FOP-1 (2.2, 4.2, 4.5; 232.1 5)
# ----------------------------------------------------------------------------


def cmd_packet(seq, func, exec_s, args):
    user = bytes([func, 1]) + cuc_tfield(exec_s * 1_000_000) + args
    return space_packet(0x080, 1, 0, 3, seq, user)


def table_packet(tid, length):
    return space_packet(0x081, 1, 0, 3, 0, bytes([tid]) + length.to_bytes(2, "big") + table_octets(length))


def ops_script():
    """2.4: (release_us, kind, payload)."""
    tab = table_octets(300)
    return [
        (0, "directive", "initiate_ad_without_clcw"),
        (1_000_000, "packet", cmd_packet(0, FN_NOOP, 6, b"")),
        (2_000_000, "packet", table_packet(1, 300)),
        (
            3_000_000,
            "packet",
            cmd_packet(1, FN_ACTIVATE, 10, bytes([1]) + crc16(tab).to_bytes(2, "big")),
        ),
        (
            4_000_000,
            "packet",
            cmd_packet(2, FN_DUMP, 12, (0x00010000).to_bytes(4, "big") + (600).to_bytes(2, "big")),
        ),
        (5_000_000, "packet", cmd_packet(3, FN_SETPL, 20, bytes([0]))),
        (5_000_000, "packet", cmd_packet(4, FN_NOOP, 20, b"")),
        (6_000_000, "packet", cmd_packet(5, FN_SETPL, 24, bytes([1]))),
    ]


class Segmenter:
    """MAP 0 blocking, MAP 1 segmentation (232.0 4.3.1; 4.2): MAP 0 first unless a
    MAP 1 packet is in progress; packets released at one instant share an FDU."""

    def __init__(self):
        self.map0 = []  # (release_us, packet)
        self.map1 = []
        self.progress = None  # (packet, offset)

    def submit(self, now, pkt):
        apid = ((pkt[0] & 0x07) << 8) | pkt[1]
        if apid == 0x080:
            self.map0.append((now, pkt))
        elif apid == 0x081:
            self.map1.append((now, pkt))
        else:
            raise NotModelled("APID without a MAP")

    def next_fdu(self):
        if self.progress is None and self.map0:
            rel, first = self.map0.pop(0)
            data = first
            while self.map0 and self.map0[0][0] == rel and len(data) + len(self.map0[0][1]) + 1 <= TC_MAX_FDU:
                data += self.map0.pop(0)[1]
            return bytes([0xC0]) + data  # flags 11, MAP 0
        if self.progress is None and self.map1:
            self.progress = (self.map1.pop(0)[1], 0)
        if self.progress is not None:
            pkt, off = self.progress
            chunk = pkt[off : off + TC_MAX_FDU - 1]
            first, last = off == 0, off + len(chunk) >= len(pkt)
            flags = 3 if (first and last) else 1 if first else 2 if last else 0
            self.progress = None if last else (pkt, off + len(chunk))
            return bytes([(flags << 6) | 1]) + chunk
        return None


def tc_frame(ctrl_bits, ns, data):
    length = 5 + len(data) + 2
    b0 = (ctrl_bits << 4) | (SCID >> 8)  # version 00, bypass/ctrl flags, spare 00
    head = bytes([b0, SCID & 0xFF, ((length - 1) >> 8) & 0x03, (length - 1) & 0xFF, ns])
    body = head + data
    return body + crc16(body).to_bytes(2, "big")


S1, S2, S3, S4, S5, S6 = 1, 2, 3, 4, 5, 6


class Fop1:
    """FOP-1 of 232.1 Table 5-1, restricted to the cells of the two cases."""

    def __init__(self):
        self.state = S6
        self.vs = 0
        self.nnr = 0
        self.sent = []  # [ns, frame, to_be_retransmitted]
        self.wait = None  # FDU (Segment Header + data)
        self.ad_out_ready = True
        self.timer = None  # deadline in us, or None
        self.count = 1
        self.pending = None  # the transmit request held for the next slot
        self.notes = []  # (now, text) for the self-check

    # 5.2.10
    def look_for_fdu(self, now):
        if not self.ad_out_ready:
            return
        for entry in self.sent:
            if entry[2]:
                entry[2] = False
                self.ad_out_ready = False
                self.pending = entry[1]
                self.notes.append((now, "retransmit N(S)=%d" % entry[0]))
                return
        if self.wait is not None and (self.vs - self.nnr) % 256 < FOP_K:
            fdu, self.wait = self.wait, None
            self.transmit_ad(now, fdu)

    # 5.2.4
    def transmit_ad(self, now, fdu):
        ns = self.vs
        frame = tc_frame(0, ns, fdu)
        self.vs = (self.vs + 1) % 256
        if not self.sent:
            self.count = 1
        self.sent.append([ns, frame, False])
        self.timer = now + FOP_T1_US
        self.ad_out_ready = False
        self.pending = frame
        self.notes.append((now, "transmit N(S)=%d" % ns))

    # 5.2.7
    def initiate_retransmission(self, now):
        self.count += 1
        self.timer = now + FOP_T1_US
        for entry in self.sent:
            entry[2] = True

    # 5.2.8
    def remove_acknowledged(self, nr):
        n = (nr - self.nnr) % 256
        self.sent = [e for e in self.sent if (e[0] - self.nnr) % 256 >= n]
        self.nnr = nr
        self.count = 1

    # Table 5-1 E19, E20 (AD service request)
    def request_ad(self, now, fdu):
        if self.state in (S1, S2):
            if self.wait is not None:
                raise NotModelled("E20 reject")
            self.wait = fdu
            self.look_for_fdu(now)
        else:
            raise NotModelled("RequestAd outside S1/S2")

    # Table 5-1 E41 (AD_Accept)
    def transmit_done(self, now):
        self.ad_out_ready = True
        if self.state in (S1, S2):
            self.look_for_fdu(now)

    # Table 5-1 E23 (only the cell the cases use)
    def directive_initiate_without_clcw(self):
        if self.state != S6:
            raise NotModelled("E23 reject")
        self.sent, self.wait = [], None  # Initialize (5.2.14)
        self.count = 1
        self.state = S1

    # Table 5-1 E16, E17 (Timer expires), Timeout_Type 0
    def timeout(self, now):
        if self.state not in (S1, S2):
            raise NotModelled("timer in S3..S6")
        if self.count >= FOP_LIMIT:
            raise NotModelled("E17 Alert[T1]")
        self.notes.append((now, "timeout E16"))
        self.initiate_retransmission(now)
        self.look_for_fdu(now)

    # Table 5-1 E1..E15 (CLCW arrives), 5.3.2.1 order: Lockout, N(R), Retransmit, Wait
    def clcw(self, now, word):
        lockout = (word >> 13) & 1
        wait = (word >> 12) & 1
        retransmit = (word >> 11) & 1
        nr = word & 0xFF
        if self.state == S6:
            return
        if self.state not in (S1, S2, S3):
            raise NotModelled("CLCW in S4/S5")
        if lockout:
            raise NotModelled("E14 Alert[Lockout]")
        if (nr - self.nnr) % 256 > (self.vs - self.nnr) % 256:
            raise NotModelled("E13 Alert[NN(R)]")
        if wait:
            raise NotModelled("Wait flag")
        all_acked = nr == self.vs
        new_acked = nr != self.nnr
        if not retransmit:
            if all_acked and not new_acked:  # E1
                if self.state != S1:
                    raise NotModelled("E1 Alert[synch]")
            elif all_acked:  # E2
                self.remove_acknowledged(nr)
                self.timer = None
                self.look_for_fdu(now)
                self.state = S1
            elif not new_acked:  # E5
                if self.state != S1:
                    raise NotModelled("E5 Alert[synch]")
            else:  # E6
                self.remove_acknowledged(nr)
                self.look_for_fdu(now)
                self.state = S1
        else:
            if all_acked:
                raise NotModelled("E3/E4 Alert[synch]")
            if FOP_LIMIT == 1:
                raise NotModelled("E101/E102")
            if new_acked:  # E8
                self.remove_acknowledged(nr)
                self.initiate_retransmission(now)
                self.look_for_fdu(now)
                self.state = S2
            elif self.count < FOP_LIMIT:  # E10
                if self.state != S2:  # S1, S3: Initiate AD Retransmission, Look for FDU
                    self.notes.append((now, "retransmit initiated E10"))
                    self.initiate_retransmission(now)
                    self.look_for_fdu(now)
                self.state = S2
            else:  # E12
                self.state = S2


class Ground:
    def __init__(self):
        self.fop = Fop1()
        self.seg = Segmenter()
        self.ops = ops_script()
        self.ops_next = 0

    def next_ops_time(self):
        return self.ops[self.ops_next][0] if self.ops_next < len(self.ops) else None

    def step(self, now, tm_frames, slot):
        for fr in tm_frames:  # (1) OCF of each delivered frame -> FOP-1 as a CLCW
            ocf = int.from_bytes(fr[-6:-2], "big")
            if (ocf >> 31) & 1 or (ocf >> 29) & 3 or (ocf >> 24) & 3 != 1 or (ocf >> 18) & 0x3F:
                raise NotModelled("OCF is not a CLCW of VC 0")
            self.fop.clcw(now, ocf)
        if self.fop.timer is not None and self.fop.timer <= now:  # (2) Timeout, once
            self.fop.timeout(now)
        while self.ops_next < len(self.ops) and self.ops[self.ops_next][0] <= now:  # (3)
            _at, kind, payload = self.ops[self.ops_next]
            self.ops_next += 1
            if kind == "directive":
                if payload != "initiate_ad_without_clcw":
                    raise NotModelled("directive")
                self.fop.directive_initiate_without_clcw()
            else:
                self.seg.submit(now, payload)
        out = None
        if slot:  # (4)
            if self.fop.wait is None:  # no RequestAd unanswered
                fdu = self.seg.next_fdu()
                if fdu is not None:
                    self.fop.request_ad(now, fdu)
            if self.fop.pending is not None:
                out, self.fop.pending = self.fop.pending, None
                self.fop.transmit_done(now)
        return out


# ----------------------------------------------------------------------------
# Harness (6.1 to 6.3): slots, 10 ms delay, scripted faults, tie order
# ----------------------------------------------------------------------------

CASES = {
    "a3-nominal": {"up": {}, "down": {}},
    # 6.8: drop up 2 and up 10; drop down 33 to 39
    "a3-uplink-loss": {
        "up": {2: "drop", 10: "drop"},
        "down": {i: "drop" for i in range(33, 40)},
    },
}


def run_case(name):
    faults = CASES[name]
    ground, flight = Ground(), Flight()
    sent = {"up": 0, "down": 0}
    arrivals = []  # heap of (arrival_us, send_us, dir_rank, index, dir, octets)
    records = []
    t = 0
    while t < END_US:
        # deliveries at t, in the order of 6.2 step 1: (send instant, down before up, index)
        to_ground, to_flight = [], []
        while arrivals and arrivals[0][0] == t:
            _a, _s, _r, _i, d, octets = heapq.heappop(arrivals)
            (to_ground if d == "down" else to_flight).append(octets)
        down_slot = t % DOWN_SLOT_US == 0
        up_slot = t % UP_SLOT_US == 0
        due_ops = ground.next_ops_time()
        # ground (6.2 step 2)
        if (
            to_ground
            or up_slot
            or (due_ops is not None and due_ops <= t)
            or (ground.fop.timer is not None and ground.fop.timer <= t)
        ):
            unit = ground.step(t, to_ground, up_slot)
            if unit is not None:
                idx = sent["up"]
                sent["up"] += 1
                records.append((t, "up", idx, unit.hex().upper()))
                if faults["up"].get(idx) != "drop":
                    heapq.heappush(arrivals, (t + DELAY_US, t, 1, idx, "up", unit))
        # flight (6.2 step 3)
        if to_flight or down_slot:
            unit = flight.step(t, to_flight, down_slot)
            if unit is not None:
                idx = sent["down"]
                sent["down"] += 1
                records.append((t, "down", idx, unit.hex().upper()))
                if faults["down"].get(idx) != "drop":
                    heapq.heappush(arrivals, (t + DELAY_US, t, 0, idx, "down", unit))
        # next instant: slots, arrivals, ops, the FOP-1 timer
        cands = [(t // DOWN_SLOT_US + 1) * DOWN_SLOT_US, (t // UP_SLOT_US + 1) * UP_SLOT_US]
        if arrivals:
            cands.append(arrivals[0][0])
        nxt = ground.next_ops_time()
        if nxt is not None and nxt > t:
            cands.append(nxt)
        if ground.fop.timer is not None and ground.fop.timer > t:
            cands.append(ground.fop.timer)
        t = min(c for c in cands if c > t)
    return records, ground, flight


def to_jsonl(records):
    return [
        '{"t_us":%d,"kind":"unit_sent","dir":"%s","index":%d,"hex":"%s"}' % (t, d, i, h)
        for t, d, i, h in records
    ]


# ----------------------------------------------------------------------------
# Checks against the figures the document states (not the golden traces)
# ----------------------------------------------------------------------------


def self_check():
    # CRC-16 check value (reference-application 2.4: 0x29B1 for "123456789")
    assert crc16(b"123456789") == 0x29B1
    # table CRC of 2.4
    assert crc16(table_octets(300)) == 0x0C4F
    # OID pattern, 132.0 4.1.4.6.2 note and Annex D
    assert OidGenerator().next_octets(20).hex().upper() == "FFFFFFFF6DB6D861451F11F19716723CBE7E00B1"
    # 4.4.4: frame headers
    nominal, ground, flight = run_case("a3-nominal")
    down = [r for r in nominal if r[1] == "down"]
    up = [r for r in nominal if r[1] == "up"]
    assert len(down) == 280 and len(up) == 8
    assert down[0][3].startswith("155F00001FFE")
    assert down[2][3].startswith("155102001800")
    assert down[4][3].startswith("155304001800")
    kinds = [bytes.fromhex(r[3])[0:2].hex() for r in down]
    assert kinds.count("155f") == 219 and kinds.count("1551") == 31 and kinds.count("1553") == 30
    # 4.4.4 and 4.5.5: uplink frame times, lengths, heads
    heads = [
        (1.0, 22, "01550015" + "00C0"),
        (2.0, 128, "0155007F" + "0141"),
        (2.25, 128, "0155007F" + "0201"),
        (2.5, 77, "0155004C" + "0381"),
        (3.0, 25, "01550018" + "04C0"),
        (4.0, 28, "0155001B" + "05C0"),
        (5.0, 37, "01550024" + "06C0"),
        (6.0, 23, "01550016" + "07C0"),
    ]
    for rec, (ts, flen, head) in zip(up, heads):
        assert rec[0] == int(ts * 1e6) and len(rec[3]) == 2 * flen, (rec[0], len(rec[3]) // 2)
        assert rec[3].startswith(head), rec[3][:16]
    # 4.5.5: the OCF changes at frames 9, 17, 19, 21, 25, 33, 41, 49
    words = {}
    last = 0x01000000
    for i, (_t, _d, _i, h) in enumerate(down):
        w = int(h[-12:-4], 16)
        if w != last:
            words[i] = w
            last = w
    assert words == {9: 0x01000001, 17: 0x01000002, 19: 0x01000003, 21: 0x01000004,
                     25: 0x01000005, 33: 0x01000006, 41: 0x01000007, 49: 0x01000008}, words
    # 6.8: the uplink table of a3-uplink-loss: (index, t, N(S))
    loss, ground, flight = run_case("a3-uplink-loss")
    up = [r for r in loss if r[1] == "up"]
    want = [(1.0, 0), (2.0, 1), (2.25, 2), (2.5, 3), (2.75, 2), (3.0, 3), (3.25, 4),
            (4.0, 5), (5.0, 5), (5.25, 6), (6.0, 7), (7.0, 7)]
    got = [(r[0] / 1e6, bytes.fromhex(r[3])[4]) for r in up]
    assert got == want, got
    assert all(bytes.fromhex(r[3])[0] == 0x01 for r in up)
    # 6.8: the CLCW words the ground sees change as the document says
    down = [r for r in loss if r[1] == "down"]
    words = {}
    last = 0x01000000
    for i, (_t, _d, _i, h) in enumerate(down):
        w = int(h[-12:-4], 16)
        if w != last:
            words[i] = w
            last = w
    assert words == {9: 16777217, 17: 16777218, 21: 16779266, 23: 16777219, 25: 16777220,
                     27: 16777221, 33: 16777222, 43: 16777223, 57: 16777224}, words
    print("self-check ok")


def compare(path, name):
    records, _g, _f = run_case(name)
    model = to_jsonl(records)
    golden = []
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            if '"kind":"unit_sent"' in line:
                golden.append(line.rstrip("\n"))
    diffs = 0
    for k in range(max(len(model), len(golden))):
        m = model[k] if k < len(model) else None
        g = golden[k] if k < len(golden) else None
        if m != g:
            diffs += 1
            print("record %d differs\n  model : %s\n  golden: %s" % (k, m, g))
    print(
        "%s: %d model records, %d golden unit_sent records, %d differences"
        % (name, len(model), len(golden), diffs)
    )
    return diffs


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--self-check", action="store_true")
    ap.add_argument("--emit", metavar="CASE")
    ap.add_argument("--compare", nargs=2, metavar=("GOLDEN", "CASE"))
    args = ap.parse_args()
    if args.self_check:
        self_check()
    if args.emit:
        records, _g, _f = run_case(args.emit)
        print("\n".join(to_jsonl(records)))
    if args.compare:
        sys.exit(1 if compare(*args.compare) else 0)


if __name__ == "__main__":
    main()
