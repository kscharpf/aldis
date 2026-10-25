"""Independent model of the AOS Transfer Frame for the vector files
test-vectors/aos/{minimal,fhec_frame,insert_zone,security_layout,mpdu_pointers,bpdu_pointers,
oid,issue4_compat,scid_10bit,config,invalid}.json (AOS-001...AOS-019, GEN-062; M2 WP-4).

Sources: CCSDS 732.0-B-5 EC1 §4.1 (figures 4-1 to 4-4: field order and widths), §4.1.2 (the
Primary Header; bits 42-43 the SCID's two most significant bits; the signaling field; the
cycle 0000 when unused), §4.1.2.6 (FHEC), §4.1.3 (Insert Zone), §4.1.4.1.5 (OID frames on
VCID 63, the PN sequence), §4.1.4.2-3 (M_PDU and B_PDU headers, their reserved pointer
values), §4.1.5 (OCF per VC), §4.1.6 (FECF), §4.3.7.2-3 (the header corrected before the CRC
is checked), §5 Tables 5-1 to 5-3, §6.3 (Security Header and Trailer); CCSDS 732.0-B-4
§4.1.2.2.3, §4.1.2.5.4, §4.1.4.2.2-3 (Issue 4: an 8-bit SCID, bits 42-43 a reserved spare, an
M_PDU Header of 5 spare bits and an 11-bit pointer); the readings of
docs/standards/aos-frame.md (AOS-001...AOS-019, the anomaly list, OI-43) and the error kinds
and field codes of docs/03-api-model.md §9 and §9.1. Parameters and code, no Blue Book text.

Method, chosen to differ from the Rust codec (crates/aldis-aos): frames are assembled and
taken apart as strings of bits, field by field from the figure widths; the FECF is the
remainder of a polynomial division of Python integers, X^16 M(X) + X^(n-16) L(X) modulo G(X)
(§4.1.6.2.2), computed over the corrected header followed by the rest of the frame; the OID
data is the Fibonacci recurrence x[n] = x[n-1] ^ x[n-2] ^ x[n-22] ^ x[n-32] on a list of bits
seeded with 32 ones (§4.1.4.1.5.2.1, 732.0 Annex D); the FHEC, encoding and correction, is the
independent table model test-vectors/models/aos_fhec.py, loaded from its path. The model was
written from the Blue Book and the specification, not from the Rust code, and must stay
independent of it: never port code from it, and never fix the model to agree with it.

Usage, from the repository root:

    python3 -I test-vectors/models/aos_frame.py --check test-vectors/aos/<file>.json ...
    python3 -I test-vectors/models/aos_frame.py --write test-vectors/aos

--check recomputes the expected outcome of every live case from its operation, configuration,
parameters, and input, compares the cases the generator writes with the file's, and checks
the field codes against test-vectors/core/fields.json; it exits 1 on any difference. --write
regenerates the files (cases and provenance notes).
"""
import importlib.util
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def _load_fhec():
    """The FHEC table model, loaded from its file (python3 -I puts no directory on the path)."""
    sys.dont_write_bytecode = True  # no __pycache__ beside the models
    spec = importlib.util.spec_from_file_location("aos_fhec_model",
                                                  os.path.join(HERE, "aos_fhec.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


FHEC = _load_fhec()

# Field codes of docs/03-api-model.md §9.1.
F = {
    "spacecraft_id": 1, "vc_id": 2, "frame_data_field": 7, "security_header_len": 8,
    "security_trailer_len": 9, "frame_version": 67, "frame_len": 68,
    "first_header_pointer": 74, "vcf_count": 93, "insert_zone_len": 97,
    "aos_header_spare": 100, "vc_frame_count_cycle": 101, "data_field_content": 102, "ocf": 103,
    "mpdu_spare": 104, "bpdu_spare": 105, "bitstream_data_pointer": 106,
}

# Primary Header, figure 4-2: (name, width in bits), in transmission order.
PRIMARY = [("version", 2), ("scid_lsb", 8), ("vcid", 6), ("count", 24), ("replay", 1),
           ("use", 1), ("bits_42_43", 2), ("cycle", 4)]
OID_VCID = 0b111111  # §4.1.4.1.5.1
# M_PDU Header per edition: (spare bits, pointer bits); all ones = no Packet starts, all ones
# minus one = only Idle Data (§4.1.4.2.2.4-5; 732.0-B-4 §4.1.4.2.2-3).
MPDU = {"issue5": (0, 16), "issue4": (5, 11)}
# B_PDU Header: 2 spare bits, 14-bit pointer (§4.1.4.3.1.4).
BPDU_ALL_VALID, BPDU_ONLY_IDLE = 0x3FFF, 0x3FFE
MAX_SCID = {"issue5": 1023, "issue4": 255}
# AOS-018: a Bitstream Data Zone of at most 2047 octets; in Issue 5 frames of at most 2048.
MAX_BPDU_ZONE, MAX_BPDU_FRAME = 2047, 2048
ANOMALY_BITS = ["CycleNonZeroWhileUnused", "BpduSpareSet", "Issue4SpareSet"]
VCS_CAPACITY = 8  # the handlers' VcTable<8>; no case reaches it


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

def header_len(pc):
    return 8 if pc["fhec"] else 6


def lengths(pc, sh, st, ocf):
    """The field lengths of a frame, §4.1.1.1 and §6.3.5.2 b; data may come out below 1."""
    data = (pc["frame_len"] - header_len(pc) - pc["iz"] - sh - st - (4 if ocf else 0)
            - (2 if pc["fecf"] else 0))
    return {"header": header_len(pc), "iz": pc["iz"], "sh": sh, "data": data, "st": st,
            "ocf": 4 if ocf else 0, "fecf": 2 if pc["fecf"] else 0}


def pdu_len(content):
    return 2 if content in ("mpdu", "bpdu") else 0


def build_config(cfg):
    """The configuration, or Fail. Keys: frame_len, fhec, insert_zone_len, fecf, edition, and
    vcs (scid, vcid, content, ocf, security_header_len, security_trailer_len)."""
    edition = cfg.get("edition", "issue5")
    pc = {"frame_len": cfg["frame_len"], "fhec": cfg.get("fhec", False),
          "iz": cfg.get("insert_zone_len", 0), "fecf": cfg.get("fecf", False),
          "edition": edition}
    if pc["frame_len"] < header_len(pc) + pc["iz"] + 1 + (2 if pc["fecf"] else 0):
        raise Fail("InvalidConfig", F["frame_len"])
    parsed = []
    for vc in cfg.get("vcs", []):
        if vc["scid"] > 1023:
            raise Fail("OutOfRange", F["spacecraft_id"])
        if vc["vcid"] > 63:
            raise Fail("OutOfRange", F["vc_id"])
        if vc["vcid"] == OID_VCID:
            raise Fail("InvalidConfig", F["vc_id"])
        sh, st = vc.get("security_header_len", 0), vc.get("security_trailer_len", 0)
        if sh > 64:
            raise Fail("InvalidConfig", F["security_header_len"])
        if st > 0xFFFF:
            raise Fail("InvalidConfig", F["security_trailer_len"])
        parsed.append({"scid": vc["scid"], "vcid": vc["vcid"], "content": vc["content"],
                       "ocf": vc.get("ocf", False), "sh": sh, "st": st})
    vcs = {}
    for vc in parsed:
        key = (vc["scid"], vc["vcid"])
        if key in vcs:
            raise Fail("InvalidConfig", F["vc_id"])
        if vc["scid"] > MAX_SCID[edition]:
            raise Fail("InvalidConfig", F["spacecraft_id"])
        n = lengths(pc, vc["sh"], vc["st"], vc["ocf"])["data"]
        if n < 1 or n <= pdu_len(vc["content"]):
            raise Fail("InvalidConfig", F["frame_data_field"])
        if vc["content"] == "bpdu":
            if edition == "issue5" and pc["frame_len"] > MAX_BPDU_FRAME:
                raise Fail("InvalidConfig", F["frame_len"])
            if n - 2 > MAX_BPDU_ZONE:
                raise Fail("InvalidConfig", F["frame_data_field"])
        if len(vcs) == VCS_CAPACITY:
            raise Fail("CapacityExceeded", F["vc_id"])
        vcs[key] = vc
    return pc, vcs


# ---- the Primary Header (AOS-002, AOS-010...AOS-014)

def correct(octets, pc):
    """The header octets after the FHEC (§4.3.7.2), and the corrected count, or Fail."""
    n = header_len(pc)
    if len(octets) < n:
        raise Fail("Incomplete")
    head = bytes(octets[:n])
    if not pc["fhec"]:
        return head, 0
    r = FHEC.decode(head, FHEC.T)
    if r is None:
        raise Fail("HeaderUncorrectable")
    return r[0], r[1]


def header_fields(head, corrected, pc):
    h = split_primary(head)
    if pc["edition"] == "issue5":
        scid, spare = (h["bits_42_43"] << 8) | h["scid_lsb"], 0
    else:
        scid, spare = h["scid_lsb"], h["bits_42_43"]
    return {
        "scid": scid, "vcid": h["vcid"], "vc_frame_count": h["count"],
        "replay_flag": bool(h["replay"]), "cycle_use_flag": bool(h["use"]),
        "cycle_field": h["cycle"], "vc_frame_count_cycle": h["cycle"] if h["use"] else None,
        "issue4_spare": spare,
        "fhec": int.from_bytes(head[6:8], "big") if pc["fhec"] else None,
        "corrected_symbols": corrected, "header_hex": head.hex().upper(),
        "is_oid": h["vcid"] == OID_VCID,
    }


def check_version(octet0):
    if octet0 >> 6 != 0b01:
        raise Fail("UnsupportedVersion", F["frame_version"])


def decode_header(octets, pc):
    head, corrected = correct(octets, pc)
    check_version(head[0])
    return header_fields(head, corrected, pc)


def peek_length(octets, pc):
    try:
        decode_header(octets, pc)
    except Fail as e:
        if e.kind != "Incomplete":
            raise
        if not pc["fhec"] and octets:
            check_version(octets[0])
    return {"len": pc["frame_len"]}


# ---- decoding (AOS-010...AOS-017), checks in the order of docs/standards/aos-frame.md

def report(ocf):
    bit0, bit1 = ocf[0] >> 7, (ocf[0] >> 6) & 1
    return "clcw" if bit0 == 0 else ("project_specific" if bit1 == 0 else "sdls_fsr")


def data_unit(content, field, edition, note):
    """The data unit of `field`: (kind, raw pointer, pointer, value, spare, zone), or Fail."""
    if content in ("vca_sdu", "idle_data"):
        return content, None, None, None, None, field
    if len(field) < 3:
        raise Fail("Malformed", F["frame_data_field"])
    bits, zone = bits_of(field[:2]), field[2:]
    if content == "mpdu":
        spare_bits, width = MPDU[edition]
        spare, raw = int(bits[:spare_bits] or "0", 2), int(bits[spare_bits:], 2)
        none, idle = (1 << width) - 1, (1 << width) - 2
        if raw == none:
            pointer, value = "no_packet_start", None
        elif raw == idle:
            pointer, value = "only_idle_data", None
        elif raw < len(zone):
            pointer, value = "starts_at", raw
        else:
            raise Fail("Malformed", F["first_header_pointer"])
        if spare:
            note("Issue4SpareSet", Fail("ReservedValue", F["mpdu_spare"]))
        return "mpdu", raw, pointer, value, spare if edition == "issue4" else 0, zone
    spare, raw = int(bits[:2], 2), int(bits[2:], 2)
    last_bit = len(zone) * 8 - 1
    if raw == BPDU_ALL_VALID:
        pointer, value = "all_valid", None
    elif raw == BPDU_ONLY_IDLE:
        pointer, value = "only_idle", None
    elif raw < last_bit:
        pointer, value = "valid_bits", raw + 1
    elif raw == last_bit:
        raise Fail("InvalidField", F["bitstream_data_pointer"])
    else:
        raise Fail("Malformed", F["bitstream_data_pointer"])
    if spare:
        note("BpduSpareSet", Fail("ReservedValue", F["bpdu_spare"]))
    return "bpdu", raw, pointer, value, spare, zone


def decode(octets, pc, vcs, mode):
    if len(octets) != pc["frame_len"]:
        raise Fail("LengthMismatch", F["frame_len"])
    head, corrected = correct(octets, pc)
    if pc["fecf"]:
        covered = head + bytes(octets[len(head):-2])
        if crc_fecf(covered) != int.from_bytes(octets[-2:], "big"):
            raise Fail("CrcMismatch")
    check_version(head[0])
    h = header_fields(head, corrected, pc)
    scids = {s for s, _ in vcs}
    if h["is_oid"]:
        if h["scid"] not in scids:
            raise Fail("UnknownChannel", F["spacecraft_id"])
        content, sh, st, ocf = "idle_data", 0, 0, False
    else:
        vc = vcs.get((h["scid"], h["vcid"]))
        if vc is None:
            raise Fail("UnknownChannel", F["vc_id"] if h["scid"] in scids else F["spacecraft_id"])
        content, sh, st, ocf = vc["content"], vc["sh"], vc["st"], vc["ocf"]
    lens = lengths(pc, sh, st, ocf)
    if lens["data"] < 1:
        raise Fail("Malformed", F["frame_data_field"])
    noted = []

    def note(flag, error):
        noted.append((flag, error))

    if not h["cycle_use_flag"] and h["cycle_field"] != 0:
        note("CycleNonZeroWhileUnused", Fail("InvalidField", F["vc_frame_count_cycle"]))
    if h["issue4_spare"]:
        note("Issue4SpareSet", Fail("ReservedValue", F["aos_header_spare"]))
    at, fields = 0, {}
    for name in ("header", "iz", "sh", "data", "st", "ocf", "fecf"):
        fields[name] = bytes(octets[at:at + lens[name]])
        at += lens[name]
    assert at == len(octets)
    kind, raw, pointer, value, spare, zone = data_unit(content, fields["data"], pc["edition"],
                                                       note)
    if mode == "strict" and noted:
        raise noted[0][1]
    flags = {flag for flag, _ in noted} if mode == "lenient" else set()
    out = dict(h)
    out.update({
        "insert_zone_hex": fields["iz"].hex().upper(),
        "security_header_hex": fields["sh"].hex().upper(),
        "data_field_hex": fields["data"].hex().upper(),
        "security_trailer_hex": fields["st"].hex().upper(),
        "ocf_hex": fields["ocf"].hex().upper() if ocf else None,
        "ocf_report": report(fields["ocf"]) if ocf else None,
        "fecf": int.from_bytes(fields["fecf"], "big") if pc["fecf"] else None,
        "data_unit": kind, "pdu_pointer_raw": raw, "pointer": pointer, "pointer_value": value,
        "pdu_spare": spare, "zone_hex": zone.hex().upper(), "consumed": len(octets),
        "anomalies": [a for a in ANOMALY_BITS if a in flags],
    })
    return out


# ---- encoding (AOS-015, AOS-016, AOS-019)

def header_octets(pc, scid, vcid, count, replay, cycle):
    """The 6 or 8 header octets a writer puts in a frame (§4.1.2, §4.2.7.3; GEN-052)."""
    msb = scid >> 8 if pc["edition"] == "issue5" else 0
    six = join_primary({"version": 0b01, "scid_lsb": scid & 0xFF, "vcid": vcid, "count": count,
                        "replay": replay, "use": cycle is not None, "bits_42_43": msb,
                        "cycle": cycle or 0})
    return FHEC.encode(six) if pc["fhec"] else six


def check_fields(p):
    """The handler's range checks of the parameters (API-001)."""
    if p["scid"] > 1023:
        raise Fail("OutOfRange", F["spacecraft_id"])
    if p.get("vcid", OID_VCID) > 63:
        raise Fail("OutOfRange", F["vc_id"])
    if p["vc_frame_count"] > 0xFFFFFF:
        raise Fail("OutOfRange", F["vcf_count"])
    if p.get("cycle") is not None and p["cycle"] > 15:
        raise Fail("OutOfRange", F["vc_frame_count_cycle"])


def pdu_header(pc, vc, lens, pdu):
    """The M_PDU or B_PDU Header octets for `pdu`, or Fail (AOS-017, AOS-019)."""
    kind = pdu["kind"]
    fits = {"mpdu": "mpdu", "bpdu": "bpdu", "none": None}[kind] == (
        vc["content"] if vc["content"] in ("mpdu", "bpdu") else None)
    if not fits:
        raise Fail("InvalidField", F["data_field_content"])
    if lens["data"] < 1 or lens["data"] <= pdu_len(vc["content"]):
        raise Fail("InvalidConfig", F["frame_data_field"])
    zone = lens["data"] - pdu_len(vc["content"])
    if kind == "mpdu":
        spare_bits, width = MPDU[pc["edition"]]
        none, idle = (1 << width) - 1, (1 << width) - 2
        pointer = pdu["pointer"]
        if pointer == "starts_at":
            raw = pdu["value"]
            if raw >= zone or raw >= idle:
                raise Fail("OutOfRange", F["first_header_pointer"])
        else:
            raw = none if pointer == "no_packet_start" else idle
        return octets_of("0" * spare_bits + to_bits(raw, width))
    if kind == "bpdu":
        pointer = pdu["pointer"]
        if pointer == "valid_bits":
            n = pdu["value"]
            if n < 1 or n >= zone * 8:
                raise Fail("OutOfRange", F["bitstream_data_pointer"])
            raw = n - 1
        else:
            raw = BPDU_ALL_VALID if pointer == "all_valid" else BPDU_ONLY_IDLE
        return octets_of("00" + to_bits(raw, 14))
    return b""


def hexb(p, key):
    value = p.get(key)
    return None if value is None else bytes.fromhex(value)


def encode(pc, vcs, p):
    check_fields(p)
    if p["vcid"] == OID_VCID:
        raise Fail("InvalidField", F["vc_id"])
    scids = {s for s, _ in vcs}
    vc = vcs.get((p["scid"], p["vcid"]))
    if vc is None:
        raise Fail("UnknownChannel", F["vc_id"] if p["scid"] in scids else F["spacecraft_id"])
    lens = lengths(pc, vc["sh"], vc["st"], vc["ocf"])
    pdu = pdu_header(pc, vc, lens, p["pdu"])
    zone_len = lens["data"] - len(pdu)
    for key, length, field in [("insert_zone_hex", lens["iz"], "insert_zone_len"),
                               ("security_header_hex", vc["sh"], "security_header_len"),
                               ("zone_hex", zone_len, "frame_data_field"),
                               ("security_trailer_hex", vc["st"], "security_trailer_len")]:
        if len(hexb(p, key) or b"") != length:
            raise Fail("LengthMismatch", F[field])
    if (p.get("ocf_hex") is not None) != vc["ocf"]:
        raise Fail("InvalidField", F["ocf"])
    if p.get("out_len", pc["frame_len"]) < pc["frame_len"]:
        raise Fail("BufferTooSmall")
    body = header_octets(pc, p["scid"], p["vcid"], p["vc_frame_count"], p.get("replay_flag", False),
                         p.get("cycle"))
    body += (hexb(p, "insert_zone_hex") or b"") + (hexb(p, "security_header_hex") or b"")
    body += pdu + hexb(p, "zone_hex") + (hexb(p, "security_trailer_hex") or b"")
    body += hexb(p, "ocf_hex") or b""
    if pc["fecf"]:
        body += crc_fecf(body).to_bytes(2, "big")
    assert len(body) == pc["frame_len"]
    return {"output_hex": body.hex().upper()}


def encode_oid(pc, vcs, p):
    """§4.1.4.1.5: OID frames of one SCID from one generator (CMN-021; AOS-016)."""
    oid = Oid()
    frames = []
    for f in p["frames"]:
        q = {"scid": p["scid"], "vc_frame_count": f["vc_frame_count"], "cycle": f.get("cycle")}
        check_fields(q)
        if p["scid"] not in {s for s, _ in vcs}:
            raise Fail("UnknownChannel", F["spacecraft_id"])
        lens = lengths(pc, 0, 0, False)
        body = header_octets(pc, p["scid"], OID_VCID, f["vc_frame_count"], False, f.get("cycle"))
        body += hexb(f, "insert_zone_hex") or bytes(lens["iz"])
        body += oid.octets(lens["data"])
        if pc["fecf"]:
            body += crc_fecf(body).to_bytes(2, "big")
        assert len(body) == pc["frame_len"]
        frames.append(body.hex().upper())
    return {"frames_hex": frames}


def expect(case):
    """The expected outcome of a case, from the model alone."""
    op = case["operation"]
    mode = case.get("mode", "strict")
    p = case.get("params", {})
    octets = bytes.fromhex(case.get("input_hex", ""))
    try:
        pc, vcs = build_config(case["config"])
        if op == "config":
            out = {}
        elif op == "peek_length":
            out = peek_length(octets, pc)
        elif op == "decode_header":
            out = decode_header(octets, pc)
        elif op == "decode":
            out = decode(octets, pc, vcs, mode)
        elif op == "encode":
            out = encode(pc, vcs, p)
        elif op == "encode_oid":
            out = encode_oid(pc, vcs, p)
        else:
            raise ValueError(f"{case['id']}: the model has no operation {op!r}")
    except Fail as e:
        res = {"ok": False, "error": e.kind}
        if e.field is not None:
            res["field"] = e.field
        return res
    return {"ok": True, **out}


# ---- the generator: how the inputs of the vector files were chosen

def raw_header(pc, fields):
    """Six header octets with any field values, and the FHEC if `pc` has one."""
    h = {"version": 0b01, "scid_lsb": 0, "vcid": 0, "count": 0, "replay": 0, "use": 0,
         "bits_42_43": 0, "cycle": 0}
    h.update(fields)
    six = join_primary(h)
    return FHEC.encode(six) if pc["fhec"] else six


def frame(cfg, fields, iz=b"", sh=b"", data=b"", st=b"", ocf=b"", fecf_over=None):
    """Assembles raw input octets, any field values allowed (for decoder inputs). The FECF is
    computed over `fecf_over` in place of the header when given (a miscorrected header)."""
    pc = {"frame_len": cfg["frame_len"], "fhec": cfg.get("fhec", False),
          "iz": cfg.get("insert_zone_len", 0), "fecf": cfg.get("fecf", False),
          "edition": cfg.get("edition", "issue5")}
    head = raw_header(pc, fields)
    body = head + iz + sh + data + st + ocf
    if pc["fecf"]:
        covered = (fecf_over + body[len(head):]) if fecf_over is not None else body
        body += crc_fecf(covered).to_bytes(2, "big")
    assert len(body) == pc["frame_len"], (len(body), pc["frame_len"])
    return body


def hdr(scid, vcid, count=0, replay=0, use=0, cycle=0, edition="issue5", spare=0):
    """Header fields for `frame`: the SCID split by the edition, `spare` in bits 42-43 of
    Issue 4."""
    if edition == "issue5":
        return {"scid_lsb": scid & 0xFF, "bits_42_43": scid >> 8, "vcid": vcid, "count": count,
                "replay": replay, "use": use, "cycle": cycle}
    return {"scid_lsb": scid, "bits_42_43": spare, "vcid": vcid, "count": count,
            "replay": replay, "use": use, "cycle": cycle}


def seq(start, n):
    return bytes((start + i) & 0xFF for i in range(n))


def channel(frame_len, vcs, fhec=False, iz=0, fecf=False, edition="issue5"):
    return {"frame_len": frame_len, "fhec": fhec, "insert_zone_len": iz, "fecf": fecf,
            "edition": edition, "vcs": vcs}


def vc(scid, vcid, content, ocf=False, sh=0, st=0):
    return {"scid": scid, "vcid": vcid, "content": content, "ocf": ocf,
            "security_header_len": sh, "security_trailer_len": st}


def mp(pointer, value=None):
    return {"kind": "mpdu", "pointer": pointer, "value": value}


def bp(pointer, value=None):
    return {"kind": "bpdu", "pointer": pointer, "value": value}


NONE = {"kind": "none"}


def enc(scid, vcid, count, pdu, zone, replay=False, cycle=None, iz="", sh="", st="", ocf=None,
        **extra):
    p = {"scid": scid, "vcid": vcid, "vc_frame_count": count, "replay_flag": replay,
         "cycle": cycle, "pdu": pdu, "insert_zone_hex": iz, "security_header_hex": sh,
         "zone_hex": zone, "security_trailer_hex": st, "ocf_hex": ocf}
    p.update(extra)
    return p


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


def both(cid, reqs, desc, config, input_hex):
    """The same input decoded strictly and leniently."""
    return [case(f"{cid}-s", reqs, desc + "; strict", "decode", config, input_hex, mode="strict"),
            case(f"{cid}-l", reqs, desc + "; lenient", "decode", config, input_hex,
                 mode="lenient")]


SCID = 0x55  # an SCID that fits both editions
CLCW = bytes.fromhex("01140C2A")  # the CLCW of core/ocf.json, ocf-classify-001
DEC = ["AOS-010", "API-010", "TV-002"]


def minimal():
    plain = channel(16, [vc(SCID, 5, "mpdu"), vc(SCID, 6, "vca_sdu"), vc(SCID, 7, "idle_data")])
    fecf = channel(18, [vc(SCID, 5, "mpdu")], fecf=True)
    ocf = channel(20, [vc(SCID, 2, "vca_sdu", ocf=True)])
    m = frame(plain, hdr(SCID, 5, 0x000102), data=b"\x00\x00" + seq(0xD0, 8))
    out = [
        case("aos-min-dec-001", DEC + ["AOS-011", "AOS-017", "API-012", "API-014", "GEN-013"],
             "16-octet frame, no FHEC, Insert Zone, OCF, FECF, or SDLS: SCID 0x55, VC 5 with an "
             "M_PDU, First Header Pointer 0", "decode", plain, m),
        case("aos-min-dec-002", DEC + ["API-030"], "The same frame, lenient: the same view",
             "decode", plain, m, mode="lenient"),
        case("aos-min-dec-003", DEC + ["GEN-013"], "VC 6 carries VCA_SDUs: the whole data field",
             "decode", plain, frame(plain, hdr(SCID, 6, 0xFFFFFF), data=seq(0x10, 10))),
        case("aos-min-dec-004", DEC, "VC 7 carries Idle Data: the whole data field", "decode",
             plain, frame(plain, hdr(SCID, 7, 7), data=bytes(10))),
        case("aos-min-dec-005", DEC + ["API-012"],
             "Shortest frame without FECF: 7 octets, one VCA_SDU octet", "decode",
             channel(7, [vc(0, 0, "vca_sdu")]), frame(channel(7, []), hdr(0, 0), data=b"\x5A")),
        case("aos-min-dec-006", DEC + ["CMN-003"], "With an FECF over the whole frame", "decode",
             fecf, frame(fecf, hdr(SCID, 5, 1), data=b"\xFF\xFF" + seq(0x20, 8))),
        case("aos-min-dec-007", DEC + ["CMN-010"],
             "VC 2 has a VC_OCF holding a CLCW (Table 5-3; §4.1.5)", "decode", ocf,
             frame(ocf, hdr(SCID, 2, 3), data=seq(0x40, 10), ocf=CLCW)),
        case("aos-min-dec-008", DEC + ["AOS-012"],
             "Replay Flag 1, Use Flag 1, cycle 1010: the cycle is reported", "decode", plain,
             frame(plain, hdr(SCID, 6, 0x123456, replay=1, use=1, cycle=0xA), data=bytes(10))),
        case("aos-min-dec-009", DEC + ["AOS-012"],
             "Use Flag 1 with cycle 0000", "decode", plain,
             frame(plain, hdr(SCID, 6, 0, use=1), data=bytes(10))),
    ]
    out += both("aos-min-dec-010", DEC + ["AOS-012", "API-031", "API-032", "GEN-043"],
                "Use Flag 0 with cycle 0101 (§4.1.2.5.5.3): InvalidField in strict mode, "
                "CycleNonZeroWhileUnused in lenient mode", plain,
                frame(plain, hdr(SCID, 6, 0, cycle=5), data=bytes(10)))
    out += [
        case("aos-min-hdr-001", ["AOS-010", "AOS-011", "API-014"],
             "The Primary Header alone, octets after it ignored", "decode_header", plain,
             m[:6] + b"\xEE\xEE"),
        case("aos-min-hdr-002", ["AOS-010", "API-013"], "Five octets: Incomplete",
             "decode_header", plain, m[:5]),
        case("aos-min-peek-001", ["API-013"], "Empty prefix: the configured length",
             "peek_length", plain, ""),
        case("aos-min-peek-002", ["API-013", "AOS-011"], "First octet with version 01",
             "peek_length", plain, m[:1]),
        case("aos-min-peek-003", ["API-013", "AOS-011"], "First octet with version 11",
             "peek_length", plain, "C0"),
        case("aos-min-enc-001", ["AOS-019", "API-020", "API-021", "GEN-052"],
             "Encode the frame of aos-min-dec-001", "encode", plain,
             params=enc(SCID, 5, 0x000102, mp("starts_at", 0), seq(0xD0, 8).hex().upper())),
        case("aos-min-enc-002", ["AOS-019", "API-020", "AOS-012"],
             "Encode a VCA_SDU with Replay Flag 1 and cycle 1010", "encode", plain,
             params=enc(SCID, 6, 0x123456, NONE, "00" * 10, replay=True, cycle=0xA)),
        case("aos-min-enc-003", ["AOS-019", "API-021", "CMN-003"],
             "Encode with the FECF computed by the writer", "encode", fecf,
             params=enc(SCID, 5, 1, mp("no_packet_start"), seq(0x20, 8).hex().upper())),
        case("aos-min-enc-004", ["AOS-019", "CMN-010"], "Encode with the OCF", "encode", ocf,
             params=enc(SCID, 2, 3, NONE, seq(0x40, 10).hex().upper(), ocf=CLCW.hex().upper())),
    ]
    return out


def fhec_frame():
    cfg = channel(24, [vc(0, 1, "vca_sdu")], fhec=True, fecf=True)
    pc = build_config(cfg)[0]
    data = bytes([0x33] * 14)
    good = frame(cfg, hdr(0, 1, 0xABCDEF), data=data)
    reqs = ["AOS-013", "AOS-014", "AOS-010"]

    def hit(octets, changes):
        b = bytearray(octets)
        for i, x in changes:
            b[i] ^= x
        return bytes(b)

    # The header 6A85123456001252 of fhec_encode.json (aos-fhec-enc-003) received with the four
    # symbol errors of aos-fhec-mis-001: the FHEC miscorrects it to 7AA5123456E05352.
    mis_cfg = channel(24, [vc(0xAA, 5, "vca_sdu"), vc(0x2EA, 37, "vca_sdu")], fhec=True,
                      fecf=True)
    sent = frame(mis_cfg, hdr(0xAA, 5, 0x123456), data=data)
    received = hit(sent, [(0, 0x10), (1, 0x20), (5, 0x30), (6, 0x40)])
    no_fecf = channel(22, mis_cfg["vcs"], fhec=True)
    sent_nf = frame(no_fecf, hdr(0xAA, 5, 0x123456), data=data)
    received_nf = hit(sent_nf, [(0, 0x10), (1, 0x20), (5, 0x30), (6, 0x40)])
    out = [
        case("aos-fhecf-dec-001", reqs + ["CMN-003"], "Frame with FHEC and FECF, no error",
             "decode", cfg, good),
    ]
    out += both("aos-fhecf-dec-002", reqs + ["API-032"],
                "One symbol (the version and SCID nibble of octet 0) wrong: corrected, reported "
                "as corrected_symbols 1 in both modes, and the FECF checked over the corrected "
                "header", cfg, hit(good, [(0, 0x80)]))
    out += both("aos-fhecf-dec-003", reqs,
                "Two symbols wrong, in octet 1 and the FHEC: corrected, corrected_symbols 2",
                cfg, hit(good, [(1, 0x0F), (7, 0x50)]))
    out += [
        case("aos-fhecf-dec-004", reqs + ["GEN-041"],
             "The frame count octets wrong: not protected, not corrected, so the FECF fails",
             "decode", cfg, hit(good, [(3, 0xFF)])),
        case("aos-fhecf-dec-005", reqs,
             "The uncorrectable header of aos-fhec-unc (5111...): HeaderUncorrectable before the "
             "FECF", "decode", cfg,
             bytes.fromhex("5111ABCDEF003EDF") + good[8:]),
        case("aos-fhecf-dec-006", reqs + ["CMN-003"],
             "Four symbol errors miscorrected by the FHEC (aos-fhec-mis-001): the FECF over the "
             "corrected header catches it (AOS-014)", "decode", mis_cfg, received),
        case("aos-fhecf-dec-007", reqs,
             "The same miscorrection without an FECF: decoded as SCID 0x2EA, VC 37, two "
             "corrections; no decoder can tell (OI-35 item 5)", "decode", no_fecf, received_nf),
        case("aos-fhecf-enc-001", ["AOS-019", "AOS-013", "API-021"],
             "Encode with the FHEC and FECF computed by the writer", "encode", cfg,
             params=enc(0, 1, 0xABCDEF, NONE, data.hex().upper())),
        case("aos-fhecf-hdr-001", ["AOS-010", "AOS-014"],
             "The Primary Header alone, two symbols corrected", "decode_header", cfg,
             hit(good, [(0, 0x0F), (5, 0x40)])[:8]),
        case("aos-fhecf-hdr-002", ["AOS-010", "API-013"], "Seven octets with the FHEC: Incomplete",
             "decode_header", cfg, good[:7]),
        case("aos-fhecf-peek-001", ["API-013", "AOS-014"],
             "Seven octets: no version check yet, even of a wrong version", "peek_length", cfg,
             bytes([0xC0]) + good[1:7]),
        case("aos-fhecf-peek-002", ["API-013", "AOS-014"],
             "Eight octets with a correctable version error: corrected first", "peek_length",
             cfg, hit(good, [(0, 0x80)])[:8]),
        case("aos-fhecf-peek-003", ["API-013", "AOS-014"],
             "Eight uncorrectable octets: HeaderUncorrectable", "peek_length", cfg,
             "5111ABCDEF003EDF"),
    ]
    assert pc["fhec"]
    return out


def insert_zone():
    cfg = channel(24, [vc(SCID, 4, "mpdu")], iz=4)
    full = channel(30, [vc(SCID, 4, "mpdu", ocf=True)], fhec=True, iz=3, fecf=True)
    reqs = ["AOS-015", "AOS-010"]
    return [
        case("aos-iz-dec-001", reqs + ["GEN-013"],
             "A 4-octet Insert Zone after the Primary Header, then the M_PDU", "decode", cfg,
             frame(cfg, hdr(SCID, 4, 9), iz=b"\x1A\x1B\x1C\x1D",
                   data=b"\x00\x01" + seq(0x80, 12))),
        case("aos-iz-dec-002", reqs,
             "Insert Zone after an 8-octet header with FHEC, with an OCF and FECF", "decode",
             full, frame(full, hdr(SCID, 4, 9), iz=b"\xC1\xC2\xC3",
                         data=b"\xFF\xFF" + seq(0x80, 11), ocf=CLCW)),
        case("aos-iz-dec-003", reqs + ["AOS-016"],
             "An OID frame keeps the Insert Zone (§4.1.3.3); no OCF on VCID 63 (OI-43 item 1). The data is not the pseudo-noise pattern, and decode accepts it: a stateless decoder cannot check it (the pattern depends on the generator's position across frames; receivers discard it, §4.1.4.1.5.2)",
             "decode", full, frame(full, hdr(SCID, 63, 1), iz=b"\xC1\xC2\xC3",
                                   data=bytes(range(17)))),
        case("aos-iz-enc-001", reqs + ["AOS-019"], "Encode with an Insert Zone", "encode", cfg,
             params=enc(SCID, 4, 9, mp("starts_at", 1), seq(0x80, 12).hex().upper(),
                        iz="1A1B1C1D")),
        case("aos-iz-enc-002", reqs + ["AOS-019", "AOS-013"],
             "Encode with an Insert Zone, FHEC, OCF, and FECF", "encode", full,
             params=enc(SCID, 4, 9, mp("no_packet_start"), seq(0x80, 11).hex().upper(),
                        iz="C1C2C3", ocf=CLCW.hex().upper())),
        case("aos-iz-enc-003", reqs + ["API-022"],
             "An Insert Zone of 3 octets where 4 are configured: LengthMismatch", "encode", cfg,
             params=enc(SCID, 4, 9, mp("starts_at", 1), seq(0x80, 12).hex().upper(),
                        iz="1A1B1C")),
        case("aos-iz-cfg-001", ["AOS-015", "API-040"],
             "An Insert Zone that leaves no data octet: the 6-octet header and the 9-octet Insert Zone fill all 15 octets, and one data octet needs 16. The error names the Physical Channel's frame_len (68), the parameter to change; insert_zone_len (97) is the error of a writer given an Insert Zone of the wrong length", "config",
             channel(15, [], iz=9)),
    ]


def security_layout():
    cfg = channel(32, [vc(SCID, 3, "mpdu", ocf=True, sh=3, st=2)], iz=2, fecf=True)
    reqs = ["API-070", "API-072", "AOS-010"]
    sh, st = b"\xA5\xA5\xA5", b"\x5A\x5A"
    return [
        case("aos-sec-dec-001", reqs,
             "Insert Zone, Security Header 3, M_PDU, Security Trailer 2, OCF, FECF (§6.3)",
             "decode", cfg, frame(cfg, hdr(SCID, 3, 0), iz=b"\x01\x02", sh=sh,
                                  data=b"\x00\x05" + seq(0x30, 11), st=st, ocf=CLCW)),
        case("aos-sec-dec-002", reqs + ["AOS-016"],
             "An OID frame on the same channel: no SDLS fields on VCID 63 (OI-43 item 1). The data is not the pseudo-noise pattern, and decode accepts it (a stateless decoder cannot check it)",
             "decode", cfg, frame(cfg, hdr(SCID, 63, 0), iz=b"\x01\x02", data=bytes(22))),
        case("aos-sec-enc-001", reqs + ["AOS-019"], "Encode with the SDLS fields", "encode", cfg,
             params=enc(SCID, 3, 0, mp("starts_at", 5), seq(0x30, 11).hex().upper(), iz="0102",
                        sh=sh.hex().upper(), st=st.hex().upper(), ocf=CLCW.hex().upper())),
        case("aos-sec-enc-002", reqs + ["API-022"], "A Security Header of 2 octets: LengthMismatch",
             "encode", cfg,
             params=enc(SCID, 3, 0, mp("starts_at", 5), seq(0x30, 11).hex().upper(), iz="0102",
                        sh="A5A5", st=st.hex().upper(), ocf=CLCW.hex().upper())),
        case("aos-sec-enc-003", reqs + ["API-022"], "A Security Trailer of 3 octets: LengthMismatch",
             "encode", cfg,
             params=enc(SCID, 3, 0, mp("starts_at", 5), seq(0x30, 11).hex().upper(), iz="0102",
                        sh=sh.hex().upper(), st="5A5A5A", ocf=CLCW.hex().upper())),
        case("aos-sec-cfg-001", ["API-071", "API-040"], "A Security Header of 65 octets",
             "config", channel(200, [vc(SCID, 3, "vca_sdu", sh=65)])),
        case("aos-sec-cfg-002", ["API-070", "CMN-041"],
             "SDLS fields that leave no data octet", "config",
             channel(20, [vc(SCID, 3, "vca_sdu", sh=8, st=6)])),
    ]


def mpdu_pointers():
    cfg = channel(20, [vc(SCID, 1, "mpdu")])  # a Packet Zone of 12 octets
    reqs = ["AOS-017", "AOS-010"]

    def f(raw):
        return frame(cfg, hdr(SCID, 1, 0), data=raw.to_bytes(2, "big") + seq(0x60, 12))

    out = [
        case("aos-mpdu-dec-001", reqs, "First Header Pointer 0", "decode", cfg, f(0)),
        case("aos-mpdu-dec-002", reqs, "Pointer 11, the last octet of the Packet Zone", "decode",
             cfg, f(11)),
        case("aos-mpdu-dec-003", reqs, "Pointer 0xFFFF: no Packet starts (§4.1.4.2.2.4)",
             "decode", cfg, f(0xFFFF)),
        case("aos-mpdu-dec-004", reqs, "Pointer 0xFFFE: only Idle Data (§4.1.4.2.2.5)", "decode",
             cfg, f(0xFFFE)),
    ]
    out += both("aos-mpdu-dec-005", reqs + ["API-032"],
                "Pointer 12, just past the Packet Zone: Malformed in both modes", cfg, f(12))
    out += both("aos-mpdu-dec-006", reqs + ["API-032"],
                "Pointer 0xFFFD, outside the zone and not reserved: Malformed in both modes", cfg,
                f(0xFFFD))
    out += [
        case("aos-mpdu-enc-001", reqs + ["AOS-019"], "Encode StartsAt(11)", "encode", cfg,
             params=enc(SCID, 1, 0, mp("starts_at", 11), seq(0x60, 12).hex().upper())),
        case("aos-mpdu-enc-002", reqs + ["AOS-019"], "Encode OnlyIdleData", "encode", cfg,
             params=enc(SCID, 1, 0, mp("only_idle_data"), seq(0x60, 12).hex().upper())),
        case("aos-mpdu-enc-003", reqs + ["AOS-019", "API-022"],
             "Encode StartsAt(12), outside the zone: OutOfRange", "encode", cfg,
             params=enc(SCID, 1, 0, mp("starts_at", 12), seq(0x60, 12).hex().upper())),
        case("aos-mpdu-cfg-001", ["AOS-017", "API-040", "CMN-041"],
             "An M_PDU VC with two data octets: no Packet Zone (§4.1.4.2.1.3)", "config",
             channel(8, [vc(SCID, 1, "mpdu")])),
    ]
    return out


def bpdu_pointers():
    cfg = channel(14, [vc(SCID, 2, "bpdu")])  # a Bitstream Data Zone of 6 octets, 48 bits
    reqs = ["AOS-017", "AOS-010"]

    def f(word):
        return frame(cfg, hdr(SCID, 2, 0), data=word.to_bytes(2, "big") + seq(0xB0, 6))

    out = [
        case("aos-bpdu-dec-001", reqs + ["GEN-053"], "Pointer 0: the first bit valid, 1 bit",
             "decode", cfg, f(0)),
        case("aos-bpdu-dec-002", reqs + ["GEN-053"],
             "Pointer 46: 47 valid bits, the last one idle", "decode", cfg, f(46)),
        case("aos-bpdu-dec-003", reqs, "Pointer 0x3FFF: all valid (§4.1.4.3.3.3)", "decode", cfg,
             f(0x3FFF)),
        case("aos-bpdu-dec-004", reqs, "Pointer 0x3FFE: only idle data (§4.1.4.3.3.4)", "decode",
             cfg, f(0x3FFE)),
    ]
    out += both("aos-bpdu-dec-005", reqs + ["API-031", "API-032"],
                "Pointer 47, the zone's last bit: a zone with no idle data has the pointer all "
                "ones (§4.1.4.3.3.3), InvalidField in both modes (the Blue Book does not say a "
                "receiver must reject 47: this is the project's reading, AOS-017, API-031, API-032)", cfg, f(47))
    out += both("aos-bpdu-dec-006", reqs + ["API-032"],
                "Pointer 48, past the zone: Malformed in both modes", cfg, f(48))
    out += both("aos-bpdu-dec-007", reqs + ["API-031", "API-032", "GEN-043"],
                "Reserved Spare 10 (§4.1.4.3.2.2): ReservedValue in strict mode, BpduSpareSet in "
                "lenient mode", cfg, f(0x8000 | 0x3FFF))
    out += [
        case("aos-bpdu-enc-001", reqs + ["AOS-019", "GEN-053"], "Encode ValidBits(47)", "encode",
             cfg, params=enc(SCID, 2, 0, bp("valid_bits", 47), seq(0xB0, 6).hex().upper())),
        case("aos-bpdu-enc-002", reqs + ["AOS-019"], "Encode AllValid", "encode", cfg,
             params=enc(SCID, 2, 0, bp("all_valid"), seq(0xB0, 6).hex().upper())),
        case("aos-bpdu-enc-003", reqs + ["AOS-019"], "Encode OnlyIdle", "encode", cfg,
             params=enc(SCID, 2, 0, bp("only_idle"), seq(0xB0, 6).hex().upper())),
        case("aos-bpdu-enc-004", reqs + ["AOS-019", "API-022"],
             "Encode ValidBits(48), every bit of the zone: OutOfRange, the writer wants AllValid",
             "encode", cfg, params=enc(SCID, 2, 0, bp("valid_bits", 48), seq(0xB0, 6).hex().upper())),
        case("aos-bpdu-enc-005", reqs + ["AOS-019", "API-022"],
             "Encode ValidBits(0): OutOfRange, the writer wants OnlyIdle", "encode", cfg,
             params=enc(SCID, 2, 0, bp("valid_bits", 0), seq(0xB0, 6).hex().upper())),
        case("aos-bpdu-cfg-001", ["AOS-018", "API-040"],
             "Issue 5, B_PDU VC on 2 048-octet frames: accepted", "config",
             channel(2048, [vc(SCID, 2, "bpdu")])),
        case("aos-bpdu-cfg-002", ["AOS-018", "API-040"],
             "Issue 5, B_PDU VC on 2 049-octet frames: InvalidConfig (frame_len). The limit comes from a NOTE to §4.1.4.3.3.4, not a SHALL; rejecting it is a project choice (AOS-018)", "config",
             channel(2049, [vc(SCID, 2, "bpdu")])),
        case("aos-bpdu-cfg-003", ["AOS-018", "API-040"],
             "Issue 5, an M_PDU VC on 2 049-octet frames: accepted", "config",
             channel(2049, [vc(SCID, 2, "mpdu")])),
        case("aos-bpdu-cfg-004", ["AOS-018", "API-040", "GEN-062"],
             "Issue 4, a Bitstream Data Zone of 2 047 octets: accepted", "config",
             channel(2055, [vc(SCID, 2, "bpdu")], edition="issue4")),
        case("aos-bpdu-cfg-005", ["AOS-018", "API-040", "GEN-062"],
             "Issue 4, a Bitstream Data Zone of 2 048 octets: InvalidConfig (frame_data_field)",
             "config", channel(2056, [vc(SCID, 2, "bpdu")], edition="issue4")),
    ]
    return out


def oid():
    cfg = channel(16, [vc(SCID, 0, "mpdu")], fecf=True)
    iz = channel(20, [vc(SCID, 0, "mpdu")], fhec=True, iz=2)
    reqs = ["AOS-016", "CMN-020", "CMN-021"]
    first = encode_oid(build_config(cfg)[0], build_config(cfg)[1],
                       {"scid": SCID, "frames": [{"vc_frame_count": 0}]})["frames_hex"][0]
    return [
        case("aos-oid-enc-001", reqs + ["AOS-019"],
             "Three OID frames from one generator: VCID 63, the data fields continue the PN "
             "sequence (FF FF FF FF 6D B6 D8 61 ...), FECF computed", "encode_oid", cfg,
             params={"scid": SCID, "frames": [{"vc_frame_count": 0}, {"vc_frame_count": 1},
                                              {"vc_frame_count": 2, "cycle": 3}]}),
        case("aos-oid-enc-002", reqs + ["AOS-015", "AOS-013"],
             "OID frames with FHEC and an Insert Zone the caller fills (§4.1.3.3)", "encode_oid",
             iz, params={"scid": SCID, "frames": [{"vc_frame_count": 5, "insert_zone_hex": "ABCD"},
                                                  {"vc_frame_count": 6}]}),
        case("aos-oid-enc-003", reqs + ["API-022"], "An SCID with no VC: UnknownChannel",
             "encode_oid", cfg, params={"scid": SCID + 1, "frames": [{"vc_frame_count": 0}]}),
        case("aos-oid-dec-001", reqs + ["AOS-010"],
             "Decode the first frame of aos-oid-enc-001: Idle Data, no OCF", "decode", cfg, first),
        case("aos-oid-dec-002", ["AOS-016", "AOS-010"],
             "An OID frame of an SCID with no VC: UnknownChannel (spacecraft_id)", "decode", cfg,
             frame(cfg, hdr(SCID + 1, 63, 0), data=bytes(8))),
        case("aos-oid-enc-004", ["AOS-016", "API-022"],
             "VCID 63 through encode, which writes data VCs only: InvalidField (vc_id)", "encode",
             cfg, params=enc(SCID, 63, 0, NONE, "00" * 8)),
        case("aos-oid-cfg-001", ["AOS-016", "API-040"],
             "VCID 63 configured as a data VC: InvalidConfig (vc_id)", "config",
             channel(16, [vc(SCID, 63, "idle_data")])),
    ]


def issue4_compat():
    cfg = channel(14, [vc(0xA5, 5, "mpdu")], edition="issue4")
    big = channel(2060, [vc(0xA5, 5, "mpdu")], edition="issue4")
    reqs = ["AOS-001", "GEN-062"]

    def f(fields, word):
        return frame(cfg, fields, data=word.to_bytes(2, "big") + seq(0x70, 6))

    out = [
        case("aos-i4-dec-001", reqs + ["AOS-002", "AOS-003"],
             "Issue 4: 8-bit SCID 0xA5, bits 42-43 00, M_PDU spare 00000 and pointer 3",
             "decode", cfg, f(hdr(0xA5, 5, edition="issue4"), 3)),
        case("aos-i4-dec-002", reqs + ["AOS-003"], "11-bit pointer 0x7FF: no Packet starts",
             "decode", cfg, f(hdr(0xA5, 5, edition="issue4"), 0x07FF)),
        case("aos-i4-dec-003", reqs + ["AOS-003"], "11-bit pointer 0x7FE: only Idle Data",
             "decode", cfg, f(hdr(0xA5, 5, edition="issue4"), 0x07FE)),
    ]
    out += both("aos-i4-dec-004", reqs + ["AOS-002", "API-031", "API-032", "GEN-043"],
                "Bits 42-43 = 10, a reserved spare in Issue 4: ReservedValue in strict mode, "
                "Issue4SpareSet in lenient mode, SCID still 0xA5", cfg,
                f(hdr(0xA5, 5, edition="issue4", spare=2), 3))
    out += both("aos-i4-dec-005", reqs + ["AOS-003", "API-031", "API-032", "GEN-043"],
                "M_PDU Reserved Spare 00001: ReservedValue in strict mode, Issue4SpareSet in "
                "lenient mode", cfg, f(hdr(0xA5, 5, edition="issue4"), 0x0800 | 3))
    out += both("aos-i4-dec-006", reqs + ["AOS-003", "AOS-017", "API-032"],
                "11-bit pointer 6, past the Packet Zone, with a spare set: Malformed in both "
                "modes (the order of the two checks is a project choice; the Blue Book gives none)", cfg, f(hdr(0xA5, 5, edition="issue4"), 0xF800 | 6))
    i5 = channel(14, [vc(0x2A5, 5, "mpdu")])
    out += [
        case("aos-i4-dec-007", ["AOS-001", "AOS-002", "AOS-003"],
             "The octets of aos-i4-dec-004-s read in Issue 5: SCID 0x2A5, 16-bit pointer 3",
             "decode", i5, f(hdr(0xA5, 5, edition="issue4", spare=2), 3)),
        case("aos-i4-dec-008", ["AOS-001", "AOS-003", "AOS-017"],
             "The octets of aos-i4-dec-002 read in Issue 5: 16-bit pointer 0x07FF is Malformed",
             "decode", channel(14, [vc(0xA5, 5, "mpdu")]), f(hdr(0xA5, 5, edition="issue4"), 0x07FF)),
        case("aos-i4-enc-001", reqs + ["AOS-002", "AOS-003", "GEN-052"],
             "Encode in Issue 4: bits 42-43 and the M_PDU spare written 00 and 00000", "encode",
             cfg, params=enc(0xA5, 5, 0x010203, mp("starts_at", 5), seq(0x70, 6).hex().upper(),
                             replay=True, cycle=0xC)),
        case("aos-i4-enc-002", reqs + ["AOS-003"], "Encode OnlyIdleData in Issue 4: 0x7FE",
             "encode", cfg, params=enc(0xA5, 5, 0, mp("only_idle_data"), seq(0x70, 6).hex().upper())),
        case("aos-i4-enc-003", reqs + ["AOS-003", "AOS-019"],
             "Issue 4, a 2 052-octet Packet Zone: StartsAt(0x7FD) is the largest offset",
             "encode", big, params=enc(0xA5, 5, 0, mp("starts_at", 0x7FD), "00" * 2052)),
        case("aos-i4-enc-004", reqs + ["AOS-003", "AOS-019"],
             "Issue 4: StartsAt(0x7FE) cannot be written: OutOfRange", "encode", big,
             params=enc(0xA5, 5, 0, mp("starts_at", 0x7FE), "00" * 2052)),
        case("aos-i4-cfg-001", reqs + ["AOS-002", "API-040"],
             "Issue 4, a VC of SCID 255: accepted", "config",
             channel(14, [vc(0xFF, 5, "mpdu")], edition="issue4")),
        case("aos-i4-cfg-002", reqs + ["AOS-002", "API-040"],
             "Issue 4, a VC of SCID 256: InvalidConfig (spacecraft_id)", "config",
             channel(14, [vc(0x100, 5, "mpdu")], edition="issue4")),
        case("aos-i4-hdr-001", reqs + ["AOS-002"],
             "The Primary Header alone in Issue 4, bits 42-43 = 11, reported raw as issue4_spare 3: decode_header takes no mode (AOS-010), so this holds in strict and lenient mode alike",
             "decode_header", cfg, f(hdr(0xA5, 5, edition="issue4", spare=3), 3)[:6]),
    ]
    return out


def scid_10bit():
    cfg = channel(12, [vc(0x3FF, 1, "vca_sdu"), vc(0x2A5, 2, "vca_sdu"), vc(0x100, 3, "vca_sdu")])
    fh = channel(14, [vc(0x2A5, 2, "vca_sdu")], fhec=True)
    reqs = ["AOS-002", "AOS-001"]
    return [
        case("aos-scid-dec-001", reqs, "SCID 0x3FF: LSBs FF in bits 2-9, MSBs 11 in bits 42-43",
             "decode", cfg, frame(cfg, hdr(0x3FF, 1), data=bytes(6))),
        case("aos-scid-dec-002", reqs, "SCID 0x2A5: MSBs 10", "decode", cfg,
             frame(cfg, hdr(0x2A5, 2, use=1, cycle=0xF), data=bytes(6))),
        case("aos-scid-dec-003", reqs, "SCID 0x100: LSBs 00, MSBs 01", "decode", cfg,
             frame(cfg, hdr(0x100, 3, replay=1), data=bytes(6))),
        case("aos-scid-dec-004", reqs + ["AOS-010"],
             "SCID 0x0A5 (the same LSBs, MSBs 00) has no VC: UnknownChannel (spacecraft_id)",
             "decode", cfg, frame(cfg, hdr(0x0A5, 2), data=bytes(6))),
        case("aos-scid-dec-005", reqs + ["AOS-013", "AOS-014"],
             "The FHEC protects bits 42-43: the SCID MSB symbol corrected", "decode", fh,
             bytes(a ^ b for a, b in zip(frame(fh, hdr(0x2A5, 2), data=bytes(6)),
                                         [0, 0, 0, 0, 0, 0x30] + [0] * 8))),
        case("aos-scid-enc-001", reqs + ["AOS-019"], "Encode SCID 0x3FF", "encode", cfg,
             params=enc(0x3FF, 1, 0xFFFFFF, NONE, "AB" * 6, cycle=0)),
        case("aos-scid-enc-002", reqs + ["AOS-019", "AOS-013"], "Encode SCID 0x2A5 with the FHEC",
             "encode", fh, params=enc(0x2A5, 2, 0x10, NONE, "CD" * 6)),
        case("aos-scid-cfg-001", reqs + ["API-040"], "Issue 5, a VC of SCID 1023: accepted",
             "config", channel(12, [vc(1023, 1, "mpdu")])),
    ]


def config():
    reqs = ["API-040", "GEN-060"]
    return [
        case("aos-cfg-001", reqs + ["AOS-001", "CMN-041"],
             "Shortest physical channel: 6 + 1 octets", "config", channel(7, [])),
        case("aos-cfg-002", reqs + ["CMN-041"], "6 octets: no data octet (CMN-041, a project rule); the error names the Physical Channel's frame_len (68)", "config",
             channel(6, [])),
        case("aos-cfg-003", reqs + ["CMN-041", "AOS-013"],
             "With FHEC, Insert Zone 2, and FECF the shortest frame is 13 octets: 12 rejected (frame_len, 68)",
             "config", channel(12, [], fhec=True, iz=2, fecf=True)),
        case("aos-cfg-004", reqs + ["CMN-041", "AOS-013"], "...and 13 accepted", "config",
             channel(13, [], fhec=True, iz=2, fecf=True)),
        case("aos-cfg-005", reqs, "The same VC given twice: InvalidConfig (vc_id)", "config",
             channel(20, [vc(1, 1, "mpdu"), vc(1, 1, "vca_sdu")])),
        case("aos-cfg-006", reqs, "The same VCID on two spacecraft: accepted", "config",
             channel(20, [vc(1, 1, "mpdu"), vc(2, 1, "vca_sdu")])),
        case("aos-cfg-007", reqs + ["CMN-041"],
             "An OCF that leaves no data octet: InvalidConfig (frame_data_field)", "config",
             channel(10, [vc(1, 1, "vca_sdu", ocf=True)])),
        case("aos-cfg-008", reqs + ["CMN-041", "AOS-017"],
             "A B_PDU VC with three data octets: accepted", "config",
             channel(9, [vc(1, 1, "bpdu")])),
        case("aos-cfg-009", reqs + ["CMN-041", "AOS-017"],
             "A B_PDU VC with two data octets: InvalidConfig (frame_data_field)", "config",
             channel(8, [vc(1, 1, "bpdu")])),
        case("aos-cfg-010", reqs, "A VC of SCID 1024: OutOfRange", "config",
             channel(20, [vc(1024, 1, "mpdu")])),
        case("aos-cfg-011", reqs, "A VC of VCID 64: OutOfRange", "config",
             channel(20, [vc(1, 64, "mpdu")])),
    ]


def invalid():
    cfg = channel(20, [vc(SCID, 1, "mpdu", ocf=True), vc(SCID, 2, "vca_sdu")], fecf=True)
    good = frame(cfg, hdr(SCID, 1, 0), data=b"\xFF\xFF" + bytes(6), ocf=CLCW)
    reqs = ["AOS-010", "GEN-041"]

    def resealed(octets):
        body = octets[:-2]
        return body + crc_fecf(body).to_bytes(2, "big")

    def with_version(v):
        b = bytearray(good)
        b[0] = (b[0] & 0x3F) | (v << 6)
        return resealed(bytes(b))

    zone = "00" * 6
    out = [
        case("aos-inv-dec-001", reqs + ["API-012"], "One octet short: LengthMismatch", "decode",
             cfg, good[:-1]),
        case("aos-inv-dec-002", reqs + ["API-012"], "One octet long: LengthMismatch", "decode",
             cfg, good + b"\x00"),
        case("aos-inv-dec-003", reqs + ["CMN-003"], "A data bit flipped: CrcMismatch", "decode",
             cfg, good[:10] + bytes([good[10] ^ 1]) + good[11:]),
    ]
    for i, v in enumerate((0, 2, 3)):
        out.append(case(f"aos-inv-dec-{4 + i:03d}", reqs + ["AOS-011"],
                        f"Version {v:02b} with a valid FECF: UnsupportedVersion", "decode", cfg,
                        with_version(v)))
    stale = bytearray(good)
    stale[0] = (stale[0] & 0x3F) | (2 << 6)
    out += [
        # Added 2026-10-25 by a blind Claude Sonnet 5.5 agent (M2 WP-8 follow-up 11).
        case("aos-inv-dec-009", reqs + ["AOS-011", "AOS-014", "CMN-003"],
             "Version 10 with the FECF of the version-01 frame left unchanged: both the "
             "version and the FECF are wrong, and the FECF check comes first (the order "
             "of checks is a project choice, aos-frame.md section 4), so CrcMismatch, not "
             "UnsupportedVersion", "decode", cfg, bytes(stale)),
        case("aos-inv-dec-007", reqs, "VC 3 is not configured: UnknownChannel (vc_id)", "decode",
             cfg, frame(cfg, hdr(SCID, 3, 0), data=bytes(12))),
        case("aos-inv-dec-008", reqs, "SCID 0x56 has no VC: UnknownChannel (spacecraft_id)",
             "decode", cfg, frame(cfg, hdr(SCID + 1, 1, 0), data=bytes(8), ocf=CLCW)),
        case("aos-inv-enc-001", ["AOS-019", "API-022"], "An unknown VC: UnknownChannel",
             "encode", cfg, params=enc(SCID, 3, 0, NONE, zone)),
        case("aos-inv-enc-002", ["AOS-019", "API-022"], "An unknown SCID: UnknownChannel",
             "encode", cfg, params=enc(SCID + 1, 1, 0, mp("no_packet_start"), zone,
                                       ocf=CLCW.hex().upper())),
        case("aos-inv-enc-003", ["AOS-019", "API-022"],
             "An M_PDU Header on a VCA_SDU VC: InvalidField (data_field_content)", "encode", cfg,
             params=enc(SCID, 2, 0, mp("no_packet_start"), zone)),
        case("aos-inv-enc-004", ["AOS-019", "API-022"],
             "No M_PDU Header on an M_PDU VC: InvalidField (data_field_content)", "encode", cfg,
             params=enc(SCID, 1, 0, NONE, zone, ocf=CLCW.hex().upper())),
        case("aos-inv-enc-005", ["AOS-019", "API-022"],
             "A B_PDU Header on an M_PDU VC: InvalidField (data_field_content)", "encode", cfg,
             params=enc(SCID, 1, 0, bp("all_valid"), zone, ocf=CLCW.hex().upper())),
        case("aos-inv-enc-006", ["AOS-019", "API-022"],
             "No OCF on a VC with an OCF: InvalidField (ocf)", "encode", cfg,
             params=enc(SCID, 1, 0, mp("no_packet_start"), zone)),
        case("aos-inv-enc-007", ["AOS-019", "API-022"],
             "An OCF on a VC without one: InvalidField (ocf)", "encode", cfg,
             params=enc(SCID, 2, 0, NONE, "00" * 12, ocf=CLCW.hex().upper())),
        case("aos-inv-enc-008", ["AOS-019", "API-022"],
             "A zone one octet short: LengthMismatch (frame_data_field)", "encode", cfg,
             params=enc(SCID, 1, 0, mp("no_packet_start"), "00" * 5, ocf=CLCW.hex().upper())),
        case("aos-inv-enc-009", ["AOS-019", "API-022", "GEN-014"],
             "An output buffer one octet short: BufferTooSmall", "encode", cfg,
             params=enc(SCID, 1, 0, mp("no_packet_start"), zone, ocf=CLCW.hex().upper(),
                        out_len=19)),
        case("aos-inv-enc-010", ["AOS-019", "API-001"],
             "A VC Frame Count of 2^24: OutOfRange (vc_frame_count)", "encode", cfg,
             params=enc(SCID, 2, 0x1000000, NONE, "00" * 12)),
        case("aos-inv-enc-011", ["AOS-019", "API-001"],
             "A cycle of 16: OutOfRange (vc_frame_count_cycle)", "encode", cfg,
             params=enc(SCID, 2, 0, NONE, "00" * 12, cycle=16)),
    ]
    return out


FILES = {
    "minimal": (minimal, "732.0-B-5 EC1 §4.1, §5"),
    "fhec_frame": (fhec_frame, "732.0-B-5 EC1 §4.1.2.6, §4.3.7"),
    "insert_zone": (insert_zone, "732.0-B-5 EC1 §4.1.3"),
    "security_layout": (security_layout, "732.0-B-5 EC1 §6.3, Table 6-1"),
    "mpdu_pointers": (mpdu_pointers, "732.0-B-5 EC1 §4.1.4.2"),
    "bpdu_pointers": (bpdu_pointers, "732.0-B-5 EC1 §4.1.4.3"),
    "oid": (oid, "732.0-B-5 EC1 §4.1.4.1.5, Annex D"),
    "issue4_compat": (issue4_compat, "732.0-B-4 §4.1.2, §4.1.4.2"),
    "scid_10bit": (scid_10bit, "732.0-B-5 EC1 §4.1.2.2.3, §4.1.2.5.4"),
    "config": (config, "732.0-B-5 EC1 §5"),
    "invalid": (invalid, "732.0-B-5 EC1 §4.1"),
}

NOTE = (
    "732.0-B-5 EC1 prints no example frame, only the first OID octets of Annex D. Cases "
    "hand-derived from 732.0-B-5 EC1 §4.1 (figures 4-1 to 4-4; the Primary Header of §4.1.2 "
    "with the SCID's two most significant bits in bits 42-43; the FHEC of §4.1.2.6; the Insert "
    "Zone of §4.1.3; OID frames on VCID 63 of §4.1.4.1.5 and Annex D; the M_PDU and B_PDU of "
    "§4.1.4.2-3 with their reserved pointer values; the OCF per VC of §4.1.5; the FECF of "
    "§4.1.6), §4.3.7.2-3 (the header corrected before the CRC is checked), §5 Tables 5-1 to 5-3, "
    "and §6.3, and from 732.0-B-4 §4.1.2.2.3, §4.1.2.5.4, and §4.1.4.2.2-3 for Issue 4, with "
    "the readings of docs/standards/aos-frame.md (AOS-001...AOS-019, the anomaly list, OI-43) "
    "and the error kinds and field codes of docs/03-api-model.md §9 and §9.1. Inputs assembled "
    "bit by bit from the figure widths; expected values computed by an independent Python "
    "model of those clauses, written from the Blue Book after the Rust codec but not from its "
    "code, and sharing no code with it (the FECF by polynomial division over the corrected "
    "header, the OID data by the Annex D recurrence, the FHEC by the table model aos_fhec.py of "
    "the fhec_*.json files, loaded from its path). The model is committed as "
    "test-vectors/models/aos_frame.py (TV-023); `python3 -I test-vectors/models/aos_frame.py "
    "--check test-vectors/aos/<file>.json` recomputes every expected value. One frame is "
    "worked by hand in docs/verification/derivations/aos-frame.md. Author of the model: an AI "
    "agent, Claude Opus 5.5, recorded as co-author of commit 5bbbf45 (2026-10-23); it wrote "
    "the model from 732.0-B-5 EC1, 732.0-B-4 and docs/standards/aos-frame.md, and states that "
    "it did not write it from the code of the Rust codec (the history shows the codec already "
    "existed and records no briefing of the agent beyond that statement; TV-023). The case aos-inv-dec-009 was added on 2026-10-25 (M2 "
    "WP-8) by a second agent, Claude Sonnet 5.5, briefed to read only the Blue Book, docs/, and "
    "the vectors, never the implementation, and to extend this model."
)


def generate(name):
    make, standard = FILES[name]
    return {"kind": "codec", "standard": standard, "provenance": "hand-derived",
            "provenance_note": NOTE + " This file covers " + standard + ".", "cases": make()}


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
            print(f"aos_frame: {bad} difference(s)")
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
