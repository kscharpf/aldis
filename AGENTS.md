# AGENTS.md

Guide for coding agents working on **Aldis**. Read [README.md](README.md) first for the project overview, crate layout, and build commands. This file adds the rules you need before changing anything. This file holds what an agent must do or must not do; mechanics, rationale, and history go in [docs/](docs/README.md) and are linked.

## Sources of truth

1. **CCSDS Blue Books** are authoritative for wire behavior. They are not in the repository. Run `specs/fetch.sh`: it links `specs/pdf/` from the main checkout when that has them, else downloads them (see [specs/README.md](specs/README.md)). If this repository disagrees with a Blue Book, the Blue Book wins: fix the spec or the code, never the other way round. **Never commit the PDFs or text copied from them.** Cite them by document number and clause.
2. **The specification** in [`docs/`](docs/README.md) defines everything else. Start with `docs/README.md`. Every requirement has an ID (e.g. `SPP-002`, `GEN-020`) and cites its CCSDS clause.
3. **Open decisions** are tracked in [`docs/09-open-issues.md`](docs/09-open-issues.md). Do not silently resolve an open issue in code: update the issue.

Terms and acronyms: [`docs/glossary.md`](docs/glossary.md).

Never write protocol values from memory. Look them up in the Blue Book. Registry values (UPIDs, EPIs, PVNs, time systems) come from SANA (OI-06).

## Tracks

v1 is built in two tracks: **R** (Rust reference, milestones M0–M5) and **A** (reference application, A0–A6); track **C** (native C++, C0–C5) was retired from v1 and deferred to Phase 3. Before starting work, find your track and milestone in [docs/08-roadmap.md](docs/08-roadmap.md), check that its entry gate is met on `main`, and follow the parallel-work rules in §6: one track's directories per PR, branch prefix `m<N>/`, `c<N>/`, or `a<N>/`, and the Rust crates as the reference when implementations disagree.

A milestone plan carries a work-package status table, one row per WP or sub-part (WP, State, PRs, Notes); do not edit its State and PRs columns: `cargo xtask status-table` derives them from GitHub, and the coordinator writes them (title a work-package PR `feat(m3/wp-3b): …`, see [pr-body.md](docs/process/pr-body.md)). Never write requirement counts into a plan: `cargo xtask trace --scope <id>` reports them (roadmap §6 rule 1). Before calling a work package done, go through its checklist in [docs/milestones/checklists.md](docs/milestones/checklists.md) (adding a codec module, a crate, an engine, a vector file, a fuzz target, C ABI functions; a spec-readings PR; a milestone exit); `cargo xtask consistency` checks the registrations it marks.

The rules and checks below are written for the Rust crates and the reference application.

## Decision rights

[docs/process/decision-rights.md](docs/process/decision-rights.md) says who decides what: the maintainer, the coordinating session that dispatches agents and opens pull requests, or the agent of a work package. In short:

- **Decide yourself**, and list it under "Judgement calls" in your report: how to build what an accepted plan decision says; a Blue Book reading the normative text settles (cite the clause); new vector cases, and the layout of vector files not yet released; fixes to review findings that change no behaviour, spec, or ABI.
- **Leave to the maintainer**, through the coordinator, as one question with a recommended option: a reading the Blue Book does not settle (as an OI), scope and plan decisions, C ABI and public API conventions, new dependencies, releases, tags, visibility and anything external, approvals, and merges to `main`. Meanwhile, continue the work the answer does not touch.
- **Never ask** about what a rule of this file, a checklist, or a check already decides.

## Independent review

Protocol: [docs/process/review.md](docs/process/review.md).

- Don't re-run the gate's checks; read the published record (`cargo xtask verify check <head>`), including non-gating checks (`rust-sparc-leon3`; `kani` is nightly-only, VER-059).
- Read every suppression the change adds (`#[expect(…, reason = "…")]` RS-016, `coverage-waivers.toml` VER-024, equivalent mutants in [mutants-triage.md](docs/verification/mutants-triage.md) VER-025) and verify the reason holds on the smallest region.
- Spend the review on what no check covers: the Blue Book readings (cite the clause), requirements and test/vector coverage, `unsafe` soundness, and the public API and C ABI shape.
- **Every statement about the implementation cites `path:line`**, quoting the code where the point depends on it.
- Report mismatches between documentation and code, or between two documents, as one batch of nits at the end.

## Rules no check enforces (Layers 0–2)

- `no_std`, **no heap**: fixed capacities via const generics (GEN-010, GEN-011). No I/O, clocks, randomness, logging, or global state; engines are sans-I/O and deterministic (GEN-030–032).
- **Panic-free** (GEN-020): the lints catch `unwrap`, indexing and arithmetic; they do not catch every panic path. An exerciser link error naming `aldis_panic_is_unreachable` means a panic path is reachable in `aldis-core`: remove it.
- **Parse, don't validate** (RS-020): make invalid values unrepresentable. Give every field with a restricted range its own type with a checked constructor, keep fields private when a constraint spans them, and check untrusted input once, where it becomes the type. A constructor may check only the relation between values that are each already valid, such as a value fitting a format. Do not add `validate`/`check` functions that re-examine a value the type could have guaranteed.
- **No clamped slice fallbacks or wire input defaults:** an untrusted wire input that is too short or out of range returns an explicit error (`Incomplete`, `LengthMismatch`, `InvalidField`, `Malformed`), never a clamped range, `unwrap_or_default`, or `unwrap_or(0)`; an absent optional or variable-length field may yield an empty slice or 0, and a clamp that only keeps `copy_within` in a range the code built itself is not a fallback. The reviewer enforces it ([review.md](docs/process/review.md)).
- **State machines** (RS-021): make invalid states and transitions unrepresentable. The criterion is who chooses the next state. Use the typestate pattern where the method the caller calls decides it (lifecycles, two-phase writers, builders whose call sequence is the order). Where the input data or an external event decides it (COP-1's protocol states; sessions inside step-function engines such as segmentation, reassembly, and CLTU packing), use a closed `enum` with an exhaustive `match` and no wildcard arm; COP-1's transition function is generated from, or checked against, the state table.

## Checks

Run `cargo xtask check --quick` after each edit and `cargo xtask check` before each commit, with zero warnings. Which tier ends a round, what `--full` adds, and how to resume: [agent-brief.md](docs/process/agent-brief.md) items 2 and 3. No tier is the gate; report which tier passed.

**Implementation agents commit and stop**, unless the brief grants the gate ([agent brief](docs/process/agent-brief.md)). The coordinator runs the merge gate and the merge trains ([coordinator.md](docs/process/coordinator.md)).

- Never edit a generated file (`include/aldis/aldis.h`, `crates/aldis-ffi/src/status.rs`, field codes, SANA tables, vector schema, refapp scenario files); change its source and run `cargo xtask fix`.
- A new test module in a crate the coverage gate measures gets `#[cfg_attr(coverage_nightly, coverage(off))]` (`xtask/` needs none).
- A check in `xtask/src/verify/checks.rs` declares its inputs; when it starts reading a new path, add the path in the same change.
- Relax a clippy lint only with a local `#[expect(clippy::<lint>, reason = "…")]` on the smallest item (RS-016), never a crate-wide `allow`. Adopting a nursery lint is recorded in [rust-lints.md](docs/profiles/rust-lints.md) (RS-019).
- The agent definitions in `.claude/agents/` and `docs/process/` change only by a pull request: propose a lesson as one, or in your report under "For the maintainer".

## Conventions

- **Traceability:** every public item's docs cite the CCSDS clause and requirement IDs it implements (RS-015). Every test names the requirements it covers with a `// req: <ID>` comment (VER-020).
- **Verification records:** evidence by test, vector, or check needs no prose record; a requirement whose evidence is a person's judgement has one in [docs/verification/records/](docs/verification/records/) ([analysis-records.md](docs/verification/analysis-records.md)). Change only the record you touch.
- **Test vectors** in [`test-vectors/`](test-vectors/README.md) are shared across implementations; follow [docs/07-test-vectors.md](docs/07-test-vectors.md). Record provenance, and never edit a released vector: add a new case and mark the old one `superseded`. An independent model (TV-023) is written blind, from the Blue Book and `docs/` only: give a model-writing agent no implementation paths.
- **Pull request body:** Summary, Evidence, Merge Danger; link decisions, never restate them ([pr-body.md](docs/process/pr-body.md)).
- **Changelog:** never edit `CHANGELOG.md` in a pull request; the PR title is the entry, a conventional commit `type(scope)!: description`.
- **Errors, status codes, field codes** come from the catalogue of [docs/03-api-model.md](docs/03-api-model.md) §9 and §9.1. Do not renumber error codes. C ABI names use the `aldis_` / `ALDIS_` prefix ([docs/profiles/c-abi.md](docs/profiles/c-abi.md)).
- **Agent comments:** end every comment or review an agent posts on GitHub with the line `🤖 Generated with [Claude Code](https://claude.com/claude-code)`. Agents post under the maintainer's account, and `cargo xtask metrics` tells their words apart by this marker.
- **Search boundaries:** confine file and text searches to the active repository workspace; prefer `git grep` and `git ls-files` over recursive filesystem walks. Never inspect external application or cache directories (e.g. `~/.gemini/`, `~/.cache`), `.git/`, or internal session worktrees (`.claude/worktrees/`).
- **GitHub CLI queries:** use structured output flags (`--json` or `-q`), never raw interactive commands.
- **Spec changes:** when behavior changes, update the requirement table in the same change. New requirements take the next free ID in their area and IDs are never reused; open issues take the next free `OI-<number>` ([09](docs/09-open-issues.md)).

## Where things go

| Change | Location |
|---|---|
| Shared algorithm or type (CRC, counters, IDs, security hooks) | `crates/aldis-core` |
| Codec for one standard | `crates/aldis-<std>` (`time`, `spp`, `epp`, `tm`, `tc`, `aos`, `uslp`) |
| Stateful protocol behavior | `crates/aldis-link`, or `crates/aldis-cop1` for COP-1 |
| Transports, run loop, adapters | none yet: deferred ([docs/05-framework.md](docs/05-framework.md)) |
| C interface | `crates/aldis-ffi` (capacities in `capacities.toml`) |
| Reference application (track A) | `refapp/` |
| Requirements and design | `docs/` |
| Conformance data | `test-vectors/` |
