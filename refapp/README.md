# Reference application (`refapp/`)

An end-to-end flight↔ground application that uses Aldis only through its public API, as an integrator would. The scenario is specified in [docs/reference-application.md](../docs/reference-application.md) (requirement area APP); the plan for each milestone is in [docs/milestones/](../docs/milestones/) (`a0-…` to `a6-…`). Track A owns this directory ([roadmap](../docs/08-roadmap.md) §3, §6).

**Status: A0 skeleton.** The nodes have no applications yet (they arrive in A1, after M1). The harness, trace writer, canonical-form check, comparator, and scenario generator work, and the header-only case `a0-empty` is reproduced in CI.

The spacecraft ID in [scenario/scenario.json](scenario/scenario.json) (`341`, `0x155`) is **fictional**, confirmed by the maintainer on 2026-10-19 ([OI-26](../docs/09-open-issues.md)).

## Layout

| Path | Contents |
|---|---|
| `refapp-scenario/` | `no_std`: the constants generated from the scenario file, and the node interface (`Node`, `StepInput`, `Output`, §8.1) |
| `refapp-flight/` | `no_std`, no heap: the flight node; flight lint set (APP-102) |
| `refapp-ground/` | The ground node; flight lint set's restriction lints (APP-103) |
| `refapp-harness/` | `std` tool: scheduler and link model, trace writer, canonical-form check, comparator, scenario generator |
| `scenario/scenario.json` | The one scenario file every implementation reads (APP-107); `scenario.schema.json` is generated from the harness's model |
| `golden/<case>.jsonl` | Golden traces (§7); `golden/CHANGES.md` records every trace-version change (APP-087) |

`refapp/` is its own Cargo workspace on the pinned toolchain, like `exerciser/` and `fuzz/`. It depends on the `aldis` facade by path with default features off; `cargo xtask refapp-deps` (from the repository root) rejects any other Aldis crate, `aldis-testkit`, a `test-hooks` feature, or a crate root without `#![forbid(unsafe_code)]` (APP-100, APP-105).

## Commands

Run from this directory:

```sh
cargo run -p refapp-harness -- run <case>          # writes target/runs/<case>.jsonl and <case>.manifest.json
cargo run -p refapp-harness -- compare [<case>]    # byte comparison with golden/<case>.jsonl (default: every case)
cargo run -p refapp-harness -- check-golden        # every golden trace is in canonical form
cargo run -p refapp-harness -- gen-scenario        # regenerate the schema and Rust constants after editing scenario.json
cargo run -p refapp-harness -- gen-scenario --check
```

The run manifest names the implementation that produced a run; it never goes into the trace (APP-084).

## Blessing a new trace version

A golden trace changes only on purpose (APP-087):

1. Find the reason. A difference is resolved against the Blue Book and the requirement text, never by tuning the trace ([plan](../docs/milestones/a0-scenario.md) §5).
2. If the case's trace was released, bump its `trace_version` in `scenario/scenario.json`.
3. `cargo run -p refapp-harness -- bless <case>` rewrites `golden/<case>.jsonl`.
4. Add a row to `golden/CHANGES.md` with the case, the new trace version, the date, and the reason.

## Checks

The CI job `refapp` runs these; run them before pushing a change here:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo build -p refapp-flight --target thumbv7em-none-eabihf
cargo clippy -p refapp-flight --target thumbv7em-none-eabihf
```

Tests carry `// req: APP-…` annotations, and every golden-trace header lists the requirements its case verifies; `cargo xtask trace` reads both (APP-108). API pain points become GitHub issues labelled `api-feedback` for track R, and the workaround cites the issue (APP-101).
