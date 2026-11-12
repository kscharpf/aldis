"""Replays the uplink frames of a golden trace to the flight node's TC port, at their trace instants.

A stand-in for the ground system's commanding in the TM-only spike (docs/milestones/
aint-interoperability.md WP-3): the frames are the Rust ground node's own, taken from the `unit_sent`
records with `"dir":"up"`. Start it right after the driver; no reaction to the CLCW, no retransmission.

Usage: python3 -I replay-uplink.py <golden-trace.jsonl> <host:port>
"""

import json
import socket
import sys
import time


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    host, port = sys.argv[2].rsplit(":", 1)
    frames = []
    with open(sys.argv[1], encoding="utf-8") as trace:
        for line in trace:
            rec = json.loads(line)
            if rec.get("kind") == "unit_sent" and rec.get("dir") == "up":
                frames.append((rec["t_us"], bytes.fromhex(rec["hex"])))
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    start = time.monotonic()
    for t_us, frame in frames:
        wait = start + t_us / 1e6 - time.monotonic()
        if wait > 0:
            time.sleep(wait)
        sock.sendto(frame, (host, int(port)))
    print(f"sent {len(frames)} frames", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
