# YAMCS black box (AINT)

The configuration this project writes for the independent ground system of the AINT milestone ([plan](../../../docs/milestones/aint-interoperability.md) D1, D2; [specification §11](../../../docs/reference-application.md#11-interoperability-profile-aint)). Nothing of YAMCS is in the repository: the release is fetched and an image is built locally. No YAMCS source is read.

| File | Use |
|---|---|
| `Containerfile` | The image: the release archive on a Java runtime |
| `etc/yamcs.yaml`, `etc/yamcs.aldis.yaml` | Server and instance: the `UdpTmFrameLink` of §11.3 (SCID `0x155`, 223 octets, CRC16, VC 0, 1, 7) and the `UdpTcFrameLink` with COP-1 (VC 0, MAP 0, K = 4, T1 = 1 s, transmission limit 3) |
| `etc/processor.yaml` | The processor: the command chain only |
| `etc/mdb/aldis.xml` | The mission database, in the public XTCE format: the commands of §3.3.5 and §3.3.6 as Space Packets (WP-4); a 100-octet table command for the variant YAMCS can send |
| `etc/aldis-streams.sql` | Tables that record the frame, CLCW, and packet streams, to read a run back through the HTTP API |
| `replay-uplink.py` | TM-only spike (WP-3): sends the uplink frames of a golden trace to the flight node, in place of YAMCS's commanding |
| `ops.py` | The ops script of the nominal timeline (§2.4), run through YAMCS's HTTP API: sets the timeout type, starts COP-1, issues each command at its release instant, reads the command history back |
| `judge.py` | Lists the facts of the pass criteria (§11.5) from a run: the driver's run log, the ops log, the golden trace, YAMCS's log |
| `readback.py`, `malformed-frames.py` | Compare a run with its run log; send malformed frames to the link |

From the repository root, with podman or docker (`CONTAINER_TOOL` names one; the TC link reaches the host as `host.containers.internal`, which docker names `host.docker.internal`: change `host` in `etc/yamcs.aldis.yaml`):

```sh
scripts/interop-yamcs build "$SCRATCH/yamcs-dl"     # fetch 5.14.0, check the SHA-256, build aldis-yamcs:5.14.0
scripts/interop-yamcs up "$SCRATCH/yamcs-data"      # HTTP API 127.0.0.1:8090, TM on UDP 127.0.0.1:10015
# COP-1 cases (WP-4): start the ops script first, it waits for the first TM frame; then the driver.
python3 -I refapp/interop/yamcs/ops.py "$SCRATCH/run.ops.jsonl" --table 100 --timeout-type UNINITIALIZE   # in one terminal
(cd refapp && cargo run --release -p refapp-interop -- run --case a3-nominal \
    --tm 127.0.0.1:10015 --tc-listen 127.0.0.1:10025 --log "$SCRATCH/run.interop.jsonl")
python3 -I refapp/interop/yamcs/judge.py "$SCRATCH/run.interop.jsonl" "$SCRATCH/run.ops.jsonl" refapp/golden/a3-nominal.jsonl
scripts/interop-yamcs down
```

`--table 100` is the table YAMCS can send (it does not segment; docs/reference-application.md §11.9); without it the table of 300 octets is refused. The lossy-link case adds `--shift DUMP_MEMORY=0.3` to `ops.py` and `--drop up:5,up:9,down:25,down:26,down:27,down:28,down:29,down:30,down:31,down:32,down:33,down:34,down:35` to the driver (§11.6). Start a fresh YAMCS (a new data directory) for each run: COP-1 starts only from its uninitialised state, and the recording tables keep every run since the directory was created.

The TM-only spike of WP-3 (no ops script; `replay-uplink.py` stands in for the uplink):

```sh
python3 -I refapp/interop/yamcs/replay-uplink.py refapp/golden/a3-nominal.jsonl 127.0.0.1:10025  # at the same time as the driver
python3 -I refapp/interop/yamcs/readback.py refapp/target/runs/a3-nominal.interop.jsonl
```
