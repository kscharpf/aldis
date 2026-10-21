"""Independent model of the AOS Frame Header Error Control (FHEC) for the vector files
test-vectors/aos/fhec_*.json (AOS-013, AOS-014; M2 plan D10).

Source: CCSDS 732.0-B-5 EC1 §4.1.2.6.1 and §4.1.2.6.5 a-h, the same code as 732.0-B-4
§4.1.2.6.5 (M2 plan D11, OI-04). Parameters, not text: a Reed-Solomon (15,11) code over
GF(2^4) with field polynomial F(x) = x^4 + x + 1 and code generator
g(x) = (x + a^6)(x + a^7)(x + a^8)(x + a^9), systematic, shortened by five leading symbols
of zero virtual fill; symbols 6-11 are header bits 0-15 and 40-47 (octets 0, 1, and 5),
symbols 12-15 the FHEC (bits 48-63), each symbol sent from its left bit; bit 0 is the most
significant bit of octet 0 (§1.6.3). Octets 2-4, the Virtual Channel Frame Count, are not
protected.

The model shares no method with the Rust codec (crates/aldis-aos/src/fhec.rs), and must stay
independent of it: never port code from it, and never fix the model to agree with it.
GF(2^4) arithmetic is carry-less multiplication reduced by F(x), with no tables; a 15-symbol
word is a codeword when its polynomial vanishes at a^6 ... a^9; the FHEC is found by solving
the syndrome equations with GF(2) linear algebra, not by division by g(x); decoding looks up
the syndrome in a table built by enumerating every pattern of one or two symbol errors over
the ten transmitted symbols (10 * 15 + 45 * 15^2 = 10 275 patterns). The Rust codec divides by
g(x) and decodes algebraically (Peterson, root search, Forney).

Usage, from the repository root:

    python3 -I test-vectors/models/aos_fhec.py --check test-vectors/aos/fhec_*.json
    python3 -I test-vectors/models/aos_fhec.py --print 400100000000 4001000000004C3A
    python3 -I test-vectors/models/aos_fhec.py --write test-vectors/aos

--check recomputes the expected outcome of every live case from its operation and input, and
compares the cases the generator below writes with the file's; it exits 1 on any difference.
--print encodes six octets or decodes eight. --write regenerates the files (cases and
provenance notes); it documents how the inputs were chosen.
"""
import itertools
import json
import os
import sys

F = 0b10011  # x^4 + x + 1 (§4.1.2.6.5)
ALPHA = 2  # a, a root of F(x)
ROOTS = (6, 7, 8, 9)  # g(x) has the roots a^6 ... a^9 (§4.1.2.6.5)
FILL = 5  # leading symbols of zero virtual fill (shortened code)


def mul(a, b):
    """GF(2^4) product: carry-less multiplication reduced by F(x)."""
    r = 0
    for i in range(4):
        if b >> i & 1:
            r ^= a << i
    for i in (7, 6, 5, 4):
        if r >> i & 1:
            r ^= F << (i - 4)
    return r


def pw(a, n):
    r = 1
    for _ in range(n):
        r = mul(r, a)
    return r


def ev(c, x):
    """Evaluates the polynomial whose coefficients are `c`, highest degree first."""
    s = 0
    for v in c:
        s = mul(s, x) ^ v
    return s


def syn(c15):
    """The four syndromes of a 15-symbol word (symbol 1 first)."""
    return [ev(c15, pw(ALPHA, j)) for j in ROOTS]


def nibbles(o):
    """The ten transmitted symbols of an 8-octet header: octets 0, 1, 5, 6, and 7."""
    return [o[0] >> 4, o[0] & 15, o[1] >> 4, o[1] & 15, o[5] >> 4, o[5] & 15,
            o[6] >> 4, o[6] & 15, o[7] >> 4, o[7] & 15]


def word(h):
    return [0] * FILL + nibbles(h)


def tobits(s):
    return sum(v << (12 - 4 * i) for i, v in enumerate(s))


def parity(info6):
    """The four check symbols, by GF(2) elimination on the syndromes of unit parity words."""
    base = syn([0] * FILL + list(info6) + [0] * 4)
    basis = {}
    for b in range(16):
        p = [0] * 4
        p[b // 4] = 1 << (3 - b % 4)
        v, comb = tobits(syn([0] * 11 + p)), 1 << b
        for bit in range(15, -1, -1):
            if not (v >> bit & 1):
                continue
            if bit in basis:
                v ^= basis[bit][0]
                comb ^= basis[bit][1]
            else:
                basis[bit] = (v, comb)
                break
    assert len(basis) == 16
    v, comb = tobits(base), 0
    for bit in range(15, -1, -1):
        if v >> bit & 1:
            v ^= basis[bit][0]
            comb ^= basis[bit][1]
    assert v == 0
    p = [0] * 4
    for b in range(16):
        if comb >> b & 1:
            p[b // 4] ^= 1 << (3 - b % 4)
    return p


def fhec(h6):
    info = [h6[0] >> 4, h6[0] & 15, h6[1] >> 4, h6[1] & 15, h6[5] >> 4, h6[5] & 15]
    p = parity(info)
    return bytes([p[0] << 4 | p[1], p[2] << 4 | p[3]])


def encode(h6):
    return bytes(h6) + fhec(h6)


def table():
    """Syndrome -> error pattern for no error and every pattern of one or two symbol errors."""
    t = {}
    pats = [()]
    for i in range(10):
        for v in range(1, 16):
            pats.append(((i, v),))
    for i, j in itertools.combinations(range(10), 2):
        for v in range(1, 16):
            for w in range(1, 16):
                pats.append(((i, v), (j, w)))
    for p in pats:
        e = [0] * 10
        for i, v in p:
            e[i] = v
        s = tuple(syn([0] * FILL + e))
        assert s not in t
        t[s] = e
    return t, len(pats)


def apply(h, e):
    c = [a ^ b for a, b in zip(nibbles(h), e)]
    o = bytearray(h)
    o[0] = c[0] << 4 | c[1]
    o[1] = c[2] << 4 | c[3]
    o[5] = c[4] << 4 | c[5]
    o[6] = c[6] << 4 | c[7]
    o[7] = c[8] << 4 | c[9]
    return bytes(o)


def decode(h, t):
    """(corrected header, corrected symbols), or None when no pattern of weight <= 2 fits."""
    s = tuple(syn(word(h)))
    if s not in t:
        return None
    e = t[s]
    return apply(h, e), sum(1 for v in e if v)


T, PATTERNS = table()


def hx(b):
    return bytes(b).hex().upper()


def expect(case):
    """The expected outcome of a case, from the model alone."""
    op, data = case["operation"], bytes.fromhex(case.get("input_hex", ""))
    if op == "fhec_encode" and len(data) == 6:
        full = encode(data)
        return {"ok": True, "fhec_hex": hx(full[6:]), "output_hex": hx(full)}
    if op == "fhec_decode" and len(data) == 8:
        r = decode(data, T)
        if r is None:
            return {"ok": False, "error": "HeaderUncorrectable"}
        return {"ok": True, "header_hex": hx(r[0]), "corrected_symbols": r[1]}
    raise ValueError(f"{case['id']}: the model has no operation {op!r} on {len(data)} octets")


# ---- the generator: how the inputs of the vector files were chosen

STD = "732.0-B-5 EC1 §4.1.2.6"
NOTE_COMMON = (
    "Hand-derived from 732.0-B-5 EC1 §4.1.2.6.1 and §4.1.2.6.5 a–h (the same code as "
    "732.0-B-4 §4.1.2.6.5, OI-04, M2 plan D11): RS(15,11) over GF(2^4), F(x) = x^4 + x + 1, "
    "g(x) = (x + a^6)(x + a^7)(x + a^8)(x + a^9), five leading symbols of zero virtual fill, "
    "systematic; symbols 6–11 are header bits 0–15 and 40–47, symbols 12–15 the FHEC "
    "(bits 48–63), each symbol sent from its left bit; bit 0 is the most significant bit of "
    "octet 0 (§1.6.3). 732.0 prints no worked FHEC example. Expected values computed by an "
    "independent Python model of the Blue Book text, written before the Rust codec: GF(2^4) by "
    "carry-less multiplication reduced by F(x), with no tables; a codeword is valid when the "
    "15-symbol polynomial vanishes at a^6…a^9; the parity is solved by GF(2) linear algebra "
    "on those syndromes, not by division by g(x); decoding is a table from the syndrome of each "
    "of the 10 275 patterns of one or two symbol errors over the ten transmitted symbols to the "
    "pattern. The Rust codec divides by g(x) and decodes algebraically (Peterson, root search, "
    "Forney), so the two share no method. Input and output are the eight octets of the primary "
    "header with FHEC; octets 2–4 (the Virtual Channel Frame Count) are not protected. "
)
NOTE_MODEL = (
    "The model is committed as test-vectors/models/aos_fhec.py (TV-023); "
    "`python3 -I test-vectors/models/aos_fhec.py --check test-vectors/aos/fhec_*.json` "
    "recomputes every expected value."
)
HAND = (
    "Case aos-fhec-enc-002 (header 400100000000) is derived by hand, polynomial division over "
    "GF(16) step by step, in docs/verification/derivations/aos-fhec.md, which also works the "
    "single-symbol correction of aos-fhec-c1-001."
)
MISCORRECTION = (
    "This file holds received headers with three or more symbol errors that lie within two "
    "symbols of another codeword: the code cannot detect them (minimum distance 5; 732.0-B-5 "
    "§4.1.2.6.5 note 3), so every conforming decoder returns that other codeword and reports "
    "two corrections (AOS-014, OI-35 item 5). A configured FECF, checked over the corrected "
    "header, rejects such a frame (AOS-014)."
)
UNCORRECTABLE = (
    "Uncorrectable inputs are chosen so that the model has no table entry for their syndrome; "
    "the virtual-fill cases have the syndrome of an error in a virtual fill symbol, which no "
    "transmitted pattern of weight ≤ 2 has."
)


def case(cid, op, data, desc, reqs):
    c = {"id": cid, "requirements": list(reqs), "description": desc, "operation": op,
         "input_hex": hx(data)}
    c["expect"] = expect(c)
    return c


def errs(h, pairs):
    e = [0] * 10
    for i, v in pairs:
        e[i] = v
    return apply(bytes(h), e)


def with_syndrome(info_header, target):
    """A received header with the given systematic octets whose syndrome is `target`."""
    for p in range(1 << 16):
        h = bytes(info_header[:6]) + p.to_bytes(2, "big")
        if tuple(syn(word(h))) == tuple(target):
            return h
    raise SystemExit("no header")


def generate():
    """The vector files: name -> document."""
    enc = [
        ([0, 0, 0, 0, 0, 0], "All-zero systematic symbols: the FHEC is zero"),
        ([0x40, 0x01, 0, 0, 0, 0], "TFVN 01, SCID 0, VCID 1 (worked by hand)"),
        ([0x6A, 0x85, 0x12, 0x34, 0x56, 0x00],
         "TFVN 01, SCID 0xAA (8 bits), VCID 5, frame count 0x123456"),
        ([0x6A, 0x85, 0, 0, 0, 0],
         "Same protected fields with frame count 0: the same FHEC (bits 16–39 not covered)"),
        ([0x40, 0x3F, 0xFF, 0xFF, 0xFF, 0x00], "OID frame: VCID 63, frame count 0xFFFFFF"),
        ([0x7F, 0xFF, 0, 0, 0, 0x3F],
         "Issue 5 10-bit SCID 0x3FF, VCID 63, signaling field 00, cycle 0xF"),
        ([0x55, 0x55, 0, 0, 0, 0xC7], "Replay flag 1, cycle use flag 1, SCID MSBs 00, cycle 7"),
        ([0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF], "All ones"),
    ]
    # One case per systematic symbol position with the symbol 1000 (a^3): the unit columns.
    for k, (octet, shift) in enumerate([(0, 4), (0, 0), (1, 4), (1, 0), (5, 4), (5, 0)]):
        h = [0] * 6
        h[octet] = 0x8 << shift
        enc.append((h, f"Systematic symbol {6 + k} = 1000 (a^3), all others zero"))
    encode_cases = [case(f"aos-fhec-enc-{i + 1:03d}", "fhec_encode", bytes(h), d,
                         ("AOS-013", "TV-002")) for i, (h, d) in enumerate(enc)]

    dec = ("AOS-013", "AOS-014")
    base = encode(bytes([0x40, 0x01, 0, 0, 0, 0]))
    base2 = encode(bytes([0x6A, 0x85, 0x12, 0x34, 0x56, 0x00]))
    c1 = [case("aos-fhec-c1-000", "fhec_decode", base, "Codeword: no correction", dec)]
    vals = [0x1, 0x8, 0xF, 0x6, 0x3, 0xA, 0x5, 0xC, 0x9, 0x7]
    for p in range(10):
        what = "systematic" if p < 6 else "parity (FHEC)"
        c1.append(case(f"aos-fhec-c1-{p + 1:03d}", "fhec_decode", errs(base, [(p, vals[p])]),
                       f"Symbol {p + 6} ({what}) received with error {vals[p]:04b}; corrected",
                       dec))
    c1.append(case("aos-fhec-c1-011", "fhec_decode", errs(base2, [(3, 0xF)]),
                   "VCID nibble all wrong in a header with a non-zero frame count; octets 2–4 "
                   "unchanged", dec))
    h = bytearray(errs(base2, [(5, 0x2)]))
    h[3] ^= 0xFF
    c1.append(case("aos-fhec-c1-012", "fhec_decode", bytes(h),
                   "One cycle-field symbol wrong and the frame count octet 3 flipped: the symbol "
                   "is corrected, the frame count is passed on unaltered", dec))
    h = bytearray(base2)
    h[2] ^= 0xFF
    h[4] ^= 0x01
    c1.append(case("aos-fhec-c1-013", "fhec_decode", bytes(h),
                   "Frame count octets changed only: not covered, no correction, octets passed on",
                   dec))
    assert all(c["expect"]["ok"] for c in c1)

    c2 = []
    pairs = [
        ((0, 0xF), (1, 0xF), "Both symbols of octet 0 (TFVN and SCID bits)"),
        ((2, 0x1), (3, 0x8), "Both symbols of octet 1 (SCID and VCID bits)"),
        ((4, 0x4), (5, 0x2), "Both symbols of octet 5 (signaling field and cycle)"),
        ((6, 0x9), (9, 0x6), "First and last parity symbols"),
        ((0, 0x3), (9, 0xC), "First systematic and last parity symbol"),
        ((1, 0x7), (6, 0xE), "A systematic and a parity symbol"),
        ((7, 0x5), (8, 0xA), "Two parity symbols, across octets 6 and 7"),
        ((3, 0xB), (4, 0xD), "VCID nibble and the first symbol of octet 5"),
    ]
    for n, (a, b, d) in enumerate(pairs):
        c2.append(case(f"aos-fhec-c2-{n + 1:03d}", "fhec_decode",
                       errs(base if n % 2 == 0 else base2, [a, b]),
                       d + f": errors {a[1]:04b} and {b[1]:04b} in symbols {a[0] + 6} and "
                       f"{b[0] + 6}; corrected", dec))
    assert all(c["expect"]["ok"] and c["expect"]["corrected_symbols"] == 2 for c in c2)

    mis = [case("aos-fhec-mis-001", "fhec_decode",
                errs(base2, [(0, 1), (2, 2), (4, 3), (6, 4)]),
                "Four symbol errors (0001, 0010, 0011, 0100 in symbols 6, 8, 10, 12) within two "
                "symbols of another codeword: miscorrected to it, reported as two corrections "
                "(732.0 §4.1.2.6.5 note 3; OI-35 item 5)", dec)]
    assert mis[0]["expect"]["corrected_symbols"] == 2

    un = []
    three = [((0, 0x1), (1, 0x1), (2, 0x1)), ((0, 0xF), (5, 0x3), (9, 0x8)),
             ((6, 0x2), (7, 0x4), (8, 0x6))]
    for t in three:
        un.append(case(f"aos-fhec-un-{len(un) + 1:03d}", "fhec_decode", errs(base, list(t)),
                       "Three symbol errors (symbols " + ", ".join(str(i + 6) for i, _ in t)
                       + "): detected, not correctable", dec))
    received = syn(word(base))
    # Errors whose single-error locator points into the virtual fill (symbols 1-5).
    for fill in (0, 4):
        e15 = [0] * 15
        e15[fill] = 0x5
        h = with_syndrome(base, [a ^ b for a, b in zip(syn(e15), received)])
        un.append(case(f"aos-fhec-un-{len(un) + 1:03d}", "fhec_decode", h,
                       f"Syndrome of one error in virtual fill symbol {fill + 1}: the locator's "
                       "root lies in the virtual fill", dec))
    # One virtual-fill error plus one transmitted error.
    e15 = [0] * 15
    e15[2] = 0x9
    e15[5 + 3] = 0x1
    h = with_syndrome(base, [a ^ b for a, b in zip(syn(e15), received)])
    un.append(case(f"aos-fhec-un-{len(un) + 1:03d}", "fhec_decode", h,
                   "Syndrome of an error in virtual fill symbol 3 and one in symbol 9: one "
                   "locator root in the fill", dec))
    # Syndromes with no consistent locator: one non-zero syndrome only.
    for idx, name in ((0, "S6"), (3, "S9")):
        s = [0, 0, 0, 0]
        s[idx] = 1
        h = with_syndrome(base, [a ^ b for a, b in zip(s, received)])
        un.append(case(f"aos-fhec-un-{len(un) + 1:03d}", "fhec_decode", h,
                       f"Only {name} non-zero (value 0001): no pattern of one or two errors",
                       dec))
    un.append(case(f"aos-fhec-un-{len(un) + 1:03d}", "fhec_decode",
                   errs(base2, [(0, 5), (2, 6), (4, 7), (6, 8)]),
                   "Four symbol errors (symbols 6, 8, 10, 12): detected", dec))
    assert not any(c["expect"]["ok"] for c in un)

    def doc(cases, extra=""):
        return {"kind": "codec", "standard": STD, "provenance": "hand-derived",
                "provenance_note": (NOTE_COMMON + extra).rstrip() + " " + NOTE_MODEL,
                "cases": cases}

    return {
        "fhec_encode.json": doc(encode_cases, HAND),
        "fhec_correct_1_symbol.json": doc(c1),
        "fhec_correct_2_symbols.json": doc(c2),
        "fhec_miscorrection.json": doc(mis, MISCORRECTION),
        "fhec_uncorrectable.json": doc(un, UNCORRECTABLE),
    }


def dump(document):
    return json.dumps(document, indent=2, ensure_ascii=False) + "\n"


def check(paths):
    """Recomputes every live case of `paths`; compares the generated cases too."""
    generated = generate()
    bad = 0
    for path in paths:
        with open(path, encoding="utf-8") as f:
            document = json.load(f)
        live = [c for c in document["cases"] if "superseded" not in c]
        for c in live:
            try:
                want = expect(c)
            except ValueError as e:
                print(f"{path}: {e}")
                bad += 1
                continue
            if c["expect"] != want:
                print(f"{path}: {c['id']}: expect {c['expect']}, the model gives {want}")
                bad += 1
        by_id = {c["id"]: c for c in live}
        for g in generated.get(os.path.basename(path), {"cases": []})["cases"]:
            if g["id"] in by_id and by_id[g["id"]] != g:
                print(f"{path}: {g['id']} differs from the generator (--write)")
                bad += 1
            elif g["id"] not in by_id and g["id"] not in {c["id"] for c in document["cases"]}:
                print(f"{path}: {g['id']}, written by the generator, is missing")
                bad += 1
        print(f"{path}: {len(live)} case(s) checked against the model")
    return bad


def main(argv):
    if len(argv) >= 2 and argv[0] == "--check":
        bad = check(argv[1:])
        if bad:
            print(f"aos_fhec: {bad} difference(s)")
            return 1
        return 0
    if len(argv) == 2 and argv[0] == "--write":
        for name, document in generate().items():
            with open(os.path.join(argv[1], name), "w", encoding="utf-8") as f:
                f.write(dump(document))
        return 0
    if len(argv) >= 2 and argv[0] == "--print":
        for arg in argv[1:]:
            h = bytes.fromhex(arg)
            if len(h) == 6:
                print(arg, "->", hx(encode(h)))
            else:
                r = decode(h, T)
                print(arg, "->", "uncorrectable" if r is None else f"{hx(r[0])}, {r[1]} corrected")
        return 0
    if not argv:
        exp = [pw(ALPHA, i) for i in range(15)]
        print("antilog", [format(x, "04b") for x in exp])
        g = [1]
        for j in ROOTS:
            r = pw(ALPHA, j)
            ng = [0] * (len(g) + 1)
            for i, c in enumerate(g):
                ng[i] ^= c
                ng[i + 1] ^= mul(c, r)
            g = ng
        print("g(x), x^4 ... x^0:", g, "as powers of a:", [exp.index(c) for c in g])
        print("patterns", PATTERNS, "distinct syndromes", len(T))
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
