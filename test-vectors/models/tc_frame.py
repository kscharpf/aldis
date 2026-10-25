"""Independent model of the TC Transfer Frame, the Communications Link Control Word, Frame
Delimiting and Fill Removal, and the Frame Validation Checks, for the vector files of
test-vectors/tc/ (TC-001...TC-009, TC-020...TC-023, TC-030, TC-031; M2 WP-3).

Sources: CCSDS 232.0-B-4 with Technical Corrigendum 1: the frame of §4.1.1-4.1.4 (Primary
Header bit positions, Table 4-1 for the Bypass and Control Command Flags, the Segment Header
and Table 4-2, the Control Commands of §4.1.3.3, the FECF of §4.1.4.2), the CLCW of §4.2.1,
the Frame Delimiting and Fill Removal Procedure of §4.4.9.2, the Frame Validation Checks of
§4.4.9.3, the VC and MAP demultiplexing rules of §4.4.7.3 and §4.4.3.3, the managed parameters
of §5 and §6.6, and the SDLS field order of §6.3. Parameters and code only, no Blue Book text.

What the Blue Book leaves to the implementation is taken from docs/standards/tc-frame.md: the
order in which the decoder applies its checks and the reason code of each failure (§4 of that
document), what a "valid header" is for the delimiter (TFVN 00 and a Frame Length of at least
six octets), and the managed parameters the codec keeps.

The model was written from those clauses, not from the Rust code (crates/aldis-tc), and must
stay independent of it: never port code from it, and never fix the model to agree with it.
Arithmetic is on Python integers: the header is assembled and read as one 40-bit number, the
CRC is computed bit by bit from the generator polynomial, and the CLCW as one 32-bit number.

Usage, from the repository root:

    python3 -I test-vectors/models/tc_frame.py --check test-vectors/tc/*.json
    python3 -I test-vectors/models/tc_frame.py --write test-vectors/tc

--check recomputes the expected outcome of every live case from its operation, configuration,
parameters, and input, compares the cases the generator writes with the files', and checks
the field codes against test-vectors/core/fields.json; it exits 1 on any difference. --write
regenerates the files (cases and provenance note).
"""
import json
import os
import sys

# Field codes of docs/03-api-model.md §9.1.
F = {
    "spacecraft_id": 1, "vc_id": 2, "map_id": 3, "frame_data_field": 7, "user_len": 54,
    "frame_version": 67, "frame_len": 68, "control_command_flag": 77, "tc_header_spare": 78,
    "max_frame_len": 79, "segment_header": 80, "control_command": 81,
    "control_word_type": 82, "clcw_version": 83, "clcw_status": 84, "cop_in_effect": 85,
    "clcw_spare_14_15": 86, "clcw_spare_23": 87, "farm_b_counter": 88,
}

# Validation reasons, docs/standards/tc-frame.md §4 (TC-021, TC-022, D14), in check order.
REASONS = {
    "UnexpectedVersion": 1, "UnexpectedSpacecraft": 2, "ReservedFlags": 3, "ReservedSpare": 4,
    "InvalidVcid": 5, "FrameTooLong": 6, "FrameTooShort": 7, "LengthMismatch": 8,
    "FecfMismatch": 9, "InvalidMapId": 10, "ReservedControlCommand": 11,
}

HEADER = 5        # §4.1.2: five octets
MAX_FRAME = 1024  # §4.1.2.7 note 2: ten-bit length count
SCID = 0x155      # the fictional SCID of the reference scenario (APP-026, OI-26)

# Table 4-1: (Bypass Flag, Control Command Flag) -> frame type; (0, 1) is reserved.
TYPES = {(0, 0): "AD", (1, 0): "BD", (1, 1): "BC"}
FLAGS = {v: k for k, v in TYPES.items()}


class Fail(Exception):
    """An error outcome: kind (catalogue name), field code or None, validation reason."""

    def __init__(self, kind, field=None, reason=None):
        super().__init__(kind)
        self.kind, self.field, self.reason = kind, field, reason


def hx(b):
    return bytes(b).hex().upper()


# ---- FECF, §4.1.4.2: G(X) = X^16 + X^12 + X^5 + 1, register preset to all ones, the first
# transmitted bit the most significant; computed one bit at a time.

def crc(data):
    reg = 0xFFFF
    for octet in data:
        for i in range(7, -1, -1):
            top = (reg >> 15) & 1
            reg = (reg << 1) & 0xFFFF
            if top ^ ((octet >> i) & 1):
                reg ^= 0x1021
    return reg


# ---- managed parameters, §5 and §6.6, as docs/standards/tc-frame.md §2 keeps them

def config(cfg):
    if cfg is None:
        raise ValueError("a frame case needs a config")
    fecf = cfg["fecf"]
    pc_max = cfg["max_frame_len"]
    if not HEADER + 1 + (2 if fecf else 0) <= pc_max <= MAX_FRAME:
        raise Fail("InvalidConfig", F["max_frame_len"])
    vcs = []
    for v in cfg["vcs"]:
        if not HEADER + 1 <= v["max_frame_len"] <= MAX_FRAME:
            raise Fail("InvalidConfig", F["max_frame_len"])
        vcs.append({"scid": v["scid"], "vcid": v["vcid"], "max": v["max_frame_len"],
                    "seg": v["segment_header"], "sh": v.get("security_header_len", 0),
                    "st": v.get("security_trailer_len", 0)})
    maps = [{"scid": m["scid"], "vcid": m["vcid"], "map": m["map_id"],
             "sh": m.get("security_header_len", 0), "st": m.get("security_trailer_len", 0)}
            for m in cfg.get("maps", [])]
    trailer = 2 if fecf else 0
    for i, v in enumerate(vcs):
        if any((w["scid"], w["vcid"]) == (v["scid"], v["vcid"]) for w in vcs[:i]):
            raise Fail("InvalidConfig", F["vc_id"])
        limit = min(pc_max, v["max"])
        if v["seg"]:
            smallest = HEADER + 1 + 1 + trailer
        else:
            smallest = HEADER + v["sh"] + 1 + v["st"] + trailer
        if smallest > limit:
            raise Fail("InvalidConfig", F["frame_data_field"])
    for i, m in enumerate(maps):
        vc = vc_of(vcs, m["scid"], m["vcid"])
        if vc is None or not vc["seg"]:
            raise Fail("InvalidConfig", F["map_id"])
        if any((n["scid"], n["vcid"], n["map"]) == (m["scid"], m["vcid"], m["map"])
               for n in maps[:i]):
            raise Fail("InvalidConfig", F["map_id"])
        if HEADER + 1 + m["sh"] + 1 + m["st"] + trailer > min(pc_max, vc["max"]):
            raise Fail("InvalidConfig", F["frame_data_field"])
    return {"fecf": fecf, "max": pc_max, "vcs": vcs, "maps": maps}


def vc_of(vcs, scid, vcid):
    return next((v for v in vcs if (v["scid"], v["vcid"]) == (scid, vcid)), None)


def map_of(cfg, scid, vcid, map_id):
    return next((m for m in cfg["maps"] if (m["scid"], m["vcid"], m["map"]) ==
                 (scid, vcid, map_id)), None)


# ---- the frame, §4.1

def header_fields(h):
    """The Primary Header as a 40-bit number, bit 0 its most significant bit (§1.6.2)."""
    n = int.from_bytes(h[:HEADER], "big")

    def bits(first, width):
        return (n >> (40 - first - width)) & ((1 << width) - 1)

    return {"tfvn": bits(0, 2), "bypass": bits(2, 1), "cc": bits(3, 1), "spare": bits(4, 2),
            "scid": bits(6, 10), "vcid": bits(16, 6), "count": bits(22, 10),
            "ns": bits(32, 8)}


def control(data):
    """§4.1.3.3: Unlock is one octet of zeros; Set V(R) is 10000010 00000000 then V(R)."""
    if data == bytes([0]):
        return ("unlock", None)
    if len(data) == 3 and data[0] == 0b10000010 and data[1] == 0:
        return ("set_vr", data[2])
    return None


def parse(cfg, frame, mode, exact):
    """The decoder (exact=False) or the Frame Validation Checks (exact=True), in the check
    order of docs/standards/tc-frame.md §4. Returns the decoded fields or raises Fail."""
    lenient = mode == "lenient"
    anomalies = []
    if len(frame) < HEADER:
        if exact:
            raise Fail("LengthMismatch", F["frame_len"], "LengthMismatch")
        raise Fail("Incomplete")
    h = header_fields(frame)
    if h["tfvn"] != 0:                                               # (a)
        raise Fail("UnsupportedVersion", F["frame_version"], "UnexpectedVersion")
    if not any(v["scid"] == h["scid"] for v in cfg["vcs"]):          # (b)
        raise Fail("UnknownChannel", F["spacecraft_id"], "UnexpectedSpacecraft")
    if (h["bypass"], h["cc"]) not in TYPES:                          # (c)
        raise Fail("ReservedValue", F["control_command_flag"], "ReservedFlags")
    if h["spare"] != 0:
        if not lenient:
            raise Fail("ReservedValue", F["tc_header_spare"], "ReservedSpare")
        anomalies.append("ReservedSpareSet")
    vc = vc_of(cfg["vcs"], h["scid"], h["vcid"])
    if vc is None:
        raise Fail("UnknownChannel", F["vc_id"], "InvalidVcid")
    total = h["count"] + 1
    if total > min(cfg["max"], vc["max"]):
        raise Fail("OutOfRange", F["frame_len"], "FrameTooLong")
    kind = TYPES[(h["bypass"], h["cc"])]
    seg = vc["seg"] and kind != "BC"
    fecf = 2 if cfg["fecf"] else 0
    if total < HEADER + (1 if seg else 0) + 1 + fecf:
        raise Fail("InvalidField", F["frame_data_field"], "FrameTooShort")
    if exact and len(frame) != total or len(frame) < total:          # (d)
        if exact:
            raise Fail("LengthMismatch", F["frame_len"], "LengthMismatch")
        raise Fail("Incomplete")
    frame = frame[:total]
    if fecf and crc(frame[:-2]) != int.from_bytes(frame[-2:], "big"):  # (e)
        raise Fail("CrcMismatch", None, "FecfMismatch")
    at = HEADER
    segment = None
    sh = st = 0
    if seg:
        octet = frame[at]
        segment = {"sequence_flags": octet >> 6, "map_id": octet & 0x3F}
        at += 1
        m = map_of(cfg, h["scid"], h["vcid"], segment["map_id"])
        if m is None:
            raise Fail("UnknownChannel", F["map_id"], "InvalidMapId")
        sh, st = m["sh"], m["st"]
    elif kind != "BC":
        sh, st = vc["sh"], vc["st"]
    if at + sh + 1 + st + fecf > total:
        raise Fail("InvalidField", F["frame_data_field"], "FrameTooShort")
    end = total - fecf
    data = frame[at + sh:end - st]
    command = None
    if kind == "BC":
        command = control(data)
        if command is None:
            if not lenient:
                raise Fail("ReservedValue", F["control_command"], "ReservedControlCommand")
            anomalies.append("UnknownControlCommand")
    order = ["ReservedSpareSet", "UnknownControlCommand", "ClcwSpareSet"]
    return {
        "frame_type": kind, "bypass_flag": h["bypass"], "control_command_flag": h["cc"],
        "spacecraft_id": h["scid"], "vc_id": h["vcid"], "frame_len": total,
        "sequence_number": h["ns"], "segment_header": segment,
        "security_header_hex": hx(frame[at:at + sh]), "data_hex": hx(data),
        "security_trailer_hex": hx(frame[end - st:end]),
        "fecf_hex": hx(frame[-2:]) if fecf else None,
        "control_command": command[0] if command else None,
        "set_vr_value": command[1] if command else None,
        "consumed": total, "anomalies": sorted(anomalies, key=order.index),
    }


def peek(prefix):
    if len(prefix) < HEADER:
        raise Fail("Incomplete")
    h = header_fields(prefix)
    if h["tfvn"] != 0:
        raise Fail("UnsupportedVersion", F["frame_version"])
    if (h["bypass"], h["cc"]) not in TYPES:
        raise Fail("ReservedValue", F["control_command_flag"])
    return h["count"] + 1


def encode(cfg, p):
    """The writer: the sender's fields and data to octets, derived fields computed (TC-007)."""
    kind, scid, vcid = p["frame_type"], p["spacecraft_id"], p["vc_id"]
    # API-001: an identifier wider than its field is OutOfRange, before any lookup.
    if scid >= 1 << 10:
        raise Fail("OutOfRange", F["spacecraft_id"])
    if vcid >= 1 << 6:
        raise Fail("OutOfRange", F["vc_id"])
    if not any(v["scid"] == scid for v in cfg["vcs"]):
        raise Fail("UnknownChannel", F["spacecraft_id"])
    vc = vc_of(cfg["vcs"], scid, vcid)
    if vc is None:
        raise Fail("UnknownChannel", F["vc_id"])
    is_control = "control_command" in p
    if is_control != (kind == "BC"):
        raise Fail("InvalidField", F["control_command_flag"])
    sh = st = 0
    head = b""
    if is_control:
        body = bytes([0]) if p["control_command"] == "unlock" else bytes(
            [0b10000010, 0, p["set_vr_value"]])
    else:
        segment = p["segment_header"]
        if (segment is not None) != vc["seg"]:
            raise Fail("InvalidField", F["segment_header"])
        if segment is not None:
            if segment["map_id"] >= 1 << 6:
                raise Fail("OutOfRange", F["map_id"])
            m = map_of(cfg, scid, vcid, segment["map_id"])
            if m is None:
                raise Fail("UnknownChannel", F["map_id"])
            sh, st = m["sh"], m["st"]
            head = bytes([segment["sequence_flags"] << 6 | segment["map_id"]])
        else:
            sh, st = vc["sh"], vc["st"]
        body = bytes.fromhex(p["user_data_hex"])
        if not body:
            raise Fail("InvalidField", F["frame_data_field"])
    fecf = 2 if cfg["fecf"] else 0
    total = HEADER + len(head) + sh + len(body) + st + fecf
    if total > min(cfg["max"], vc["max"]):
        raise Fail("OutOfRange", F["frame_len"])
    bypass, cc = FLAGS[kind]
    n = 0
    for value, width in [(0, 2), (bypass, 1), (cc, 1), (0, 2), (scid, 10), (vcid, 6),
                         (total - 1, 10), (p["sequence_number"], 8)]:
        n = (n << width) | value
    frame = n.to_bytes(HEADER, "big") + head + bytes(sh) + body + bytes(st)
    if fecf:
        frame += crc(frame).to_bytes(2, "big")
    return frame


def delimit(unit):
    """§4.4.9.2.4-6 as TC-020 states it: frames by Frame Length; a declared frame longer than
    what remains is discarded; octets that cannot start a valid header are fill."""
    items, at = [], 0
    while at < len(unit):
        rest = unit[at:]
        h = header_fields(rest) if len(rest) >= HEADER else None
        if h is None or h["tfvn"] != 0 or h["count"] + 1 < HEADER + 1:
            items.append({"kind": "fill", "offset": at, "octets_hex": hx(rest),
                          "declared_len": None})
            break
        total = h["count"] + 1
        if total > len(rest):
            items.append({"kind": "truncated", "offset": at, "octets_hex": hx(rest),
                          "declared_len": total})
            break
        items.append({"kind": "frame", "offset": at, "octets_hex": hx(rest[:total]),
                      "declared_len": None})
        at += total
    return {"items": items}


# ---- the CLCW, §4.2.1: ten fields, bit 0 first

CLCW_FIELDS = [("control_word_type", 1), ("clcw_version", 2), ("status", 3),
               ("cop_in_effect", 2), ("vc_id", 6), ("spare_14_15", 2), ("no_rf_available", 1),
               ("no_bit_lock", 1), ("lockout", 1), ("wait", 1), ("retransmit", 1),
               ("farm_b_counter", 2), ("spare_23", 1), ("report_value", 8)]
FLAG_NAMES = ["no_rf_available", "no_bit_lock", "lockout", "wait", "retransmit"]


def clcw_decode(octets, mode):
    if len(octets) != 4:
        raise ValueError("a CLCW has four octets")
    n, at, f = int.from_bytes(octets, "big"), 0, {}
    for name, width in CLCW_FIELDS:
        f[name] = (n >> (32 - at - width)) & ((1 << width) - 1)
        at += width
    if f["control_word_type"] != 0:
        raise Fail("InvalidField", F["control_word_type"])
    if f["clcw_version"] != 0:
        raise Fail("UnsupportedVersion", F["clcw_version"])
    anomalies = []
    for name, code in [("spare_14_15", "clcw_spare_14_15"), ("spare_23", "clcw_spare_23")]:
        if f[name] != 0:
            if mode != "lenient":
                raise Fail("ReservedValue", F[code])
            if not anomalies:
                anomalies.append("ClcwSpareSet")
    out = {"status": f["status"], "cop_in_effect": f["cop_in_effect"], "vc_id": f["vc_id"]}
    for name in FLAG_NAMES:
        out[name] = f[name] == 1
    out.update({"farm_b_counter": f["farm_b_counter"], "report_value": f["report_value"],
                "anomalies": anomalies})
    return out


def clcw_encode(p):
    for name, width, code in [("status", 3, "clcw_status"), ("cop_in_effect", 2, "cop_in_effect"),
                              ("vc_id", 6, "vc_id"), ("farm_b_counter", 2, "farm_b_counter")]:
        if p[name] >= 1 << width:
            raise Fail("OutOfRange", F[code])
    values = dict(p)
    values.update({"control_word_type": 0, "clcw_version": 0, "spare_14_15": 0, "spare_23": 0})
    n = 0
    for name, width in CLCW_FIELDS:
        v = values[name]
        n = (n << width) | (int(v) if isinstance(v, bool) else v)
    return n.to_bytes(4, "big")


# ---- expected outcomes

def err(e):
    out = {"ok": False, "error": e.kind}
    if e.field is not None:
        out["field"] = e.field
    return out


def expect(case):
    op = case["operation"]
    data = bytes.fromhex(case.get("input_hex", ""))
    mode = case.get("mode", "strict")
    params = case.get("params", {})
    try:
        if op == "clcw_decode":
            return {"ok": True, **clcw_decode(data, mode)}
        if op == "clcw_encode":
            return {"ok": True, "output_hex": hx(clcw_encode(params))}
        if op == "peek_length":
            return {"ok": True, "frame_len": peek(data)}
        if op == "delimit":
            return {"ok": True, **delimit(data)}
        cfg = config(case.get("config"))
        if op == "config":
            return {"ok": True, "vcs": len(cfg["vcs"]), "maps": len(cfg["maps"])}
        if op == "decode":
            return {"ok": True, **parse(cfg, data, mode, exact=False)}
        if op == "encode":
            return {"ok": True, "output_hex": hx(encode(cfg, params))}
        if op == "validate":
            try:
                f = parse(cfg, data, mode, exact=True)
            except Fail as e:
                return {"ok": True, "accepted": False, "reason": e.reason,
                        "reason_code": REASONS[e.reason], "error_kind": e.kind,
                        "error_field": e.field}
            return {"ok": True, "accepted": True, "frame_len": f["frame_len"]}
    except Fail as e:
        return err(e)
    raise ValueError(f"{case['id']}: the model has no operation {op!r}")


# ---- the generator: how the inputs of the vector files were chosen

def vc(vcid, seg=False, max_len=MAX_FRAME, sh=0, st=0, scid=SCID):
    v = {"scid": scid, "vcid": vcid, "max_frame_len": max_len, "segment_header": seg}
    if sh or st:
        v["security_header_len"], v["security_trailer_len"] = sh, st
    return v


def mp(vcid, map_id, sh=0, st=0, scid=SCID):
    m = {"scid": scid, "vcid": vcid, "map_id": map_id}
    if sh or st:
        m["security_header_len"], m["security_trailer_len"] = sh, st
    return m


def cfg(fecf=True, max_len=MAX_FRAME, vcs=None, maps=None):
    c = {"max_frame_len": max_len, "fecf": fecf,
         "vcs": vcs if vcs is not None else [vc(1), vc(2, seg=True), vc(3, sh=4, st=2)]}
    c["maps"] = maps if maps is not None else [mp(2, 0), mp(2, 5), mp(2, 9, sh=3, st=1)]
    return c


STD = cfg()
NO_FECF = cfg(fecf=False)
# Field extremes (follow-up 6): SCID with bit 9 set and the largest SCID, VCID 32 and 63, MAP 63.
EXT_HI = cfg(vcs=[vc(63, scid=0x3FF)], maps=[])
EXT_SEG = cfg(vcs=[vc(32, seg=True, scid=0x200)],
              maps=[mp(32, 63, scid=0x200), mp(32, 0, scid=0x200)])


def fields(kind, vcid, ns, scid=SCID):
    return {"frame_type": kind, "spacecraft_id": scid, "vc_id": vcid, "sequence_number": ns}


def data_params(kind, vcid, ns, user, segment=None, scid=SCID):
    p = fields(kind, vcid, ns, scid)
    p["segment_header"] = segment
    p["user_data_hex"] = hx(user)
    return p


def control_params(vcid, ns, command, value=None, kind="BC", scid=SCID):
    p = fields(kind, vcid, ns, scid)
    p["control_command"] = command
    if command == "set_vr":
        p["set_vr_value"] = value
    return p


def seg(flags, map_id):
    return {"sequence_flags": flags, "map_id": map_id}


def frame_of(c, p):
    return encode(config(c), p)


def hdr(kind, vcid, total, ns, spare=0, tfvn=0, scid=SCID, flags=None):
    """Raw header octets, for inputs that the writer would not produce."""
    b, c = flags if flags is not None else FLAGS[kind]
    n = 0
    for value, width in [(tfvn, 2), (b, 1), (c, 1), (spare, 2), (scid, 10), (vcid, 6),
                         (total - 1, 10), (ns, 8)]:
        n = (n << width) | value
    return n.to_bytes(HEADER, "big")


def with_fecf(body):
    return body + crc(body).to_bytes(2, "big")


def bad_fecf(body):
    """The frame with the last bit of its FECF flipped."""
    good = with_fecf(body)
    return good[:-1] + bytes([good[-1] ^ 1])


class Builder:
    def __init__(self):
        self.cases = []

    def add(self, cid, reqs, desc, op, mode=None, config_=None, params=None, data=None):
        c = {"id": cid, "requirements": reqs, "description": desc, "operation": op}
        if mode is not None:
            c["mode"] = mode
        if config_ is not None:
            c["config"] = config_
        if params is not None:
            c["params"] = params
        c["input_hex"] = hx(data if data is not None else b"")
        c["expect"] = expect(c)
        self.cases.append(c)
        return c

    def encode_decode(self, prefix, n, reqs, desc, c, p, extra=b""):
        """An encode case and a decode case of the octets it gives (API-020)."""
        self.add(f"{prefix}-enc-{n:03}", reqs + ["API-020", "API-021", "TV-002"], desc,
                 "encode", config_=c, params=p)
        frame = frame_of(c, p)
        self.add(f"{prefix}-dec-{n:03}", reqs + ["API-010", "TV-002"], desc + ": decoded",
                 "decode", mode="strict", config_=c, data=frame + extra)
        return frame


def gen_ad():
    b = Builder()
    b.encode_decode("tc-ad", 1, ["TC-001", "TC-006", "TC-007", "GEN-053"],
                    "AD frame, VC 1, N(S) 0, one data octet, FECF", STD,
                    data_params("AD", 1, 0, b"\xA5"))
    b.encode_decode("tc-ad", 2, ["TC-001", "TC-006", "TC-007"],
                    "AD frame, VC 1, N(S) 255, five data octets, FECF", STD,
                    data_params("AD", 1, 255, bytes([1, 2, 3, 4, 5])))
    b.encode_decode("tc-ad", 3, ["TC-001", "TC-007"],
                    "AD frame without FECF (physical channel without the field)", NO_FECF,
                    data_params("AD", 1, 7, bytes([0xDE, 0xAD, 0xBE, 0xEF])))
    b.encode_decode("tc-ad", 4, ["TC-001", "API-011"],
                    "AD frame followed by three more octets: exactly one frame is consumed",
                    STD, data_params("AD", 1, 0x42, b"\x11\x22"), extra=b"\x00\xFF\x55")
    b.encode_decode("tc-ad", 5, ["TC-001", "TC-006", "API-070", "API-072"],
                    "AD frame on VC 3 with a 4-octet Security Header and a 2-octet Security "
                    "Trailer (§6.3): written as zeros, exposed as slices", STD,
                    data_params("AD", 3, 9, b"\x01\x02\x03"))
    b.encode_decode("tc-ad", 6, ["TC-001", "TC-006", "API-001"],
                    "AD frame on the largest SCID 0x3FF and the largest VCID 63, N(S) 255: "
                    "every bit of both fields set", EXT_HI,
                    data_params("AD", 63, 255, b"\x5A\xA5", scid=0x3FF))
    b.add("tc-ad-enc-err-001", ["API-001", "TC-007"],
          "SCID 1024 does not fit ten bits: OutOfRange spacecraft_id, not UnknownChannel",
          "encode", config_=STD, params=data_params("AD", 1, 0, b"\x01", scid=1024))
    b.add("tc-ad-enc-err-002", ["API-001", "TC-007"],
          "VCID 64 does not fit six bits: OutOfRange vc_id, not UnknownChannel", "encode",
          config_=STD, params=data_params("AD", 64, 0, b"\x01"))
    frame = frame_of(STD, data_params("AD", 1, 0, b"\xA5"))
    b.add("tc-ad-peek-001", ["TC-001", "API-013", "GEN-053"],
          "peek_length from the five header octets", "peek_length", data=frame[:5])
    b.add("tc-ad-peek-002", ["API-013"], "peek_length from four octets: incomplete",
          "peek_length", data=frame[:4])
    b.add("tc-ad-peek-003", ["TC-002", "API-013"], "peek_length of a header with TFVN 01",
          "peek_length", data=hdr("AD", 1, 8, 0, tfvn=1))
    b.add("tc-ad-peek-004", ["TC-002", "API-013"],
          "peek_length of a header with the reserved flags 0/1", "peek_length",
          data=hdr("AD", 1, 8, 0, flags=(0, 1)))
    b.add("tc-ad-peek-005", ["TC-001", "API-013", "GEN-053"],
          "peek_length of the longest frame: Frame Length 1023 is 1024 octets", "peek_length",
          data=hdr("BD", 1, 1024, 0))
    return b.cases


def gen_bd():
    b = Builder()
    b.encode_decode("tc-bd", 1, ["TC-001", "TC-006", "TC-008"],
                    "BD frame with N(S) 0, as COP-1 sets it", STD,
                    data_params("BD", 1, 0, b"\xC0\xFF\xEE"))
    b.encode_decode("tc-bd", 2, ["TC-008"],
                    "BD frame with N(S) 0x5A: the caller's value is written and decoded, "
                    "not forced to 0", STD, data_params("BD", 1, 0x5A, b"\x01"))
    b.encode_decode("tc-bd", 3, ["TC-001", "TC-008"], "BD frame without FECF, N(S) 255",
                    NO_FECF, data_params("BD", 1, 255, bytes(range(16))))
    b.encode_decode("tc-bd", 4, ["TC-001", "TC-006", "API-001"],
                    "BD frame on SCID 0x3FF and VCID 63, N(S) 0", EXT_HI,
                    data_params("BD", 63, 0, b"\xC3", scid=0x3FF))
    b.add("tc-bd-enc-err-001", ["TC-009"],
          "A BD frame cannot carry a Control Command (§4.1.2.3.2.2): InvalidField "
          "control_command_flag",
          "encode", config_=STD, params=control_params(1, 0, "unlock", kind="BD"))
    return b.cases


def gen_bc_unlock():
    b = Builder()
    b.encode_decode("tc-bc-unlock", 1, ["TC-005", "TC-006"],
                    "BC frame with the Unlock Control Command, FECF", STD,
                    control_params(1, 0, "unlock"))
    b.encode_decode("tc-bc-unlock", 2, ["TC-005"], "Unlock without FECF", NO_FECF,
                    control_params(1, 0, "unlock"))
    b.encode_decode("tc-bc-unlock", 3, ["TC-004", "TC-005"],
                    "Unlock on VC 2, which has Segment Headers: a Type-C frame has none",
                    STD, control_params(2, 0, "unlock"))
    b.encode_decode("tc-bc-unlock", 7, ["TC-004", "TC-005", "API-070"],
                    "Unlock on VC 3, whose Security Header (4) and Trailer (2) apply to Type-D "
                    "frames only: a Type-C frame has neither (§6.3.4.1, §6.3.6.1), so it is 8 "
                    "octets", STD, control_params(3, 0, "unlock"))
    two = with_fecf(hdr("BC", 1, 9, 0) + b"\x00\x00")
    b.add("tc-bc-unlock-dec-004", ["TC-005", "API-031"],
          "Two octets of zeros is not Unlock (one octet): reserved, strict", "decode",
          mode="strict", config_=STD, data=two)
    b.add("tc-bc-unlock-dec-005", ["TC-005", "API-032"],
          "The same frame in lenient mode: UnknownControlCommand, raw octets in data_hex",
          "decode", mode="lenient", config_=STD, data=two)
    one = with_fecf(hdr("BC", 1, 8, 0) + b"\x01")
    b.add("tc-bc-unlock-dec-006", ["TC-005", "API-031"],
          "One octet 0x01: a reserved Control Command, strict", "decode", mode="strict",
          config_=STD, data=one)
    b.add("tc-bc-unlock-enc-err-001", ["TC-009"],
          "An AD frame cannot carry a Control Command (§4.1.2.3.2.2): InvalidField "
          "control_command_flag",
          "encode", config_=STD, params=control_params(1, 0, "unlock", kind="AD"))
    b.add("tc-bc-unlock-enc-err-002", ["TC-009"],
          "A BC frame cannot carry a Frame Data Unit (§4.1.2.3.2.2): InvalidField "
          "control_command_flag",
          "encode", config_=STD, params=data_params("BC", 1, 0, b"\x00"))
    return b.cases


def gen_bc_setvr():
    b = Builder()
    b.encode_decode("tc-bc-setvr", 1, ["TC-005", "TC-006"], "Set V(R) to 0, FECF", STD,
                    control_params(1, 0, "set_vr", 0))
    b.encode_decode("tc-bc-setvr", 2, ["TC-005"], "Set V(R) to 255, N(S) 0x11", STD,
                    control_params(1, 0x11, "set_vr", 255))
    b.encode_decode("tc-bc-setvr", 3, ["TC-005"], "Set V(R) to 0x80, no FECF", NO_FECF,
                    control_params(1, 0, "set_vr", 0x80))
    b.encode_decode("tc-bc-setvr", 12, ["TC-004", "TC-005", "API-070"],
                    "Set V(R) 9 on VC 3, whose Security Header and Trailer apply to Type-D "
                    "frames only: a Type-C frame has neither (§6.3.4.1, §6.3.6.1)", STD,
                    control_params(3, 0, "set_vr", 9))
    for n, (body, desc) in enumerate([
            (b"\x82\x00", "82 00 without the V(R) octet"),
            (b"\x82\x01\x05", "82 01 05: the second octet must be zero"),
            (b"\x83\x00\x05", "83 00 05: not the Set V(R) pattern"),
            (b"\x82\x00\x05\x00", "82 00 05 00: one octet too many")], start=4):
        frame = with_fecf(hdr("BC", 1, 5 + len(body) + 2, 0) + body)
        b.add(f"tc-bc-setvr-dec-{n:03}", ["TC-005", "API-031"], desc + ": reserved, strict",
              "decode", mode="strict", config_=STD, data=frame)
        b.add(f"tc-bc-setvr-dec-{n + 4:03}", ["TC-005", "API-032"],
              desc + ": lenient, UnknownControlCommand", "decode", mode="lenient",
              config_=STD, data=frame)
    return b.cases


def gen_segment_header():
    b = Builder()
    for n, (flags, name) in enumerate([(0b01, "first"), (0b00, "continuing"),
                                        (0b10, "last"), (0b11, "unsegmented")], start=1):
        b.encode_decode("tc-seg", n, ["TC-004", "TC-001"],
                        f"Segment Header on VC 2, MAP 5, Sequence Flags {flags:02b} ({name})",
                        STD, data_params("AD", 2, n, bytes([0x10 * n, 0x20]), seg(flags, 5)))
    b.encode_decode("tc-seg", 5, ["TC-004", "API-070", "API-072"],
                    "Segment Header on MAP 9, which has a 3-octet Security Header and a 1-octet "
                    "Security Trailer: the Security Header follows the Segment Header (§6.3.4.2)",
                    STD, data_params("BD", 2, 0, b"\xAA\xBB", seg(0b11, 9)))
    b.encode_decode("tc-seg", 6, ["TC-004"], "MAP 0, no FECF", cfg(fecf=False),
                    data_params("AD", 2, 0, b"\x77", seg(0b11, 0)))
    b.encode_decode("tc-seg", 7, ["TC-004", "API-001"],
                    "Segment Header on SCID 0x200, VCID 32, MAP 63 with Sequence Flags 11: the "
                    "Segment Header octet is FF", EXT_SEG,
                    data_params("AD", 32, 0x80, b"\x12\x34", seg(0b11, 63), scid=0x200))
    b.encode_decode("tc-seg", 8, ["TC-004", "API-001"],
                    "Segment Header on SCID 0x200, VCID 32, MAP 0 with Sequence Flags 10 "
                    "(last): the Segment Header octet is 80", EXT_SEG,
                    data_params("BD", 32, 0, b"\x56", seg(0b10, 0), scid=0x200))
    b.add("tc-seg-enc-err-005", ["API-001", "TC-004"],
          "MAP ID 64 does not fit six bits: OutOfRange map_id, not UnknownChannel", "encode",
          config_=STD, params=data_params("AD", 2, 0, b"\x01", seg(0b11, 64)))
    unknown = with_fecf(hdr("AD", 2, 10, 0) + bytes([0b11 << 6 | 7, 1, 2]))
    b.add("tc-seg-dec-err-001", ["TC-004", "TC-022"],
          "MAP 7 is not configured on VC 2: UnknownChannel map_id", "decode", mode="strict",
          config_=STD, data=unknown)
    b.add("tc-seg-enc-err-001", ["TC-004", "TC-007"],
          "A Segment Header on VC 1, which has none: InvalidField segment_header", "encode",
          config_=STD, params=data_params("AD", 1, 0, b"\x01", seg(0b11, 0)))
    b.add("tc-seg-enc-err-002", ["TC-004", "TC-007"],
          "No Segment Header on VC 2, which needs one: InvalidField segment_header", "encode",
          config_=STD, params=data_params("AD", 2, 0, b"\x01"))
    b.add("tc-seg-enc-err-003", ["TC-022", "TC-007"],
          "MAP 7 is not configured: UnknownChannel map_id", "encode", config_=STD,
          params=data_params("AD", 2, 0, b"\x01", seg(0b11, 7)))
    b.add("tc-seg-enc-err-004", ["TC-007", "CMN-041"],
          "No user data after the Segment Header: InvalidField frame_data_field", "encode",
          config_=STD, params=data_params("AD", 2, 0, b"", seg(0b11, 5)))
    short = with_fecf(hdr("AD", 2, 8, 0) + bytes([0b11 << 6 | 5]))
    b.add("tc-seg-dec-err-002", ["TC-004", "CMN-041"],
          "Frame Length leaves no User Data after the Segment Header: InvalidField "
          "frame_data_field", "decode", mode="strict", config_=STD, data=short)
    sec = with_fecf(hdr("AD", 2, 12, 0) + bytes([0b11 << 6 | 9]) + bytes(4))
    b.add("tc-seg-dec-err-003", ["TC-004", "API-070", "CMN-041"],
          "MAP 9's Security Header and Trailer leave no User Data: InvalidField "
          "frame_data_field", "decode", mode="strict", config_=STD, data=sec)
    return b.cases


def gen_max_length():
    b = Builder()
    b.encode_decode("tc-max", 1, ["TC-007", "TC-001", "TC-006"],
                    "1024-octet AD frame with FECF: 1017 data octets", STD,
                    data_params("AD", 1, 3, bytes((i * 7) & 0xFF for i in range(1017))))
    b.encode_decode("tc-max", 2, ["TC-007", "TC-001"],
                    "1024-octet BD frame without FECF: 1019 data octets", NO_FECF,
                    data_params("BD", 1, 0, bytes((255 - i) & 0xFF for i in range(1019))))
    b.add("tc-max-enc-err-001", ["TC-007"],
          "1018 data octets with FECF: 1025 octets, above 1024: OutOfRange frame_len",
          "encode", config_=STD, params=data_params("AD", 1, 0, bytes(1018)))
    b.add("tc-max-enc-err-002", ["TC-007"],
          "1020 data octets without FECF: OutOfRange frame_len", "encode", config_=NO_FECF,
          params=data_params("AD", 1, 0, bytes(1020)))
    small = cfg(vcs=[vc(1, max_len=64)], maps=[])
    b.encode_decode("tc-max", 3, ["TC-007", "GEN-060"],
                    "A 64-octet frame on a VC whose maximum is 64", small,
                    data_params("AD", 1, 0, bytes(57)))
    b.add("tc-max-enc-err-003", ["TC-007", "GEN-060"],
          "A 65-octet frame on a VC whose maximum is 64: OutOfRange frame_len", "encode",
          config_=small, params=data_params("AD", 1, 0, bytes(58)))
    long_frame = with_fecf(hdr("AD", 1, 65, 0) + bytes(58))
    b.add("tc-max-dec-err-001", ["TC-007", "TC-021"],
          "Decoding a 65-octet frame on that VC: OutOfRange frame_len", "decode",
          mode="strict", config_=small, data=long_frame)
    pc = cfg(max_len=100, vcs=[vc(1)], maps=[])
    b.add("tc-max-dec-004", ["TC-007", "GEN-060"],
          "Decoding a 100-octet frame, exactly the physical channel's maximum (the VC allows "
          "1024): accepted", "decode", mode="strict", config_=pc,
          data=with_fecf(hdr("AD", 1, 100, 0) + bytes(93)))
    b.add("tc-max-dec-err-002", ["TC-007", "TC-021"],
          "Decoding a 101-octet frame, one above the physical channel's maximum of 100: "
          "OutOfRange frame_len", "decode", mode="strict", config_=pc,
          data=with_fecf(hdr("AD", 1, 101, 0) + bytes(94)))
    b.add("tc-max-enc-err-004", ["TC-007", "GEN-060"],
          "The physical channel's maximum (100) applies when the VC allows 1024", "encode",
          config_=pc, params=data_params("AD", 1, 0, bytes(94)))
    b.add("tc-max-enc-005", ["TC-007", "GEN-060"],
          "Exactly the physical channel's maximum, 100 octets", "encode", config_=pc,
          params=data_params("AD", 1, 0, bytes(93)))
    return b.cases


def gen_delimit():
    b = Builder()
    f1 = frame_of(STD, data_params("AD", 1, 0, b"\x01\x02"))
    f2 = frame_of(STD, control_params(1, 0, "unlock"))
    f3 = frame_of(STD, data_params("BD", 1, 0, bytes(range(10))))
    f4 = frame_of(NO_FECF, data_params("AD", 1, 1, b"\x09"))
    b.add("tc-delimit-001", ["TC-020"], "One frame filling the data unit exactly", "delimit",
          data=f1)
    b.add("tc-delimit-002", ["TC-020"],
          "Two frames and three octets of fill 0x55 (fewer than a header)", "delimit",
          data=f1 + f2 + b"\x55" * 3)
    b.add("tc-delimit-003", ["TC-020"],
          "Three frames and six octets of fill 0x55: TFVN 01 is not a valid header "
          "(§4.4.9.2.6 note 2). The rest is reported as Fill, which is the reading of OI-41 item 2; "
          "the Blue Book only requires the octets to be discarded", "delimit",
          data=f1 + f2 + f3 + b"\x55" * 6)
    b.add("tc-delimit-004", ["TC-020"],
          "A frame, then a frame cut short: the rest is discarded as a truncated frame "
          "(§4.4.9.2.5)", "delimit", data=f1 + f3[:-4])
    b.add("tc-delimit-005", ["TC-020"], "An empty data unit yields nothing", "delimit",
          data=b"")
    b.add("tc-delimit-006", ["TC-020"], "Only fill octets", "delimit", data=b"\x55" * 7)
    b.add("tc-delimit-007", ["TC-020"],
          "A header that declares a 5-octet frame is not a valid header: fill (OI-41 item 2, a "
          "project reading)", "delimit",
          data=f4 + hdr("AD", 1, 5, 0) + b"\x00")
    b.add("tc-delimit-008", ["TC-020"],
          "Frames without FECF back to back, then four octets of zero fill", "delimit",
          data=f4 + f4 + bytes(4))
    b.add("tc-delimit-009", ["TC-020"],
          "Five zero octets of fill: TFVN 00, but a Frame Length of 1 octet", "delimit",
          data=f2 + bytes(5))
    return b.cases


def gen_validation():
    b = Builder()
    ok_ad = frame_of(STD, data_params("AD", 1, 4, b"\x01\x02\x03"))
    ok_bd = frame_of(STD, data_params("BD", 1, 0, b"\x01\x02\x03"))
    b.add("tc-val-001", ["TC-021", "TC-023"], "A valid AD frame is accepted", "validate",
          mode="strict", config_=STD, data=ok_ad)
    b.add("tc-val-002", ["TC-021", "TC-023"],
          "The same frame as BD is accepted by the same checks", "validate", mode="strict",
          config_=STD, data=ok_bd)

    def bad(n, reqs, desc, data, mode="strict", c=STD):
        b.add(f"tc-val-{n:03}", reqs, desc, "validate", mode=mode, config_=c, data=data)

    bad(3, ["TC-021", "TC-002"], "(a) TFVN 01: UnexpectedVersion",
        with_fecf(hdr("AD", 1, 10, 0, tfvn=1) + b"\x01\x02\x03"))
    bad(4, ["TC-021"], "(b) SCID 0x156, not an expected MCID: UnexpectedSpacecraft",
        with_fecf(hdr("AD", 1, 10, 0, scid=0x156) + b"\x01\x02\x03"))
    bad(5, ["TC-021", "TC-002"], "(c) Bypass 0 / Control Command 1: ReservedFlags",
        with_fecf(hdr("AD", 1, 10, 0, flags=(0, 1)) + b"\x01\x02\x03"))
    spare = with_fecf(hdr("AD", 1, 10, 0, spare=0b10) + b"\x01\x02\x03")
    bad(6, ["TC-021", "TC-003"], "(c) Reserved Spare 10, strict: ReservedSpare", spare)
    b.add("tc-val-007", ["TC-021", "TC-003", "API-032"],
          "Reserved Spare 10, lenient: accepted", "validate", mode="lenient", config_=STD,
          data=spare)
    bad(8, ["TC-021", "TC-022"], "(c) VCID 4 is not configured: InvalidVcid",
        with_fecf(hdr("AD", 4, 10, 0) + b"\x01\x02\x03"))
    small = cfg(vcs=[vc(1, max_len=16)], maps=[])
    bad(9, ["TC-021"], "(c) Frame Length 17 above the VC's maximum 16: FrameTooLong",
        with_fecf(hdr("AD", 1, 17, 0) + bytes(10)), c=small)
    bad(10, ["TC-021"], "(c) Frame Length 7 leaves no data field with FECF: FrameTooShort",
        hdr("AD", 1, 7, 0) + b"\x00\x00")
    bad(11, ["TC-021"], "(d) Frame Length 10, eleven octets present: LengthMismatch",
        ok_ad + b"\x00")
    bad(12, ["TC-021"], "(d) Frame Length 10, nine octets present: LengthMismatch",
        ok_ad[:-1])
    bad(13, ["TC-021"], "(d) Four octets, no complete header: LengthMismatch", ok_ad[:4])
    flipped = bytearray(ok_ad)
    flipped[6] ^= 0x01
    bad(14, ["TC-021", "TC-006"], "(e) One data bit flipped: FecfMismatch", bytes(flipped))
    bad(15, ["TC-022"], "MAP 7 on VC 2 is not configured: InvalidMapId",
        with_fecf(hdr("AD", 2, 10, 0) + bytes([0b11 << 6 | 7, 1, 2])))
    bad(16, ["TC-005"], "Type-C frame with a reserved Control Command, strict: "
        "ReservedControlCommand", with_fecf(hdr("BC", 1, 8, 0) + b"\x07"))
    b.add("tc-val-017", ["TC-005", "API-032"],
          "The same frame in lenient mode is accepted", "validate", mode="lenient",
          config_=STD, data=with_fecf(hdr("BC", 1, 8, 0) + b"\x07"))
    bad(18, ["TC-021", "TC-022"],
        "(b) SCID 0x3FF is not expected, and VCID 4 is not configured (c) either: the SCID check (b) comes first, so UnexpectedSpacecraft",
        with_fecf(hdr("AD", 4, 10, 0, scid=0x3FF) + b"\x01\x02\x03"))
    bad(19, ["TC-021", "TC-006"], "(e) the BD frame of tc-val-002 with the first bit of its FECF flipped: every other check passes, FecfMismatch",
        ok_bd[:-2] + bytes([ok_bd[-2] ^ 0x80, ok_bd[-1]]))
    bad(20, ["TC-021", "API-070"],
        "Frame on VC 3 whose Security Header and Trailer leave no data: FrameTooShort",
        with_fecf(hdr("AD", 3, 13, 0) + bytes(6)))
    # Added 2026-10-25 by a blind Claude Sonnet 5.5 agent (M2 WP-8 follow-ups 3 and 4).
    bad(21, ["TC-021", "TC-006", "TC-022"],
        "MAP 7 is not configured on VC 2 and the FECF is wrong: the FECF check (e) comes "
        "first, so FecfMismatch (reason 9), not InvalidMapId (reason 10)",
        bad_fecf(hdr("AD", 2, 10, 0) + bytes([0b11 << 6 | 7, 1, 2])))
    bad(22, ["TC-021"],
        "Frame Length 17 above the VC's maximum 16, with only 10 octets present: the length "
        "limit (c) comes before the length consistency (d), so FrameTooLong (reason 6), not "
        "LengthMismatch (reason 8)", hdr("AD", 1, 17, 0) + bytes(5), c=small)
    bad(23, ["TC-021"],
        "Frame Length 17 above the VC's maximum 16, with 20 octets present: FrameTooLong "
        "(reason 6), not LengthMismatch (reason 8)", hdr("AD", 1, 17, 0) + bytes(15), c=small)
    bad(24, ["TC-021"],
        "Frame Length 7 leaves no data field with FECF, with 12 octets present: the "
        "minimum-length check (c) comes before (d), so FrameTooShort (reason 7), not "
        "LengthMismatch (reason 8)", hdr("AD", 1, 7, 0) + bytes(7))
    bad(25, ["TC-021"],
        "Frame Length 7 leaves no data field with FECF, with 6 octets present: FrameTooShort "
        "(reason 7), not LengthMismatch (reason 8)", hdr("AD", 1, 7, 0) + bytes(1))
    return b.cases


def gen_decode_errors():
    b = Builder()
    ok = frame_of(STD, data_params("AD", 1, 0, b"\x01\x02\x03"))
    b.add("tc-dec-err-001", ["TC-001", "API-011"], "Four octets: Incomplete", "decode",
          mode="strict", config_=STD, data=ok[:4])
    b.add("tc-dec-err-002", ["TC-001", "API-011"],
          "Frame Length 10, nine octets present: Incomplete", "decode", mode="strict",
          config_=STD, data=ok[:-1])
    for n, mode in [(3, "strict"), (4, "lenient")]:
        b.add(f"tc-dec-err-{n:03}", ["TC-002", "API-032"],
              f"TFVN 01 is an error in both modes ({mode})", "decode", mode=mode,
              config_=STD, data=with_fecf(hdr("AD", 1, 10, 0, tfvn=1) + b"\x01\x02\x03"))
    for n, mode in [(5, "strict"), (6, "lenient")]:
        b.add(f"tc-dec-err-{n:03}", ["TC-002"],
              f"Bypass 0 / Control Command 1 is reserved in both modes ({mode})", "decode",
              mode=mode, config_=STD,
              data=with_fecf(hdr("AD", 1, 10, 0, flags=(0, 1)) + b"\x01\x02\x03"))
    spare = with_fecf(hdr("BD", 1, 10, 0, spare=0b01) + b"\x01\x02\x03")
    b.add("tc-dec-err-007", ["TC-003", "API-031"], "Reserved Spare 01, strict: ReservedValue",
          "decode", mode="strict", config_=STD, data=spare)
    b.add("tc-dec-err-008", ["TC-003", "API-032"],
          "Reserved Spare 01, lenient: decoded with ReservedSpareSet", "decode",
          mode="lenient", config_=STD, data=spare)
    b.add("tc-dec-err-009", ["TC-021"], "SCID 0x156 is not configured: UnknownChannel",
          "decode", mode="strict", config_=STD,
          data=with_fecf(hdr("AD", 1, 10, 0, scid=0x156) + b"\x01\x02\x03"))
    b.add("tc-dec-err-010", ["TC-022"], "VCID 63 is not configured: UnknownChannel", "decode",
          mode="strict", config_=STD,
          data=with_fecf(hdr("AD", 63, 10, 0) + b"\x01\x02\x03"))
    b.add("tc-dec-err-011", ["TC-006"], "FECF mismatch: CrcMismatch", "decode",
          mode="lenient", config_=STD, data=ok[:-1] + bytes([ok[-1] ^ 1]))
    b.add("tc-dec-err-012", ["TC-001", "CMN-041"],
          "Frame Length 5 leaves no data field: InvalidField frame_data_field", "decode",
          mode="strict", config_=NO_FECF, data=hdr("AD", 1, 5, 0) + bytes(3))
    b.add("tc-dec-err-014", ["TC-006", "TC-022"],
          "MAP 7 is not configured on VC 2 and the FECF is wrong: the FECF is checked before "
          "the Segment Header is read (tc-frame.md section 4.1), so CrcMismatch, not "
          "UnknownChannel", "decode", mode="strict", config_=STD,
          data=bad_fecf(hdr("AD", 2, 10, 0) + bytes([0b11 << 6 | 7, 1, 2])))
    b.add("tc-dec-err-013", ["TC-003", "TC-005", "API-032"],
          "Spare set and a reserved Control Command, lenient: both anomalies", "decode",
          mode="lenient", config_=STD,
          data=with_fecf(hdr("BC", 1, 9, 0x33, spare=0b11) + b"\x82\x00"))
    return b.cases


def gen_config():
    b = Builder()
    b.add("tc-cfg-001", ["GEN-060", "API-040"], "The configuration the other files use",
          "config", config_=STD)
    b.add("tc-cfg-002", ["GEN-060", "API-040", "TC-007"],
          "Physical channel maximum 1025: InvalidConfig max_frame_len", "config",
          config_=cfg(max_len=1025))
    b.add("tc-cfg-003", ["GEN-060", "API-040"],
          "Physical channel maximum 7 with FECF: no room for a frame", "config",
          config_=cfg(max_len=7, vcs=[vc(1)], maps=[]))
    b.add("tc-cfg-004", ["GEN-060", "API-040"],
          "Physical channel maximum 8 with FECF: the smallest frame fits", "config",
          config_=cfg(max_len=8, vcs=[vc(1)], maps=[]))
    b.add("tc-cfg-005", ["GEN-060", "API-040"], "VC maximum 5: InvalidConfig max_frame_len",
          "config", config_=cfg(vcs=[vc(1, max_len=5)], maps=[]))
    b.add("tc-cfg-006", ["GEN-060", "API-040"], "VC 1 twice: InvalidConfig vc_id", "config",
          config_=cfg(vcs=[vc(1), vc(1, seg=True)], maps=[]))
    b.add("tc-cfg-007", ["GEN-060", "API-040"],
          "The same VCID on two spacecraft is two channels", "config",
          config_=cfg(vcs=[vc(1), vc(1, scid=0x156)], maps=[]))
    b.add("tc-cfg-008", ["GEN-060", "API-040", "TC-004"],
          "A MAP on VC 1, which has no Segment Header: InvalidConfig map_id", "config",
          config_=cfg(vcs=[vc(1)], maps=[mp(1, 0)]))
    b.add("tc-cfg-009", ["GEN-060", "API-040"], "A MAP on an unknown VC: InvalidConfig map_id",
          "config", config_=cfg(vcs=[vc(2, seg=True)], maps=[mp(5, 0)]))
    b.add("tc-cfg-010", ["GEN-060", "API-040"], "MAP 3 twice: InvalidConfig map_id", "config",
          config_=cfg(vcs=[vc(2, seg=True)], maps=[mp(2, 3), mp(2, 3, sh=2)]))
    b.add("tc-cfg-011", ["GEN-060", "API-070", "CMN-041"],
          "A VC whose Security Header and Trailer fill its maximum: InvalidConfig "
          "frame_data_field", "config", config_=cfg(vcs=[vc(1, max_len=20, sh=8, st=5)],
                                                    maps=[]))
    b.add("tc-cfg-012", ["GEN-060", "API-070", "CMN-041"],
          "The same VC one octet longer leaves one data octet", "config",
          config_=cfg(vcs=[vc(1, max_len=21, sh=8, st=5)], maps=[]))
    b.add("tc-cfg-013", ["GEN-060", "API-070", "CMN-041"],
          "A MAP whose Security Header and Trailer fill its VC's maximum: InvalidConfig "
          "frame_data_field", "config",
          config_=cfg(vcs=[vc(2, seg=True, max_len=16)], maps=[mp(2, 1, sh=6, st=2)]))
    b.add("tc-cfg-014", ["GEN-060", "API-040", "CMN-041"],
          "A VC maximum of 6 with FECF and no SDLS: the smallest frame is 5 + 1 + 2 = 8 "
          "octets, so InvalidConfig frame_data_field", "config",
          config_=cfg(vcs=[vc(1, max_len=6)], maps=[]))
    b.add("tc-cfg-015", ["GEN-060", "API-040", "CMN-041"],
          "A VC maximum of 7 with FECF and no SDLS: still no data octet, InvalidConfig "
          "frame_data_field", "config", config_=cfg(vcs=[vc(1, max_len=7)], maps=[]))
    b.add("tc-cfg-016", ["GEN-060", "API-040", "CMN-041"],
          "A VC maximum of 8 with FECF and no SDLS: one data octet fits", "config",
          config_=cfg(vcs=[vc(1, max_len=8)], maps=[]))
    b.add("tc-cfg-017", ["GEN-060", "API-040", "CMN-041"],
          "A VC maximum of 6 without FECF and without SDLS: one data octet fits", "config",
          config_=cfg(fecf=False, vcs=[vc(1, max_len=6)], maps=[]))
    return b.cases


def gen_clcw():
    b = Builder()

    def word(**f):
        n = 0
        for name, width in CLCW_FIELDS:
            n = (n << width) | f.get(name, 0)
        return n.to_bytes(4, "big")

    def enc(n, desc, reqs, **p):
        params = {"status": 0, "cop_in_effect": 1, "vc_id": 0, "no_rf_available": False,
                  "no_bit_lock": False, "lockout": False, "wait": False, "retransmit": False,
                  "farm_b_counter": 0, "report_value": 0}
        params.update(p)
        b.add(f"tc-clcw-enc-{n:03}", reqs + ["TV-002"], desc, "clcw_encode", params=params)

    def dec(n, desc, reqs, data, mode="strict"):
        b.add(f"tc-clcw-dec-{n:03}", reqs, desc, "clcw_decode", mode=mode, data=data)

    dec(1, "COP-1, VCID 5, Retransmit, FARM-B 2, N(R) 42 (the CLCW of core/ocf.json)",
        ["TC-030"], word(cop_in_effect=1, vc_id=5, retransmit=1, farm_b_counter=2,
                         report_value=42))
    dec(2, "Every flag set, VCID 63, N(R) 255", ["TC-030"],
        word(cop_in_effect=1, vc_id=63, no_rf_available=1, no_bit_lock=1, lockout=1, wait=1,
             retransmit=1, farm_b_counter=3, report_value=255))
    dec(3, "Status Field 101 is passed through", ["TC-031"],
        word(status=0b101, cop_in_effect=1, vc_id=1, report_value=7))
    dec(4, "COP in Effect 00 is decoded and exposed, not rejected", ["TC-030"],
        word(cop_in_effect=0, vc_id=2))
    dec(5, "COP in Effect 11 is decoded and exposed", ["TC-030"], word(cop_in_effect=3))
    dec(6, "Control Word Type 1: not a CLCW, InvalidField", ["TC-030"],
        word(control_word_type=1, cop_in_effect=1))
    dec(7, "Control Word Type 1 in lenient mode: still InvalidField", ["TC-030", "API-032"],
        word(control_word_type=1, cop_in_effect=1), mode="lenient")
    dec(8, "Version 01: UnsupportedVersion", ["TC-030"], word(clcw_version=1, cop_in_effect=1))
    dec(9, "Version 11 in lenient mode: UnsupportedVersion", ["TC-030", "API-032"],
        word(clcw_version=3, cop_in_effect=1), mode="lenient")
    dec(10, "Spare bits 14-15 = 10, strict: ReservedValue", ["TC-030"],
        word(cop_in_effect=1, spare_14_15=2))
    dec(11, "Spare bit 23 = 1, strict: ReservedValue", ["TC-030"],
        word(cop_in_effect=1, spare_23=1))
    dec(12, "Spare bits 14-15 = 01, lenient: ClcwSpareSet", ["TC-030", "API-032"],
        word(cop_in_effect=1, spare_14_15=1, report_value=3), mode="lenient")
    dec(13, "Spare bit 23 and bits 14-15 set, lenient: one ClcwSpareSet", ["TC-030", "API-032"],
        word(cop_in_effect=1, spare_14_15=3, spare_23=1, vc_id=9), mode="lenient")
    dec(14, "Status 111 with spare bits clear: passed through", ["TC-031"],
        word(status=7, cop_in_effect=1, lockout=1, wait=1))
    dec(15, "Spare bits 14-15 and bit 23 both set, strict: ReservedValue, the field order "
        "decides, so bits 14-15 (clcw_spare_14_15) come first", ["TC-030", "API-031"],
        word(cop_in_effect=1, spare_14_15=3, spare_23=1, vc_id=9))
    dec(16, "Spare bit 23 alone set, lenient: one ClcwSpareSet", ["TC-030", "API-032"],
        word(cop_in_effect=1, spare_23=1, report_value=5), mode="lenient")
    enc(1, "COP-1, VCID 5, Retransmit, FARM-B 2, N(R) 42", ["TC-030"], vc_id=5,
        retransmit=True, farm_b_counter=2, report_value=42)
    enc(2, "Every field at its largest", ["TC-030", "TC-031"], status=7, cop_in_effect=3,
        vc_id=63, no_rf_available=True, no_bit_lock=True, lockout=True, wait=True,
        retransmit=True, farm_b_counter=3, report_value=255)
    enc(3, "Status Field 010 is written as given", ["TC-031"], status=2, vc_id=12)
    enc(4, "Status Field 8 does not fit three bits: OutOfRange", ["TC-030", "API-001"],
        status=8)
    enc(5, "COP in Effect 4: OutOfRange", ["TC-030", "API-001"], cop_in_effect=4)
    enc(6, "VCID 64: OutOfRange", ["TC-030", "API-001"], vc_id=64)
    enc(7, "FARM-B Counter 4: OutOfRange", ["TC-030", "API-001"], farm_b_counter=4)
    return b.cases


FILES = {
    "ad.json": (gen_ad, "Type-AD frames: encode, decode, peek_length, SDLS slices"),
    "bd.json": (gen_bd, "Type-BD frames, N(S) as given (TC-008)"),
    "bc_unlock.json": (gen_bc_unlock, "Type-BC frames with Unlock"),
    "bc_setvr.json": (gen_bc_setvr, "Type-BC frames with Set V(R)"),
    "segment_header.json": (gen_segment_header, "Segment Headers, MAPs, per-MAP SDLS"),
    "max_length.json": (gen_max_length, "Frame length limits"),
    "delimit_multiple_frames_with_fill.json": (gen_delimit, "Frame Delimiting and Fill Removal"),
    "validation_failures.json": (gen_validation, "Frame Validation Checks"),
    "decode_errors.json": (gen_decode_errors, "Decoder errors and lenient anomalies"),
    "config.json": (gen_config, "Managed parameters"),
    "clcw.json": (gen_clcw, "Communications Link Control Word"),
}

NOTE = (
    "232.0-B-4 prints no example frame or CLCW. Hand-derived from 232.0-B-4 with Cor. 1: "
    "§4.1.2 (Primary Header bits 0-1 TFVN 00, 2 Bypass, 3 Control Command, 4-5 spare, 6-15 "
    "SCID, 16-21 VCID, 22-31 Frame Length = total - 1, 32-39 N(S); Table 4-1, flags 0/1 "
    "reserved), §4.1.3 (Segment Header: Sequence Flags 2 bits, Table 4-2, MAP ID 6 bits; "
    "Unlock = 00, Set V(R) = 82 00 V(R)), §4.1.4 (FECF: CRC with G(X) = X^16 + X^12 + X^5 + "
    "1, register preset to ones), §4.2.1 (CLCW fields and widths), §4.4.9.2-3 (delimiting "
    "and validation), §4.4.3.3 and §4.4.7.3 (invalid MAP and VC), §5 and §6.3, §6.6 (managed "
    "parameters, Security Header after the Segment Header). Check order, validation reason "
    "codes, the delimiter's valid header, and the managed parameters kept, as "
    "docs/standards/tc-frame.md states them. SCID 0x155 is the fictional SCID of the "
    "reference scenario (APP-026). Expected values computed by an independent Python model "
    "of those clauses, written before the Rust codec, which shares no code or method with "
    "it (the header is one 40-bit integer, the CRC is bitwise from G(X)). Error cases name "
    "the field code of 03 §9.1. The BC Unlock frame of tc-bc-unlock-enc-001 and the CLCW of "
    "tc-clcw-enc-001 are worked by hand, the CRC register octet by octet, in "
    "docs/verification/derivations/tc-frame.md. The model is committed as "
    "test-vectors/models/tc_frame.py (TV-023); `python3 -I test-vectors/models/tc_frame.py "
    "--check test-vectors/tc/*.json` recomputes every expected value. Author of the model: an "
    "AI agent, Claude Opus 5.5, recorded as co-author of commit afff003 (2026-10-23), whose "
    "message states that it wrote the model from 232.0-B-4 Cor. 1 before the Rust codec "
    "existed; the history records no briefing of the agent beyond that statement (TV-023). "
    "The cases added on 2026-10-25 (M2 WP-8: tc-ad-6, tc-ad-enc-err-001 and -002, tc-bd-4, "
    "tc-bc-unlock-7, tc-bc-setvr-12, tc-seg-7, -8, tc-seg-enc-err-005, tc-max-dec-004, "
    "tc-max-dec-err-002, tc-val-021 to 025, tc-dec-err-014, tc-cfg-014 to 017, "
    "tc-clcw-dec-015 and -016) were written by a second agent, Claude Sonnet 5.5, briefed to "
    "read only the Blue Book, docs/, and the vectors, never the implementation, and to "
    "extend this model."
)


def generate():
    out = {}
    for name, (gen, title) in FILES.items():
        out[name] = {"kind": "codec", "standard": "232.0-B-4 Cor. 1 — " + title,
                     "provenance": "hand-derived",
                     "provenance_note": NOTE + " This file covers " + title + ".",
                     "cases": gen()}
    return out


def dump(document):
    return json.dumps(document, indent=2, ensure_ascii=False) + "\n"


def check_field_codes(path):
    catalogue = os.path.join(os.path.dirname(path), "..", "core", "fields.json")
    if not os.path.exists(catalogue):
        return 0
    with open(catalogue, encoding="utf-8") as f:
        names = {e["code"]: e["name"] for e in json.load(f)["fields"]}
    bad = 0
    for name, code in F.items():
        if names.get(code) != name:
            print(f"{catalogue}: code {code} is {names.get(code)!r}, the model expects {name!r}")
            bad += 1
    return bad


def check(paths):
    generated = generate()
    bad = check_field_codes(paths[0]) if paths else 0
    for path in paths:
        with open(path, encoding="utf-8") as f:
            document = json.load(f)
        live = [c for c in document["cases"] if "superseded" not in c]
        for c in live:
            try:
                want = expect(c)
            except (KeyError, ValueError) as e:
                print(f"{path}: {c['id']}: {e!r}")
                bad += 1
                continue
            if c["expect"] != want:
                print(f"{path}: {c['id']}: expect {c['expect']}, the model gives {want}")
                bad += 1
        by_id = {c["id"]: c for c in document["cases"]}
        for g in generated.get(os.path.basename(path), {"cases": []})["cases"]:
            if g["id"] not in by_id:
                print(f"{path}: {g['id']}, written by the generator, is missing")
                bad += 1
            elif "superseded" not in by_id[g["id"]] and by_id[g["id"]] != g:
                print(f"{path}: {g['id']} differs from the generator (--write)")
                bad += 1
        print(f"{path}: {len(live)} case(s) checked against the model")
    return bad


def main(argv):
    if len(argv) >= 2 and argv[0] == "--check":
        bad = check(argv[1:])
        if bad:
            print(f"tc_frame: {bad} difference(s)")
            return 1
        return 0
    if len(argv) == 2 and argv[0] == "--write":
        for name, document in generate().items():
            with open(os.path.join(argv[1], name), "w", encoding="utf-8") as f:
                f.write(dump(document))
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
