"""Independent model of the USLP Transfer Frame, the truncated frame, OID frames, Frame
Delimiting and Fill Data Removal, and the USLP Frame Validation Checks, for the vector files
of test-vectors/uslp/ (USLP-001...USLP-016; M2 WP-5).

Sources: CCSDS 732.1-B-3: the frame of §4.1 (the field widths of the Primary Header in
§4.1.2.1.2 and Figure 4-2, TFVN 1100 of §4.1.2.2.2.2, Table 4-1 for the Bypass/Sequence
Control and Protocol Control Command Flags, Table 4-2 for the VCF Count Length, the Insert
Zone of §4.1.3, the TFDF Header of §4.1.4.2 and Table 4-3, the OID frame of §4.1.4.1.5-12,
the OCF of §4.1.5, the FECF of §4.1.6 and Annex B), the reception procedures of §4.3.11.2
and §4.3.11.3, the managed parameters of §5 (Tables 5-1 to 5-4), the SDLS fields of §6.3,
the truncated frame of Annex D, and the OID generator of §4.1.4.1.10 and Annex H. The UPID
values are read at run time from the committed SANA snapshot specs/sana/uslp_protocol_id.tsv
(OI-06), never written here. Parameters and code only, no Blue Book text.

What the Blue Book leaves to the implementation is taken from docs/standards/uslp-frame.md:
the configuration the codec keeps and its errors (§2.1), the check order, error kinds, and
reason codes (§3.1), the delimiter's valid header (§3.2), and the writers' error order (§3.3);
and from the vector file contract of the aldis-uslp host runner (the JSON keys and the order
in which a configuration is built).

The model was written from those clauses, not from the Rust code (crates/aldis-uslp), and
must stay independent of it: never port code from it, and never fix the model to agree with
it. Its methods: the headers are assembled and read as bit strings from the field widths of
Figure 4-2 and Figure D-2; the FECF is the remainder of the Annex B equation computed by
long division of Python integers over GF(2), and a received FECF is checked with the
syndrome of B1.2.1; the OID data come from the Galois form of Figure H-2 with its seed,
checked at start-up against the Fibonacci recurrence of Figure H-1 and the printed pattern.

Usage, from the repository root:

    python3 -I test-vectors/models/uslp_frame.py --check test-vectors/uslp/*.json
    python3 -I test-vectors/models/uslp_frame.py --write test-vectors/uslp

--check recomputes the expected outcome of every live case from its operation, configuration,
parameters, and input, compares the cases the generator writes with the files', and checks
the field codes against test-vectors/core/fields.json; it exits 1 on any difference. --write
regenerates the files (cases and provenance note).
"""
import json
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# Field codes of docs/03-api-model.md §9.1 (checked against test-vectors/core/fields.json).
F = {
    "spacecraft_id": 1, "vc_id": 2, "map_id": 3, "frame_data_field": 7,
    "security_header_len": 8, "security_trailer_len": 9, "frame_version": 67, "frame_len": 68,
    "ocf_flag": 69, "max_frame_len": 79, "protocol_control_command_flag": 89,
    "uslp_header_spare": 90, "end_of_frame_primary_header_flag": 91, "vcf_count_len": 92,
    "vcf_count": 93, "tfdz_construction_rule": 94, "upid": 95, "fhp_lvo_pointer": 96,
    "insert_zone_len": 97, "truncated_frame_len": 98, "transfer_frame_type": 99,
}

# Validation reasons, docs/standards/uslp-frame.md §3.1 (USLP-014, D14).
REASONS = {
    "UnexpectedVersion": 1, "UnexpectedSpacecraft": 2, "ReservedFlags": 3, "ReservedSpare": 4,
    "InvalidVcid": 5, "InvalidMapId": 6, "TruncatedFrameNotAllowed": 7,
    "VcfCountLengthMismatch": 8, "OcfFlagMismatch": 9, "FixedLengthMismatch": 10,
    "FrameTooLong": 11, "FrameTooShort": 12, "LengthMismatch": 13, "FecfMismatch": 14,
    "ConstructionRuleMismatch": 15, "PointerBeyondZone": 16, "OidHeaderMismatch": 17,
}

# Anomaly flags in bit order, docs/standards/uslp-frame.md §4.
ANOMALIES = ["ReserveSparesSet", "OidHeaderMismatch"]

# §4.1.2.1.2 and Figure 4-2: the thirteen fields of the non-truncated Primary Header, the
# VCF Count (0-7 octets) following the first twelve. Annex D1.3.1, Figure D-2: the truncated
# header is the first six.
PRIMARY = [("tfvn", 4), ("scid", 16), ("sod", 1), ("vcid", 6), ("map", 4), ("eofph", 1),
           ("frame_len", 16), ("bypass", 1), ("pcc", 1), ("spare", 2), ("ocf", 1),
           ("vcf_len", 3)]
TRUNCATED = PRIMARY[:6]
TFVN = 0b1100            # §4.1.2.2.2.2
FIXED_HEADER = sum(w for _, w in PRIMARY) // 8    # 7 octets before the VCF Count
TRUNC_HEADER = sum(w for _, w in TRUNCATED) // 8  # 4 octets, Annex D1.2.2 a)
MAX_FRAME = 1 << 16      # §4.1.2.7 note 4: a 16-bit length count
OID_VCID = (1 << 6) - 1  # §4.1.4.1.6: all ones
NO_POINTER = 0xFFFF      # §4.1.4.2.4.4 and §4.1.4.2.4.6: all ones
TRUNC_RANGE = (6, 32)    # Table 5-3, Annex D1.3.2 notes 2 and 3
TRUNC_SDLS = (6, 16)     # Annex D1.2.3: Security Header and Trailer of a truncated frame
TRUNC_SDLS_MIN = 28      # Annex D1.2.4
OCF_LEN, FECF_LEN = 4, 2  # §4.1.5.2, §4.1.6.2.2
# Table 4-1: (Bypass/Sequence Control Flag, Protocol Control Command Flag); (0, 1) reserved.
KINDS = {(0, 0): "sequence_controlled", (1, 0): "expedited", (1, 1): "protocol_control"}
FLAGS = {v: k for k, v in KINDS.items()}
FIXED_RULES = (0b000, 0b001, 0b010)  # Table 4-3: the rules of a fixed-length TFDZ

# The contract's limits of a SecurityLayout and of the VC table (not Blue Book values).
MAX_SEC_HEADER, MAX_SEC_TRAILER = 64, 65535
VC_CAPACITY, MAP_CAPACITY = 16, 32

SCID = 0x155        # the fictional SCID of the reference scenario (APP-026)
SCID16 = 0xF0A5     # a fictional 16-bit SCID


class Fail(Exception):
    """An error outcome: kind (catalogue name), field code or None, validation reason."""

    def __init__(self, kind, field=None, reason=None):
        super().__init__(kind)
        self.kind, self.field, self.reason = kind, field, reason


def hx(b):
    return bytes(b).hex().upper()


def fail(kind, field=None, reason=None):
    raise Fail(kind, F[field] if field else None, reason)


# ---- the SANA UPID registry snapshot (OI-06), read at run time

def load_upids():
    path = os.path.join(ROOT, "specs", "sana", "uslp_protocol_id.tsv")
    rows, header = {}, None
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.rstrip("\n")
            if not line or line.startswith("#"):
                continue
            cols = line.split("\t")
            if header is None:
                header = cols
                continue
            row = dict(zip(header, cols + [""] * (len(header) - len(cols))))
            rows[int(row["upid"], 2)] = row
    return rows


UPIDS = load_upids()


def upid_where(text):
    found = [k for k, r in UPIDS.items() if text.lower() in r["description"].lower()]
    if len(found) != 1:
        raise ValueError(f"the UPID snapshot has {len(found)} rows matching {text!r}")
    return found[0]


UPID_PACKETS = upid_where("Space Packets or Encapsulation packets")
UPID_COP1 = upid_where("COP-1 Control Commands")
UPID_OCTETS = upid_where("Octet Stream")
UPID_MSI1 = upid_where("Mission-specific Information-1")
UPID_IDLE = upid_where("contains idle data")
UPID_UNASSIGNED = min(k for k, r in UPIDS.items() if r["status"] == "Unassigned")


def upid_assigned(v):
    return UPIDS.get(v, {}).get("status") == "Assigned"


# ---- bit strings (bit 0 first, §1.6)

def pack(layout, values):
    bits = ""
    for name, width in layout:
        v = values[name]
        if not 0 <= v < 1 << width:
            raise ValueError(f"{name}={v} does not fit {width} bits")
        bits += format(v, f"0{width}b")
    return bytes(int(bits[i:i + 8], 2) for i in range(0, len(bits), 8))


def unpack(layout, octets):
    bits = "".join(format(o, "08b") for o in octets)
    out, at = {}, 0
    for name, width in layout:
        out[name] = int(bits[at:at + width], 2)
        at += width
    return out


# ---- FECF, Annex B: G(X) = X^16 + X^12 + X^5 + 1, L(X) = X^15 + ... + 1

G = (1 << 16) | (1 << 12) | (1 << 5) | 1
L = (1 << 16) - 1


def gf2_mod(a, g):
    while a.bit_length() >= g.bit_length():
        a ^= g << (a.bit_length() - g.bit_length())
    return a


def fecf(message):
    """B1.1.3: FECF = (X^16 M(X) + X^(n-16) L(X)) mod G(X), n-16 the message bits."""
    k = 8 * len(message)
    return gf2_mod((int.from_bytes(message, "big") << 16) ^ (L << k), G)


def syndrome(block):
    """B1.2.1: S(X) = (X^16 C*(X) + X^n L(X)) mod G(X), zero when no error is detected."""
    n = 8 * len(block)
    return gf2_mod((int.from_bytes(block, "big") << 16) ^ (L << n), G)


def seal(body):
    return body + fecf(body).to_bytes(FECF_LEN, "big")


# ---- OID data, §4.1.4.1.10 and Annex H: polynomial 1 + D + D^2 + D^22 + D^32

GALOIS_SEED = "00000000001111111111111111111101"  # §4.1.4.1.10.2
GALOIS_TAPS = sum(1 << (k - 1) for k in (32, 22, 2, 1))


class OidGenerator:
    """Figure H-2 (Galois form): the output is the last cell; when it is 1, it is added into
    the cells of the polynomial's terms as the register shifts."""

    def __init__(self):
        self.state = int(GALOIS_SEED, 2)

    def octets(self, n):
        out = bytearray()
        for _ in range(n):
            octet = 0
            for _ in range(8):
                bit = self.state & 1
                self.state >>= 1
                if bit:
                    self.state ^= GALOIS_TAPS
                octet = (octet << 1) | bit
            out.append(octet)
        return bytes(out)


def oid_fibonacci(n):
    """Figure H-1 (Fibonacci form, all-ones seed): x[k] = x[k-1] + x[k-2] + x[k-22] + x[k-32]."""
    x = [1] * 32
    while len(x) < 8 * n:
        k = len(x)
        x.append(x[k - 1] ^ x[k - 2] ^ x[k - 22] ^ x[k - 32])
    return bytes(int("".join(map(str, x[8 * i:8 * i + 8])), 2) for i in range(n))


def oid_octets(skip, n):
    g = OidGenerator()
    g.octets(skip)
    return g.octets(n)


# ---- managed parameters (§5; uslp-frame.md §2.1; the contract's build order)

def check_security(sh, st):
    if sh > MAX_SEC_HEADER:
        fail("InvalidConfig", "security_header_len")
    if st > MAX_SEC_TRAILER:
        fail("InvalidConfig", "security_trailer_len")


def config(c):
    if c is None:
        raise ValueError("every case needs a config")
    pc_fixed = c["frame_type"] == "fixed"
    length, iz, has_fecf = c["frame_len"], c["insert_zone_len"], c["fecf"]
    if not 9 <= length <= MAX_FRAME:
        fail("InvalidConfig", "frame_len" if pc_fixed else "max_frame_len")
    if iz > 0 and not pc_fixed:
        fail("InvalidConfig", "insert_zone_len")                  # Table 5-1 note 5
    vcs = []
    for v in c["vcs"]:
        if v["vcid"] > OID_VCID:
            fail("OutOfRange", "vc_id")
        for key in ("vcf_count_len_sequence", "vcf_count_len_expedited"):
            if v[key] > 7:
                fail("InvalidConfig", "vcf_count_len")             # Table 4-2
        t = v["truncated_frame_len"]
        if t is not None and not TRUNC_RANGE[0] <= t <= TRUNC_RANGE[1]:
            fail("InvalidConfig", "truncated_frame_len")
        sec = v["security"]
        if sec != "per_map":
            check_security(sec["header_len"], sec["trailer_len"])
        fixed = v["frame_type"] == "fixed"
        if v["ocf"] == "allowed" and fixed or v["ocf"] == "required" and not fixed:
            fail("InvalidConfig", "ocf_flag")                      # Table 5-3
        if t is not None and fixed:
            fail("InvalidConfig", "truncated_frame_len")           # Annex D1.2.1
        if v["vcid"] == OID_VCID and not fixed:
            fail("InvalidConfig", "transfer_frame_type")           # USLP-016
        if v["vcid"] == OID_VCID and sec == "per_map":
            fail("InvalidConfig", "vc_id")
        vcs.append({"scid": v["scid"], "vcid": v["vcid"], "fixed": fixed,
                    "len": {0: v["vcf_count_len_sequence"], 1: v["vcf_count_len_expedited"]},
                    "ocf": v["ocf"], "trunc": t,
                    "sec": None if sec == "per_map" else (sec["header_len"], sec["trailer_len"])})
    maps = []
    for m in c["maps"]:
        if m["vcid"] > OID_VCID:
            fail("OutOfRange", "vc_id")
        if m["map_id"] > 15:
            fail("OutOfRange", "map_id")
        check_security(m["security_header_len"], m["security_trailer_len"])
        maps.append({"scid": m["scid"], "vcid": m["vcid"], "map": m["map_id"],
                     "sec": (m["security_header_len"], m["security_trailer_len"])})
    cfg = {"fixed": pc_fixed, "len": length, "iz": iz, "fecf": has_fecf, "vcs": [], "maps": []}

    def room(v, sec):
        return (length - (FIXED_HEADER + max(v["len"].values())) - iz - sec[0] - sec[1]
                - (3 if v["fixed"] else 1) - (OCF_LEN if v["ocf"] != "absent" else 0)
                - (FECF_LEN if has_fecf else 0))

    for v in vcs:
        if vc_of(cfg, v["scid"], v["vcid"]) is not None:
            fail("InvalidConfig", "vc_id")
        if not v["fixed"] and pc_fixed:
            fail("InvalidConfig", "transfer_frame_type")           # Table 5-3 note 2
        if v["vcid"] == OID_VCID and not pc_fixed:
            fail("InvalidConfig", "vc_id")                         # Table 5-1 note 4
        if v["sec"] is not None and room(v, v["sec"]) < 1:
            fail("InvalidConfig", "frame_data_field")              # CMN-041
        if len(cfg["vcs"]) == VC_CAPACITY:
            fail("CapacityExceeded", "vc_id")
        cfg["vcs"].append(v)
    for m in maps:
        v = vc_of(cfg, m["scid"], m["vcid"])
        if v is None or m["vcid"] == OID_VCID:
            fail("InvalidConfig", "map_id")
        if map_of(cfg, m["scid"], m["vcid"], m["map"]) is not None:
            fail("InvalidConfig", "map_id")
        if v["sec"] is not None and m["sec"] != (0, 0):
            fail("InvalidConfig", "map_id")
        if v["sec"] is None and room(v, m["sec"]) < 1:
            fail("InvalidConfig", "frame_data_field")
        if len(cfg["maps"]) == MAP_CAPACITY:
            fail("CapacityExceeded", "map_id")
        cfg["maps"].append(m)
    for v in cfg["vcs"]:
        if v["trunc"] is not None and v["trunc"] < TRUNC_SDLS_MIN and uses_sdls(cfg, v):
            fail("InvalidConfig", "truncated_frame_len")           # Annex D1.2.4
    return cfg


def vc_of(cfg, scid, vcid):
    return next((v for v in cfg["vcs"] if (v["scid"], v["vcid"]) == (scid, vcid)), None)


def map_of(cfg, scid, vcid, map_id):
    return next((m for m in cfg["maps"]
                 if (m["scid"], m["vcid"], m["map"]) == (scid, vcid, map_id)), None)


def uses_sdls(cfg, v, m=None):
    """A truncated frame uses SDLS when the layout of its VC or MAP is not empty."""
    if v["sec"] is not None:
        return v["sec"] != (0, 0)
    if m is not None:
        return m["sec"] != (0, 0)
    return any(n["sec"] != (0, 0) for n in cfg["maps"]
               if (n["scid"], n["vcid"]) == (v["scid"], v["vcid"]))


def security(v, m, pcc):
    """§6.3.1: a protocol control frame has no Security Header or Trailer."""
    if pcc:
        return (0, 0)
    if v["sec"] is not None:
        return v["sec"]
    return m["sec"]


def overhead(cfg, v, sec, vcf_len, ocf):
    """Octets of a non-truncated frame other than its TFDZ."""
    return (FIXED_HEADER + vcf_len + cfg["iz"] + sec[0] + (3 if v["fixed"] else 1) + sec[1]
            + (OCF_LEN if ocf else 0) + (FECF_LEN if cfg["fecf"] else 0))


def pointer_kind(rule, pointer):
    if rule == 0:
        return "no_packet_start_or_end" if pointer == NO_POINTER else "packet_starts_at"
    return "sdu_continues" if pointer == NO_POINTER else "last_valid_octet"


# ---- the decoder and the USLP Frame Validation Checks (uslp-frame.md §3.1)

def parse(cfg, data, mode, exact):
    lenient = mode == "lenient"
    anomalies = []

    def short():
        if exact:
            fail("LengthMismatch", "frame_len", "LengthMismatch")
        fail("Incomplete")

    if len(data) < TRUNC_HEADER:
        short()
    t = unpack(TRUNCATED, data[:TRUNC_HEADER])
    if t["eofph"] == 0 and len(data) < FIXED_HEADER:
        short()
    if t["tfvn"] != TFVN:                                                     # (a)
        fail("UnsupportedVersion", "frame_version", "UnexpectedVersion")
    if not any(v["scid"] == t["scid"] for v in cfg["vcs"]):                   # (b)
        fail("UnknownChannel", "spacecraft_id", "UnexpectedSpacecraft")
    if t["eofph"] == 1:
        return parse_truncated(cfg, data, t, exact)
    h = unpack(PRIMARY, data[:FIXED_HEADER])
    if (h["bypass"], h["pcc"]) not in KINDS:                                  # (c)
        fail("ReservedValue", "protocol_control_command_flag", "ReservedFlags")
    if h["spare"] != 0:
        if not lenient:
            fail("ReservedValue", "uslp_header_spare", "ReservedSpare")
        anomalies.append("ReserveSparesSet")
    v = vc_of(cfg, h["scid"], h["vcid"])
    if v is None:
        fail("UnknownChannel", "vc_id", "InvalidVcid")
    m = None
    if h["vcid"] != OID_VCID:
        m = map_of(cfg, h["scid"], h["vcid"], h["map"])
        if m is None:
            fail("UnknownChannel", "map_id", "InvalidMapId")
    if h["vcf_len"] != v["len"][h["bypass"]]:
        fail("LengthMismatch", "vcf_count_len", "VcfCountLengthMismatch")
    if h["ocf"] == 1 and v["ocf"] == "absent" or h["ocf"] == 0 and v["ocf"] == "required":
        fail("InvalidField", "ocf_flag", "OcfFlagMismatch")
    total = h["frame_len"] + 1
    if cfg["fixed"] and total != cfg["len"]:
        fail("LengthMismatch", "frame_len", "FixedLengthMismatch")
    if not cfg["fixed"] and total > cfg["len"]:
        fail("OutOfRange", "frame_len", "FrameTooLong")
    sec = security(v, m, h["pcc"])
    if total < overhead(cfg, v, sec, h["vcf_len"], h["ocf"]) + 1:
        fail("InvalidField", "frame_data_field", "FrameTooShort")
    if (exact or cfg["fixed"]) and len(data) != total:                       # (d)
        fail("LengthMismatch", "frame_len", "LengthMismatch")
    if len(data) < total:
        fail("Incomplete")
    frame = data[:total]
    if cfg["fecf"] and syndrome(frame) != 0:                                  # (e)
        fail("CrcMismatch", None, "FecfMismatch")
    at = FIXED_HEADER
    count = int.from_bytes(frame[at:at + h["vcf_len"]], "big")
    at += h["vcf_len"]
    iz = frame[at:at + cfg["iz"]]
    at += cfg["iz"]
    sec_header = frame[at:at + sec[0]]
    at += sec[0]
    rule, upid = frame[at] >> 5, frame[at] & 0x1F
    at += 1
    fixed_vc = v["fixed"] or h["vcid"] == OID_VCID
    if (rule in FIXED_RULES) != fixed_vc:                                     # USLP-016
        fail("InvalidField", "tfdz_construction_rule", "ConstructionRuleMismatch")
    pointer = None
    if rule in FIXED_RULES:
        pointer = int.from_bytes(frame[at:at + 2], "big")
        at += 2
    end = total - (FECF_LEN if cfg["fecf"] else 0) - (OCF_LEN if h["ocf"] else 0)
    ocf = frame[end:end + OCF_LEN] if h["ocf"] else None
    tfdz = frame[at:end - sec[1]]
    trailer = frame[end - sec[1]:end]
    if pointer is not None and pointer != NO_POINTER and pointer >= len(tfdz):  # USLP-006
        fail("Malformed", "fhp_lvo_pointer", "PointerBeyondZone")
    if h["vcid"] == OID_VCID:                                                 # §4.1.4.1.8-9
        wrong = [name for name, bad in [
            ("map_id", h["map"] != 0), ("tfdz_construction_rule", rule != 0b001),
            ("upid", upid != UPID_IDLE), ("fhp_lvo_pointer", pointer != len(tfdz) - 1)] if bad]
        if wrong:
            if not lenient:
                fail("InvalidField", wrong[0], "OidHeaderMismatch")
            anomalies.append("OidHeaderMismatch")
    return {
        "truncated": False, "spacecraft_id": h["scid"],
        "source_or_destination": "destination" if h["sod"] else "source",
        "vc_id": h["vcid"], "map_id": h["map"], "kind": KINDS[(h["bypass"], h["pcc"])],
        "frame_len": total, "ocf_flag": h["ocf"] == 1, "vcf_count_len": h["vcf_len"],
        "vcf_count": count, "insert_zone_hex": hx(iz), "security_header_hex": hx(sec_header),
        "tfdz_construction_rule": rule, "upid": upid, "upid_assigned": upid_assigned(upid),
        "pointer": pointer,
        "pointer_kind": None if pointer is None else pointer_kind(rule, pointer),
        "tfdz_hex": hx(tfdz), "security_trailer_hex": hx(trailer),
        "ocf_hex": None if ocf is None else hx(ocf),
        "fecf_hex": hx(frame[-FECF_LEN:]) if cfg["fecf"] else None,
        "consumed": total, "anomalies": sorted(anomalies, key=ANOMALIES.index),
    }


def parse_truncated(cfg, data, t, exact):
    """Annex D: reasons 5, 6, 7, 13 (d)(2), and 15."""
    v = vc_of(cfg, t["scid"], t["vcid"])
    if v is None:
        fail("UnknownChannel", "vc_id", "InvalidVcid")
    m = None
    if t["vcid"] != OID_VCID:
        m = map_of(cfg, t["scid"], t["vcid"], t["map"])
        if m is None:
            fail("UnknownChannel", "map_id", "InvalidMapId")
    if v["trunc"] is None:
        fail("InvalidField", "end_of_frame_primary_header_flag", "TruncatedFrameNotAllowed")
    total = v["trunc"]
    if exact and len(data) != total:
        fail("LengthMismatch", "frame_len", "LengthMismatch")
    if len(data) < total:
        fail("Incomplete")
    frame = data[:total]
    sh, st = TRUNC_SDLS if uses_sdls(cfg, v, m) else (0, 0)
    at = TRUNC_HEADER + sh
    rule, upid = frame[at] >> 5, frame[at] & 0x1F
    if rule in FIXED_RULES:                                                   # USLP-016
        fail("InvalidField", "tfdz_construction_rule", "ConstructionRuleMismatch")
    return {
        "truncated": True, "spacecraft_id": t["scid"],
        "source_or_destination": "destination" if t["sod"] else "source",
        "vc_id": t["vcid"], "map_id": t["map"], "frame_len": total,
        "security_header_hex": hx(frame[TRUNC_HEADER:at]),
        "tfdz_construction_rule": rule, "upid": upid, "upid_assigned": upid_assigned(upid),
        "tfdz_hex": hx(frame[at + 1:total - st]), "security_trailer_hex": hx(frame[total - st:]),
        "consumed": total, "anomalies": [],
    }


def peek(cfg, prefix):
    """API-013 as uslp-frame.md §3.1 (last paragraph) states it."""
    if len(prefix) < TRUNC_HEADER:
        fail("Incomplete")
    t = unpack(TRUNCATED, prefix[:TRUNC_HEADER])
    if t["tfvn"] != TFVN:
        fail("UnsupportedVersion", "frame_version")
    if t["eofph"] == 1:
        if not any(v["scid"] == t["scid"] for v in cfg["vcs"]):
            fail("UnknownChannel", "spacecraft_id")
        v = vc_of(cfg, t["scid"], t["vcid"])
        if v is None:
            fail("UnknownChannel", "vc_id")
        if v["trunc"] is None:
            fail("InvalidField", "end_of_frame_primary_header_flag")
        return v["trunc"]
    if cfg["fixed"]:
        return cfg["len"]
    if len(prefix) < 6:
        fail("Incomplete")
    return int.from_bytes(prefix[4:6], "big") + 1


# ---- the writers (uslp-frame.md §3.3, in row order)

def encode(cfg, p, op):
    scid, vcid, map_id = p["spacecraft_id"], p["vc_id"], p["map_id"]
    if not any(v["scid"] == scid for v in cfg["vcs"]):
        fail("UnknownChannel", "spacecraft_id")
    v = vc_of(cfg, scid, vcid)
    if v is None:
        fail("UnknownChannel", "vc_id")
    if (vcid == OID_VCID) != (op == "encode_oid"):
        fail("InvalidField", "vc_id")
    kind = p.get("kind", "expedited")
    if op == "encode_oid":
        if map_id != 0:
            fail("InvalidField", "map_id")
        if kind == "protocol_control":
            fail("InvalidField", "protocol_control_command_flag")
    m = None
    if vcid != OID_VCID:
        m = map_of(cfg, scid, vcid, map_id)
        if m is None:
            fail("UnknownChannel", "map_id")
    if op == "encode_truncated":
        return encode_truncated(cfg, p, v, m)
    if op == "encode_oid":
        rule, upid = 0b001, UPID_IDLE
    else:
        rule, upid = p["tfdz_construction_rule"], p["upid"]
        if (rule in FIXED_RULES) != v["fixed"]:
            fail("InvalidField", "tfdz_construction_rule")
    bypass, pcc = FLAGS[kind]
    vcf_len = v["len"][bypass]
    if p["vcf_count"] >= 1 << (8 * vcf_len):
        fail("OutOfRange", "vcf_count")
    ocf = None if p["ocf_hex"] is None else bytes.fromhex(p["ocf_hex"])
    if ocf is not None and v["ocf"] == "absent" or ocf is None and v["ocf"] == "required":
        fail("InvalidField", "ocf_flag")
    iz = bytes.fromhex(p["insert_zone_hex"])
    if len(iz) != cfg["iz"]:
        fail("LengthMismatch", "insert_zone_len")
    sec = security(v, m, pcc)
    rest = overhead(cfg, v, sec, vcf_len, ocf is not None)
    if op == "encode_oid":
        tfdz = oid_octets(p["oid_skip_octets"], cfg["len"] - rest)
        pointer = len(tfdz) - 1
    else:
        tfdz, pointer = bytes.fromhex(p["tfdz_hex"]), p["pointer"]
        if not tfdz:
            fail("InvalidField", "frame_data_field")
        if cfg["fixed"] and rest + len(tfdz) != cfg["len"]:
            fail("LengthMismatch", "frame_data_field")
    total = rest + len(tfdz)
    if not cfg["fixed"] and total > cfg["len"]:
        fail("OutOfRange", "frame_len")
    if rule in FIXED_RULES and pointer != NO_POINTER and pointer >= len(tfdz):
        fail("OutOfRange", "fhp_lvo_pointer")
    head = pack(PRIMARY, {"tfvn": TFVN, "scid": scid,
                          "sod": 1 if p["source_or_destination"] == "destination" else 0,
                          "vcid": vcid, "map": map_id, "eofph": 0, "frame_len": total - 1,
                          "bypass": bypass, "pcc": pcc, "spare": 0,
                          "ocf": 0 if ocf is None else 1, "vcf_len": vcf_len})
    tfdf = bytes([rule << 5 | upid])
    if rule in FIXED_RULES:
        tfdf += pointer.to_bytes(2, "big")
    frame = (head + p["vcf_count"].to_bytes(vcf_len, "big") + iz + bytes(sec[0]) + tfdf
             + tfdz + bytes(sec[1]) + (ocf or b""))
    return seal(frame) if cfg["fecf"] else frame


def encode_truncated(cfg, p, v, m):
    if v["trunc"] is None:
        fail("InvalidField", "end_of_frame_primary_header_flag")
    rule, upid = p["tfdz_construction_rule"], p["upid"]
    if rule in FIXED_RULES:
        fail("InvalidField", "tfdz_construction_rule")
    tfdz = bytes.fromhex(p["tfdz_hex"])
    if not tfdz:
        fail("InvalidField", "frame_data_field")
    sh, st = TRUNC_SDLS if uses_sdls(cfg, v, m) else (0, 0)
    if TRUNC_HEADER + sh + 1 + len(tfdz) + st != v["trunc"]:
        fail("LengthMismatch", "frame_data_field")
    head = pack(TRUNCATED, {"tfvn": TFVN, "scid": p["spacecraft_id"],
                            "sod": 1 if p["source_or_destination"] == "destination" else 0,
                            "vcid": p["vc_id"], "map": p["map_id"], "eofph": 1})
    return head + bytes(sh) + bytes([rule << 5 | upid]) + tfdz + bytes(st)


# ---- Frame Delimiting and Fill Data Removal (§4.3.11.2; uslp-frame.md §3.2)

def delimit(cfg, unit):
    items, at = [], 0

    def item(kind, octets, declared=None):
        items.append({"kind": kind, "offset": at, "octets_hex": hx(octets),
                      "declared_len": declared})

    while at < len(unit):
        rest = unit[at:]
        if len(rest) < TRUNC_HEADER or unpack(TRUNCATED, rest[:TRUNC_HEADER])["tfvn"] != TFVN:
            item("fill", rest)
            break
        t = unpack(TRUNCATED, rest[:TRUNC_HEADER])
        if t["eofph"] == 1:
            v = vc_of(cfg, t["scid"], t["vcid"])
            if v is None or v["trunc"] is None:
                item("no_truncated_length", rest)
                break
            total = v["trunc"]
        else:
            if len(rest) < FIXED_HEADER:
                item("fill", rest)
                break
            h = unpack(PRIMARY, rest[:FIXED_HEADER])
            total = h["frame_len"] + 1
            if total < FIXED_HEADER + h["vcf_len"] + 2:
                item("fill", rest)
                break
        if total > len(rest):
            item("incomplete", rest, total)
            break
        item("frame", rest[:total])
        at += total
    return {"items": items}


# ---- expected outcomes

def err(e):
    out = {"ok": False, "error": e.kind}
    if e.field is not None:
        out["field"] = e.field
    return out


def expect(case):
    op = case["operation"]
    data = bytes.fromhex(case.get("input_hex") or "")
    mode = case.get("mode", "strict")
    try:
        cfg = config(case.get("config"))
        if op == "config":
            return {"ok": True, "vcs": len(cfg["vcs"]), "maps": len(cfg["maps"])}
        if op == "decode":
            return {"ok": True, **parse(cfg, data, mode, exact=False)}
        if op == "validate":
            try:
                f = parse(cfg, data, mode, exact=True)
            except Fail as e:
                return {"ok": True, "accepted": False, "reason": e.reason,
                        "reason_code": REASONS[e.reason], "error_kind": e.kind,
                        "error_field": e.field}
            return {"ok": True, "accepted": True, "frame_len": f["frame_len"]}
        if op == "peek_length":
            return {"ok": True, "frame_len": peek(cfg, data)}
        if op in ("encode", "encode_truncated", "encode_oid"):
            return {"ok": True, "output_hex": hx(encode(cfg, case["params"], op))}
        if op == "delimit":
            return {"ok": True, **delimit(cfg, data)}
    except Fail as e:
        return err(e)
    raise ValueError(f"{case['id']}: the model has no operation {op!r}")


# ---- the generator: how the inputs of the vector files were chosen

def vc(vcid, ftype="fixed", seq=1, exp=1, ocf="absent", trunc=None, sec=(0, 0), scid=SCID):
    return {"scid": scid, "vcid": vcid, "frame_type": ftype, "vcf_count_len_sequence": seq,
            "vcf_count_len_expedited": exp, "ocf": ocf, "truncated_frame_len": trunc,
            "security": sec if sec == "per_map" else {"header_len": sec[0],
                                                      "trailer_len": sec[1]}}


def mp(vcid, map_id, sh=0, st=0, scid=SCID):
    return {"scid": scid, "vcid": vcid, "map_id": map_id, "security_header_len": sh,
            "security_trailer_len": st}


def pc(ftype, frame_len, vcs, maps, iz=0, fecf_=True):
    return {"frame_type": ftype, "frame_len": frame_len, "insert_zone_len": iz, "fecf": fecf_,
            "vcs": vcs, "maps": maps}


# A fixed-length Physical Channel of 40 octets with FECF (Table 5-1).
FIXED = pc("fixed", 40, [
    vc(1, seq=1, exp=1),
    vc(2, seq=2, exp=0, ocf="required", sec=(4, 2)),
    vc(3, seq=0, exp=0, sec="per_map"),
    vc(OID_VCID, seq=0, exp=0),
    vc(7, seq=1, exp=1, scid=SCID16),
], [mp(1, 0), mp(1, 3), mp(2, 5), mp(3, 1, 2, 1), mp(3, 2), mp(7, 0, scid=SCID16)])
FIXED_SMALL = pc("fixed", 16, [vc(1, seq=1, exp=1)], [mp(1, 0)])
FIXED_NO_FECF = pc("fixed", 20, [vc(1, seq=1, exp=1)], [mp(1, 0)], fecf_=False)

# A variable-length Physical Channel with a maximum of 300 octets and FECF.
VAR = pc("variable", 300, [
    vc(1, "variable", seq=1, exp=1, ocf="allowed"),
    vc(4, "variable", seq=2, exp=1, sec=(6, 4)),
    vc(9, "variable", seq=3, exp=2, ocf="allowed", scid=SCID16),
], [mp(1, 0), mp(1, 7), mp(4, 0), mp(9, 15, scid=SCID16)])
VAR_NO_FECF = pc("variable", 64, [vc(1, "variable", seq=1, exp=0)], [mp(1, 0)], fecf_=False)
VAR_LARGE = pc("variable", MAX_FRAME, [vc(1, "variable", seq=1, exp=1)], [mp(1, 0)])
VAR_LARGE_1 = pc("variable", MAX_FRAME - 1, [vc(1, "variable", seq=1, exp=1)], [mp(1, 0)])

# One VC per VCF Count Length: VC 20+n has n octets for sequence control, 7-n for expedited.
VCF = pc("variable", 64, [vc(20 + n, "variable", seq=n, exp=7 - n) for n in range(8)],
         [mp(20 + n, 0) for n in range(8)])

# Truncated frames: variable-length VCs with a Truncated Transfer Frame Length.
TRUNC = pc("variable", 128, [
    vc(1, "variable"),
    vc(5, "variable", trunc=12),
    vc(8, "variable", trunc=6),
    vc(10, "variable", trunc=32),
    vc(11, "variable", trunc=9, scid=SCID16),
    vc(6, "variable", trunc=32, sec=(8, 8)),
    vc(7, "variable", trunc=28, sec="per_map"),
], [mp(1, 0), mp(5, 2), mp(8, 0), mp(10, 0), mp(11, 4, scid=SCID16), mp(6, 0), mp(7, 0),
    mp(7, 1, 4, 0)])

# OID frames: a fixed-length channel with a 3-octet Insert Zone and FECF; and one whose
# VCs carry an OCF ("Inclusion of OCF Required") and a VCF Count on VCID 63.
OID = pc("fixed", 24, [vc(OID_VCID, seq=0, exp=0), vc(1, seq=1, exp=1)], [mp(1, 0)], iz=3)
OID_OCF = pc("fixed", 32, [vc(OID_VCID, seq=2, exp=1, ocf="required"),
                           vc(1, seq=1, exp=1, ocf="required")], [mp(1, 0)])
NO_OID = pc("fixed", 24, [vc(1, seq=1, exp=1)], [mp(1, 0)], iz=3)

# Insert Zone of 4 octets on a fixed-length channel (§4.1.3, Table 5-1).
INSERT = pc("fixed", 48, [vc(1, seq=1, exp=1), vc(2, seq=0, exp=0, ocf="required", sec=(2, 2)),
                          vc(OID_VCID, seq=0, exp=0)], [mp(1, 0), mp(2, 0)], iz=4)

# Delimiting: a variable-length channel (TC Synchronization and Channel Coding below).
DELIM = pc("variable", 64, [vc(1, "variable", seq=1, exp=1), vc(3, "variable", seq=0, exp=0),
                            vc(5, "variable", trunc=8), vc(6, "variable", trunc=28,
                                                           sec=(6, 16))],
           [mp(1, 0), mp(3, 0), mp(5, 2), mp(6, 0)])


def pat(n, start=0x10, step=1):
    return bytes((start + step * i) & 0xFF for i in range(n))


def params(vcid, tfdz, rule=0b111, upid=None, pointer=None, map_id=0,
           kind="sequence_controlled", count=0, iz=b"", ocf=None, scid=SCID, sod="source"):
    if upid is None:
        upid = UPID_PACKETS if rule == 0 else UPID_OCTETS if rule == 3 else UPID_MSI1
    return {"spacecraft_id": scid, "source_or_destination": sod, "vc_id": vcid,
            "map_id": map_id, "kind": kind, "vcf_count": count, "tfdz_construction_rule": rule,
            "upid": upid, "pointer": pointer, "tfdz_hex": hx(tfdz), "insert_zone_hex": hx(iz),
            "ocf_hex": None if ocf is None else hx(ocf)}


def tparams(vcid, tfdz, map_id=0, rule=0b111, upid=None, scid=SCID, sod="source"):
    return {"spacecraft_id": scid, "source_or_destination": sod, "vc_id": vcid,
            "map_id": map_id, "tfdz_construction_rule": rule,
            "upid": UPID_MSI1 if upid is None else upid, "tfdz_hex": hx(tfdz)}


def oparams(skip=0, vcid=OID_VCID, map_id=0, kind="sequence_controlled", count=0, iz=b"",
            ocf=None, scid=SCID):
    return {"spacecraft_id": scid, "source_or_destination": "source", "vc_id": vcid,
            "map_id": map_id, "kind": kind, "vcf_count": count, "insert_zone_hex": hx(iz),
            "ocf_hex": None if ocf is None else hx(ocf), "oid_skip_octets": skip}


def zone(c, vcid, map_id=0, kind="sequence_controlled", ocf=False, scid=SCID):
    """The TFDZ length of a fixed-length frame on the channel (generator helper)."""
    cfg = config(c)
    v = vc_of(cfg, scid, vcid)
    m = map_of(cfg, scid, vcid, map_id)
    bypass, pcc = FLAGS[kind]
    return cfg["len"] - overhead(cfg, v, security(v, m, pcc), v["len"][bypass], ocf)


def frame_of(c, p, op="encode"):
    return encode(config(c), p, op)


def hdr(vcid, total, map_id=0, kind=None, flags=None, spare=0, ocf=0, vcf_len=1, tfvn=TFVN,
        scid=SCID, sod=0, eofph=0):
    """Raw Primary Header octets (without the VCF Count), for inputs no writer produces."""
    b, c = flags if flags is not None else FLAGS[kind or "sequence_controlled"]
    return pack(PRIMARY, {"tfvn": tfvn, "scid": scid, "sod": sod, "vcid": vcid, "map": map_id,
                          "eofph": eofph, "frame_len": total - 1, "bypass": b, "pcc": c,
                          "spare": spare, "ocf": ocf, "vcf_len": vcf_len})


def thdr(vcid, map_id=0, tfvn=TFVN, scid=SCID, sod=0):
    return pack(TRUNCATED, {"tfvn": tfvn, "scid": scid, "sod": sod, "vcid": vcid,
                            "map": map_id, "eofph": 1})


def reheader(frame, has_fecf=True, **fields):
    """A copy of a non-truncated frame with Primary Header fields changed, FECF recomputed."""
    h = unpack(PRIMARY, frame[:FIXED_HEADER])
    h.update(fields)
    body = pack(PRIMARY, h) + frame[FIXED_HEADER:len(frame) - (FECF_LEN if has_fecf else 0)]
    return seal(body) if has_fecf else body


def retfdf(frame, offset, octets, has_fecf=True):
    """A copy of a frame with the octets at offset replaced, FECF recomputed."""
    end = len(frame) - (FECF_LEN if has_fecf else 0)
    body = frame[:offset] + octets + frame[offset + len(octets):end]
    return seal(body) if has_fecf else body


ENC = ["USLP-010", "API-020", "API-021", "TV-002"]
DEC = ["API-010", "TV-002"]
WERR = ["USLP-010", "API-022"]
OCF_WORD = bytes.fromhex("01140C2A")  # a Type-1 Report (bit 0 = 0, §4.1.5.2.1)


class Builder:
    def __init__(self, tag):
        self.tag, self.cases = tag, []

    def add(self, reqs, desc, op, c, mode=None, p=None, data=b""):
        reqs = list(dict.fromkeys(reqs))
        case = {"id": f"uslp-{self.tag}-{len(self.cases) + 1:03}", "requirements": reqs,
                "description": desc, "operation": op}
        if mode is not None:
            case["mode"] = mode
        case["config"] = c
        if p is not None:
            case["params"] = p
        case["input_hex"] = hx(data)
        case["expect"] = expect(case)
        self.cases.append(case)
        return case

    def pair(self, reqs, desc, c, p, op="encode", extra=b"", mode="strict"):
        """An encode case and a decode case of the octets it gives (API-020)."""
        self.add(reqs + ENC, desc, op, c, p=p)
        frame = frame_of(c, p, op)
        self.add(reqs + DEC + (["API-011"] if extra else []),
                 desc + ": decoded" + (f", followed by {len(extra)} more octets" if extra
                                       else ""), "decode", c, mode=mode, data=frame + extra)
        return frame

    def dec(self, reqs, desc, c, data, mode="strict"):
        return self.add(reqs, desc, "decode", c, mode=mode, data=data)

    def val(self, reqs, desc, c, data, mode="strict"):
        return self.add(reqs + ["USLP-014"], desc, "validate", c, mode=mode, data=data)

    def enc(self, reqs, desc, c, p, op="encode"):
        return self.add(reqs, desc, op, c, p=p)


def gen_fixed_packets():
    b = Builder("fixed000")
    z1 = zone(FIXED, 1)
    req = ["USLP-003", "USLP-005", "USLP-006", "USLP-016", "GEN-053"]
    b.pair(req, "Rule 000 on a 16-octet channel, VC 1 MAP 0, count 0x2A, FHP 1, 3-octet TFDZ, "
           "FECF (worked by hand in docs/verification/derivations/uslp-frame.md)", FIXED_SMALL,
           params(1, bytes([0xAB, 0x08, 0x00]), rule=0, pointer=1, count=0x2A))
    b.pair(req, f"Rule 000, VC 1 MAP 0, sequence-controlled count 0, FHP 0, {z1}-octet TFDZ, "
           "FECF", FIXED, params(1, pat(z1), rule=0, pointer=0))
    b.pair(req, "Rule 000, VC 1 MAP 3, expedited count 255 (the largest 1-octet count), FHP "
           "0xFFFF (no packet starts or ends), destination SCID", FIXED,
           params(1, pat(z1, 0x80), rule=0, pointer=NO_POINTER, map_id=3, kind="expedited",
                  count=255, sod="destination"))
    b.pair(req, "Rule 000, FHP 13 (mid-zone)", FIXED,
           params(1, pat(z1, 0xA0, 3), rule=0, pointer=13, count=0x5A))
    b.pair(req, f"Rule 000, FHP {z1 - 1} (the last TFDZ octet)", FIXED,
           params(1, pat(z1, 0x01, 5), rule=0, pointer=z1 - 1, count=1))
    z2 = zone(FIXED, 2, 5, ocf=True)
    b.pair(req + ["API-070", "API-072", "USLP-004"],
           "Rule 000 on VC 2: 2-octet sequence-controlled count 0x1234, Security Header 4 and "
           "Trailer 2 written as zeros (§6.3), OCF required (Table 5-3)", FIXED,
           params(2, pat(z2, 0x40), rule=0, pointer=5, map_id=5, count=0x1234, ocf=OCF_WORD))
    z31 = zone(FIXED, 3, 1)
    b.pair(req + ["API-070", "API-072"],
           "Rule 000 on VC 3 MAP 1, SDLS set per MAP (Header 2, Trailer 1), no VCF Count",
           FIXED, params(3, pat(z31, 0x22), rule=0, pointer=0, map_id=1))
    z32 = zone(FIXED, 3, 2)
    b.pair(req + ["API-070"], "Rule 000 on VC 3 MAP 2, a MAP of the same VC without SDLS",
           FIXED, params(3, pat(z32, 0x33), rule=0, pointer=2, map_id=2))
    z7 = zone(FIXED, 7, scid=SCID16)
    b.pair(req, "Rule 000 on the fictional 16-bit SCID 0xF0A5, VC 7", FIXED,
           params(7, pat(z7, 0xC0), rule=0, pointer=4, count=0x99, scid=SCID16))
    zn = zone(FIXED_NO_FECF, 1)
    b.pair(req, "Rule 000 on a channel without FECF", FIXED_NO_FECF,
           params(1, pat(zn, 0x61), rule=0, pointer=1, count=7))
    p = params(1, pat(z1, 0x05), rule=0, pointer=0, upid=UPID_UNASSIGNED)
    b.pair(req + ["USLP-007"], f"UPID {UPID_UNASSIGNED}, unassigned in the SANA snapshot: "
           "decoded as Other, no anomaly", FIXED, p)
    b.dec(["USLP-007", "API-032"] + DEC, f"UPID {UPID_UNASSIGNED} in lenient mode: no anomaly",
          FIXED, frame_of(FIXED, p), mode="lenient")
    frame = frame_of(FIXED, params(1, pat(z1), rule=0, pointer=0))
    b.enc(["USLP-006"] + WERR, f"FHP {z1}, one beyond the {z1}-octet TFDZ: OutOfRange", FIXED,
          params(1, pat(z1), rule=0, pointer=z1))
    at = FIXED_HEADER + 1 + 1
    b.dec(["USLP-006"], f"FHP {z1}, one beyond the TFDZ: Malformed", FIXED,
          retfdf(frame, at, z1.to_bytes(2, "big")))
    b.dec(["USLP-006", "API-032"], "FHP 0xFFFE in lenient mode: Malformed in both modes",
          FIXED, retfdf(frame, at, (0xFFFE).to_bytes(2, "big")), mode="lenient")
    vz = 5
    b.enc(["USLP-016"] + WERR, "Rule 000 on a variable-length VC: InvalidField", VAR,
          params(1, pat(vz), rule=0, pointer=0))
    vframe = frame_of(VAR, params(1, pat(vz), rule=0b111))
    b.dec(["USLP-016"], "Rule 000 on a variable-length VC: InvalidField", VAR,
          retfdf(vframe, FIXED_HEADER + 1, bytes([0 << 5 | UPID_PACKETS])))
    b.dec(["USLP-003", "API-012"], "Fixed-length channel, one octet more than the frame: "
          "LengthMismatch", FIXED, frame + b"\x00")
    b.dec(["USLP-003", "API-012"], "Fixed-length channel, the frame without its last octet: "
          "LengthMismatch", FIXED, frame[:-1])
    b.add(["API-013", "USLP-003"], "peek_length on a fixed-length channel: the configured "
          "length from 4 octets", "peek_length", FIXED, data=frame[:4])
    return b.cases


def gen_fixed_mapa():
    b = Builder("fixedmapa")
    z = zone(FIXED, 1)
    req = ["USLP-005", "USLP-006", "USLP-016"]
    b.pair(req, f"Rule 001, UPID Mission-specific Information-1, LVO 9: a 10-octet MAPA_SDU "
           f"and {z - 10} idle octets", FIXED,
           params(1, pat(10, 0x70) + bytes(z - 10), rule=1, pointer=9, count=3))
    b.pair(req, "Rule 001, LVO 0xFFFF: the MAPA_SDU continues", FIXED,
           params(1, pat(z, 0x90), rule=1, pointer=NO_POINTER, count=4))
    b.pair(req, "Rule 010, LVO 0xFFFF: continuing portion, the SDU continues", FIXED,
           params(1, pat(z, 0xB0), rule=2, pointer=NO_POINTER, count=5))
    b.pair(req, f"Rule 010, LVO {z - 1} (the last TFDZ octet)", FIXED,
           params(1, pat(z, 0xD0), rule=2, pointer=z - 1, count=6, kind="expedited",
                  map_id=3))
    b.pair(req, "Rule 010, LVO 0", FIXED, params(1, pat(z, 0xE0), rule=2, pointer=0, count=7))
    b.pair(req, "Rule 001, VCA_SDU on VC 3 MAP 2 with UPID Octet Stream", FIXED,
           params(3, pat(zone(FIXED, 3, 2), 0x0F), rule=1, pointer=17, map_id=2,
                  upid=UPID_OCTETS))
    b.enc(["USLP-006"] + WERR, f"Rule 001, LVO {z} (one beyond the zone): OutOfRange", FIXED,
          params(1, pat(z), rule=1, pointer=z))
    b.enc(["USLP-006"] + WERR, "Rule 010, LVO 0xFFFE: OutOfRange", FIXED,
          params(1, pat(z), rule=2, pointer=0xFFFE))
    frame = frame_of(FIXED, params(1, pat(z), rule=1, pointer=0))
    at = FIXED_HEADER + 1 + 1
    b.dec(["USLP-006"], f"Rule 001, LVO {z} (one beyond the zone): Malformed", FIXED,
          retfdf(frame, at, z.to_bytes(2, "big")))
    b.dec(["USLP-006", "API-032"], f"Rule 010, LVO {z} in lenient mode: Malformed", FIXED,
          retfdf(frame, at - 1, bytes([2 << 5 | UPID_MSI1]) + z.to_bytes(2, "big")),
          mode="lenient")
    vframe = frame_of(VAR, params(1, pat(5), rule=0b111))
    for rule in (1, 2):
        b.enc(["USLP-016"] + WERR, f"Rule {rule:03b} on a variable-length VC: InvalidField",
              VAR, params(1, pat(5), rule=rule, pointer=NO_POINTER))
        b.dec(["USLP-016"], f"Rule {rule:03b} on a variable-length VC: InvalidField", VAR,
              retfdf(vframe, FIXED_HEADER + 1, bytes([rule << 5 | UPID_MSI1])))
    b.dec(["USLP-016", "API-032"], "Rule 001 on a variable-length VC in lenient mode: "
          "InvalidField in both modes", VAR,
          retfdf(vframe, FIXED_HEADER + 1, bytes([1 << 5 | UPID_MSI1])), mode="lenient")
    return b.cases


def gen_variable():
    b = Builder("var")
    req = ["USLP-003", "USLP-005", "USLP-016", "GEN-053"]
    names = {3: "octet stream", 4: "starting segment", 5: "continuing segment",
             6: "last segment", 7: "no segmentation"}
    b.pair(req, "Rule 011 (octet stream), UPID Octet Stream, VC 1 MAP 0, count 1, 5 octets",
           VAR, params(1, pat(5), rule=3, upid=UPID_OCTETS, count=1))
    b.pair(req, "Rule 100 (starting segment), UPID Packets, VC 1 MAP 7, expedited", VAR,
           params(1, pat(12, 0x21), rule=4, upid=UPID_PACKETS, map_id=7, kind="expedited",
                  count=0x80))
    b.pair(req, "Rule 101 (continuing segment)", VAR,
           params(1, pat(20, 0x41), rule=5, upid=UPID_PACKETS, map_id=7, kind="expedited",
                  count=0x81))
    b.pair(req, "Rule 110 (last segment)", VAR,
           params(1, pat(3, 0x61), rule=6, upid=UPID_PACKETS, map_id=7, kind="expedited",
                  count=0x82))
    b.pair(req + ["USLP-004"], "Rule 111 (no segmentation) with an OCF (allowed, §4.1.5.4), "
           "count 255", VAR,
           params(1, pat(9, 0x81), rule=7, upid=UPID_MSI1, count=255, ocf=OCF_WORD))
    b.pair(req + ["API-070", "API-072"], "Rule 111 on VC 4 with Security Header 6 and Trailer "
           "4 written as zeros, 2-octet sequence-controlled count", VAR,
           params(4, pat(6, 0xA1), rule=7, upid=UPID_PACKETS, count=0xBEEF))
    b.pair(req, "Rule 111 on the fictional 16-bit SCID 0xF0A5, VC 9 MAP 15, 3-octet count, "
           "OCF, destination", VAR,
           params(9, pat(4, 0xC1), rule=7, map_id=15, count=0xABCDEF, ocf=OCF_WORD,
                  scid=SCID16, sod="destination"))
    b.pair(req + ["API-011"], "Rule 111, 2 octets", VAR,
           params(1, pat(2, 0xE1), rule=7, count=2), extra=bytes([0xC0, 0x55, 0x00]))
    b.pair(req + ["CMN-041"], "Rule 111, one TFDZ octet (the shortest frame)", VAR,
           params(1, b"\x5A", rule=7, count=3))
    big = 300 - overhead(config(VAR), vc_of(config(VAR), SCID, 1), (0, 0), 1, False)
    b.pair(req, f"Rule 111, a {big}-octet TFDZ: the frame is the 300-octet maximum", VAR,
           params(1, pat(big, 0x00), rule=7, count=4))
    b.enc(req + WERR, f"Rule 111, a {big + 1}-octet TFDZ: 301 octets, above the maximum: "
          "OutOfRange", VAR, params(1, pat(big + 1), rule=7))
    b.pair(req, "Rule 111 without FECF", VAR_NO_FECF,
           params(1, pat(4, 0x31), rule=7, count=9))
    b.pair(req, "Rule 111 on the 65536-octet maximum channel, a short frame", VAR_LARGE,
           params(1, pat(3, 0x77), rule=7, count=0x10))
    fz = zone(FIXED, 1)
    fframe = frame_of(FIXED, params(1, pat(fz), rule=1, pointer=NO_POINTER))
    for rule in range(3, 8):
        b.enc(["USLP-016"] + WERR, f"Rule {rule:03b} ({names[rule]}) on a fixed-length VC: "
              "InvalidField", FIXED, params(1, pat(fz + 2), rule=rule))
        b.dec(["USLP-016"], f"Rule {rule:03b} ({names[rule]}) on a fixed-length VC: "
              "InvalidField", FIXED,
              retfdf(fframe, FIXED_HEADER + 1, bytes([rule << 5 | UPID_OCTETS])))
    frame = frame_of(VAR, params(1, pat(5), rule=3, upid=UPID_OCTETS, count=1))
    b.add(["API-013", "USLP-003"], "peek_length, variable-length channel, 6 octets: Frame "
          "Length + 1", "peek_length", VAR, data=frame[:6])
    b.add(["API-013"], "peek_length, variable-length channel, 5 octets: Incomplete",
          "peek_length", VAR, data=frame[:5])
    b.add(["API-013", "GEN-053"], "peek_length, Frame Length 0xFFFF on a 65536-octet channel: "
          "65536", "peek_length", VAR_LARGE, data=hdr(1, MAX_FRAME)[:6])
    b.dec(["USLP-003", "API-011"], "Variable-length channel, a frame missing its last octet: "
          "Incomplete", VAR, frame[:-1])
    return b.cases


def gen_vcf():
    b = Builder("vcf")
    req = ["USLP-004", "USLP-010"]
    for n in range(8):
        top = (1 << (8 * n)) - 1
        b.pair(req, f"Sequence-controlled count length {n} on VC {20 + n}: count {top} "
               f"(the largest)", VCF, params(20 + n, pat(3, 0x10 * n), rule=7, count=top))
        top_e = (1 << (8 * (7 - n))) - 1
        b.pair(req, f"Expedited count length {7 - n} on VC {20 + n}: count {top_e} "
               f"(the largest)", VCF,
               params(20 + n, pat(2, 0x0F * n), rule=7, kind="expedited", count=top_e))
    b.pair(req, "Count length 3, count 0x123456: most significant octet first", VCF,
           params(23, pat(1), rule=7, count=0x123456))
    b.pair(req, "Count length 7, count 0x0123456789ABCD", VCF,
           params(27, pat(1), rule=7, count=0x0123456789ABCD))
    for vcid, kind, count in [(20, "sequence_controlled", 1), (21, "sequence_controlled", 256),
                              (27, "sequence_controlled", 1 << 56), (24, "expedited", 1 << 24),
                              (27, "expedited", 1)]:
        n = config(VCF)["vcs"][vcid - 20]["len"][FLAGS[kind][0]]
        b.enc(req + ["API-022"], f"{kind}, count length {n} on VC {vcid}, count {count}: "
              "does not fit, OutOfRange", VCF, params(vcid, pat(2), rule=7, kind=kind,
                                                        count=count))
    good = frame_of(VCF, params(21, pat(3), rule=7, count=9))
    wrong = seal(hdr(21, len(good) + 1, vcf_len=2) + bytes([0, 9]) + good[8:-2])
    b.dec(req, "VC 21 expects a 1-octet sequence-controlled count, the header says 2: "
          "LengthMismatch", VCF, wrong)
    b.dec(req + ["API-032"], "The same in lenient mode: LengthMismatch in both modes", VCF,
          wrong, mode="lenient")
    exp = frame_of(VCF, params(22, pat(3), rule=7, kind="sequence_controlled", count=5))
    b.dec(req, "VC 22: a 2-octet count in an expedited frame (expedited length 5): "
          "LengthMismatch", VCF, reheader(exp, bypass=1))
    return b.cases


def gen_truncated():
    b = Builder("trunc")
    req = ["USLP-002", "USLP-016"]
    b.pair(req, "Truncated frame on VC 5 MAP 2 (length 12): rule 111, UPID Mission-specific "
           "Information-1, 7 TFDZ octets", TRUNC, tparams(5, pat(7, 0x31), map_id=2),
           op="encode_truncated")
    b.pair(req + ["API-011"], "Truncated frame on VC 5", TRUNC,
           tparams(5, pat(7, 0x41), map_id=2), op="encode_truncated", extra=pat(5, 0xC0))
    b.pair(req, "Truncated frame of length 6 (the shortest, Annex D1.3.2 note 2) on VC 8: one "
           "TFDZ octet", TRUNC, tparams(8, b"\xA5"), op="encode_truncated")
    b.pair(req, "Truncated frame of length 32 (the longest, Annex D1.3.2 note 3) on VC 10: "
           "27 TFDZ octets (D1.4.2.4)", TRUNC, tparams(10, pat(27, 0x01)), op="encode_truncated")
    b.pair(req, "Truncated frame on the fictional 16-bit SCID 0xF0A5, VC 11 MAP 4, "
           "destination", TRUNC, tparams(11, pat(4, 0xD1), map_id=4, scid=SCID16,
                                         sod="destination"), op="encode_truncated")
    b.pair(req, "Truncated frame with rule 011 and UPID Packets: exposed and not checked "
           "(OI-35 item 1)", TRUNC, tparams(5, pat(7, 0x51), map_id=2, rule=3,
                                            upid=UPID_PACKETS), op="encode_truncated")
    b.pair(req + ["USLP-007"], f"Truncated frame with the unassigned UPID {UPID_UNASSIGNED}",
           TRUNC, tparams(5, pat(7, 0x61), map_id=2, rule=4, upid=UPID_UNASSIGNED),
           op="encode_truncated")
    frame = frame_of(TRUNC, tparams(5, pat(7, 0x31), map_id=2), "encode_truncated")
    b.dec(req + ["API-032"], "Truncated frame in lenient mode", TRUNC, frame, mode="lenient")
    for rule in FIXED_RULES:
        b.dec(req, f"Truncated frame with rule {rule:03b}: InvalidField (Annex D1.2.1)",
              TRUNC, frame[:4] + bytes([rule << 5 | UPID_MSI1]) + frame[5:])
    b.dec(req + ["API-032"], "Truncated frame with rule 000 in lenient mode: InvalidField",
          TRUNC, frame[:4] + bytes([UPID_MSI1]) + frame[5:], mode="lenient")
    other = thdr(1) + bytes([7 << 5 | UPID_MSI1]) + pat(3)
    b.dec(req, "Truncated header on VC 1, which has no truncated frame length: InvalidField",
          TRUNC, other)
    b.dec(req + ["API-032"], "The same in lenient mode: InvalidField in both modes (OI-35 "
          "item 2)", TRUNC, other, mode="lenient")
    b.dec(req, "Truncated header on VC 5 with MAP 3, not configured: UnknownChannel", TRUNC,
          thdr(5, 3) + frame[4:])
    b.dec(req, "Truncated header on VC 12, not configured: UnknownChannel", TRUNC,
          thdr(12) + frame[4:])
    b.dec(req, "Truncated frame, 11 of 12 octets: Incomplete", TRUNC, frame[:-1])
    b.dec(req, "Truncated header, 3 octets: Incomplete", TRUNC, frame[:3])
    b.enc(req + WERR, "encode_truncated on VC 1 (no truncated frame length): InvalidField",
          TRUNC, tparams(1, pat(3)), op="encode_truncated")
    b.enc(req + WERR, "encode_truncated, 6 TFDZ octets on VC 5 (7 fill the frame): "
          "LengthMismatch", TRUNC, tparams(5, pat(6), map_id=2), op="encode_truncated")
    b.enc(req + WERR, "encode_truncated, 8 TFDZ octets on VC 5: LengthMismatch", TRUNC,
          tparams(5, pat(8), map_id=2), op="encode_truncated")
    b.enc(req + WERR + ["CMN-041"], "encode_truncated, an empty TFDZ: InvalidField", TRUNC,
          tparams(5, b"", map_id=2), op="encode_truncated")
    b.enc(req + WERR, "encode_truncated on VC 5 MAP 0, not configured: UnknownChannel", TRUNC,
          tparams(5, pat(7)), op="encode_truncated")
    b.enc(req + WERR, "encode_truncated on VC 12, not configured: UnknownChannel", TRUNC,
          tparams(12, pat(7)), op="encode_truncated")
    b.enc(req + WERR, "encode_truncated on VCID 63 of a fixed-length channel: InvalidField",
          FIXED, tparams(OID_VCID, pat(7)), op="encode_truncated")
    b.add(req + ["API-013"], "peek_length, truncated header on VC 5: 12", "peek_length", TRUNC,
          data=frame[:4])
    b.add(req + ["API-013"], "peek_length, truncated header on SCID 0x0001: UnknownChannel",
          "peek_length", TRUNC, data=thdr(5, 2, scid=1))
    b.add(req + ["API-013"], "peek_length, truncated header on VC 12: UnknownChannel",
          "peek_length", TRUNC, data=thdr(12))
    b.add(req + ["API-013"], "peek_length, truncated header on VC 1 (no truncated length): "
          "InvalidField", "peek_length", TRUNC, data=thdr(1))
    b.add(req + ["API-013"], "peek_length, truncated header on a fixed-length channel: "
          "InvalidField", "peek_length", FIXED, data=thdr(1))
    b.add(req + ["USLP-002"], "A truncated frame length on a fixed-length VC: InvalidConfig",
          "config", pc("fixed", 40, [vc(1, trunc=12)], [mp(1, 0)]))
    return b.cases


def gen_truncated_sdls():
    b = Builder("tsdls")
    req = ["USLP-002", "USLP-012", "API-075", "API-070", "API-072"]
    b.pair(req, "Truncated frame on VC 6 (SDLS 8/8 on full frames): the 6-octet Security "
           "Header and 16-octet Trailer of Annex D1.2.3, 5 TFDZ octets", TRUNC,
           tparams(6, pat(5, 0x91)), op="encode_truncated")
    b.pair(["API-070", "API-072"], "A non-truncated frame on the same VC 6 keeps its own 8/8 "
           "layout", TRUNC, params(6, pat(4, 0x81), rule=7, count=1))
    b.pair(req, "Truncated frame on VC 7 MAP 1 (SDLS per MAP, header 4): 6/16, length 28, one "
           "TFDZ octet", TRUNC, tparams(7, b"\xEE", map_id=1), op="encode_truncated")
    b.pair(req, "Truncated frame on VC 7 MAP 0 (no SDLS on that MAP): no security fields, 23 "
           "TFDZ octets", TRUNC, tparams(7, pat(23, 0x01), map_id=0), op="encode_truncated")
    frame = frame_of(TRUNC, tparams(6, pat(5, 0x91)), "encode_truncated")
    b.val(req, "validate, truncated frame with SDLS on VC 6: accepted", TRUNC, frame)
    b.val(req, "validate, truncated frame with SDLS, one octet short: LengthMismatch", TRUNC,
          frame[:-1])
    b.enc(req + WERR, "encode_truncated on VC 6, 6 TFDZ octets (5 fill 32 - 4 - 6 - 1 - 16): "
          "LengthMismatch", TRUNC, tparams(6, pat(6)), op="encode_truncated")
    b.add(req, "SDLS VC (4/0) with truncated frame length 27: InvalidConfig (Annex D1.2.4)",
          "config", pc("variable", 64, [vc(2, "variable", trunc=27, sec=(4, 0))], [mp(2, 0)]))
    b.add(req, "SDLS VC (0/2) with truncated frame length 28: accepted", "config",
          pc("variable", 64, [vc(2, "variable", trunc=28, sec=(0, 2))], [mp(2, 0)]))
    b.add(req, "Per-MAP VC with truncated frame length 20 and a MAP with SDLS: InvalidConfig",
          "config", pc("variable", 64, [vc(2, "variable", trunc=20, sec="per_map")],
                       [mp(2, 0), mp(2, 1, 0, 8)]))
    b.add(req, "Per-MAP VC with truncated frame length 20, no MAP with SDLS: accepted",
          "config", pc("variable", 64, [vc(2, "variable", trunc=20, sec="per_map")],
                       [mp(2, 0), mp(2, 1)]))
    b.add(req, "VC without SDLS (0/0) and truncated frame length 6: accepted", "config",
          pc("variable", 64, [vc(2, "variable", trunc=6)], [mp(2, 0)]))
    return b.cases


def gen_oid():
    b = Builder("oid")
    req = ["USLP-009", "CMN-020", "CMN-021"]
    iz = bytes([0xA1, 0xA2, 0xA3])
    z = zone(OID, OID_VCID)
    f1 = b.pair(req + ["USLP-008"], f"OID frame from a fresh generator: VCID 63, MAP 0, rule "
                f"001, UPID Only Idle Data, LVO {z - 1}, the first {z} PN octets, Insert Zone "
                "and FECF", OID, oparams(0, iz=iz), op="encode_oid")
    b.pair(req + ["USLP-008"], f"The next OID frame of the same generator (skip {z} octets): "
           f"PN octets {z}-{2 * z - 1}", OID, oparams(z, iz=bytes([0xB1, 0xB2, 0xB3])),
           op="encode_oid")
    zo = zone(OID_OCF, OID_VCID, ocf=True)
    b.pair(req + ["USLP-004"], "OID frame with a 2-octet VCF Count (0x0102) and a required "
           f"OCF, {zo} PN octets", OID_OCF, oparams(0, count=0x0102, ocf=OCF_WORD),
           op="encode_oid")
    b.pair(req, "Expedited OID frame, 1-octet count 0xFF, from octet 100 of the sequence",
           OID_OCF, oparams(100, kind="expedited", count=0xFF, ocf=OCF_WORD), op="encode_oid")
    b.add(req + ["API-013"], "peek_length on an OID frame: 24", "peek_length", OID,
          data=f1[:4])
    b.val(req, "validate, OID frame: accepted", OID, f1)
    werr = req + WERR
    b.enc(werr, "encode_oid on VC 1: InvalidField", OID, oparams(vcid=1, iz=iz),
          op="encode_oid")
    b.enc(werr, "encode_oid with MAP 1: InvalidField", OID, oparams(map_id=1, iz=iz),
          op="encode_oid")
    b.enc(werr, "encode_oid with protocol control flags: InvalidField", OID,
          oparams(kind="protocol_control", iz=iz), op="encode_oid")
    b.enc(werr, "encode_oid with VCF Count 1 on a 0-octet count: OutOfRange", OID,
          oparams(count=1, iz=iz), op="encode_oid")
    b.enc(werr, "encode_oid with an OCF on VCID 63 without OCF: InvalidField", OID,
          oparams(iz=iz, ocf=OCF_WORD), op="encode_oid")
    b.enc(werr, "encode_oid without the required OCF: InvalidField", OID_OCF, oparams(),
          op="encode_oid")
    b.enc(werr + ["USLP-008"], "encode_oid with a 2-octet Insert Zone (3 configured): "
          "LengthMismatch", OID, oparams(iz=iz[:2]), op="encode_oid")
    b.enc(werr, "encode_oid on SCID 0x0001: UnknownChannel", OID, oparams(iz=iz, scid=1),
          op="encode_oid")
    b.enc(werr, "encode_oid on a channel without VCID 63: UnknownChannel", NO_OID,
          oparams(iz=iz), op="encode_oid")
    b.enc(werr, "encode on VCID 63: InvalidField (OID frames come from encode_oid)", OID,
          params(OID_VCID, pat(z), rule=1, upid=UPID_IDLE, pointer=z - 1, iz=iz))
    tfdf = FIXED_HEADER + len(iz)
    lvo = (z - 1).to_bytes(2, "big")
    deviations = [
        ("MAP ID 1", reheader(f1, map=1)),
        ("rule 000", retfdf(f1, tfdf, bytes([0 << 5 | UPID_IDLE]))),
        ("rule 010", retfdf(f1, tfdf, bytes([2 << 5 | UPID_IDLE]))),
        (f"UPID {UPID_PACKETS}", retfdf(f1, tfdf, bytes([1 << 5 | UPID_PACKETS]))),
        ("LVO 0xFFFF", retfdf(f1, tfdf + 1, (NO_POINTER).to_bytes(2, "big"))),
        ("LVO 3", retfdf(f1, tfdf + 1, (3).to_bytes(2, "big"))),
        ("MAP ID 2 and UPID 0", retfdf(reheader(f1, map=2), tfdf,
                                       bytes([1 << 5 | UPID_PACKETS]) + lvo)),
    ]
    for name, frame in deviations:
        b.dec(req + ["API-031"], f"OID frame with {name}, strict: InvalidField", OID, frame)
        b.dec(req + ["API-032"], f"OID frame with {name}, lenient: OidHeaderMismatch", OID,
              frame, mode="lenient")
    b.val(req + ["API-031"], "validate, OID frame with UPID 0, strict: OidHeaderMismatch",
          OID, deviations[3][1])
    b.val(req + ["API-032"], "validate, OID frame with UPID 0, lenient: accepted", OID,
          deviations[3][1], mode="lenient")
    b.dec(req + ["API-032"], "OID frame with Reserve Spares 10 and MAP ID 1, lenient: both "
          "anomalies in bit order", OID, reheader(f1, map=1, spare=2), mode="lenient")
    rule7 = retfdf(f1, tfdf, bytes([7 << 5 | UPID_IDLE]))
    b.dec(req + ["USLP-016"], "OID frame with rule 111: InvalidField", OID, rule7)
    b.dec(req + ["USLP-016", "API-032"], "OID frame with rule 111, lenient: InvalidField in "
          "both modes (USLP-016)", OID, rule7, mode="lenient")
    b.dec(req + ["USLP-006"], f"OID frame with LVO {z}, one beyond the zone: Malformed in "
          "lenient mode too", OID, retfdf(f1, tfdf + 1, z.to_bytes(2, "big")), mode="lenient")
    b.add(req + ["API-040"], "VCID 63 on a variable-length Physical Channel: InvalidConfig "
          "(Table 5-1 note 4)", "config",
          pc("variable", 64, [vc(OID_VCID, "fixed", seq=0, exp=0)], []))
    return b.cases


def gen_insert_zone():
    b = Builder("iz")
    req = ["USLP-008"]
    zi = zone(INSERT, 1)
    b.pair(req + ["USLP-005"], f"Rule 000 frame with the 4-octet Insert Zone DEADBEEF, {zi} "
           "TFDZ octets", INSERT, params(1, pat(zi, 0x20), rule=0, pointer=0, count=1,
                                         iz=bytes.fromhex("DEADBEEF")))
    z2 = zone(INSERT, 2, ocf=True)
    b.pair(req + ["API-070", "API-072"], "Insert Zone before the Security Header (2/2), OCF "
           "required, no VCF Count (§6.3.4)", INSERT,
           params(2, pat(z2, 0x50), rule=1, pointer=NO_POINTER, iz=bytes.fromhex("01020304"),
                  ocf=OCF_WORD))
    b.pair(req + ["USLP-009", "CMN-020"], "OID frame with the Insert Zone (§4.1.3.2.2)",
           INSERT, oparams(0, iz=bytes.fromhex("CAFEF00D")), op="encode_oid")
    frame = frame_of(INSERT, params(1, pat(zi, 0x20), rule=0, pointer=0, count=1,
                                    iz=bytes.fromhex("DEADBEEF")))
    b.val(req, "validate, frame with an Insert Zone: accepted", INSERT, frame)
    b.enc(req + WERR, "A 3-octet Insert Zone (4 configured): LengthMismatch", INSERT,
          params(1, pat(zi + 1), rule=0, pointer=0, iz=bytes(3)))
    b.enc(req + WERR, "No Insert Zone (4 configured): LengthMismatch", INSERT,
          params(1, pat(zi + 4), rule=0, pointer=0))
    b.enc(req + WERR, "An Insert Zone on a variable-length channel: LengthMismatch", VAR,
          params(1, pat(4), rule=7, iz=bytes(2)))
    b.add(req + ["API-040"], "Insert Zone length 4 on a variable-length Physical Channel: "
          "InvalidConfig (Table 5-1 note 5)", "config",
          pc("variable", 64, [vc(1, "variable")], [mp(1, 0)], iz=4))
    b.add(req + ["API-040", "CMN-041"], "A fixed-length channel whose Insert Zone leaves one "
          "TFDZ octet: accepted", "config",
          pc("fixed", 24, [vc(1, seq=1, exp=1)], [mp(1, 0)], iz=10))
    b.add(req + ["API-040", "CMN-041"], "The same with an Insert Zone of 11: no TFDZ octet, "
          "InvalidConfig", "config", pc("fixed", 24, [vc(1, seq=1, exp=1)], [mp(1, 0)], iz=11))
    return b.cases


def gen_protocol_control():
    b = Builder("pc")
    req = ["USLP-013", "USLP-001", "USLP-005"]
    unlock = bytes([0x00])
    set_vr = bytes([0x82, 0x00, 0x17])
    b.pair(req + ["API-070"], "Protocol control frame (flags 11) on VC 4, which has SDLS "
           "6/4: no Security Header or Trailer (§6.3.1); rule 111, UPID COP-1, Unlock, "
           "expedited count length 1", VAR,
           params(4, unlock, rule=7, upid=UPID_COP1, kind="protocol_control", count=0))
    b.pair(req + ["API-070"], "Protocol control frame on VC 4 carrying Set V(R) = 0x17", VAR,
           params(4, set_vr, rule=7, upid=UPID_COP1, kind="protocol_control", count=1))
    b.pair(req, "Protocol control frame on VC 1 (no SDLS), Unlock, count 255", VAR,
           params(1, unlock, rule=7, upid=UPID_COP1, kind="protocol_control", count=255))
    b.pair(req, "Protocol control frame on VC 1 with Set V(R) = 0xFF and an OCF", VAR,
           params(1, bytes([0x82, 0x00, 0xFF]), rule=7, upid=UPID_COP1,
                  kind="protocol_control", count=2, ocf=OCF_WORD))
    frame = frame_of(VAR, params(4, set_vr, rule=7, upid=UPID_COP1, kind="protocol_control",
                                 count=1))
    b.val(req, "validate, protocol control frame: accepted", VAR, frame)
    b.dec(req + ["USLP-004"], "Protocol control frame with the 2-octet count of the "
          "sequence-controlled QoS (VC 4 expedited length is 1): LengthMismatch", VAR,
          seal(hdr(4, len(frame) + 1, kind="protocol_control", vcf_len=2) + bytes([0, 1])
               + frame[8:-2]))
    b.dec(req + ["USLP-001"], "Flags 01 (reserved, Table 4-1): ReservedValue", VAR,
          reheader(frame, bypass=0, pcc=1))
    b.dec(req + ["USLP-001", "API-032"], "Flags 01 in lenient mode: ReservedValue", VAR,
          reheader(frame, bypass=0, pcc=1), mode="lenient")
    b.add(req + ["API-013"], "peek_length, protocol control frame", "peek_length", VAR,
          data=frame[:6])
    return b.cases


def gen_delimit():
    b = Builder("delim")
    req = ["USLP-015"]
    f1 = frame_of(DELIM, params(1, pat(4, 0x11), rule=7, count=1))
    f2 = frame_of(DELIM, params(1, pat(9, 0x21), rule=3, upid=UPID_OCTETS, count=2))
    f3 = frame_of(DELIM, params(3, pat(1, 0x31), rule=7, kind="expedited"))
    t5 = frame_of(DELIM, tparams(5, pat(3, 0x41), map_id=2), "encode_truncated")
    t6 = frame_of(DELIM, tparams(6, pat(1, 0x51)), "encode_truncated")
    fill = bytes([0x55] * 3)
    d = lambda desc, unit: b.add(req, desc, "delimit", DELIM, data=unit)
    d("Two frames and 3 octets of fill 0x55", f1 + f2 + fill)
    d("Three frames back to back, no fill", f1 + f2 + f3)
    d("A frame and a last frame that declares more octets than remain: incomplete",
      f1 + f2[:-3])
    d("A truncated frame (VC 5, length 8 from the configuration) and fill", t5 + fill)
    d("A truncated frame with SDLS (VC 6, length 28) then a non-truncated frame: mixing is "
      "not checked on reception (USLP-011 is a sending rule)", t6 + f1)
    d("A frame and a truncated header on VC 1, which has no truncated frame length: "
      "no_truncated_length", f1 + thdr(1) + pat(4))
    d("A truncated header on SCID 0x0001: no_truncated_length", thdr(5, 2, scid=1) + pat(4))
    d("A truncated header on VC 5 with 6 of its 8 octets: incomplete", f1 + t5[:6])
    d("A frame followed by 4 zero octets (TFVN 0000): fill", f1 + bytes(4))
    d("Only fill 0x55 (TFVN 0101 is never a valid header)", bytes([0x55] * 12))
    d("A frame and 3 trailing octets C0 55 55: fewer than 4, fill", f1 + bytes([0xC0, 0x55,
                                                                                0x55]))
    d("A frame and 5 octets starting with TFVN 1100, End of Frame Primary Header Flag 0: fewer "
      "than 7, fill", f1 + hdr(1, 40)[:5])
    d("A header whose Frame Length + 1 = 9 with a 1-octet VCF Count (below 9 + 1): fill",
      hdr(1, 9, vcf_len=1) + bytes(2))
    d("A header whose Frame Length + 1 = 9 with no VCF Count: a 9-octet frame, left to "
      "validation", hdr(3, 9, vcf_len=0, kind="expedited") + bytes([0xE0, 0x00]) + fill)
    d("A frame with the reserved flags 01: the delimiter does not check them",
      reheader(f1, bypass=0, pcc=1) + fill)
    d("An empty data unit", b"")
    return b.cases


def gen_validation():
    b = Builder("val")
    v = lambda reqs, desc, c, data, mode="strict": b.val(reqs, desc, c, data, mode)
    z = zone(FIXED, 1)
    ff = frame_of(FIXED, params(1, pat(z), rule=0, pointer=0, count=1))
    vf = frame_of(VAR, params(1, pat(5), rule=7, count=1))
    tf = frame_of(TRUNC, tparams(5, pat(7), map_id=2), "encode_truncated")
    v(["USLP-003"], "Fixed-length frame: accepted", FIXED, ff)
    v(["USLP-003"], "Variable-length frame: accepted", VAR, vf)
    v(["USLP-002"], "Truncated frame: accepted", TRUNC, tf)
    v([], "Reason 13: 3 octets", VAR, vf[:3])
    v([], "Reason 13: 6 octets of a non-truncated header", VAR, vf[:6])
    v(["USLP-001"], "Reason 1: TFVN 0000", VAR, reheader(vf, tfvn=0))
    v(["USLP-001", "API-032"], "Reason 1: TFVN 1101, lenient", VAR, reheader(vf, tfvn=0b1101),
      mode="lenient")
    v([], "Reason 2: SCID 0x0001", VAR, reheader(vf, scid=1))
    v(["USLP-001"], "Reason 3: flags 01", VAR, reheader(vf, bypass=0, pcc=1))
    v(["API-031"], "Reason 4: Reserve Spares 01, strict", VAR, reheader(vf, spare=1))
    v(["API-032"], "Reason 4 in lenient mode: accepted (ReserveSparesSet)", VAR,
      reheader(vf, spare=1), mode="lenient")
    v([], "Reason 5: VCID 2, not on the Master Channel", VAR, reheader(vf, vcid=2))
    v(["USLP-009"], "Reason 5: VCID 63 on a variable-length channel", VAR,
      reheader(vf, vcid=OID_VCID))
    v([], "Reason 6: MAP 4 on VC 1", VAR, reheader(vf, map=4))
    v(["USLP-002"], "Reason 7: truncated header on VC 1", TRUNC, thdr(1) + tf[4:])
    v(["USLP-004"], "Reason 8: expedited frame on VC 4 with the sequence-controlled length",
      VAR, reheader(frame_of(VAR, params(4, pat(3), rule=7)), bypass=1))
    v([], "Reason 9: OCF Flag 1 on a VC without OCF", FIXED,
      seal(ff[:6] + bytes([ff[6] | 0x08]) + ff[7:-6] + OCF_WORD))
    f2 = frame_of(FIXED, params(2, pat(zone(FIXED, 2, 5, ocf=True)), rule=0, pointer=0,
                                map_id=5, ocf=OCF_WORD))
    v([], "Reason 9: OCF Flag 0 on a VC that requires the OCF", FIXED, reheader(f2, ocf=0))
    v(["USLP-003", "API-012"], "Reason 10: Frame Length + 1 = 39 on a 40-octet channel", FIXED,
      seal(hdr(1, 39) + ff[7:-3]))
    v([], "Reason 11: 301 octets on a 300-octet channel (header only)", VAR,
      hdr(1, 301) + bytes(1))
    v(["GEN-053"], "Reason 11: Frame Length 0xFFFF (65536 octets) on a 65535-octet channel",
      VAR_LARGE_1, hdr(1, MAX_FRAME) + bytes(1))
    v(["CMN-041"], "Reason 12: 11 octets on VC 1, no TFDZ octet after the 1-octet TFDF Header",
      VAR, seal(hdr(1, 11) + bytes([1, 0xE0 | UPID_MSI1])))
    v(["CMN-041"], "Reason 12: OCF Flag 1 and 15 octets, no TFDZ octet", VAR,
      seal(hdr(1, 15, ocf=1) + bytes([1, 0xE0]) + OCF_WORD))
    v(["CMN-041", "API-070"], "Reason 12: VC 4 with SDLS 6/4 and 22 octets, no TFDZ octet",
      VAR, seal(hdr(4, 22, vcf_len=2) + bytes(2 + 6) + bytes([0xE0]) + bytes(4)))
    v([], "Reason 13: one octet more than Frame Length + 1", VAR, vf + b"\x00")
    v([], "Reason 13: one octet fewer", VAR, vf[:-1])
    v(["USLP-002"], "Reason 13: truncated frame, one octet more than its length", TRUNC,
      tf + b"\x00")
    v([], "Reason 14: FECF wrong", VAR, vf[:-1] + bytes([vf[-1] ^ 1]))
    v(["USLP-016"], "Reason 14 before 15: a bit error in the rule is a FECF error", FIXED,
      ff[:8] + bytes([ff[8] ^ 0x60]) + ff[9:])
    v(["USLP-016"], "Reason 15: rule 011 on a fixed-length VC", FIXED,
      retfdf(ff, 8, bytes([3 << 5 | UPID_OCTETS])))
    v(["USLP-016"], "Reason 15: rule 001 on a variable-length VC", VAR,
      retfdf(vf, 8, bytes([1 << 5 | UPID_MSI1])))
    v(["USLP-016", "USLP-002"], "Reason 15: truncated frame with rule 010", TRUNC,
      tf[:4] + bytes([2 << 5 | UPID_MSI1]) + tf[5:])
    v(["USLP-006"], f"Reason 16: FHP {z} beyond the zone", FIXED,
      retfdf(ff, 9, z.to_bytes(2, "big")))
    of = frame_of(OID, oparams(0, iz=bytes(3)), "encode_oid")
    v(["USLP-009", "API-031"], "Reason 17: OID frame with MAP ID 3, strict", OID,
      reheader(of, map=3))
    v(["USLP-009", "API-032"], "Reason 17 in lenient mode: accepted", OID, reheader(of, map=3),
      mode="lenient")
    v(["USLP-001"], "Reason 1 comes before reason 2: TFVN 0000 and SCID 0x0001", VAR,
      reheader(vf, tfvn=0, scid=1))
    v(["USLP-004"], "Reason 8 comes before reason 9: count length 2 and OCF Flag 1 on VC 1 "
      "(1-octet count, no OCF)", FIXED,
      reheader(ff, vcf_len=2, ocf=1))
    return b.cases


def gen_invalid():
    b = Builder("inv")
    d = lambda reqs, desc, c, data, mode="strict": b.dec(
        reqs + ["API-031" if mode == "strict" else "API-032"], desc, c, data, mode)
    z = zone(FIXED, 1)
    ff = frame_of(FIXED, params(1, pat(z), rule=0, pointer=0, count=1))
    vf = frame_of(VAR, params(1, pat(5), rule=7, count=1))
    d([], "3 octets: Incomplete", VAR, vf[:3])
    d([], "5 octets of a non-truncated header: Incomplete", VAR, vf[:5])
    d(["USLP-001"], "TFVN 0000: UnsupportedVersion", VAR, reheader(vf, tfvn=0))
    d(["USLP-001", "API-032"], "TFVN 1111 in lenient mode: UnsupportedVersion", VAR,
      reheader(vf, tfvn=0xF), mode="lenient")
    d([], "SCID 0x0001: UnknownChannel", VAR, reheader(vf, scid=1))
    d(["USLP-001"], "Flags 01: ReservedValue", VAR, reheader(vf, bypass=0, pcc=1))
    d(["API-031"], "Reserve Spares 11, strict: ReservedValue", VAR, reheader(vf, spare=3))
    d(["API-032"] + DEC, "Reserve Spares 11, lenient: ReserveSparesSet", VAR,
      reheader(vf, spare=3), mode="lenient")
    d([], "VCID 2: UnknownChannel", VAR, reheader(vf, vcid=2))
    d([], "MAP 1 on VC 1: UnknownChannel", VAR, reheader(vf, map=1))
    d(["USLP-004"], "VCF Count Length 0 on VC 1 (expects 1): LengthMismatch", VAR,
      seal(hdr(1, len(vf) - 1, vcf_len=0) + vf[8:-2]))
    d([], "OCF Flag 1 on a VC without OCF: InvalidField", FIXED,
      seal(ff[:6] + bytes([ff[6] | 0x08]) + ff[7:-6] + OCF_WORD))
    d(["USLP-003", "API-012"], "Frame Length + 1 = 41 on a 40-octet channel: LengthMismatch",
      FIXED, seal(hdr(1, 41) + ff[7:-2] + b"\x00"))
    d(["API-032"], "301 octets on a 300-octet channel, lenient: OutOfRange", VAR,
      hdr(1, 301) + bytes(294), mode="lenient")
    d(["CMN-041"], "No TFDZ octet: InvalidField", VAR,
      seal(hdr(1, 11) + bytes([1, 0xE0 | UPID_MSI1])))
    d(["API-011"], "Variable-length frame, 3 octets missing: Incomplete", VAR, vf[:-3])
    d([], "FECF wrong: CrcMismatch", FIXED, ff[:-2] + bytes([ff[-2] ^ 0x80, ff[-1]]))
    d(["API-032"], "FECF wrong in lenient mode: CrcMismatch", VAR,
      vf[:-1] + bytes([vf[-1] ^ 0xFF]), mode="lenient")
    d(["USLP-016"], "Rule 111 on a fixed-length VC: InvalidField", FIXED,
      retfdf(ff, 8, bytes([7 << 5 | UPID_PACKETS])))
    d(["USLP-006"], "FHP 0xFFFE: Malformed", FIXED, retfdf(ff, 9, bytes([0xFF, 0xFE])))
    d(["USLP-002"], "Truncated header on VC 1 (no truncated length): InvalidField", VAR,
      thdr(1) + bytes([0xE0]) + pat(4))
    w = lambda reqs, desc, c, p, op="encode": b.enc(reqs + WERR, desc, c, p, op)
    w([], "encode on SCID 0x0001: UnknownChannel", VAR, params(1, pat(4), scid=1))
    w([], "encode on VCID 2: UnknownChannel", VAR, params(2, pat(4)))
    w([], "encode on VC 1 MAP 1: UnknownChannel", VAR, params(1, pat(4), map_id=1))
    w(["USLP-016"], "encode, rule 011 on a fixed-length VC: InvalidField", FIXED,
      params(1, pat(z + 2), rule=3))
    w(["USLP-004"], "encode, count 256 on a 1-octet count: OutOfRange", VAR,
      params(1, pat(4), count=256))
    w([], "encode, an OCF on a VC without OCF: InvalidField", FIXED,
      params(1, pat(z - 4), rule=0, pointer=0, ocf=OCF_WORD))
    w([], "encode, no OCF on a VC that requires it: InvalidField", FIXED,
      params(2, pat(zone(FIXED, 2, 5)), rule=0, pointer=0, map_id=5))
    w(["CMN-041"], "encode, an empty TFDZ on a variable-length VC: InvalidField", VAR,
      params(1, b""))
    w(["CMN-041"], "encode, an empty TFDZ on a fixed-length VC: InvalidField", FIXED,
      params(1, b"", rule=0, pointer=NO_POINTER))
    w(["USLP-003"], f"encode, {z - 1} TFDZ octets on a fixed-length VC ({z} fill it): "
      "LengthMismatch", FIXED, params(1, pat(z - 1), rule=0, pointer=0))
    w(["USLP-003"], f"encode, {z + 1} TFDZ octets on a fixed-length VC: LengthMismatch", FIXED,
      params(1, pat(z + 1), rule=0, pointer=0))
    w(["USLP-006"], "encode, rule 000, FHP 0xFFFE: OutOfRange", FIXED,
      params(1, pat(z), rule=0, pointer=0xFFFE))
    w(["USLP-016"], "Row order: MAP not configured before the rule", FIXED,
      params(1, pat(z), rule=3, map_id=9))
    w(["USLP-016"], "Row order: the rule before the VCF Count", FIXED,
      params(1, pat(z), rule=4, count=1 << 20))
    w([], "Row order: the VCF Count before the OCF", FIXED,
      params(1, pat(z), rule=0, pointer=0, count=256, ocf=OCF_WORD))
    w(["USLP-008"], "Row order: the OCF before the Insert Zone", VAR,
      params(4, pat(3), ocf=OCF_WORD, iz=bytes(2)))
    w(["USLP-008"], "Row order: the Insert Zone before the empty TFDZ", FIXED,
      params(1, b"", rule=0, pointer=0, iz=bytes(1)))
    w([], "Row order: the TFDZ length before the pointer", FIXED,
      params(1, pat(z - 1), rule=0, pointer=0xFFF0))
    w([], "encode, a 300-octet TFDZ on the 300-octet channel: OutOfRange", VAR,
      params(1, pat(300)))
    w([], "Row order: SCID before VCID", VAR, params(5, pat(4), scid=2))
    b.add(["API-013"], "peek_length, 3 octets: Incomplete", "peek_length", VAR, data=vf[:3])
    b.add(["API-013", "USLP-001"], "peek_length, TFVN 0000: UnsupportedVersion",
          "peek_length", VAR, data=reheader(vf, tfvn=0)[:6])
    b.add(["API-013"], "peek_length, fixed-length channel, 3 octets: Incomplete",
          "peek_length", FIXED, data=ff[:3])
    return b.cases


def gen_config():
    b = Builder("cfg")
    req = ["GEN-060", "API-040"]
    c = lambda reqs, desc, cfg: b.add(req + reqs, desc, "config", cfg)
    c([], "The fixed-length channel of the rule 000 cases: accepted", FIXED)
    c([], "The variable-length channel of the rule 011-111 cases: accepted", VAR)
    c(["USLP-002"], "The truncated-frame channel: accepted", TRUNC)
    c([], "Fixed-length frame length 8: InvalidConfig", pc("fixed", 8, [], []))
    c([], "Fixed-length frame length 65537: InvalidConfig", pc("fixed", 65537, [], []))
    c([], "Variable-length maximum 65537: InvalidConfig", pc("variable", 65537, [], []))
    c([], "Variable-length maximum 8: InvalidConfig", pc("variable", 8, [], []))
    c([], "Variable-length maximum 65536 (§4.1.2.7 note 4): accepted", VAR_LARGE)
    c(["USLP-008"], "Insert Zone 2 on a variable-length channel: InvalidConfig",
      pc("variable", 64, [vc(1, "variable")], [mp(1, 0)], iz=2))
    c([], "VCID 64: OutOfRange", pc("fixed", 64, [vc(64)], []))
    c(["USLP-004"], "Sequence-controlled count length 8: InvalidConfig",
      pc("fixed", 64, [vc(1, seq=8)], []))
    c(["USLP-004"], "Expedited count length 8: InvalidConfig", pc("fixed", 64, [vc(1, exp=8)],
                                                                    []))
    c(["USLP-002"], "Truncated frame length 5: InvalidConfig",
      pc("variable", 64, [vc(1, "variable", trunc=5)], []))
    c(["USLP-002"], "Truncated frame length 33: InvalidConfig",
      pc("variable", 64, [vc(1, "variable", trunc=33)], []))
    c(["API-070"], "Security Header 65: InvalidConfig", pc("fixed", 200, [vc(1, sec=(65, 0))],
                                                          []))
    c(["API-070"], "Security Trailer 65536: InvalidConfig",
      pc("fixed", 200, [vc(1, sec=(0, 65536))], []))
    c([], "OCF allowed on a fixed-length VC: InvalidConfig",
      pc("fixed", 64, [vc(1, ocf="allowed")], []))
    c([], "OCF required on a variable-length VC: InvalidConfig",
      pc("variable", 64, [vc(1, "variable", ocf="required")], []))
    c(["USLP-002"], "A truncated frame length on a fixed-length VC: InvalidConfig",
      pc("fixed", 64, [vc(1, trunc=10)], []))
    c(["USLP-016", "USLP-009"], "VCID 63 as a variable-length VC: InvalidConfig",
      pc("variable", 64, [vc(OID_VCID, "variable")], []))
    c(["USLP-009"], "VCID 63 with SDLS per MAP: InvalidConfig",
      pc("fixed", 64, [vc(OID_VCID, sec="per_map")], []))
    c([], "MAP on VCID 64: OutOfRange", pc("fixed", 64, [vc(1)], [mp(64, 0)]))
    c([], "MAP ID 16: OutOfRange", pc("fixed", 64, [vc(1)], [mp(1, 16)]))
    c(["API-070"], "MAP Security Header 65: InvalidConfig",
      pc("fixed", 200, [vc(1, sec="per_map")], [mp(1, 0, 65, 0)]))
    c(["API-070"], "MAP Security Trailer 65536: InvalidConfig",
      pc("fixed", 200, [vc(1, sec="per_map")], [mp(1, 0, 0, 65536)]))
    c([], "The same VC twice: InvalidConfig", pc("fixed", 64, [vc(1), vc(1, seq=2)], []))
    c([], "The same VCID on two SCIDs: accepted",
      pc("fixed", 64, [vc(1), vc(1, scid=SCID16)], [mp(1, 0), mp(1, 0, scid=SCID16)]))
    c([], "A variable-length VC on a fixed-length channel: InvalidConfig (Table 5-3 note 2)",
      pc("fixed", 64, [vc(1, "variable")], []))
    c([], "A fixed-length VCID 63 on a variable-length channel: InvalidConfig",
      pc("variable", 64, [vc(OID_VCID, seq=0, exp=0)], []))
    c(["CMN-041"], "Fixed-length channel of 13 octets, VC with a 1-octet count and FECF: "
      "no TFDZ octet, InvalidConfig", pc("fixed", 13, [vc(1, seq=1, exp=0)], [mp(1, 0)]))
    c(["CMN-041"], "Fixed-length channel of 14 octets, the same VC: one TFDZ octet, accepted",
      pc("fixed", 14, [vc(1, seq=1, exp=0)], [mp(1, 0)]))
    c(["CMN-041"], "Room counts the longer of the two count lengths: 7 expedited, InvalidConfig",
      pc("fixed", 14, [vc(1, seq=1, exp=7)], [mp(1, 0)]))
    c(["CMN-041", "API-070"], "Room counts the VC's SDLS and OCF: InvalidConfig",
      pc("fixed", 24, [vc(1, seq=1, exp=1, ocf="required", sec=(6, 2))], [mp(1, 0)]))
    c(["CMN-041"], "Variable-length channel of 9 octets without FECF, VC without count: "
      "accepted", pc("variable", 9, [vc(1, "variable", seq=0, exp=0)], [mp(1, 0)],
                     fecf_=False))
    c([], "16 VCs: accepted", pc("variable", 64, [vc(n, "variable") for n in range(16)], []))
    c([], "17 VCs: CapacityExceeded", pc("variable", 64, [vc(n, "variable")
                                                          for n in range(17)], []))
    c([], "A MAP whose VC is not given: InvalidConfig", pc("fixed", 64, [vc(1)], [mp(2, 0)]))
    c(["USLP-009"], "A MAP on VCID 63: InvalidConfig",
      pc("fixed", 64, [vc(OID_VCID, seq=0, exp=0)], [mp(OID_VCID, 0)]))
    c([], "The same MAP twice: InvalidConfig", pc("fixed", 64, [vc(1)], [mp(1, 0), mp(1, 0)]))
    c(["API-070"], "A MAP with SDLS lengths on a VC that sets them: InvalidConfig",
      pc("fixed", 64, [vc(1, sec=(2, 0))], [mp(1, 0, 0, 1)]))
    c(["CMN-041", "API-070"], "A MAP whose SDLS lengths leave no TFDZ octet: InvalidConfig",
      pc("fixed", 32, [vc(1, sec="per_map")], [mp(1, 0), mp(1, 1, 17, 2)]))
    c(["CMN-041", "API-070"], "A MAP whose SDLS lengths leave one TFDZ octet: accepted",
      pc("fixed", 32, [vc(1, sec="per_map")], [mp(1, 0), mp(1, 1, 16, 2)]))
    c([], "32 MAPs: accepted", pc("fixed", 64, [vc(1), vc(2)],
                                  [mp(v, m) for v in (1, 2) for m in range(16)]))
    c([], "33 MAPs: CapacityExceeded", pc("fixed", 64, [vc(1), vc(2), vc(3)],
                                          [mp(v, m) for v in (1, 2, 3) for m in range(16)][:33]))
    c(["USLP-012", "API-075"], "Truncated frame length 27 on a VC with SDLS: InvalidConfig",
      pc("variable", 64, [vc(1, "variable", trunc=27, sec=(1, 0))], [mp(1, 0)]))
    c(["USLP-012", "API-075"], "Truncated frame length 27 on a per-MAP VC whose MAP 3 uses "
      "SDLS: InvalidConfig", pc("variable", 64, [vc(1, "variable", trunc=27, sec="per_map")],
                                [mp(1, 0), mp(1, 3, 0, 4)]))
    c(["USLP-012", "API-075"], "Truncated frame length 28 with SDLS: accepted",
      pc("variable", 64, [vc(1, "variable", trunc=28, sec=(1, 0))], [mp(1, 0)]))
    # Build order (the contract): the first error in that order is reported.
    c([], "Order: the Physical Channel before a VC (Insert Zone on variable, VCID 64)",
      pc("variable", 64, [vc(64)], [], iz=1))
    c([], "Order: a VC's own values before its relations (count length 8 and OCF allowed on "
      "fixed)", pc("fixed", 64, [vc(1, seq=8, ocf="allowed")], []))
    c([], "Order: the count lengths before the truncated frame length",
      pc("variable", 64, [vc(1, "variable", exp=9, trunc=40)], []))
    c([], "Order: OCF before the truncated frame length on a fixed VC",
      pc("fixed", 64, [vc(1, ocf="allowed", trunc=10)], []))
    c([], "Order: every VC before every MAP (MAP ID 16, then a VC with VCID 64)",
      pc("fixed", 64, [vc(1), vc(64)], [mp(1, 16)]))
    c([], "Order: the MAPs' own values before the table (duplicate VC, MAP ID 16)",
      pc("fixed", 64, [vc(1), vc(1)], [mp(1, 16)]))
    c([], "Order: the table's VCs before its MAPs (variable VC on fixed channel, MAP of a "
      "missing VC)", pc("fixed", 64, [vc(1), vc(2, "variable")], [mp(5, 0)]))
    c(["USLP-012"], "Order: the truncated SDLS length is checked last (a duplicate MAP first)",
      pc("variable", 64, [vc(1, "variable", trunc=20, sec=(2, 0))], [mp(1, 0), mp(1, 0)]))
    return b.cases


FILES = {
    "fixed_packets_rule000.json": (gen_fixed_packets, "fixed-length TFDZ, rule 000, FHP"),
    "fixed_mapa_rules001_010.json": (gen_fixed_mapa, "fixed-length TFDZ, rules 001 and 010, "
                                                     "LVO"),
    "variable_rules011_111.json": (gen_variable, "variable-length TFDZ, rules 011-111"),
    "vcf_count_lengths_0_to_7.json": (gen_vcf, "VCF Count Lengths 0-7 for both QoS"),
    "truncated.json": (gen_truncated, "truncated Transfer Frames (Annex D)"),
    "truncated_sdls_layout.json": (gen_truncated_sdls, "truncated frames with SDLS "
                                                       "(Annex D1.2.3-4)"),
    "oid.json": (gen_oid, "Only Idle Data frames"),
    "insert_zone.json": (gen_insert_zone, "Insert Zone"),
    "protocol_control_cop1.json": (gen_protocol_control, "protocol control frames carrying "
                                                         "COP-1 control commands"),
    "delimit_multiple_frames_with_fill.json": (gen_delimit, "Frame Delimiting and Fill Data "
                                                            "Removal"),
    "validation_failures.json": (gen_validation, "USLP Frame Validation Checks"),
    "invalid.json": (gen_invalid, "decoder, writer, and peek_length errors"),
    "config.json": (gen_config, "managed parameters"),
}

NOTE = (
    "732.1-B-3 prints no example frame, only the first OID octets (Annex H). Hand-derived "
    "from 732.1-B-3: §4.1.2 (Primary Header: TFVN 4 bits = 1100, SCID 16, Source-or-"
    "Destination 1, VCID 6, MAP ID 4, End of Frame Primary Header Flag 1, Frame Length 16 = "
    "total - 1, Bypass/Sequence Control 1, Protocol Control Command 1, Reserve Spares 2, OCF "
    "Flag 1, VCF Count Length 3, VCF Count 0-7 octets; Table 4-1, flags 01 reserved; Table "
    "4-2), §4.1.3 (Insert Zone), §4.1.4 (TFDF Header: rule 3 bits, UPID 5 bits, pointer 16 "
    "bits for rules 000-010 only; Table 4-3; the OID frame of §4.1.4.1.5-12), §4.1.5 (OCF), "
    "§4.1.6 and Annex B (FECF, G(X) = X^16 + X^12 + X^5 + 1, preset by L(X)), §4.3.11.2-3 "
    "(delimiting and the USLP Frame Validation Checks), §5 Tables 5-1 to 5-4 (managed "
    "parameters), §6.3 (Security Header after the Insert Zone, Trailer before the OCF; none "
    "in protocol control frames), Annex D (truncated frames: 4-octet header, 1-octet TFDF "
    "Header, length 6-32 from the VC, SDLS 6/16 and at least 28 octets), Annex H (OID "
    "generator). UPID values from the SANA snapshot specs/sana/uslp_protocol_id.tsv, read by "
    "the model at run time. COP-1 control commands from 232.0-B-4 §4.1.3.3 (Unlock = 00, Set "
    "V(R) = 82 00 V(R)). Check order, validation reason codes, the delimiter's valid header, "
    "the writers' error order, and the configuration kept, as docs/standards/uslp-frame.md "
    "§2.1 and §3 state them (OI-35, OI-44). SCID 0x155 is the fictional SCID of the reference "
    "scenario (APP-026); SCID 0xF0A5 is also fictional. Expected values computed by an "
    "independent Python model of those clauses, test-vectors/models/uslp_frame.py (TV-023), "
    "written independently of the Rust codec and sharing no code or method with it (headers "
    "as bit strings from the figure widths, the FECF as the Annex B remainder by GF(2) long "
    "division and checked by the B1.2.1 syndrome, the OID data from the Galois form of "
    "Figure H-2, checked against the Fibonacci form and the printed pattern). Error cases "
    "name the field code of 03 §9.1. One rule-000 frame with its FECF, register octet by "
    "octet, and one truncated frame are worked by hand in "
    "docs/verification/derivations/uslp-frame.md. `python3 -I "
    "test-vectors/models/uslp_frame.py --check test-vectors/uslp/*.json` recomputes every "
    "expected value. The OID data of a decoded frame is not checked against the generator's "
    "sequence: it depends on the generator's position across frames, which a stateless decoder "
    "does not know, and receivers discard it. 732.1-B-3 §6.3.2.2 refers to D1.2.5 for the "
    "truncated header of a frame with SDLS, where D1.3 is meant; the cases follow D1.2.3, "
    "D1.2.4 and D1.3. Author of the model: an AI agent, Claude Opus 5.5, recorded as "
    "co-author of commit 0459ef0 (2026-10-23), whose message states that it wrote the model "
    "from 732.1-B-3 and uslp-frame.md without reading the Rust code; the history records no "
    "briefing of the agent beyond that statement (TV-023)."
)


SUPERSEDED = {
    "uslp-oid-019": (
        "VCID 63 is valid on a fixed-length channel without an entry (maintainer decision of "
        "2026-10-25 following 732.1-B-3 Table 5-2 note 2): encode_oid on a channel that does "
        "not list VCID 63 succeeds, with no VCF Count, OCF, or Security fields"),
}

LOWER_LIMIT = (
    " A Physical Channel frame length below 9 octets is rejected even when no VC is "
    "configured: 9 is the 7-octet Primary Header, the 1-octet TFDF Header of a variable-length "
    "frame, and one TFDZ octet (OI-44 item 2); the TFDF Header of a fixed-length VC has 3 "
    "octets, so such a VC needs 11."
)
TRAILER_LIMIT = (
    " The Security Trailer is limited to 65 535 octets by the 16-bit Frame Length; API-071 "
    "states the limit of the Header (64 octets) only."
)
ORDER = (
    " The order of the configuration checks (uslp-frame.md §2.1) is a project choice; the "
    "Blue Book gives none."
)
EXTRA = {
    "uslp-cfg-004": LOWER_LIMIT, "uslp-cfg-007": LOWER_LIMIT, "uslp-cfg-034": LOWER_LIMIT,
    "uslp-cfg-016": TRAILER_LIMIT, "uslp-cfg-025": TRAILER_LIMIT,
    **{f"uslp-cfg-{n:03d}": ORDER for n in range(48, 56)},
    "uslp-val-013": " VCID 63 is rejected on a variable-length channel by Table 5-1 note 4.",
    "uslp-cfg-029": " Table 5-1 note 4: OID frames are not generated for variable-length frames.",
}


def generate():
    out = {}
    for name, (gen, title) in FILES.items():
        cases = gen()
        for c in cases:
            if c["id"] in EXTRA:
                c["description"] += EXTRA[c["id"]]
            if c["id"] in SUPERSEDED:
                c["superseded"] = {"reason": SUPERSEDED[c["id"]]}
        out[name] = {"kind": "codec", "standard": "732.1-B-3 — " + title,
                     "provenance": "hand-derived",
                     "provenance_note": NOTE + " This file covers " + title + ".",
                     "cases": cases}
    return out


def dump(document):
    return json.dumps(document, indent=2, ensure_ascii=False) + "\n"


def self_test():
    """The methods against the values the Blue Book and the seed vectors print."""
    bad = 0
    with open(os.path.join(ROOT, "test-vectors", "core", "crc16.json"), encoding="utf-8") as f:
        for c in json.load(f)["cases"]:
            if "superseded" in c or c.get("operation") != "crc16":
                continue
            want = int(c["expect"]["value_hex"], 16)
            if fecf(bytes.fromhex(c["input_hex"])) != want:
                print(f"core/crc16.json {c['id']}: the Annex B remainder differs")
                bad += 1
            if syndrome(seal(bytes.fromhex(c["input_hex"]))) != 0:
                print(f"core/crc16.json {c['id']}: the B1.2.1 syndrome is not zero")
                bad += 1
    with open(os.path.join(ROOT, "test-vectors", "core", "oid.json"), encoding="utf-8") as f:
        for c in json.load(f)["cases"]:
            if "superseded" in c or c.get("operation") != "oid_fill":
                continue
            want = c["expect"]["output_hex"]
            n = c["params"]["octets"]
            if hx(OidGenerator().octets(n)) != want or hx(oid_fibonacci(n)) != want:
                print(f"core/oid.json {c['id']}: the Annex H pattern differs")
                bad += 1
    if OidGenerator().octets(512) != oid_fibonacci(512):
        print("the Galois and Fibonacci forms of Annex H differ")
        bad += 1
    return bad


def check_field_codes():
    catalogue = os.path.join(ROOT, "test-vectors", "core", "fields.json")
    with open(catalogue, encoding="utf-8") as f:
        names = {e["code"]: e["name"] for e in json.load(f)["fields"]}
    bad = 0
    for name, code in F.items():
        if names.get(code) != name:
            print(f"{catalogue}: code {code} is {names.get(code)!r}, the model expects {name!r}")
            bad += 1
    return bad


def check(paths):
    bad = self_test() + check_field_codes()
    generated = generate()
    seen = set()
    for path in paths:
        with open(path, encoding="utf-8") as f:
            document = json.load(f)
        seen.add(os.path.basename(path))
        live = [c for c in document["cases"] if "superseded" not in c]
        for c in live:
            try:
                want = expect(c)
            except (KeyError, ValueError, TypeError) as e:
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
            print(f"uslp_frame: {bad} difference(s)")
            return 1
        return 0
    if len(argv) == 2 and argv[0] == "--write":
        if self_test() + check_field_codes():
            return 1
        for name, document in generate().items():
            with open(os.path.join(argv[1], name), "w", encoding="utf-8") as f:
                f.write(dump(document))
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
