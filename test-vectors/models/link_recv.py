"""Independent model of the receiving-side link engines for the vector files under
test-vectors/link/recv/ (LNK-050 to LNK-054, LNK-060 to LNK-064, LNK-070 to LNK-072, LNK-077,
LNK-079, LNK-003, TV-010, TV-012, TV-023; M4 WP-2b).

Sources, all read as text, none of it copied: the receiving-end procedures of CCSDS 132.0-B-3
(4.3), 732.0-B-5 (4.3), 232.0-B-4 (4.4), and 732.1-B-3 (4.3); the frame layouts of the same
documents (132.0 4.1, 732.0 4.1, 232.0 4.1, 732.1 4.1 and Annex B for the FECF); the packet
layouts of 133.0-B-2 (4.1.3) and 133.1-B-3 (4.1.2, Table 4-1); the SANA UPID value of "Only Idle
Data" (specs/sana/uslp_protocol_id.tsv, 0b11111); and, for the contract of an engine (a pure step
returning an ordered list of actions), the requirements LNK-001 to LNK-079 of docs/04-engines.md,
API-050 to API-059 of docs/03-api-model.md, CMN-001, CMN-020, CMN-030, CMN-031 of
docs/standards/common-link-features.md, and the decisions OI-68 to OI-72 of docs/09-open-issues.md.

This program was written blind (TV-023): by an agent that had not read any implementation under
test (crates/, exerciser/, fuzz/, crosscheck/, refapp/, include/, the Rust source of xtask/). It
read, for format only, test-vectors/README.md, test-vectors/schema/, test-vectors/models/README.md,
test-vectors/models/cop1.py, and test-vectors/cop1/. It must stay independent of the Rust code:
never port code from it, and never change the model to agree with it. When a model and an
implementation disagree, the Blue Book decides.

What it does
------------
* Three engines as pure steps over plain Python state: `FrameReceiver` (04 3.1), `PacketExtractor`
  (04 3.2), and `MapReassembler` (04 3.3). A step takes one input and returns the complete ordered
  list of outputs. Where the specification is silent the model raises `Unspecified`, so that no
  vector can pin a behavior the documents do not settle; the cases it refuses are listed in the
  README of test-vectors/link/recv/.
* Frame builders for TM, AOS (Issue 5), USLP (fixed length), and TC frames, written from the
  figures of the Blue Books, so that a frame input is the exact octets the codec would see. The
  engines parse the octets again with their own code.
* The scenarios of each file, as input scripts chosen by hand; the expected outputs are the
  model's. The file is written by the model (--write) and never edited by hand.

Vocabulary of inputs and outputs: test-vectors/link/recv/README.md.

Usage, from the repository root:

    python3 -I test-vectors/models/link_recv.py --check test-vectors/link/recv/*.json
    python3 -I test-vectors/models/link_recv.py --write [dir]    # regenerate every file

--check replays every live scenario through the model and compares inputs and expected outputs,
compares the file's metadata with the generator's, and exits 1 on any difference.
"""
import json
import os
import sys


class Unspecified(Exception):
    """The documents do not say what the engine does; no vector may contain this case."""


# --------------------------------------------------------------------------------------------
# Common helpers
# --------------------------------------------------------------------------------------------

def crc16(data):
    """FECF of 132.0 4.1.6, 232.0 4.1.4, 732.0 4.1.6, 732.1 Annex B (CMN-001): generator
    x^16 + x^12 + x^5 + 1, register preset to all ones, MSB first, no reflection."""
    reg = 0xFFFF
    for byte in data:
        reg ^= byte << 8
        for _ in range(8):
            reg = ((reg << 1) ^ 0x1021) & 0xFFFF if reg & 0x8000 else (reg << 1) & 0xFFFF
    return reg


def oid_octets(count, skip=0):
    """The OID sequence of 132.0 4.1.4.6.2 / Annex D, Fibonacci form with an all-ones seed
    (CMN-020): x[n] = x[n-1] ^ x[n-2] ^ x[n-22] ^ x[n-32]; MSB first. `skip` octets are dropped."""
    nbits = (count + skip) * 8
    x = [1] * 32
    while len(x) < nbits:
        n = len(x)
        x.append(x[n - 1] ^ x[n - 2] ^ x[n - 22] ^ x[n - 32])
    out = bytearray()
    for i in range(0, nbits, 8):
        v = 0
        for b in x[i:i + 8]:
            v = (v << 1) | b
        out.append(v)
    return bytes(out[skip:])


def hx(b):
    return bytes(b).hex().upper()


def unhx(s):
    return bytes.fromhex(s)


class Oid:
    """The OID generator of a physical channel: one sequence that continues across frames and is
    never restarted (132.0 4.1.4.6.2.1, CMN-021)."""

    def __init__(self):
        self.used = 0

    def take(self, n):
        out = oid_octets(n, self.used)
        self.used += n
        return out


def ramp(seed, n):
    """Deterministic filler: seed, seed+1, ... modulo 256."""
    return bytes((seed + i) & 0xFF for i in range(n))


def data_bytes(data, n, fill=0x55):
    """The data of a frame: an Oid takes the next octets of the sequence, an int is the seed of a
    ramp, octets are padded to fill."""
    if isinstance(data, Oid):
        return data.take(n)
    if isinstance(data, int):
        return ramp(data, n)
    return fit(data, n, fill)


def fit(payload, n, fill=0x55):
    """`payload` padded with `fill` to exactly n octets."""
    assert len(payload) <= n, (len(payload), n)
    return bytes(payload) + bytes([fill]) * (n - len(payload))


# --------------------------------------------------------------------------------------------
# Packets: 133.0 4.1.3 (Space Packet), 133.1 4.1.2 (Encapsulation Packet)
# --------------------------------------------------------------------------------------------

def spp(apid, data, seq=0, pkt_type=0):
    """Space Packet: PVN 000, type, secondary header flag 0, APID, sequence flags 11, count,
    Packet Data Length = data octets - 1 (133.0 4.1.3.5.2)."""
    assert len(data) >= 1
    w0 = (0 << 13) | (pkt_type << 12) | (0 << 11) | apid
    w1 = (3 << 14) | (seq & 0x3FFF)
    return bytes([w0 >> 8, w0 & 0xFF, w1 >> 8, w1 & 0xFF]) + \
        (len(data) - 1).to_bytes(2, "big") + bytes(data)


def spp_total(n, apid=1, seed=0, seq=0):
    """Space Packet of n octets in all (n >= 7), data ramp."""
    return spp(apid, ramp(seed, n - 6), seq)


def spp_idle(n):
    """Space Packet Idle Packet (APID 2047, 133.0 4.1.3.3.4.4) of n octets in all."""
    return spp(2047, bytes([0x00]) * (n - 6))


def epp(epi, payload, lol=1, ud=0):
    """Encapsulation Packet (133.1 4.1.2): PVN 111, EPI, Length of Length, then the length
    field(s); the Packet Length is the total length (4.1.2.8.2). lol 0 is the one-octet idle
    packet; lol 1 has a one-octet length, lol 2 a two-octet length after the octet that holds the
    User Defined and extension fields."""
    first = (7 << 5) | (epi << 2) | lol
    if lol == 0:
        assert epi == 0 and not payload
        return bytes([first])
    if lol == 1:
        total = 2 + len(payload)
        assert total <= 255
        return bytes([first, total]) + bytes(payload)
    if lol == 2:
        total = 4 + len(payload)
        return bytes([first, ud & 0xF0]) + total.to_bytes(2, "big") + bytes(payload)
    raise AssertionError(lol)


def epp_idle(n):
    """Encapsulation Idle Packet (EPI 000) of n octets in all (n >= 1), 133.1 4.1.2.4.4."""
    if n == 1:
        return epp(0, b"", 0)
    return epp(0, bytes(n - 2), 1)


# --------------------------------------------------------------------------------------------
# Frame builders (the octets a codec would see), from the figures of the Blue Books
# --------------------------------------------------------------------------------------------

def with_fecf(body):
    return bytes(body) + crc16(body).to_bytes(2, "big")


def tm_frame(cfg, vcid, mc, vc, fhp, data, sync=0, fsh=None, ocf=None):
    """TM Transfer Frame, 132.0 4.1.2: TFVN 00 (2 bits), SCID (10), VCID (3), OCF flag (1), MC
    count (8), VC count (8), then the Data Field Status: secondary header flag, sync flag, packet
    order flag 0, segment length ID 11, FHP (11). `fsh` is the whole Secondary Header (ID octet
    included, 4.1.3.1.3); the data field is `data` padded to fill the frame."""
    w0 = (0 << 14) | (cfg["scid"] << 4) | (vcid << 1) | (1 if ocf else 0)
    w2 = ((1 if fsh else 0) << 15) | (sync << 14) | (0 << 13) | (3 << 11) | fhp
    head = w0.to_bytes(2, "big") + bytes([mc, vc]) + w2.to_bytes(2, "big")
    n = cfg["frame_len"] - 6 - len(fsh or b"") - (4 if ocf else 0) - (2 if cfg["fecf"] else 0)
    body = head + bytes(fsh or b"") + data_bytes(data, n) + bytes(ocf or b"")
    return with_fecf(body) if cfg["fecf"] else body


def tm_fsh(*data):
    """Secondary Header: ID octet (version 00, length - 1 in 6 bits, 4.1.3.2) then the data."""
    return bytes([len(data)]) + bytes(data)


def aos_frame(cfg, vcid, count, data, replay=0, cycle=None, iz=None, ocf=None, scid=None):
    """AOS Transfer Frame, Issue 5, 732.0 4.1.2, without a Frame Header Error Control: TFVN 01
    (bits 0-1), SCID LSB (bits 2-9), VCID (10-15), VC Frame Count (16-39), Replay Flag (40), VC
    Frame Count Cycle Use Flag (41), SCID MSB (42-43), Cycle (44-47). The data field is `data`
    padded to fill the frame; the Insert Zone follows the header (4.1.3.1), the OCF the data."""
    scid = cfg["scid"] if scid is None else scid
    w0 = (1 << 14) | ((scid & 0xFF) << 6) | vcid
    sig = (replay << 7) | ((1 if cycle is not None else 0) << 6) | (((scid >> 8) & 3) << 4) | \
        ((cycle or 0) & 0xF)
    head = w0.to_bytes(2, "big") + count.to_bytes(3, "big") + bytes([sig])
    izl = cfg["insert_zone_len"]
    assert (iz is None) == (izl == 0) or (iz is not None and len(iz) == izl)
    n = cfg["frame_len"] - 6 - izl - (4 if ocf else 0) - (2 if cfg["fecf"] else 0)
    body = head + bytes(iz or b"") + data_bytes(data, n) + bytes(ocf or b"")
    return with_fecf(body) if cfg["fecf"] else body


def mpdu(fhp, zone_len, packets=b"", fill=0x00):
    """M_PDU (732.0 4.1.4.2): 16-bit First Header Pointer, then the Packet Zone."""
    return fhp.to_bytes(2, "big") + fit(packets, zone_len, fill)


def bpdu(bdp, zone):
    """B_PDU (732.0 4.1.4.3): reserved spare 00 (2 bits), Bitstream Data Pointer (14), zone."""
    return (bdp & 0x3FFF).to_bytes(2, "big") + bytes(zone)


def uslp_frame(cfg, vcid, mapid, tfdf, vcf=None, bypass=0, pcc=0, iz=None, ocf=None, scid=None):
    """USLP Transfer Frame, fixed length, 732.1 4.1.2: TFVN 1100 (4), SCID (16), Source or
    Destination 0 (1), VCID (6), MAP ID (4), End of Frame Primary Header Flag 0 (1), Frame Length
    = octets - 1 (16), Bypass/Sequence Control (1), Protocol Control Command (1), spares 00 (2),
    OCF flag (1), VCF Count Length (3), VCF Count (0 to 7 octets). `vcf` is (length, count).
    The TFDF is `tfdf` padded to fill the frame; Insert Zone before it, OCF after it."""
    scid = cfg["scid"] if scid is None else scid
    vlen, vcount = vcf if vcf else (0, 0)
    top = (0b1100 << 28) | (scid << 12) | (0 << 11) | (vcid << 5) | (mapid << 1) | 0
    flags = (bypass << 7) | (pcc << 6) | (0 << 4) | ((1 if ocf else 0) << 3) | vlen
    izl = cfg["insert_zone_len"]
    assert (iz is None and izl == 0) or len(iz) == izl
    head_len = 7 + vlen
    n = cfg["frame_len"] - head_len - izl - (4 if ocf else 0) - (2 if cfg["fecf"] else 0)
    head = top.to_bytes(4, "big") + (cfg["frame_len"] - 1).to_bytes(2, "big") + bytes([flags]) + \
        (vcount.to_bytes(vlen, "big") if vlen else b"")
    body = head + bytes(iz or b"") + data_bytes(tfdf, n) + bytes(ocf or b"")
    return with_fecf(body) if cfg["fecf"] else body


def tfdf(rule, upid, pointer, zone_len, payload=b"", fill=0x00):
    """TFDF with a pointer (rules 000, 001, 010): rule (3 bits), UPID (5), 16-bit First Header or
    Last Valid Octet Pointer, zone (732.1 4.1.4.2)."""
    return bytes([(rule << 5) | upid]) + pointer.to_bytes(2, "big") + fit(payload, zone_len, fill)


def tfdf_var(rule, upid, payload):
    """TFDF without a pointer (rules 011 to 111, 732.1 4.1.4.2.4.1): one header octet, the zone."""
    return bytes([(rule << 5) | upid]) + bytes(payload)


def tc_frame(cfg, vcid, ns, fdu, bypass=0, control=0):
    """TC Transfer Frame, 232.0 4.1.2: TFVN 00 (2), Bypass (1), Control Command (1), spares 00 (2),
    SCID (10), VCID (6), Frame Length = octets - 1 (10), Frame Sequence Number (8), then the data
    field and the FECF."""
    total = 5 + len(fdu) + (2 if cfg["fecf"] else 0)
    w0 = (0 << 14) | (bypass << 13) | (control << 12) | (0 << 10) | cfg["scid"]
    w1 = (vcid << 10) | (total - 1)
    body = w0.to_bytes(2, "big") + w1.to_bytes(2, "big") + bytes([ns]) + bytes(fdu)
    return with_fecf(body) if cfg["fecf"] else body


UPID_PACKETS = 0b00000       # specs/sana/uslp_protocol_id.tsv
UPID_OCTET_STREAM = 0b00100
UPID_MISSION_1 = 0b00101
UPID_IDLE = 0b11111          # "Only Idle Data"

OCF_CLCW = bytes([0x01, 0x04, 0x08, 0x05])     # Type Flag 0: a Type-1 Report (132.0 4.1.5.4)
OCF_T2_PROJECT = bytes([0x80, 0x11, 0x22, 0x33])  # Type Flag 1, bit 1 = 0 (4.1.5.5)
OCF_T2_FSR = bytes([0xC0, 0x44, 0x55, 0x66])      # Type Flag 1, bit 1 = 1: SDLS FSR


# --------------------------------------------------------------------------------------------
# Counters: modular frame counts (CMN-030, CMN-031, LNK-051)
# --------------------------------------------------------------------------------------------

def missing(count, expected, modulus):
    """Frames missing, (count - expected) modulo the range (LNK-051). A forward distance beyond
    half the range is a backwards or reset count (CMN-031) that the documents leave open."""
    d = (count - expected) % modulus
    if d > modulus // 2:
        raise Unspecified("count goes backwards or resets: LNK-051 and CMN-031 differ")
    return d


class Counters:
    def __init__(self):
        self.c = {}

    def add(self, name, n=1):
        self.c[name] = self.c.get(name, 0) + n

    def get(self, name):
        return self.c.get(name, 0)


def query_out(counters, inp):
    name = inp["query"]["counter"]
    return [{"counter": {"name": name, "value": counters.get(name)}}]


# --------------------------------------------------------------------------------------------
# FrameReceiver (04 3.1; LNK-050 to LNK-054, LNK-077, LNK-079)
# --------------------------------------------------------------------------------------------

OCF_REPORT = {0: "Type1", 1: "Type2"}


class FrameReceiver:
    """One instance serves one physical channel and one protocol (LNK-001). The frame has been
    validated by the caller (LNK-050), so its length, FECF, and header fields are not checked."""

    def __init__(self, cfg):
        self.cfg = cfg
        self.counters = Counters()      # persist across Reconfigure (LNK-003)
        self.restart()

    def restart(self):
        """Start-up or reconfiguration: the first frame sets the expectation (LNK-051)."""
        self.mc_expect = None
        self.vc_expect = {}             # key -> expected count
        self.pending_gap = {}           # vcid -> a gap not yet flagged on a delivered data field

    def vcs(self):
        return {v["vcid"]: v for v in self.cfg["vcs"]}

    def step(self, inp):
        if "frame" in inp:
            return self.frame(unhx(inp["frame"]["frame_hex"]))
        if "frame_loss" in inp:
            return self.frame_loss()
        if "raw_frame" in inp:
            return self.raw_frame(unhx(inp["raw_frame"]["frame_hex"]))
        if "reconfigure" in inp:
            self.restart()
            return []        # a receiver retains no octets (LNK-003: Discarded only when any)
        if "query" in inp:
            return query_out(self.counters, inp)
        raise AssertionError(inp)

    # -- FrameLoss (LNK-053, OI-68) --
    def frame_loss(self):
        outs = [{"notify": {"kind": "LossFlag", "scope": "vc", "vc": v["vcid"]}}
                for v in self.cfg["vcs"]]
        outs.append({"notify": {"kind": "LossFlag", "scope": "physical_channel"}})
        return outs

    # -- helpers --
    def discard(self, reason, vc, map_id=None):
        self.counters.add(reason)
        d = {"reason": reason, "vc": vc}
        if map_id is not None:
            d["map"] = map_id
        return [{"frame_discarded": d}]

    def gap(self, key, count, modulus, outs, notify):
        """Compare `count` with the expectation of `key`; raise the gap action (LNK-051)."""
        exp = self.vc_expect.get(key)
        if exp is not None:
            m = missing(count, exp, modulus)
            if m:
                self.counters.add("gaps")
                outs.append({"notify": dict(notify, missing=m)})
                self.vc_expect[key] = (count + 1) % modulus
                return True
        self.vc_expect[key] = (count + 1) % modulus
        return False

    def ocf_out(self, ocf, scope, vc, extra):
        d = {"kind": "Ocf", "scope": scope}
        if scope == "vc":
            d["vc"] = vc
        d["report"] = OCF_REPORT[ocf[0] >> 7]
        d["ocf_hex"] = hx(ocf)
        d.update(extra)
        return {"deliver": d}

    def take_gap(self, vcid, detected):
        """A gap is flagged on the next data field delivered for the VC (LNK-051)."""
        g = detected or self.pending_gap.get(vcid, False)
        return g

    # -- the frame --
    def frame(self, f):
        p = self.cfg["protocol"]
        if len(f) != self.cfg["frame_len"]:
            raise Unspecified("the caller validates the frame length (LNK-050)")
        return {"tm": self.tm, "aos": self.aos, "uslp": self.uslp, "tc": self.tc}[p](f)

    def tm(self, f):
        cfg = self.cfg
        w0 = int.from_bytes(f[0:2], "big")
        vcid = (w0 >> 1) & 7
        ocf_flag = w0 & 1
        mc, vcc = f[2], f[3]
        w2 = int.from_bytes(f[4:6], "big")
        shf, sync, fhp = w2 >> 15, (w2 >> 14) & 1, w2 & 0x7FF
        vcmap = self.vcs()
        if vcid not in vcmap:
            return self.discard("UnknownChannel", vcid)          # LNK-050: alone
        role = vcmap[vcid]["role"]
        end = len(f) - (2 if cfg["fecf"] else 0) - (4 if ocf_flag else 0)
        start = 6
        fsh = None
        if shf:
            n = (f[6] & 0x3F) + 1
            fsh = f[6:6 + n]
            start += n
        data = f[start:end]
        ocf = f[end:end + 4] if ocf_flag else None
        outs = []
        # Master Channel and Virtual Channel Frame Counts, 8 bits each (132.0 4.1.2.5-6); an OID
        # frame counts in both (LNK-051).
        mc_gap = False
        if self.mc_expect is not None:
            m = missing(mc, self.mc_expect, 256)
            if m:
                self.counters.add("gaps")
                outs.append({"notify": {"kind": "FrameGap", "scope": "mc", "missing": m}})
                mc_gap = True
        self.mc_expect = (mc + 1) % 256
        vc_gap = self.gap(vcid, vcc, 256, outs,
                          {"kind": "FrameGap", "scope": "vc", "vc": vcid})
        if vc_gap:
            self.pending_gap[vcid] = True
        oid = role == "packets" and sync == 0 and fhp == 0x7FE
        if fsh is not None and cfg["fsh_service"] != "none":
            outs.append({"deliver": {"kind": "FshSdu", "scope": cfg["fsh_service"],
                                     **({"vc": vcid} if cfg["fsh_service"] == "vc" else {}),
                                     "data_hex": hx(fsh)}})
        if ocf is not None:
            outs.append(self.ocf_out(ocf, cfg["ocf_service"], vcid, {}))
        if oid:
            self.counters.add("oid_frames")
            return outs
        d = {"kind": "DataField", "vc": vcid, "data_hex": hx(data)}
        if role == "packets":
            d["fhp"] = fhp
        d["gap"] = self.pending_gap.pop(vcid, False)
        outs.append({"deliver": d})
        return outs

    def aos(self, f):
        cfg = self.cfg
        w0 = int.from_bytes(f[0:2], "big")
        vcid = w0 & 0x3F
        count = int.from_bytes(f[2:5], "big")
        sig = f[5]
        replay, cycle_use, cycle = sig >> 7, (sig >> 6) & 1, sig & 0xF
        vcmap = self.vcs()
        oid = vcid == 63                       # 732.0 4.1.4.1.5.1
        if not oid and vcid not in vcmap:
            return self.discard("UnknownChannel", vcid)
        vcd = vcmap.get(vcid, {"role": None, "ocf": False})
        izl = cfg["insert_zone_len"]
        end = len(f) - (2 if cfg["fecf"] else 0) - (4 if vcd["ocf"] else 0)
        iz = f[6:6 + izl]
        data = f[6 + izl:end]
        ocf = f[end:end + 4] if vcd["ocf"] else None
        outs = []
        rp = {"replay": bool(replay)}
        if not oid:
            if replay:
                pass       # OI-70: replay frames are excluded from counting and gaps
            else:
                if cycle_use:
                    value, modulus = (cycle << 24) | count, 1 << 28
                else:
                    value, modulus = count, 1 << 24
                mode = self.vc_expect.get(("mode", vcid))
                if mode is not None and mode != cycle_use:
                    raise Unspecified("the Cycle Use Flag changes on a VC")
                self.vc_expect[("mode", vcid)] = cycle_use
                if self.gap(vcid, value, modulus, outs,
                            {"kind": "FrameGap", "scope": "vc", "vc": vcid}):
                    self.pending_gap[vcid] = True
        if izl:
            outs.append({"deliver": {"kind": "InsertSdu", "data_hex": hx(iz), **rp}})
        if ocf is not None:
            outs.append(self.ocf_out(ocf, "vc", vcid, rp))
        if oid:
            self.counters.add("oid_frames")
            return outs
        role = vcd["role"]
        d = {"kind": "DataField", "vc": vcid}
        if role == "bitstream":
            zone = data[2:]
            bdp = int.from_bytes(data[0:2], "big") & 0x3FFF
            if bdp == 0x3FFE:
                return outs            # no valid bit: the idle B_PDU is dropped (LNK-077)
            n = len(zone) * 8 if bdp == 0x3FFF else bdp + 1
            if n > len(zone) * 8:
                raise Unspecified("Bitstream Data Pointer beyond the zone")
            nb = (n + 7) // 8
            valid = bytearray(zone[:nb])
            if n % 8:
                valid[-1] &= (0xFF << (8 - n % 8)) & 0xFF
            d["data_hex"] = hx(valid)
            d["bit_count"] = n
        else:
            d["data_hex"] = hx(data)
        d["gap"] = self.pending_gap.pop(vcid, False)
        d.update(rp)
        outs.append({"deliver": d})
        return outs

    def uslp(self, f):
        cfg = self.cfg
        top = int.from_bytes(f[0:4], "big")
        vcid = (top >> 5) & 0x3F
        mapid = (top >> 1) & 0xF
        flags = f[6]
        bypass, pcc, ocf_flag, vlen = flags >> 7, (flags >> 6) & 1, (flags >> 3) & 1, flags & 7
        count = int.from_bytes(f[7:7 + vlen], "big") if vlen else None
        oid = vcid == 63                       # 732.1 4.1.4.1.6
        vcmap = {v["vcid"]: v for v in cfg["vcs"]}
        if not oid:
            if vcid not in vcmap:
                return self.discard("UnknownChannel", vcid, mapid)
            if mapid not in vcmap[vcid]["maps"]:
                return self.discard("UnknownChannel", vcid, mapid)
        if pcc:
            raise Unspecified("Protocol Control Command frames are not specified for the receiver")
        izl = cfg["insert_zone_len"]
        hl = 7 + vlen
        end = len(f) - (2 if cfg["fecf"] else 0) - (4 if ocf_flag else 0)
        iz = f[hl:hl + izl]
        tf = f[hl + izl:end]
        ocf = f[end:end + 4] if ocf_flag else None
        outs = []
        if not oid and count is not None and not vcmap[vcid]["cop"]:
            # The VCF Count of the VC for the QoS; with COP-1 FARM-1 decides (LNK-051).
            if self.gap((vcid, bypass), count, 1 << (8 * vlen), outs,
                        {"kind": "FrameGap", "scope": "vc", "vc": vcid}):
                self.pending_gap[vcid] = True
        if izl:
            outs.append({"deliver": {"kind": "InsertSdu", "data_hex": hx(iz)}})
        if ocf is not None:
            outs.append(self.ocf_out(ocf, cfg["ocf_service"], vcid, {}))
        if oid:
            self.counters.add("oid_frames")
            return outs
        outs.append({"deliver": {"kind": "DataField", "vc": vcid, "data_hex": hx(tf),
                                 "gap": self.pending_gap.pop(vcid, False)}})
        return outs

    def tc(self, f):
        w1 = int.from_bytes(f[2:4], "big")
        vcid = w1 >> 10
        if vcid not in {v["vcid"] for v in self.cfg["vcs"]}:
            return self.discard("UnknownChannel", vcid)
        raise Unspecified("what the engine returns for a TC frame it routes to FARM-1")

    # -- RawFrame (LNK-079) --
    def raw_frame(self, f):
        cfg = self.cfg
        if not cfg.get("accept_rejected_frame_insert_zone"):
            raise Unspecified("RawFrame while the option is off")
        if len(f) != cfg["frame_len"]:
            raise Unspecified("the error for a wrong frame length")
        if cfg["protocol"] == "aos":
            raise Unspecified("replay metadata of an AOS frame whose header is not validated")
        vlen = f[6] & 7
        if vlen != 0:
            raise Unspecified("Insert Zone offset when the VCF Count Length is not 0")
        izl = cfg["insert_zone_len"]
        return [{"deliver": {"kind": "InsertSdu", "data_hex": hx(f[7:7 + izl])}}]


# --------------------------------------------------------------------------------------------
# PacketExtractor (04 3.2; LNK-060 to LNK-064)
# --------------------------------------------------------------------------------------------

FHP_NONE = {"tm": 0x7FF, "aos": 0xFFFF, "uslp": 0xFFFF}      # 04 2.1 table
FHP_IDLE = {"tm": 0x7FE, "aos": 0xFFFE, "uslp": None}


class PacketExtractor:
    def __init__(self, cfg):
        self.cfg = cfg
        self.counters = Counters()
        self.reset()

    def reset(self):
        self.partial = bytearray()
        self.sync = False            # start-up: resynchronise at the next FHP (LNK-061)

    # -- the field --
    def parse(self, inp):
        d = inp["data_field"]
        p = self.cfg["protocol"]
        raw = unhx(d["data_hex"])
        gap = d["gap"]
        if p == "tm":
            zone, fhp = raw, d["fhp"]
        elif p == "aos":
            fhp, zone = int.from_bytes(raw[0:2], "big"), raw[2:]
        else:
            rule, upid = raw[0] >> 5, raw[0] & 31
            fhp, zone = int.from_bytes(raw[1:3], "big"), raw[3:]
            if upid == UPID_IDLE:
                return zone, "idle", 0, gap
            if rule != 0:
                raise Unspecified("TFDF rule other than 000 for the extractor")
        if fhp == FHP_IDLE[p]:
            return zone, "idle", 0, gap
        if fhp == FHP_NONE[p]:
            return zone, "none", 0, gap
        if fhp >= len(zone):
            raise Unspecified("FHP beyond the zone (a codec error)")
        return zone, "offset", fhp, gap

    def step(self, inp):
        if "data_field" in inp:
            return self.data_field(inp)
        if "reconfigure" in inp:
            outs = []
            if self.partial:
                outs.append({"notify": {"kind": "Discarded", "octets": len(self.partial)}})
            self.reset()
            return outs
        if "query" in inp:
            return query_out(self.counters, inp)
        raise AssertionError(inp)

    # -- packet recognition on the octets collected so far --
    def status(self, b):
        """('more', None) | ('done', total) | ('error', kind). Looks only at what the octets
        collected so far allow: the PVN needs one octet, the length the whole length field."""
        pvn = b[0] >> 5
        p = self.cfg["protocol"]
        valid = pvn in self.cfg["valid_pvns"]
        if pvn == 7 and p == "uslp" and ((b[0] >> 2) & 7) == 0:
            valid = True       # an Encapsulation Idle Packet is delimited whatever the set (LNK-063)
        if not valid:
            return ("error", "InvalidPvn")
        total = None
        if pvn == 0:
            if len(b) >= 6:
                total = ((b[4] << 8) | b[5]) + 7                       # 133.0 4.1.3.5.2
        elif pvn == 7:
            lol = b[0] & 3
            if lol == 0:
                total = 1
            elif lol == 1:
                if len(b) >= 2:
                    total = b[1]
            elif lol == 2:
                if len(b) >= 4:
                    total = (b[2] << 8) | b[3]
            else:
                raise Unspecified("four-octet Encapsulation length field")
        else:
            raise Unspecified("a valid PVN that is neither a Space Packet nor an Encapsulation Packet")
        if total is None:
            return ("more", None)
        if total > self.cfg["max_packet_len"]:
            return ("error", "PacketTooLong")
        if pvn == 7 and total < (1 if (b[0] & 3) == 0 else (2 if (b[0] & 3) == 1 else 4)):
            raise Unspecified("Packet Length shorter than the header")
        if len(b) == total:
            return ("done", total)
        return ("more", None)

    def is_idle(self, b):
        pvn = b[0] >> 5
        if pvn == 0:
            return len(b) >= 2 and (((b[0] & 7) << 8) | b[1]) == 2047
        return ((b[0] >> 2) & 7) == 0

    def emit(self, pkt, outs):
        if self.is_idle(pkt):
            self.counters.add("idle_packets")
            if not self.cfg["deliver_idle"]:
                return
        outs.append({"deliver": {"kind": "Packet", "pvn": pkt[0] >> 5, "data_hex": hx(pkt),
                                 "incomplete": False}})

    def abandon(self, outs):
        """The outcome for the packet in progress: Incomplete or Discarded (LNK-061)."""
        if not self.partial:
            return
        if len(self.partial) >= 2 and self.is_idle(self.partial):
            raise Unspecified("a partial idle packet is discarded")
        if self.cfg["deliver_incomplete"]:
            outs.append({"deliver": {"kind": "Packet", "pvn": self.partial[0] >> 5,
                                     "data_hex": hx(self.partial), "incomplete": True}})
        else:
            outs.append({"notify": {"kind": "Discarded", "octets": len(self.partial)}})
        self.partial = bytearray()

    def error(self, kind, outs):
        self.counters.add(kind)
        outs.append({"extraction_error": {"kind": kind}})

    def run(self, zone, lo, hi, outs, ends_at_hi=False):
        """Feed zone[lo:hi] to the packet in progress and the packets that follow. Returns True
        when an extraction error stopped the run (the rest is discarded, LNK-062). With
        `ends_at_hi` the octets are the continuation before an FHP (or a whole field in which
        no packet starts): a packet that completes before the last of them is not specified,
        since the FHP and the length field would disagree in a way 04 does not settle."""
        for i in range(lo, hi):
            self.partial.append(zone[i])
            st, val = self.status(self.partial)
            if st == "error":
                self.error(val, outs)
                self.partial = bytearray()
                return True
            if st == "done":
                if ends_at_hi and i != hi - 1:
                    raise Unspecified("a packet ends before the FHP, or inside a field in which "
                                      "no packet starts")
                self.emit(bytes(self.partial), outs)
                self.partial = bytearray()
        return False

    def data_field(self, inp):
        zone, kind, off, gap = self.parse(inp)
        outs = []
        if kind == "idle":
            if gap:
                raise Unspecified("gap flagged on a field of only idle data")
            return outs        # LNK-064: ignored, a packet in progress is kept
        if gap:
            self.abandon(outs)          # LNK-061
            self.sync = False
        if not self.sync:
            if kind == "none":
                return outs             # no start: ignored until a field has an FHP
            self.sync = True
            self.partial = bytearray()
            self.sync = not self.run(zone, off, len(zone), outs)
            return outs
        # Synchronised: the octets before the FHP belong to the packet in progress.
        if kind == "none":
            if not self.partial:
                self.error("LengthInconsistent", outs)       # nothing in progress, nothing starts
                self.sync = False
                return outs
            self.sync = not self.run(zone, 0, len(zone), outs, ends_at_hi=True)
            return outs
        if not self.partial:
            if off != 0:
                self.error("LengthInconsistent", outs)       # FHP wins (LNK-060)
        elif not self.run(zone, 0, off, outs, ends_at_hi=True) and self.partial:
            self.abandon(outs)
            self.error("LengthInconsistent", outs)           # LNK-061: FHP is correct
        # An error before the FHP ends the continuation only; the packets from the FHP follow.
        self.sync = not self.run(zone, off, len(zone), outs)
        return outs


# --------------------------------------------------------------------------------------------
# MapReassembler (04 3.3; LNK-070 to LNK-072, LNK-071)
# --------------------------------------------------------------------------------------------

def split_packets(data, valid_pvns=(0, 7)):
    """Blocked packets split by the length field of each (LNK-071; 232.0 4.4.1.3, 4.4.4.3;
    732.1 4.3.2.3.3). A leftover or a packet that does not fit is not specified."""
    pkts = []
    i = 0
    while i < len(data):
        b = data[i:]
        pvn = b[0] >> 5
        if pvn == 0 and len(b) >= 6:
            total = ((b[4] << 8) | b[5]) + 7
        elif pvn == 7:
            lol = b[0] & 3
            if lol == 0:
                total = 1
            elif lol == 1 and len(b) >= 2:
                total = b[1]
            elif lol == 2 and len(b) >= 4:
                total = (b[2] << 8) | b[3]
            else:
                raise Unspecified("Encapsulation header split")
        else:
            raise Unspecified("blocked packets of another PVN or a header split")
        if total > len(b):
            raise Unspecified("a blocked packet that does not fit its FDU")
        pkts.append(bytes(b[:total]))
        i += total
    return pkts


class MapReassembler:
    def __init__(self, cfg):
        self.cfg = cfg
        self.counters = Counters()
        self.reset()

    def reset(self):
        self.partial = {}            # (vc, map) -> bytearray: the SDU in progress

    def maps(self):
        if self.cfg["protocol"] == "tc":
            return {(self.cfg["vc"], m["map"]): m for m in self.cfg["maps"]}
        return {(m["vc"], m["map"]): m for m in self.cfg["maps"]}

    def step(self, inp):
        if "fdu" in inp:
            return self.fdu(inp["fdu"])
        if "reconfigure" in inp:
            n = sum(len(b) for b in self.partial.values())
            self.reset()
            return [{"notify": {"kind": "Discarded", "octets": n}}] if n else []
        if "query" in inp:
            return query_out(self.counters, inp)
        raise AssertionError(inp)

    def event(self, kind, vc, mp):
        self.counters.add(kind)
        return {"reassembly_event": {"kind": kind, "vc": vc, "map": mp}}

    def deliver_sdu(self, m, vc, mp, sdu, whole):
        """One complete SDU, segment sequence, or blocked FDU (LNK-070, LNK-071)."""
        if m["service"] == "sdu":
            return [{"deliver": {"kind": "Sdu", "vc": vc, "map": mp, "data_hex": hx(sdu)}}]
        pkts = split_packets(sdu) if whole else [bytes(sdu)]
        if not whole and split_packets(sdu) != pkts:
            raise Unspecified("a segmented packet whose length field disagrees")
        return [{"deliver": {"kind": "Packet", "vc": vc, "map": mp, "pvn": p[0] >> 5,
                             "data_hex": hx(p), "incomplete": False}} for p in pkts]

    def fdu(self, d):
        p = self.cfg["protocol"]
        data = unhx(d["data_hex"])
        vc = d["vc"]
        gap = d.get("gap", False)
        outs = []
        if p == "tc":
            # Segment Header (232.0 4.1.3.2.2): sequence flags (2 bits), MAP ID (6 bits).
            flags, mp, body = data[0] >> 6, data[0] & 0x3F, data[1:]
            seq = {1: "first", 0: "cont", 2: "last", 3: "whole"}[flags]
        else:
            mp = d["map"]
            rule, upid = data[0] >> 5, data[0] & 31
            if rule <= 2:
                ptr, body = int.from_bytes(data[1:3], "big"), data[3:]
            else:
                ptr, body = None, data[1:]
            seq = {1: "first", 2: "cont", 4: "first", 5: "cont", 6: "last", 7: "whole",
                   3: "stream"}.get(rule)
            if rule == 0:
                raise Unspecified("rule 000 belongs to the PacketExtractor")
        key = (vc, mp)
        m = self.maps().get(key)
        if m is None:
            return [self.event("UnknownMap", vc, mp)]      # LNK-070
        if gap:
            if p == "tc":
                raise Unspecified("TC has no frame gap signal")
            if seq == "stream":
                raise Unspecified("a gap on an octet stream")
            lost = [k for k, v in self.partial.items() if k[0] == vc]
            if lost:
                for k in lost:
                    del self.partial[k]
                outs.append(self.event("GapDiscard", vc, mp))
        if len([k for k in self.partial if k[0] == vc and k != key]) and p == "uslp":
            raise Unspecified("two SDUs in progress on one VC")
        have = key in self.partial
        usl_fixed = p == "uslp" and rule in (1, 2)
        if seq == "stream":
            if m["service"] != "octet_stream":
                raise Unspecified("octet stream rule on another MAP service")
            outs.append({"deliver": {"kind": "Sdu", "vc": vc, "map": mp, "data_hex": hx(body),
                                     "octet_stream": True}})
            return outs
        if seq == "whole":
            if have:
                raise Unspecified("a complete SDU while one is in progress")
            return outs + self.deliver_sdu(m, vc, mp, body, True)
        if seq == "first":
            if have:
                outs.append(self.event("SequenceViolation", vc, mp))     # OI-72
                del self.partial[key]
            if usl_fixed:
                if ptr != 0xFFFF:
                    if ptr >= len(body):
                        raise Unspecified("Last Valid Octet Pointer beyond the zone")
                    return outs + self.deliver_sdu(m, vc, mp, body[:ptr + 1], False)
            self.partial[key] = bytearray(body)
            return outs
        # a continuation or the last segment
        if not have:
            outs.append(self.event("SequenceViolation", vc, mp))
            return outs
        buf = self.partial[key]
        if usl_fixed:
            if ptr == 0xFFFF:
                buf += body
                return outs
            if ptr >= len(body):
                raise Unspecified("Last Valid Octet Pointer beyond the zone")
            buf += body[:ptr + 1]
            del self.partial[key]
            return outs + self.deliver_sdu(m, vc, mp, bytes(buf), False)
        buf += body
        if seq == "last":
            del self.partial[key]
            return outs + self.deliver_sdu(m, vc, mp, bytes(buf), False)
        return outs


ENGINES = {"frame-receiver": FrameReceiver, "packet-extractor": PacketExtractor,
           "map-reassembler": MapReassembler}


def replay(engine, cfg, inputs):
    """Run the inputs through a fresh engine; returns the steps with their expected outputs."""
    eng = ENGINES[engine](cfg)
    steps = []
    for at, inp in inputs:
        steps.append({"at_us": at, "input": inp, "expect": eng.step(inp)})
    return steps


# --------------------------------------------------------------------------------------------
# Scenario scripts
# --------------------------------------------------------------------------------------------

REQ_NOTE = "requirements"


class Scn:
    """A scenario under construction: the inputs are given, the outputs are the model's."""

    def __init__(self, sid, reqs, description, config=None):
        self.sid, self.reqs, self.description, self.config = sid, reqs, description, config
        self.inputs = []

    def add(self, inp):
        self.inputs.append((1000 * (len(self.inputs) + 1), inp))
        return self

    def counter(self, name):
        return self.add({"query": {"counter": name}})


def q(name):
    return {"query": {"counter": name}}


def frame_in(f):
    return {"frame": {"frame_hex": hx(f)}}


# ---- FrameReceiver, TM ----

TM_CFG = {"protocol": "tm", "scid": 291, "frame_len": 48, "fecf": True,
          "ocf_service": "vc", "fsh_service": "vc",
          "vcs": [{"vcid": 0, "role": "packets"}, {"vcid": 1, "role": "vca"},
                  {"vcid": 7, "role": "packets"}]}


def fr_tm_scenarios():
    c = TM_CFG
    out = []

    s = Scn("link-recv-tm-routing", ["LNK-050", "LNK-077"],
            "A frame of a packets VC is delivered with its FHP, a frame of a VCA VC (sync flag 1) "
            "as one VCA_SDU; each data field is the whole TM data field (132.0 4.3.3.2).")
    s.add(frame_in(tm_frame(c, 0, 10, 20, 0, 0x10)))
    s.add(frame_in(tm_frame(c, 1, 11, 5, 0x7FF, 0x40, sync=1)))
    s.add(frame_in(tm_frame(c, 0, 12, 21, 0x7FF, 0x70)))
    out.append(s)

    s = Scn("link-recv-tm-unknown-vc", ["LNK-050"],
            "A frame for a VC that is not configured is discarded and counted: FrameDiscarded "
            "(UnknownChannel) alone, no gap action and no delivery (132.0 4.3.4.3, LNK-050).")
    s.add(frame_in(tm_frame(c, 3, 40, 9, 0, 1)))
    s.counter("UnknownChannel")
    out.append(s)

    s = Scn("link-recv-tm-counts-in-sequence", ["LNK-051"],
            "The first frame sets both expectations; consecutive counts, including the wrap "
            "255 to 0 of both counters, raise no gap action (132.0 4.1.2.5.2, 4.1.2.6.2).")
    s.add(frame_in(tm_frame(c, 0, 254, 250, 0, 1)))
    s.add(frame_in(tm_frame(c, 0, 255, 251, 0, 2)))
    s.add(frame_in(tm_frame(c, 0, 0, 252, 0, 3)))
    s.counter("gaps")
    out.append(s)

    s = Scn("link-recv-tm-vc-and-mc-gap", ["LNK-051", "LNK-061"],
            "Two frames of a VC lost: the MC gap action first, then the VC gap action, each with "
            "the number missing; the data field delivered carries gap = true (LNK-051).")
    s.add(frame_in(tm_frame(c, 0, 10, 20, 0, 1)))
    s.add(frame_in(tm_frame(c, 0, 13, 23, 0, 2)))
    s.add(frame_in(tm_frame(c, 0, 14, 24, 0, 3)))
    s.counter("gaps")
    out.append(s)

    s = Scn("link-recv-tm-mc-gap-only", ["LNK-051"],
            "A frame of another VC is lost: the MC Frame Count jumps by one, the VC Frame Counts "
            "stay in sequence; only the MC gap action is raised and the data field carries "
            "gap = false.")
    s.add(frame_in(tm_frame(c, 0, 10, 20, 0, 1)))
    s.add(frame_in(tm_frame(c, 0, 12, 21, 0, 2)))
    out.append(s)

    s = Scn("link-recv-tm-gap-across-wrap", ["LNK-051"],
            "The missing count is computed modulo the range: 254 then 2 is three frames lost on "
            "the MC and on the VC.")
    s.add(frame_in(tm_frame(c, 0, 254, 254, 0, 1)))
    s.add(frame_in(tm_frame(c, 0, 2, 2, 0, 2)))
    out.append(s)

    s = Scn("link-recv-tm-oid-frame", ["LNK-052", "LNK-051"],
            "An OID frame (FHP 0x7FE) on the dedicated VC 7 carries a Secondary Header and an OCF: "
            "both are delivered, the data is dropped, no FrameDiscarded; it counts in the MC and "
            "VC Frame Counts of its channel (the next frame of VC 0 follows the MC count without "
            "a gap), and the OID frame counter is 2.")
    s.add(frame_in(tm_frame(c, 0, 30, 3, 0, 1)))
    o = Oid()
    s.add(frame_in(tm_frame(c, 7, 31, 100, 0x7FE, o, fsh=tm_fsh(0xAA), ocf=OCF_CLCW)))
    s.add(frame_in(tm_frame(c, 0, 32, 4, 0, 2)))
    s.add(frame_in(tm_frame(c, 7, 33, 101, 0x7FE, o)))
    s.counter("oid_frames")
    s.counter("gaps")
    out.append(s)

    s = Scn("link-recv-tm-oid-frame-gap", ["LNK-052", "LNK-051"],
            "A lost OID frame is a gap in the MC Frame Count and in the VC Frame Count of the "
            "dedicated VC: both gap actions, then no delivery of the OID data.")
    o = Oid()
    s.add(frame_in(tm_frame(c, 7, 30, 100, 0x7FE, o)))
    s.add(frame_in(tm_frame(c, 7, 32, 102, 0x7FE, o)))
    out.append(s)

    s = Scn("link-recv-tm-fsh-ocf-delivery", ["LNK-053"],
            "The whole Secondary Header (ID octet included) is delivered as an FSH SDU and the OCF "
            "as an Ocf action naming the VC, Type-1 for a CLCW, Type-2 otherwise (project "
            "specific or SDLS FSR); the data field follows (132.0 4.3.3.3).")
    s.add(frame_in(tm_frame(c, 0, 1, 1, 0, 1, fsh=tm_fsh(0x11, 0x22), ocf=OCF_CLCW)))
    s.add(frame_in(tm_frame(c, 0, 2, 2, 0, 2, ocf=OCF_T2_PROJECT)))
    s.add(frame_in(tm_frame(c, 0, 3, 3, 0, 3, ocf=OCF_T2_FSR)))
    out.append(s)

    s = Scn("link-recv-tm-mc-services", ["LNK-053"],
            "With the MC services configured the FSH SDU and the OCF name the master channel and "
            "not the VC.",
            config=dict(c, ocf_service="mc", fsh_service="mc"))
    s.add(frame_in(tm_frame(c, 0, 1, 1, 0, 1, fsh=tm_fsh(0x11), ocf=OCF_CLCW)))
    out.append(s)

    s = Scn("link-recv-tm-no-fsh-service", ["LNK-053"],
            "With no FSH service configured a Secondary Header is not delivered.",
            config=dict(c, fsh_service="none"))
    s.add(frame_in(tm_frame(c, 0, 1, 1, 0, 1, fsh=tm_fsh(0x11, 0x22))))
    out.append(s)

    s = Scn("link-recv-tm-frame-loss", ["LNK-053"],
            "The loss signal of the coding sublayer is the FrameLoss input: one Notify(LossFlag) "
            "for each configured VC, then one for the physical channel (OI-68, 04 section 4).")
    s.add({"frame_loss": {}})
    out.append(s)

    s = Scn("link-recv-tm-reconfigure", ["LNK-051", "LNK-003"],
            "Reconfigure restarts the expected counts: the frame after it sets a new expectation "
            "and raises no gap; a receiver retains no octets, so nothing is discarded.")
    s.add(frame_in(tm_frame(c, 0, 10, 20, 0, 1)))
    s.add({"reconfigure": {}})
    s.add(frame_in(tm_frame(c, 0, 90, 80, 0, 2)))
    out.append(s)

    s = Scn("link-recv-tm-gap-kept-for-delivery", ["LNK-051", "LNK-061"],
            "A gap is flagged on the data field delivered after it: the VCA VC loses a frame, and "
            "the field delivered with the new count carries gap = true; the next one does not.")
    s.add(frame_in(tm_frame(c, 1, 5, 7, 0, 1, sync=1)))
    s.add(frame_in(tm_frame(c, 1, 7, 9, 0, 2, sync=1)))
    s.add(frame_in(tm_frame(c, 1, 8, 10, 0, 3, sync=1)))
    out.append(s)
    return out


# ---- FrameReceiver, AOS ----

AOS_CFG = {"protocol": "aos", "scid": 677, "frame_len": 48, "fecf": True, "insert_zone_len": 4,
           "accept_rejected_frame_insert_zone": False,
           "vcs": [{"vcid": 1, "role": "packets", "ocf": True},
                   {"vcid": 2, "role": "vca", "ocf": False},
                   {"vcid": 3, "role": "bitstream", "ocf": False}]}
AOS_ZONE = AOS_CFG["frame_len"] - 6 - AOS_CFG["insert_zone_len"] - 2     # 36 without OCF
IZ1 = bytes([0xDE, 0xAD, 0xBE, 0xEF])
IZ2 = bytes([0x01, 0x02, 0x03, 0x04])


def fr_aos_scenarios():
    c = AOS_CFG
    zl = AOS_ZONE
    out = []

    s = Scn("link-recv-aos-routing", ["LNK-050", "LNK-077", "LNK-053"],
            "A frame of a packets VC delivers its Insert Zone, the OCF (VC_OCF), and the M_PDU "
            "(header and packet zone); a VCA VC delivers the whole data field as one VCA_SDU. The "
            "SCID uses its two MSBs (732.0 4.1.2.5.4).")
    s.add(frame_in(aos_frame(c, 1, 100, mpdu(0, zl - 4 - 2, ramp(1, 20)), iz=IZ1, ocf=OCF_CLCW)))
    s.add(frame_in(aos_frame(c, 2, 7, 0x40, iz=IZ2)))
    out.append(s)

    s = Scn("link-recv-aos-unknown-vc", ["LNK-050"],
            "A frame for a VC that is not configured is discarded and counted; FrameDiscarded "
            "(UnknownChannel) alone (732.0 4.3.5.4).")
    s.add(frame_in(aos_frame(c, 9, 1, 1, iz=IZ1)))
    s.counter("UnknownChannel")
    out.append(s)

    s = Scn("link-recv-aos-vc-gap", ["LNK-051", "LNK-061"],
            "The VC Frame Count is tracked per VC; a jump of three is a gap of two frames, and "
            "the field delivered carries gap = true. Another VC keeps its own expectation.")
    s.add(frame_in(aos_frame(c, 1, 100, mpdu(0, zl - 6), iz=IZ1, ocf=OCF_CLCW)))
    s.add(frame_in(aos_frame(c, 2, 50, 1, iz=IZ1)))
    s.add(frame_in(aos_frame(c, 1, 103, mpdu(0, zl - 6), iz=IZ1, ocf=OCF_CLCW)))
    s.add(frame_in(aos_frame(c, 2, 51, 2, iz=IZ1)))
    s.add(frame_in(aos_frame(c, 1, 104, mpdu(0, zl - 6), iz=IZ1, ocf=OCF_CLCW)))
    out.append(s)

    s = Scn("link-recv-aos-count-wrap", ["LNK-051"],
            "The 24-bit VC Frame Count wraps from 16777215 to 0 without a gap, and a gap is "
            "counted modulo 2^24 (732.0 4.1.2.4.2).")
    s.add(frame_in(aos_frame(c, 2, 0xFFFFFF, 1, iz=IZ1)))
    s.add(frame_in(aos_frame(c, 2, 0, 2, iz=IZ1)))
    s.add(frame_in(aos_frame(c, 2, 3, 3, iz=IZ1)))
    out.append(s)

    s = Scn("link-recv-aos-cycle-extension", ["LNK-051"],
            "With the VC Frame Count Cycle in use the count is the 28-bit value cycle:count "
            "(732.0 4.1.2.5.5): 0xFFFFFF in cycle 3 followed by 0 in cycle 4 is in sequence; "
            "count 4 after count 1 in cycle 4 is a gap of two; count 4 in cycle 5 after count "
            "4 in cycle 4 is a gap of 2^24 - 1 frames, which a 24-bit comparison would miss.")
    s.add(frame_in(aos_frame(c, 2, 0xFFFFFF, 1, iz=IZ1, cycle=3)))
    s.add(frame_in(aos_frame(c, 2, 0, 2, iz=IZ1, cycle=4)))
    s.add(frame_in(aos_frame(c, 2, 1, 3, iz=IZ1, cycle=4)))
    s.add(frame_in(aos_frame(c, 2, 4, 4, iz=IZ1, cycle=4)))
    s.add(frame_in(aos_frame(c, 2, 4, 5, iz=IZ1, cycle=5)))
    out.append(s)

    s = Scn("link-recv-aos-oid-frame", ["LNK-052", "LNK-051"],
            "An OID frame (VCID 63) delivers its Insert Zone, then its data is dropped; it "
            "returns no FrameDiscarded and its VC Frame Count is not tracked, so counts that "
            "jump raise no gap action; the OID frame counter is 3.")
    o = Oid()
    s.add(frame_in(aos_frame(c, 63, 5, o, iz=IZ1)))
    s.add(frame_in(aos_frame(c, 63, 5, o, iz=IZ2)))
    s.add(frame_in(aos_frame(c, 63, 900, o, iz=IZ1)))
    s.counter("oid_frames")
    s.counter("gaps")
    out.append(s)

    s = Scn("link-recv-aos-replay-flag", ["LNK-054", "LNK-051"],
            "Replay frames (Replay Flag 1) are delivered with replay = true on every delivered "
            "SDU, take no part in counting, and raise no gap even when the count decreases; the "
            "real-time frame after them is compared with the last real-time frame (OI-70).")
    s.add(frame_in(aos_frame(c, 1, 100, mpdu(0, zl - 6), iz=IZ1, ocf=OCF_CLCW)))
    s.add(frame_in(aos_frame(c, 1, 40, mpdu(0, zl - 6), iz=IZ2, ocf=OCF_CLCW, replay=1)))
    s.add(frame_in(aos_frame(c, 1, 41, mpdu(0, zl - 6), iz=IZ2, ocf=OCF_T2_PROJECT, replay=1)))
    s.add(frame_in(aos_frame(c, 1, 101, mpdu(0, zl - 6), iz=IZ1, ocf=OCF_CLCW)))
    s.add(frame_in(aos_frame(c, 1, 103, mpdu(0, zl - 6), iz=IZ1, ocf=OCF_CLCW)))
    out.append(s)

    s = Scn("link-recv-aos-bitstream", ["LNK-077"],
            "A B_PDU delivers its valid bits as a bit count: pointer all ones, every bit of the "
            "zone is valid; pointer 19, 20 bits (the unused bits of the last octet are zero); "
            "pointer all ones minus one, no valid bit, nothing is delivered (732.0 4.3.3.2).")
    bz = zl - 2
    s.add(frame_in(aos_frame(c, 3, 1, bpdu(0x3FFF, ramp(0x80, bz)), iz=IZ1)))
    s.add(frame_in(aos_frame(c, 3, 2, bpdu(19, bytes([0xFF]) * bz), iz=IZ1)))
    s.add(frame_in(aos_frame(c, 3, 3, bpdu(0x3FFE, bytes([0x55]) * bz), iz=IZ1)))
    out.append(s)

    s = Scn("link-recv-aos-bitstream-gap", ["LNK-077", "LNK-051"],
            "A gap on a bitstream VC stays flagged until the next valid bitstream data is "
            "delivered: the B_PDU of only idle data that follows the gap is dropped, and the next "
            "one carries gap = true (732.0 4.3.3.3 note).")
    s.add(frame_in(aos_frame(c, 3, 1, bpdu(0x3FFF, ramp(1, bz)), iz=IZ1)))
    s.add(frame_in(aos_frame(c, 3, 4, bpdu(0x3FFE, ramp(2, bz)), iz=IZ1)))
    s.add(frame_in(aos_frame(c, 3, 5, bpdu(7, ramp(3, bz)), iz=IZ1)))
    s.add(frame_in(aos_frame(c, 3, 6, bpdu(0x3FFF, ramp(4, bz)), iz=IZ1)))
    out.append(s)

    s = Scn("link-recv-aos-ocf-types", ["LNK-053"],
            "The OCF is delivered as an Ocf action naming the VC: Type-1 for a CLCW, Type-2 for "
            "a project-specific report and for an SDLS FSR.")
    s.add(frame_in(aos_frame(c, 1, 1, mpdu(0, zl - 6), iz=IZ1, ocf=OCF_CLCW)))
    s.add(frame_in(aos_frame(c, 1, 2, mpdu(0, zl - 6), iz=IZ1, ocf=OCF_T2_PROJECT)))
    s.add(frame_in(aos_frame(c, 1, 3, mpdu(0, zl - 6), iz=IZ1, ocf=OCF_T2_FSR)))
    out.append(s)

    s = Scn("link-recv-aos-frame-loss", ["LNK-053"],
            "FrameLoss: one Notify(LossFlag) for each configured VC in configuration order, "
            "then one for the physical channel; the OID VC 63 has no configuration entry.")
    s.add({"frame_loss": {}})
    out.append(s)

    s = Scn("link-recv-aos-reconfigure", ["LNK-051", "LNK-003"],
            "Reconfigure restarts the expected counts.")
    s.add(frame_in(aos_frame(c, 2, 10, 1, iz=IZ1)))
    s.add({"reconfigure": {}})
    s.add(frame_in(aos_frame(c, 2, 500, 2, iz=IZ1)))
    out.append(s)
    return out


# ---- FrameReceiver, USLP ----

USLP_CFG = {"protocol": "uslp", "scid": 0xC0DE, "frame_len": 48, "fecf": True,
            "insert_zone_len": 3, "ocf_service": "mc",
            "accept_rejected_frame_insert_zone": False,
            "vcs": [{"vcid": 1, "maps": [0, 2], "cop": False},
                    {"vcid": 2, "maps": [0], "cop": True}]}


def uz(c, vcf_len=0, ocf=False):
    return c["frame_len"] - 7 - vcf_len - c["insert_zone_len"] - 2 - (4 if ocf else 0)


UIZ = bytes([0xA1, 0xA2, 0xA3])
UIZ2 = bytes([0xB1, 0xB2, 0xB3])


def fr_uslp_scenarios():
    c = USLP_CFG
    out = []

    def td(vlen, ocf=False, rule=0, upid=0, ptr=0, payload=b""):
        return tfdf(rule, upid, ptr, uz(c, vlen, ocf) - 3, payload)

    s = Scn("link-recv-uslp-routing", ["LNK-050", "LNK-053"],
            "A frame of a configured VC and MAP delivers its Insert Zone, then the TFDF (header "
            "and zone) as the data field; the OCF is an MC_OCF Ocf action (732.1 4.3.9.2).")
    s.add(frame_in(uslp_frame(c, 1, 0, td(2, True, 0, 0, 0, ramp(1, 8)), vcf=(2, 1), iz=UIZ,
                              ocf=OCF_CLCW)))
    s.add(frame_in(uslp_frame(c, 1, 2, td(2, False, 1, UPID_MISSION_1, 0xFFFF, ramp(9, 8)),
                              vcf=(2, 2), iz=UIZ2)))
    out.append(s)

    s = Scn("link-recv-uslp-unknown-vc-and-map", ["LNK-050"],
            "A frame for a VC that is not configured, and a frame for a configured VC with a "
            "MAP ID that is not configured, are discarded and counted: FrameDiscarded "
            "(UnknownChannel) alone, naming the VC and, for USLP, the MAP (732.1 4.3.8.3 note 2).")
    s.add(frame_in(uslp_frame(c, 9, 0, td(0), vcf=(0, 0), iz=UIZ)))
    s.add(frame_in(uslp_frame(c, 1, 7, td(2), vcf=(2, 1), iz=UIZ)))
    s.counter("UnknownChannel")
    out.append(s)

    s = Scn("link-recv-uslp-vcf-gap", ["LNK-051", "LNK-061"],
            "The VCF Count is tracked per VC for the QoS of the Bypass/Sequence Control Flag: "
            "sequence-controlled and expedited frames have separate counts; a gap of two on the "
            "sequence-controlled count does not disturb the expedited one (732.1 4.1.2.12.4-5).")
    s.add(frame_in(uslp_frame(c, 1, 0, td(2), vcf=(2, 10), iz=UIZ)))
    s.add(frame_in(uslp_frame(c, 1, 0, td(2), vcf=(2, 200), bypass=1, iz=UIZ)))
    s.add(frame_in(uslp_frame(c, 1, 0, td(2), vcf=(2, 11), iz=UIZ)))
    s.add(frame_in(uslp_frame(c, 1, 0, td(2), vcf=(2, 201), bypass=1, iz=UIZ)))
    s.add(frame_in(uslp_frame(c, 1, 0, td(2), vcf=(2, 14), iz=UIZ)))
    s.add(frame_in(uslp_frame(c, 1, 0, td(2), vcf=(2, 202), bypass=1, iz=UIZ)))
    out.append(s)

    s = Scn("link-recv-uslp-count-wrap", ["LNK-051"],
            "A one-octet VCF Count wraps from 255 to 0 in sequence, and a gap is counted "
            "modulo 256.")
    s.add(frame_in(uslp_frame(c, 1, 0, td(1), vcf=(1, 255), iz=UIZ)))
    s.add(frame_in(uslp_frame(c, 1, 0, td(1), vcf=(1, 0), iz=UIZ)))
    s.add(frame_in(uslp_frame(c, 1, 0, td(1), vcf=(1, 3), iz=UIZ)))
    out.append(s)

    s = Scn("link-recv-uslp-no-count-no-gap", ["LNK-051"],
            "A VCF Count Length of 0 has no count: nothing is tracked; with COP-1 in effect "
            "on the VC the FARM-1 checks decide and the engine raises no gap action either.")
    s.add(frame_in(uslp_frame(c, 1, 0, td(0), vcf=(0, 0), iz=UIZ)))
    s.add(frame_in(uslp_frame(c, 1, 0, td(0), vcf=(0, 0), iz=UIZ)))
    s.add(frame_in(uslp_frame(c, 2, 0, td(2), vcf=(2, 5), iz=UIZ)))
    s.add(frame_in(uslp_frame(c, 2, 0, td(2), vcf=(2, 9), iz=UIZ)))
    out.append(s)

    s = Scn("link-recv-uslp-oid-frame", ["LNK-052", "LNK-051"],
            "An OID frame (VCID 63, MAP 0, rule 001, UPID Only Idle Data) delivers its Insert "
            "Zone and, when its OCF flag is set, the OCF; then its data is dropped; no "
            "FrameDiscarded; its VCF Count, when present, is not tracked, so a jump raises no "
            "gap action (732.1 4.3.8.3 note 1, 4.1.4.1.10 note 4).")
    og = Oid()
    oidhdr = lambda v, o: tfdf(1, UPID_IDLE, uz(c, v, o) - 3 - 1, uz(c, v, o) - 3,
                               og.take(uz(c, v, o) - 3))
    s.add(frame_in(uslp_frame(c, 63, 0, oidhdr(0, False), vcf=(0, 0), iz=UIZ)))
    s.add(frame_in(uslp_frame(c, 63, 0, oidhdr(2, True), vcf=(2, 40), iz=UIZ2, ocf=OCF_CLCW)))
    s.add(frame_in(uslp_frame(c, 63, 0, oidhdr(2, False), vcf=(2, 90), iz=UIZ)))
    s.counter("oid_frames")
    s.counter("gaps")
    out.append(s)

    s = Scn("link-recv-uslp-ocf-types", ["LNK-053"],
            "The OCF is delivered as an Ocf action: Type-1 for a CLCW or PLCW, Type-2 otherwise.")
    s.add(frame_in(uslp_frame(c, 1, 0, td(0, True), vcf=(0, 0), iz=UIZ, ocf=OCF_CLCW)))
    s.add(frame_in(uslp_frame(c, 1, 0, td(0, True), vcf=(0, 0), iz=UIZ, ocf=OCF_T2_PROJECT)))
    s.add(frame_in(uslp_frame(c, 1, 0, td(0, True), vcf=(0, 0), iz=UIZ, ocf=OCF_T2_FSR)))
    out.append(s)

    s = Scn("link-recv-uslp-frame-loss", ["LNK-053"],
            "FrameLoss: one Notify(LossFlag) per configured VC, then one for the physical channel.")
    s.add({"frame_loss": {}})
    out.append(s)

    s = Scn("link-recv-uslp-reconfigure", ["LNK-051", "LNK-003"],
            "Reconfigure restarts the expected counts.")
    s.add(frame_in(uslp_frame(c, 1, 0, td(2), vcf=(2, 10), iz=UIZ)))
    s.add({"reconfigure": {}})
    s.add(frame_in(uslp_frame(c, 1, 0, td(2), vcf=(2, 700), iz=UIZ)))
    out.append(s)

    on = dict(c, accept_rejected_frame_insert_zone=True)
    s = Scn("link-recv-uslp-raw-frame-insert-zone", ["LNK-079"],
            "With the option on, the octets of a frame the caller rejected (a wrong FECF here) "
            "give the Insert Zone IN_SDU at its fixed offset after the 7-octet header "
            "(VCF Count Length 0), after the frame length check (732.1 4.3.11.1.4-5).",
            config=on)
    good = uslp_frame(on, 1, 0, td(0), vcf=(0, 0), iz=UIZ)
    bad = good[:-1] + bytes([good[-1] ^ 0xFF])
    s.add({"raw_frame": {"frame_hex": hx(bad)}})
    out.append(s)
    return out


# ---- FrameReceiver, TC ----

TC_CFG = {"protocol": "tc", "scid": 42, "frame_len": 16, "fecf": True,
          "vcs": [{"vcid": 1}]}


def fr_tc_scenarios():
    c = TC_CFG
    out = []
    s = Scn("link-recv-tc-unknown-vc", ["LNK-050"],
            "A TC frame for a VC that is not configured is discarded and counted: FrameDiscarded "
            "(UnknownChannel) alone (232.0 4.4.7.3).")
    s.add(frame_in(tc_frame(c, 5, 0, ramp(1, 9))))
    s.counter("UnknownChannel")
    out.append(s)
    return out


# ---- PacketExtractor ----

EXT_ZONE = 24


def ext_cfg(proto, **kw):
    cfg = {"protocol": proto, "valid_pvns": [0, 7], "max_packet_len": 40,
           "deliver_incomplete": False, "deliver_idle": False}
    cfg.update(kw)
    return cfg


def ext_field(proto, zone, fhp="start0", gap=False):
    """The input for one data field. `fhp` is an offset, "none" (no packet starts), or "idle"
    (only idle data); the encoding is that of the protocol (04 section 2.1 table)."""
    zone = bytes(zone)
    assert len(zone) == EXT_ZONE, len(zone)
    off = fhp
    if proto == "tm":
        raw = {"none": 0x7FF, "idle": 0x7FE}.get(fhp, fhp)
        return {"data_field": {"data_hex": hx(zone), "fhp": raw, "gap": gap}}
    if proto == "aos":
        raw = {"none": 0xFFFF, "idle": 0xFFFE}.get(fhp, fhp)
        return {"data_field": {"data_hex": hx(mpdu(raw, EXT_ZONE, zone)), "gap": gap}}
    if fhp == "idle":
        raw, rule, upid = 0, 1, UPID_IDLE      # an idle TFDZ: rule 001, UPID Only Idle Data
        pointer = EXT_ZONE - 1
        return {"data_field": {"data_hex": hx(tfdf(rule, upid, pointer, EXT_ZONE, zone)),
                               "gap": gap}}
    raw = 0xFFFF if fhp == "none" else off
    return {"data_field": {"data_hex": hx(tfdf(0, UPID_PACKETS, raw, EXT_ZONE, zone)), "gap": gap}}


def split_into(stream, n=EXT_ZONE):
    return [stream[i:i + n] for i in range(0, len(stream), n)]


def ext_scenarios(proto):
    c = ext_cfg(proto)
    out = []
    F = lambda zone, fhp, gap=False: ext_field(proto, zone, fhp, gap)
    tag = "link-recv-extract-%s-" % proto
    name = {"tm": "TM data field, FHP 0x7FF and 0x7FE", "aos": "AOS M_PDU, FHP 0xFFFF and 0xFFFE",
            "uslp": "USLP rule 000, FHP 0xFFFF, UPID Only Idle Data"}[proto]

    p10 = spp_total(10, 1, 0x10)
    p14 = spp_total(14, 2, 0x20)

    s = Scn(tag + "whole-packets", ["LNK-060"],
            "Two Space Packets fill a field exactly from FHP 0: each is delivered once, by its "
            "PVN (%s)." % name)
    s.add(F(p10 + p14, 0))
    out.append(s)

    # A packet spanning fields, then another packet.
    big = spp_total(30, 3, 0x30)
    nxt = spp_total(18, 4, 0x40)
    s = Scn(tag + "packet-spans-fields", ["LNK-060"],
            "A 30-octet packet starts at FHP 0 and ends 6 octets into the next field, whose FHP "
            "is 6 (the residue does not count as a start); the packet is delivered when it "
            "completes, then the packet that starts at the FHP.")
    s.add(F(big[:24], 0))
    s.add(F(big[24:] + nxt, 6))
    out.append(s)

    # A header split across fields: 3 of the 6 header octets at the end of the first field.
    a = spp_total(21, 5, 0x50)
    b = spp_total(12, 6, 0x60)
    c3 = spp_total(15, 7, 0x70)
    s = Scn(tag + "header-split", ["LNK-060"],
            "The primary header of a packet is split across two fields (3 octets in each): the "
            "length is known only when the second field arrives, and the packet is delivered "
            "there, followed by the packet at the FHP.")
    s.add(F(a + b[:3], 0))
    s.add(F(b[3:] + c3, 9))
    out.append(s)

    # A packet longer than two fields: the FHP none value in the middle.
    huge = spp_total(60, 8, 0x80)
    p12 = spp_total(12, 9, 0x90)
    s = Scn(tag + "no-packet-starts", ["LNK-060", "LNK-061"],
            "A 60-octet packet spans three fields; the middle field has the FHP value of "
            "'no packet starts' and is taken whole as the continuation; the packet is delivered "
            "when it ends, then the packet that starts at the FHP of the last field.",
            config=ext_cfg(proto, max_packet_len=100))
    s.add(F(huge[:24], 0))
    s.add(F(huge[24:48], "none"))
    s.add(F(huge[48:] + p12, 12))
    out.append(s)

    # A packet ends exactly at the field end; the next field's FHP is 0 (also the all-ones case).
    e1 = spp_total(24, 10, 0xA0)
    pk48 = spp_total(48, 12, 0xC0)
    s = Scn(tag + "packet-ends-at-field-end", ["LNK-060", "LNK-061"],
            "A packet ends at the last octet of a field and the next field starts a packet at "
            "FHP 0. A 48-octet packet then spans two fields: the second has the 'no packet "
            "starts' value, ends the packet exactly at its last octet, and the extractor takes "
            "the end from the length field (OI-69).",
            config=ext_cfg(proto, max_packet_len=100))
    s.add(F(e1, 0))
    s.add(F(pk48[:24], 0))
    s.add(F(pk48[24:], "none"))
    s.add(F(p10 + p14, 0))
    out.append(s)

    # Encapsulation packets and a mix.
    e_a = epp(0b111, ramp(0x11, 6), 1)              # 8 octets, EPI 111 mission-specific
    e_b = epp(0b111, ramp(0x22, 4), 2)              # 8 octets, 4-octet header
    s_c = spp_total(8, 13, 0x33)
    s = Scn(tag + "space-and-encapsulation", ["LNK-060"],
            "Space Packets and Encapsulation Packets (one-octet and two-octet length fields) in "
            "one field are delimited by their own length fields (133.0 4.1.3.5, 133.1 4.1.2.8) "
            "and delivered by the PVN in their header (0 and 7).")
    s.add(F(e_a + e_b + s_c, 0))
    out.append(s)

    # FHP wins: continuation disagrees.
    q1 = spp_total(18, 15, 0x55)
    q2 = spp_total(20, 16, 0x66)
    q3 = spp_total(10, 17, 0x77)
    s = Scn(tag + "fhp-wins-over-length", ["LNK-060", "LNK-061"],
            "The packet in progress needs 14 more octets but the FHP of the next field is 8: "
            "the FHP is correct (132.0 4.3.2.4); the partial packet is discarded with its 6 "
            "octets, an extraction error LengthInconsistent is raised before the FHP, and the "
            "extraction continues from the FHP.")
    s.add(F(q1 + q2[:6], 0))
    s.add(F(bytes(8) + q3 + q1[:6], 8))
    s.counter("LengthInconsistent")
    out.append(s)

    s = Scn(tag + "fhp-none-without-packet-in-progress", ["LNK-060", "LNK-061"],
            "The previous packet ended exactly at the end of its field, so the next octet must "
            "start a packet, but the field says that no packet starts: the length fields "
            "disagree with the FHP, which is correct. LengthInconsistent is raised, the field "
            "is skipped, and the extractor resynchronises at the next FHP.")
    s.add(F(e1, 0))
    s.add(F(ramp(0xE0, 24), "none"))
    s.add(F(bytes(5) + q3 + q1[:9], 5))
    out.append(s)

    # Gap: discard.
    r1 = spp_total(20, 18, 0x10)
    r2 = spp_total(14, 19, 0x20)
    s = Scn(tag + "gap-discard", ["LNK-061", "LNK-051"],
            "After a frame gap the partial packet (10 octets) is discarded with a Discarded "
            "notification; the octets before the FHP are skipped and the extraction continues at "
            "the FHP of the field that carries the gap flag.")
    s.add(F(p10 + r1[:14], 0))
    s.add(F(ramp(0xC0, 6) + r2 + p10[:4], 6, gap=True))
    out.append(s)

    s = Scn(tag + "gap-deliver-incomplete", ["LNK-061", "LNK-051"],
            "With 'deliver incomplete packets' configured, the partial packet is delivered "
            "flagged Incomplete (the Packet Quality Indicator) instead of discarded.",
            config=ext_cfg(proto, deliver_incomplete=True))
    s.add(F(p10 + r1[:14], 0))
    s.add(F(ramp(0xC0, 6) + r2 + p10[:4], 6, gap=True))
    out.append(s)

    s = Scn(tag + "gap-no-start-then-resync", ["LNK-061", "LNK-051"],
            "A gap on a field in which no packet starts: the partial packet is discarded, the "
            "whole field is skipped, and the extractor stays out of synchronisation until a "
            "field has an FHP; the field after it is skipped as well, then a field with an FHP "
            "resumes.")
    s.add(F(p10 + r1[:14], 0))
    s.add(F(ramp(0x90, 24), "none", gap=True))
    s.add(F(ramp(0x91, 24), "none"))
    s.add(F(ramp(0x92, 7) + p10 + r2[:7], 7))
    out.append(s)

    s = Scn(tag + "start-up", ["LNK-061"],
            "At start-up the extractor ignores the octets before the first FHP: a field with "
            "no packet start is skipped, the octets before the FHP of the next one are "
            "skipped, and the packets from the FHP are delivered; nothing is reported.")
    s.add(F(ramp(0x01, 24), "none"))
    s.add(F(ramp(0x02, 5) + p10 + p10[:9], 5))
    out.append(s)

    # Too long.
    tl = spp_total(90, 20, 0x30)
    s = Scn(tag + "packet-too-long", ["LNK-062"],
            "A packet whose length (90) exceeds the Maximum Packet Length (40) raises the "
            "extraction error PacketTooLong as soon as its length field is known; the rest of "
            "the field is discarded, and the extraction resumes at the next FHP (OI-72).")
    s.add(F(tl[:24], 0))
    s.add(F(ramp(0xD0, 8) + p14 + p10[:2], 8))
    s.counter("PacketTooLong")
    out.append(s)

    s = Scn(tag + "invalid-pvn", ["LNK-062"],
            "A packet whose PVN is not in the valid set (PVN 1, here the octet 0x20) raises "
            "the extraction error InvalidPvn; the rest of the field is discarded, and the "
            "extraction resumes at the next FHP (OI-72).")
    s.add(F(p10 + bytes([0x20]) + ramp(0x11, 13), 0))
    s.add(F(ramp(0xD0, 3) + p14 + p10[:7], 3))
    s.counter("InvalidPvn")
    out.append(s)

    s = Scn(tag + "pvn-not-in-set", ["LNK-062"],
            "A VC configured for Space Packets only: an Encapsulation Packet (PVN 7) is not "
            "in the valid set, whichever kind it is, unless it is an idle packet on USLP "
            "(LNK-063).",
            config=ext_cfg(proto, valid_pvns=[0]))
    s.add(F(p10 + e_a + fit(b"", 6), 0))
    s.add(F(ramp(0xD0, 2) + p10 + p14[:12], 2))
    out.append(s)
    # (the 6 trailing octets above are only filler after the error and are never examined)

    # Idle packets.
    i1, i2, i3 = spp_idle(8), epp_idle(1), epp_idle(5)
    s = Scn(tag + "idle-dropped", ["LNK-063"],
            "Idle packets (a Space Packet with APID 2047, Encapsulation Idle Packets of one "
            "and of five octets) are dropped by default and counted; the other packets are "
            "delivered (132.0 4.3.2.5 note 1).")
    s.add(F(p10 + i1 + i2 + i3, 0))
    s.counter("idle_packets")
    out.append(s)

    s = Scn(tag + "idle-delivered", ["LNK-063"],
            "With delivery of idle packets enabled for diagnostics they are delivered like other "
            "packets, in order.", config=ext_cfg(proto, deliver_idle=True))
    s.add(F(p10 + i1 + i2 + i3, 0))
    out.append(s)

    s = Scn(tag + "idle-encapsulation-valid-set", ["LNK-063"],
            "A VC whose valid-PVN set holds only Space Packets receives an Encapsulation Idle "
            "Packet in its field. On TM and AOS it is a packet of a PVN that is not valid. On "
            "USLP rule 000 the extractor delimits an Encapsulation Idle Packet whatever the set "
            "(the sender fills with it, 133.1 4.1.4.3.4): it is dropped and counted.",
            config=ext_cfg(proto, valid_pvns=[0]))
    s.add(F(p10 + epp_idle(8) + p10[:6], 0))
    s.counter("idle_packets")
    s.counter("InvalidPvn")
    out.append(s)

    # Only idle data.
    s = Scn(tag + "only-idle-data-keeps-packet", ["LNK-064"],
            "A field with the FHP of 'only idle data' (%s) is ignored and the packet being "
            "assembled is kept: the packet that started in the first field is delivered when "
            "the third field completes it." % name)
    s.add(F(p10 + r1[:14], 0))
    s.add(F(ramp(0x00, 24), "idle"))
    s.add(F(r1[14:] + p14 + p10[:4], 6))
    out.append(s)

    # Reconfigure.
    s = Scn(tag + "reconfigure-discards", ["LNK-003", "LNK-061"],
            "Reconfigure discards the partial packet (one Discarded notification with its 14 "
            "octets) and the extractor restarts: the next field is skipped up to its FHP; a second "
            "Reconfigure discards the 10 octets of the packet then in progress (04 LNK-003).")
    s.add(F(p10 + r1[:14], 0))
    s.add({"reconfigure": {}})
    s.add(F(ramp(0x20, 4) + p10 + p14[:10], 4))
    s.add({"reconfigure": {}})
    out.append(s)
    return out


# ---- MapReassembler ----

TC_MAP_CFG = {"protocol": "tc", "vc": 1,
              "maps": [{"map": 0, "service": "packets"}, {"map": 1, "service": "sdu"},
                       {"map": 2, "service": "sdu"}]}


def tc_fdu(flags, mp, body):
    """Segment Header (sequence flags, MAP ID) and the user data (232.0 4.1.3.2.2)."""
    return bytes([(flags << 6) | mp]) + bytes(body)


def fdu_in(vc, hexdata, mp=None, gap=None):
    d = {"vc": vc, "data_hex": hx(hexdata)}
    if mp is not None:
        d["map"] = mp
    if gap is not None:
        d["gap"] = gap
    return {"fdu": d}


def re_tc_scenarios():
    c = TC_MAP_CFG
    out = []
    F = lambda flags, mp, body: fdu_in(1, tc_fdu(flags, mp, body))
    FIRST, CONT, LAST, WHOLE = 1, 0, 2, 3
    sdu = ramp(0x10, 21)

    s = Scn("link-recv-reassemble-tc-segments", ["LNK-070"],
            "A MAP_SDU in three segments (flags 01, 00, 10) is rebuilt in order and delivered when "
            "the last segment arrives; a whole SDU (flags 11) is delivered at once "
            "(232.0 4.1.3.2.2.2, Table 4-2, 4.4.2.3).")
    s.add(F(FIRST, 1, sdu[:8]))
    s.add(F(CONT, 1, sdu[8:16]))
    s.add(F(LAST, 1, sdu[16:]))
    s.add(F(WHOLE, 1, ramp(0x50, 5)))
    out.append(s)

    s = Scn("link-recv-reassemble-tc-maps-interleaved", ["LNK-070"],
            "The Blue Book lets FDUs of different MAPs of a TC VC be multiplexed after any FDU: "
            "each MAP is reassembled separately (232.0 4.3.3.1-2, LNK-041).")
    s.add(F(FIRST, 1, sdu[:8]))
    s.add(F(FIRST, 2, ramp(0x60, 6)))
    s.add(F(CONT, 1, sdu[8:16]))
    s.add(F(LAST, 2, ramp(0x70, 4)))
    s.add(F(LAST, 1, sdu[16:]))
    out.append(s)

    s = Scn("link-recv-reassemble-tc-sequence-violations", ["LNK-070"],
            "A continuation, or a last segment, without a first segment raises SequenceViolation "
            "and delivers nothing; a first segment while an SDU is in progress discards the "
            "partial SDU, raises SequenceViolation, and begins the new SDU (OI-72); the counter "
            "counts the events.")
    s.add(F(CONT, 1, ramp(1, 4)))
    s.add(F(LAST, 1, ramp(2, 4)))
    s.add(F(FIRST, 1, ramp(3, 4)))
    s.add(F(FIRST, 1, ramp(4, 4)))
    s.add(F(LAST, 1, ramp(5, 4)))
    s.counter("SequenceViolation")
    out.append(s)

    s = Scn("link-recv-reassemble-tc-unknown-map", ["LNK-070"],
            "A FDU for a MAP ID that is not configured is discarded with the event UnknownMap "
            "(232.0 4.4.3.3); an SDU in progress on a known MAP is not disturbed.")
    s.add(F(FIRST, 1, sdu[:8]))
    s.add(F(WHOLE, 9, ramp(1, 4)))
    s.add(F(LAST, 1, sdu[8:]))
    s.counter("UnknownMap")
    out.append(s)

    p1, p2, p3 = spp_total(9, 1, 1), epp(0b111, ramp(7, 5), 1), spp_total(12, 2, 2)
    long_pkt = spp_total(30, 3, 3)
    s = Scn("link-recv-reassemble-tc-blocked-packets", ["LNK-071", "LNK-070"],
            "A FDU of a packet MAP with flags 11 holds several packets and is split into "
            "individual packets by the length field of each, Space and Encapsulation Packets "
            "mixed (232.0 4.4.1.3); a packet cut into three segments is delivered once, whole.")
    s.add(F(WHOLE, 0, p1 + p2 + p3))
    s.add(F(FIRST, 0, long_pkt[:10]))
    s.add(F(CONT, 0, long_pkt[10:20]))
    s.add(F(LAST, 0, long_pkt[20:]))
    out.append(s)

    s = Scn("link-recv-reassemble-tc-reconfigure", ["LNK-003", "LNK-070"],
            "Reconfigure discards the SDUs in progress: one Discarded notification with the "
            "total number of octets; a later last segment is then a sequence violation.")
    s.add(F(FIRST, 1, sdu[:8]))
    s.add(F(FIRST, 2, ramp(1, 5)))
    s.add({"reconfigure": {}})
    s.add(F(LAST, 1, sdu[8:]))
    s.add({"reconfigure": {}})
    out.append(s)
    return out


US_MAP_CFG = {"protocol": "uslp",
              "maps": [{"vc": 1, "map": 0, "service": "sdu"},
                       {"vc": 1, "map": 1, "service": "octet_stream"},
                       {"vc": 2, "map": 0, "service": "packets"},
                       {"vc": 2, "map": 3, "service": "sdu"}]}
UZ = 20       # fixed-length TFDZ of the reassembler scenarios


def re_uslp_scenarios():
    out = []

    def fx(rule, ptr, payload):
        return tfdf(rule, UPID_MISSION_1, ptr, UZ, payload)

    def var(rule, payload, upid=UPID_PACKETS):
        return tfdf_var(rule, upid, payload)

    I = lambda vc, mp, data, gap=False: fdu_in(vc, data, mp, gap)
    sdu = ramp(0x10, 45)

    s = Scn("link-recv-reassemble-uslp-fixed-sdu", ["LNK-072", "LNK-070"],
            "Rules 001 and 010 of fixed-length TFDZs: the Last Valid Octet Pointer delimits the "
            "SDU and the octets after it are idle and discarded (732.1 4.1.4.2.4.5-6, "
            "4.3.3.2.4). Case 1: an SDU that ends inside its first TFDZ. Case 2: an SDU that "
            "fills its TFDZ to the last octet (pointer 19). Case 3: an SDU over three TFDZs, "
            "pointer all ones while it continues.")
    s.add(I(1, 0, fx(1, 6, ramp(1, 7))))
    s.add(I(1, 0, fx(1, UZ - 1, ramp(2, UZ))))
    s.add(I(1, 0, fx(1, 0xFFFF, sdu[:20])))
    s.add(I(1, 0, fx(2, 0xFFFF, sdu[20:40])))
    s.add(I(1, 0, fx(2, 4, sdu[40:] + bytes([0xEE]) * 15)))
    out.append(s)

    s = Scn("link-recv-reassemble-uslp-fixed-violations", ["LNK-072", "LNK-070"],
            "A rule 010 TFDZ without a preceding rule 001 is a sequence violation, and so is a "
            "rule 001 while an SDU is in progress (the partial SDU is discarded and the new "
            "one begins, OI-72).")
    s.add(I(1, 0, fx(2, 0xFFFF, ramp(1, 20))))
    s.add(I(1, 0, fx(1, 0xFFFF, ramp(2, 20))))
    s.add(I(1, 0, fx(1, 3, ramp(3, 20))))
    s.add(I(1, 0, fx(2, 5, ramp(4, 20))))
    s.counter("SequenceViolation")
    out.append(s)

    s = Scn("link-recv-reassemble-uslp-gap", ["LNK-070", "LNK-051"],
            "A gap in the VCF Count while an SDU is in progress discards it with the event "
            "GapDiscard (the frame carries gap = true); if the same TFDZ is a continuation "
            "there is nothing to continue, which is also a SequenceViolation (two events); a "
            "gap with no SDU in progress raises no GapDiscard.")
    s.add(I(1, 0, fx(1, 0xFFFF, ramp(1, 20))))
    s.add(I(1, 0, fx(2, 4, ramp(2, 20)), True))
    s.add(I(1, 0, fx(1, 0xFFFF, ramp(3, 20))))
    s.add(I(1, 0, fx(1, 6, ramp(4, 20)), True))
    s.add(I(1, 0, fx(1, 2, ramp(5, 20)), True))
    s.add(I(1, 0, fx(2, 2, ramp(6, 20)), True))
    s.counter("GapDiscard")
    out.append(s)

    s = Scn("link-recv-reassemble-uslp-unknown-map", ["LNK-070"],
            "A TFDF for a MAP ID that is not configured on the VC is discarded with UnknownMap "
            "(732.1 4.3.3.2.6).")
    s.add(I(1, 9, fx(1, 3, ramp(1, 20))))
    s.add(I(1, 0, fx(1, 3, ramp(2, 20))))
    out.append(s)

    s = Scn("link-recv-reassemble-uslp-octet-stream", ["LNK-072"],
            "Rule 011 (octet stream) is delivered as it arrives, one delivery per TFDZ "
            "(732.1 4.3.4.2-3).")
    s.add(I(1, 1, var(3, ramp(1, 9), UPID_OCTET_STREAM)))
    s.add(I(1, 1, var(3, ramp(20, 14), UPID_OCTET_STREAM)))
    out.append(s)

    s = Scn("link-recv-reassemble-uslp-variable-sdu", ["LNK-070"],
            "Variable-length TFDZs of a MAPA_SDU: rules 100, 101, 110 rebuild one SDU; rule 111 is "
            "one complete SDU (732.1 4.3.3.3.3-4); a continuation or a last segment without "
            "a start is a sequence violation, and a start in progress is replaced (OI-72).")
    s.add(I(1, 0, var(4, sdu[:15], UPID_MISSION_1)))
    s.add(I(1, 0, var(5, sdu[15:30], UPID_MISSION_1)))
    s.add(I(1, 0, var(6, sdu[30:], UPID_MISSION_1)))
    s.add(I(1, 0, var(7, ramp(0x70, 11), UPID_MISSION_1)))
    s.add(I(1, 0, var(5, ramp(1, 5), UPID_MISSION_1)))
    s.add(I(1, 0, var(6, ramp(2, 5), UPID_MISSION_1)))
    s.add(I(1, 0, var(4, ramp(3, 5), UPID_MISSION_1)))
    s.add(I(1, 0, var(4, ramp(4, 5), UPID_MISSION_1)))
    s.add(I(1, 0, var(6, ramp(5, 5), UPID_MISSION_1)))
    out.append(s)

    p1, p2, p3 = spp_total(9, 1, 1), epp(0b111, ramp(7, 5), 1), spp_total(12, 2, 2)
    big = spp_total(33, 3, 3)
    s = Scn("link-recv-reassemble-uslp-variable-packets", ["LNK-071", "LNK-070"],
            "Packet MAP, variable-length TFDZs: rule 111 holds one or more complete packets and "
            "is split into individual packets by the length field of each (732.1 4.3.2.3.3); a "
            "packet in segments (100, 101, 110) is delivered once, whole.")
    s.add(I(2, 0, var(7, p1 + p2 + p3)))
    s.add(I(2, 0, var(4, big[:11])))
    s.add(I(2, 0, var(5, big[11:22])))
    s.add(I(2, 0, var(6, big[22:])))
    out.append(s)

    s = Scn("link-recv-reassemble-uslp-variable-gap", ["LNK-070", "LNK-051"],
            "A gap while a variable-length segmented SDU is in progress discards it (GapDiscard); "
            "the continuation that follows is a SequenceViolation.")
    s.add(I(2, 3, var(4, ramp(1, 8), UPID_MISSION_1)))
    s.add(I(2, 3, var(5, ramp(2, 8), UPID_MISSION_1), True))
    s.add(I(2, 3, var(7, ramp(3, 6), UPID_MISSION_1)))
    out.append(s)

    s = Scn("link-recv-reassemble-uslp-reconfigure", ["LNK-003", "LNK-070"],
            "Reconfigure discards the SDU in progress with one Discarded notification and its "
            "octets.")
    s.add(I(1, 0, fx(1, 0xFFFF, ramp(1, 20))))
    s.add({"reconfigure": {}})
    s.add(I(1, 0, fx(2, 4, ramp(2, 20))))
    out.append(s)
    return out


# --------------------------------------------------------------------------------------------
# The files
# --------------------------------------------------------------------------------------------

NOTE = ("Hand-derived from %s. Model test-vectors/models/link_recv.py, written blind (TV-023) by "
        "a Claude agent from the Blue Books and docs/ only, without reading any implementation; "
        "for format only it read the COP-1 model and scenario files (cop1.py, the cop1 directory). The input "
        "scripts were chosen by hand; the expected outputs are the model's. Where the documents "
        "are silent the model refuses the case (see the README of test-vectors/link/recv/), so "
        "no scenario pins a behavior that 04 or the Blue Books leave open. The file is written "
        "by the model (--write) and never edited by hand.")

FILES = [
    ("frame_receiver_tm.json", "frame-receiver", TM_CFG, fr_tm_scenarios,
     "132.0-B-3 4.1.2, 4.1.3, 4.1.4.6, 4.3.3 to 4.3.7 and 04-engines LNK-050 to LNK-053, "
     "LNK-077, OI-68"),
    ("frame_receiver_aos.json", "frame-receiver", AOS_CFG, fr_aos_scenarios,
     "732.0-B-5 4.1.2 to 4.1.5, 4.3.3 to 4.3.7 and 04-engines LNK-050 to LNK-054, LNK-077, "
     "OI-68, OI-70"),
    ("frame_receiver_uslp.json", "frame-receiver", USLP_CFG, fr_uslp_scenarios,
     "732.1-B-3 4.1.2, 4.1.3, 4.1.4.1, 4.1.5, 4.3.7 to 4.3.11 and 04-engines LNK-050 to LNK-053, "
     "LNK-079, OI-68"),
    ("frame_receiver_tc.json", "frame-receiver", TC_CFG, fr_tc_scenarios,
     "232.0-B-4 4.1.2, 4.4.7 and 04-engines LNK-050"),
    ("packet_extractor_tm.json", "packet-extractor", ext_cfg("tm"),
     lambda: ext_scenarios("tm"),
     "132.0-B-3 4.1.2.7.6, 4.3.2, 133.0-B-2 4.1.3, 133.1-B-3 4.1.2 and 04-engines LNK-060 to "
     "LNK-064, OI-69, OI-72"),
    ("packet_extractor_aos.json", "packet-extractor", ext_cfg("aos"),
     lambda: ext_scenarios("aos"),
     "732.0-B-5 4.1.4.2, 4.3.2, 133.0-B-2 4.1.3, 133.1-B-3 4.1.2 and 04-engines LNK-060 to "
     "LNK-064, OI-69, OI-72"),
    ("packet_extractor_uslp.json", "packet-extractor", ext_cfg("uslp"),
     lambda: ext_scenarios("uslp"),
     "732.1-B-3 4.1.4.2, 4.3.2.2, 133.0-B-2 4.1.3, 133.1-B-3 4.1.2 and 04-engines LNK-060 to "
     "LNK-064, OI-69, OI-72"),
    ("map_reassembler_tc.json", "map-reassembler", TC_MAP_CFG, re_tc_scenarios,
     "232.0-B-4 4.1.3.2, 4.4.1 to 4.4.4 and 04-engines LNK-070, LNK-071, OI-72"),
    ("map_reassembler_uslp.json", "map-reassembler", US_MAP_CFG, re_uslp_scenarios,
     "732.1-B-3 4.1.4.2, 4.3.2 to 4.3.5 and 04-engines LNK-070 to LNK-072, OI-72"),
]


def build_file(engine, base_cfg, scenarios, source):
    scs = []
    for s in scenarios:
        cfg = s.config if s.config is not None else base_cfg
        try:
            steps = replay(engine, cfg, s.inputs)
        except (Unspecified, AssertionError) as e:
            raise SystemExit("%s: %s: %r" % (s.sid, type(e).__name__, e))
        d = {"id": s.sid, "description": s.description, "requirements": s.reqs}
        if s.config is not None:
            d["config"] = s.config
        d["steps"] = steps
        scs.append(d)
    return {"kind": "engine-scenario", "engine": engine, "provenance": "hand-derived",
            "provenance_note": NOTE % source, "config": base_cfg, "scenarios": scs}


def all_files():
    files = {}
    for name, engine, cfg, fn, source in FILES:
        files[name] = build_file(engine, cfg, fn(), source)
    return files


def root_dir():
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.normpath(os.path.join(here, "..", ".."))


def dump(obj, level=0):
    """Deterministic text: a file or a scenario is expanded, one key per line; a step and every
    other small object is one line; the steps of a list go one to a line."""
    pad = "  " * level
    inner = "  " * (level + 1)
    if isinstance(obj, dict) and ("kind" in obj and "engine" in obj or "id" in obj):
        items = ['%s%s: %s' % (inner, json.dumps(k), dump(v, level + 1)) for k, v in obj.items()]
        return "{\n" + ",\n".join(items) + "\n" + pad + "}"
    if isinstance(obj, list) and obj and all(isinstance(x, dict) for x in obj):
        return "[\n" + ",\n".join(inner + dump(x, level + 1) for x in obj) + "\n" + pad + "]"
    return json.dumps(obj, separators=(", ", ": "))


def render(doc):
    return dump(doc) + "\n"


def write_all(outdir=None):
    base = outdir or os.path.join(root_dir(), "test-vectors", "link", "recv")
    os.makedirs(base, exist_ok=True)
    files = all_files()
    for name in sorted(os.listdir(base)):
        if name.endswith(".json") and name not in files:
            os.remove(os.path.join(base, name))
    for name, doc in files.items():
        with open(os.path.join(base, name), "w") as f:
            f.write(render(doc))
    n = sum(len(d["scenarios"]) for d in files.values())
    print("wrote %d file(s), %d scenario(s)" % (len(files), n))
    return 0


# --------------------------------------------------------------------------------------------
# --check
# --------------------------------------------------------------------------------------------

def check_file(path, generated):
    problems = []
    with open(path) as f:
        doc = json.load(f)
    engine = doc["engine"]
    seen = set()
    for sc in doc["scenarios"]:
        sid = sc["id"]
        if sid in seen:
            problems.append("%s: scenario id twice" % sid)
        seen.add(sid)
        if sc.get("superseded"):
            continue
        cfg = sc.get("config", doc["config"])
        inputs = [(st["at_us"], st["input"]) for st in sc["steps"]]
        try:
            steps = replay(engine, cfg, inputs)
        except Unspecified as e:
            problems.append("%s: the model refuses the case: %s" % (sid, e))
            continue
        for i, (want, got) in enumerate(zip(steps, sc["steps"])):
            if want != got:
                problems.append("%s: step %d: model says %s, file says %s" %
                                (sid, i, json.dumps(want), json.dumps(got)))
    name = os.path.basename(path)
    if name not in generated:
        problems.append("%s: not a file the generator writes" % name)
    elif generated[name] != doc:
        problems.append("%s: differs from what the generator writes (run --write)" % name)
    return problems


def selftest():
    assert crc16(b"123456789") == 0x29B1                                  # core/crc16.json
    assert oid_octets(20).hex().upper() == "FFFFFFFF6DB6D861451F11F19716723CBE7E00B1"  # CMN-020
    assert len(spp_total(10)) == 10 and len(epp_idle(5)) == 5 and len(epp_idle(1)) == 1


def check(paths):
    selftest()
    problems = []
    generated = all_files()
    for p in paths:
        problems.extend(check_file(p, generated))
    for p in problems:
        print("error:", p)
    print("link_recv model: %d file(s), %d problem(s)" % (len(paths), len(problems)))
    return 1 if problems else 0


def main(argv):
    if len(argv) >= 2 and argv[1] == "--check":
        return check(argv[2:])
    if len(argv) in (2, 3) and argv[1] == "--write":
        return write_all(argv[2] if len(argv) == 3 else None)
    print("usage: link_recv.py --check FILE... | --write [DIR]")
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
