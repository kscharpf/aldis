"""The ops script of the nominal timeline, run through YAMCS's own command chain (AINT WP-4, plan D7,
docs/reference-application.md section 2.4 and 11.9).

It waits for the first TM frame to arrive (the driver sends its first frame at its instant 0), takes that
moment as its instant 0, starts COP-1 (Initiate AD Service without CLCW check) and issues each command of the timeline at
its release instant through the HTTP API (`POST /api/processors/aldis/realtime/commands/<name>`), the
command names and arguments being those of etc/mdb/aldis.xml. The frames and COP-1 are YAMCS's; YAMCS
does not segment (section 11.9). It writes one JSON line per event to the file given, with the instant in seconds since the
script started, polls the COP-1 status every 0.25 s, and at the end reads the command history back.

Usage: python3 -I ops.py <log.jsonl> [--api http://127.0.0.1:8090] [--end 35] [--table 300|100]
       [--shift NAME=SECONDS]...
       [--timeout-type SUSPEND|UNINITIALIZE]
Standard library only (plan D4).
"""

import argparse
import json
import sys
import time
import urllib.error
import urllib.request

INSTANCE = "aldis"
LINK = "aldis-tc.vc0"
T0 = 2_100_000_000  # on-board T0 in seconds (section 5)

def table(length):
    return bytes((3 * i + 1) % 256 for i in range(length)).hex().upper()  # section 3.3.6

# (release s, command name, arguments): the entries of section 2.4 from 1.0 s on.
SCRIPT = [
    (1.0, "NOOP", {"seq_count": 0, "t_coarse": T0 + 6}),
    (2.0, "TABLE_UPLOAD", {"seq_count": 0, "table_id": 1, "table": table(300)}),
    (3.0, "ACTIVATE_TABLE", {"seq_count": 1, "t_coarse": T0 + 10, "table_id": 1, "crc": 0x0C4F}),
    (4.0, "DUMP_MEMORY", {"seq_count": 2, "t_coarse": T0 + 12, "address": 0x00010000, "length": 600}),
    (5.0, "SET_PAYLOAD_MODE", {"seq_count": 3, "t_coarse": T0 + 20, "mode": 0}),
    (5.0, "NOOP", {"seq_count": 4, "t_coarse": T0 + 20}),
    (6.0, "SET_PAYLOAD_MODE", {"seq_count": 5, "t_coarse": T0 + 24, "mode": 1}),
]


# The variant of the table that YAMCS can send (it does not segment, section 11.9): 100 octets in
# one frame, with the CRC-16 (CMN-001) of those octets.
SHORT_TABLE = {"TABLE_UPLOAD": ("TABLE_UPLOAD_SHORT", {"table": table(100)}), "ACTIVATE_TABLE": (None, {"crc": 0xF6C5})}


def call(method, url, body=None):
    data = None if body is None else json.dumps(body).encode()
    request = urllib.request.Request(url, data=data, method=method)
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            text = response.read().decode()
            return response.status, (json.loads(text) if text else None)
    except urllib.error.HTTPError as error:
        text = error.read().decode()
        try:
            return error.code, json.loads(text)
        except ValueError:
            return error.code, text


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("log")
    parser.add_argument("--api", default="http://127.0.0.1:8090")
    parser.add_argument("--end", type=float, default=35.0)
    parser.add_argument("--timeout-type", default="UNINITIALIZE", choices=["SUSPEND", "UNINITIALIZE"],
                        help="Timeout_Type of the profile is 0 (UNINITIALIZE)")
    parser.add_argument("--shift", action="append", default=[], metavar="NAME=SECONDS",
                        help="release the command NAME later by SECONDS (plan D7: the instants are not compared)")
    parser.add_argument("--table", choices=["300", "100"], default="300",
                        help="300: the table of section 2.4 (YAMCS cannot send it); 100: the variant of one frame")
    opts = parser.parse_args()
    cop1 = f"{opts.api}/api/cop1/{INSTANCE}/{LINK}"
    commands = f"{opts.api}/api/processors/{INSTANCE}/realtime/commands"
    out = open(opts.log, "w", encoding="utf-8")
    while True:
        status, body = call("GET", f"{opts.api}/api/links/{INSTANCE}/aldis-tm")
        if status == 200 and int(body.get("dataInCount", "0")) > 0:
            break
        time.sleep(0.005)
    start = time.monotonic()

    def emit(kind, **fields):
        record = {"kind": kind, "t": round(time.monotonic() - start, 4), "wall": time.time(), **fields}
        out.write(json.dumps(record) + "\n")
        out.flush()

    def wait_until(t):
        while True:
            now = time.monotonic() - start
            if now >= t:
                return
            time.sleep(min(0.05, t - now))

    def poll():
        status, body = call("GET", cop1 + "/status")
        emit("cop1_status", http=status, status=body)

    emit("start", table=opts.table, shift=opts.shift)
    status, body = call("GET", cop1 + "/config")
    emit("cop1_config", http=status, config=body)
    if opts.timeout_type:
        status, body = call("PATCH", cop1 + "/config", {"timeoutType": opts.timeout_type})
        emit("cop1_config_update", http=status, config=body)
    status, body = call("POST", cop1 + ":initialize", {"type": "WITHOUT_CLCW_CHECK"})
    emit("cop1_initialize", http=status, response=body)
    poll()

    pending = []
    for t, name, args in SCRIPT:
        if opts.table == "100" and name in SHORT_TABLE:
            new_name, changes = SHORT_TABLE[name]
            name, args = new_name or name, {**args, **changes}
        for item in opts.shift:
            shifted, seconds = item.split("=")
            if shifted == name:
                t += float(seconds)
        pending.append((t, name, args))
    pending.sort(key=lambda entry: entry[0])
    next_poll = 0.25
    while True:
        now = time.monotonic() - start
        if now >= opts.end:
            break
        if pending and now >= pending[0][0]:
            _, name, args = pending.pop(0)
            status, body = call("POST", f"{commands}/Aldis/{name}", {"args": args})
            emit("command", name=name, http=status, response=body)
            continue
        if now >= next_poll:
            poll()
            next_poll += 0.25
            continue
        time.sleep(0.01)
    poll()
    status, body = call("GET", f"{opts.api}/api/archive/{INSTANCE}/commands?order=asc&limit=100")
    emit("command_history", http=status, response=body)
    status, body = call("GET", f"{opts.api}/api/links/{INSTANCE}")
    emit("links", http=status, response=body)
    out.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
