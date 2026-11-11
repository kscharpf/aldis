# YAMCS black box (AINT)

The configuration this project writes for the independent ground system of the AINT milestone ([plan](../../../docs/milestones/aint-interoperability.md) D1, D2; [specification §11](../../../docs/reference-application.md#11-interoperability-profile-aint)). Nothing of YAMCS is in the repository: the release is fetched and an image is built locally. No YAMCS source is read.

| File | Use |
|---|---|
| `Containerfile` | The image: the release archive on a Java runtime |
| `etc/yamcs.yaml`, `etc/yamcs.aldis.yaml` | Server and instance: the `UdpTmFrameLink` of §11.3 (SCID `0x155`, 223 octets, CRC16, VC 0, 1, 7) |
| `etc/aldis-streams.sql` | Tables that record the frame, CLCW, and packet streams, to read a run back through the HTTP API (no mission database) |
| `replay-uplink.py` | TM-only spike: sends the uplink frames of a golden trace to the flight node, in place of YAMCS's commanding |
| `readback.py`, `malformed-frames.py` | Compare a run with its run log; send malformed frames to the link |

From the repository root, with podman or docker (`CONTAINER_TOOL` names one):

```sh
scripts/interop-yamcs build "$SCRATCH/yamcs-dl"     # fetch 5.14.0, check the SHA-256, build aldis-yamcs:5.14.0
scripts/interop-yamcs up "$SCRATCH/yamcs-data"      # HTTP API 127.0.0.1:8090, TM on UDP 127.0.0.1:10015
(cd refapp && cargo run --release -p refapp-interop -- run --case a3-nominal \
    --tm 127.0.0.1:10015 --tc-listen 127.0.0.1:10025 --log target/runs/a3-nominal.interop.jsonl)
python3 -I refapp/interop/yamcs/replay-uplink.py refapp/golden/a3-nominal.jsonl 127.0.0.1:10025  # at the same time
python3 -I refapp/interop/yamcs/readback.py refapp/target/runs/a3-nominal.interop.jsonl
scripts/interop-yamcs down
```

Use a new data directory for each run: the recording tables keep every run since the directory was created.
