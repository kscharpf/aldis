# Reference application (`refapp/`)

An end-to-end flight↔ground application that uses Aldis only through its public API, as an integrator would. The scenario is specified in [docs/reference-application.md](../docs/reference-application.md) (requirement area APP); the plan for each milestone is in [docs/milestones/](../docs/milestones/) (`a0-…` to `a6-…`). Track A owns this directory ([roadmap](../docs/08-roadmap.md) §3, §6).

**Status: A3 COP-1 and AINT interoperability complete.** The flight node produces the telemetry packets and executes the commands of the scenario, and the ground node decodes them with the strict codecs; the two nodes exchange TM and TC frames built and received with the `aldis-link` engines, and since A3 the uplink is the Sequence-Controlled service of COP-1: the ground node runs `Fop1` and the flight node `Farm1`, which reports its state in the CLCW of every frame. The harness, trace writer, canonical-form check, comparator, and scenario generator work, and the golden traces of `a0-empty`, the four `a1-*` cases, the four `a2-*` cases, and the five `a3-*` cases are reproduced in CI; the `a3-*` traces are frozen (APP-004). Since AINT the flight node has also run against an independent ground system, YAMCS 5.14.0, through the driver `refapp-interop`: the nominal and the lossy-link COP-1 cases met their criteria with a 100-octet table (YAMCS does not segment), and every discrepancy is filed ([record](../docs/verification/records/APP-155.md)). The manually dispatched workflow `interop-yamcs.yml` repeats the run.

The spacecraft ID in [scenario/scenario.json](scenario/scenario.json) (`341`, `0x155`) is **fictional**, confirmed by the maintainer on 2026-10-19 ([OI-26](../docs/09-open-issues.md)).

## Layout

| Path | Contents |
|---|---|
| `refapp-scenario/` | `no_std`: the constants generated from the scenario file, and the node interface (`Node`, `StepInput`, `Output`, §8.1) |
| `refapp-flight/` | `no_std`, no heap: the flight node; flight lint set (APP-102) |
| `refapp-ground/` | The ground node; flight lint set's restriction lints (APP-103) |
| `refapp-harness/` | `std` tool: scheduler and link model, trace writer, canonical-form check, comparator, scenario generator; the harness side of the node protocol over a child's stdin and stdout and over loopback TCP, the cFS launcher, and the test node `refapp-test-node` (§8.2, §10.6) |
| `refapp-interop/` | `std` tool, no third-party dependency of its own (std plus workspace crates): the transport driver of the interoperability milestone (AINT): steps the flight node against a UDP socket and a clock, the loss script, the run log, and the self-test over loopback in virtual time (APP-157…APP-163) |
| `scenario/scenario.json` | The one scenario file every implementation reads (APP-107); `scenario.schema.json` is generated from the harness's model |
| `golden/<case>.jsonl` | Golden traces (§7); `golden/CHANGES.md` records every trace-version change (APP-087) |

`refapp/` is its own Cargo workspace on the pinned toolchain, like `exerciser/` and `fuzz/`. It depends on the `aldis` facade by path with default features off; `cargo xtask refapp-deps` (from the repository root) rejects any other Aldis crate, `aldis-testkit`, a `test-hooks` feature, or a crate root without `#![forbid(unsafe_code)]` (APP-100, APP-105).

## Commands

Run from this directory:

```sh
cargo run -p refapp-harness -- run <case>          # writes target/runs/<case>.jsonl and <case>.manifest.json
cargo run -p refapp-harness -- run-node <case> [--out <dir>] [--component <name>=<version>]... stdio <program> [<arg>...]
cargo run -p refapp-harness -- run-node <case> [--out <dir>] [--component <name>=<version>]... cfs <staging-dir> <port>
                                                   # the flight node in another process (node protocol, §8.2, §10.6); the node's console goes to <case>.node.log, never to the trace
cargo run -p refapp-harness -- compare [<case>]    # byte comparison with golden/<case>.jsonl (default: every case)
cargo run -p refapp-harness -- check-golden        # every golden trace is canonical and decodes with the strict codecs
cargo run -p refapp-harness -- gen-scenario        # (also run by `cargo xtask fix`) regenerate the schema and Rust constants after editing scenario.json
cargo run -p refapp-harness -- gen-scenario --check
```

```sh
# The flight node against a ground system's TM and TC ports (AINT, docs/reference-application.md §11);
# writes a JSON Lines run log, which is not a golden trace and is not committed.
cargo run -p refapp-interop -- run --case a3-nominal --tm 127.0.0.1:10015 \
    --tc-listen 127.0.0.1:10025 --log target/runs/a3-nominal.interop.jsonl [--loss-script a3-uplink-loss | --drop up:5,down:25]
```
The YAMCS image, its configuration, and the TM-only spike are in [interop/yamcs/](interop/yamcs/README.md) (`scripts/interop-yamcs`).

The self-test of the driver (both Rust nodes over loopback UDP in virtual time, reproducing the packet projections of `a3-nominal` and `a3-uplink-loss`) is part of `cargo test`.

The run manifest names the implementation that produced a run; it never goes into the trace (APP-084).

## Blessing a new trace version

A golden trace changes only on purpose (APP-087):

1. Find the reason. A difference is resolved against the Blue Book and the requirement text, never by tuning the trace ([plan](../docs/milestones/a0-scenario.md) §5).
2. If the case's trace was released, bump its `trace_version` in `scenario/scenario.json`.
3. `cargo run -p refapp-harness -- bless <case>` rewrites `golden/<case>.jsonl`.
4. Add a row to `golden/CHANGES.md` with the case, the new trace version, the date, and the reason.

## Checks

The verification check `refapp` (`cargo xtask verify`) runs these; run them before pushing a change here:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo build -p refapp-flight --target thumbv7em-none-eabihf
cargo clippy -p refapp-flight --target thumbv7em-none-eabihf
```

Tests carry `// req: APP-…` annotations, and every golden-trace header lists the requirements its case verifies; `cargo xtask trace` reads both (APP-108). API pain points become GitHub issues labelled `api-feedback` for track R, and the workaround cites the issue (APP-101).
