"""Reads a TM run back from YAMCS (HTTP API, the tables of etc/aldis-streams.sql) and compares it
with the driver's run log (AINT WP-3, docs/reference-application.md section 11.8).

Usage: python3 -I readback.py <run-log.jsonl> [http://127.0.0.1:8090]
Run it against a fresh YAMCS data directory: the tables hold every run since it was created.
"""
import base64, json, sys, urllib.request, collections

API = (sys.argv[2] if len(sys.argv) > 2 else "http://127.0.0.1:8090") + "/api/archive/aldis:executeSql"

def sql(stmt):
    req = urllib.request.Request(API, json.dumps({"statement": stmt}).encode())
    rs = json.load(urllib.request.urlopen(req))
    cols = [c["name"] for c in rs["columns"]]
    out = []
    for r in rs.get("rows", []):
        row = {}
        for c, v in zip(cols, r["values"]):
            row[c] = next((x for k, x in v.items() if k != "type"), None)
            if v["type"] == "BINARY":
                row[c] = base64.b64decode(row[c])
            elif v["type"].startswith("SINT") or v["type"] == "TIMESTAMP":
                row[c] = int(row[c] if v["type"] != "TIMESTAMP" else v["timestampValue"])
        out.append(row)
    return out

log = [json.loads(l) for l in open(sys.argv[1])]
sent = [r for r in log if r["kind"] == "datagram" and r["dir"] == "down" and r["event"] == "sent"]
pout = [r for r in log if r["kind"] == "packet_out"]
print("run log: header", {k: v for k, v in log[0].items() if k != "loss_script"})
print("run log: down frames sent", len(sent), "packet_out", len(pout),
      "slots_missed", [r for r in log if r["kind"] == "slots_missed"])

frames = sql("select vcid, frameSeq, scid, seq, rectime, data from frames_good")
bad = sql("select count(*) as n from frames_bad")
print("YAMCS: good frames", len(frames), "bad frames", bad[0] if bad else None)
byseq = sorted(frames, key=lambda r: r["seq"])
exp = [bytes.fromhex(r["hex"]) for r in sent]
got = [r["data"] for r in byseq]
print("frames identical to the driver's, in order:", got == exp, " (lengths", {len(g) for g in got}, ")")
vc_log = [(b[1] >> 1) & 7 for b in exp]
vc_got = [r["vcid"] for r in byseq]
print("VC of YAMCS rows equals VC in header octets:", vc_log == vc_got, collections.Counter(vc_got))
print("scid values:", {r["scid"] for r in byseq})
# OCF
ocf = [int.from_bytes(b[217:221], "big") for b in exp]
clcw = [r["clcw"] & 0xFFFFFFFF for r in sql("select n, clcw from clcw_log")]
print("CLCW rows", len(clcw), "frames", len(exp), "equal to OCFs in order:", clcw == ocf)
print("distinct OCF words:", sorted({hex(x) for x in ocf}))
# per-VC frame counters
for vc in (0, 1, 7):
    seqs = [r["frameSeq"] for r in byseq if r["vcid"] == vc]
    print(f"VC {vc}: frames {len(seqs)}, frameSeq first/last {seqs[:1]}/{seqs[-1:]}, "
          f"contiguous: {seqs == list(range(seqs[0], seqs[0] + len(seqs))) if seqs else None}")
# packets
exp_pk = collections.Counter(bytes.fromhex(r["hex"]) for r in pout)
got_pk = collections.Counter()
for vc in (0, 1, 7):
    for r in sql(f"select gentime, seqNum, packet from pkt_vc{vc}"):
        got_pk[r["packet"]] += 1
print("packets: generated", sum(exp_pk.values()), "extracted by YAMCS", sum(got_pk.values()),
      "equal as multisets:", exp_pk == got_pk)
print("by APID generated:", dict(collections.Counter(((p[0] & 7) << 8 | p[1]) for p in exp_pk.elements())))
print("by APID extracted:", dict(collections.Counter(((p[0] & 7) << 8 | p[1]) for p in got_pk.elements())))
print("missing:", [(p[:2].hex()) for p in (exp_pk - got_pk).elements()][:10],
      "unexpected:", [(p[:2].hex()) for p in (got_pk - exp_pk).elements()][:10])
# time of VC1 packets
rows = sql("select gentime, packet from pkt_vc1")
offs = collections.Counter()
for r in rows:
    p = r["packet"]
    coarse = int.from_bytes(p[6:10], "big")
    frac = int.from_bytes(p[10:12], "big")
    count_ms = coarse * 1000 + frac * 1000 / 65536  # the count since 1958-01-01, in ms
    # gentime is milliseconds in YAMCS's time scale; Unix(1958-01-01) = -378691200 s
    offs[round(r["gentime"] - count_ms + 378691200000, 3)] += 1
print("VC1 gentime minus (1958-01-01 + count), ms:", dict(offs))

# --- header counters, schedule, jitter
print("--- header checks")
ok_vc = {0: 0, 1: 0, 7: 0}
for r, b in zip(byseq, exp):
    ok_vc[r["vcid"]] += (r["frameSeq"] & 0xFF) == b[3]
print("YAMCS frameSeq equals the VC frame count octet of the header (frames):", ok_vc)
rec = [r["rectime"] for r in byseq]
d = [b - a for a, b in zip(rec, rec[1:])]
print("YAMCS rectime spacing ms: min", min(d), "max", max(d), "mean", sum(d) / len(d))
tl = [r["t_us"] for r in sent]
print("driver sent instants: slot-aligned:", all(t == i * 125000 for i, t in enumerate(tl)))
def pkts(b):
    fhp = int.from_bytes(b[4:6], "big") & 0x7FF
    out = []
    if fhp in (0x7FE, 0x7FF):
        return out
    i = 6 + fhp
    while i + 6 <= 217:
        ln = int.from_bytes(b[i + 4:i + 6], "big") + 7
        out.append(((b[i] & 7) << 8 | b[i + 1], b[i + 2] >> 6, ln))
        i += ln
    return out
print("--- schedule: index: VC, packets starting in the frame (apid, seq flags, length)")
for i, b in enumerate(exp):
    vc = (b[1] >> 1) & 7
    if vc != 7 or i < 8:
        print(i, f"t={i*0.125:g}s", "VC", vc, pkts(b))
