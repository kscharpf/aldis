"""Independent model of the sending-side link engines for the vector files under
test-vectors/link/send/ (LNK-010..015, LNK-020, LNK-021, LNK-030..036, LNK-040..042, LNK-076,
LNK-003 and LNK-036 for Reconfigure; TV-010, TV-023; M4 WP-2a).

Sources: the Blue Books 132.0-B-3 (TM), 732.0-B-5 EC1 (AOS), 732.1-B-3 (USLP), 232.0-B-4
Cor. 1 (TC), 133.0-B-2 (Space Packet), 133.1-B-3 (Encapsulation Packet); the engine contract
of docs/03-api-model.md sections 5 to 8; the engines of docs/04-engines.md sections 1, 2, 4,
and 5 after M4 WP-1; docs/standards/common-link-features.md (the OID pattern, the counters);
docs/milestones/m4-link-engines.md (decisions D2 and D3); the resolved OI-11, OI-34, OI-53,
OI-68, OI-69, OI-71 of docs/09-open-issues.md; and specs/sana/uslp_protocol_id.tsv for the
UPIDs. Each function and table below names the clause or the LNK row it implements.

This program was written blind (TV-023): by an agent that had not read any implementation
(nothing under crates/, exerciser/, fuzz/, crosscheck/, refapp/, include/, nor the Rust source
of xtask/; there is no link-engine implementation yet), from the Blue Books and docs/ only,
with the test-vectors/ README, schema, models/README.md, models/cop1.py and cop1/ read for the
format only. It must stay independent of the Rust code: never port code from it, and never
change the model to agree with it. When a model and an implementation disagree, the Blue Book
decides which is wrong.

The model holds parameters and code, never Blue Book text. Comments paraphrase.

What it does
------------
* The engines are written as pure steps over plain variables (docs/03 section 5): PacketGen,
  AccessGen (TM, AOS, USLP), BitstreamGen, ChannelMux (TM, AOS, fixed-length USLP) and
  MapSegmenter (TC, USLP variable-length). A step takes one input and the instant, returns the
  complete ordered outputs, and leaves the state unchanged when it returns an error (API-058).
* The scenarios are scripts of inputs chosen by hand (SCENARIOS below); the expected outputs
  and deadlines are the model's. The files are written by the model (--write) and never edited
  by hand.
* The OID pattern is generated in the Fibonacci form of 132.0 Annex D (CMN-020) and
  cross-checked against the Galois form (CMN-022) and the 20 octets printed in Annex D.

The vocabulary of inputs and outputs, and the readings the model makes where the documents are
silent: test-vectors/link/send/README.md.

Usage, from the repository root:

    python3 -I test-vectors/models/link_send.py --check test-vectors/link/send/*.json
    python3 -I test-vectors/models/link_send.py --write          # regenerate every file
    python3 -I test-vectors/models/link_send.py --coverage       # LNK rows of the scope

--check replays every live scenario through the model, compares the steps and the metadata
with the file, compares the file with what the generator writes, and checks that every
requirement of the scope has a scenario; it exits 1 on any difference.
"""
import copy
import json
import os
import sys

# --------------------------------------------------------------------------------------------
# Errors and helpers
# --------------------------------------------------------------------------------------------


class Err(Exception):
    """An input the engine refuses (docs/04 section 5, "Rejection of an input")."""

    def __init__(self, kind, field=None):
        Exception.__init__(self, kind)
        self.kind = kind
        self.field = field


def err_out(e):
    out = {"error": e.kind}
    if e.field:
        out["field"] = e.field
    return [out]


def hx(b):
    return bytes(b).hex().upper()


def unhx(s):
    return bytes.fromhex(s)


def cyc(pattern, n):
    """n octets of the idle pattern, repeated from its first octet."""
    if n <= 0:
        return b""
    return (pattern * (n // len(pattern) + 1))[:n]


def u16(v):
    return v.to_bytes(2, "big")


# --------------------------------------------------------------------------------------------
# The OID pattern (132.0-B-3 4.1.4.6.2, Annex D; CMN-020, CMN-022, CMN-023)
# --------------------------------------------------------------------------------------------

ANNEX_D_PRINTED = unhx("FFFFFFFF6DB6D861451F11F19716723CBE7E00B1")
GALOIS_SEED = "00000000001111111111111111111101"


class OidGen:
    """The Fibonacci form: x[n] = x[n-1] ^ x[n-2] ^ x[n-22] ^ x[n-32] (polynomial
    1 + D + D^2 + D^22 + D^32), all-ones seed, output MSB first, never restarted."""

    def __init__(self):
        self.w = [1] * 32  # the last 32 output bits, oldest first

    def next_bit(self):
        x = self.w[-1] ^ self.w[-2] ^ self.w[-22] ^ self.w[-32]
        # the bit emitted now is the oldest of the window; the window slides by one
        out = self.w[0]
        self.w = self.w[1:] + [x]
        return out

    def take(self, n):
        out = bytearray()
        for _ in range(n):
            v = 0
            for _ in range(8):
                v = (v << 1) | self.next_bit()
            out.append(v)
        return bytes(out)


def galois_stream(nbytes):
    """The Galois form of Annex D (figure D-2): 32 cells D32..D1, the seed above, feedback from
    D1 into D32, D22, D2 and D1."""
    c = [int(ch) for ch in GALOIS_SEED]
    out = bytearray()
    for _ in range(nbytes):
        v = 0
        for _ in range(8):
            o = c[31]
            v = (v << 1) | o
            n = [0] * 32
            n[0] = o
            for i in range(1, 32):
                n[i] = c[i - 1]
            for t in (10, 30, 31):
                n[t] ^= o
            c = n
        out.append(v)
    return bytes(out)


def selftest():
    assert OidGen().take(20) == ANNEX_D_PRINTED
    assert galois_stream(20) == ANNEX_D_PRINTED
    assert OidGen().take(300) == galois_stream(300)
    g = OidGen()
    a = g.take(7) + g.take(13)  # continuing across calls equals one call (CMN-023)
    assert a == ANNEX_D_PRINTED


# --------------------------------------------------------------------------------------------
# Packets: Space Packet (133.0-B-2) and Encapsulation Packet (133.1-B-3)
# --------------------------------------------------------------------------------------------

PVN_SPP = 0  # 133.0 4.1.3.2.2
PVN_EPP = 7  # 133.1 4.1.2.2.2


def spp_packet(apid, total, seq=0, start=0, flags=3, ptype=0):
    """A Space Packet of `total` octets (>= 7), user data = incrementing octets from `start`.
    Header: PVN, type, secondary header flag 0, APID, sequence flags, count, length C = data - 1
    (133.0 4.1.3)."""
    assert total >= 7
    h = (PVN_SPP << 13) | (ptype << 12) | apid
    h2 = (flags << 14) | seq
    hdr = u16(h) + u16(h2) + u16(total - 7)
    return hdr + bytes((start + i) & 0xFF for i in range(total - 6))


def epp_packet(total, epi=7, start=0):
    """An Encapsulation Packet of `total` octets (>= 2) with EPI `epi`; the header is chosen
    as the smallest that holds the length (133.1 4.1.2.4, Table 4-2). EPI 7 is mission-specific
    data."""
    assert total >= 2
    if total <= 255:
        hdr = bytes([(7 << 5) | (epi << 2) | 1, total])
    else:
        hdr = bytes([(7 << 5) | (epi << 2) | 2, 0]) + u16(total)
    return hdr + bytes((start + i) & 0xFF for i in range(total - len(hdr)))


def spp_idle(total, pattern):
    """A Space Packet Idle Packet (133.0 4.1.3.3.4.4): APID all ones, secondary header flag 0
    (4.1.3.3.3.4), unsegmented, count 0, type 0 (telemetry; 133.0 states none for idle packets,
    the README names it a reading), data = the idle pattern."""
    assert total >= 7
    return spp_packet_raw(total, pattern)


def spp_packet_raw(total, pattern):
    h = (PVN_SPP << 13) | (0 << 12) | 0x7FF
    return u16(h) + u16(3 << 14) + u16(total - 7) + cyc(pattern, total - 6)


def epp_idle(total, pattern):
    """An Encapsulation Idle Packet (133.1 4.1.2.3 note 1, 4.1.2.4.4): EPI 000; one octet when
    total is 1, else the smallest header."""
    assert total >= 1
    if total == 1:
        return bytes([0xE0])
    if total <= 255:
        return bytes([(7 << 5) | 1, total]) + cyc(pattern, total - 2)
    assert total <= 65535
    return bytes([(7 << 5) | 2, 0]) + u16(total) + cyc(pattern, total - 4)


def packet_total(p):
    """The length peek of an SPP or an EPP (133.0 4.1.3.5; 133.1 4.1.2.4, 4.1.2.8). The PVN is
    the first three bits."""
    if len(p) == 0:
        raise Err("LengthMismatch", "packet_len")
    pvn = p[0] >> 5
    if pvn == PVN_SPP:
        if len(p) < 6:
            raise Err("LengthMismatch", "packet_len")
        return int.from_bytes(p[4:6], "big") + 7
    lol = p[0] & 3
    if lol == 0:
        return 1
    need = {1: 2, 2: 4, 3: 8}[lol]
    if len(p) < need:
        raise Err("LengthMismatch", "packet_len")
    if lol == 1:
        return p[1]
    if lol == 2:
        return int.from_bytes(p[2:4], "big")
    return int.from_bytes(p[4:8], "big")


def check_packet(p, valid_pvns, max_len):
    """LNK-010, the checks of SubmitPacket: PVN in the valid set, total length at most the
    maximum, the length field agreeing with the octets given."""
    if len(p) == 0:
        raise Err("LengthMismatch", "packet_len")
    if (p[0] >> 5) not in valid_pvns:
        raise Err("InvalidField", "packet_version")
    total = packet_total(p)
    if total > max_len:
        raise Err("OutOfRange", "max_packet_len")
    if len(p) != total:
        raise Err("LengthMismatch", "packet_len")


# --------------------------------------------------------------------------------------------
# PacketGenerator (LNK-010..015; 04 section 2.1)
# --------------------------------------------------------------------------------------------

# FHP values (04 section 2.1 table): protocol -> (header octets of the field, none, idle)
FHP = {
    "tm": (0, 0x7FF, 0x7FE),  # 132.0 4.1.2.7.6.4-5
    "aos5": (2, 0xFFFF, 0xFFFE),  # 732.0-B-5 4.1.4.2.2.4-5
    "aos4": (2, 0x7FF, 0x7FE),  # 732.0-B-4 4.1.4.2
    "uslp": (3, 0xFFFF, None),  # 732.1 4.1.4.2.4.4
}


def pg_kind(cfg):
    p = cfg["protocol"]
    if p == "aos":
        return "aos4" if cfg.get("aos_issue", 5) == 4 else "aos5"
    return p


class PacketGen:
    """One VC (or MAP) that carries packets. State: the queue of octets of accepted packets not
    yet in a field, the field being assembled, and the idle residue."""

    def __init__(self, cfg):
        self.cfg = cfg
        self.kind = pg_kind(cfg)
        self.hdr, self.none, self.idle = FHP[self.kind]
        self.zone = cfg["field_len"] - self.hdr
        self.pat = unhx(cfg.get("idle_pattern_hex", "00"))
        self.deadline_us = cfg.get("release_deadline_us")
        assert self.zone >= 8
        self.queue = []  # [bytes, continued] items, in order
        self.field = bytearray()
        self.starts = []
        self.started_at = None
        self.releasable = False
        self.residue = b""

    # --- helpers
    def qocts(self):
        return sum(len(q[0]) for q in self.queue)

    def idle_kind(self):
        if self.cfg["protocol"] == "uslp":
            return "encapsulation"  # LNK-013 USLP: always an Encapsulation Idle Packet
        return self.cfg.get("idle_packet_kind", "space_packet")

    def pump(self, now):
        """Write queued octets into the field being assembled, as far as they go. A field is
        started when its first octet is written (LNK-012); it is releasable when full."""
        while not self.releasable:
            if len(self.field) == 0 and self.residue:
                self.field += self.residue  # the residue of an idle packet: no start (LNK-011)
                self.residue = b""
                self.started_at = now
            elif self.queue:
                item = self.queue[0]
                space = self.zone - len(self.field)
                n = min(space, len(item[0]))
                if not item[1]:
                    self.starts.append(len(self.field))
                if not self.field:
                    self.started_at = now
                self.field += item[0][:n]
                if n == len(item[0]):
                    self.queue.pop(0)
                else:
                    self.queue[0] = [item[0][n:], True]
            else:
                break
            if len(self.field) == self.zone:
                self.releasable = True

    def complete(self, remaining):
        """Fill `remaining` octets with idle packets (LNK-013). Returns (written, residue)."""
        if self.idle_kind() == "encapsulation":
            return epp_idle(remaining, self.pat), b""
        if remaining >= 7:
            return spp_idle(remaining, self.pat), b""
        full = spp_idle(7, self.pat)  # the Space Packet idle packet is at least 7 octets
        return full[:remaining], full[remaining:]

    def wrap(self, zone, fhp):
        """The field as the engine returns it (04 section 4: the TM data field, the AOS M_PDU,
        the USLP TFDF) and its metadata."""
        if self.kind == "tm":
            body = bytes(zone)
        elif self.kind == "aos5":
            body = u16(fhp) + bytes(zone)
        elif self.kind == "aos4":
            body = u16(fhp & 0x7FF) + bytes(zone)  # 5 spare bits 0 (732.0-B-4 4.1.4.2.2)
        else:
            upid = self.cfg.get("upid", 0)  # rule 000 (732.1 4.1.4.2.2.2.1)
            body = bytes([(0 << 5) | upid]) + u16(fhp) + bytes(zone)
        act = {"vc": self.cfg["vc"], "field_hex": hx(body), "fhp": fhp}
        if self.kind == "uslp":
            act["map"] = self.cfg.get("map_id", 0)
            act["qos"] = self.cfg.get("qos", "sequence_controlled")
        return {"data_field": act}

    def finalize(self):
        zone = bytearray(self.field)
        starts = list(self.starts)
        residue = b""
        if len(zone) < self.zone:
            written, residue = self.complete(self.zone - len(zone))
            starts.append(len(zone))  # the idle packet starts here: a start
            zone += written
        fhp = starts[0] if starts else self.none
        return zone, fhp, residue

    def idle_field(self):
        """The field of the IdleField policy (LNK-014)."""
        if self.kind == "tm":
            if self.idle_kind() == "encapsulation":
                zone = epp_idle(self.zone, self.pat)
            else:
                zone = spp_idle(self.zone, self.pat)
            return self.wrap(zone, 0)
        if self.kind in ("aos5", "aos4"):
            return self.wrap(cyc(self.pat, self.zone), self.idle)
        return self.wrap(epp_idle(self.zone, self.pat), 0)

    # --- the step
    def step(self, inp, now):
        (name, arg), = inp.items()
        if name == "submit_packet":
            p = unhx(arg["packet_hex"])
            check_packet(p, self.cfg["valid_pvns"], self.cfg["max_packet_len"])
            if self.qocts() + len(p) > self.cfg["queue_capacity"]:
                raise Err("WouldBlock")  # LNK-015
            self.queue.append([p, False])
            self.pump(now)
            return []
        if name == "timeout":
            if (self.started_at is not None and not self.releasable and
                    self.deadline_us is not None and now >= self.started_at + self.deadline_us):
                self.releasable = True  # LNK-012: marks, writes nothing
            return []
        if name == "release":
            if self.releasable:
                zone, fhp, residue = self.finalize()
                out = self.wrap(zone, fhp)
                self.field = bytearray()
                self.starts = []
                self.started_at = None
                self.releasable = False
                self.residue = residue
                self.pump(now)
                return [out]
            if self.cfg.get("idle_policy", "oid") == "idle_field":
                return [self.idle_field()]
            return []
        if name == "reconfigure":
            held = len(self.field) + self.qocts() + len(self.residue)
            self.__init__(arg["config"])
            if held:
                return [{"notify": {"kind": "Discarded", "octets": held}}]
            return []
        raise AssertionError(name)

    def deadline(self):
        if (self.started_at is not None and not self.releasable and
                self.deadline_us is not None):
            return self.started_at + self.deadline_us
        return None


# --------------------------------------------------------------------------------------------
# AccessGenerator (LNK-020; 04 section 2.2)
# --------------------------------------------------------------------------------------------


class AccessGen:
    """TM and AOS: one SDU of exactly the data field length per field, unchanged. USLP fixed
    length: rules 001 and 010 with the Last Valid Octet Pointer (732.1 4.1.4.2.2.2.2-3,
    4.1.4.2.4.5-6, 4.2.3.1)."""

    def __init__(self, cfg):
        self.cfg = cfg
        self.uslp = cfg["protocol"] == "uslp"
        self.pat = unhx(cfg.get("idle_pattern_hex", "00"))
        self.queue = []
        self.cur = None  # the SDU in progress
        self.pos = 0
        if self.uslp:
            self.zone = cfg["field_len"] - 3

    def step(self, inp, now):
        (name, arg), = inp.items()
        if name == "submit_sdu":
            s = unhx(arg["sdu_hex"])
            if self.uslp and len(s) > self.cfg["max_sdu_len"]:
                raise Err("OutOfRange", "max_sdu_len")
            self.queue.append(s)
            return []
        if name == "release":
            if not self.uslp:
                if not self.queue:
                    return []
                s = self.queue.pop(0)
                return [{"data_field": {"vc": self.cfg["vc"], "field_hex": hx(s)}}]
            if self.cur is None:
                if not self.queue:
                    return []
                self.cur = self.queue.pop(0)
                self.pos = 0
                rule = 1
            else:
                rule = 2
            rest = self.cur[self.pos:]
            if len(rest) > self.zone:
                zone = rest[:self.zone]
                lvo = 0xFFFF  # the SDU does not end here
                self.pos += self.zone
            else:
                zone = rest + cyc(self.pat, self.zone - len(rest))
                lvo = len(rest) - 1  # the last octet of the SDU, also when it is the last of the zone
                self.cur = None
            upid = self.cfg.get("upid", 5)
            field = bytes([(rule << 5) | upid]) + u16(lvo) + zone
            return [{"data_field": {"vc": self.cfg["vc"], "field_hex": hx(field), "lvo": lvo,
                                    "map": self.cfg.get("map_id", 0),
                                    "qos": self.cfg.get("qos", "sequence_controlled")}}]
        if name == "reconfigure":
            held = sum(len(q) for q in self.queue) + (len(self.cur) - self.pos if self.cur else 0)
            self.__init__(arg["config"])
            return [{"notify": {"kind": "Discarded", "octets": held}}] if held else []
        raise AssertionError(name)

    def deadline(self):
        return None


# --------------------------------------------------------------------------------------------
# BitstreamGenerator (AOS B_PDU; LNK-021; 732.0 4.1.4.3, 4.2.3)
# --------------------------------------------------------------------------------------------


class BitstreamGen:
    """Bits in order and unchanged into the Bitstream Data Zone; the rest of an incomplete
    zone is the idle pattern (only 00 and FF, which do not depend on the bit alignment)."""

    def __init__(self, cfg):
        self.cfg = cfg
        self.zone = cfg["field_len"] - 2
        assert 1 <= self.zone <= 2047  # AOS-018, OI-53
        self.pat = unhx(cfg.get("idle_pattern_hex", "00"))
        assert self.pat in (b"\x00", b"\xff")
        self.bits = []

    def step(self, inp, now):
        (name, arg), = inp.items()
        if name == "submit_bits":
            data = unhx(arg["bits_hex"])
            n = arg["bit_len"]
            assert 1 <= n <= 8 * len(data) and 8 * len(data) - n < 8
            for i in range(n):
                self.bits.append((data[i // 8] >> (7 - i % 8)) & 1)
            return []
        if name == "release":
            zbits = 8 * self.zone
            n = min(len(self.bits), zbits)
            taken = self.bits[:n]
            self.bits = self.bits[n:]
            fill = 1 if self.pat == b"\xff" else 0
            allbits = taken + [fill] * (zbits - n)
            zone = bytearray()
            for i in range(0, zbits, 8):
                v = 0
                for b in allbits[i:i + 8]:
                    v = (v << 1) | b
                zone.append(v)
            if n == zbits:
                ptr = 0x3FFF  # no idle data: all ones (4.1.4.3.3.3)
            elif n == 0:
                ptr = 0x3FFE  # no valid data: all ones minus one (4.1.4.3.3.4)
            else:
                ptr = n - 1  # the last valid bit (4.1.4.3.3.2)
            field = u16(ptr) + bytes(zone)  # 2 spare bits 00 and the 14-bit pointer
            return [{"data_field": {"vc": self.cfg["vc"], "field_hex": hx(field), "bdp": ptr}}]
        if name == "reconfigure":
            held = len(self.bits)
            self.__init__(arg["config"])
            return [{"notify": {"kind": "Discarded", "octets": (held + 7) // 8}}] if held else []
        raise AssertionError(name)

    def deadline(self):
        return None


# --------------------------------------------------------------------------------------------
# ChannelMux (LNK-030..036, LNK-076; 04 section 2.3)
# --------------------------------------------------------------------------------------------

TM_OID_FHP = 0x7FE  # 132.0 4.1.4.6.1
USLP_UPID_OID = 31  # SANA registry snapshot: 0b11111 "Only Idle Data"
SC, EXP = "sequence_controlled", "expedited"


class Policy:
    """The multiplexing policies (LNK-031); 132.0 4.2.4.3 leaves the algorithm to the project."""

    def __init__(self, kind, vcs):
        self.kind = kind
        self.vcids = sorted(v["vcid"] for v in vcs)
        self.prio = {v["vcid"]: v.get("priority", 0) for v in vcs}
        self.weight = {v["vcid"]: v.get("weight", 1) for v in vcs}
        self.last = None  # round robin: the VC served last
        self.cur = 0  # weighted: index into vcids
        self.credit = self.weight[self.vcids[0]]

    def advance(self):
        self.cur = (self.cur + 1) % len(self.vcids)
        self.credit = self.weight[self.vcids[self.cur]]

    def select(self, offered):
        if self.kind == "fixed_priority":
            return max(offered, key=lambda v: self.prio[v])
        if self.kind == "round_robin":
            order = sorted(offered)
            later = [v for v in order if self.last is not None and v > self.last]
            pick = later[0] if later else order[0]
            self.last = pick
            return pick
        # weighted round robin: up to `weight` consecutive selections per turn; a VC with no
        # field forfeits the rest of its turn
        for _ in range(len(self.vcids)):
            v = self.vcids[self.cur]
            if v in offered:
                self.credit -= 1
                if self.credit == 0:
                    self.advance()
                return v
            self.advance()
        return None


class ChannelMux:
    def __init__(self, cfg, carry=None):
        self.cfg = cfg
        self.proto = cfg["protocol"]
        self.F = cfg["frame_len"]
        assert cfg["policy"]["kind"] in ("fixed_priority", "round_robin", "weighted_round_robin")
        self.fecf = 2 if cfg.get("fecf") else 0
        self.vcs = {v["vcid"]: v for v in cfg["vcs"]}
        self.policy = Policy(cfg["policy"]["kind"], cfg["vcs"])
        self.slots = {v: None for v in self.vcs}
        if carry is None:
            self.oid = OidGen()
            self.mc_count = 0
            self.vc_count = {}  # key: vcid (tm, aos) or (vcid, qos) (uslp)
        else:  # reconfiguration: the OID generator and the counts persist (CMN-021, LNK-036)
            self.oid, self.mc_count, self.vc_count = carry
        self.ocf = {}
        self.fsh = {}
        for v in cfg["vcs"]:
            if v.get("ocf_init_hex"):
                self.ocf[("vc", v["vcid"])] = unhx(v["ocf_init_hex"])
            if v.get("fsh_init_hex"):
                self.fsh[("vc", v["vcid"])] = unhx(v["fsh_init_hex"])
        if cfg.get("mc_ocf_init_hex"):
            self.ocf[("mc",)] = unhx(cfg["mc_ocf_init_hex"])
        if cfg.get("oid", {}).get("ocf_init_hex"):  # USLP: the OCF of VCID 63
            self.ocf[("vc", 63)] = unhx(cfg["oid"]["ocf_init_hex"])
        if cfg.get("mc_fsh_init_hex"):
            self.fsh[("mc",)] = unhx(cfg["mc_fsh_init_hex"])
        self.izl = cfg.get("insert_zone_len", 0)
        self.insert = unhx(cfg["insert_init_hex"]) if self.izl else b""
        self.fresh = False

    # --- layout
    def oid_vcid(self):
        return self.cfg["oid_vc"] if self.proto == "tm" else 63

    def has_ocf(self, vcid, oid):
        c = self.cfg
        if self.proto == "tm":
            return bool(c.get("mc_ocf_init_hex")) or bool(self.vcs[vcid].get("ocf_init_hex"))
        if self.proto == "aos":
            return False if oid else bool(self.vcs[vcid].get("ocf_init_hex"))
        if oid:
            return bool(c.get("oid", {}).get("ocf_init_hex"))
        return bool(self.vcs[vcid].get("ocf_init_hex"))

    def vcf_len(self, vcid, qos, oid):
        if oid:
            return c_oid_len(self.cfg)
        v = self.vcs[vcid]
        return v["vcf_len_sc"] if qos == SC else v["vcf_len_exp"]

    def fsh_len(self, vcid):
        if self.proto != "tm":
            return 0
        if ("vc", vcid) in self.fsh:
            return 1 + len(self.fsh[("vc", vcid)])
        if ("mc",) in self.fsh:
            return 1 + len(self.fsh[("mc",)])
        return 0

    def sec(self, vcid, oid):
        if oid:
            return (0, 0)
        v = self.vcs[vcid]
        return (v.get("sec_header_len", 0), v.get("sec_trailer_len", 0))

    def data_len(self, vcid, qos=SC, oid=False):
        """The field the VC gives the multiplexer (04 section 4: D), by the layout of the frame
        (TM 132.0 4.1.4.2, AOS 732.0 4.1.4.1.2, USLP 732.1 4.1.4.1.2, 6.3.5.2)."""
        sh, st = self.sec(vcid, oid)
        ocf = 4 if self.has_ocf(vcid, oid) else 0
        if self.proto == "tm":
            head = 6 + self.fsh_len(vcid)
        elif self.proto == "aos":
            head = 6 + (2 if self.cfg.get("fhec") else 0) + self.izl
        else:
            head = 7 + self.vcf_len(vcid, qos, oid) + self.izl
        return self.F - head - sh - st - ocf - self.fecf

    # --- frames
    def counts(self, vcid, qos, oid):
        """Assign the counts when the frame is selected (LNK-032); returns (mc, vc, cycle)."""
        mc = None
        if self.proto == "tm":
            mc = self.mc_count
            self.mc_count = (mc + 1) % 256
            vc = self.vc_count.get(vcid, 0)
            self.vc_count[vcid] = (vc + 1) % 256
            return mc, vc, 0
        if self.proto == "aos":
            vc = self.vc_count.get(vcid, 0)
            nxt = vc + 1
            self.vc_count[vcid] = nxt % (1 << 28)
            if self.cfg.get("cycle_in_use"):
                return None, vc % (1 << 24), (vc >> 24) & 0xF
            return None, vc % (1 << 24), 0
        n = self.vcf_len(vcid, qos, oid)
        key = (vcid, qos)
        vc = self.vc_count.get(key, 0)
        if n:
            self.vc_count[key] = (vc + 1) % (1 << (8 * n))
        return None, vc, 0

    def build(self, vcid, kind, offer, oid_data):
        c = self.cfg
        oid = kind == "oid"
        qos = SC
        mapid = 0
        if self.proto == "uslp":
            qos = (c.get("oid", {}).get("qos", SC)) if oid else offer.get("qos", self.vcs[vcid].get("qos", SC))
            mapid = 0 if oid else offer.get("map", self.vcs[vcid].get("map_id", 0))
        data_len = self.data_len(vcid, qos, oid)
        if oid and self.proto == "uslp":
            zl = data_len - 3  # TFDF Header (3 octets) and the TFDZ (732.1 4.1.4.1.9)
            data = bytes([(1 << 5) | USLP_UPID_OID]) + u16(zl - 1) + oid_data(zl)
        elif oid:
            data = oid_data(data_len)
        else:
            data = unhx(offer["field_hex"])
            assert len(data) == data_len, (vcid, len(data), data_len)
        mc, vc, cycle = self.counts(vcid, qos, oid)
        ocf_present = self.has_ocf(vcid, oid)
        sh, st = self.sec(vcid, oid)
        parts = []
        if self.proto == "tm":
            fsh = b""
            if self.fsh_len(vcid):
                d = self.fsh.get(("vc", vcid), self.fsh.get(("mc",)))
                fsh = bytes([len(d)]) + d  # version 00, length - 1 in 6 bits (132.0 4.1.3.2)
            fhp = TM_OID_FHP if oid else offer["fhp"]
            hdr = ((0 << 46) | (c["scid"] << 36) | (vcid << 33) | (int(ocf_present) << 32) |
                   (mc << 24) | (vc << 16) | (int(bool(fsh)) << 15) | (0 << 14) | (0 << 13) |
                   (3 << 11) | fhp)
            parts.append(("hdr", hdr.to_bytes(6, "big")))
            parts.append(("fsh", fsh))
        elif self.proto == "aos":
            scid = c["scid"]
            hdr = ((1 << 46) | ((scid & 0xFF) << 38) | (vcid << 32) | (vc << 8) |
                   (0 << 7) | (int(bool(c.get("cycle_in_use"))) << 6) | ((scid >> 8) << 4) | cycle)
            parts.append(("hdr", hdr.to_bytes(6, "big")))
            if c.get("fhec"):
                parts.append(("fhec", b"\x00\x00"))  # not yet computed (API-074)
            parts.append(("iz", self.insert))
        else:
            n = self.vcf_len(vcid, qos, oid)
            h1 = (0b1100 << 28) | (c["scid"] << 12) | (c.get("source_or_destination", 0) << 11) | \
                 (vcid << 5) | (mapid << 1) | 0
            flags = (int(qos == EXP) << 7) | (0 << 6) | (int(ocf_present) << 3) | n
            hdr = h1.to_bytes(4, "big") + u16(self.F - 1) + bytes([flags])
            if n:
                hdr += vc.to_bytes(n, "big")
            parts.append(("hdr", hdr))
            parts.append(("iz", self.insert))
        parts.append(("sech", bytes(sh)))
        parts.append(("data", bytes(data)))
        parts.append(("sect", bytes(st)))
        if ocf_present:
            parts.append(("ocf", self.pick_ocf(vcid)))
        if self.fecf:
            parts.append(("fecf", b"\x00\x00"))  # not yet computed (API-074)
        frame = b"".join(p[1] for p in parts)
        assert len(frame) == self.F, (len(frame), self.F)
        off = 0
        rng = {}
        for name, b in parts:
            rng[name] = [off, off + len(b)]
            off += len(b)
        return frame, rng

    def pick_ocf(self, vcid):
        if ("vc", vcid) in self.ocf:
            return self.ocf[("vc", vcid)]
        return self.ocf[("mc",)]

    # --- the step
    def step(self, inp, now):
        (name, arg), = inp.items()
        if name == "offer":
            v = arg["vc"]
            if v not in self.vcs:
                raise Err("UnknownChannel")
            if self.slots[v] is not None:
                raise Err("WouldBlock")  # LNK-076
            self.slots[v] = dict(arg)
            return []
        if name == "set_ocf":
            key = ("mc",) if arg["scope"] == "mc" else ("vc", arg["vc"])
            self.ocf[key] = unhx(arg["ocf_hex"])
            return []
        if name == "set_fsh":
            key = ("mc",) if arg["scope"] == "mc" else ("vc", arg["vc"])
            d = unhx(arg["data_hex"])
            if len(d) != len(self.fsh[key]):
                raise Err("LengthMismatch", "fsh_len")
            self.fsh[key] = d
            return []
        if name == "set_insert":
            d = unhx(arg["data_hex"])
            if len(d) != self.izl:
                raise Err("LengthMismatch", "insert_zone_len")
            self.insert = d
            self.fresh = True
            return []
        if name == "opportunity":
            offered = [v for v in self.vcs if self.slots[v] is not None]
            if offered:
                vcid = self.policy.select(offered)
                offer = self.slots[vcid]
                self.slots[vcid] = None
                frame, rng = self.build(vcid, "data", offer, None)
                kind = "data"
            else:
                vcid = self.oid_vcid()
                frame, rng = self.build(vcid, "oid", None, self.oid.take)
                kind = "oid"
            tx = {"kind": kind, "vc": vcid, "frame_hex": hx(frame),
                  "security_header": rng["sech"], "data": rng["data"],
                  "security_trailer": rng["sect"]}
            out = [{"transmit": tx}]
            if self.izl:
                if not self.fresh:
                    out.append({"notify": {"kind": "InsertSduRepeated"}})  # OI-68 (a)
                self.fresh = False
            return out
        if name == "reconfigure":
            held = sum(len(unhx(s["field_hex"])) for s in self.slots.values() if s)
            carry = (self.oid, self.mc_count, self.vc_count)
            self.__init__(arg["config"], carry)
            return [{"notify": {"kind": "Discarded", "octets": held}}] if held else []
        raise AssertionError(name)

    def deadline(self):
        return None


def c_oid_len(cfg):
    return cfg.get("oid", {}).get("vcf_len", 0)


# --------------------------------------------------------------------------------------------
# MapSegmenter (LNK-040..042; 04 section 2.4)
# --------------------------------------------------------------------------------------------

FLAGS = {"01": 1, "00": 0, "10": 2, "11": 3}  # TC sequence flags (232.0 Table 4-2)
USLP_RULE = {"011": 3, "100": 4, "101": 5, "110": 6, "111": 7}  # 732.1 Table 4-3


class MapSegmenter:
    """TC with or without a Segment Header (232.0 4.1.3, 4.3.1-4.3.3) and USLP variable-length
    TFDFs (732.1 4.2.2.2, 4.2.3.2, 4.2.4, 4.2.6.2). Submitted SDUs are queued in order; NextFdu
    hands out one FDU or TFDF, finishing an SDU before it starts another (LNK-041)."""

    def __init__(self, cfg):
        self.cfg = cfg
        self.proto = cfg["protocol"]
        self.maps = {m["map_id"]: m for m in cfg.get("maps", [])}
        self.queue = []  # {"map", "data", "packet"}
        self.cur = None  # [item, pos] while an SDU is segmented
        # octet stream (USLP rule 011)
        self.buf = bytearray()
        self.backlog = bytearray()
        self.buf_map = None
        self.started_at = None
        self.releasable = False
        if self.proto == "tc":
            self.cap = cfg["max_fdu_len"] - (1 if cfg["segment_header"] else 0)
        else:
            self.cap = cfg["max_tfdf_len"] - 1

    def mapcfg(self, m):
        if m not in self.maps:
            raise Err("UnknownChannel")
        return self.maps[m]

    def submit(self, m, data, packet):
        cfg = self.cfg
        if self.proto == "tc" and not cfg["segment_header"]:
            if len(data) > self.cap:
                raise Err("OutOfRange", "max_fdu_len")  # LNK-040
            self.queue.append({"map": None, "data": data, "packet": packet})
            return
        mc = self.mapcfg(m)
        if len(data) > mc.get("max_sdu_len", 1 << 30):
            raise Err("OutOfRange", "max_sdu_len")
        if len(data) > self.cap and self.proto == "tc" and not mc["segmentation"]:
            raise Err("OutOfRange", "max_fdu_len")
        self.queue.append({"map": m, "data": data, "packet": packet})

    def pump_stream(self, now):
        while not self.releasable and self.backlog:
            if not self.buf:
                self.started_at = now
            room = self.cap - len(self.buf)
            take = self.backlog[:room]
            self.buf += take
            del self.backlog[:len(take)]
            if len(self.buf) == self.cap:
                self.releasable = True

    def fdu_tc(self, h, qos):
        cfg = self.cfg
        if not cfg["segment_header"]:
            item = self.queue.pop(0)
            data = item["data"]
            if item["packet"] and cfg.get("blocking"):
                while self.queue and self.queue[0]["packet"] and \
                        len(data) + len(self.queue[0]["data"]) <= self.cap:
                    data += self.queue.pop(0)["data"]
            return {"fdu": {"handle": h, "qos": qos, "fdu_hex": hx(data)}}
        if self.cur is None:
            item = self.queue.pop(0)
            m = item["map"]
            mc = self.maps[m]
            data = item["data"]
            if len(data) <= self.cap:  # one FDU, flags 11
                if item["packet"] and mc["blocking"]:
                    while self.queue and self.queue[0]["packet"] and self.queue[0]["map"] == m and \
                            len(data) + len(self.queue[0]["data"]) <= self.cap:
                        data += self.queue.pop(0)["data"]
                return {"fdu": {"handle": h, "map": m, "flags": "11", "qos": qos,
                                "fdu_hex": hx(bytes([(3 << 6) | m]) + data)}}
            self.cur = [item, self.cap]
            return {"fdu": {"handle": h, "map": m, "flags": "01", "qos": qos,
                            "fdu_hex": hx(bytes([(1 << 6) | m]) + data[:self.cap])}}
        item, pos = self.cur
        m = item["map"]
        rest = item["data"][pos:]
        if len(rest) > self.cap:
            self.cur[1] = pos + self.cap
            return {"fdu": {"handle": h, "map": m, "flags": "00", "qos": qos,
                            "fdu_hex": hx(bytes([(0 << 6) | m]) + rest[:self.cap])}}
        self.cur = None
        return {"fdu": {"handle": h, "map": m, "flags": "10", "qos": qos,
                        "fdu_hex": hx(bytes([(2 << 6) | m]) + rest)}}

    def fdu_uslp(self, h, qos):
        def mk(rule, m, upid, body):
            return {"fdu": {"handle": h, "map": m, "rule": format(rule, "03b"), "upid": upid,
                            "qos": qos, "fdu_hex": hx(bytes([(rule << 5) | upid]) + body)}}
        if self.cur is None:
            item = self.queue.pop(0)
            m = item["map"]
            mc = self.maps[m]
            upid = mc["upid"]
            data = item["data"]
            if len(data) <= self.cap:  # rule 111, no segmentation
                if item["packet"] and mc["blocking"]:
                    while self.queue and self.queue[0]["packet"] and self.queue[0]["map"] == m and \
                            len(data) + len(self.queue[0]["data"]) <= self.cap:
                        data += self.queue.pop(0)["data"]
                return mk(7, m, upid, data)
            self.cur = [item, self.cap]
            return mk(4, m, upid, data[:self.cap])
        item, pos = self.cur
        m = item["map"]
        upid = self.maps[m]["upid"]
        rest = item["data"][pos:]
        if len(rest) > self.cap:
            self.cur[1] = pos + self.cap
            return mk(5, m, upid, rest[:self.cap])
        self.cur = None
        return mk(6, m, upid, rest)

    def step(self, inp, now):
        (name, arg), = inp.items()
        if name in ("submit_packet", "submit_sdu"):
            data = unhx(arg["packet_hex" if name == "submit_packet" else "sdu_hex"])
            m = arg.get("map")
            if name == "submit_packet":
                check_packet(data, self.cfg.get("valid_pvns", [0, 7]), 1 << 30)
            self.submit(m, data, name == "submit_packet")
            return []
        if name == "submit_octets":
            m = arg["map"]
            self.mapcfg(m)
            self.buf_map = m
            self.backlog += unhx(arg["data_hex"])
            self.pump_stream(now)
            return []
        if name == "timeout":
            dl = self.stream_deadline()
            if dl is not None and now >= dl:
                self.releasable = True
            return []
        if name == "next_fdu":
            h, qos = arg["handle"], arg["qos"]
            if self.cur is not None or self.queue:
                return [self.fdu_tc(h, qos) if self.proto == "tc" else self.fdu_uslp(h, qos)]
            if self.releasable:
                mc = self.maps[self.buf_map]
                body = bytes(self.buf)
                self.buf = bytearray()
                self.releasable = False
                self.started_at = None
                self.pump_stream(now)
                return [{"fdu": {"handle": h, "map": self.buf_map, "rule": "011",
                                 "upid": mc["upid"], "qos": qos,
                                 "fdu_hex": hx(bytes([(3 << 5) | mc["upid"]]) + body)}}]
            return []
        if name == "reconfigure":
            held = sum(len(q["data"]) for q in self.queue)
            if self.cur:
                held += len(self.cur[0]["data"]) - self.cur[1]
            held += len(self.buf) + len(self.backlog)
            self.__init__(arg["config"])
            return [{"notify": {"kind": "Discarded", "octets": held}}] if held else []
        raise AssertionError(name)

    def stream_deadline(self):
        if self.started_at is None or self.releasable:
            return None
        d = self.maps[self.buf_map].get("release_deadline_us")
        return None if d is None else self.started_at + d

    def deadline(self):
        return self.stream_deadline()


ENGINES = {
    "packet-generator": PacketGen,
    "access-generator": AccessGen,
    "bitstream-generator": BitstreamGen,
    "channel-mux": ChannelMux,
    "map-segmenter": MapSegmenter,
}
HAS_TIMER = ("packet-generator", "map-segmenter")


# --------------------------------------------------------------------------------------------
# Replay
# --------------------------------------------------------------------------------------------


def replay(engine, cfg, inputs):
    """Steps (as stored in a file) for a list of (at_us, input)."""
    eng = ENGINES[engine](copy.deepcopy(cfg))
    steps = []
    last = 0
    for at, inp in inputs:
        assert at >= last, "at_us never decreases"
        last = at
        saved = copy.deepcopy(eng.__dict__)
        try:
            out = eng.step(copy.deepcopy(inp), at)
        except Err as e:
            eng.__dict__.clear()
            eng.__dict__.update(saved)
            out = err_out(e)
        st = {"at_us": at, "input": inp, "expect": out}
        if engine in HAS_TIMER:
            d = eng.deadline()
            if d is None:
                st["expect_no_timeout"] = True
            else:
                st["expect_timeout_us"] = d
        steps.append(st)
    return steps


# --------------------------------------------------------------------------------------------
# Scenario definitions
# --------------------------------------------------------------------------------------------


class Sc:
    """A scenario under construction: an id, the requirements, and a script of inputs."""

    def __init__(self, sid, reqs, desc, cfg=None):
        self.id = sid
        self.reqs = reqs
        self.desc = desc
        self.cfg = cfg
        self.inputs = []
        self.t = 0

    def at(self, t):
        self.t = t
        return self

    def do(self, inp, t=None):
        if t is not None:
            self.t = t
        self.inputs.append((self.t, inp))
        return self

    def tick(self, dt):
        self.t += dt
        return self


def submit_packet(p):
    return {"submit_packet": {"packet_hex": hx(p)}}


def release():
    return {"release": {}}


def timeout():
    return {"timeout": {}}


NOTE_BLIND = (" Model test-vectors/models/link_send.py, written blind (TV-023) by a Claude agent "
              "from the Blue Books and docs/ only, without reading any implementation. The input "
              "scripts were chosen by hand; the expected outputs are the model's. The file is "
              "written by the model (--write) and never edited by hand.")


def pg_cfg(proto, **kw):
    base = {"protocol": proto, "vc": 1, "valid_pvns": [0, 7], "max_packet_len": 64,
            "queue_capacity": 64, "idle_policy": "oid", "idle_pattern_hex": "00"}
    if proto != "uslp":
        base["idle_packet_kind"] = "space_packet"
    base.update(kw)
    return base


def pg_common(proto, fl, sc_prefix, deadline=1000):
    """Scenarios shared by the TM, AOS, and USLP packet generators; `fl` is the field length."""
    cfg = pg_cfg(proto, field_len=fl, release_deadline_us=deadline)
    z = fl - FHP[proto if proto != "aos" else "aos5"][0]  # the zone length
    out = []
    # packets back to back, ending exactly at the end of the zone (LNK-011)
    s = Sc(sc_prefix + "lnk-011-contiguous-exact-fit", ["LNK-011"],
           "Two packets fill the zone exactly and the next packet starts the next field: FHP 0 in "
           "both; a field that was released is not offered again.")
    a, b = spp_packet(1, 9, 0, 0x10), spp_packet(1, z - 9, 1, 0x20)
    s.do(submit_packet(a), 0).do(submit_packet(b), 10).do(release(), 20)
    s.do(submit_packet(spp_packet(2, z, 0, 0x30)), 30).do(release(), 40).do(release(), 50)
    out.append(s)
    # a packet spans fields: FHP in the field where the next packet starts, and "no packet starts"
    s = Sc(sc_prefix + "lnk-011-spanning-no-start", ["LNK-011"],
           "A packet of 2 zones + 3 octets: FHP 0, then no packet starts, then the 3-octet "
           "residue and a packet that starts after it (the residue is not a start).")
    s.do(submit_packet(spp_packet(3, 2 * z + 3, 0, 0x40)), 0)
    s.do(submit_packet(spp_packet(4, z - 3, 0, 0x50)), 10)
    s.do(release(), 20).do(release(), 30).do(release(), 40).do(release(), 50)
    out.append(s)
    # OI-69: tail of a spanning packet ends at the end of the zone; no packet starts
    s = Sc(sc_prefix + "lnk-011-oi69-tail-ends-at-zone-end", ["LNK-011"],
           "A packet of exactly 2 zones: the second field holds only its tail, which ends at the "
           "last octet and starts no packet: the 'no packet starts' value, not 0 (OI-69); the next "
           "packet starts the next field at 0.")
    s.do(submit_packet(spp_packet(5, 2 * z, 0, 0x60)), 0)
    s.do(submit_packet(spp_packet(6, z, 0, 0x70)), 10)
    s.do(release(), 20).do(release(), 30).do(release(), 40)
    out.append(s)
    # a packet starts mid-field after the tail of the previous one
    s = Sc(sc_prefix + "lnk-011-start-after-tail", ["LNK-011"],
           "The tail of a spanning packet, then two packets: FHP is the offset of the first "
           "packet that starts, not of the second.")
    s.do(submit_packet(spp_packet(7, z + 5, 0, 0x01)), 0)
    s.do(submit_packet(spp_packet(7, 7, 1, 0x02)), 5)
    s.do(submit_packet(spp_packet(7, z - 12, 2, 0x03)), 6)
    s.do(release(), 10).do(release(), 20).do(release(), 30)
    out.append(s)
    # mixed PVNs on one VC join one stream in submission order (LNK-010)
    s = Sc(sc_prefix + "lnk-010-mixed-pvn-one-stream", ["LNK-010", "LNK-011"],
           "A Space Packet, an Encapsulation Packet, and a Space Packet join one stream in "
           "submission order.")
    s.do(submit_packet(spp_packet(8, 8, 0, 0x11)), 0).do(submit_packet(epp_packet(7, 7, 0x21)), 1)
    s.do(submit_packet(spp_packet(8, z - 15, 1, 0x31)), 2).do(release(), 10)
    out.append(s)
    # SubmitPacket checks (LNK-010)
    s = Sc(sc_prefix + "lnk-010-submit-errors", ["LNK-010"],
           "A PVN outside the valid set, a packet longer than the maximum, and a length field that "
           "disagrees with the octets given are refused with no state change.")
    bad = bytearray(spp_packet(1, 8)); bad[0] |= 0x20  # PVN 1
    s.do(submit_packet(bytes(bad)), 0)
    s.do(submit_packet(spp_packet(1, 65)), 1)
    s.do(submit_packet(spp_packet(1, 12)[:-1]), 2)
    s.do(release(), 3)
    out.append(s)
    # release deadline (LNK-012) and idle completion (LNK-013)
    s = Sc(sc_prefix + "lnk-012-deadline-release", ["LNK-012", "LNK-013"],
           "A started field is releasable only at its deadline: an early Timeout and an early "
           "Release change nothing; at the deadline Timeout marks it and Release returns it "
           "completed with an idle packet; the next field starts when its first octet is written.")
    s.do(submit_packet(spp_packet(9, 8, 0, 0x41)), 100)
    s.do(timeout(), 600).do(release(), 700).do(timeout(), 999 + 100 - 1).do(release(), 1099)
    s.do(timeout(), 1100).do(release(), 1100)
    s.do(submit_packet(spp_packet(9, 8, 1, 0x51)), 1200).do(timeout(), 2200).do(release(), 2200)
    out.append(s)
    s = Sc(sc_prefix + "lnk-012-no-deadline-only-when-full", ["LNK-012"],
           "Without a release deadline a started field is released only when full; Timeout is "
           "harmless.", cfg=pg_cfg(proto, field_len=fl))
    s.do(submit_packet(spp_packet(9, 8, 0, 0x41)), 0).do(timeout(), 10 ** 9).do(release(), 10 ** 9)
    s.do(submit_packet(spp_packet(9, z - 8, 1, 0x51)), 10 ** 9 + 1).do(release(), 10 ** 9 + 2)
    out.append(s)
    if proto == "uslp":
        # LNK-013 USLP: one Encapsulation Idle Packet of exactly the remainder, whatever the
        # valid-PVN set and however short the remainder (OI-34, USLP-010)
        s = Sc(sc_prefix + "lnk-013-eip-exact-remainder", ["LNK-013", "LNK-012"],
               "The fill is one Encapsulation Idle Packet of exactly the remainder, always, also "
               "when the remainder is 1, 2, or below 7, and although only Space Packets are "
               "valid on the VC (no residue is carried to the next field).",
               cfg=pg_cfg(proto, field_len=fl, release_deadline_us=100, valid_pvns=[0],
                          idle_pattern_hex="A5"))
        for k, rem in enumerate((1, 2, 6, 7, 9)):
            t = k * 1000
            s.do(submit_packet(spp_packet(10, z - rem, k, 0x61)), t).do(timeout(), t + 100)
            s.do(release(), t + 100)
        out.append(s)
    else:
        # idle packet kinds (LNK-013): an Encapsulation Idle Packet of exactly the remainder
        s = Sc(sc_prefix + "lnk-013-encapsulation-idle", ["LNK-013"],
               "Idle completion with Encapsulation Idle Packets of exactly the remainder: 1 octet "
               "(the 1-octet header), 2 octets, and a longer remainder.",
               cfg=pg_cfg(proto, field_len=fl, release_deadline_us=100,
                          idle_packet_kind="encapsulation", idle_pattern_hex="A5"))
        for k, rem in enumerate((1, 2, 5)):
            t = k * 1000
            s.do(submit_packet(spp_packet(10, z - rem, k, 0x61)), t).do(timeout(), t + 100)
            s.do(release(), t + 100)
        out.append(s)
        # LNK-013 Space Packet idle: remainders 7 and 8 fit; 6 and 1 spill
        s = Sc(sc_prefix + "lnk-013-spp-idle-sizes", ["LNK-013"],
               "Idle completion with a Space Packet Idle Packet: a remainder of 7 octets is the "
               "shortest idle packet (no spill); 8 is one packet of 8 octets.",
               cfg=pg_cfg(proto, field_len=fl, release_deadline_us=100, idle_pattern_hex="5A"))
        for k, rem in enumerate((7, 8)):
            t = k * 1000
            s.do(submit_packet(spp_packet(10, z - rem, k, 0x61)), t).do(timeout(), t + 100)
            s.do(release(), t + 100)
        out.append(s)
        s = Sc(sc_prefix + "lnk-013-spp-idle-spill", ["LNK-012", "LNK-013"],
               "A remainder of 3 octets (below 7): a 7-octet idle packet is begun, its last 4 octets "
               "are the first octets of the next field (before any packet) and are not a start; that "
               "field is started when they are written, so its deadline runs from the Release.")
        s.do(submit_packet(spp_packet(11, z - 3, 0, 0x71)), 0).do(timeout(), 1000).do(release(), 1000)
        s.do(submit_packet(spp_packet(11, 9, 1, 0x72)), 1500).do(release(), 1600)
        s.do(timeout(), 2000).do(release(), 2000)
        out.append(s)
        s = Sc(sc_prefix + "lnk-013-spp-idle-spill-1", ["LNK-013"],
               "A remainder of 6 octets: the 7-octet idle packet spills 1 octet; a remainder of 1 "
               "octet spills 6 (a field holding only a residue and the next idle completion).")
        s.do(submit_packet(spp_packet(11, z - 6, 0, 0x71)), 0).do(timeout(), 1000).do(release(), 1000)
        s.do(timeout(), 2000).do(release(), 2000)
        out.append(s)
    # LNK-015 WouldBlock
    s = Sc(sc_prefix + "lnk-015-would-block", ["LNK-015"],
           "A packet is taken whole or refused with WouldBlock and no state change: a full field "
           "waits for Release, the queue fills to its capacity, and the next packet is refused; "
           "after a Release it is taken.", cfg=pg_cfg(proto, field_len=fl, queue_capacity=2 * z))
    s.do(submit_packet(spp_packet(12, z, 0, 0x01)), 0)
    s.do(submit_packet(spp_packet(12, z, 1, 0x02)), 1)
    s.do(submit_packet(spp_packet(12, z, 2, 0x03)), 2)
    s.do(submit_packet(spp_packet(12, 7, 3, 0x04)), 3)
    s.do(release(), 4).do(submit_packet(spp_packet(12, 7, 3, 0x04)), 5)
    s.do(release(), 6).do(release(), 7).do(release(), 8)
    out.append(s)
    # Reconfigure (LNK-003, input kinds of 04 section 4)
    s = Sc(sc_prefix + "lnk-003-reconfigure-discards", ["LNK-003", "LNK-011"],
           "Reconfigure discards the started field and the queue and returns one Discarded with "
           "the octets; a second Reconfigure discards nothing and returns nothing.")
    s.do(submit_packet(spp_packet(13, z + 4, 0, 0x01)), 0)
    s.do(submit_packet(spp_packet(13, 9, 1, 0x02)), 1)
    s.do({"reconfigure": {"config": dict(cfg, idle_pattern_hex="FF")}}, 2)
    s.do({"reconfigure": {"config": dict(cfg, idle_pattern_hex="FF")}}, 3)
    s.do(submit_packet(spp_packet(13, 8, 0, 0x03)), 4).do(timeout(), 1004).do(release(), 1004)
    out.append(s)
    return cfg, out


def idle_policy_scenarios(proto, fl, prefix):
    cfg = pg_cfg(proto, field_len=fl, release_deadline_us=1000)
    z = fl - FHP[proto if proto != "aos" else "aos5"][0]
    out = []
    s = Sc(prefix + "lnk-014-oid-policy-offers-nothing", ["LNK-014"],
           "With the Oid policy (default) the VC offers no field when none is releasable: Release "
           "returns nothing, with a field started or not.")
    s.do(release(), 0).do(submit_packet(spp_packet(1, 9)), 10).do(release(), 20)
    out.append(s)
    s = Sc(prefix + "lnk-014-idle-field-policy", ["LNK-014", "LNK-013"],
           "With the IdleField policy the VC always has a field to offer: a releasable field is "
           "offered first, then an idle field each time; the idle field does not disturb a "
           "the data field.", cfg=pg_cfg(proto, field_len=fl, idle_policy="idle_field",
                                          idle_pattern_hex="C3"))
    s.do(release(), 0).do(submit_packet(spp_packet(1, z, 0, 0x21)), 10).do(release(), 20)
    s.do(release(), 30).do(release(), 40)
    out.append(s)
    return cfg, out


def build_pg(proto, fl):
    pre = {"tm": "link-send-pg-tm-", "aos": "link-send-pg-aos-", "uslp": "link-send-pg-uslp-"}[proto]
    cfg, scs = pg_common(proto, fl, pre)
    _, more = idle_policy_scenarios(proto, fl, pre)
    return cfg, scs + more


# --------------------------------------------------------------------------------------------
# AccessGenerator and BitstreamGenerator scenarios (LNK-020, LNK-021)
# --------------------------------------------------------------------------------------------


def fld(n, start=0):
    """n octets of a recognisable pattern (incrementing from `start`)."""
    return bytes((start + i) & 0xFF for i in range(n))


def sdu(s):
    return {"submit_sdu": {"sdu_hex": hx(s)}}


def access_tm_aos(proto):
    pre = "link-send-ag-%s-" % proto
    cfg = {"protocol": proto, "vc": 2, "field_len": 12}
    out = []
    s = Sc(pre + "lnk-020-one-sdu-per-field", ["LNK-020"],
           "SDUs of exactly the data field length, one per field and unchanged; with nothing "
           "queued Release returns nothing.")
    s.do(release(), 0).do(sdu(fld(12, 0x10)), 10).do(sdu(fld(12, 0x20)), 11)
    s.do(release(), 20).do(release(), 30).do(release(), 40)
    out.append(s)
    return cfg, out


def access_uslp():
    cfg = {"protocol": "uslp", "vc": 2, "field_len": 11, "upid": 5, "map_id": 0,
           "qos": SC, "max_sdu_len": 40, "idle_pattern_hex": "00"}
    z = 8  # the TFDZ is the field minus the 3-octet TFDF header
    out = []
    s = Sc("link-send-ag-uslp-lnk-020-sdu-fits-one-tfdz", ["LNK-020"],
           "An SDU shorter than the TFDZ: rule 001, the Last Valid Octet Pointer is the offset of "
           "its last octet, the rest of the TFDZ is the idle pattern; a second SDU does not share "
           "the TFDZ.", cfg=dict(cfg, idle_pattern_hex="A5"))
    s.do(sdu(fld(5, 0x10)), 0).do(sdu(fld(3, 0x20)), 1).do(release(), 10).do(release(), 20)
    s.do(release(), 30)
    out.append(s)
    s = Sc("link-send-ag-uslp-lnk-020-sdu-ends-at-zone-end", ["LNK-020"],
           "An SDU of exactly the TFDZ length: rule 001 and the pointer is the last octet of the "
           "TFDZ (not all ones).")
    s.do(sdu(fld(z, 0x30)), 0).do(release(), 10)
    out.append(s)
    s = Sc("link-send-ag-uslp-lnk-020-sdu-spans-frames", ["LNK-020"],
           "An SDU of 2 zones + 3 octets: rule 001 with all ones, rule 010 with all ones, rule "
           "010 with the pointer at its last octet; the next SDU starts with rule 001.")
    s.do(sdu(fld(2 * z + 3, 0x40)), 0).do(sdu(fld(2, 0x50)), 1)
    for k in range(4):
        s.do(release(), 10 * (k + 1))
    out.append(s)
    s = Sc("link-send-ag-uslp-lnk-020-sdu-spans-exact", ["LNK-020"],
           "An SDU of exactly 2 zones: rule 001 with all ones, then rule 010 with the pointer at "
           "the last octet of the TFDZ, the end of the SDU being the end of the zone.")
    s.do(sdu(fld(2 * z, 0x60)), 0).do(release(), 10).do(release(), 20).do(release(), 30)
    out.append(s)
    s = Sc("link-send-ag-uslp-lnk-020-mapa-expedited", ["LNK-020"],
           "A MAPA service on MAP 3, expedited: the MAP ID and the QoS are in the action; the SDU "
           "longer than the maximum is refused with no state change.",
           cfg=dict(cfg, map_id=3, qos=EXP))
    s.do(sdu(fld(41)), 0).do(sdu(fld(6, 0x70)), 1).do(release(), 10)
    out.append(s)
    s = Sc("link-send-ag-uslp-lnk-003-reconfigure", ["LNK-003", "LNK-020"],
           "Reconfigure discards the SDU in progress and the queue, with the octets left; the "
           "next SDU starts afresh with rule 001.")
    s.do(sdu(fld(20, 0x10)), 0).do(sdu(fld(4, 0x20)), 1).do(release(), 10)
    s.do({"reconfigure": {"config": cfg}}, 20).do(sdu(fld(4, 0x30)), 30).do(release(), 40)
    out.append(s)
    return cfg, out


def bits(hexstr, n):
    return {"submit_bits": {"bits_hex": hexstr, "bit_len": n}}


def bitstream_aos():
    cfg = {"protocol": "aos", "vc": 3, "field_len": 6, "idle_pattern_hex": "00"}  # zone: 4 octets
    out = []
    s = Sc("link-send-bg-aos-lnk-021-full-and-partial", ["LNK-021"],
           "Bits fill the zone in order and unchanged: a full zone has pointer all ones; a "
           "partial zone has the pointer at the last valid bit and the idle pattern after it; "
           "the bits that did not fit start the next B_PDU.")
    s.do(bits("0123456789", 40), 0).do(release(), 10).do(release(), 20)
    out.append(s)
    s = Sc("link-send-bg-aos-lnk-021-idle-bpdu", ["LNK-021"],
           "With no bits at release time an idle B_PDU is generated: pointer all ones minus "
           "one, the zone the idle pattern.", cfg=dict(cfg, idle_pattern_hex="FF"))
    s.do(release(), 0).do(bits("A0", 3), 5).do(release(), 10).do(release(), 20)
    out.append(s)
    s = Sc("link-send-bg-aos-lnk-021-bit-granularity", ["LNK-021"],
           "Bits that are not a multiple of 8: 1 valid bit (pointer 0), 31 valid bits (pointer "
           "30, the last bit before the end of the zone), then 33 bits: the zone is full with "
           "pointer all ones and 1 bit remains.")
    s.do(bits("80", 1), 0).do(release(), 10)
    s.do(bits("FFFFFFFE", 31), 20).do(release(), 30)
    s.do(bits("00000000FF", 33), 40).do(release(), 50).do(release(), 60)
    out.append(s)
    s = Sc("link-send-bg-aos-lnk-003-reconfigure", ["LNK-003", "LNK-021"],
           "Reconfigure discards the queued bits and returns the octets that held them.")
    s.do(bits("FFFFFFFFFF", 40), 0).do({"reconfigure": {"config": cfg}}, 5).do(release(), 10)
    out.append(s)
    return cfg, out


# --------------------------------------------------------------------------------------------
# ChannelMux scenarios (LNK-030..036, LNK-076)
# --------------------------------------------------------------------------------------------


class MuxSc(Sc):
    """A scenario that keeps a shadow engine, so a script can offer a field to every VC whose
    slot was consumed."""

    def __init__(self, sid, reqs, desc, base):
        Sc.__init__(self, sid, reqs, desc, base)
        self.shadow = ChannelMux(copy.deepcopy(base))
        self.log = []

    def do(self, inp, t=None):
        Sc.do(self, inp, t)
        try:
            out = self.shadow.step(copy.deepcopy(inp), self.t)
        except Err as e:
            out = err_out(e)
        self.log.append(out)
        return self

    def served(self):
        for o in self.log[-1]:
            if "transmit" in o:
                return o["transmit"]["vc"], o["transmit"]["kind"]
        return None


def offer(vc, field, fhp=None, **extra):
    d = {"vc": vc, "field_hex": hx(field)}
    if fhp is not None:
        d["fhp"] = fhp
    d.update(extra)
    return {"offer": d}


def opp():
    return {"opportunity": {}}


TM_VCS = [
    {"vcid": 1, "role": "packets", "priority": 1, "weight": 1},
    {"vcid": 2, "role": "packets", "priority": 3, "weight": 1},
    {"vcid": 3, "role": "packets", "priority": 2, "weight": 1},
    {"vcid": 7, "role": "packets", "priority": 0, "weight": 1},
]


def tm_cfg(**kw):
    c = {"protocol": "tm", "scid": 0x0A5, "frame_len": 24, "fecf": True,
         "policy": {"kind": "fixed_priority"}, "oid_vc": 7,
         "vcs": copy.deepcopy(TM_VCS)}
    c.update(kw)
    return c


def vc_with(vcs, vcid, **kw):
    out = copy.deepcopy(vcs)
    for v in out:
        if v["vcid"] == vcid:
            v.update(kw)
    return out


def mux_tm():
    cfg = tm_cfg()
    D = 16  # the data field: 24 - 6 - 2
    out = []
    s = MuxSc("link-send-mux-tm-lnk-030-oid-frames", ["LNK-030", "LNK-036", "LNK-032"],
              "With no VC offering, each Opportunity returns an OID frame on the dedicated VC 7: "
              "FHP 0x7FE, data from the OID generator, which continues across frames; the MC and "
              "VC Frame Counts count the OID frames.", cfg)
    for k in range(4):
        s.do(opp(), 100 * k)
    out.append(s)
    s = MuxSc("link-send-mux-tm-lnk-030-data-then-oid", ["LNK-030", "LNK-032"],
              "A frame of an offered field (VC 2, FHP 5), then an OID frame; the MC Frame Count "
              "counts both, the VC Frame Count of VC 7 starts at 0 for the OID frame.", cfg)
    s.do(offer(2, fld(D, 0x10), 5), 0).do(opp(), 10).do(opp(), 20)
    out.append(s)
    s = MuxSc("link-send-mux-tm-lnk-032-counts-independent", ["LNK-032"],
              "The MC Frame Count increments for every frame of the channel, the VC Frame Count "
              "for every frame of its VC, assigned in the order of transmission.", cfg)
    for k, v in enumerate((1, 2, 1, 1)):
        s.do(offer(v, fld(D, 0x20 * (k + 1)), 0), 100 * k).do(opp(), 100 * k + 10)
    s.do(opp(), 500)
    s.do(offer(2, fld(D, 0x90), 0), 600).do(opp(), 610)
    out.append(s)
    s = MuxSc("link-send-mux-tm-lnk-032-counts-wrap", ["LNK-032", "LNK-036", "LNK-030"],
              "258 OID frames: the MC Frame Count and the VC Frame Count of VC 7 (8 bits) return "
              "to 0 after 255; the OID generator continues without a restart.",
              cfg)
    for k in range(258):
        s.do(opp(), 10 * k)
    out.append(s)
    # policies
    p = tm_cfg(policy={"kind": "fixed_priority"})
    s = MuxSc("link-send-mux-tm-lnk-031-fixed-priority", ["LNK-031"],
              "Fixed priority: the offered VC with the highest priority (VC 2 over 3 over 1) "
              "goes first; a VC that offers later goes before a lower one that waited.", p)
    for v in (1, 2, 3):
        s.do(offer(v, fld(D, 0x10 * v), 0), 0)
    s.do(opp(), 10).do(opp(), 20).do(offer(2, fld(D, 0x77), 0), 25).do(opp(), 30).do(opp(), 40)
    s.do(opp(), 50)
    out.append(s)
    rr = tm_cfg(policy={"kind": "round_robin"})
    s = MuxSc("link-send-mux-tm-lnk-031-round-robin", ["LNK-031"],
              "Round robin in ascending VCID order, wrapping, from the VC served last: with all of "
              "VC 1, 2, 3 always offering, 1 2 3 1 2 3; VCs that do not offer are skipped; the "
              "first selection after start-up is the lowest VCID.", rr)
    for v in (1, 2, 3):
        s.do(offer(v, fld(D, 0x10 * v), 0), 0)
    t = 10
    for _ in range(6):
        s.do(opp(), t)
        vc, kind = s.served()
        s.do(offer(vc, fld(D, t & 0xFF), 0), t + 1)
        t += 10
    out.append(s)
    s = MuxSc("link-send-mux-tm-lnk-031-round-robin-skips", ["LNK-031"],
              "Round robin skips a VC with no field and wraps: only VC 1 and VC 3 offer; after VC "
              "3 comes VC 1.", rr)
    for v in (1, 3):
        s.do(offer(v, fld(D, 0x10 * v), 0), 0)
    s.do(opp(), 10)
    s.do(offer(1, fld(D, 0x41), 0), 11).do(opp(), 20)
    s.do(offer(3, fld(D, 0x42), 0), 21).do(opp(), 30).do(opp(), 40)
    out.append(s)
    wv = vc_with(TM_VCS, 1, weight=2)
    wv = vc_with(wv, 2, weight=1)
    wv = vc_with(wv, 3, weight=3)
    w = tm_cfg(policy={"kind": "weighted_round_robin"}, vcs=wv)
    s = MuxSc("link-send-mux-tm-lnk-031-weighted-round-robin", ["LNK-031"],
              "Weighted round robin with weights VC 1: 2, VC 2: 1, VC 3: 3 (VC 7, which never "
              "offers, has weight 1 and forfeits its turn): with every VC backlogged the order of "
              "turns is 1 1 2 3 3 3 repeated.", w)
    for v in (1, 2, 3):
        s.do(offer(v, fld(D, 0x10 * v), 0), 0)
    t = 10
    for _ in range(12):
        s.do(opp(), t)
        vc, kind = s.served()
        s.do(offer(vc, fld(D, t & 0xFF), 0), t + 1)
        t += 10
    out.append(s)
    s = MuxSc("link-send-mux-tm-lnk-031-weighted-forfeit", ["LNK-031"],
              "A VC that has no field forfeits the rest of its turn: VC 3 (weight 3) is served "
              "once, with two selections of its turn left; at the next Opportunity it has no "
              "field, so VC 7 forfeits too and VC 1 is served.", w)
    s.do(offer(3, fld(D, 0x31), 0), 0)
    s.do(opp(), 10)
    s.do(offer(1, fld(D, 0x11), 0), 11)
    s.do(opp(), 20)
    out.append(s)
    # OCF and FSH
    ocfv = vc_with(TM_VCS, 1, ocf_init_hex="01020304")
    ocfv = vc_with(ocfv, 7, ocf_init_hex="81020304")
    c = tm_cfg(vcs=ocfv)
    s = MuxSc("link-send-mux-tm-lnk-033-vc-ocf", ["LNK-033"],
              "A VC_OCF slot, initialised by the configuration and replaced by SetOcf, is inserted "
              "in every frame of its VC (the OCF Flag is set, the data field is 4 octets shorter); "
              "a VC without it has none; the OID frame carries the OCF of its VC.", c)
    s.do(offer(1, fld(12, 0x10), 0), 0).do(opp(), 10)
    s.do(offer(2, fld(16, 0x20), 0), 20).do(opp(), 30)
    s.do({"set_ocf": {"scope": "vc", "vc": 1, "ocf_hex": "05060708"}}, 40)
    s.do(offer(1, fld(12, 0x30), 0), 50).do(opp(), 60)
    s.do(offer(1, fld(12, 0x40), 0), 70).do(opp(), 80).do(opp(), 90)
    s.do({"set_ocf": {"scope": "vc", "vc": 7, "ocf_hex": "A1A2A3A4"}}, 95).do(opp(), 100)
    out.append(s)
    c = tm_cfg(mc_ocf_init_hex="0A0B0C0D", mc_fsh_init_hex="EEFF")
    s = MuxSc("link-send-mux-tm-lnk-033-mc-ocf-and-fsh", ["LNK-033"],
              "An MC_OCF and an MC_FSH are in every frame of the master channel, the OID frame "
              "included; SetOcf and SetFsh replace the slots; a Secondary Header of the wrong "
              "length is refused with LengthMismatch (fsh_len).", c)
    s.do(offer(3, fld(9, 0x10), 0), 0).do(opp(), 10).do(opp(), 20)
    s.do({"set_ocf": {"scope": "mc", "ocf_hex": "81828384"}}, 30)
    s.do({"set_fsh": {"scope": "mc", "data_hex": "AA"}}, 31)
    s.do({"set_fsh": {"scope": "mc", "data_hex": "AABB"}}, 32)
    s.do(offer(3, fld(9, 0x20), 0), 40).do(opp(), 50).do(opp(), 60)
    out.append(s)
    c = tm_cfg(vcs=vc_with(TM_VCS, 1, fsh_init_hex="C1C2C3"))
    s = MuxSc("link-send-mux-tm-lnk-033-vc-fsh", ["LNK-033"],
              "A VC_FSH of 3 data octets: the Secondary Header Flag, the ID octet (length - 1) and "
              "the data are in each frame of that VC and not in the others.", c)
    s.do(offer(1, fld(12, 0x10), 0), 0).do(opp(), 10)
    s.do(offer(2, fld(16, 0x20), 0), 20).do(opp(), 30)
    s.do({"set_fsh": {"scope": "vc", "vc": 1, "data_hex": "D1D2D3"}}, 40)
    s.do(offer(1, fld(12, 0x30), 0), 50).do(opp(), 60)
    out.append(s)
    # security
    sv = vc_with(TM_VCS, 1, sec_header_len=4, sec_trailer_len=2)
    c = tm_cfg(vcs=sv)
    s = MuxSc("link-send-mux-tm-lnk-035-security-ranges", ["LNK-035"],
              "A VC with a 4-octet Security Header and a 2-octet Security Trailer: the frame has "
              "them as zero octets and the action gives their ranges and the range of the data; "
              "FECF and the others stay as built (the FECF is not yet computed).", c)
    s.do(offer(1, fld(10, 0x10), 0), 0).do(opp(), 10)
    s.do(offer(2, fld(16, 0x20), 0), 20).do(opp(), 30)
    out.append(s)
    s = MuxSc("link-send-mux-tm-lnk-076-offer-slot", ["LNK-076", "LNK-030"],
              "At most one offered field per VC: a second Offer for the same VC is refused with "
              "WouldBlock and no state change; an Offer for an unknown VC is refused; Opportunity "
              "consumes the slot of the VC it selects; SetOcf, SetFsh and SetInsert give no "
              "action.", tm_cfg(vcs=vc_with(TM_VCS, 1, ocf_init_hex="01020304")))
    s.do(offer(2, fld(16, 0x10), 3), 0).do(offer(2, fld(16, 0x20), 4), 1)
    s.do(offer(9, fld(16, 0x30), 0), 2)
    s.do({"set_ocf": {"scope": "vc", "vc": 1, "ocf_hex": "0A0B0C0D"}}, 3)
    s.do(opp(), 10).do(offer(2, fld(16, 0x20), 4), 11).do(opp(), 20).do(opp(), 30)
    out.append(s)
    s = MuxSc("link-send-mux-tm-lnk-036-reconfigure-keeps-oid", ["LNK-036", "LNK-003", "LNK-032"],
              "Reconfigure discards the offered fields (Discarded, with the octets) but not the "
              "OID generator nor the frame counts (132.0 4.1.2.5.3, 4.1.4.6.2.1): the OID data "
              "and the counts continue.", cfg)
    s.do(opp(), 0).do(opp(), 10).do(offer(1, fld(D, 0x10), 0), 20)
    s.do({"reconfigure": {"config": cfg}}, 30).do(opp(), 40)
    out.append(s)
    return cfg, out


AOS_VCS = [
    {"vcid": 1, "role": "packets", "priority": 1, "weight": 1},
    {"vcid": 2, "role": "vca", "priority": 2, "weight": 1},
    {"vcid": 3, "role": "bitstream", "priority": 3, "weight": 1},
]


def aos_cfg(**kw):
    c = {"protocol": "aos", "scid": 0x1A5, "frame_len": 28, "fecf": True, "fhec": False,
         "cycle_in_use": False, "insert_zone_len": 0,
         "policy": {"kind": "fixed_priority"}, "vcs": copy.deepcopy(AOS_VCS)}
    c.update(kw)
    return c


def mux_aos():
    cfg = aos_cfg()
    D = 20  # 28 - 6 - 2
    out = []
    s = MuxSc("link-send-mux-aos-lnk-030-oid-frames", ["LNK-030", "LNK-032", "LNK-036"],
              "With no VC offering, an OID frame on VCID 63 (the MCID of the channel): data from "
              "the OID generator, which continues; the VC Frame Count of VCID 63 counts them like "
              "any VC; the 10-bit SCID is split over the header.", cfg)
    for k in range(3):
        s.do(opp(), 10 * k)
    out.append(s)
    s = MuxSc("link-send-mux-aos-lnk-032-vc-counts", ["LNK-032", "LNK-030"],
              "The VC Frame Count (24 bits) is kept per VC; VCs 1, 2 and 3 carry an M_PDU, a "
              "VCA_SDU and a B_PDU; the OID frame has its own count.", cfg)
    s.do(offer(1, u16(0) + fld(D - 2, 0x10), 0), 0).do(opp(), 1)
    s.do(offer(2, fld(D, 0x20)), 10).do(opp(), 11)
    s.do(offer(3, u16(0x3FFF) + fld(D - 2, 0x30)), 20).do(opp(), 21)
    s.do(offer(1, u16(0xFFFF) + fld(D - 2, 0x40), 0xFFFF), 30).do(opp(), 31).do(opp(), 40)
    out.append(s)
    c = aos_cfg(fhec=True, cycle_in_use=True)
    s = MuxSc("link-send-mux-aos-lnk-035-fhec-reserved", ["LNK-035", "LNK-032"],
              "With the Frame Header Error Control configured, its two octets are in the frame, "
              "not yet computed (zero); with the VC Frame Count Cycle in use the Use Flag is set "
              "and the cycle (4 bits) is 0 before the first wrap.", c)
    s.do(offer(2, fld(18, 0x20)), 0).do(opp(), 1).do(opp(), 10)
    out.append(s)
    c = aos_cfg(insert_zone_len=3, insert_init_hex="000000")
    s = MuxSc("link-send-mux-aos-lnk-034-insert-zone", ["LNK-034", "LNK-030"],
              "An Insert Zone of one IN_SDU of 3 octets in every frame, the OID frame included: "
              "the slot is replaced by SetInsert; a frame with no new IN_SDU since the last one "
              "repeats the slot's value and the action is followed by Notify(InsertSduRepeated) "
              "(OI-68); SetInsert of another length is refused with LengthMismatch.", c)
    D2 = 17
    s.do({"set_insert": {"data_hex": "010203"}}, 0)
    s.do(offer(2, fld(D2, 0x10)), 1).do(opp(), 10)
    s.do(opp(), 20)
    s.do({"set_insert": {"data_hex": "0405"}}, 25)
    s.do({"set_insert": {"data_hex": "040506"}}, 30).do(opp(), 40)
    s.do({"set_insert": {"data_hex": "070809"}}, 50).do(offer(2, fld(D2, 0x20)), 51).do(opp(), 60)
    s.do(offer(2, fld(D2, 0x30)), 61).do(opp(), 70)
    out.append(s)
    c = aos_cfg(vcs=vc_with(AOS_VCS, 2, ocf_init_hex="01FF00AA"))
    s = MuxSc("link-send-mux-aos-lnk-033-vc-ocf", ["LNK-033"],
              "A VC_OCF in every frame of its VC (VC 2); VC 1 has none and its data field is 4 "
              "octets longer; the OID frame (VCID 63) has none.", c)
    s.do(offer(2, fld(16, 0x20)), 0).do(opp(), 1)
    s.do(offer(1, u16(0) + fld(D - 2, 0x10), 0), 10).do(opp(), 11)
    s.do({"set_ocf": {"scope": "vc", "vc": 2, "ocf_hex": "81000000"}}, 20)
    s.do(offer(2, fld(16, 0x30)), 21).do(opp(), 22).do(opp(), 30)
    out.append(s)
    c = aos_cfg(vcs=vc_with(AOS_VCS, 1, sec_header_len=2, sec_trailer_len=3))
    s = MuxSc("link-send-mux-aos-lnk-035-security-ranges", ["LNK-035"],
              "A 2-octet Security Header and a 3-octet Security Trailer on VC 1: zero octets in "
              "the frame and their ranges in the action.", c)
    s.do(offer(1, u16(0) + fld(13, 0x10), 0), 0).do(opp(), 1)
    out.append(s)
    s = MuxSc("link-send-mux-aos-lnk-076-offer-slot", ["LNK-076"],
              "A second Offer for a VC whose slot is occupied is refused with WouldBlock; "
              "another VC accepts its own.", cfg)
    s.do(offer(2, fld(D, 0x10)), 0).do(offer(2, fld(D, 0x20)), 1).do(offer(1, u16(0) + fld(D - 2), 2), 2)
    s.do(opp(), 10).do(opp(), 20)
    out.append(s)
    return cfg, out


USLP_VCS = [
    {"vcid": 1, "role": "packets", "priority": 1, "weight": 1, "map_id": 0, "qos": SC,
     "vcf_len_sc": 2, "vcf_len_exp": 1},
    {"vcid": 2, "role": "mapa", "priority": 2, "weight": 1, "map_id": 3, "qos": SC,
     "vcf_len_sc": 2, "vcf_len_exp": 2},
]


def uslp_cfg(**kw):
    c = {"protocol": "uslp", "scid": 0x1234, "frame_len": 32, "fecf": True,
         "source_or_destination": 0, "insert_zone_len": 0,
         "policy": {"kind": "fixed_priority"},
         "oid": {"qos": SC, "vcf_len": 0},
         "vcs": copy.deepcopy(USLP_VCS)}
    c.update(kw)
    return c


def tfdf(rule, upid, ptr, zone):
    return bytes([(rule << 5) | upid]) + u16(ptr) + zone


def mux_uslp():
    cfg = uslp_cfg()
    out = []
    # SC TFDF = 32 - 7 - 2 - 2 = 21 ; EXP on VC 1: 32 - 7 - 1 - 2 = 22
    s = MuxSc("link-send-mux-uslp-lnk-030-oid-frames", ["LNK-030", "LNK-036"],
              "With no VC offering, an OID frame: VCID 63, MAP ID 0, rule 001, UPID Only Idle "
              "Data, Last Valid Octet Pointer at the last TFDZ octet, the TFDZ from the OID "
              "generator, which continues; no VCF Count by default on VCID 63.", cfg)
    for k in range(3):
        s.do(opp(), 10 * k)
    out.append(s)
    c = uslp_cfg(oid={"qos": SC, "vcf_len": 2})
    s = MuxSc("link-send-mux-uslp-lnk-030-oid-with-count", ["LNK-030", "LNK-032"],
              "An OID frame with a VCF Count Length of 2 for VCID 63: the mux counts OID frames "
              "like any VC; the count has the configured length.", c)
    for k in range(3):
        s.do(opp(), 10 * k)
    out.append(s)
    s = MuxSc("link-send-mux-uslp-lnk-032-qos-counts", ["LNK-032"],
              "Sequence-controlled and expedited frames of a VC are counted separately, each with "
              "the VCF Count Length of its QoS (2 and 1 octets on VC 1, so the TFDF is 1 octet "
              "longer for expedited frames); the Bypass Flag is set for expedited.", cfg)
    s.do(offer(1, tfdf(0, 0, 0, fld(18, 0x10)), 0), 0).do(opp(), 1)
    s.do(offer(1, tfdf(0, 0, 0, fld(19, 0x20)), 0, qos=EXP), 10).do(opp(), 11)
    s.do(offer(1, tfdf(0, 0, 0, fld(18, 0x30)), 0), 20).do(opp(), 21)
    s.do(offer(1, tfdf(0, 0, 0, fld(19, 0x40)), 0, qos=EXP), 30).do(opp(), 31)
    out.append(s)
    c = uslp_cfg(vcs=[dict(USLP_VCS[0], vcf_len_sc=1, vcf_len_exp=1), USLP_VCS[1]])
    s = MuxSc("link-send-mux-uslp-lnk-032-vcf-wrap", ["LNK-032"],
              "A VCF Count Length of 1 octet: the count wraps modulo 256 after 255 (257 frames of "
              "VC 1).", c)
    for k in range(257):
        s.do(offer(1, tfdf(0, 0, 0, fld(19, k & 0xFF)), 0), 10 * k).do(opp(), 10 * k + 1)
    out.append(s)
    s = MuxSc("link-send-mux-uslp-lnk-032-map-id", ["LNK-032", "LNK-030"],
              "The MAP ID of a frame is the one the offer names (or the VC's); VC 2 carries MAP "
              "3; the VC Frame Count is per VC, not per MAP.", cfg)
    s.do(offer(2, tfdf(1, 5, 0xFFFF, fld(18, 0x10)), map=3), 0).do(opp(), 1)
    s.do(offer(2, tfdf(2, 5, 4, fld(18, 0x20)), map=4), 10).do(opp(), 11)
    out.append(s)
    c = uslp_cfg(vcs=vc_with(USLP_VCS, 1, ocf_init_hex="C1020304"),
                 oid={"qos": EXP, "vcf_len": 0, "ocf_init_hex": "01020304"})
    s = MuxSc("link-send-mux-uslp-lnk-033-ocf", ["LNK-033"],
              "VC 1 requires an OCF in every frame (the OCF Flag is set); VC 2 has none; the OID "
              "frame has one because the entry for VCID 63 says so, and it is a expedited-QoS "
              "frame.", c)
    s.do(offer(1, tfdf(0, 0, 0, fld(14, 0x10)), 0), 0).do(opp(), 1)
    s.do(offer(2, tfdf(1, 5, 3, fld(18, 0x20))), 10).do(opp(), 11)
    s.do({"set_ocf": {"scope": "vc", "vc": 1, "ocf_hex": "81828384"}}, 20)
    s.do(offer(1, tfdf(0, 0, 0, fld(14, 0x30)), 0), 21).do(opp(), 22)
    s.do({"set_ocf": {"scope": "vc", "vc": 63, "ocf_hex": "05060708"}}, 30).do(opp(), 31)
    out.append(s)
    c = uslp_cfg(insert_zone_len=2, insert_init_hex="0000")
    s = MuxSc("link-send-mux-uslp-lnk-034-insert-zone", ["LNK-034"],
              "An Insert Zone of 2 octets in every frame, the OID frame included; a frame with no "
              "new IN_SDU repeats the slot and is followed by Notify(InsertSduRepeated) (OI-68).", c)
    s.do({"set_insert": {"data_hex": "A1A2"}}, 0)
    s.do(offer(1, tfdf(0, 0, 0, fld(16, 0x10)), 0), 1).do(opp(), 10).do(opp(), 20)
    s.do({"set_insert": {"data_hex": "B1B2"}}, 25).do(opp(), 30)
    out.append(s)
    c = uslp_cfg(vcs=vc_with(USLP_VCS, 1, sec_header_len=3, sec_trailer_len=2))
    s = MuxSc("link-send-mux-uslp-lnk-035-security-ranges", ["LNK-035"],
              "A 3-octet Security Header and a 2-octet Security Trailer on VC 1: they surround the "
              "TFDF (6.3.5); the action gives their ranges and the range of the TFDF.", c)
    s.do(offer(1, tfdf(0, 0, 0, fld(13, 0x10)), 0), 0).do(opp(), 1)
    out.append(s)
    s = MuxSc("link-send-mux-uslp-lnk-031-round-robin", ["LNK-031"],
              "Round robin applies to a USLP channel as to the others: VC 1 and VC 2 alternate.",
              uslp_cfg(policy={"kind": "round_robin"}))
    for v in (1, 2):
        s.do(offer(v, tfdf(0, 0, 0, fld(18, 0x10 * v)), 0), 0)
    t = 10
    for _ in range(4):
        s.do(opp(), t)
        vc, _k = s.served()
        s.do(offer(vc, tfdf(0, 0, 0, fld(18, t & 0xFF)), 0), t + 1)
        t += 10
    out.append(s)
    return cfg, out


# --------------------------------------------------------------------------------------------
# MapSegmenter scenarios (LNK-040, LNK-041, LNK-042)
# --------------------------------------------------------------------------------------------


def sub_p(m, p):
    d = {"packet_hex": hx(p)}
    if m is not None:
        d["map"] = m
    return {"submit_packet": d}


def sub_s(m, s):
    d = {"sdu_hex": hx(s)}
    if m is not None:
        d["map"] = m
    return {"submit_sdu": d}


def nxt(h, qos=SC):
    return {"next_fdu": {"handle": h, "qos": qos}}


TC_MAPS = [
    {"map_id": 1, "segmentation": True, "blocking": False, "max_sdu_len": 60},
    {"map_id": 2, "segmentation": False, "blocking": True},
    {"map_id": 3, "segmentation": True, "blocking": True, "max_sdu_len": 100},
]


def seg_tc():
    # the FDU holds 16 octets, the Segment Header and 15 of user data (232.0 4.1.3.1.2)
    cfg = {"protocol": "tc", "segment_header": True, "fecf": True, "max_fdu_len": 16,
           "maps": copy.deepcopy(TC_MAPS)}
    out = []
    s = Sc("link-send-seg-tc-lnk-040-whole-sdu", ["LNK-040"],
           "An SDU that fits one FDU is sent as one FDU with sequence flags 11 and the Segment "
           "Header (flags, MAP ID); with nothing queued NextFdu gives nothing; the QoS is the "
           "request's.")
    s.do(nxt(1), 0).do(sub_s(1, fld(10, 0x10)), 1).do(sub_s(1, fld(15, 0x20)), 2)
    s.do(nxt(2), 3).do(nxt(3, EXP), 4).do(nxt(4), 5)
    out.append(s)
    s = Sc("link-send-seg-tc-lnk-040-segmented-sdu", ["LNK-040"],
           "A longer SDU on a MAP that permits segmentation: FDUs of the maximum length, flags 01 "
           "first, 00 continuing, 10 last; an SDU of exactly two FDUs has no 00 and a last FDU "
           "that is full.")
    s.do(sub_s(1, fld(40, 0x10)), 0).do(sub_s(1, fld(30, 0x50)), 1)
    for k in range(6):
        s.do(nxt(k + 1), 10 + k)
    out.append(s)
    s = Sc("link-send-seg-tc-lnk-040-segmented-packet", ["LNK-040", "LNK-041"],
           "A packet longer than the FDU is segmented like an SDU; the segments of a MAP are "
           "contiguous in the output, and a packet submitted meanwhile on another MAP waits.")
    p = spp_packet(5, 33, 0, 0x10)
    s.do(sub_p(1, p), 0).do(sub_p(3, spp_packet(6, 8, 0, 0x60)), 1)
    for k in range(4):
        s.do(nxt(k + 1), 10 + k)
    out.append(s)
    s = Sc("link-send-seg-tc-lnk-040-blocking", ["LNK-040"],
           "Where blocking is permitted several complete packets share one FDU (flags 11, one "
           "Segment Header) as long as they fit: here 4 + 5 octets, and the 7-octet packet that "
           "would make 16 octets of user data starts the next FDU; a MAP that does not permit "
           "blocking sends one packet per FDU.")
    s.do(sub_p(2, epp_packet(4, 7, 0x10)), 0).do(sub_p(2, epp_packet(5, 7, 0x20)), 1)
    s.do(sub_p(2, spp_packet(1, 7, 0, 0x30)), 2)
    s.do(sub_p(1, epp_packet(4, 7, 0x40)), 3).do(sub_p(1, epp_packet(5, 7, 0x50)), 4)
    for k in range(5):
        s.do(nxt(k + 1), 10 + k)
    out.append(s)
    s = Sc("link-send-seg-tc-lnk-040-blocking-not-with-segments", ["LNK-040", "LNK-041"],
           "A packet that needs segmentation is not blocked with the packets before it: the "
           "6-octet packet goes alone, then the 20-octet packet is segmented.")
    s.do(sub_p(3, epp_packet(6, 7, 0x10)), 0).do(sub_p(3, spp_packet(2, 20, 0, 0x20)), 1)
    for k in range(4):
        s.do(nxt(k + 1), 10 + k)
    out.append(s)
    s = Sc("link-send-seg-tc-lnk-040-refused", ["LNK-040"],
           "An SDU longer than the maximum MAP_SDU length of its MAP is refused with OutOfRange "
           "(max_sdu_len); an SDU longer than one FDU on a MAP that does not permit segmentation "
           "is refused with OutOfRange (max_fdu_len); an unknown MAP is refused; nothing changes.")
    s.do(sub_s(1, fld(61)), 0).do(sub_s(2, fld(16)), 1).do(sub_s(9, fld(4)), 2)
    s.do(sub_s(1, fld(60, 0x10)), 3).do(nxt(1), 4)
    out.append(s)
    s = Sc("link-send-seg-tc-lnk-041-no-interleave", ["LNK-041", "LNK-040"],
           "The portions of two SDUs of the same MAP are never interleaved, and an SDU is "
           "completed before an SDU of another MAP is started, although the other SDU was "
           "submitted while the first was in progress.")
    s.do(sub_s(1, fld(35, 0x10)), 0).do(sub_s(3, fld(5, 0x80)), 1).do(nxt(1), 10)
    s.do(sub_s(1, fld(5, 0x90)), 11).do(sub_s(3, fld(3, 0xA0)), 12)
    for k in range(4):
        s.do(nxt(k + 2), 20 + k)
    out.append(s)
    s = Sc("link-send-seg-tc-lnk-003-reconfigure", ["LNK-003", "LNK-041"],
           "Reconfigure discards the SDU in progress and the queue, with the octets left; the "
           "next SDU starts afresh with flags 01 or 11.")
    s.do(sub_s(1, fld(40, 0x10)), 0).do(sub_s(3, fld(5, 0x80)), 1).do(nxt(1), 10)
    s.do({"reconfigure": {"config": cfg}}, 20).do(sub_s(1, fld(5, 0x30)), 30).do(nxt(2), 40)
    out.append(s)
    # no Segment Header
    c2 = {"protocol": "tc", "segment_header": False, "fecf": True, "max_fdu_len": 16,
          "blocking": True}
    s = Sc("link-send-seg-tc-lnk-040-no-segment-header", ["LNK-040"],
           "Without a Segment Header a packet, packets, or a VCA_SDU are one FDU of user data "
           "only (no flags, no MAP); a longer SDU is refused with OutOfRange (max_fdu_len).",
           cfg=c2)
    s.do(sub_p(None, epp_packet(4, 7, 0x10)), 0).do(sub_p(None, epp_packet(5, 7, 0x20)), 1)
    s.do(sub_p(None, spp_packet(1, 12, 0, 0x30)), 2).do(sub_s(None, fld(16, 0x40)), 3)
    s.do(sub_s(None, fld(17)), 4)
    for k in range(4):
        s.do(nxt(k + 1), 10 + k)
    out.append(s)
    return cfg, out


USLP_MAPS = [
    {"map_id": 1, "service": "packets", "upid": 0, "blocking": False, "max_sdu_len": 40},
    {"map_id": 2, "service": "mapa", "upid": 5, "blocking": False, "max_sdu_len": 40},
    {"map_id": 3, "service": "packets", "upid": 0, "blocking": True, "max_sdu_len": 30},
    {"map_id": 4, "service": "octet_stream", "upid": 4, "release_deadline_us": 500},
]


def seg_uslp():
    # the TFDF holds 12 octets: the 1-octet header and 11 of TFDZ (732.1 4.1.4.2.1.2)
    cfg = {"protocol": "uslp", "max_tfdf_len": 12, "maps": copy.deepcopy(USLP_MAPS)}
    out = []
    s = Sc("link-send-seg-uslp-lnk-042-rule-111", ["LNK-042", "LNK-040"],
           "An SDU or packet that fits the TFDZ is not segmented: rule 111 with the UPID of its "
           "MAP; the request's QoS and handle are in the action.")
    s.do(sub_p(1, spp_packet(1, 8, 0, 0x10)), 0).do(sub_s(2, fld(11, 0x20)), 1)
    s.do(nxt(1), 10).do(nxt(2, EXP), 11).do(nxt(3), 12)
    out.append(s)
    s = Sc("link-send-seg-uslp-lnk-042-segments", ["LNK-042"],
           "An SDU longer than the TFDZ: rule 100 (start), 101 (continuing) and 110 (last); an SDU "
           "of exactly two TFDZs has no 101 and a full last segment; one octet more than two "
           "TFDZs has a last segment of 1 octet.")
    s.do(sub_s(2, fld(25, 0x10)), 0).do(sub_s(2, fld(22, 0x50)), 1).do(sub_s(2, fld(23, 0x80)), 2)
    for k in range(8):
        s.do(nxt(k + 1), 10 + k)
    out.append(s)
    s = Sc("link-send-seg-uslp-lnk-042-packet-segments", ["LNK-042"],
           "A packet longer than the TFDZ is segmented with its MAP's UPID (packets, UPID 0).")
    s.do(sub_p(1, spp_packet(3, 30, 0, 0x10)), 0)
    for k in range(4):
        s.do(nxt(k + 1), 10 + k)
    out.append(s)
    s = Sc("link-send-seg-uslp-lnk-042-blocking", ["LNK-042", "LNK-040"],
           "Where blocking is permitted several complete packets share one TFDZ (rule 111, one "
           "TFDF Header): 4 + 5 octets; the next packet of 7 octets would exceed the TFDZ and "
           "starts the next TFDF.")
    s.do(sub_p(3, epp_packet(4, 7, 0x10)), 0).do(sub_p(3, epp_packet(5, 7, 0x20)), 1)
    s.do(sub_p(3, spp_packet(1, 7, 0, 0x30)), 2)
    s.do(nxt(1), 10).do(nxt(2), 11).do(nxt(3), 12)
    out.append(s)
    s = Sc("link-send-seg-uslp-lnk-041-complete-before-another-map", ["LNK-041", "LNK-042"],
           "An SDU of one MAP is completed (start, continuing, last, contiguous) before an SDU of "
           "another MAP on the VC is started, although the other SDU was submitted while the "
           "first was in progress.")
    s.do(sub_s(2, fld(25, 0x10)), 0).do(nxt(1), 5).do(sub_p(1, spp_packet(2, 8, 0, 0x60)), 6)
    s.do(sub_s(2, fld(3, 0x70)), 7)
    for k in range(4):
        s.do(nxt(k + 2), 10 + k)
    out.append(s)
    s = Sc("link-send-seg-uslp-lnk-042-refused", ["LNK-042"],
           "An SDU longer than the MAP's maximum is refused with OutOfRange (max_sdu_len); an "
           "unknown MAP is refused; nothing changes.")
    s.do(sub_s(2, fld(41)), 0).do(sub_s(9, fld(3)), 1).do(sub_s(2, fld(40, 0x10)), 2).do(nxt(1), 3)
    out.append(s)
    s = Sc("link-send-seg-uslp-lnk-042-octet-stream-deadline", ["LNK-042", "LNK-012"],
           "Octet stream (rule 011): the TFDZ is filled in order; a TFDZ that is not full is "
           "released by the deadline of its MAP: the first octet started it, Timeout marks it, "
           "NextFdu returns it with the octets it holds (variable length); a full TFDZ needs no "
           "deadline and the octets that do not fit start the next one.")
    s.do({"submit_octets": {"map": 4, "data_hex": hx(fld(5, 0x10))}}, 100).do(nxt(1), 200)
    s.do(timeout(), 599).do(nxt(2), 599).do(timeout(), 600).do(nxt(3), 600).do(nxt(4), 601)
    s.do({"submit_octets": {"map": 4, "data_hex": hx(fld(25, 0x20))}}, 1000).do(nxt(5), 1010)
    s.do(nxt(6), 1011).do(timeout(), 1511).do(nxt(7), 1511).do(nxt(8), 1512)
    out.append(s)
    s = Sc("link-send-seg-uslp-lnk-003-reconfigure", ["LNK-003", "LNK-042"],
           "Reconfigure discards the SDU in progress, the queue, and the octets of a started "
           "TFDZ, with the number of octets.")
    s.do(sub_s(2, fld(25, 0x10)), 0).do(nxt(1), 5)
    s.do({"submit_octets": {"map": 4, "data_hex": hx(fld(3, 0x40))}}, 6)
    s.do({"reconfigure": {"config": cfg}}, 10).do(timeout(), 700).do(nxt(2), 701)
    out.append(s)
    return cfg, out


# --------------------------------------------------------------------------------------------
# Files
# --------------------------------------------------------------------------------------------

REQUIREMENTS_SCOPE = (["LNK-011", "LNK-012", "LNK-013", "LNK-014", "LNK-020", "LNK-021",
                       "LNK-030", "LNK-031", "LNK-032", "LNK-033", "LNK-034", "LNK-035",
                       "LNK-036", "LNK-040", "LNK-041", "LNK-042", "LNK-015", "LNK-076"])

PROV = {
    "pg_tm": "Hand-derived from 132.0-B-3 §4.1.2.7.6, §4.1.4, §4.2.2 (FHP 0x7FF none, 0x7FE idle "
             "data), 133.0-B-2 §4.1.3 and 133.1-B-3 §4.1.2 (the packets and the idle packets), "
             "docs/04-engines.md §2.1 (LNK-010…LNK-015) and OI-11, OI-69.",
    "pg_aos": "Hand-derived from 732.0-B-5 EC1 §4.1.4.2, §4.2.2 (M_PDU, FHP all ones / all ones "
              "minus one) and 732.0-B-4 §4.1.4.2 (Issue 4: 5 spare bits and an 11-bit FHP), "
              "133.0-B-2 §4.1.3, 133.1-B-3 §4.1.2, docs/04-engines.md §2.1 (LNK-010…LNK-015).",
    "pg_uslp": "Hand-derived from 732.1-B-3 §4.1.4.2 (TFDF Header, rule 000, FHP all ones), "
               "§4.2.2.1, §4.2.6.1 (fill with one Encapsulation Idle Packet), 133.0-B-2, "
               "133.1-B-3, docs/04-engines.md §2.1 and OI-34, OI-69.",
    "ag": "Hand-derived from 132.0-B-3 §4.1.4, §4.2.3.1, 732.0-B-5 EC1 §4.1.4.1, §4.2.4.1 (a "
          "VCA_SDU is the data field) and docs/04-engines.md §2.2 (LNK-020).",
    "ag_uslp": "Hand-derived from 732.1-B-3 §4.1.4.2.2.2.2-3, §4.1.4.2.4.5-6, §4.2.3.1 (rules "
               "001 and 010, the Last Valid Octet Pointer) and docs/04-engines.md §2.2 (LNK-020).",
    "bg": "Hand-derived from 732.0-B-5 EC1 §4.1.4.3, §4.2.3 (B_PDU, the Bitstream Data Pointer) "
          "and docs/04-engines.md §2.2 (LNK-021).",
    "mux_tm": "Hand-derived from 132.0-B-3 §4.1.2 (the Primary Header), §4.1.3 (the Secondary "
              "Header), §4.1.4.6 and Annex D (OID frames), §4.1.5, §4.2.4-4.2.5 and "
              "docs/04-engines.md §2.3 (LNK-030…LNK-036, LNK-076), docs/standards/"
              "common-link-features.md §3, OI-11 (D2 of the M4 plan).",
    "mux_aos": "Hand-derived from 732.0-B-5 EC1 §4.1.2 (the Primary Header, the split SCID), "
               "§4.1.3 (the Insert Zone), §4.1.4.1.5, §4.1.5, §4.2.5-4.2.7 and "
               "docs/04-engines.md §2.3 (LNK-030…LNK-036, LNK-076), OI-68.",
    "mux_uslp": "Hand-derived from 732.1-B-3 §4.1.2 (the Primary Header), §4.1.3, §4.1.4.1.5-12 "
                "(OID frames), §4.1.5, §6.3, §4.2.8-4.2.11 and docs/04-engines.md §2.3 "
                "(LNK-030…LNK-036), OI-68; UPIDs from specs/sana/uslp_protocol_id.tsv.",
    "seg_tc": "Hand-derived from 232.0-B-4 Cor. 1 §4.1.3 (the FDU, the Segment Header, Table "
              "4-2), §4.3.1-4.3.3 and docs/04-engines.md §2.4 (LNK-040, LNK-041), OI-71.",
    "seg_uslp": "Hand-derived from 732.1-B-3 §4.1.4.2.2.2.4-8, Table 4-3, §4.2.2.2, §4.2.3.2, "
                "§4.2.4, §4.2.6.2 and docs/04-engines.md §2.4 (LNK-041, LNK-042); UPIDs from "
                "specs/sana/uslp_protocol_id.tsv.",
}


def file_specs():
    """(path under test-vectors/link/send/, engine, provenance key, (config, scenarios))."""
    return [
        ("packet_generator_tm.json", "packet-generator", "pg_tm", build_pg("tm", 24)),
        ("packet_generator_aos.json", "packet-generator", "pg_aos", build_pg("aos", 26)),
        ("packet_generator_aos_issue4.json", "packet-generator", "pg_aos", aos_issue4()),
        ("packet_generator_uslp.json", "packet-generator", "pg_uslp", build_pg("uslp", 27)),
        ("access_generator_tm.json", "access-generator", "ag", access_tm_aos("tm")),
        ("access_generator_aos.json", "access-generator", "ag", access_tm_aos("aos")),
        ("access_generator_uslp.json", "access-generator", "ag_uslp", access_uslp()),
        ("bitstream_generator_aos.json", "bitstream-generator", "bg", bitstream_aos()),
        ("channel_mux_tm.json", "channel-mux", "mux_tm", mux_tm()),
        ("channel_mux_aos.json", "channel-mux", "mux_aos", mux_aos()),
        ("channel_mux_uslp.json", "channel-mux", "mux_uslp", mux_uslp()),
        ("map_segmenter_tc.json", "map-segmenter", "seg_tc", seg_tc()),
        ("map_segmenter_uslp.json", "map-segmenter", "seg_uslp", seg_uslp()),
    ]


def aos_issue4():
    cfg = pg_cfg("aos", field_len=26, release_deadline_us=1000, aos_issue=4)
    out = []
    s = Sc("link-send-pg-aos4-lnk-011-fhp-11-bit", ["LNK-011", "LNK-014"],
           "Issue 4 edition: the M_PDU header is 5 spare bits and an 11-bit FHP: 0x7FF when no "
           "packet starts, offsets as in Issue 5.", cfg=cfg)
    s.do(submit_packet(spp_packet(1, 60, 0, 0x10)), 0).do(release(), 10).do(release(), 20)
    s.do(release(), 30)
    out.append(s)
    s = Sc("link-send-pg-aos4-lnk-014-idle-field", ["LNK-014"],
           "Issue 4 edition: an idle M_PDU has FHP 0x7FE and the idle pattern as zone.",
           cfg=dict(cfg, idle_policy="idle_field", idle_pattern_hex="55"))
    s.do(release(), 0)
    out.append(s)
    return cfg, out


def make_doc(path, engine, provkey, built):
    fcfg, scs = built
    doc = {
        "kind": "engine-scenario",
        "engine": engine,
        "provenance": "hand-derived",
        "provenance_note": PROV[provkey] + NOTE_BLIND,
        "config": fcfg,
        "scenarios": [],
    }
    for sc in scs:
        cfg = sc.cfg if sc.cfg is not None else fcfg
        steps = replay(engine, cfg, sc.inputs)
        d = {"id": sc.id, "description": sc.desc, "requirements": sc.reqs}
        if cfg != fcfg:
            d["config"] = cfg
        d["steps"] = steps
        doc["scenarios"].append(d)
    return doc


def all_docs():
    return {path: make_doc(path, e, k, b) for path, e, k, b in file_specs()}


# --------------------------------------------------------------------------------------------
# Output format, --write, --check
# --------------------------------------------------------------------------------------------


def dump(obj, level=0):
    """A file, a scenario, or a list of scenarios is expanded one key per line; a step and every
    other small object is one line."""
    pad = "  " * level
    inner = "  " * (level + 1)
    if isinstance(obj, dict) and ("kind" in obj or "id" in obj):
        items = ['%s%s: %s' % (inner, json.dumps(k), dump(v, level + 1)) for k, v in obj.items()]
        return "{\n" + ",\n".join(items) + "\n" + pad + "}"
    if isinstance(obj, list) and obj and all(isinstance(x, dict) for x in obj) and \
            ("at_us" in obj[0] or "id" in obj[0]):
        return "[\n" + ",\n".join(inner + dump(x, level + 1) for x in obj) + "\n" + pad + "]"
    return json.dumps(obj, separators=(", ", ": "))


def render(doc):
    return dump(doc) + "\n"


def root_dir():
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.normpath(os.path.join(here, "..", ".."))


def out_dir():
    return os.path.join(root_dir(), "test-vectors", "link", "send")


def write_all(outdir=None):
    base = outdir or out_dir()
    os.makedirs(base, exist_ok=True)
    docs = all_docs()
    for name in sorted(os.listdir(base)):
        if name.endswith(".json") and name not in docs:
            os.remove(os.path.join(base, name))
    for name, doc in docs.items():
        with open(os.path.join(base, name), "w") as f:
            f.write(render(doc))
    n = sum(len(d["scenarios"]) for d in docs.values())
    print("wrote %d file(s), %d scenario(s)" % (len(docs), n))
    return 0


def coverage_problems(docs):
    seen = set()
    for d in docs.values():
        for sc in d["scenarios"]:
            seen.update(sc["requirements"])
    return ["requirement %s has no scenario" % r for r in REQUIREMENTS_SCOPE if r not in seen]


def check_file(path, generated):
    problems = []
    with open(path) as f:
        doc = json.load(f)
    engine = doc["engine"]
    ids = set()
    for sc in doc["scenarios"]:
        if sc["id"] in ids:
            problems.append("%s: scenario id twice" % sc["id"])
        ids.add(sc["id"])
        if sc.get("superseded"):
            continue
        cfg = sc.get("config", doc["config"])
        inputs = [(st["at_us"], st["input"]) for st in sc["steps"]]
        steps = replay(engine, cfg, inputs)
        for i, (want, got) in enumerate(zip(steps, sc["steps"])):
            if want != got:
                problems.append("%s: step %d: model says %s, file says %s" %
                                (sc["id"], i, json.dumps(want), json.dumps(got)))
    name = os.path.basename(path)
    if name not in generated:
        problems.append("%s: not a file the generator writes" % name)
    elif generated[name] != doc:
        problems.append("%s: differs from what the generator writes (run --write)" % name)
    return problems


def check(paths):
    selftest()
    generated = all_docs()
    problems = []
    for p in paths:
        problems.extend(check_file(p, generated))
    seen = {os.path.basename(p) for p in paths}
    if not paths or seen >= set(generated):
        problems.extend(coverage_problems(generated))
    for name in generated:
        if paths and seen >= set(generated) and name not in seen:
            problems.append("%s: not checked" % name)
    for p in problems:
        print("error:", p)
    print("link_send model: %d file(s), %d problem(s)" % (len(paths), len(problems)))
    return 1 if problems else 0


def coverage():
    docs = all_docs()
    count = {}
    for d in docs.values():
        for sc in d["scenarios"]:
            for r in sc["requirements"]:
                count[r] = count.get(r, 0) + 1
    for r in sorted(count):
        print("%-8s %d scenario(s)" % (r, count[r]))
    probs = coverage_problems(docs)
    for p in probs:
        print("error:", p)
    return 1 if probs else 0


def main(argv):
    if len(argv) >= 2 and argv[1] == "--check":
        return check(argv[2:])
    if len(argv) in (2, 3) and argv[1] == "--write":
        selftest()
        return write_all(argv[2] if len(argv) == 3 else None)
    if len(argv) == 2 and argv[1] == "--coverage":
        return coverage()
    print("usage: link_send.py --check FILE... | --write [DIR] | --coverage")
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
