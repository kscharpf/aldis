"""Sends malformed TM frames to the YAMCS frame link and reports what it counts (AINT WP-3).

Usage: python3 -I malformed-frames.py <run-log.jsonl>   (the log of a run, for a sample frame)
"""
import json, socket, sys, time, urllib.request

def crc(b):
    c = 0xFFFF
    for x in b:
        c ^= x << 8
        for _ in range(8):
            c = ((c << 1) ^ 0x1021) & 0xFFFF if c & 0x8000 else (c << 1) & 0xFFFF
    return c

log = [json.loads(l) for l in open(sys.argv[1])]
f = bytes.fromhex([r["hex"] for r in log if r["kind"] == "datagram" and r["dir"] == "down"
                   and r["event"] == "sent"][2])
assert crc(f[:-2]) == int.from_bytes(f[-2:], "big"), "FECF of the sample is CRC-16/CCITT-FALSE"
print("sample frame FECF verifies as CRC-16 (poly 0x1021, init 0xFFFF):", True)
def fix(b):
    return b[:-2] + crc(b[:-2]).to_bytes(2, "big")
cases = {
    "bit flip in data (stale FECF)": f[:50] + bytes([f[50] ^ 1]) + f[51:],
    "SCID 0x156, FECF recomputed": fix(bytes([f[0], (f[1] & 0x0F) | 0x60]) + f[2:]),
    "VCID 5 (not configured), FECF recomputed": fix(bytes([f[0], (f[1] & 0xF1) | (5 << 1)]) + f[2:]),
    "version 1, FECF recomputed": fix(bytes([(f[0] & 0x3F) | 0x40]) + f[1:]),
    "222 octets": f[:-1],
    "224 octets": f + b"\x00",
    "attached sync marker 1ACFFC1D + frame (227 octets)": bytes.fromhex("1ACFFC1D") + f,
}
api = "http://127.0.0.1:8090/api/links/aldis/aldis-tm"
def counts():
    j = json.load(urllib.request.urlopen(api))
    return j["extra"]["Valid frames"], j["extra"]["Invalid frames"], j.get("dataInCount")
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
for name, frame in cases.items():
    before = counts()
    s.sendto(frame, ("127.0.0.1", 10015))
    time.sleep(0.6)
    print(f"{name}: length {len(frame)}, (valid, invalid, in) {before} -> {counts()}")
