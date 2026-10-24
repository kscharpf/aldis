"""Independent model of the TM Transfer Frame for the vector files test-vectors/tm/*.json
(TM-001...TM-011; M2 WP-2).

Sources: CCSDS 132.0-B-3 §4.1 (figures 4-1 to 4-4: field order and widths), §4.1.2.7
(Synchronization Flag, Packet Order Flag, Segment Length Identifier, First Header Pointer and
its two reserved values), §4.1.3 (Secondary Header: version 00, length minus one, 1 to 63 data
octets), §4.1.4.6 and Annex D (OID frames and the PN sequence), §4.1.6 (FECF), §5 Tables 5-1
to 5-3 (managed parameters), §6.3 (Security Header and Trailer around the data field); the
readings of docs/standards/tm-frame.md (TM-001...TM-011, the anomaly list) and the error kinds
and field codes of docs/03-api-model.md §9 and §9.1. Parameters and code, no Blue Book text.

Method, chosen to differ from the Rust codec (crates/aldis-tm): frames are assembled and taken
apart as strings of bits, field by field from the figure widths; the FECF is the remainder of
a polynomial division of Python integers, X^16 M(X) + X^(n-16) L(X) modulo G(X) (§4.1.6.2.2);
the OID data is the Fibonacci recurrence x[n] = x[n-1] ^ x[n-2] ^ x[n-22] ^ x[n-32] on a list
of bits seeded with 32 ones (§4.1.4.6.2.1, Annex D). The model was written from the Blue Book
and the specification, not from the Rust code, and must stay independent of it: never port
code from it, and never fix the model to agree with it.

Usage, from the repository root:

    python3 -I test-vectors/models/tm_frame.py --check test-vectors/tm/*.json
    python3 -I test-vectors/models/tm_frame.py --write test-vectors/tm

--check recomputes the expected outcome of every live case from its operation, configuration,
parameters, and input, compares the cases the generator writes with the file's, and checks
the field codes against test-vectors/core/fields.json; it exits 1 on any difference. --write
regenerates the six files (cases and provenance notes).
"""
import json
import os
import sys

# Field codes of docs/03-api-model.md §9.1.
F = {
    "spacecraft_id": 1, "vc_id": 2, "frame_data_field": 7, "security_header_len": 8,
    "security_trailer_len": 9, "frame_version": 67, "frame_len": 68, "ocf_flag": 69,
    "fsh_flag": 70, "sync_flag": 71, "packet_order_flag": 72, "segment_length_id": 73,
    "first_header_pointer": 74, "fsh_version": 75, "fsh_len": 76,
}

# Primary Header, figure 4-2 and 4-3: (name, width in bits), in transmission order.
PRIMARY = [("version", 2), ("scid", 10), ("vcid", 3), ("ocf_flag", 1), ("mc_frame_count", 8),
           ("vc_frame_count", 8), ("fsh_flag", 1), ("sync_flag", 1), ("packet_order_flag", 1),
           ("segment_length_id", 2), ("first_header_pointer", 11)]
NO_PACKET_START = 0b11111111111  # §4.1.2.7.6.4
ONLY_IDLE_DATA = 0b11111111110  # §4.1.2.7.6.5
SLID_SYNC0 = 0b11  # §4.1.2.7.5.2


class Fail(Exception):
    def __init__(self, kind, field=None):
        super().__init__(kind)
        self.kind = kind
        self.field = field


def to_bits(value, width):
    assert 0 <= value < (1 << width), (value, width)
    return format(value, f"0{width}b")


def octets_of(bits):
    assert len(bits) % 8 == 0
    return bytes(int(bits[i:i + 8], 2) for i in range(0, len(bits), 8))


def bits_of(octets):
    return "".join(format(b, "08b") for b in octets)


def split_primary(octets):
    bits = bits_of(octets[:6])
    out, at = {}, 0
    for name, width in PRIMARY:
        out[name] = int(bits[at:at + width], 2)
        at += width
    return out


def join_primary(fields):
    return octets_of("".join(to_bits(int(fields[name]), width) for name, width in PRIMARY))


def crc_fecf(message):
    """§4.1.6.2.2: FECF = (X^16 M(X) + X^(n-16) L(X)) mod G(X), with L(X) sixteen ones."""
    g = (1 << 16) | (1 << 12) | (1 << 5) | 1
    k = len(message) * 8
    dividend = (int.from_bytes(message, "big") << 16) ^ (0xFFFF << k)
    while dividend.bit_length() > 16:
        dividend ^= g << (dividend.bit_length() - 17)
    return dividend


class Oid:
    """Annex D Fibonacci form: 32 ones, then x[n] = x[n-1]^x[n-2]^x[n-22]^x[n-32]."""

    def __init__(self):
        self.x = [1] * 32
        self.at = 0

    def octets(self, n):
        out = []
        for _ in range(n):
            byte = 0
            for _ in range(8):
                while len(self.x) <= self.at:
                    m = len(self.x)
                    self.x.append(self.x[m - 1] ^ self.x[m - 2] ^ self.x[m - 22] ^ self.x[m - 32])
                byte = (byte << 1) | self.x[self.at]
                self.at += 1
            out.append(byte)
        return bytes(out)


# ---- configuration (Tables 5-1 to 5-3, 6-1), built in the handler's order

def build_config(cfg):
    """The configuration, or Fail. Keys: frame_len, fecf, and for a master channel scid, mc_fsh,
    mc_ocf, vcs (vcid, content, fsh, ocf, security_header_len, security_trailer_len)."""
    fecf = cfg["fecf"]
    minimum = 6 + 1 + (2 if fecf else 0)
    if cfg["frame_len"] < minimum:
        raise Fail("InvalidConfig", F["frame_len"])
    pc = {"frame_len": cfg["frame_len"], "fecf": fecf}
    if "scid" not in cfg:
        return pc, None
    if cfg["scid"] > 1023:
        raise Fail("OutOfRange", F["spacecraft_id"])
    mc_fsh = cfg.get("mc_fsh")
    if mc_fsh is not None and not 2 <= mc_fsh <= 64:
        raise Fail("OutOfRange", F["fsh_len"])
    mc = {"scid": cfg["scid"], "fsh": mc_fsh, "ocf": cfg.get("mc_ocf", False), "vcs": {}}
    for vc in cfg.get("vcs", []):
        if vc["vcid"] > 7:
            raise Fail("OutOfRange", F["vc_id"])
        fsh = vc.get("fsh")
        if fsh is not None and not 2 <= fsh <= 64:
            raise Fail("OutOfRange", F["fsh_len"])
        sh, st = vc.get("security_header_len", 0), vc.get("security_trailer_len", 0)
        if sh > 64:
            raise Fail("InvalidConfig", F["security_header_len"])
        if st > 0xFFFF:
            raise Fail("InvalidConfig", F["security_trailer_len"])
        if mc_fsh is not None and fsh is not None:
            raise Fail("InvalidConfig", F["fsh_len"])
        if mc["ocf"] and vc.get("ocf", False):
            raise Fail("InvalidConfig", F["ocf_flag"])
        entry = {"content": vc["content"], "fsh": mc_fsh if mc_fsh is not None else fsh,
                 "ocf": mc["ocf"] or vc.get("ocf", False), "sh": sh, "st": st,
                 "own_fsh": fsh, "own_ocf": vc.get("ocf", False)}
        if data_len(pc, entry["fsh"] or 0, sh, st, entry["ocf"]) < 1:
            raise Fail("InvalidConfig", F["frame_data_field"])
        if vc["vcid"] in mc["vcs"]:
            raise Fail("InvalidConfig", F["vc_id"])
        mc["vcs"][vc["vcid"]] = entry
    return pc, mc


def data_len(pc, fsh, sh, st, ocf):
    """§4.1.4.2 and §6.3.5.2 b."""
    return pc["frame_len"] - 6 - fsh - sh - st - (4 if ocf else 0) - (2 if pc["fecf"] else 0)


# ---- decoding (TM-001...TM-008), checks in the order of docs/standards/tm-frame.md

def report(ocf):
    bit0, bit1 = ocf[0] >> 7, (ocf[0] >> 6) & 1
    return "clcw" if bit0 == 0 else ("project_specific" if bit1 == 0 else "sdls_fsr")


def decode(octets, pc, mc, mode, check_fecf, lookup):
    if len(octets) != pc["frame_len"]:
        raise Fail("LengthMismatch", F["frame_len"])
    if check_fecf and pc["fecf"] and crc_fecf(octets[:-2]) != int.from_bytes(octets[-2:], "big"):
        raise Fail("CrcMismatch")
    h = split_primary(octets)
    if h["version"] != 0:
        raise Fail("UnsupportedVersion", F["frame_version"])
    sh = st = 0
    if lookup:
        if h["scid"] != mc["scid"]:
            raise Fail("UnknownChannel", F["spacecraft_id"])
        if h["vcid"] not in mc["vcs"]:
            raise Fail("UnknownChannel", F["vc_id"])
        sh, st = mc["vcs"][h["vcid"]]["sh"], mc["vcs"][h["vcid"]]["st"]
    fsh = 0
    if h["fsh_flag"]:
        ident = bits_of(octets[6:7])
        if int(ident[:2], 2) != 0:
            raise Fail("UnsupportedVersion", F["fsh_version"])
        fsh = int(ident[2:], 2) + 1
        if fsh < 2:
            raise Fail("InvalidField", F["fsh_len"])
        if 6 + fsh > len(octets):
            raise Fail("Malformed", F["fsh_len"])
    n = data_len(pc, fsh, sh, st, h["ocf_flag"])
    if n < 1:
        raise Fail("Malformed", F["frame_data_field"])
    anomalies, pointer, offset = [], None, None
    if not h["sync_flag"]:
        fhp = h["first_header_pointer"]
        if fhp == NO_PACKET_START:
            pointer = "no_packet_start"
        elif fhp == ONLY_IDLE_DATA:
            pointer = "only_idle_data"
        elif fhp < n:
            pointer, offset = "starts_at", fhp
        else:
            raise Fail("Malformed", F["first_header_pointer"])
        if h["packet_order_flag"]:
            if mode == "strict":
                raise Fail("ReservedValue", F["packet_order_flag"])
            anomalies.append("PacketOrderFlagSet")
        if h["segment_length_id"] != SLID_SYNC0:
            if mode == "strict":
                raise Fail("InvalidField", F["segment_length_id"])
            anomalies.append("SegmentLengthIdNot11")
    at = 6
    fields = {}
    for name, length in [("fsh", fsh), ("sh", sh), ("data", n), ("st", st),
                         ("ocf", 4 if h["ocf_flag"] else 0), ("fecf", 2 if pc["fecf"] else 0)]:
        fields[name] = octets[at:at + length]
        at += length
    assert at == len(octets)
    return {
        "scid": h["scid"], "vcid": h["vcid"], "ocf_flag": bool(h["ocf_flag"]),
        "mc_frame_count": h["mc_frame_count"], "vc_frame_count": h["vc_frame_count"],
        "fsh_flag": bool(h["fsh_flag"]), "sync_flag": bool(h["sync_flag"]),
        "packet_order_flag": bool(h["packet_order_flag"]),
        "segment_length_id": h["segment_length_id"],
        "first_header_pointer": h["first_header_pointer"],
        "pointer": pointer, "packet_offset": offset,
        "fsh_len": fsh or None,
        "fsh_data_hex": fields["fsh"][1:].hex().upper() if fsh else None,
        "security_header_hex": fields["sh"].hex().upper(),
        "data_hex": fields["data"].hex().upper(),
        "security_trailer_hex": fields["st"].hex().upper(),
        "ocf_hex": fields["ocf"].hex().upper() if h["ocf_flag"] else None,
        "ocf_report": report(fields["ocf"]) if h["ocf_flag"] else None,
        "fecf": int.from_bytes(fields["fecf"], "big") if pc["fecf"] else None,
        "consumed": len(octets), "anomalies": anomalies,
    }


# ---- validation against the master channel (TM-011)

def validate(octets, pc, mc, mode):
    # Decoded with the security layout of the frame's VC from the master channel: a check that
    # takes a master channel always uses its layouts (c-abi.md §11, maintainer decision on #94).
    v = decode(octets, pc, mc, mode, True, True)
    if v["scid"] != mc["scid"]:
        raise Fail("UnknownChannel", F["spacecraft_id"])
    if v["vcid"] not in mc["vcs"]:
        raise Fail("UnknownChannel", F["vc_id"])
    vc = mc["vcs"][v["vcid"]]
    if v["ocf_flag"] != vc["ocf"]:
        raise Fail("InvalidField", F["ocf_flag"])
    if v["fsh_flag"] != (vc["fsh"] is not None):
        raise Fail("InvalidField", F["fsh_flag"])
    if v["sync_flag"] != (vc["content"] == "vca_sdu"):
        raise Fail("InvalidField", F["sync_flag"])
    if (v["fsh_len"] or 0) != (vc["fsh"] or 0):
        raise Fail("LengthMismatch", F["fsh_len"])
    if len(v["security_header_hex"]) // 2 != vc["sh"]:
        raise Fail("LengthMismatch", F["security_header_len"])
    if len(v["security_trailer_hex"]) // 2 != vc["st"]:
        raise Fail("LengthMismatch", F["security_trailer_len"])
    return {}


# ---- encoding (TM-009, TM-010)

def plan(pc, mc, p):
    vcid = p["vcid"]
    if vcid not in mc["vcs"]:
        raise Fail("UnknownChannel", F["vc_id"])
    vc = mc["vcs"][vcid]
    status = p["status"]
    if status["sync"] != (vc["content"] == "vca_sdu"):
        raise Fail("InvalidField", F["sync_flag"])
    n = data_len(pc, vc["fsh"] or 0, vc["sh"], vc["st"], vc["ocf"])
    if status["sync"]:
        pof, slid, fhp = (status["packet_order_flag"], status["segment_length_id"],
                          status["first_header_pointer"])
    else:
        pof, slid = False, SLID_SYNC0
        kind = status["pointer"]
        if kind == "starts_at":
            fhp = status["packet_offset"]
            if fhp > min(n, ONLY_IDLE_DATA) - 1:
                raise Fail("OutOfRange", F["first_header_pointer"])
        else:
            fhp = NO_PACKET_START if kind == "no_packet_start" else ONLY_IDLE_DATA
    header = join_primary({
        "version": 0, "scid": mc["scid"], "vcid": vcid, "ocf_flag": vc["ocf"],
        "mc_frame_count": p["mc_frame_count"], "vc_frame_count": p["vc_frame_count"],
        "fsh_flag": vc["fsh"] is not None, "sync_flag": status["sync"],
        "packet_order_flag": pof, "segment_length_id": slid, "first_header_pointer": fhp,
    })
    return vc, n, header


def assemble(pc, vc, header, fsh_data, sh, data, st, ocf):
    body = header
    if vc["fsh"] is not None:
        body += octets_of("00" + to_bits(vc["fsh"] - 1, 6)) + fsh_data
    body += sh + data + st + (ocf if ocf is not None else b"")
    if pc["fecf"]:
        body += crc_fecf(body).to_bytes(2, "big")
    assert len(body) == pc["frame_len"]
    return body


def hexb(p, key):
    value = p.get(key)
    return None if value is None else bytes.fromhex(value)


def encode(pc, mc, p):
    vc, n, header = plan(pc, mc, p)
    content = [("fsh_data_hex", (vc["fsh"] or 1) - 1, "fsh_len"),
               ("security_header_hex", vc["sh"], "security_header_len"),
               ("data_hex", n, "frame_data_field"),
               ("security_trailer_hex", vc["st"], "security_trailer_len")]
    for key, length, field in content:
        if len(hexb(p, key) or b"") != length:
            raise Fail("LengthMismatch", F[field])
    if (p.get("ocf_hex") is not None) != vc["ocf"]:
        raise Fail("InvalidField", F["ocf_flag"])
    if p.get("out_len", pc["frame_len"]) < pc["frame_len"]:
        raise Fail("BufferTooSmall")
    frame = assemble(pc, vc, header, hexb(p, "fsh_data_hex") or b"",
                     hexb(p, "security_header_hex") or b"", hexb(p, "data_hex"),
                     hexb(p, "security_trailer_hex") or b"", hexb(p, "ocf_hex"))
    return {"output_hex": frame.hex().upper()}


def encode_oid(pc, mc, p):
    """§4.1.4.6: one generator for the whole sequence of frames (CMN-021)."""
    oid = Oid()
    frames = []
    for f in p["frames"]:
        q = {"vcid": p["vcid"], "mc_frame_count": f["mc_frame_count"],
             "vc_frame_count": f["vc_frame_count"],
             "status": {"sync": False, "pointer": "only_idle_data"}}
        vc, n, header = plan(pc, mc, q)
        fsh_data = hexb(f, "fsh_data_hex") or bytes((vc["fsh"] or 1) - 1)
        ocf = hexb(f, "ocf_hex") if vc["ocf"] else None
        if vc["ocf"] and ocf is None:
            ocf = bytes(4)
        frame = assemble(pc, vc, header, fsh_data, bytes(vc["sh"]), oid.octets(n),
                         bytes(vc["st"]), ocf)
        frames.append(frame.hex().upper())
    return {"frames_hex": frames}


def expect(case):
    """The expected outcome of a case, from the model alone."""
    op = case["operation"]
    mode = case.get("mode", "strict")
    p = case.get("params", {})
    octets = bytes.fromhex(case.get("input_hex", ""))
    try:
        pc, mc = build_config(case["config"])
        if op == "config":
            out = {}
        elif op == "peek_length":
            if octets and octets[0] >> 6 != 0:
                raise Fail("UnsupportedVersion", F["frame_version"])
            out = {"len": pc["frame_len"]}
        elif op == "decode":
            out = decode(octets, pc, mc, mode, True, False)
        elif op == "decode_with":
            out = decode(octets, pc, mc, mode, True, True)
        elif op == "decode_unchecked":
            out = decode(octets, pc, mc, mode, False, False)
        elif op == "validate":
            out = validate(octets, pc, mc, mode)
        elif op == "encode":
            out = encode(pc, mc, p)
        elif op == "encode_oid":
            out = encode_oid(pc, mc, p)
        else:
            raise ValueError(f"{case['id']}: the model has no operation {op!r}")
    except Fail as e:
        res = {"ok": False, "error": e.kind}
        if e.field is not None:
            res["field"] = e.field
        return res
    return {"ok": True, **out}


# ---- the generator: how the inputs of the vector files were chosen

def frame(pc_len, fecf, fields, fsh=None, sh=b"", data=b"", st=b"", ocf=None):
    """Assembles raw input octets, any field values allowed (for decoder inputs)."""
    h = {"version": 0, "ocf_flag": ocf is not None, "mc_frame_count": 0, "vc_frame_count": 0,
         "fsh_flag": fsh is not None, "sync_flag": 0, "packet_order_flag": 0,
         "segment_length_id": 3, "first_header_pointer": 0}
    h.update(fields)
    body = join_primary(h)
    if fsh is not None:
        body += fsh
    body += sh + data + st + (ocf or b"")
    if fecf:
        body += crc_fecf(body).to_bytes(2, "big")
    assert len(body) == pc_len, (len(body), pc_len)
    return body


def seq(start, n):
    return bytes((start + i) & 0xFF for i in range(n))


def pc_only(frame_len, fecf):
    return {"frame_len": frame_len, "fecf": fecf}


def channel(frame_len, fecf, scid, vcs, mc_fsh=None, mc_ocf=False):
    return {"frame_len": frame_len, "fecf": fecf, "scid": scid, "mc_fsh": mc_fsh,
            "mc_ocf": mc_ocf, "vcs": vcs}


def vc(vcid, content="packets", fsh=None, ocf=False, sh=0, st=0):
    return {"vcid": vcid, "content": content, "fsh": fsh, "ocf": ocf,
            "security_header_len": sh, "security_trailer_len": st}


def status(pointer=None, offset=None, sync=False, pof=False, slid=3, fhp=0):
    if sync:
        return {"sync": True, "packet_order_flag": pof, "segment_length_id": slid,
                "first_header_pointer": fhp}
    s = {"sync": False, "pointer": pointer}
    if pointer == "starts_at":
        s["packet_offset"] = offset
    return s


def enc(vcid, mcc, vcc, st, data, fsh_data="", sh="", tr="", ocf=None, **extra):
    p = {"vcid": vcid, "mc_frame_count": mcc, "vc_frame_count": vcc, "status": st,
         "fsh_data_hex": fsh_data, "security_header_hex": sh, "data_hex": data,
         "security_trailer_hex": tr, "ocf_hex": ocf}
    p.update(extra)
    return p


def superseded(c, op, expect_, reason, by):
    """A superseded case as it was released: its operation and expectation are kept, and the
    model does not recompute them (TV-021)."""
    c["operation"] = op
    c["expect"] = expect_
    c["superseded"] = {"reason": reason, "by": by}
    return c


def case(cid, reqs, desc, op, config, input_hex=None, params=None, mode=None):
    c = {"id": cid, "requirements": reqs, "description": desc, "operation": op}
    if mode is not None:
        c["mode"] = mode
    c["config"] = config
    if params is not None:
        c["params"] = params
    if input_hex is not None:
        c["input_hex"] = input_hex.hex().upper() if isinstance(input_hex, bytes) else input_hex
    c["expect"] = expect(c)
    return c


SCID = 0x2A5
CLCW = bytes.fromhex("01140C2A")  # the CLCW of core/ocf.json, ocf-classify-001


def minimal():
    pc = pc_only(16, False)
    plain = frame(16, False, {"scid": SCID, "vcid": 5, "mc_frame_count": 1, "vc_frame_count": 2},
                  data=seq(0xD0, 10))
    mc = channel(16, False, SCID, [vc(5)])
    return [
        case("tm-min-dec-001", ["TM-001", "TM-002", "TM-006", "TM-008", "API-010", "API-012",
                                "GEN-013"],
             "16-octet frame, no FSH, OCF, FECF, or SDLS: 6-octet header, 10 data octets, FHP 0",
             "decode", pc, plain),
        case("tm-min-dec-002", ["TM-001", "TM-008", "API-010"],
             "Highest SCID and VCID, counts 255, FHP 9 (the last data octet)", "decode", pc,
             frame(16, False, {"scid": 1023, "vcid": 7, "mc_frame_count": 255,
                               "vc_frame_count": 255, "first_header_pointer": 9},
                   data=bytes(10))),
        case("tm-min-dec-003", ["TM-001", "TM-006", "API-012"],
             "Shortest frame without FECF: 7 octets, one data octet", "decode", pc_only(7, False),
             frame(7, False, {"scid": 0, "vcid": 0}, data=b"\x5A")),
        case("tm-min-peek-001", ["API-013"], "peek_length returns the configured length",
             "peek_length", pc_only(1115, True), "00"),
        case("tm-min-peek-002", ["API-013"], "peek_length of an empty prefix", "peek_length",
             pc_only(16, False), ""),
        case("tm-min-enc-001", ["TM-009", "API-020", "API-021"],
             "Encode the frame of tm-min-dec-001", "encode", mc,
             params=enc(5, 1, 2, status("starts_at", 0), seq(0xD0, 10).hex().upper())),
        case("tm-min-enc-002", ["TM-009", "API-020"],
             "Encode with no packet start (FHP 0x7FF)", "encode", mc,
             params=enc(5, 0x80, 0x7F, status("no_packet_start"), "AA" * 10)),
        case("tm-min-val-001", ["TM-011"], "The frame of tm-min-dec-001 matches its channel",
             "validate", mc, plain),
        case("tm-min-cfg-001", ["API-040", "TM-001"], "Physical channel of 7 octets, no FECF",
             "config", pc_only(7, False)),
        case("tm-min-cfg-002", ["API-040", "TM-011"],
             "Master channel with all eight VCs, packets and VCA_SDUs", "config",
             channel(64, True, 1, [vc(i, "packets" if i % 2 == 0 else "vca_sdu")
                                   for i in range(8)])),
    ]


def fsh_ocf_fecf():
    pc = pc_only(40, True)
    vc_fsh = channel(40, True, SCID, [vc(1, fsh=4, ocf=True), vc(2, "vca_sdu")])
    mc_fsh = channel(40, True, SCID, [vc(0), vc(3, "vca_sdu")], mc_fsh=6, mc_ocf=True)
    fsh4 = bytes([0x03]) + b"\x11\x22\x33"
    f1 = frame(40, True, {"scid": SCID, "vcid": 1, "mc_frame_count": 3, "vc_frame_count": 4,
                          "first_header_pointer": 2},
               fsh=fsh4, data=seq(0x40, 40 - 6 - 4 - 4 - 2), ocf=CLCW)
    cases = [
        case("tm-fof-dec-001", ["TM-003", "TM-004", "TM-005", "TM-006", "API-010", "CMN-010"],
             "4-octet FSH, CLCW in the OCF, FECF: every field placed", "decode", pc, f1),
        case("tm-fof-dec-002", ["TM-004", "CMN-010"], "OCF holding a project-specific report",
             "decode", pc,
             frame(40, True, {"scid": SCID, "vcid": 2}, data=bytes(28), ocf=bytes.fromhex("80ABCDEF"))),
        case("tm-fof-dec-003", ["TM-004", "CMN-010"], "OCF holding an SDLS FSR", "decode", pc,
             frame(40, True, {"scid": SCID, "vcid": 2}, data=bytes(28), ocf=bytes.fromhex("C0000001"))),
        case("tm-fof-dec-004", ["TM-003"], "Longest FSH: 64 octets, length field 0x3F", "decode",
             pc_only(80, False),
             frame(80, False, {"scid": 1, "vcid": 1}, fsh=bytes([0x3F]) + seq(1, 63),
                   data=seq(0xE0, 10))),
        case("tm-fof-dec-005", ["TM-003"], "Shortest FSH: 2 octets, length field 1", "decode",
             pc_only(12, True),
             frame(12, True, {"scid": 1, "vcid": 1}, fsh=b"\x01\x99", data=b"\x01\x02")),
        case("tm-fof-dec-006", ["TM-005", "CMN-003"],
             "decode_unchecked accepts the frame of tm-fof-dec-001 with a corrupted data octet",
             "decode_unchecked", pc, f1[:20] + bytes([f1[20] ^ 0xFF]) + f1[21:]),
        case("tm-fof-enc-001", ["TM-009", "API-020", "API-021"],
             "Encode the frame of tm-fof-dec-001 from its VC_FSH and VC_OCF configuration",
             "encode", vc_fsh,
             params=enc(1, 3, 4, status("starts_at", 2), seq(0x40, 24).hex().upper(),
                        fsh_data="112233", ocf=CLCW.hex().upper())),
        case("tm-fof-enc-002", ["TM-009", "API-021"],
             "MC_FSH of 6 octets and MC_OCF on a VCA_SDU channel", "encode", mc_fsh,
             params=enc(3, 9, 9, status(sync=True, slid=3, fhp=NO_PACKET_START), "77" * 22,
                        fsh_data=seq(0xA0, 5).hex().upper(), ocf="80000000")),
        case("tm-fof-val-002", ["TM-011"], "A frame of the master channel of tm-fof-enc-002",
             "validate", mc_fsh,
             frame(40, True, {"scid": SCID, "vcid": 0}, fsh=b"\x05" + bytes(5),
                   data=bytes(22), ocf=CLCW)),
        case("tm-fof-val-001", ["TM-011"], "The frame of tm-fof-dec-001 matches its channel",
             "validate", vc_fsh, f1),
    ]
    return cases


def fhp_values():
    pc = pc_only(20, False)
    mc = channel(20, False, SCID, [vc(0), vc(1, "vca_sdu")])
    data = seq(0x10, 14)

    def f(fhp, sync=0, pof=0, slid=3):
        return frame(20, False, {"scid": SCID, "vcid": sync, "first_header_pointer": fhp,
                                 "sync_flag": sync, "packet_order_flag": pof,
                                 "segment_length_id": slid}, data=data)

    cases = []
    for i, (fhp, desc) in enumerate([
            (0, "FHP 0: a packet starts at the first data octet"),
            (13, "FHP 13: the last data octet"),
            (NO_PACKET_START, "FHP 0x7FF: no packet starts"),
            (ONLY_IDLE_DATA, "FHP 0x7FE: only idle data (OID frame)")], 1):
        cases.append(case(f"tm-fhp-dec-{i:03}", ["TM-008"], desc, "decode", pc, f(fhp)))
    for i, (mode, fhp) in enumerate([("strict", 14), ("lenient", 14), ("strict", 0x7FD),
                                     ("lenient", 0x400)], 1):
        cases.append(case(f"tm-fhp-err-{i:03}", ["TM-008", "API-032"],
                          f"FHP {fhp:#x} outside a 14-octet data field: Malformed in {mode} mode",
                          "decode", pc, f(fhp), mode=mode))
    for i, (pof, slid, fhp) in enumerate([(0, 3, 14), (1, 0, 0x7FD), (1, 2, NO_PACKET_START)], 1):
        cases.append(case(f"tm-fhp-vca-{i:03}", ["TM-007", "TM-008"],
                          f"Sync Flag 1: POF {pof}, SLID {slid}, FHP {fhp:#x} exposed raw",
                          "decode", pc, f(fhp, sync=1, pof=pof, slid=slid)))
    for i, (st, desc) in enumerate([
            (status("starts_at", 13), "Encode FHP 13, the last data octet"),
            (status("only_idle_data"), "Encode FHP 0x7FE"),
            (status("no_packet_start"), "Encode FHP 0x7FF")], 1):
        cases.append(case(f"tm-fhp-enc-{i:03}", ["TM-009", "TM-008"], desc, "encode", mc,
                          params=enc(0, 0, 0, st, data.hex().upper())))
    cases.append(case("tm-fhp-enc-004", ["TM-009", "TM-007"],
                      "Encode Sync Flag 1 with POF 1, SLID 01, FHP 0x123", "encode", mc,
                      params=enc(1, 0, 0, status(sync=True, pof=True, slid=1, fhp=0x123),
                                 data.hex().upper())))
    cases.append(case("tm-fhp-enc-err-001", ["TM-009"],
                      "Encode FHP 14 past a 14-octet data field: OutOfRange", "encode", mc,
                      params=enc(0, 0, 0, status("starts_at", 14), data.hex().upper())))
    big = channel(2100, False, 1, [vc(0)])
    cases.append(case("tm-fhp-enc-err-002", ["TM-009", "TM-008"],
                      "Encode FHP 0x7FE as an offset in a 2094-octet data field: OutOfRange",
                      "encode", big,
                      params=enc(0, 0, 0, status("starts_at", 0x7FE), "00" * 2094)))
    cases.append(case("tm-fhp-enc-005", ["TM-009", "TM-008"],
                      "Encode FHP 0x7FD, the largest offset, in a 2094-octet data field",
                      "encode", big,
                      params=enc(0, 0, 0, status("starts_at", 0x7FD), "00" * 2094)))
    return cases


def oid_sequence():
    mc = channel(32, True, 0x0AB, [vc(0, fsh=4, sh=2, st=1), vc(1, "vca_sdu"), vc(2, ocf=True)],
                 mc_ocf=False)
    return [
        case("tm-oid-001", ["TM-010", "CMN-020", "CMN-021", "CMN-023"],
             "Two consecutive OID frames on VC 2 continue the PN sequence: the first starts with "
             "FF FF FF FF 6D B6 D8 61 45 1F (Annex D)", "encode_oid", mc,
             params={"vcid": 2, "frames": [
                 {"mc_frame_count": 0, "vc_frame_count": 0, "ocf_hex": CLCW.hex().upper()},
                 {"mc_frame_count": 1, "vc_frame_count": 1, "ocf_hex": CLCW.hex().upper()}]}),
        case("tm-oid-002", ["TM-010", "CMN-021"],
             "Three OID frames on VC 0 with an FSH carrying data and SDLS fields left zero",
             "encode_oid", mc,
             params={"vcid": 0, "frames": [
                 {"mc_frame_count": i, "vc_frame_count": 10 + i, "fsh_data_hex": "ABCDEF"}
                 for i in range(3)]}),
        case("tm-oid-003", ["TM-010"],
             "An OID frame on a VCA_SDU channel is rejected: OID frames use a packet VC",
             "encode_oid", mc, params={"vcid": 1, "frames": [
                 {"mc_frame_count": 0, "vc_frame_count": 0}]}),
        case("tm-oid-004", ["TM-008", "TM-010"],
             "The first frame of tm-oid-001 decodes as only idle data", "decode",
             pc_only(32, True), expect({"operation": "encode_oid", "config": mc, "params": {
                 "vcid": 2, "frames": [{"mc_frame_count": 0, "vc_frame_count": 0,
                                        "ocf_hex": CLCW.hex().upper()}]}})["frames_hex"][0]),
    ]


def security_layout():
    mc = channel(40, True, SCID, [vc(4, sh=6, st=8), vc(5, "vca_sdu", ocf=True, sh=3)])
    pattern = frame(40, True, {"scid": SCID, "vcid": 4, "first_header_pointer": 0},
                    sh=b"\xA5" * 6, data=seq(0x30, 40 - 6 - 6 - 8 - 2), st=b"\x5A" * 8)
    return [
        case("tm-sec-dec-001", ["API-070", "API-072", "TM-006"],
             "Security Header of 6 octets (A5) and Trailer of 8 (5A), FixedPatternSecurity: "
             "the data field is 18 octets", "decode_with", mc, pattern),
        case("tm-sec-dec-002", ["API-070", "TM-006"],
             "The same octets without the lookup: a 32-octet data field", "decode",
             pc_only(40, True), pattern),
        case("tm-sec-dec-003", ["API-070", "TM-008"],
             "FHP 18 points past the 18-octet data field of the SDLS channel: Malformed",
             "decode_with", mc,
             frame(40, True, {"scid": SCID, "vcid": 4, "first_header_pointer": 18},
                   sh=b"\xA5" * 6, data=bytes(18), st=b"\x5A" * 8)),
        case("tm-sec-dec-004", ["API-070"], "VC 6 is not configured: UnknownChannel",
             "decode_with", mc, frame(40, True, {"scid": SCID, "vcid": 6}, data=bytes(32))),
        case("tm-sec-dec-005", ["API-070"], "SCID 0x2A4 is not configured: UnknownChannel",
             "decode_with", mc, frame(40, True, {"scid": SCID - 1, "vcid": 4}, data=bytes(32))),
        case("tm-sec-enc-001", ["API-070", "API-072", "TM-009"],
             "Encode the frame of tm-sec-dec-001 with the hook's patterns", "encode", mc,
             params=enc(4, 0, 0, status("starts_at", 0), seq(0x30, 18).hex().upper(),
                        sh="A5" * 6, tr="5A" * 8)),
        case("tm-sec-enc-002", ["API-070", "TM-009"],
             "VCA_SDU channel with a 3-octet Security Header and an OCF", "encode", mc,
             params=enc(5, 1, 1, status(sync=True, fhp=NO_PACKET_START), "C3" * 25,
                        sh="A5A5A5", ocf="00000000")),
        superseded(
            case("tm-sec-val-001", ["TM-011", "API-070"],
                 "Decoded without SDLS, the frame of tm-sec-dec-001 does not match VC 4's layout",
                 "config", mc, pattern),
            "validate", {"ok": False, "error": "LengthMismatch", "field": F["security_header_len"]},
            "the validate operation decodes with the master channel's security layouts "
            "(maintainer decision on #94), so a frame cannot be validated as decoded without SDLS",
            "tm-sec-val-002"),
        case("tm-sec-val-002", ["TM-011", "API-070"],
             "Decoded with VC 4's layout, the frame of tm-sec-dec-001 matches its channel",
             "validate", mc, pattern),
        case("tm-sec-cfg-001", ["API-040", "API-071"],
             "Security Header longer than 64 octets", "config",
             channel(200, False, 1, [vc(0, sh=65)])),
        case("tm-sec-cfg-002", ["API-040", "CMN-041"],
             "SDLS fields leave no data octet", "config",
             channel(40, True, 1, [vc(0, sh=20, st=12)])),
    ]


def invalid():
    pc = pc_only(20, True)
    mc = channel(20, True, SCID, [vc(0), vc(1, "vca_sdu"), vc(2, fsh=3), vc(3, ocf=True)])

    def f(fields, **kw):
        base = {"scid": SCID, "vcid": 0}
        base.update(fields)
        kw.setdefault("data", seq(0x60, 20 - 6 - 2 - len(kw.get("fsh") or b"")
                                 - (4 if kw.get("ocf") else 0)))
        return frame(20, True, base, **kw)

    good = f({})
    cases = [
        case("tm-inv-len-001", ["TM-001", "API-012"], "One octet short: LengthMismatch",
             "decode", pc, good[:-1]),
        case("tm-inv-len-002", ["TM-001", "API-012"], "One octet long: LengthMismatch",
             "decode", pc, good + b"\x00"),
        case("tm-inv-crc-001", ["TM-005", "CMN-003"], "One data bit flipped: CrcMismatch",
             "decode", pc, good[:10] + bytes([good[10] ^ 1]) + good[11:]),
        case("tm-inv-crc-002", ["TM-005"],
             "Version 01 with a wrong FECF: the FECF is checked first", "decode", pc,
             bytes([good[0] | 0x40]) + good[1:]),
        case("tm-inv-ver-001", ["TM-002"], "Transfer Frame Version Number 01", "decode", pc,
             f({"version": 1})),
        case("tm-inv-ver-002", ["TM-002"], "Transfer Frame Version Number 11", "decode", pc,
             f({"version": 3})),
        case("tm-inv-ver-003", ["TM-002", "API-013"], "peek_length rejects version 01",
             "peek_length", pc, "40"),
        case("tm-inv-fsh-001", ["TM-003"], "FSH version 01", "decode", pc,
             f({}, fsh=b"\x42\x00\x00")),
        case("tm-inv-fsh-002", ["TM-003"], "FSH length field 0: no data octet", "decode", pc,
             f({}, fsh=b"\x00")),
        case("tm-inv-fsh-003", ["TM-003", "API-031"],
             "FSH of 15 octets runs past the 14 that follow the Primary Header", "decode",
             pc_only(20, False),
             frame(20, False, {"scid": SCID, "vcid": 0}, fsh=b"\x0E" + bytes(13))),
        case("tm-inv-fsh-004", ["TM-003", "TM-006", "CMN-041"],
             "FSH of 12 octets fits but leaves no data octet before the FECF", "decode", pc,
             f({}, fsh=b"\x0B" + bytes(11), data=b"")),
        case("tm-inv-ocf-001", ["TM-006", "CMN-041"],
             "OCF Flag set on a 12-octet frame: no data octet", "decode", pc_only(12, True),
             frame(12, True, {"scid": SCID, "vcid": 0}, ocf=b"\x00\x00\x00\x00")),
        case("tm-inv-pof-001", ["TM-007", "API-031"], "Sync 0, Packet Order Flag 1: strict",
             "decode", pc, f({"packet_order_flag": 1})),
        case("tm-inv-pof-002", ["TM-007", "API-032", "GEN-043"],
             "Sync 0, Packet Order Flag 1: lenient flags PacketOrderFlagSet", "decode", pc,
             f({"packet_order_flag": 1}), mode="lenient"),
        case("tm-inv-slid-001", ["TM-007", "API-031"], "Sync 0, Segment Length ID 00: strict",
             "decode", pc, f({"segment_length_id": 0})),
        case("tm-inv-slid-002", ["TM-007", "API-032", "GEN-043"],
             "Sync 0, Segment Length ID 10: lenient flags SegmentLengthIdNot11", "decode", pc,
             f({"segment_length_id": 2}), mode="lenient"),
        case("tm-inv-both-001", ["TM-007", "API-032", "GEN-043"],
             "Sync 0, POF 1 and SLID 01: lenient flags both", "decode", pc,
             f({"packet_order_flag": 1, "segment_length_id": 1}), mode="lenient"),
        case("tm-inv-both-002", ["TM-007", "TM-008", "API-032"],
             "Sync 0, POF 1 and FHP outside the data field: Malformed, even in lenient mode",
             "decode", pc, f({"packet_order_flag": 1, "first_header_pointer": 12}),
             mode="lenient"),
        case("tm-inv-val-001", ["TM-011"], "SCID other than the master channel's", "validate",
             mc, f({"scid": 1})),
        case("tm-inv-val-002", ["TM-011"], "VCID not configured", "validate", mc,
             f({"vcid": 5})),
        case("tm-inv-val-003", ["TM-011"], "OCF Flag set on a VC without an OCF", "validate", mc,
             f({}, ocf=b"\x00\x00\x00\x00")),
        case("tm-inv-val-004", ["TM-011"], "OCF Flag clear on a VC with a VC_OCF", "validate",
             mc, f({"vcid": 3})),
        case("tm-inv-val-005", ["TM-011"], "FSH Flag clear on a VC with a VC_FSH", "validate",
             mc, f({"vcid": 2})),
        case("tm-inv-val-006", ["TM-011"], "FSH of 4 octets on a VC whose VC_FSH has 3",
             "validate", mc, f({"vcid": 2}, fsh=b"\x03\x01\x02\x03")),
        case("tm-inv-val-007", ["TM-011"], "Sync Flag 1 on a packet VC", "validate", mc,
             f({"sync_flag": 1})),
        case("tm-inv-val-008", ["TM-011"], "Sync Flag 0 on a VCA_SDU VC", "validate", mc,
             f({"vcid": 1})),
        case("tm-inv-val-009", ["TM-011"], "A matching VCA_SDU frame", "validate", mc,
             f({"vcid": 1, "sync_flag": 1})),
        case("tm-inv-enc-001", ["TM-009"], "Encode on an unconfigured VC", "encode", mc,
             params=enc(6, 0, 0, status("no_packet_start"), "00" * 12)),
        case("tm-inv-enc-002", ["TM-009", "TM-011"], "Encode packets on a VCA_SDU VC",
             "encode", mc, params=enc(1, 0, 0, status("no_packet_start"), "00" * 12)),
        case("tm-inv-enc-003", ["TM-009", "TM-011"], "Encode a VCA_SDU on a packet VC",
             "encode", mc, params=enc(0, 0, 0, status(sync=True), "00" * 12)),
        case("tm-inv-enc-004", ["TM-009", "API-021"], "Data one octet short", "encode", mc,
             params=enc(0, 0, 0, status("no_packet_start"), "00" * 11)),
        case("tm-inv-enc-005", ["TM-009", "API-021"], "FSH data of the wrong length", "encode",
             mc, params=enc(2, 0, 0, status("no_packet_start"), "00" * 9, fsh_data="01")),
        case("tm-inv-enc-006", ["TM-009", "API-021"], "OCF missing on a VC with a VC_OCF",
             "encode", mc, params=enc(3, 0, 0, status("no_packet_start"), "00" * 8)),
        case("tm-inv-enc-007", ["TM-009", "API-021"], "OCF given on a VC without one",
             "encode", mc, params=enc(0, 0, 0, status("no_packet_start"), "00" * 12,
                                      ocf="00000000")),
        case("tm-inv-enc-008", ["TM-009", "API-021"], "Security Header given on a VC without SDLS",
             "encode", mc, params=enc(0, 0, 0, status("no_packet_start"), "00" * 12, sh="A5")),
        case("tm-inv-enc-009", ["TM-009", "API-021"],
             "Security Trailer given on a VC without SDLS", "encode", mc,
             params=enc(0, 0, 0, status("no_packet_start"), "00" * 12, tr="5A")),
        case("tm-inv-enc-010", ["API-022"], "Output buffer one octet short: BufferTooSmall",
             "encode", mc, params=enc(0, 0, 0, status("no_packet_start"), "00" * 12, out_len=19)),
        case("tm-inv-cfg-001", ["API-040", "CMN-041"], "Frame of 8 octets with FECF",
             "config", pc_only(8, True)),
        case("tm-inv-cfg-002", ["API-040"], "SCID 1024", "config", channel(20, False, 1024, [])),
        case("tm-inv-cfg-003", ["API-040"], "VCID 8", "config", channel(20, False, 1, [vc(8)])),
        case("tm-inv-cfg-004", ["API-040"], "VC_FSH of 1 octet", "config",
             channel(20, False, 1, [vc(0, fsh=1)])),
        case("tm-inv-cfg-005", ["API-040"], "MC_FSH of 65 octets", "config",
             channel(200, False, 1, [], mc_fsh=65)),
        case("tm-inv-cfg-006", ["API-040", "TM-011"], "Both an MC_FSH and a VC_FSH", "config",
             channel(40, False, 1, [vc(0, fsh=2)], mc_fsh=2)),
        case("tm-inv-cfg-007", ["API-040", "TM-011"], "Both an MC_OCF and a VC_OCF", "config",
             channel(40, False, 1, [vc(0, ocf=True)], mc_ocf=True)),
        case("tm-inv-cfg-008", ["API-040", "CMN-041"],
             "MC_FSH of 8 octets and an OCF leave no data octet in 20-octet frames", "config",
             channel(20, True, 1, [vc(0, ocf=True)], mc_fsh=8)),
        case("tm-inv-cfg-009", ["API-040"], "VCID 3 configured twice", "config",
             channel(20, False, 1, [vc(3), vc(3, "vca_sdu")])),
    ]
    return cases


NOTE = (
    "132.0-B-3 prints no example frame; only the first OID octets of Annex D. Cases "
    "hand-derived from 132.0-B-3 §4.1 (figures 4-1 to 4-4, the Data Field Status of "
    "§4.1.2.7, the Secondary Header of §4.1.3, OID frames of §4.1.4.6 and Annex D, the FECF "
    "of §4.1.6), §5 Tables 5-1 to 5-3, and §6.3, with the readings of "
    "docs/standards/tm-frame.md (TM-001...TM-011 and the anomaly list) and the error kinds "
    "and field codes of docs/03-api-model.md §9 and §9.1. Inputs assembled bit by bit from "
    "the figure widths; expected values computed by an independent Python model of those "
    "clauses, written from the Blue Book after the Rust codec but not from its code, and sharing "
    "no code with it (the FECF by "
    "polynomial division, the OID data by the Annex D recurrence). The model is committed as "
    "test-vectors/models/tm_frame.py (TV-023); `python3 -I test-vectors/models/tm_frame.py "
    "--check test-vectors/tm/*.json` recomputes every expected value. One frame is worked "
    "by hand in docs/verification/derivations/tm-frame.md."
)

FILES = {
    "minimal": (minimal, "132.0-B-3 §4.1, §5"),
    "fsh_ocf_fecf": (fsh_ocf_fecf, "132.0-B-3 §4.1.3, §4.1.5, §4.1.6"),
    "fhp_values": (fhp_values, "132.0-B-3 §4.1.2.7"),
    "oid_sequence": (oid_sequence, "132.0-B-3 §4.1.4.6, Annex D"),
    "security_layout": (security_layout, "132.0-B-3 §6.3, Table 6-1"),
    "invalid": (invalid, "132.0-B-3 §4.1, §5"),
}


def generate(name):
    make, standard = FILES[name]
    return {"kind": "codec", "standard": standard, "provenance": "hand-derived",
            "provenance_note": NOTE, "cases": make()}


def check_field_codes(path):
    catalogue = os.path.join(os.path.dirname(os.path.dirname(path)), "core", "fields.json")
    if not os.path.exists(catalogue):
        return 0
    with open(catalogue, encoding="utf-8") as fh:
        names = {e["code"]: e["name"] for e in json.load(fh)["fields"]}
    bad = 0
    for name, code in F.items():
        if names.get(code) != name:
            print(f"{catalogue}: code {code} is {names.get(code)!r}, the model expects {name!r}")
            bad += 1
    return bad


def check(paths):
    bad = 0
    for path in paths:
        name = os.path.splitext(os.path.basename(path))[0]
        with open(path, encoding="utf-8") as fh:
            document = json.load(fh)
        bad += check_field_codes(path)
        generated = {c["id"]: c for c in generate(name)["cases"]} if name in FILES else {}
        live = [c for c in document["cases"] if "superseded" not in c]
        for c in live:
            try:
                want = expect(c)
            except (KeyError, ValueError) as e:
                print(f"{path}: {c['id']}: {e}")
                bad += 1
                continue
            if c["expect"] != want:
                print(f"{path}: {c['id']}: expect {c['expect']}, the model gives {want}")
                bad += 1
            if c["id"] in generated and generated[c["id"]] != c:
                print(f"{path}: {c['id']} differs from the generator (--write)")
                bad += 1
        ids = {c["id"] for c in document["cases"]}
        for cid in generated:
            if cid not in ids:
                print(f"{path}: {cid}, written by the generator, is missing")
                bad += 1
        print(f"{path}: {len(live)} case(s) checked against the model")
    return bad


def main(argv):
    if len(argv) >= 2 and argv[0] == "--check":
        bad = check(argv[1:])
        if bad:
            print(f"tm_frame: {bad} difference(s)")
            return 1
        return 0
    if len(argv) == 2 and argv[0] == "--write":
        os.makedirs(argv[1], exist_ok=True)
        for name in FILES:
            with open(os.path.join(argv[1], f"{name}.json"), "w", encoding="utf-8") as fh:
                json.dump(generate(name), fh, indent=2, ensure_ascii=False)
                fh.write("\n")
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
