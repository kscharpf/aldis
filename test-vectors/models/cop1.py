"""Independent model of COP-1 (FOP-1 and FARM-1) for the vector files under test-vectors/cop1/
(COP-001, COP-002, COP-005, TV-010, TV-011, TV-023; M3 WP-2).

Sources: CCSDS 232.1-B-2 and its Technical Corrigendum 1: the variables of 5.1 and 6.1, the
actions of 5.2 and 6.2, the notes of 5.3.2 and 6.3.2, Table 5-1 (FOP-1) and Table 6-1
(FARM-1), and the managed parameters of Tables 7-1 and 7-2. The frame and CLCW layouts come
from CCSDS 232.0-B-4: 4.1.2 (Transfer Frame Primary Header), 4.1.3.3 (Unlock and Set V(R)),
4.1.4.2 (the FECF), and 4.2.1 (the CLCW). The contract of an engine (a pure step returning
actions, timer expiry as an input, `next_deadline` as a query) comes from docs/03-api-model.md
sections 5 to 8 and docs/standards/cop1.md; the cell review docs/verification/reviews/
cop1-tables.md was read as specification input and cross-checked against the transcription
below.

This program was written blind (TV-023): by an agent that had not read any implementation
under test (aldis-cop1, the codecs, aldis-ffi, exerciser/, fuzz/, include/, refapp/), from the
Blue Books and docs/. For the file layout and schema change only, the author also read the test
infrastructure crates/aldis-testkit and the schema generator, which hold no COP-1 logic. It must
stay independent of the Rust code: never port code from it, and never change the model to
agree with it. When a model and an implementation disagree, the Blue Book decides.

Both tables were transcribed from the Blue Book PDF. docs/verification/reviews/cop1-tables.md,
the review record of the same two tables, was also read as specification input, and the
transcription was then compared with it (Not applicable marks and next states agree); that
comparison is a check, and the transcription's independence rests on the Blue Book, not on
the review.

The model holds parameters and code, never Blue Book text. Event descriptions below are
paraphrases.

What it does
------------
* The two tables are transcribed as data (TABLE_FOP, TABLE_FARM): event -> state -> (action
  tokens, next state), `None` for a cell the Blue Book marks "Not applicable".
* `Fop` and `Farm` are the engines: `classify` derives the event number from the input and the
  variables (the work done before the table is entered), and `step` runs the cell.
* The generator finds, by breadth-first search over the inputs of a small alphabet, the
  shortest input sequence that reaches every cell, so that each vector is a setup sequence, the
  trigger, and the expected outputs of the model. A cell that is "Not applicable" is driven
  with the conformance-only input {"table_event": n} (FOP-1) or, for FARM-1 (S2, E1), with the
  frame that selects it.

Vocabulary of inputs and outputs: test-vectors/cop1/README.md.

Usage, from the repository root:

    python3 -I test-vectors/models/cop1.py --check test-vectors/cop1/*/*.json
    python3 -I test-vectors/models/cop1.py --write           # regenerate every file
    python3 -I test-vectors/models/cop1.py --audit           # search the Not applicable cells

--check replays every live case through the model, compares inputs and expected outputs, checks
that the trigger of a table cell reaches exactly the cell its id names, and compares the file's
metadata (requirements, description, provenance) with the generator's; it exits 1 on any
difference. --write regenerates test-vectors/cop1/ (cases and provenance notes), and is for
files that are not released yet (TV-021). --audit searches the reachable states for an input
that reaches a "Not applicable" cell and reports any it finds (there should be none).
"""
import hashlib
import json
import os
import sys

# --------------------------------------------------------------------------------------------
# Frames and the CLCW (232.0-B-4 4.1.2, 4.1.3.3, 4.1.4.2, 4.2.1)
# --------------------------------------------------------------------------------------------

# (Bypass Flag, Control Command Flag) of each frame type, 232.0 Table 4-1.
FLAGS = {"AD": (0, 0), "BD": (1, 0), "BC": (1, 1)}
KIND_OF_FLAGS = {v: k for k, v in FLAGS.items()}
UNLOCK = bytes([0x00])  # 4.1.3.3.2
SET_VR_PREFIX = bytes([0x82, 0x00])  # 4.1.3.3.3


def crc16(data):
    """FECF of 4.1.4.2: G(X) = X^16 + X^12 + X^5 + 1, register preset to all ones."""
    reg = 0xFFFF
    for byte in data:
        reg ^= byte << 8
        for _ in range(8):
            reg = ((reg << 1) ^ 0x1021) & 0xFFFF if reg & 0x8000 else (reg << 1) & 0xFFFF
    return reg


def tc_frame(kind, scid, vcid, ns, data, fecf):
    bypass, control = FLAGS[kind]
    total = 5 + len(data) + (2 if fecf else 0)
    header = (bypass << 37) | (control << 36) | (scid << 24) | (vcid << 18) | ((total - 1) << 8) | ns
    body = header.to_bytes(5, "big") + bytes(data)
    if fecf:
        body += crc16(body).to_bytes(2, "big")
    return body


def parse_tc_frame(frame, fecf):
    """(kind, ns, data) of a frame, or None for the reserved flag combination."""
    first = frame[0]
    kind = KIND_OF_FLAGS.get(((first >> 5) & 1, (first >> 4) & 1))
    end = len(frame) - (2 if fecf else 0)
    return kind, frame[4], frame[5:end]


def clcw_word(lockout, wait, retransmit, farm_b, n_r, vcid, status=0, no_rf=0, no_bit_lock=0):
    """The 32-bit CLCW of 4.2.1 as 4 octets (bit 0 is the most significant)."""
    word = (
        (0 << 31)  # Control Word Type
        | (0 << 29)  # CLCW Version Number
        | (status << 26)
        | (1 << 24)  # COP in Effect = 01 for COP-1
        | (vcid << 18)
        | (no_rf << 15)
        | (no_bit_lock << 14)
        | (lockout << 13)
        | (wait << 12)
        | (retransmit << 11)
        | ((farm_b & 3) << 9)
        | n_r
    )
    return word.to_bytes(4, "big")


class Err(Exception):
    """An input the engine refuses: the error kind of docs/03-api-model.md section 9."""


def wrap(n):
    return n % 256


# --------------------------------------------------------------------------------------------
# FOP-1: Table 5-1 transcribed (232.1-B-2 5.3, Rev. B)
# --------------------------------------------------------------------------------------------

STATES = ["S1", "S2", "S3", "S4", "S5", "S6"]
NA = None


def row(*cells):
    """Six cells S1..S6: each 'tokens|next' or None (Not applicable)."""
    assert len(cells) == 6
    out = []
    for c in cells:
        if c is None:
            out.append(None)
        else:
            tokens, nxt = c.split("|")
            out.append((tuple(t for t in tokens.split(",") if t), nxt))
    return out


def same(tokens, n=6):
    """The same tokens in every state, staying in the state."""
    return row(*[tokens + "|S%d" % (i + 1) for i in range(n)])


def to(tokens, nxt):
    return tokens + "|" + nxt


TABLE_FOP = {
    # --- CLCW arrives (valid COP-1 pattern) ---
    1: row("ignore|S1", "alert:synch|S6", "alert:synch|S6", "confirm,cancel_timer|S1",
           "confirm,release_bc,cancel_timer|S1", "ignore|S6"),
    2: row(*(["remove_acked,cancel_timer,look_fdu|S1"] * 3 + [NA, NA, "ignore|S6"])),
    3: row(*(["alert:clcw|S6"] * 5 + ["ignore|S6"])),
    4: row("alert:synch|S6", "alert:synch|S6", "alert:synch|S6", "alert:synch|S6", "ignore|S5",
           "ignore|S6"),
    5: row("ignore|S1", "alert:synch|S6", "alert:synch|S6", NA, NA, "ignore|S6"),
    6: row(*(["remove_acked,look_fdu|S1"] * 3 + [NA, NA, "ignore|S6"])),
    7: row(*(["alert:clcw|S6"] * 3 + [NA, NA, "ignore|S6"])),
    101: row(*(["remove_acked,alert:limit|S6"] * 3 + [NA, NA, "ignore|S6"])),
    102: row(*(["alert:limit|S6"] * 3 + [NA, NA, "ignore|S6"])),
    8: row(*(["remove_acked,init_ad_retx,look_fdu|S2"] * 3 + [NA, NA, "ignore|S6"])),
    9: row(*(["remove_acked|S3"] * 3 + [NA, NA, "ignore|S6"])),
    10: row("init_ad_retx,look_fdu|S2", "ignore|S2", "init_ad_retx,look_fdu|S2", NA, NA,
            "ignore|S6"),
    11: row(*(["ignore|S3"] * 3 + [NA, NA, "ignore|S6"])),
    12: row(*(["ignore|S2"] * 3 + [NA, NA, "ignore|S6"])),
    103: row(*(["ignore|S3"] * 3 + [NA, NA, "ignore|S6"])),
    13: row("alert:nn_r|S6", "alert:nn_r|S6", "alert:nn_r|S6", "alert:nn_r|S6", "ignore|S5",
            "ignore|S6"),
    14: row("alert:lockout|S6", "alert:lockout|S6", "alert:lockout|S6", "alert:lockout|S6",
            "ignore|S5", "ignore|S6"),
    15: row(*(["alert:clcw|S6"] * 5 + ["ignore|S6"])),
    # --- Timer T1 expires ---
    16: row("init_ad_retx,look_fdu|S1", "init_ad_retx,look_fdu|S2", "ignore|S3", "alert:t1|S6",
            "init_bc_retx,look_directive|S5", NA),
    104: row("init_ad_retx,look_fdu|S1", "init_ad_retx,look_fdu|S2", "ignore|S3",
             "set_ss:4,suspend|S6", "init_bc_retx,look_directive|S5", NA),
    17: row(*(["alert:t1|S6"] * 5 + [NA])),
    18: row("set_ss:1,suspend|S6", "set_ss:2,suspend|S6", "set_ss:3,suspend|S6",
            "set_ss:4,suspend|S6", "alert:t1|S6", NA),
    # --- Requests to transfer ---
    19: row("add_wait,look_fdu|S1", "add_wait,look_fdu|S2", "add_wait|S3", "reject|S4",
            "reject|S5", "reject|S6"),
    20: same("reject"),
    21: same("accept,transmit_bd"),
    22: same("reject"),
    # --- Directives ---
    23: row(*(["reject|S%d" % i for i in range(1, 6)] + ["accept,initialize,confirm|S1"])),
    24: row(*(["reject|S%d" % i for i in range(1, 6)] + ["accept,initialize,start_timer|S4"])),
    25: row(*(["reject|S%d" % i for i in range(1, 6)] + ["accept,initialize,transmit_unlock|S5"])),
    26: same("reject"),
    27: row(*(["reject|S%d" % i for i in range(1, 6)]
              + ["accept,initialize,set_vs_from_vr,transmit_setvr|S5"])),
    28: same("reject"),
    29: row(*(["accept,alert:term,confirm|S6"] * 5 + ["accept,confirm|S6"])),
    30: same("reject"),
    31: row(NA, NA, NA, NA, NA, "accept,resume,confirm|S1"),
    32: row(NA, NA, NA, NA, NA, "accept,resume,confirm|S2"),
    33: row(NA, NA, NA, NA, NA, "accept,resume,confirm|S3"),
    34: row(NA, NA, NA, NA, NA, "accept,resume,confirm|S4"),
    35: row(*(["reject|S%d" % i for i in range(1, 6)] + ["set_vs_if_ss0|S6"])),
    36: same("accept,set_k,confirm"),
    37: same("accept,set_t1,confirm"),
    38: same("accept,set_limit,confirm"),
    39: same("accept,set_tt,confirm"),
    40: same("reject"),
    # --- Responses of the Lower Procedures ---
    41: row("ad_ready,look_fdu|S1", "ad_ready,look_fdu|S2", "ad_ready|S3", "ad_ready|S4",
            "ad_ready|S5", "ad_ready|S6"),
    42: row(*(["alert:llif|S6"] * 6)),
    43: row("bc_ready|S1", "bc_ready|S2", "bc_ready|S3", "bc_ready|S4",
            "bc_ready,look_directive|S5", "bc_ready|S6"),
    44: row(*(["alert:llif|S6"] * 6)),
    45: same("bd_ready,accept"),
    46: row(*(["alert:llif|S6"] * 6)),
}

FOP_EVENTS = [1, 2, 3, 4, 5, 6, 7, 101, 102, 8, 9, 10, 11, 12, 103, 13, 14, 15, 16, 104, 17, 18]
FOP_EVENTS += list(range(19, 47))

# My words for each event (not the Blue Book's).
FOP_EVENT_TEXT = {
    1: "CLCW, lockout clear: N(R) = V(S), no retransmit, no wait, N(R) = NN(R) (nothing outstanding)",
    2: "CLCW, lockout clear: N(R) = V(S), no retransmit, no wait, N(R) != NN(R) (all acknowledged)",
    3: "CLCW, lockout clear: N(R) = V(S), no retransmit, Wait set",
    4: "CLCW, lockout clear: N(R) = V(S), Retransmit set",
    5: "CLCW, lockout clear: NN(R) <= N(R) < V(S), no retransmit, no wait, N(R) = NN(R)",
    6: "CLCW, lockout clear: NN(R) < N(R) < V(S), no retransmit, no wait (partial acknowledgement)",
    7: "CLCW, lockout clear: NN(R) <= N(R) < V(S), no retransmit, Wait set",
    101: "CLCW, lockout clear: NN(R) < N(R) < V(S), Retransmit set, Transmission_Limit = 1",
    102: "CLCW, lockout clear: N(R) = NN(R) < V(S), Retransmit set, Transmission_Limit = 1",
    8: "CLCW, lockout clear: NN(R) < N(R) < V(S), Retransmit set, Transmission_Limit > 1, no wait",
    9: "CLCW, lockout clear: NN(R) < N(R) < V(S), Retransmit set, Transmission_Limit > 1, Wait set",
    10: "CLCW, lockout clear: N(R) = NN(R) < V(S), Retransmit set, Transmission_Limit > 1, "
        "Transmission_Count < limit, no wait",
    11: "CLCW, lockout clear: N(R) = NN(R) < V(S), Retransmit set, Transmission_Limit > 1, "
        "Transmission_Count < limit, Wait set",
    12: "CLCW, lockout clear: N(R) = NN(R) < V(S), Retransmit set, Transmission_Limit > 1, "
        "Transmission_Count >= limit, no wait",
    103: "CLCW, lockout clear: N(R) = NN(R) < V(S), Retransmit set, Transmission_Limit > 1, "
         "Transmission_Count >= limit, Wait set",
    13: "CLCW with N(R) outside NN(R)..V(S) (modulo 256), lockout clear",
    14: "CLCW with the Lockout Flag set",
    15: "CLCW whose bit pattern is invalid under COP-1",
    16: "Timer T1 expires, Transmission_Count < limit, Timeout_Type 0",
    104: "Timer T1 expires, Transmission_Count < limit, Timeout_Type 1",
    17: "Timer T1 expires, Transmission_Count >= limit, Timeout_Type 0",
    18: "Timer T1 expires, Transmission_Count >= limit, Timeout_Type 1",
    19: "Request to transfer an AD FDU, Wait_Queue empty",
    20: "Request to transfer an AD FDU, Wait_Queue occupied",
    21: "Request to transfer a BD FDU, BD_Out_Flag Ready",
    22: "Request to transfer a BD FDU, BD_Out_Flag Not_Ready",
    23: "Directive Initiate AD Service (without CLCW check)",
    24: "Directive Initiate AD Service (with CLCW check)",
    25: "Directive Initiate AD Service (with Unlock), BC_Out_Flag Ready",
    26: "Directive Initiate AD Service (with Unlock), BC_Out_Flag Not_Ready",
    27: "Directive Initiate AD Service (with Set V(R)), BC_Out_Flag Ready",
    28: "Directive Initiate AD Service (with Set V(R)), BC_Out_Flag Not_Ready",
    29: "Directive Terminate AD Service",
    30: "Directive Resume AD Service, Suspend_State 0",
    31: "Directive Resume AD Service, Suspend_State 1",
    32: "Directive Resume AD Service, Suspend_State 2",
    33: "Directive Resume AD Service, Suspend_State 3",
    34: "Directive Resume AD Service, Suspend_State 4",
    35: "Directive Set V(S)",
    36: "Directive Set FOP_Sliding_Window_Width (K)",
    37: "Directive Set T1_Initial",
    38: "Directive Set Transmission_Limit",
    39: "Directive Set Timeout_Type",
    40: "Invalid directive",
    41: "Response AD_Accept",
    42: "Response AD_Reject",
    43: "Response BC_Accept",
    44: "Response BC_Reject",
    45: "Response BD_Accept",
    46: "Response BD_Reject",
}

ALERT_REASONS = ("limit", "t1", "lockout", "synch", "nn_r", "clcw", "llif", "term")


def is_transfer_event(event):
    """Events 19 to 22 and 45 concern an FDU; the other cells answer a directive."""
    return event in (19, 20, 21, 22, 45)


class Fop:
    """FOP-1 for one virtual channel: variables of 5.1, actions of 5.2, Table 5-1."""

    def __init__(self, cfg):
        self.cfg = cfg
        self.scid = cfg["scid"]
        self.vcid = cfg["vcid"]
        self.fecf = cfg["fecf"]
        self.t1 = cfg["t1_initial_us"]
        self.tl = cfg["transmission_limit"]
        self.tt = cfg["timeout_type"]
        self.k = cfg["k"]
        # Start-up values are not given by the Blue Book (proposed OI): Initial state, V(S) and
        # NN(R) zero, every Out flag Ready, no timer, no suspension.
        self.state = "S6"
        self.vs = 0
        self.nnr = 0
        self.tc = 1
        self.ss = 0
        self.sent = []  # [kind, ns, handle, data, to_be_retransmitted]
        self.wait = None  # (handle, data)
        self.ad_out = True
        self.bc_out = True
        self.bd_out = True
        self.timer = None  # absolute deadline in microseconds, or None
        self.pending = None  # handle of an Initiate AD directive that awaits its confirmation
        self.bd_handle = None
        self.event = None
        self.prior = None

    def copy(self):
        other = Fop.__new__(Fop)
        other.__dict__.update(self.__dict__)
        other.sent = [e[:] for e in self.sent]
        return other

    # ---- queries -------------------------------------------------------------------------
    def next_deadline(self):
        return self.timer

    def key(self):
        """Everything that decides future behaviour, with sequence numbers made relative."""
        return (
            self.state,
            wrap(self.vs - self.nnr),
            min(self.tc, 4),
            self.ss,
            self.tl,
            self.tt,
            tuple((e[0], wrap(e[1] - self.nnr), e[4]) for e in self.sent),
            self.wait is not None,
            self.ad_out,
            self.bc_out,
            self.bd_out,
            self.timer is not None,
            self.pending is not None,
        )

    def coherent(self):
        """Setup states that mean what they say. A suspension keeps the queues it had (5.1.11):
        an Alert after it leaves Suspend_State set over purged queues, which no setup should
        resume; and an Initiate AD directive that awaits its confirmation belongs to S4 or S5,
        or to the S4 that was suspended (what a new Initiate does to it the Blue Book does not
        say)."""
        ads = [e for e in self.sent if e[0] == "AD"]
        if self.state == "S6" and self.ss in (1, 2, 3) and not ads:
            return False
        if self.state == "S6" and self.ss == 4 and self.pending is None:
            return False
        waiting = self.state in ("S4", "S5") or (self.state == "S6" and self.ss == 4)
        return waiting == (self.pending is not None)

    # ---- classification: what happens before the table is entered -------------------------
    def classify(self, inp, now):
        """The event number of an input, or None when the input changes nothing."""
        (name, arg), = inp.items()
        if name == "table_event":
            return arg["event"]
        if name == "clcw":
            return self.classify_clcw(arg)
        if name == "timeout":
            if self.timer is None or now < self.timer:
                return None
            if self.tc < self.tl:
                return 16 if self.tt == 0 else 104
            return 17 if self.tt == 0 else 18
        if name == "request_ad":
            return 19 if self.wait is None else 20
        if name == "request_bd":
            return 21 if self.bd_out else 22
        if name == "directive":
            return self.classify_directive(arg)
        if name == "transmit_done":
            base = {"AD": 41, "BC": 43, "BD": 45}[arg["kind"]]
            return base if arg["response"] == "accept" else base + 1
        raise Err("InvalidInput")

    def classify_clcw(self, c):
        if c.get("vcid", self.vcid) != self.vcid:
            raise Err("UnknownChannel")  # a routing error, not a COP-1 event (cop1.md 2.2)
        if c.get("cop_in_effect", 1) != 1 or c.get("control_word_type", 0) != 0:
            return 15
        if c["lockout"]:
            return 14
        nr = c["n_r"]
        if wrap(nr - self.nnr) > wrap(self.vs - self.nnr):
            return 13
        retx, wait = c["retransmit"], c["wait"]
        if nr == self.vs:
            if retx:
                return 4
            if wait:
                return 3
            return 1 if nr == self.nnr else 2
        if not retx:
            if wait:
                return 7
            return 5 if nr == self.nnr else 6
        if self.tl == 1:
            return 101 if nr != self.nnr else 102
        if nr != self.nnr:
            return 9 if wait else 8
        if self.tc < self.tl:
            return 11 if wait else 10
        return 103 if wait else 12

    def classify_directive(self, d):
        kind = d["type"]
        value = d.get("value")
        ok8 = isinstance(value, int) and 0 <= value <= 255
        if kind == "initiate_ad_without_clcw_check":
            return 23
        if kind == "initiate_ad_with_clcw_check":
            return 24
        if kind == "initiate_ad_with_unlock":
            return 25 if self.bc_out else 26
        if kind == "initiate_ad_with_set_vr":
            if not ok8:
                return 40
            return 27 if self.bc_out else 28
        if kind == "terminate_ad":
            return 29
        if kind == "resume_ad":
            return 30 + self.ss
        if kind == "set_v_s":
            return 35 if ok8 else 40
        if kind == "set_k":
            return 36 if isinstance(value, int) and 1 <= value <= 255 else 40
        if kind == "set_t1_initial":
            return 37 if isinstance(value, int) and value >= 1 else 40
        if kind == "set_transmission_limit":
            return 38 if isinstance(value, int) and 1 <= value <= 255 else 40
        if kind == "set_timeout_type":
            return 39 if value in (0, 1) else 40
        return 40

    # ---- the step ---------------------------------------------------------------------------
    def step(self, inp, now):
        """Apply an input; return the ordered outputs. An error changes nothing."""
        (name, arg), = inp.items()
        event = self.classify(inp, now)
        if event is None:
            return []  # a Timeout before the deadline (API-053)
        cell = TABLE_FOP[event][STATES.index(self.state)]
        if cell is None:
            raise Err("InvalidState")
        tokens, nxt = cell
        self.event, self.prior = event, self.state
        if name == "table_event":
            # The cell is applied as it is; an ordinary input may carry the payload.
            inp = arg.get("input") or {"table_event": {}}
            (name, arg), = inp.items()
        ctx = {"event": event, "now": now, "arg": arg, "out": []}
        if name == "timeout":
            self.timer = None  # the timer has expired; a cell may start it again
        if name == "request_ad":
            ctx["handle"], ctx["data"] = arg["handle"], bytes.fromhex(arg["data_hex"])
        elif name == "request_bd":
            ctx["handle"], ctx["data"] = arg["handle"], bytes.fromhex(arg["data_hex"])
        elif name == "directive":
            ctx["handle"] = arg["handle"]
            ctx["value"] = arg.get("value")
        elif name == "clcw":
            ctx["nr"] = arg["n_r"]
        if event in (24, 25, 27) and self.state == "S6":
            self.pending = arg["handle"]
        for token in tokens:
            self.run(token, ctx)
        self.state = nxt
        return ctx["out"]

    # ---- helpers for outputs ---------------------------------------------------------------
    def notify(self, ctx, kind, handle=None):
        if is_transfer_event(ctx["event"]):
            typ = "transfer_notify"
        else:
            typ = "directive_notify"
        ctx["out"].append({"event": typ, "handle": ctx["handle"] if handle is None else handle,
                           "type": kind})

    def emit_frame(self, ctx, kind, ns, data, retransmission):
        frame = tc_frame(kind, self.scid, self.vcid, ns, data, self.fecf)
        ctx["out"].append({"transmit": {"kind": kind, "ns": ns, "retransmission": retransmission,
                                        "frame_hex": frame.hex().upper()}})

    def purge_sent(self, ctx):
        for kind, ns, handle, data, tbr in self.sent:
            if kind == "AD":
                ctx["out"].append({"event": "transfer_notify", "handle": handle,
                                   "type": "negative_confirm"})
        self.sent = []
        self.nnr = self.vs  # OI-60: an empty queue has no oldest frame (5.1.8)

    def purge_wait(self, ctx):
        if self.wait is not None:
            ctx["out"].append({"event": "transfer_notify", "handle": self.wait[0],
                               "type": "reject"})
            self.wait = None

    def look_fdu(self, ctx):
        if not self.ad_out:
            return
        for entry in self.sent:
            if entry[0] == "AD" and entry[4]:
                self.ad_out = False
                entry[4] = False
                self.emit_frame(ctx, "AD", entry[1], entry[3], True)
                return
        if self.wait is not None and wrap(self.vs - self.nnr) < self.k:
            handle, data = self.wait
            self.wait = None
            ctx["out"].append({"event": "transfer_notify", "handle": handle, "type": "accept"})
            self.transmit_ad(ctx, handle, data)

    def transmit_ad(self, ctx, handle, data):
        ns = self.vs
        self.vs = wrap(self.vs + 1)
        if not self.sent:
            self.tc = 1
        self.sent.append(["AD", ns, handle, data, False])
        self.timer = ctx["now"] + self.t1
        self.ad_out = False
        self.emit_frame(ctx, "AD", ns, data, False)

    def transmit_bc(self, ctx, data):
        self.sent.append(["BC", 0, ctx["handle"], data, False])
        self.tc = 1
        self.timer = ctx["now"] + self.t1
        self.bc_out = False
        self.emit_frame(ctx, "BC", 0, data, False)

    def look_directive(self, ctx):
        if not self.bc_out:
            return
        for entry in self.sent:
            if entry[0] == "BC" and entry[4]:
                self.bc_out = False
                entry[4] = False
                self.emit_frame(ctx, "BC", 0, entry[3], True)
                return

    def init_retx(self, ctx, kind):
        ctx["out"].append({"event": "abort"})
        self.tc += 1
        self.timer = ctx["now"] + self.t1
        for entry in self.sent:
            if entry[0] == kind:
                entry[4] = True

    def alert(self, ctx, reason):
        self.timer = None
        self.purge_sent(ctx)
        self.purge_wait(ctx)
        if self.pending is not None:
            ctx["out"].append({"event": "directive_notify", "handle": self.pending,
                               "type": "negative_confirm"})
            self.pending = None
        ctx["out"].append({"event": "alert", "reason": reason, "event_number": ctx["event"],
                           "prior_state": self.prior})

    # ---- one token of a cell ---------------------------------------------------------------
    def run(self, token, ctx):
        name, _, arg = token.partition(":")
        out = ctx["out"]
        if name == "ignore":
            return
        if name == "accept":
            handle = self.bd_handle if ctx["event"] == 45 else ctx["handle"]
            self.notify(ctx, "accept", handle)
        elif name == "reject":
            self.notify(ctx, "reject")
        elif name == "confirm":
            if ctx["event"] == 1:
                self.notify(ctx, "positive_confirm", self.pending)
                self.pending = None
            else:
                self.notify(ctx, "positive_confirm")
        elif name == "cancel_timer":
            self.timer = None
        elif name == "start_timer":
            self.timer = ctx["now"] + self.t1
        elif name == "remove_acked":
            count = wrap(ctx["nr"] - self.nnr)
            keep = []
            for entry in self.sent:
                if entry[0] == "AD" and wrap(entry[1] - self.nnr) < count:
                    out.append({"event": "transfer_notify", "handle": entry[2],
                                "type": "positive_confirm"})
                else:
                    keep.append(entry)
            self.sent = keep
            self.nnr = ctx["nr"]
            self.tc = 1
        elif name == "release_bc":
            self.sent = [e for e in self.sent if e[0] != "BC"]
        elif name == "look_fdu":
            self.look_fdu(ctx)
        elif name == "look_directive":
            self.look_directive(ctx)
        elif name == "init_ad_retx":
            self.init_retx(ctx, "AD")
        elif name == "init_bc_retx":
            self.init_retx(ctx, "BC")
        elif name == "add_wait":
            self.wait = (ctx["handle"], ctx["data"])
        elif name == "transmit_bd":
            self.bd_out = False
            self.bd_handle = ctx["handle"]
            self.emit_frame(ctx, "BD", 0, ctx["data"], False)
        elif name == "initialize":
            self.purge_sent(ctx)
            self.purge_wait(ctx)
            self.tc = 1
            self.ss = 0
        elif name == "transmit_unlock":
            self.transmit_bc(ctx, UNLOCK)
        elif name == "set_vs_from_vr":
            self.vs = self.nnr = ctx["value"]
        elif name == "transmit_setvr":
            self.transmit_bc(ctx, SET_VR_PREFIX + bytes([ctx["value"]]))
        elif name == "alert":
            self.alert(ctx, arg)
        elif name == "set_ss":
            self.ss = int(arg)
        elif name == "suspend":
            out.append({"event": "suspend"})
        elif name == "resume":
            self.timer = ctx["now"] + self.t1
            self.ss = 0
        elif name == "set_vs_if_ss0":
            if self.ss == 0:
                self.notify(ctx, "accept")
                self.vs = self.nnr = ctx["value"]
                self.notify(ctx, "positive_confirm")
            else:
                self.notify(ctx, "reject")
        elif name == "set_k":
            self.k = ctx["value"]
        elif name == "set_t1":
            self.t1 = ctx["value"]
        elif name == "set_limit":
            self.tl = ctx["value"]
        elif name == "set_tt":
            self.tt = ctx["value"]
        elif name == "ad_ready":
            self.ad_out = True
        elif name == "bc_ready":
            self.bc_out = True
        elif name == "bd_ready":
            self.bd_out = True
        else:
            raise AssertionError("unknown token " + token)


# --------------------------------------------------------------------------------------------
# FARM-1: Table 6-1 transcribed (232.1-B-2 6.3)
# --------------------------------------------------------------------------------------------

FARM_STATES = ["S1", "S2", "S3"]


def frow(*cells):
    out = []
    for c in cells:
        if c is None:
            out.append(None)
        else:
            tokens, nxt = c.split("|")
            out.append((tuple(t for t in tokens.split(",") if t), nxt))
    return out


TABLE_FARM = {
    1: frow("accept,vr_inc,retx:0|S1", None, "discard|S3"),
    2: frow("discard,retx:1,wait:1|S2", "discard|S2", "discard|S3"),
    3: frow("discard,retx:1|S1", "discard|S2", "discard|S3"),
    4: frow("discard|S1", "discard|S2", "discard|S3"),
    5: frow("discard,lock:1|S3", "discard,lock:1|S3", "discard|S3"),
    6: frow("accept,farmb_inc|S1", "accept,farmb_inc|S2", "accept,farmb_inc|S3"),
    # An Unlock also gives the "unlocked" notification of cop1.md 3.2 (COP-036).
    7: frow("farmb_inc,retx:0,unlocked|S1", "farmb_inc,retx:0,wait:0,unlocked|S1",
            "farmb_inc,retx:0,wait:0,lock:0,unlocked|S1"),
    8: frow("farmb_inc,retx:0,set_vr|S1", "farmb_inc,retx:0,wait:0,set_vr|S1", "farmb_inc|S3"),
    9: frow("discard|S1", "discard|S2", "discard|S3"),
    10: frow("ignore|S1", "wait:0|S1", "wait:0|S3"),
    11: frow("report|S1", "report|S2", "report|S3"),
}

FARM_EVENTS = list(range(1, 12))

FARM_EVENT_TEXT = {
    1: "Valid Type-AD frame, N(S) = V(R), buffer available",
    2: "Valid Type-AD frame, N(S) = V(R), no buffer available",
    3: "Valid Type-AD frame, N(S) in the positive window, not V(R)",
    4: "Valid Type-AD frame, N(S) in the negative window",
    5: "Valid Type-AD frame, N(S) outside the sliding window",
    6: "Valid Type-BD frame",
    7: "Valid Type-BC frame carrying Unlock",
    8: "Valid Type-BC frame carrying Set V(R)",
    9: "Invalid frame, or a Type-BC frame carrying neither command",
    10: "Buffer release signal",
    11: "CLCW report time",
}


class Farm:
    """FARM-1 for one virtual channel: variables of 6.1, Table 6-1."""

    def __init__(self, cfg):
        self.cfg = cfg
        self.scid = cfg["scid"]
        self.vcid = cfg["vcid"]
        self.fecf = cfg["fecf"]
        self.status = cfg.get("status_field", 0)
        window = cfg["window"]
        if window["retransmission_allowed"]:
            self.pw = self.nw = window["w"] // 2  # 6.1.8.3.1
        else:
            self.pw = window["pw"]  # 6.1.8.3.2 (Cor. 1)
            self.nw = window["w"] - window["pw"]
        self.state = "S1"
        self.lock = self.wait = self.retx = 0
        self.farm_b = 0
        self.vr = cfg["v_r"]
        self.no_rf = self.no_bit_lock = 0
        self.event = None
        self.prior = None

    def copy(self):
        other = Farm.__new__(Farm)
        other.__dict__.update(self.__dict__)
        return other

    def key(self):
        return (self.state, self.lock, self.wait, self.retx, self.farm_b, self.vr)

    def clcw(self):
        word = clcw_word(self.lock, self.wait, self.retx, self.farm_b, self.vr, self.vcid,
                         self.status, self.no_rf, self.no_bit_lock)
        return {
            "clcw": {"lockout": bool(self.lock), "wait": bool(self.wait),
                     "retransmit": bool(self.retx), "farm_b_counter": self.farm_b & 3,
                     "n_r": self.vr, "cop_in_effect": 1, "vcid": self.vcid,
                     "status_field": self.status, "no_rf_available": bool(self.no_rf),
                     "no_bit_lock": bool(self.no_bit_lock)},
            "clcw_hex": word.hex().upper(),
        }

    def classify(self, inp):
        (name, arg), = inp.items()
        if name in ("query", "set_physical_flags"):
            return None
        if name == "invalid_frame":
            return 9
        if name == "buffer_released":
            return 10
        kind, ns, data = parse_tc_frame(bytes.fromhex(arg["frame_hex"]), self.fecf)
        if kind == "AD":
            ahead = wrap(ns - self.vr)
            if ahead == 0:
                return 1 if arg["buffer_available"] else 2
            if ahead <= self.pw - 1:
                return 3
            if wrap(self.vr - ns) <= self.nw:
                return 4
            return 5
        if kind == "BD":
            return 6
        if kind == "BC":
            if data == UNLOCK:
                return 7
            if len(data) == 3 and data[:2] == SET_VR_PREFIX:
                return 8
            return 9  # neither command (6.3.2.1)
        raise Err("InvalidFrame")

    def step(self, inp, now):
        (name, arg), = inp.items()
        if name == "query":
            self.event, self.prior = 11, self.state
            return [self.clcw()]
        if name == "set_physical_flags":
            self.no_rf = int(arg["no_rf_available"])
            self.no_bit_lock = int(arg["no_bit_lock"])
            return []
        event = self.classify(inp)
        cell = TABLE_FARM[event][FARM_STATES.index(self.state)]
        if cell is None:
            raise Err("InvalidState")
        tokens, nxt = cell
        self.event, self.prior = event, self.state
        out = []
        frame = None
        if "frame_hex" in (arg if isinstance(arg, dict) else {}):
            frame = parse_tc_frame(bytes.fromhex(arg["frame_hex"]), self.fecf)
        for token in tokens:
            what, _, val = token.partition(":")
            if what == "accept":
                kind, ns, data = frame
                out.append({"deliver": {"kind": kind, "ns": ns if kind == "AD" else 0,
                                        "data_hex": data.hex().upper()}})
            elif what == "discard":
                out.append({"event": "frame_discarded", "event_number": event,
                            "state": self.prior})
            elif what == "vr_inc":
                self.vr = wrap(self.vr + 1)
            elif what == "retx":
                self.retx = int(val)
            elif what == "wait":
                self.wait = int(val)
            elif what == "lock":
                self.lock = int(val)
            elif what == "farmb_inc":
                self.farm_b = (self.farm_b + 1) & 3
            elif what == "set_vr":
                self.vr = frame[2][2]
            elif what == "unlocked":
                out.append({"event": "unlocked"})
            elif what in ("ignore", "report"):
                pass
            else:
                raise AssertionError("unknown token " + token)
        if event == 11:
            out.append(self.clcw())
        self.state = nxt
        if nxt != self.prior:
            out.append({"event": "state_changed", "event_number": event, "from": self.prior,
                        "to": nxt})
        return out


# --------------------------------------------------------------------------------------------
# Replay of a scenario
# --------------------------------------------------------------------------------------------

def make_engine(engine, cfg):
    return Fop(cfg) if engine == "fop1" else Farm(cfg)


def run_step(eng, inp, now):
    """(outputs, error). An error leaves the engine unchanged."""
    probe = eng.copy()
    try:
        out = probe.step(inp, now)
    except Err as e:
        return [{"error": str(e)}], True
    eng.__dict__.update(probe.__dict__)
    return out, False


def replay(engine, cfg, inputs):
    """Run the inputs [(at_us, input)]; return the steps with the model's expectations, the
    (prior state, event) each step reached, and the engine."""
    eng = make_engine(engine, cfg)
    steps = []
    reached = []
    for at, inp in inputs:
        probe = eng.copy()
        probe.event = probe.prior = None
        try:
            out = probe.step(inp, at)
            eng = probe
            reached.append((eng.prior, eng.event))
        except Err as e:
            out = [{"error": str(e)}]
            try:
                event = eng.classify(inp, at) if engine == "fop1" else eng.classify(inp)
            except Err:
                event = None
            reached.append((eng.state, event))
        step = {"at_us": at, "input": inp, "expect": out, "expect_state": eng.state}
        if engine == "fop1":
            if eng.timer is None:
                step["expect_no_timeout"] = True
            else:
                step["expect_timeout_us"] = eng.timer
        steps.append(step)
    return steps, reached, eng


# --------------------------------------------------------------------------------------------
# Search for the shortest setup of each cell
# --------------------------------------------------------------------------------------------

STEP_US = 1000


def next_time(eng, now, inp):
    """The instant of the next step: 1 ms later, or the deadline for a timeout."""
    at = now + STEP_US
    if "timeout" in inp and eng.timer is not None:
        at = max(at, eng.timer)
    return at


def data_of(handle):
    return "%04X" % (0xA000 + handle)


# Symbolic inputs of the FOP-1 alphabet, resolved against the engine when applied.
FOP_MOVES = (
    [("directive", t) for t in (
        "initiate_ad_without_clcw_check", "initiate_ad_with_clcw_check",
        "initiate_ad_with_unlock", "initiate_ad_with_set_vr", "terminate_ad", "resume_ad")]
    + [("set", "set_timeout_type", 1), ("set", "set_timeout_type", 0),
       ("set", "set_transmission_limit", 1), ("set", "set_transmission_limit", 2),
       ("set", "set_transmission_limit", 3)]
    + [("request_ad",), ("request_bd",), ("timeout",)]
    + [("done", kind, resp) for kind in ("AD", "BC", "BD") for resp in ("accept", "reject")]
    + [("clcw", sel, retx, wait, 0) for sel in ("nnr", "mid", "vs", "bad")
       for retx in (0, 1) for wait in (0, 1)]
    + [("clcw", "nnr", 0, 0, 1), ("clcw_invalid",)]
)

# Inputs tried at each node but not used to move on (they only select a cell).
FOP_TRIGGERS = [
    ("directive_bad", "set_k", 0),
    ("directive_bad", "set_k", 256),
    ("set", "set_v_s", 3),
    ("set", "set_k", 2),
    ("set", "set_t1_initial", 3000000),
]


def resolve_fop(eng, move, handle):
    kind = move[0]
    if kind == "directive":
        d = {"handle": handle, "type": move[1]}
        if move[1] == "initiate_ad_with_set_vr":
            d["value"] = 9
        return {"directive": d}
    if kind in ("set", "directive_bad"):
        return {"directive": {"handle": handle, "type": move[1], "value": move[2]}}
    if kind == "request_ad":
        return {"request_ad": {"handle": handle, "data_hex": data_of(handle)}}
    if kind == "request_bd":
        return {"request_bd": {"handle": handle, "data_hex": data_of(handle)}}
    if kind == "timeout":
        return {"timeout": {}} if eng.timer is not None else None
    if kind == "done":
        out_flag = {"AD": eng.ad_out, "BC": eng.bc_out, "BD": eng.bd_out}[move[1]]
        if out_flag:
            return None  # a response answers a request that is outstanding
        return {"transmit_done": {"kind": move[1], "response": move[2]}}
    if kind == "clcw":
        sel = move[1]
        span = wrap(eng.vs - eng.nnr)
        nr = {"nnr": eng.nnr, "vs": eng.vs, "bad": wrap(eng.vs + 1)}.get(sel)
        if sel == "mid":
            if span < 2:
                return None
            nr = wrap(eng.nnr + 1)
        if sel == "vs" and span == 0:
            return None
        if sel == "bad" and span == 255:
            return None
        return {"clcw": {"lockout": bool(move[4]), "wait": bool(move[3]),
                         "retransmit": bool(move[2]), "n_r": nr}}
    if kind == "clcw_invalid":
        return {"clcw": {"lockout": False, "wait": False, "retransmit": False,
                         "n_r": eng.nnr, "cop_in_effect": 2}}
    raise AssertionError(move)


def canonical_payload(event, handle):
    """The ordinary input whose payload an injected event needs, if any."""
    if event in (19, 20):
        return {"request_ad": {"handle": handle, "data_hex": data_of(handle)}}
    if event in (21, 22):
        return {"request_bd": {"handle": handle, "data_hex": data_of(handle)}}
    return None


def search_fop(cfg, max_depth=9, limit=400000):
    """Breadth-first search; returns {(state, event): (inputs, trigger)} for every cell the
    alphabet reaches, shortest first."""
    start = Fop(cfg)
    found = {}
    full = {}
    seen = {start.key()}
    queue = [(start, [], 0)]
    head = 0
    while head < len(queue) and len(queue) < limit:
        eng, path, now = queue[head]
        head += 1
        handle = len(path) + 1
        # Triggers: every move and the extra triggers.
        for move in FOP_MOVES + FOP_TRIGGERS:
            inp = resolve_fop(eng, move, handle)
            if inp is None:
                continue
            at = next_time(eng, now, inp)
            try:
                event = eng.classify(inp, at)
            except Err:
                continue
            if event is None:
                continue
            cell_key = (eng.state, event)
            if cell_key not in found:
                found[cell_key] = (path, (at, inp))
            # A cell that looks for an FDU or a directive is shown best where the look finds
            # one to send, so prefer a setup in which a frame is transmitted.
            cell = TABLE_FOP[event][STATES.index(eng.state)]
            if cell is not None and cell_key not in full and \
                    {"look_fdu", "look_directive"} & set(cell[0]):
                probe = eng.copy()
                try:
                    out = probe.step(inp, at)
                except Err:
                    continue
                if any("transmit" in o for o in out):
                    full[cell_key] = (path, (at, inp))
        if len(path) >= max_depth:
            continue
        for move in FOP_MOVES:
            inp = resolve_fop(eng, move, handle)
            if inp is None:
                continue
            at = next_time(eng, now, inp)
            nxt = eng.copy()
            try:
                nxt.step(inp, at)
            except Err:
                continue
            if not nxt.coherent():
                continue
            k = nxt.key()
            if k in seen:
                continue
            seen.add(k)
            queue.append((nxt, path + [(at, inp)], at))
    found.update(full)
    # Cells no input reaches: the Not applicable ones, and the applicable cells whose event the
    # variables can never select in that state. Each is driven with {"table_event": ...} at the
    # first node of the state.
    first_in_state = {}
    for eng, path, now in queue:
        first_in_state.setdefault(eng.state, (path, now))
    for event in FOP_EVENTS:
        for state in STATES:
            if (state, event) not in found and state in first_in_state:
                path, now = first_in_state[state]
                arg = {"event": event}
                payload = canonical_payload(event, len(path) + 1)
                if payload is not None:
                    arg["input"] = payload
                found[(state, event)] = (path, (now + STEP_US, {"table_event": arg}))
    return found, queue


# --- FARM-1 ---------------------------------------------------------------------------------

def farm_frame(cfg, kind, ns, data):
    return tc_frame(kind, cfg["scid"], cfg["vcid"], ns, data, cfg["fecf"]).hex().upper()


def farm_moves(eng):
    cfg = eng.cfg
    vr = eng.vr
    moves = []
    positions = {"vr": vr, "pos": wrap(vr + 1) if eng.pw > 1 else None,
                 "neg": wrap(vr - 1) if eng.nw > 0 else None, "out": wrap(vr + eng.pw)}
    for tag, ns in positions.items():
        if ns is None:
            continue
        for buf in (True, False):
            if tag != "vr" and not buf:
                continue
            moves.append({"frame_hex": farm_frame(cfg, "AD", ns, bytes.fromhex("C0DE")),
                          "buffer_available": buf})
    moves.append({"frame_hex": farm_frame(cfg, "BD", 0, bytes.fromhex("BD01")),
                  "buffer_available": True})
    moves.append({"frame_hex": farm_frame(cfg, "BC", 0, UNLOCK), "buffer_available": True})
    moves.append({"frame_hex": farm_frame(cfg, "BC", 0, SET_VR_PREFIX + bytes([20])),
                  "buffer_available": True})
    moves.append({"frame_hex": farm_frame(cfg, "BC", 0, bytes([0x01])),
                  "buffer_available": True})
    return moves


def farm_inputs(eng):
    ins = [{"frame": m} for m in farm_moves(eng)]
    ins.append({"invalid_frame": {}})
    ins.append({"buffer_released": {}})
    ins.append({"query": "clcw"})
    return ins


def search_farm(cfg, max_depth=8):
    start = Farm(cfg)
    found = {}
    seen = {start.key()}
    queue = [(start, [], 0)]
    head = 0
    while head < len(queue):
        eng, path, now = queue[head]
        head += 1
        for inp in farm_inputs(eng):
            at = now + STEP_US
            try:
                event = 11 if "query" in inp else eng.classify(inp)
            except Err:
                continue
            if event is not None and (eng.state, event) not in found:
                found[(eng.state, event)] = (path, (at, inp))
        if len(path) >= max_depth:
            continue
        for inp in farm_inputs(eng):
            if "query" in inp:
                continue
            at = now + STEP_US
            nxt = eng.copy()
            try:
                nxt.step(inp, at)
            except Err:
                continue
            k = nxt.key()
            if k in seen:
                continue
            seen.add(k)
            queue.append((nxt, path + [(at, inp)], at))
    return found, queue


# --------------------------------------------------------------------------------------------
# Scenario files
# --------------------------------------------------------------------------------------------

FOP_CFG = {"frame_target": "tc", "scid": 42, "vcid": 1, "t1_initial_us": 2000000,
           "transmission_limit": 3, "timeout_type": 0, "k": 3, "fecf": True}
FARM_CFG = {"frame_target": "tc", "scid": 42, "vcid": 1, "fecf": True, "status_field": 0,
            "window": {"retransmission_allowed": True, "w": 8}, "v_r": 0}

BLIND = (
    "Model test-vectors/models/cop1.py, written blind (TV-023) by a Claude agent from the Blue "
    "Books and docs/ only, without reading any implementation; docs/verification/reviews/"
    "cop1-tables.md was read as input and the transcription compared with it. For the file "
    "layout and schema change only, its author also read crates/aldis-testkit and the schema "
    "generator, which are test infrastructure and not an implementation under test."
)


def note_for(engine, how, tables, frames):
    return ("Hand-derived from 232.1-B-2 Cor. 1 %s and 232.0-B-4 %s. %s %s" %
            (tables, frames, BLIND, how))


def provenance(engine, how):
    if engine == "fop1":
        return note_for(engine, how, "Table 5-1, 5.1 and 5.2", "4.1.2, 4.1.3.3, 4.1.4.2")
    return note_for(engine, how, "Table 6-1, 6.1 and 6.2", "4.1.2, 4.1.3.3, 4.2.1")


SETUP_HOW = ("Each setup is the shortest input sequence found by breadth-first search over the "
             "model's input alphabet, shared by the scenarios that start from it; the expected "
             "outputs are the model's. The file is written by the model (--write) and never "
             "edited by hand.")
SCRIPT_HOW = ("The input scripts were chosen by hand; the expected outputs are the model's. The "
              "file is written by the model (--write) and never edited by hand.")


def requirements_for(engine, scenario_cell, steps, reached, extra_reqs=()):
    """Only what the vector exercises. A table cell lists the requirement of its engine, COP-005,
    and what its trigger step shows; a scenario lists its engine's requirement and the ones its
    author names."""
    reqs = ["COP-001" if engine == "fop1" else "COP-002"]
    if scenario_cell:
        reqs.append("COP-005")
        i = max(i for i, r in enumerate(reached) if r[0] is not None)
        trig, event = steps[i], reached[i][1]
        out = json.dumps(trig["expect"])
        injected = "table_event" in trig["input"]
        if '"error"' in out:
            reqs.append("API-058")
        if engine == "fop1" and not injected:
            if event in (1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 101, 102, 103):
                reqs.append("COP-012")
            if '"transmit"' in out and '"kind": "BD"' not in out:
                reqs.append("COP-010")
            if '"retransmission": true' in out:
                reqs.append("COP-011")
            if event in (16, 17, 18, 104) or (
                    '"transmit"' in out and ('"kind": "AD"' in out or '"kind": "BC"' in out)):
                reqs.append("COP-013")
            if '"suspend"' in out or event in (31, 32, 33, 34):
                reqs.append("COP-014")
            if event == 40 and "set_k" in json.dumps(trig["input"]):
                reqs.append("COP-015")
            if '"alert"' in out:
                reqs.append("COP-017")
        if engine == "farm1":
            for ids, req in (((1, 2, 3, 4, 5), "COP-030"), ((9,), "COP-031"),
                             ((1, 2, 10), "COP-033"), ((6, 7, 8), "COP-034"),
                             ((11,), "COP-035"), ((7,), "COP-036")):
                if event in ids:
                    reqs.append(req)
    for r in extra_reqs:
        if r not in reqs:
            reqs.append(r)
    return reqs


def cell_scenario(engine, state, event, path, trigger):
    """(id, description, inputs, split) of a table cell: the setup path, then the trigger."""
    injected = "table_event" in trigger[1]
    na = injected and TABLE_FOP[event][STATES.index(state)] is None
    text = (FOP_EVENT_TEXT if engine == "fop1" else FARM_EVENT_TEXT)[event]
    table = "5-1" if engine == "fop1" else "6-1"
    kind = "FOP-1" if engine == "fop1" else "FARM-1"
    if na or (engine == "farm1" and (state, event) == ("S2", 1)):
        how = "a cell the Blue Book marks Not applicable: the engine refuses it with InvalidState"
    elif injected:
        how = ("no input reaches this cell, so the event is injected after a setup of %d "
               "step(s)" % len(path))
    else:
        how = "setup of %d step(s), then the trigger" % len(path)
    description = "%s Table %s, %s x E%d: %s. %s." % (kind, table, state, event, text, how)
    sid = "cop1-%s-%s-e%d" % (engine, state.lower(), event)
    return sid, description, list(path) + [trigger], len(path)


# --- Scenarios that are not cells -------------------------------------------------------------

def fop_h(n):
    return {"handle": n, "data_hex": data_of(n)}


def clcw(n_r, retransmit=False, wait=False, lockout=False, **extra):
    c = {"lockout": lockout, "wait": wait, "retransmit": retransmit, "n_r": n_r}
    c.update(extra)
    return {"clcw": c}


def directive(handle, kind, value=None):
    d = {"handle": handle, "type": kind}
    if value is not None:
        d["value"] = value
    return {"directive": d}


def done(kind, response="accept"):
    return {"transmit_done": {"kind": kind, "response": response}}


def timed(inputs, start=0, gap=1000):
    """Number the steps in time; a tuple (at, input) keeps its instant."""
    out = []
    at = start
    for item in inputs:
        if isinstance(item, tuple):
            at, inp = item
        else:
            inp = item
        out.append((at, inp))
        at += gap
    return out


def extras_fop():
    """Scenarios for behaviour around the cells: the normal run, retransmission, the sliding
    window, the timer, suspension, and the errors."""
    t1 = FOP_CFG["t1_initial_us"]
    out = []

    # Normal operation: initiate, three FDUs, acknowledgements (COP-010, COP-011, COP-013).
    inputs = timed([
        directive(1, "initiate_ad_without_clcw_check"),
        {"request_ad": fop_h(2)}, done("AD"),
        {"request_ad": fop_h(3)}, done("AD"),
        clcw(1), clcw(2), clcw(2),
    ])
    out.append(("fop1", "cop1-fop1-ad-run", "Initiate AD, two FDUs sent and acknowledged one "
                "by one, then a CLCW with nothing outstanding.", FOP_CFG, inputs,
                ("COP-010", "COP-013")))

    # Retransmission is byte-identical and comes in order (COP-011).
    inputs = timed([
        directive(1, "initiate_ad_without_clcw_check"),
        {"request_ad": fop_h(2)}, done("AD"),
        {"request_ad": fop_h(3)}, done("AD"),
        {"request_ad": fop_h(4)}, done("AD"),
        clcw(0, retransmit=True), done("AD"), done("AD"), done("AD"),
        clcw(3),
    ])
    out.append(("fop1", "cop1-fop1-retransmit-in-order", "A Retransmit CLCW makes FOP-1 send the "
                "whole Sent_Queue again, oldest first, one frame per AD_Accept.", FOP_CFG,
                inputs, ("COP-011",)))

    # The sliding window K stops a new frame (5.2.10 c).
    cfg = dict(FOP_CFG, k=2)
    inputs = timed([
        directive(1, "initiate_ad_without_clcw_check"),
        {"request_ad": fop_h(2)}, done("AD"),
        {"request_ad": fop_h(3)}, done("AD"),
        {"request_ad": fop_h(4)},
        clcw(1), done("AD"),
    ])
    out.append(("fop1", "cop1-fop1-window-k", "With K = 2 the third FDU waits in the Wait_Queue "
                "until a CLCW acknowledges a frame; it is accepted then.", cfg, inputs,
                ("COP-010",)))

    # A second request while the Wait_Queue is occupied is rejected (E20) and the FDU waits.
    inputs = timed([
        directive(1, "initiate_ad_without_clcw_check"),
        {"request_ad": fop_h(2)},
        {"request_ad": fop_h(3)},
        {"request_ad": fop_h(4)},
        done("AD"),
    ])
    out.append(("fop1", "cop1-fop1-wait-queue", "While the Lower Procedures have not accepted "
                "the first frame, one FDU waits; a second request is rejected.", FOP_CFG,
                inputs, ()))

    # Timer: an early Timeout changes nothing, expiry retransmits, the limit raises an Alert
    # (API-053, COP-013).
    inputs = [
        (0, directive(1, "initiate_ad_without_clcw_check")),
        (1000, {"request_ad": fop_h(2)}),
        (2000, done("AD")),
        (1000 + t1 - 1, {"timeout": {}}),
        (1000 + t1, {"timeout": {}}),
        (1000 + t1 + 1000, done("AD")),
        (1000 + 2 * t1, {"timeout": {}}),
        (1000 + 2 * t1 + 1000, done("AD")),
        (1000 + 3 * t1, {"timeout": {}}),
    ]
    out.append(("fop1", "cop1-fop1-timer-and-limit", "A Timeout before the deadline is harmless; "
                "at the deadline it retransmits and restarts the timer; with the limit reached "
                "it raises Alert[T1] and purges.", FOP_CFG, inputs, ("API-053", "COP-013")))

    # Suspend and Resume (COP-014, 4.3.3).
    cfg = dict(FOP_CFG, transmission_limit=2, timeout_type=1)
    inputs = [
        (0, directive(1, "initiate_ad_without_clcw_check")),
        (1000, {"request_ad": fop_h(2)}),
        (2000, done("AD")),
        (1000 + t1, {"timeout": {}}),
        (1000 + t1 + 1000, done("AD")),
        (1000 + 2 * t1, {"timeout": {}}),
        (1000 + 2 * t1 + 1000, directive(3, "resume_ad")),
        (1000 + 2 * t1 + 2000, clcw(1)),
    ]
    out.append(("fop1", "cop1-fop1-suspend-resume", "With Timeout_Type 1 the exhausted "
                "Transmission_Limit suspends the service without purging; Resume returns to the "
                "saved state and restarts the timer.", cfg, inputs, ("COP-014",)))

    # Initiate AD with Unlock: the BC frame, its timer, and the confirmation by a clean CLCW.
    inputs = timed([
        directive(1, "initiate_ad_with_unlock"), done("BC"), clcw(0),
    ])
    out.append(("fop1", "cop1-fop1-initiate-unlock", "Initiate AD with Unlock sends a Type-BC "
                "frame, keeps its master copy, and confirms the directive on a clean CLCW.",
                FOP_CFG, inputs, ("COP-010", "COP-013")))
    inputs = timed([
        directive(1, "initiate_ad_with_set_vr", 200), done("BC"), clcw(200),
        {"request_ad": fop_h(2)}, done("AD"), clcw(201),
    ])
    out.append(("fop1", "cop1-fop1-initiate-set-vr", "Initiate AD with Set V(R) 200 sets V(S) "
                "and NN(R) to 200, sends the Set V(R) frame, and the next AD frame carries "
                "N(S) 200.", FOP_CFG, inputs, ("COP-010",)))

    # Modulo 256: sequence numbers wrap (COP-012).
    inputs = timed([
        directive(1, "set_v_s", 254), directive(2, "initiate_ad_without_clcw_check"),
        {"request_ad": fop_h(3)}, done("AD"),
        {"request_ad": fop_h(4)}, done("AD"),
        {"request_ad": fop_h(5)}, done("AD"),
        clcw(0), clcw(1), clcw(2),
    ])
    out.append(("fop1", "cop1-fop1-modulo-256", "Frames numbered 254, 255, 0; N(R) = 0 "
                "acknowledges the first two across the wrap; N(R) = 2 afterwards is invalid.", dict(FOP_CFG, k=3), inputs,
                ("COP-012",)))
    inputs = timed([
        directive(1, "set_v_s", 254), directive(2, "initiate_ad_without_clcw_check"),
        {"request_ad": fop_h(3)}, done("AD"),
        clcw(2),
    ])
    out.append(("fop1", "cop1-fop1-nr-invalid-wrap", "V(S) = 255 after one frame from 254: "
                "N(R) = 2 is outside NN(R)..V(S) modulo 256 and raises Alert[NN(R)].",
                FOP_CFG, inputs, ("COP-012",)))

    # The directive errors (COP-015).
    inputs = timed([
        directive(1, "set_k", 0), directive(2, "set_k", 256), directive(3, "set_k", 2),
        directive(4, "set_transmission_limit", 0), directive(5, "set_timeout_type", 2),
        directive(6, "set_t1_initial", 0), directive(7, "no_such_directive"),
        directive(8, "set_v_s", 256),
    ])
    out.append(("fop1", "cop1-fop1-directive-values", "A K outside 1..255, a Transmission_Limit "
                "of 0, a Timeout_Type above 1, a T1_Initial of 0, a V(S) above 255, and an "
                "unknown directive are rejected; K = 2 is accepted.", FOP_CFG, inputs,
                ("COP-015",)))

    # A CLCW for another virtual channel is a routing error (cop1.md 2.2).
    inputs = timed([
        directive(1, "initiate_ad_without_clcw_check"),
        clcw(0, vcid=2), clcw(0, retransmit=True, vcid=2),
    ])
    out.append(("fop1", "cop1-fop1-other-vcid", "A CLCW for another virtual channel is refused "
                "with UnknownChannel and changes nothing.", FOP_CFG, inputs, ("API-058",)))

    # The invalid bit patterns of E15.
    inputs = timed([
        directive(1, "initiate_ad_without_clcw_check"),
        clcw(0, control_word_type=1),
    ])
    out.append(("fop1", "cop1-fop1-clcw-pattern-type", "A word whose Control Word Type is 1 is "
                "an invalid pattern (E15) and raises Alert[CLCW].", FOP_CFG, inputs, ()))
    inputs = timed([
        directive(1, "initiate_ad_without_clcw_check"),
        clcw(0, cop_in_effect=2),
    ])
    out.append(("fop1", "cop1-fop1-clcw-pattern-cop", "A word whose COP in Effect is not 01 is "
                "an invalid pattern (E15) and raises Alert[CLCW].", FOP_CFG, inputs, ()))

    # A BD frame needs no Sent_Queue and no timer (COP-013).
    inputs = timed([
        {"request_bd": fop_h(1)}, {"request_bd": fop_h(2)}, done("BD"), {"request_bd": fop_h(3)},
    ])
    out.append(("fop1", "cop1-fop1-bd-service", "BD frames carry N(S) 0, do not start the "
                "timer, and a request is refused while the Lower Procedures hold the previous "
                "frame.", FOP_CFG, inputs, ("COP-010", "COP-013")))

    # Terminate with FDUs outstanding purges with negative confirmations (5.2.15).
    inputs = timed([
        directive(1, "initiate_ad_without_clcw_check"),
        {"request_ad": fop_h(2)}, done("AD"),
        {"request_ad": fop_h(3)}, done("AD"),
        {"request_ad": fop_h(4)}, done("AD"),
        {"request_ad": fop_h(5)},
        directive(6, "terminate_ad"),
    ])
    out.append(("fop1", "cop1-fop1-terminate-purge", "Terminate AD with three frames outstanding "
                "and one FDU waiting: negative confirmations in queue order, a reject for the "
                "waiting FDU, Alert[Term], then the confirmation of the directive.",
                dict(FOP_CFG, k=3), inputs, ("COP-017",)))

    # An Alert is not followed by another until Initiate AD (COP-017).
    inputs = timed([
        directive(1, "initiate_ad_without_clcw_check"),
        {"request_ad": fop_h(2)},
        clcw(0, lockout=True), clcw(0, lockout=True), clcw(5), clcw(0, wait=True),
        done("AD", "reject"),
        directive(2, "initiate_ad_without_clcw_check"),
        clcw(0, lockout=True),
    ])
    out.append(("fop1", "cop1-fop1-alert-suppression", "After Alert[Lockout] the CLCWs that would "
                "alert again are ignored; AD_Reject still raises Alert[LLIF]; a new Initiate AD "
                "lets the next Alert through.", FOP_CFG, inputs, ("COP-017",)))

    return out


def extras_farm():
    out = []
    cfg = FARM_CFG

    def ad(ns, buf=True, data="C0DE"):
        return {"frame_hex": farm_frame(cfg, "AD", ns, bytes.fromhex(data)),
                "buffer_available": buf}

    def bd(data="BD01"):
        return {"frame_hex": farm_frame(cfg, "BD", 0, bytes.fromhex(data)),
                "buffer_available": False}

    def bc(data):
        return {"frame_hex": farm_frame(cfg, "BC", 0, data), "buffer_available": True}

    q = {"query": "clcw"}

    # Window edges with W = 8 (PW = NW = 4) and V(R) = 0 (COP-030).
    inputs = timed([{"frame": ad(3)}, {"frame": ad(4)}, q])
    out.append(("farm1", "cop1-farm1-window-positive-edge", "N(S) = V(R) + PW - 1 is in the "
                "positive window (Retransmit set); V(R) + PW is outside and locks out.", cfg,
                inputs, ("COP-030",)))
    inputs = timed([{"frame": ad(0)}, {"frame": ad(253)}, {"frame": ad(0)},
                    {"frame": ad(252)}, q])
    out.append(("farm1", "cop1-farm1-window-negative-edge", "After V(R) = 1, N(S) = V(R) - NW and "
                "V(R) - 1 are in the negative window; V(R) - NW - 1 is outside and locks out.",
                cfg, inputs, ("COP-030",)))

    # V(R) wraps (COP-030, COP-035).
    cfg255 = dict(cfg, v_r=255)
    inputs = timed([{"frame": farm_frame_for(cfg255, 255)}, q, {"frame": farm_frame_for(cfg255, 0)}, q])
    out.append(("farm1", "cop1-farm1-vr-wrap", "V(R) 255 accepts N(S) 255 and wraps to 0.", cfg255,
                inputs, ("COP-030", "COP-035")))

    # FARM-B counter is modulo 4 (COP-034).
    inputs = timed([{"frame": bd()}, {"frame": bd()}, {"frame": bd()}, {"frame": bd()},
                    {"frame": bd()}, q, {"frame": bc(UNLOCK)}, q])
    out.append(("farm1", "cop1-farm1-farm-b-counter", "Each Type-BD and each valid Type-BC "
                "frame increments the FARM-B Counter, modulo 4 in the CLCW; a BD frame is "
                "accepted whether or not a buffer is available.", cfg, inputs, ("COP-034",)))

    # Buffer flow control (COP-033).
    inputs = timed([{"frame": ad(0, False)}, q, {"frame": ad(0, True)}, {"buffer_released": {}},
                    q, {"frame": ad(0, True)}, q, {"frame": ad(1, True)}, q])
    out.append(("farm1", "cop1-farm1-buffer-wait", "No buffer for the expected frame: Wait and "
                "Retransmit set; the frame is refused until the buffer is released; the "
                "retransmission is then accepted and Retransmit clears.", cfg, inputs,
                ("COP-033",)))

    # The Set V(R) command in Lockout is counted and not executed (6.3.2.2) (COP-031, COP-036).
    inputs = timed([{"frame": ad(100)}, {"frame": bc(SET_VR_PREFIX + bytes([50]))}, q,
                    {"frame": bc(UNLOCK)}, q, {"frame": bc(SET_VR_PREFIX + bytes([50]))}, q])
    out.append(("farm1", "cop1-farm1-set-vr-in-lockout", "In Lockout a Set V(R) command is "
                "counted by the FARM-B Counter but not executed; Unlock leaves Lockout; Set V(R) "
                "then sets V(R).", cfg, inputs, ("COP-031", "COP-036")))

    # BC frames that are not commands (COP-031).
    inputs = timed([{"frame": bc(bytes([0x01]))}, {"frame": bc(bytes([0x82, 0x00]))},
                    {"frame": bc(bytes([0x82, 0x01, 0x05]))}, {"frame": bc(bytes([0x00, 0x00]))},
                    {"invalid_frame": {}}, q])
    out.append(("farm1", "cop1-farm1-bc-not-a-command", "A Type-BC frame whose data is neither "
                "Unlock nor Set V(R) is an invalid frame (E9) and does not count.", cfg, inputs,
                ("COP-031",)))

    # Retransmission not allowed (Cor. 1 6.1.8.3.2): PW > NW.
    cfgna = dict(cfg, window={"retransmission_allowed": False, "w": 8, "pw": 7})
    inputs = timed([{"frame": farm_frame_for(cfgna, 6)}, {"frame": farm_frame_for(cfgna, 255)},
                    {"frame": farm_frame_for(cfgna, 254)}, q])
    out.append(("farm1", "cop1-farm1-window-not-allowed", "W = 8, PW = 7, NW = 1: N(S) = 6 is "
                "positive, 255 is the negative window, 254 is outside and locks out.", cfgna,
                inputs, ("COP-030",)))

    # The physical flags reach the CLCW (COP-035).
    inputs = timed([q, {"set_physical_flags": {"no_rf_available": True, "no_bit_lock": False}}, q,
                    {"set_physical_flags": {"no_rf_available": False, "no_bit_lock": True}}, q])
    out.append(("farm1", "cop1-farm1-physical-flags", "No RF Available and No Bit Lock are set "
                "from outside and appear in the CLCW; they are not part of the state table.",
                cfg, inputs, ("COP-035",)))

    # A CLCW with a status field and another VCID (COP-035).
    cfgs = dict(cfg, status_field=5, vcid=17)
    inputs = timed([q, {"frame": farm_frame_for(cfgs, 0)}, q])
    out.append(("farm1", "cop1-farm1-clcw-fields", "The CLCW carries COP in Effect 01, the "
                "configured VCID and Status Field, and V(R).", cfgs, inputs, ("COP-035",)))
    return out


def farm_frame_for(cfg, ns, data="C0DE", buf=True):
    return {"frame_hex": farm_frame(cfg, "AD", ns, bytes.fromhex(data)), "buffer_available": buf}


# --------------------------------------------------------------------------------------------
# Generating every file
# --------------------------------------------------------------------------------------------

# Topic files for the scenarios that are not cells: file name -> scenario ids.
TOPICS = {
    "fop1_ad_service.json": ["ad-run", "retransmit-in-order", "initiate-unlock",
                             "initiate-set-vr", "bd-service", "terminate-purge"],
    "fop1_window_timer.json": ["window-k", "wait-queue", "timer-and-limit", "suspend-resume"],
    "fop1_wrap_directives.json": ["modulo-256", "nr-invalid-wrap", "directive-values"],
    "fop1_clcw.json": ["other-vcid", "clcw-pattern-type", "clcw-pattern-cop",
                       "alert-suppression"],
    "farm1_window.json": ["window-positive-edge", "window-negative-edge", "vr-wrap",
                          "window-not-allowed"],
    "farm1_flow_clcw.json": ["farm-b-counter", "buffer-wait", "set-vr-in-lockout",
                             "bc-not-a-command", "physical-flags", "clcw-fields"],
}

# A first step shared by many scenarios, kept as a named setup.
INITIATE = (0, {"directive": {"handle": 1, "type": "initiate_ad_without_clcw_check"}})


def compile_file(engine, file_cfg, raws, how):
    """Build one file. `raws` are (id, description, cfg, inputs, split, cell, extra_reqs): the
    first `split` inputs are the setup chain, shared by name between the scenarios that start
    with the same inputs."""
    setups = []
    names = {}  # tuple of the setup's inputs -> name
    scenarios = []
    for sid, desc, cfg, inputs, split, cell, extra in raws:
        steps, reached, _ = replay(engine, cfg, inputs)
        chain = None
        for i in range(1, split + 1):
            key = tuple(json.dumps(x, sort_keys=True) for x in inputs[:i])
            if key not in names:
                state = steps[i - 1]["expect_state"]
                digest = hashlib.sha1("\n".join(key).encode()).hexdigest()[:8]
                name = "%s-%s" % (state, digest)
                names[key] = name
                entry = {"name": name}
                if chain is not None:
                    entry["extends"] = chain
                entry["steps"] = [steps[i - 1]]
                setups.append(entry)
            chain = names[key]
        sc = {"id": sid, "description": desc,
              "requirements": requirements_for(engine, cell, steps, reached, extra)}
        if chain is not None:
            sc["setup"] = chain
        if cfg != file_cfg:
            sc["config"] = cfg
        sc["steps"] = steps[split:]
        scenarios.append(sc)
    doc = {"kind": "engine-scenario", "engine": engine, "provenance": "hand-derived",
           "provenance_note": provenance(engine, how), "config": file_cfg}
    if setups:
        doc["setups"] = setups
    doc["scenarios"] = scenarios
    return doc


def all_files():
    """Every file as {relative name: document}, and the cells found."""
    files = {}
    fop_found, _ = search_fop(FOP_CFG)
    raws = []
    for event in FOP_EVENTS:
        for state in STATES:
            if (state, event) in fop_found:
                path, trigger = fop_found[(state, event)]
                sid, desc, inputs, split = cell_scenario("fop1", state, event, path, trigger)
                raws.append((sid, desc, FOP_CFG, inputs, split, True, ()))
    files["scenarios/fop1_table.json"] = compile_file("fop1", FOP_CFG, raws, SETUP_HOW)
    farm_found, _ = search_farm(FARM_CFG)
    raws = []
    for event in FARM_EVENTS:
        for state in FARM_STATES:
            if (state, event) in farm_found:
                path, trigger = farm_found[(state, event)]
                sid, desc, inputs, split = cell_scenario("farm1", state, event, path, trigger)
                raws.append((sid, desc, FARM_CFG, inputs, split, True, ()))
    files["scenarios/farm1_table.json"] = compile_file("farm1", FARM_CFG, raws, SETUP_HOW)
    extras = {e[1].replace("cop1-fop1-", "").replace("cop1-farm1-", ""): e
              for e in extras_fop() + extras_farm()}
    listed = [i for ids in TOPICS.values() for i in ids]
    assert sorted(listed) == sorted(extras), (set(listed) ^ set(extras))
    for name, ids in TOPICS.items():
        engine = "fop1" if name.startswith("fop1") else "farm1"
        file_cfg = FOP_CFG if engine == "fop1" else FARM_CFG
        raws = []
        for i in ids:
            _, sid, desc, cfg, inputs, reqs = extras[i]
            split = 1 if engine == "fop1" and inputs[0] == INITIATE else 0
            raws.append((sid, desc, cfg, inputs, split, False, reqs))
        files["scenarios/" + name] = compile_file(engine, file_cfg, raws, SCRIPT_HOW)
    return files, fop_found, farm_found


def missing_cells(fop_found, farm_found):
    miss = []
    for event in FOP_EVENTS:
        for state in STATES:
            if (state, event) not in fop_found:
                miss.append(("fop1", state, event))
    for event in FARM_EVENTS:
        for state in FARM_STATES:
            if (state, event) not in farm_found:
                miss.append(("farm1", state, event))
    return miss


def root_dir():
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.normpath(os.path.join(here, "..", ".."))


def dump(obj, level=0):
    """Deterministic text: a file, a setup, or a scenario is expanded, one key per line; a step
    and every other small object is one line; the steps of a list go one to a line."""
    pad = "  " * level
    inner = "  " * (level + 1)
    if isinstance(obj, dict) and ("kind" in obj or "name" in obj or "id" in obj):
        items = []
        for k, v in obj.items():
            items.append('%s%s: %s' % (inner, json.dumps(k), dump(v, level + 1)))
        return "{\n" + ",\n".join(items) + "\n" + pad + "}"
    if isinstance(obj, list) and obj and all(isinstance(x, dict) for x in obj) and \
            not (len(obj) and "at_us" in obj[0] and False):
        items = [inner + dump(x, level + 1) for x in obj]
        return "[\n" + ",\n".join(items) + "\n" + pad + "]"
    return json.dumps(obj, separators=(", ", ": "))


def render(doc):
    return dump(doc) + "\n"


def write_all(outdir=None):
    files, fop_found, farm_found = all_files()
    base = outdir or os.path.join(root_dir(), "test-vectors", "cop1")
    os.makedirs(os.path.join(base, "scenarios"), exist_ok=True)
    # Remove what the generator no longer writes.
    for top, _, names in os.walk(base):
        for name in sorted(names):
            rel = os.path.relpath(os.path.join(top, name), base)
            if name.endswith(".json") and rel not in files:
                os.remove(os.path.join(top, name))
    for name, doc in files.items():
        with open(os.path.join(base, name), "w") as f:
            f.write(render(doc))
    miss = missing_cells(fop_found, farm_found)
    print("wrote %d file(s); %d cell(s) not reached: %s" % (len(files), len(miss), miss))
    return 0


# --------------------------------------------------------------------------------------------
# --check
# --------------------------------------------------------------------------------------------

def parse_id(sid):
    """('fop1', 'S3', 10) for the id of a table cell, else None."""
    parts = sid.split("-")
    if len(parts) == 4 and parts[0] == "cop1" and parts[1] in ("fop1", "farm1") and \
            parts[2].startswith("s") and parts[3].startswith("e"):
        return parts[1], parts[2].upper(), int(parts[3][1:])
    return None


def check_file(path, generated):
    problems = []
    with open(path) as f:
        doc = json.load(f)
    engine = doc["engine"]
    by_name = {s["name"]: s for s in doc.get("setups", [])}
    seen_ids = set()
    for sc in doc["scenarios"]:
        sid = sc["id"]
        if sid in seen_ids:
            problems.append("%s: scenario id twice" % sid)
        seen_ids.add(sid)
        if sc.get("superseded"):
            continue
        chain = []
        name = sc.get("setup")
        while name is not None:
            if name not in by_name or len(chain) > len(by_name):
                problems.append("%s: the setup %r is missing or loops" % (sid, name))
                break
            chain.append(by_name[name])
            name = by_name[name].get("extends")
        stored = [st for s in reversed(chain) for st in s["steps"]] + sc["steps"]
        cfg = sc.get("config", doc["config"])
        inputs = [(st["at_us"], st["input"]) for st in stored]
        steps, reached, _ = replay(engine, cfg, inputs)
        for i, (want, got) in enumerate(zip(steps, stored)):
            if want != got:
                problems.append("%s: step %d: model says %s, file says %s" %
                                (sid, i, json.dumps(want), json.dumps(got)))
        cell = parse_id(sid)
        if cell:
            _, state, event = cell
            trigger = [r for r in reached if r[0] is not None][-1]
            if trigger != (state, event):
                problems.append("%s: the trigger reaches %s, not the cell of the id" %
                                (sid, trigger))
    rel = os.path.relpath(path, os.path.join(root_dir(), "test-vectors", "cop1"))
    if rel not in generated:
        problems.append("%s: not a file the generator writes" % rel)
    elif generated[rel] != doc:
        problems.append("%s: differs from what the generator writes (run --write)" % rel)
    return problems


def selftest():
    assert crc16(b"123456789") == 0x29B1  # core/crc16.json, CMN-002
    # A CLCW with V(R) 5, Retransmit set on VCID 1 (232.0 4.2.1).
    assert clcw_word(0, 0, 1, 0, 5, 1).hex() == "01040805"


def check(paths):
    selftest()
    problems = []
    generated, _, _ = all_files()
    for p in paths:
        problems.extend(check_file(p, generated))
    for p in problems:
        print("error:", p)
    print("cop1 model: %d file(s), %d problem(s)" % (len(paths), len(problems)))
    return 1 if problems else 0


# --------------------------------------------------------------------------------------------
# --audit
# --------------------------------------------------------------------------------------------

def audit():
    """Search for an input that reaches a Not applicable cell."""
    _, fop_queue = search_fop(FOP_CFG)
    _, farm_queue = search_farm(FARM_CFG)
    bad = []
    for eng, path, now in fop_queue:
        handle = len(path) + 1
        for move in FOP_MOVES + FOP_TRIGGERS:
            inp = resolve_fop(eng, move, handle)
            if inp is None:
                continue
            try:
                event = eng.classify(inp, next_time(eng, now, inp))
            except Err:
                continue
            if event is not None and TABLE_FOP[event][STATES.index(eng.state)] is None:
                bad.append(("fop1", eng.state, event, inp))
    print("FOP-1: bounded search, depth %d, %d node(s), alphabet of %d input(s); %d Not "
          "applicable cell(s) reached (evidence, not a proof)" %
          (max(len(p) for _, p, _ in fop_queue), len(fop_queue),
           len(FOP_MOVES) + len(FOP_TRIGGERS), len({(s, e) for _, s, e, _ in bad})))
    for item in bad[:10]:
        print("  ", item)
    farm_na = set()
    for eng, path, now in farm_queue:
        for inp in farm_inputs(eng):
            if "query" in inp:
                continue
            try:
                event = eng.classify(inp)
            except Err:
                continue
            if event is not None and TABLE_FARM[event][FARM_STATES.index(eng.state)] is None:
                farm_na.add((eng.state, event))
    print("FARM-1: bounded search, depth %d, %d node(s); Not applicable cell(s) reached by an "
          "input: %s" % (max(len(p) for _, p, _ in farm_queue), len(farm_queue),
                         sorted(farm_na)))
    return 0


def main(argv):
    if len(argv) >= 2 and argv[1] == "--check":
        return check(argv[2:])
    if len(argv) in (2, 3) and argv[1] == "--write":
        return write_all(argv[2] if len(argv) == 3 else None)
    if len(argv) == 2 and argv[1] == "--audit":
        return audit()
    print(__doc__.split("Usage")[1].split("--check recomputes")[0] if False else
          "usage: cop1.py --check FILE... | --write | --audit")
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
