# AGENTS.md

Guide for coding agents working on **Aldis**. Read [README.md](README.md) first for the project overview, crate layout, and build commands. This file adds the rules you need before changing anything. This file holds what an agent must do or must not do; mechanics, rationale, and history go in [docs/](docs/README.md) and are linked.

## Sources of truth

1. **CCSDS Blue Books** are authoritative for wire behavior. They are not in the repository. Run `specs/fetch.sh` to download them into `specs/pdf/` (see [specs/README.md](specs/README.md)). If this repository disagrees with a Blue Book, the Blue Book wins: fix the spec or the code, never the other way round. **Never commit the PDFs or text copied from them.** Cite them by document number and clause.
2. **The specification** in [`docs/`](docs/README.md) defines everything else. Start with `docs/README.md`. Every requirement has an ID (e.g. `SPP-002`, `GEN-020`) and cites its CCSDS clause.
3. **Open decisions** are tracked in [`docs/09-open-issues.md`](docs/09-open-issues.md). Do not silently resolve an open issue in code: update the issue.

Terms and acronyms: [`docs/glossary.md`](docs/glossary.md).

Never write protocol values from memory. Look them up in the Blue Book. Registry values (UPIDs, EPIs, PVNs, time systems) come from SANA (OI-06).

## Tracks

v1 is built in three tracks: **R** (Rust reference, milestones M0–M6), **C** (native C++, C0–C5), and **A** (reference application, A0–A6). Before starting work, find your track and milestone in [docs/08-roadmap.md](docs/08-roadmap.md), check that its entry gate is met on `main`, and follow the parallel-work rules in §6: one track's directories per PR, branch prefix `m<N>/`, `c<N>/`, or `a<N>/`, and the Rust crates as the reference when implementations disagree.

A milestone plan carries a work-package status table, one row per WP or sub-part (WP, State, PRs, Notes); update only your WP's row. Never write requirement counts into a plan: `cargo xtask trace --scope <id>` reports them (roadmap §6 rule 1). Before calling a work package done, go through its checklist in [docs/milestones/checklists.md](docs/milestones/checklists.md) (adding a codec module, a crate, a vector file, a fuzz target, C ABI functions; a spec-readings PR; a milestone exit); `cargo xtask consistency` checks the registrations it marks.

The rules and checks below are written for the Rust crates and the reference application.

## Decision rights

[docs/process/decision-rights.md](docs/process/decision-rights.md) says who decides what: the maintainer, the coordinating session that dispatches agents and opens pull requests, or the agent of a work package. In short:

- **Decide yourself**, and list it under "Judgement calls" in your report: how to build what an accepted plan decision says; a Blue Book reading the normative text settles (cite the clause); new vector cases, and the layout of vector files not yet released; fixes to review findings that change no behaviour, spec, or ABI.
- **Leave to the maintainer**, through the coordinator, as one question with a recommended option: a reading the Blue Book does not settle (as an OI), scope and plan decisions, C ABI and public API conventions, new dependencies, releases, tags, visibility and anything external, approvals, and merges to `main`. Meanwhile, continue the work the answer does not touch.
- **Never ask** about what a rule of this file, a checklist, or a check already decides.

Every maintainer decision gets a line in the [decision log](docs/process/decisions.md) (VER-062), which `cargo xtask decisions` checks.

## Independent review

Detailed protocol: [docs/process/review.md](docs/process/review.md).

- Don't re-run the gate's checks; read the published record (`cargo xtask verify check <head>`), including non-gating checks (`rust-sparc-leon3`; `kani` is nightly-only, VER-059).
- Read every suppression the change adds (`#[expect(…, reason = "…")]` RS-016, `coverage-waivers.toml` VER-024, and equivalent mutants in [mutants-triage.md](docs/verification/mutants-triage.md) VER-025) and verify the reason holds on the smallest region.
- Spend the review on what no check covers: the Blue Book readings (cite the clause), requirements and test/vector coverage, `unsafe` soundness, and the public API and C ABI shape.
- **Every statement about the implementation cites `path:line`**, quoting the code where the point depends on it; a review whose description of the code does not match the code does not count and is done again.

## Non-negotiable rules (Layers 0–2)

- `no_std`, **no heap**: fixed capacities via const generics (GEN-010, GEN-011).
- **No panics** on any input: no `unwrap`/`expect`, no unchecked indexing or arithmetic (GEN-020).
- No I/O, clocks, randomness, logging, or global state. Engines are sans-I/O and deterministic (GEN-030–032).
- No `unsafe` outside `aldis-ffi` (GEN-070). Every `unsafe` block there needs a `// SAFETY:` comment.
- No third-party runtime dependencies in Layers 0–2 (GEN-001).
- Layering: a crate may depend only on lower layers ([docs/01-architecture.md](docs/01-architecture.md) §2).
- **Parse, don't validate** (RS-020): make invalid values unrepresentable. Give every field with a restricted range its own type with a checked constructor, keep fields private when a constraint spans them, and check untrusted input once, where it becomes the type. A constructor may check only the relation between values that are each already valid, such as a value fitting a format. Do not add `validate`/`check` functions that re-examine a value the type could have guaranteed.
- **State machines** (RS-021): make invalid states and transitions unrepresentable. Always use the typestate pattern where the caller's call sequence decides the next state (lifecycles, two-phase writers, builders, sessions). Where an external event decides it (COP-1's protocol states), use a closed `enum` whose transition function is generated from, or checked against, the state table, with an exhaustive `match` and no wildcard arm.

## Toolchain and checks

The toolchain is pinned to Rust **1.99.1** in `rust-toolchain.toml`. Do not change it without updating RS-001 and OI-07.

**The merge gate is local** ([docs/06-verification.md](docs/06-verification.md) §7, OI-31). GitHub Actions runs only at night (VER-059). A pull request merges when its head commit contains the tip of `main` and has a published, passing record of the verification run (VER-058):

```sh
git push                            # the commit must be on GitHub before its status can be set
cargo xtask verify --publish        # every check, in the pinned image, stopping at the first failure; then the git note and status aldis/verify
cargo xtask verify check            # re-check the published record of HEAD; it fails while HEAD lacks the tip of main
```

If you cannot run it locally, start the same run on a GitHub-hosted runner instead (VER-060). It takes 20 to 30 minutes when it has to build the image, and less when a workflow has already pushed the image for the same pins to `ghcr.io/kscharpf/aldis-verify` (VER-060). It publishes the same record and status, and names the run they came from:

```sh
git push
gh workflow run verify.yml --ref <branch>   # or "Run workflow" on the Actions tab
gh run watch                                # then: cargo xtask verify check
```

`cargo xtask verify` refuses a dirty working tree, runs in a fresh checkout of HEAD (`cargo xtask verify list` names the checks; `--only <check>,...` runs a subset while working, which never passes the gate), and runs on one machine wait for each other. A record is tied to the commit's tree, so any new commit—including a merge from `main`—needs a new run. If `main` moves during a passing run, `--publish` still publishes the record for reuse, but sets no `aldis/verify` status: merge `main` and verify again (see [docs/06-verification.md](docs/06-verification.md) §7 for container engine memory and disk setup). Source `scripts/agent-env.sh` to configure the agent environment (`NO_COLOR=1`, `GH_FORCE_TTY=0`, `GH_NO_UPDATE_NOTIFIER=1`, and `gh run-status`).

While you work, run these. All must pass with **zero warnings**:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-features --all-targets
cargo test --workspace --all-features
cargo clippy -p aldis --no-default-features --features full --target thumbv7em-none-eabihf
cargo xtask ci        # spec lint, links, decision log, layering, registrations (consistency), bare integers in the codec APIs (rs020), loops against the inspection records (loops), vectors, schema, generated files (field codes, C header), traces, changelog
```

| Directory | Extra commands |
|---|---|
| `crates/aldis-core` | `cargo xtask coverage` (VER-024)<br>`(cd exerciser && cargo build --release --target thumbv7em-none-eabihf)` (VER-010)<br>`cargo clippy --manifest-path fuzz/Cargo.toml --all-targets` |
| `crates/aldis-ffi` (or wrapped crates) | `cargo xtask header --write && cargo xtask header` (CABI-002, CABI-003)<br>`cargo xtask header c-smoke cc c++` (CABI-001; requires `cargo install --locked cbindgen@0.29.4`; never edit `include/aldis/aldis.h` by hand) |
| `refapp/` ([refapp/README.md](refapp/README.md)) | `(cd refapp && cargo fmt --all --check && cargo clippy --workspace --all-targets && cargo test --workspace && cargo build -p refapp-flight --target thumbv7em-none-eabihf)` |

A failed exerciser link with `undefined symbol: aldis_panic_is_unreachable` means a panic path is reachable in `aldis-core`. Remove it; do not work around the check. Mark new test modules `#[cfg_attr(coverage_nightly, coverage(off))]`.

Each check in `xtask/src/verify/checks.rs` declares its inputs (`cargo xtask verify list` shows which checks HEAD needs); when a check starts reading a new path, add that path to its inputs in the same change.

Clippy runs with the `all` and `pedantic` groups denied, plus restriction lints and selected nursery lints ([docs/profiles/rust.md](docs/profiles/rust.md) §5). Do not enable the whole `nursery` group; adopt a nursery lint only after checking its findings, and record the decision there (RS-019). To relax a lint, use a local `#[expect(clippy::<lint>, reason = "…")]` on the smallest item (RS-016). Never use crate-wide `allow`.

## Conventions

- **Traceability:** every public item's docs cite the CCSDS clause and requirement IDs it implements (RS-015). Every test names the requirements it covers with a `// req: <ID>` comment (VER-020).
- **Verification records:** requirements verified by a check of `cargo xtask verify` or by review (method A/I) each have a record file in [docs/verification/records/](docs/verification/records/), named after its first requirement ID, in the format of [analysis-records.md](docs/verification/analysis-records.md). Change only the record you touch; there is no index.
- **Inspections (GEN-022/023):** when your change adds or changes a loop, a call between a crate's own functions, or a blocking construct in library code, revise that crate's record in [docs/verification/inspections/](docs/verification/inspections/) in the same change: update the loop table (its `ID` column names each loop by file, item, and index, as `cargo xtask loops list` prints it; `cargo xtask loops` fails on a loop the table lacks) and call graph, and add a `Revised` row "<date>, <branch> (<WP>), prepared by <you>, pending exit review: …". Do not ask for or wait for a maintainer approval, and never edit an `Approved` row: the maintainer approves every pending revision once, in the milestone exit review or before a release (VER-061).
- **Test vectors** in [`test-vectors/`](test-vectors/README.md) are shared across the Rust and C ABI implementations. Follow [docs/07-test-vectors.md](docs/07-test-vectors.md). Record provenance, and never edit a released vector: add a new case and mark the old one `superseded`. An independent model (TV-023) is written blind: by an agent or person who has not read the implementation it checks, from the Blue Book and `docs/` only; brief a model-writing agent that way, and give it no implementation paths.
- **Changelog:** never edit `CHANGELOG.md` in a pull request. Add one fragment file to [`changelog.d/`](changelog.d/README.md), named after the branch; `cargo xtask changelog check` validates it.
- **Errors** use the stable catalogue in [docs/03-api-model.md](docs/03-api-model.md) §9. Do not renumber error codes.
- **Field codes** come from the catalogue of [docs/03-api-model.md](docs/03-api-model.md) §9.1; `cargo xtask fields` checks the per-crate and common blocks.
- **C ABI names** use the `aldis_` / `ALDIS_` prefix ([docs/profiles/c-abi.md](docs/profiles/c-abi.md)).
- **Agent comments:** end every comment or review an agent posts on GitHub with the line `🤖 Generated with [Claude Code](https://claude.com/claude-code)`. Agents post under the maintainer's account, and `cargo xtask metrics` tells their words apart by this marker.
- **Search boundaries:** confine all file and text searches strictly to the active repository workspace; prefer git-aware search tools (`git grep`, `git ls-files`) over recursive filesystem walks (`grep -r`, `find .`). Never inspect external application or cache directories (e.g. `~/.gemini/`, `~/.cache`), `.git/`, or internal session worktrees (`.claude/`).
- **GitHub CLI queries:** always query GitHub CLI commands with structured output flags (`--json` or `-q`), never raw interactive commands (e.g. use `gh run view --json status,conclusion,jobs` or `gh run list --workflow= -L 1`).
- **Spec changes:** when behavior changes, update the requirement table in the same change. New requirements get the next free ID in their area; IDs are never reused.

## Where things go

| Change | Location |
|---|---|
| Shared algorithm or type (CRC, counters, IDs, security hooks) | `crates/aldis-core` |
| Codec for one standard | `crates/aldis-<std>` (`time`, `spp`, `epp`, `tm`, `tc`, `aos`, `uslp`) |
| Stateful protocol behavior | `crates/aldis-link`, or `crates/aldis-cop1` for COP-1 |
| Transports, run loop, adapters | `crates/aldis-framework` |
| C interface | `crates/aldis-ffi` (capacities in `capacities.toml`) |
| Reference application (track A) | `refapp/` |
| Requirements and design | `docs/` |
| Conformance data | `test-vectors/` |
