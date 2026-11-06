"""Independent model of TM synchronization: the ASM and CADU, the two pseudo-random sequences,
and the ASM search engine, for the vector files under test-vectors/sync/ (SYNC-001...SYNC-020,
TV-023; M5 WP-2).

Sources: CCSDS 131.0-B-6: 9.1.2 and Table 9-1 (the CADU), 9.2 (the position of the marker),
9.3.1 and Figure 9-1 (the 32-bit marker, bit 0 first), 10.2 to 10.4 (the randomizer: where it
is applied, the polynomials, the seed of each, the note with the first 40 bits of each
sequence), 11.1 to 11.4 (frame lengths). 132.0-B-3, 732.0-B-5 and 732.1-B-3 (2.4.1 of each)
for the frame protocols that use 131.0. The behaviour of the search engine, its configuration,
the error kinds and the field codes come from docs/standards/tm-sync.md (SYNC-001 to SYNC-020,
OI-80 to OI-82 as decided), docs/03-api-model.md (the step contract, 5; the error catalogue,
9; the field catalogue, 9.1) and docs/07-test-vectors.md. docs/glossary.md for terms.

This program was written blind (TV-023): by an agent that had not read any implementation
(crates/, include/, fuzz/, exerciser/, refapp/; none existed for sync), from the Blue Books
and docs/. For the file layout and the house style, the author also read the existing models
(cop1.py, link_recv.py) and the vector schemas, nothing else. It must stay independent of the
Rust code: never port code from it, and never change the model to agree with it. When a model
and an implementation disagree, the Blue Book decides.

The model holds parameters and code, never Blue Book text.

What it does
------------
* The marker is read from the bit pattern of Figure 9-1 (written as bits), and the hexadecimal
  form is derived from it.
* Both randomizer sequences are generated twice, by the recurrence of the polynomial and by a
  shift register with its taps, and are checked against the 40 bits of 131.0 10.4.3 note 2,
  against each other, and for their period.
* The search engine (`Engine`) is a pure step over plain variables: the last 32 stream bits,
  the count of bits read since the search began, the bits of the unit in progress, and the
  counters. `selftest` compares it with a second formulation that scans the whole stream
  offline, and checks that the chunking of the stream changes nothing (SYNC-010).
* A reading the documents leave open is not pinned: the model raises `Unspecified`, and no
  vector contains such a case (see test-vectors/sync/README.md, "Readings the model refuses").

Usage, from the repository root:

    python3 -I test-vectors/models/sync.py --check test-vectors/sync/*.json
    python3 -I test-vectors/models/sync.py --write [dir]   # regenerate every file

--check replays every live case through the model (the codec cases from their operation and
input, the scenarios step by step), compares the expected outputs, and compares each file with
what the generator writes; it exits 1 on any difference. --write regenerates the files, and is
for files that are not released yet (TV-021).
"""
import copy
import json
import os
import sys

# --------------------------------------------------------------------------------------------
# Bits
# --------------------------------------------------------------------------------------------


def bits_of(data):
    """The octets as a list of bits, most significant bit of each octet first."""
    return [(b >> (7 - i)) & 1 for b in data for i in range(8)]


def bytes_of(bits):
    """The bits as octets; the number of bits must be a multiple of 8."""
    assert len(bits) % 8 == 0
    out = bytearray()
    for i in range(0, len(bits), 8):
        v = 0
        for b in bits[i:i + 8]:
            v = (v << 1) | b
        out.append(v)
    return bytes(out)


def bits_from_text(text):
    return [int(c) for c in text if c in "01"]


def hexs(data):
    return bytes(data).hex().upper()


# --------------------------------------------------------------------------------------------
# The marker (131.0 9.3.1, Figure 9-1: bit 0, the first transmitted bit, on the left)
# --------------------------------------------------------------------------------------------

ASM_BITS = bits_from_text("0001 1010 1100 1111 1111 1100 0001 1101")
ASM = bytes_of(ASM_BITS)
assert len(ASM_BITS) == 32 and ASM.hex().upper() == "1ACFFC1D"

# Frame length limits of 11.3 and 11.4 (uncoded, convolutional only): TM 2048, AOS and USLP 65536.
MAX_FRAME = {"tm": 2048, "aos": 65536, "uslp": 65536}
MAX_ASM_ERRORS = 15  # OI-80, SYNC-016

# Field codes of the catalogue (03 9.1), the sync block and frame_len.
FIELD = {"frame_len": 68, "asm_errors": 250, "sync_protocol": 251, "chunk_len": 252,
         "randomizer_sequence": 253}

# --------------------------------------------------------------------------------------------
# The pseudo-random sequences (131.0 10.4)
# --------------------------------------------------------------------------------------------

# The first 40 bits of each, from the note of 10.4.3 (the leftmost bit first).
NOTE_40 = {
    131071: bits_from_text("0001 1100 0111 0001 1011 1001 0001 1011 1010 1001"),
    255: bits_from_text("1111 1111 0100 1000 0000 1110 1100 0000 1001 1010"),
}
# h(x) as the exponents of its terms, the constant term left out (10.4.1, 10.4.2).
POLY = {131071: (17, 14), 255: (8, 7, 5, 3)}
# The generator register at the start of each unit (10.4.3), the leftmost digit first; the 255-bit
# one is all ones.
SEED_TEXT = {131071: "11000111000111000", 255: "11111111"}


def seq_recurrence(kind, n):
    """The first n bits as a linear recurrence over the sequence s(0), s(1), ...: the sequence of
    a polynomial x^m + x^e1 + ... + 1 satisfies s(k) = s(k-m+e1) + ... + s(k-m); the first m
    bits are the seed digits read from the right (the stage that is shifted out first)."""
    exps = POLY[kind]
    m = exps[0]
    seed = bits_from_text(SEED_TEXT[kind])
    assert len(seed) == m
    s = list(reversed(seed))
    while len(s) < n:
        k = len(s)
        v = s[k - m]  # the constant term of h(x)
        for e in exps[1:]:
            v ^= s[k - m + e]
        s.append(v)
    return s[:n]


def seq_register(kind, n):
    """The first n bits from a shift register with taps (Figures 10-2 and 10-3 show one such
    generator): stage 1 is the leftmost seed digit, the output is the last stage, and the bit
    fed into stage 1 is the exclusive-OR of the last stage and the stages at positions
    m - e + ... for the other exponents e of h(x)."""
    exps = POLY[kind]
    m = exps[0]
    reg = bits_from_text(SEED_TEXT[kind])  # reg[0] is stage 1
    out = []
    for _ in range(n):
        out.append(reg[m - 1])
        fb = reg[m - 1]
        for e in exps[1:]:
            fb ^= reg[m - e - 1]
        reg = [fb] + reg[:m - 1]
    return out


_PERIOD = {}


def period_bits(kind):
    """One full period of the sequence of `kind` (131071 or 255)."""
    if kind not in _PERIOD:
        _PERIOD[kind] = seq_register(kind, kind)
    return _PERIOD[kind]


def randomize(data, kind):
    """Exclusive-OR of the buffer, bit for bit, with the sequence from its first bit (10.3.2):
    one function for both directions. The sequence restarts for every buffer (10.4.3)."""
    seq = period_bits(kind)
    n = len(seq)
    bits = bits_of(data)
    return bytes_of([b ^ seq[i % n] for i, b in enumerate(bits)])


# --------------------------------------------------------------------------------------------
# Configuration (SYNC-002, SYNC-016, SYNC-020)
# --------------------------------------------------------------------------------------------


class Refused(Exception):
    """An input the engine or function rejects: an error kind of the catalogue (03 9) and, where
    it names one, the field name; `needed` for BufferTooSmall."""

    def __init__(self, kind, field=None, needed=None):
        Exception.__init__(self, kind)
        self.kind, self.field, self.needed = kind, field, needed


class Unspecified(Exception):
    """A case the documents leave open: no vector pins it."""


def check_config(cfg):
    """Validates a sync configuration: protocol, frame length, tolerance, and the capacity of the
    instance when the configuration names one (`frame_capacity`). Each kind of fault is tested
    alone, so the order of the checks is not pinned (README, readings the model refuses)."""
    protocol = cfg.get("protocol")
    if protocol not in MAX_FRAME:
        raise Refused("InvalidConfig", "sync_protocol")
    flen = cfg["frame_len"]
    if flen < 1 or flen > MAX_FRAME[protocol]:
        raise Refused("InvalidConfig", "frame_len")
    tol = cfg.get("asm_errors", 0)
    if tol < 0 or tol > MAX_ASM_ERRORS:
        raise Refused("InvalidConfig", "asm_errors")
    cap = cfg.get("frame_capacity")
    if cap is not None and flen > cap:
        raise Refused("CapacityExceeded")


def check_sequence(kind):
    if kind not in (131071, 255):
        raise Refused("InvalidConfig", "randomizer_sequence")


# --------------------------------------------------------------------------------------------
# The ASM attach (SYNC-003)
# --------------------------------------------------------------------------------------------


def attach(cfg, frame, out_len=None):
    check_config(cfg)
    if len(frame) != cfg["frame_len"]:
        raise Refused("LengthMismatch", "frame_len")
    cadu = ASM + bytes(frame)
    if out_len is not None and out_len < len(cadu):
        raise Refused("BufferTooSmall", needed=len(cadu))
    return cadu


# --------------------------------------------------------------------------------------------
# The ASM search engine (SYNC-007 to SYNC-016)
# --------------------------------------------------------------------------------------------


class Engine:
    """Two states, Searching and Collecting. In Searching the window holds the last (up to) 32
    stream bits and `read` counts the bits read since the search began; once 32 are read, every
    position is compared with the ASM, and the first position whose differing bits number at
    most the tolerance starts Collecting. Collecting takes the next 8 x frame_len bits as the
    unit, returns it, and goes back to Searching with an empty window."""

    def __init__(self, cfg):
        check_config(cfg)
        self.cfg = cfg
        self.flen = cfg["frame_len"]
        self.tol = cfg.get("asm_errors", 0)
        self.state = "Searching"
        self.win = []
        self.read = 0
        self.unit = []
        self.skip_cur = 0
        self.err_cur = 0
        self.units = 0
        self.total = 0
        self.tainted = False  # a Reset threw away bits whose accounting no document settles

    def _bit(self, b, actions):
        if self.state == "Searching":
            self.win.append(b)
            if len(self.win) > 32:
                self.win.pop(0)
            self.read += 1
            if self.read >= 32:
                diff = sum(1 for w, a in zip(self.win, ASM_BITS) if w != a)
                if diff <= self.tol:
                    self.state = "Collecting"
                    self.err_cur = diff
                    self.skip_cur = self.read - 32
                    self.unit = []
        else:
            self.unit.append(b)
            if len(self.unit) == 8 * self.flen:
                actions.append({"unit": {"data_hex": hexs(bytes_of(self.unit)),
                                         "errors": self.err_cur,
                                         "skipped_bits": self.skip_cur}})
                self.units += 1
                self.total += self.skip_cur
                self.state = "Searching"
                self.win = []
                self.read = 0
                self.unit = []

    def feed(self, data, out_len=None):
        cap = self.cfg.get("chunk_capacity")
        if cap is not None and len(data) > cap:
            raise Refused("OutOfRange", "chunk_len")
        trial = copy.deepcopy(self)
        actions = []
        for b in bits_of(data):
            trial._bit(b, actions)
        needed = len(actions) * self.flen
        if out_len is not None and out_len < needed:
            raise Refused("BufferTooSmall", needed=needed)
        self.__dict__.update(trial.__dict__)
        return actions

    def reset(self):
        if (self.state == "Searching" and self.read > 0) or self.state == "Collecting":
            self.tainted = True
        self.state = "Searching"
        self.win = []
        self.read = 0
        self.unit = []

    def counter(self, name):
        if name == "units_returned":
            return self.units
        if name == "partial_octets":
            if self.state == "Searching":
                return 0
            if len(self.unit) % 8:
                raise Unspecified("partial octets of a unit that ends inside an octet")
            return len(self.unit) // 8
        if name == "skipped_bits_total":
            if self.tainted or self.state != "Searching" or self.read:
                raise Unspecified("total of skipped bits while bits are pending or after a Reset")
            return self.total
        raise Unspecified("counter " + name)


def replay(cfg, inputs):
    """Runs a list of (at_us, input) through a fresh engine and returns the steps with their
    expected outputs and states."""
    eng = Engine(cfg)
    steps = []
    for at, inp in inputs:
        if "chunk" in inp:
            c = inp["chunk"]
            try:
                out = eng.feed(bytes.fromhex(c["data_hex"]), c.get("out_len"))
            except Refused as e:
                out = [error_output(e)]
        elif "reset" in inp:
            eng.reset()
            out = []
        elif "query" in inp:
            name = inp["query"]["counter"]
            out = [{"counter": {"name": name, "value": eng.counter(name)}}]
        else:
            raise Unspecified("input " + json.dumps(inp))
        steps.append({"at_us": at, "input": inp, "expect": out, "expect_state": eng.state})
    return steps


def error_output(e):
    d = {"error": e.kind}
    if e.field:
        d["field"] = e.field
    if e.needed is not None:
        d["needed"] = e.needed
    return d


def offline_search(stream_bits, flen, tol):
    """A second formulation of the search over a whole stream: the list of
    (errors, skipped_bits, unit_bits) of the units that END inside the stream. It does not use
    the state of `Engine`: it scans an index over the bit string."""
    out = []
    start = 0  # the first bit of the current search
    n = len(stream_bits)
    pos = start + 32  # exclusive end of the window under test
    while pos <= n:
        window = stream_bits[pos - 32:pos]
        diff = sum(a != b for a, b in zip(window, ASM_BITS))
        if diff <= tol:
            end = pos + 8 * flen
            if end > n:
                break
            out.append((diff, pos - 32 - start, stream_bits[pos:end]))
            start = end
            pos = start + 32
        else:
            pos += 1
    return out


# --------------------------------------------------------------------------------------------
# Self-check
# --------------------------------------------------------------------------------------------


def lcg_bytes(n, seed):
    """n deterministic octets (a 32-bit linear congruential generator, high byte)."""
    x = seed & 0xFFFFFFFF
    out = bytearray()
    for _ in range(n):
        x = (x * 1664525 + 1013904223) & 0xFFFFFFFF
        out.append(x >> 24)
    return bytes(out)


def selftest():
    # The marker: the figure and the hexadecimal form agree.
    assert ASM == bytes([0x1A, 0xCF, 0xFC, 0x1D])
    # The 40 bits of 10.4.3 note 2, both sequences, both formulations.
    for kind in (131071, 255):
        assert seq_recurrence(kind, 40) == NOTE_40[kind], kind
        assert seq_register(kind, 40) == NOTE_40[kind], kind
        n = 3 * kind + 11
        assert seq_recurrence(kind, n) == seq_register(kind, n), kind
    assert bytes_of(NOTE_40[131071]).hex().upper() == "1C71B91BA9"
    assert bytes_of(NOTE_40[255]).hex().upper() == "FF480EC09A"
    # Period: the generator state returns to its seed after exactly one period.
    for kind in (131071, 255):
        exps = POLY[kind]
        m = exps[0]
        seed = bits_from_text(SEED_TEXT[kind])
        reg = list(seed)
        for step in range(1, kind + 1):
            fb = reg[m - 1]
            for e in exps[1:]:
                fb ^= reg[m - e - 1]
            reg = [fb] + reg[:m - 1]
            if reg == seed:
                assert step == kind, (kind, step)
                break
        else:
            raise AssertionError("no period within %d" % kind)
        seq = seq_recurrence(kind, 3 * kind)
        assert seq[:kind] == seq[kind:2 * kind] == seq[2 * kind:3 * kind]
        assert period_bits(kind) == seq[:kind]
        # no shorter period of the bit sequence either
        for d in range(1, kind):
            if kind % d == 0:
                assert seq[:kind - d] != seq[d:kind], (kind, d)
    # Randomizing twice restores, for any length, and an empty buffer is empty.
    for kind in (131071, 255):
        for n in (0, 1, 2, 31, 255, 256, 16384):
            data = lcg_bytes(n, 7 + n)
            assert randomize(randomize(data, kind), kind) == data
    # The engine, on random streams: one chunk, bit-wise and random splits, and the offline
    # formulation agree.
    for trial in range(300):
        flen = 1 + trial % 5
        tol = trial % 16
        junk = lcg_bytes(40, 1000 + trial)
        bits = bits_of(junk)[:trial % 37]
        errs = [(trial * 7 + i * 11) % 32 for i in range(trial % 3)]
        asm = list(ASM_BITS)
        for p in errs:
            asm[p] ^= 1
        bits += asm + bits_of(lcg_bytes(flen, 5 + trial)) + bits_of(lcg_bytes(9, 77 + trial))
        bits += [0] * (-len(bits) % 8)
        data = bytes_of(bits)
        cfg = {"protocol": "tm", "frame_len": flen, "asm_errors": tol}
        whole = Engine(cfg).feed(data)
        ref = [{"unit": {"data_hex": hexs(bytes_of(u)), "errors": e, "skipped_bits": s}}
               for e, s, u in offline_search(bits, flen, tol)]
        assert whole == ref, (trial, whole, ref)
        eng = Engine(cfg)
        got = []
        cut = 0
        for i in range(len(data)):
            if (i * 5 + trial) % 3 != 0 or i == len(data) - 1:
                got += eng.feed(data[cut:i + 1])
                cut = i + 1
        assert got == whole, trial
        eng = Engine(cfg)
        assert sum((eng.feed(data[i:i + 1]) for i in range(len(data))), []) == whole
    # A hand case: the CADU with its marker intact at offset 0 and 3 bits.
    eng = Engine({"protocol": "tm", "frame_len": 2, "asm_errors": 0})
    assert eng.feed(ASM + b"\xAB\xCD") == [{"unit": {"data_hex": "ABCD", "errors": 0,
                                                       "skipped_bits": 0}}]


# --------------------------------------------------------------------------------------------
# Vector builders
# --------------------------------------------------------------------------------------------

REQ_NOTE = "131.0-B-6 9.1.2 Table 9-1, 9.2, 9.3.1 Figure 9-1, 10.2 to 10.4, 11.1 to 11.4"


def junk_bits(n, tag=0):
    """n bits of a fixed junk pattern (alternating, shifted by the tag): it never holds the marker
    and is the same in every run."""
    return [((i + tag) // 1) % 2 for i in range(n)]


def frame_octets(n, tag):
    """A frame of n octets of fixed pseudo-random content (the content is opaque to sync)."""
    return lcg_bytes(n, 0x5A5A0000 + 977 * tag + n)


def cadu_stream_bits(frame, kind=131071, asm_flips=()):
    """The bits of a CADU as the sender writes it: the frame randomized, then the ASM attached
    (the marker is not randomized, 10.3.4 note 1)."""
    asm = list(ASM_BITS)
    for p in asm_flips:
        asm[p] ^= 1
    return asm + bits_of(randomize(frame, kind) if kind else frame)


def to_chunk(bits):
    """Pads the bits with zeros to a whole number of octets."""
    return bytes_of(bits + [0] * (-len(bits) % 8))


def flip_positions(k):
    """The first k of a fixed order of distinct bit positions of the marker (11 is odd, so the
    multiples of 11 mod 32 are all different); a longer list contains the shorter."""
    return [(5 + 11 * i) % 32 for i in range(k)]


class Codec:
    def __init__(self, cid, reqs, desc, op, cfg, inp, params=None):
        self.cid, self.reqs, self.desc, self.op = cid, reqs, desc, op
        self.cfg, self.inp, self.params = cfg, inp, params


def run_codec(op, cfg, inp, params):
    """The expected outcome of a codec case."""
    try:
        if op == "attach":
            out = attach(cfg, inp, (params or {}).get("out_len"))
            return {"ok": True, "cadu_hex": hexs(out)}
        if op == "randomize":
            check_sequence(cfg["randomizer_sequence"])
            return {"ok": True, "output_hex": hexs(randomize(inp, cfg["randomizer_sequence"]))}
        if op == "config":
            check_config(cfg)
            return {"ok": True}
    except Refused as e:
        d = {"ok": False, "error": e.kind}
        if e.field:
            d["field"] = FIELD[e.field]
        if e.needed is not None:
            d["needed"] = e.needed
        return d
    raise Unspecified("operation " + op)


def build_codec(c):
    d = {"id": c.cid, "description": c.desc, "requirements": c.reqs, "operation": c.op}
    if c.cfg is not None:
        d["config"] = c.cfg
    if c.inp is not None:
        d["input_hex"] = hexs(c.inp)
    if c.params:
        d["params"] = c.params
    d["expect"] = run_codec(c.op, c.cfg, c.inp, c.params)
    return d


# ---- attach ----

def cadu_cases(protocol):
    lens = {"tm": [1, 6, 48, 1115, 2048],
            "aos": [1, 6, 64, 1115, 2048, 65536],
            "uslp": [1, 7, 100, 4096, 65535]}[protocol]
    reqs = ["SYNC-001", "SYNC-002", "SYNC-003"]
    out = []
    for i, n in enumerate(lens):
        cfg = {"protocol": protocol, "frame_len": n}
        out.append(Codec("sync-cadu-%s-%d" % (protocol, n), reqs,
                         "CADU of a %d-octet %s frame: the marker 1ACFFC1D, then the frame "
                         "octets unchanged (131.0 9.1.2, 9.2.1; 11.3, 11.4 for the length)."
                         % (n, protocol.upper() if protocol != "uslp" else "fixed-length USLP"),
                         "attach", cfg, frame_octets(n, i + 1)))
    # Frame contents that a marker search could trip on: the marker itself, zeros, ones.
    cfg = {"protocol": protocol, "frame_len": 8}
    out.append(Codec("sync-cadu-%s-frame-holds-asm" % protocol, reqs,
                     "A frame that holds the marker twice is attached as given: the CADU is "
                     "the marker, then those octets (131.0 9.2.1).", "attach", cfg, ASM + ASM))
    out.append(Codec("sync-cadu-%s-frame-zeros" % protocol, reqs,
                     "An all-zero frame is not changed by the attach (the randomizer is a "
                     "separate step, 10.3.4 note 1).", "attach", cfg, bytes(8)))
    out.append(Codec("sync-cadu-%s-frame-ones" % protocol, reqs,
                     "An all-ones frame is not changed by the attach.", "attach", cfg,
                     b"\xFF" * 8))
    return out


def cadu_error_cases():
    reqs = ["SYNC-003"]
    out = []
    for proto in ("tm", "aos", "uslp"):
        cfg = {"protocol": proto, "frame_len": 12}
        out.append(Codec("sync-cadu-%s-short-frame" % proto, reqs,
                         "A frame one octet shorter than the configured length fails with "
                         "LengthMismatch naming frame_len (nothing is written).",
                         "attach", cfg, frame_octets(11, 3)))
        out.append(Codec("sync-cadu-%s-long-frame" % proto, reqs,
                         "A frame one octet longer than the configured length fails with "
                         "LengthMismatch naming frame_len.", "attach", cfg, frame_octets(13, 3)))
    cfg = {"protocol": "tm", "frame_len": 12}
    out.append(Codec("sync-cadu-empty-frame", reqs,
                     "An empty frame fails with LengthMismatch naming frame_len.", "attach",
                     cfg, b""))
    fr = frame_octets(12, 4)
    out.append(Codec("sync-cadu-out-exact", reqs,
                     "An output area of exactly 4 + frame length octets (16) is enough.",
                     "attach", cfg, fr, {"out_len": 16}))
    out.append(Codec("sync-cadu-out-one-short", reqs,
                     "An output area one octet short fails with BufferTooSmall and needed = 16 "
                     "(4 + frame length).", "attach", cfg, fr, {"out_len": 15}))
    out.append(Codec("sync-cadu-out-zero", reqs,
                     "An output area of 0 octets fails with BufferTooSmall, needed = 16.",
                     "attach", cfg, fr, {"out_len": 0}))
    out.append(Codec("sync-cadu-out-marker-only", reqs,
                     "An output area that holds the marker (4 octets) but not the frame fails "
                     "with BufferTooSmall, needed = 16.", "attach", cfg, fr, {"out_len": 4}))
    return out


# ---- randomizer ----

def rand_cases(kind):
    reqs = ["SYNC-004", "SYNC-005" if kind == 131071 else "SYNC-020"]
    cfg = {"randomizer_sequence": kind}
    tag = "%d" % kind
    out = []
    out.append(Codec("sync-rand-%s-first-40-bits" % tag, reqs,
                     "Five zero octets give the first 40 bits of the sequence, the bits of "
                     "the note of 131.0 10.4.3 (note 2).", "randomize", cfg, bytes(5)))
    out.append(Codec("sync-rand-%s-empty" % tag, reqs,
                     "An empty buffer is accepted and stays empty (SYNC-004).", "randomize",
                     cfg, b""))
    for n in (1, 2, 3, 4, 7, 8, 9, 16, 33, 64, 100):
        out.append(Codec("sync-rand-%s-zeros-%d" % (tag, n), reqs,
                         "%d zero octets give the first %d bits of the sequence, from the "
                         "first bit of the buffer (10.3.2)." % (n, 8 * n),
                         "randomize", cfg, bytes(n)))
    data = lcg_bytes(40, 12)
    out.append(Codec("sync-rand-%s-data" % tag, reqs,
                     "Data octets are exclusive-ORed bit for bit with the sequence.",
                     "randomize", cfg, data))
    out.append(Codec("sync-rand-%s-involution" % tag, reqs,
                     "Applying the function to the output of sync-rand-%s-data restores the "
                     "data: one function for both directions (10.3.3)." % tag,
                     "randomize", cfg, randomize(data, kind)))
    out.append(Codec("sync-rand-%s-ones" % tag, reqs,
                     "An all-ones buffer gives the complement of the sequence.", "randomize",
                     cfg, b"\xFF" * 8))
    if kind == 131071:
        # 131071 bits = 16383 octets and 7 bits: a period ends inside octet 16383.
        for n, why in ((16383, "ends 7 bits before the end of the first period"),
                       (16384, "ends one bit after the first period; the last octet holds the "
                               "last bit of the period and the first 7 bits of the next"),
                       (16385, "is one octet past the first period"),
                       (32768, "holds two full periods and 2 bits, so the sequence repeats at a "
                               "bit that is not an octet boundary (10.4.3)")):
            out.append(Codec("sync-rand-131071-zeros-%d" % n, reqs,
                             "%d zero octets: the buffer %s." % (n, why), "randomize", cfg,
                             bytes(n)))
        out.append(Codec("sync-rand-131071-zeros-65536", reqs,
                         "A 65536-octet unit of zeros: 524288 bits, four periods and 4 bits, "
                         "the largest frame of AOS and USLP (11.3).", "randomize", cfg,
                         bytes(65536)))
        out.append(Codec("sync-rand-131071-data-20000", reqs,
                         "20000 data octets, across the end of the first period.", "randomize",
                         cfg, lcg_bytes(20000, 99)))
    else:
        for n, why in ((31, "ends one bit before the end of the first period (255 bits = 31 "
                            "octets and 7 bits)"),
                       (32, "ends one bit after the first period"),
                       (255, "is exactly 8 periods"),
                       (256, "is 8 periods and 8 bits"),
                       (1000, "is 31 periods and 115 bits")):
            out.append(Codec("sync-rand-255-zeros-%d" % n, reqs,
                             "%d zero octets: the buffer %s." % (n, why), "randomize", cfg,
                             bytes(n)))
        out.append(Codec("sync-rand-255-zeros-2048", reqs,
                         "A 2048-octet unit of zeros (the TM maximum): 64 periods and 64 "
                         "bits.", "randomize", cfg, bytes(2048)))
    return out


# ---- configuration ----

def config_cases():
    out = []
    ok = ["SYNC-002", "SYNC-016"]
    for proto, n in (("tm", 1), ("tm", 2048), ("aos", 1), ("aos", 65536), ("uslp", 1),
                     ("uslp", 65536)):
        out.append(Codec("sync-config-%s-frame-%d" % (proto, n), ok,
                         "%s with a frame length of %d octets is accepted (131.0 11.3, 11.4)."
                         % (proto, n), "config",
                         {"protocol": proto, "frame_len": n, "asm_errors": 0}, None))
    for proto, n in (("tm", 0), ("tm", 2049), ("aos", 0), ("aos", 65537), ("uslp", 0),
                     ("uslp", 65537)):
        out.append(Codec("sync-config-%s-frame-%d" % (proto, n), ["SYNC-002"],
                         "%s with a frame length of %d octets is rejected with InvalidConfig "
                         "naming frame_len (0, or above the maximum of the protocol)."
                         % (proto, n), "config",
                         {"protocol": proto, "frame_len": n, "asm_errors": 0}, None))
    for t in (0, 1, 7, 15):
        out.append(Codec("sync-config-asm-errors-%d" % t, ["SYNC-016"],
                         "A tolerance of %d is accepted (0 to 15, OI-80)." % t, "config",
                         {"protocol": "tm", "frame_len": 48, "asm_errors": t}, None))
    for t in (16, 17, 32, 255):
        out.append(Codec("sync-config-asm-errors-%d" % t, ["SYNC-016"],
                         "A tolerance of %d is rejected with InvalidConfig naming asm_errors."
                         % t, "config",
                         {"protocol": "tm", "frame_len": 48, "asm_errors": t}, None))
    out.append(Codec("sync-config-capacity-ok", ["SYNC-002"],
                     "A frame length equal to the capacity of the instance is accepted "
                     "(frame_capacity names the FRAME capacity the instance has).", "config",
                     {"protocol": "tm", "frame_len": 64, "asm_errors": 0, "frame_capacity": 64},
                     None))
    out.append(Codec("sync-config-capacity-exceeded", ["SYNC-002"],
                     "A valid frame length above the capacity of the instance is rejected "
                     "with CapacityExceeded.", "config",
                     {"protocol": "tm", "frame_len": 65, "asm_errors": 0, "frame_capacity": 64},
                     None))
    out.append(Codec("sync-config-capacity-exceeded-aos", ["SYNC-002"],
                     "The same for AOS: 1024 octets on an instance of capacity 1023.", "config",
                     {"protocol": "aos", "frame_len": 1024, "asm_errors": 0,
                      "frame_capacity": 1023}, None))
    out.append(Codec("sync-config-protocol-invalid", ["SYNC-002"],
                     "A frame protocol that is none of TM, AOS, USLP (a C ABI input; the Rust "
                     "type cannot hold one) is rejected with InvalidConfig naming "
                     "sync_protocol.", "config",
                     {"protocol": "none", "frame_len": 48, "asm_errors": 0}, None))
    for v in (0, 1, 7, 255, 131072):
        out.append(Codec("sync-rand-sequence-invalid-%d" % v, ["SYNC-020"],
                         "A randomizer sequence choice that is neither 131071 nor 255 (a C ABI "
                         "input) is rejected with InvalidConfig naming randomizer_sequence.",
                         "randomize", {"randomizer_sequence": v}, bytes(4)))
    return out


# ---- scenarios ----

class Scn:
    def __init__(self, sid, reqs, desc, cfg=None):
        self.sid, self.reqs, self.desc, self.cfg = sid, reqs, desc, cfg
        self.inputs = []
        self.intent = None  # the list of (errors, skipped_bits) the scenario is built to show

    def chunk(self, data, out_len=None):
        c = {"data_hex": hexs(data)}
        if out_len is not None:
            c["out_len"] = out_len
        self.inputs.append({"chunk": c})
        return self

    def reset(self):
        self.inputs.append({"reset": {}})
        return self

    def query(self, name):
        self.inputs.append({"query": {"counter": name}})
        return self

    def expect_units(self, units):
        self.intent = units
        return self


def cfg_of(protocol, flen, tol=0, **extra):
    d = {"protocol": protocol, "frame_len": flen, "asm_errors": tol}
    d.update(extra)
    return d


def units_in(steps):
    return [(a["unit"]["errors"], a["unit"]["skipped_bits"])
            for st in steps for a in st["expect"] if "unit" in a]


def offsets_scenarios(protocol, flen, skips):
    reqs = ["SYNC-007", "SYNC-009", "SYNC-011", "SYNC-017"]
    out = []
    for n in skips:
        fr = frame_octets(flen, n + 1)
        s = Scn("sync-search-%s-skip-%03d" % (protocol, n), reqs,
                "%d junk bits, then the CADU of a %d-octet %s frame (frame randomized with the "
                "131071-bit sequence, then the marker): the engine finds the marker at bit "
                "offset %d of its octet, after %d octets and %d bits, and returns the unit "
                "shifted to an octet boundary with skipped_bits %d."
                % (n, flen, protocol, n % 8, n // 8, n % 8, n))
        s.chunk(to_chunk(junk_bits(n) + cadu_stream_bits(fr)))
        s.expect_units([(0, n)])
        s.expected_unit = hexs(randomize(fr, 131071))
        out.append(s)
    return out


def errors_scenarios():
    out = []
    flen = 6
    for t in range(16):
        s = Scn("sync-search-tol-%02d" % t, ["SYNC-007", "SYNC-011", "SYNC-016"],
                "Tolerance %d. A marker with %d flipped bits (one more than the tolerance) alone "
                "gives no unit; after a Reset, a marker with %d flipped bits returns its unit "
                "with errors %d and skipped_bits 0 (the flipped positions are %s)."
                % (t, t + 1, t, t, flip_positions(t)),
                cfg_of("tm", flen, t))
        bad = to_chunk(cadu_stream_bits(b"", 0, flip_positions(t + 1)))
        s.chunk(bad).reset()
        fr = frame_octets(flen, 40 + t)
        s.chunk(to_chunk(cadu_stream_bits(fr, 131071, flip_positions(t))))
        s.expect_units([(t, 0)])
        out.append(s)
    # One flipped bit at every position, tolerance 1.
    s = Scn("sync-search-single-error-every-position", ["SYNC-007", "SYNC-011"],
            "Tolerance 1: 32 CADUs back to back (one per step), the i-th with bit i of the "
            "marker flipped, are each found, with errors 1 and skipped_bits 0.",
            cfg_of("aos", 2, 1))
    exp = []
    for i in range(32):
        s.chunk(to_chunk(cadu_stream_bits(frame_octets(2, 100 + i), 131071, [i])))
        exp.append((1, 0))
    s.expect_units(exp)
    out.append(s)
    s = Scn("sync-search-two-errors-tolerance-one", ["SYNC-007"],
            "Tolerance 1: a marker with two flipped bits alone gives no unit, at any pair "
            "of positions tried (each pair is followed by a Reset).", cfg_of("aos", 2, 1))
    for i in range(0, 32, 5):
        s.chunk(to_chunk(cadu_stream_bits(b"", 0, [i, (i + 13) % 32]))).reset()
    s.expect_units([])
    out.append(s)
    s = Scn("sync-search-exact-marker-high-tolerance", ["SYNC-007", "SYNC-011"],
            "Tolerance 15 and an exact marker at the start: errors 0, skipped_bits 0.",
            cfg_of("uslp", 4, 15))
    s.chunk(to_chunk(cadu_stream_bits(frame_octets(4, 9))))
    s.expect_units([(0, 0)])
    out.append(s)
    s = Scn("sync-search-tolerance-counts-differing-bits", ["SYNC-007"],
            "Tolerance 3: the marker with 3 bits flipped at the start of the stream is found "
            "(errors 3); the marker with 4 flipped is not (no unit, Searching); the "
            "differing bits are counted over all 32 positions, not only a leading run.",
            cfg_of("tm", 3, 3))
    s.chunk(to_chunk(cadu_stream_bits(frame_octets(3, 8), 131071, [0, 15, 31])))
    s.chunk(to_chunk(cadu_stream_bits(b"", 0, [0, 15, 31, 16]))).reset()
    s.expect_units([(3, 0)])
    out.append(s)
    return out


def hamming(a, b):
    return sum(x != y for x, y in zip(a, b))


def first_match_scenarios():
    reqs = ["SYNC-007", "SYNC-008", "SYNC-011"]
    out = []
    s = Scn("sync-first-ones-tolerance-13", reqs,
            "Tolerance 13: the first 32 bits, all ones, differ from the marker in the 13 bits "
            "where the marker is 0, so that window matches (errors 13, skipped_bits 0) and "
            "its unit is the next 2 octets, even though the real marker follows (the first "
            "position wins, OI-80).", cfg_of("tm", 2, 13))
    s.chunk(b"\xFF" * 4 + ASM[:2])
    s.expect_units([(13, 0)])
    out.append(s)
    s = Scn("sync-first-ones-tolerance-12", reqs,
            "Tolerance 12: the same 32 bits do not match (13 differ), so there is no unit.",
            cfg_of("tm", 2, 12))
    s.chunk(b"\xFF" * 4)
    s.expect_units([])
    out.append(s)
    for shift in (1, 2, 3, 5):
        for variant in (0, 1):
            # variant 0: the tail of the marker as junk; variant 1: alternating bits
            junk = [(i + 1) % 2 for i in range(shift)] if variant else ASM_BITS[32 - shift:]
            stream = junk + ASM_BITS
            # the windows that end before the exact marker: shift windows
            ds = [hamming(stream[j:j + 32], ASM_BITS) for j in range(shift)]
            best = min(ds)
            assert best >= 1
            first = ds.index(best)
            fr = frame_octets(3, shift * 2 + variant)
            tail = bits_of(randomize(fr, 131071))
            sid = "sync-first-near-miss-shift%d-v%d" % (shift, variant)
            s = Scn(sid, reqs,
                    "%d junk bits, then an exact marker and a 3-octet frame. With tolerance %d "
                    "the window that starts at bit %d, which differs from the marker in %d "
                    "bits, is the first within the tolerance, so it wins over the exact "
                    "marker %d bits later: errors %d, skipped_bits %d, and the unit is read "
                    "from that earlier position." % (shift, best, first, best, shift - first,
                                                     best, first), cfg_of("aos", 3, best))
            s.chunk(to_chunk(stream + tail))
            s.expect_units([(best, first)])
            out.append(s)
            s = Scn(sid + "-below", reqs,
                    "The same stream with tolerance %d: no earlier window is within the "
                    "tolerance, so the exact marker %d bits in is found: errors 0, "
                    "skipped_bits %d." % (best - 1, shift, shift), cfg_of("aos", 3, best - 1))
            s.chunk(to_chunk(stream + tail))
            s.expect_units([(0, shift)])
            out.append(s)
    return out


def chunk_scenarios():
    out = []
    reqs = ["SYNC-010", "SYNC-009", "SYNC-011"]
    flen = 5
    for o in (0, 1, 3, 7):
        fr = frame_octets(flen, 20 + o)
        data = to_chunk(junk_bits(o) + cadu_stream_bits(fr))
        n = len(data)
        for k in range(1, n):
            s = Scn("sync-chunk-o%d-k%02d" % (o, k), reqs,
                    "%d junk bits, then a CADU (%d octets in all); the stream is cut into "
                    "chunks of %d and %d octets, so the marker or the unit crosses the "
                    "boundary: the same unit as in one chunk, with skipped_bits %d."
                    % (o, n, k, n - k, o), cfg_of("tm", flen))
            s.chunk(data[:k]).chunk(data[k:])
            s.expect_units([(0, o)])
            out.append(s)
    for o in range(8):
        fr = frame_octets(flen, 30 + o)
        data = to_chunk(junk_bits(o) + cadu_stream_bits(fr))
        s = Scn("sync-chunk-bytewise-o%d" % o, reqs,
                "%d junk bits and a CADU fed one octet per step: no action until the octet "
                "that completes the unit, and the same unit as in one chunk." % o,
                cfg_of("aos", flen))
        for i in range(len(data)):
            s.chunk(data[i:i + 1])
        s.expect_units([(0, o)])
        out.append(s)
    s = Scn("sync-chunk-empty", ["SYNC-010"],
            "A chunk of 0 octets changes nothing and returns no action, with the marker half "
            "read and with the unit half collected.", cfg_of("tm", 4))
    fr = frame_octets(4, 55)
    stream = to_chunk(cadu_stream_bits(fr))
    s.chunk(b"")
    s.chunk(stream[:2]).chunk(b"").chunk(stream[2:6]).chunk(b"").chunk(stream[6:])
    s.expect_units([(0, 0)])
    out.append(s)
    s = Scn("sync-chunk-split-asm-three-chunks", reqs,
            "The marker is spread over three chunks (1, 1, and 3 octets) at bit offset 5, "
            "then the unit follows in a fourth.", cfg_of("uslp", 3))
    fr = frame_octets(3, 56)
    stream = to_chunk(junk_bits(5) + cadu_stream_bits(fr))
    s.chunk(stream[:1]).chunk(stream[1:2]).chunk(stream[2:5]).chunk(stream[5:])
    s.expect_units([(0, 5)])
    out.append(s)
    s = Scn("sync-chunk-unit-state-queries", ["SYNC-010", "SYNC-012", "SYNC-015"],
            "A CADU at octet alignment fed in three chunks, with the state and the octets of "
            "the partial unit after each: the state is Collecting after the marker, the "
            "partial unit grows by the octets collected, and the counter of units rises when "
            "the unit completes.", cfg_of("tm", 10))
    fr = frame_octets(10, 57)
    stream = to_chunk(cadu_stream_bits(fr))
    s.chunk(stream[:4]).query("partial_octets")
    s.chunk(stream[4:9]).query("partial_octets").query("units_returned")
    s.chunk(stream[9:]).query("partial_octets").query("units_returned")
    s.expect_units([(0, 0)])
    out.append(s)
    # Many units in one chunk: the action bound, ceil(c / (frame_len + 4)).
    s = Scn("sync-chunk-many-units-bound", ["SYNC-011", "SYNC-013", "SYNC-014"],
            "A chunk of 20 octets that holds four 1-octet-frame CADUs back to back: four "
            "actions, in stream order, each with skipped_bits 0 (the bound ceil(20 / 5) = 4 "
            "is reached).", cfg_of("tm", 1))
    data = b"".join(to_chunk(cadu_stream_bits(frame_octets(1, 70 + i))) for i in range(4))
    assert len(data) == 20
    s.chunk(data)
    s.expect_units([(0, 0)] * 4)
    out.append(s)
    s = Scn("sync-chunk-many-units-misaligned", ["SYNC-010", "SYNC-011", "SYNC-013"],
            "Five 2-octet-frame CADUs, each after a different number of junk bits (0 to 4), in "
            "chunks cut at octet 7, 15 and 25: the units come in stream order, with the "
            "skipped_bits of each measured from the end of the one before.", cfg_of("aos", 2))
    bits = []
    exp = []
    for i in range(5):
        bits += junk_bits(i, i) + cadu_stream_bits(frame_octets(2, 80 + i))
        exp.append((0, i))
    data = to_chunk(bits)
    s.chunk(data[:7]).chunk(data[7:15]).chunk(data[15:25]).chunk(data[25:])
    s.expect_units(exp)
    out.append(s)
    return out


def unit_scenarios():
    out = []
    s = Scn("sync-units-back-to-back", ["SYNC-009", "SYNC-011", "SYNC-015"],
            "Three CADUs back to back at octet alignment: skipped_bits 0 each, and the counters "
            "after the third: units_returned 3, skipped_bits_total 0.", cfg_of("tm", 16))
    s.chunk(b"".join(to_chunk(cadu_stream_bits(frame_octets(16, 90 + i))) for i in range(3)))
    s.query("units_returned").query("skipped_bits_total")
    s.expect_units([(0, 0)] * 3)
    out.append(s)
    s = Scn("sync-units-gaps", ["SYNC-009", "SYNC-011", "SYNC-015"],
            "CADUs separated by 0, 13, 40 and 3 junk bits: skipped_bits counts the bits "
            "between the end of the previous unit and the first marker bit; "
            "skipped_bits_total is their sum, 56.", cfg_of("aos", 12))
    bits = []
    exp = []
    for i, g in enumerate((0, 13, 40, 3)):
        bits += junk_bits(g, i) + cadu_stream_bits(frame_octets(12, 91 + i))
        exp.append((0, g))
    s.chunk(to_chunk(bits))
    s.query("units_returned").query("skipped_bits_total")
    s.expect_units(exp)
    out.append(s)
    s = Scn("sync-units-asm-inside-unit", ["SYNC-009"],
            "A frame that holds the marker (at octet 3; at bit 3 in a second unit) is returned whole: the bits of a unit are not searched. Only two units are "
            "returned, with nothing skipped between them.", cfg_of("uslp", 12))
    f1 = b"\x00" * 3 + ASM + b"\x11" * 5
    f2 = bytes_of([0] * 3 + list(ASM_BITS) + [1] * 61)  # the marker at bit 3 of a 12-octet unit
    s.chunk(to_chunk(cadu_stream_bits(f1, 0) + cadu_stream_bits(f2, 0)))
    s.expect_units([(0, 0), (0, 0)])
    out.append(s)
    s = Scn("sync-units-tail-of-unit-not-reused", ["SYNC-007", "SYNC-009"],
            "A unit that ends with the first three octets of the marker (1A CF FC) is followed "
            "directly by the octet 1D: the window is empty after a unit, so these 32 bits "
            "are not a marker and no second unit starts. The CADU after 9 junk bits "
            "is then found, with skipped_bits 8 + 9.", cfg_of("tm", 6))
    f1 = b"\x01\x02\x03" + ASM[:3]
    bits = (cadu_stream_bits(f1, 0) + bits_of(b"\x1D") + junk_bits(9)
            + cadu_stream_bits(frame_octets(6, 92)))
    s.chunk(to_chunk(bits))
    s.expect_units([(0, 0), (0, 17)])
    out.append(s)
    s = Scn("sync-units-partial-at-end", ["SYNC-011", "SYNC-012", "SYNC-015"],
            "A chunk that ends inside the unit returns no action and leaves the engine "
            "Collecting; the rest of the unit in the next chunk completes it.",
            cfg_of("tm", 20))
    fr = frame_octets(20, 93)
    st = to_chunk(cadu_stream_bits(fr))
    s.chunk(st[:14]).query("partial_octets")
    s.chunk(st[14:])
    s.expect_units([(0, 0)])
    out.append(s)
    s = Scn("sync-units-reset-partial", ["SYNC-015"],
            "A Reset in the middle of a unit discards the partial unit: no action, the state "
            "is Searching, the octets that follow are searched as new stream (the rest of the "
            "unit, 80 bits, holds no marker), and the next CADU is found with skipped_bits "
            "80; the units already returned stay counted.", cfg_of("tm", 20))
    a = to_chunk(cadu_stream_bits(frame_octets(20, 94)))
    b = to_chunk(cadu_stream_bits(frame_octets(20, 95)))
    c = to_chunk(cadu_stream_bits(frame_octets(20, 96)))
    s.chunk(a).query("units_returned")
    s.chunk(b[:14]).reset().query("partial_octets")
    s.chunk(b[14:] + c).query("units_returned")
    s.expect_units([(0, 0), (0, 80)])
    out.append(s)
    s = Scn("sync-units-reset-window", ["SYNC-007", "SYNC-015"],
            "A Reset empties the window: the first 24 bits of the marker, a Reset, then the "
            "last 8 bits do not make a marker. A whole marker after that is found with "
            "skipped_bits 8: the 8 bits read after the Reset (the count restarts at the Reset).", cfg_of("aos", 2))
    s.chunk(ASM[:3]).reset().chunk(ASM[3:])
    s.chunk(to_chunk(cadu_stream_bits(frame_octets(2, 97))))
    s.expect_units([(0, 8)])
    out.append(s)
    s = Scn("sync-units-reset-keeps-counters", ["SYNC-015"],
            "A Reset keeps units_returned: two units, a Reset, a third unit; the counter reads "
            "2 before the Reset, 2 after it, and 3 after the third unit.", cfg_of("tm", 3))
    cad = lambda i: to_chunk(cadu_stream_bits(frame_octets(3, 98 + i)))
    s.chunk(cad(0) + cad(1)).query("units_returned")
    s.reset().query("units_returned")
    s.chunk(cad(2)).query("units_returned")
    s.expect_units([(0, 0)] * 3)
    out.append(s)
    s = Scn("sync-units-marker-needs-32-bits", ["SYNC-007"],
            "Tolerance 13: 24 bits of ones read since the start are not a window (a match needs "
            "32 bits): no unit, the engine stays Searching; the fourth octet of ones completes "
            "32 bits, which differ from the marker in 13 bits, and the engine starts "
            "Collecting.", cfg_of("tm", 1, 13))
    s.chunk(b"\xFF" * 3).chunk(b"\xFF")
    out.append(s)
    s = Scn("sync-units-length-1", ["SYNC-009", "SYNC-011"],
            "A 1-octet frame length: the unit has exactly one octet.", cfg_of("tm", 1))
    s.chunk(to_chunk(cadu_stream_bits(frame_octets(1, 120))))
    s.expect_units([(0, 0)])
    out.append(s)
    for flen in (2, 5, 64):
        s = Scn("sync-units-length-%d" % flen, ["SYNC-009", "SYNC-011"],
                "A %d-octet frame length: the unit has exactly %d octets, found after 2 octets "
                "and 6 bits of junk, in chunks of 17 octets." % (flen, flen),
                cfg_of("uslp", flen))
        data = to_chunk(junk_bits(22) + cadu_stream_bits(frame_octets(flen, 121 + flen)))
        for i in range(0, len(data), 17):
            s.chunk(data[i:i + 17])
        s.expect_units([(0, 22)])
        out.append(s)
    return out


def bound_scenarios():
    out = []
    reqs = ["SYNC-014", "SYNC-011"]
    s = Scn("sync-bound-buffer-too-small", reqs,
            "A chunk that completes two 3-octet units needs 6 octets of output: an area of 5 "
            "fails with BufferTooSmall, needed 6, and changes nothing (the state stays "
            "Searching, no unit counted); the same chunk with an area of 6 returns both units.",
            cfg_of("tm", 3))
    data = to_chunk(cadu_stream_bits(frame_octets(3, 130)) + cadu_stream_bits(frame_octets(3, 131)))
    s.chunk(data, 5).query("units_returned").chunk(data, 6).query("units_returned")
    s.expect_units([(0, 0), (0, 0)])
    out.append(s)
    s = Scn("sync-bound-needed-counts-completed-units", reqs,
            "Only the units that the chunk completes count: a chunk that completes one 4-octet "
            "unit and starts a second needs 4 octets; an area of 3 fails with needed 4, an "
            "area of 4 succeeds and the second unit is still partial (Collecting).",
            cfg_of("aos", 4))
    cad = to_chunk(cadu_stream_bits(frame_octets(4, 132)))
    nxt = to_chunk(cadu_stream_bits(frame_octets(4, 133)))
    data = cad + nxt[:6]
    s.chunk(data, 3).chunk(data, 4).query("partial_octets")
    s.expect_units([(0, 0)])
    out.append(s)
    s = Scn("sync-bound-no-unit-no-area", reqs,
            "A chunk that completes no unit needs no output area: an area of 0 is enough.",
            cfg_of("tm", 8))
    s.chunk(to_chunk(cadu_stream_bits(frame_octets(8, 134)))[:9], 0)
    out.append(s)
    s = Scn("sync-bound-error-then-same-chunk", reqs,
            "After a BufferTooSmall error the state is exactly as before: the half-read marker "
            "of the first chunk is still there, so the second chunk, refused once for want of "
            "output, then accepted, gives the same unit as an uninterrupted run.",
            cfg_of("tm", 2))
    st = to_chunk(junk_bits(3) + cadu_stream_bits(frame_octets(2, 135)))
    s.chunk(st[:3]).chunk(st[3:], 1).chunk(st[3:], 2)
    s.expect_units([(0, 3)])
    out.append(s)
    return out


def capacity_scenarios():
    out = []
    s = Scn("sync-capacity-chunk-too-large", ["SYNC-014"],
            "On an instance whose CHUNK capacity is 16 (chunk_capacity): a chunk of 17 octets "
            "fails with OutOfRange naming chunk_len and changes nothing; the first 16 octets "
            "of it are accepted, and the 17th octet then completes the unit as if the chunk "
            "had never been refused.",
            cfg_of("tm", 12, 0, chunk_capacity=16))
    st = to_chunk(junk_bits(8) + cadu_stream_bits(frame_octets(12, 140)))
    assert len(st) == 17
    s.chunk(st).chunk(st[:16]).chunk(st[16:])
    s.expect_units([(0, 8)])
    out.append(s)
    return out


# ---- the files ----

NOTE = ("Hand-derived from %s. Model test-vectors/models/sync.py, written blind (TV-023) by a "
        "Claude agent from the Blue Books and docs/ only, without reading any implementation; "
        "for format only it read the existing models (cop1.py, link_recv.py) and the vector "
        "schemas. The inputs were chosen by hand; the expected values are the model's, whose "
        "sequences it checks against the 40-bit notes of 131.0-B-6 10.4.3 (note 2) and whose "
        "search engine it checks against a second, offline formulation. Where the documents "
        "are silent the model refuses the case (README of test-vectors/sync/). The file is "
        "written by the model (--write) and never edited by hand.")

CODEC_FILES = [
    ("cadu_tm.json", "ASM attach for TM frames", lambda: cadu_cases("tm"),
     "131.0-B-6 9.1.2, 9.2.1, 9.3.1 Figure 9-1, 11.3 and 132.0-B-3 2.4.1"),
    ("cadu_aos.json", "ASM attach for AOS frames", lambda: cadu_cases("aos"),
     "131.0-B-6 9.1.2, 9.2.1, 9.3.1 Figure 9-1, 11.3 and 732.0-B-5 2.4.1"),
    ("cadu_uslp.json", "ASM attach for fixed-length USLP frames", lambda: cadu_cases("uslp"),
     "131.0-B-6 9.1.2, 9.2.1, 9.3.1 Figure 9-1, 11.3 and 732.1-B-3 2.4.1"),
    ("cadu_errors.json", "ASM attach errors", cadu_error_cases,
     "131.0-B-6 9.2.1 and docs/standards/tm-sync.md SYNC-003"),
    ("randomizer_131071.json", "The 131071-bit sequence", lambda: rand_cases(131071),
     "131.0-B-6 10.2.1, 10.3.2, 10.3.3, 10.4.1, 10.4.3 and its note 2"),
    ("randomizer_255.json", "The 255-bit sequence", lambda: rand_cases(255),
     "131.0-B-6 10.2.1, 10.3.2, 10.3.3, 10.4.2, 10.4.3 and its note 2"),
    ("config.json", "Configuration and sequence-choice errors", config_cases,
     "131.0-B-6 11.1, 11.3, 11.4 and docs/standards/tm-sync.md SYNC-002, SYNC-016, SYNC-020"),
]

SCENARIO_FILES = [
    ("search_offsets_tm.json", "TM, 4-octet frames: every skip from 0 to 40 bits",
     lambda: offsets_scenarios("tm", 4, range(0, 41)), cfg_of("tm", 4)),
    ("search_offsets_aos.json", "AOS, 9-octet frames: skips around octet and word boundaries",
     lambda: offsets_scenarios("aos", 9, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 15, 16, 17, 31, 32, 33,
                                          63, 64, 65, 100, 127, 128, 129]),
     cfg_of("aos", 9)),
    ("search_offsets_uslp.json", "Fixed-length USLP, 16-octet frames: skips around the "
     "boundaries", lambda: offsets_scenarios("uslp", 16, [0, 1, 7, 8, 9, 23, 24, 25, 56, 57,
                                                         121, 255, 256]),
     cfg_of("uslp", 16)),
    ("search_errors.json", "Tolerance 0 to 15, errors up to and past it", errors_scenarios,
     cfg_of("tm", 6)),
    ("search_first_match.json", "The first position within the tolerance wins",
     first_match_scenarios, cfg_of("tm", 2)),
    ("search_chunks.json", "The result does not depend on the chunking", chunk_scenarios,
     cfg_of("tm", 5)),
    ("search_units.json", "Units, skipped bits, counters, Reset", unit_scenarios,
     cfg_of("tm", 16)),
    ("search_bounds.json", "Output area bound and BufferTooSmall", bound_scenarios,
     cfg_of("tm", 3)),
    ("search_capacity.json", "The input chunk capacity", capacity_scenarios, cfg_of("tm", 4)),
]

SCENARIO_SOURCE = ("131.0-B-6 9.2, 9.3.1 Figure 9-1, 10.3.4 and docs/standards/tm-sync.md "
                   "SYNC-007 to SYNC-016 (OI-80, OI-81)")


def build_scenario_file(name, scenarios, base_cfg):
    scs = []
    for s in scenarios:
        cfg = s.cfg if s.cfg is not None else base_cfg
        try:
            steps = replay(cfg, [(1000 * (i + 1), inp) for i, inp in enumerate(s.inputs)])
        except Unspecified as e:
            raise SystemExit("%s: the model refuses the case: %s" % (s.sid, e))
        if s.intent is not None:
            got = units_in(steps)
            if got != s.intent:
                raise SystemExit("%s: built to show %s, the model returns %s" %
                                 (s.sid, s.intent, got))
        d = {"id": s.sid, "description": s.desc, "requirements": s.reqs}
        if s.cfg is not None:
            d["config"] = s.cfg
        d["steps"] = steps
        scs.append(d)
    return {"kind": "engine-scenario", "engine": "asm-search", "provenance": "hand-derived",
            "provenance_note": NOTE % SCENARIO_SOURCE, "config": base_cfg, "scenarios": scs}


def all_files():
    files = {}
    for name, _what, fn, source in CODEC_FILES:
        cases = [build_codec(c) for c in fn()]
        files[name] = {"kind": "codec", "standard": "131.0-B-6", "provenance": "hand-derived",
                       "provenance_note": NOTE % source, "cases": cases}
    for name, _what, fn, cfg in SCENARIO_FILES:
        files["scenarios/" + name] = build_scenario_file(name, fn(), cfg)
    return files


def root_dir():
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.normpath(os.path.join(here, "..", ".."))


def dump(obj, level=0):
    """Deterministic text: a file or a scenario is expanded, one key per line; a case, a step and
    every other small object is one line; the elements of a list of objects go one to a line."""
    pad = "  " * level
    inner = "  " * (level + 1)
    if isinstance(obj, dict) and ("kind" in obj or "steps" in obj):
        items = ['%s%s: %s' % (inner, json.dumps(k), dump(v, level + 1)) for k, v in obj.items()]
        return "{\n" + ",\n".join(items) + "\n" + pad + "}"
    if isinstance(obj, list) and obj and all(isinstance(x, dict) for x in obj):
        return "[\n" + ",\n".join(inner + dump(x, level + 1) for x in obj) + "\n" + pad + "]"
    return json.dumps(obj, separators=(", ", ": "))


def render(doc):
    return dump(doc) + "\n"


def write_all(outdir=None):
    base = outdir or os.path.join(root_dir(), "test-vectors", "sync")
    os.makedirs(os.path.join(base, "scenarios"), exist_ok=True)
    selftest()
    files = all_files()
    for sub in ("", "scenarios"):
        for name in sorted(os.listdir(os.path.join(base, sub))):
            rel = os.path.join(sub, name) if sub else name
            if name.endswith(".json") and rel not in files:
                os.remove(os.path.join(base, rel))
    for name, doc in files.items():
        with open(os.path.join(base, name), "w") as f:
            f.write(render(doc))
    n = sum(len(d.get("cases", d.get("scenarios", []))) for d in files.values())
    print("wrote %d file(s), %d case(s) and scenario(s)" % (len(files), n))
    return 0


# --------------------------------------------------------------------------------------------
# --check
# --------------------------------------------------------------------------------------------


def check_file(path, generated):
    problems = []
    with open(path) as f:
        doc = json.load(f)
    seen = set()
    if doc["kind"] == "codec":
        for c in doc["cases"]:
            if c["id"] in seen:
                problems.append("%s: case id twice" % c["id"])
            seen.add(c["id"])
            if c.get("superseded"):
                continue
            inp = bytes.fromhex(c["input_hex"]) if "input_hex" in c else None
            try:
                want = run_codec(c["operation"], c.get("config"), inp, c.get("params"))
            except Unspecified as e:
                problems.append("%s: the model refuses the case: %s" % (c["id"], e))
                continue
            if want != c["expect"]:
                problems.append("%s: model says %s, file says %s" %
                                (c["id"], json.dumps(want), json.dumps(c["expect"])))
    else:
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
                steps = replay(cfg, inputs)
            except Unspecified as e:
                problems.append("%s: the model refuses the case: %s" % (sid, e))
                continue
            for i, (want, got) in enumerate(zip(steps, sc["steps"])):
                if want != got:
                    problems.append("%s: step %d: model says %s, file says %s" %
                                    (sid, i, json.dumps(want), json.dumps(got)))
    name = os.path.basename(path)
    if doc["kind"] != "codec":
        name = "scenarios/" + name
    if name not in generated:
        problems.append("%s: not a file the generator writes" % name)
    elif generated[name] != doc:
        problems.append("%s: differs from what the generator writes (run --write)" % name)
    return problems


def check(paths):
    selftest()
    problems = []
    generated = all_files()
    for p in paths:
        problems.extend(check_file(p, generated))
    for p in problems:
        print("error:", p)
    print("sync model: %d file(s), %d problem(s)" % (len(paths), len(problems)))
    return 1 if problems else 0


def main(argv):
    if len(argv) >= 2 and argv[1] == "--check":
        return check(argv[2:])
    if len(argv) in (2, 3) and argv[1] == "--write":
        return write_all(argv[2] if len(argv) == 3 else None)
    print("usage: sync.py --check FILE... | --write [DIR]")
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
