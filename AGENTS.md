# AGENTS.md

Rules for coding agents working on **Aldis**. This file holds what an agent must or must not do; mechanics and rationale are in [docs/](docs/README.md). The rules apply to the Rust crates and the reference application. Terms: [glossary](docs/glossary.md).

## Sources of truth

1. **CCSDS Blue Books** are authoritative for wire behavior. They are not in the repository. Run `specs/fetch.sh` ([specs/README.md](specs/README.md)). Where this repository disagrees with a Blue Book, the Blue Book wins: fix the spec or the code. **Never commit the PDFs or text copied from them.** Cite document number and clause. Never write protocol values from memory; registry values (UPIDs, EPIs, PVNs, time systems) come from SANA (OI-06).
2. **The specification** in [`docs/`](docs/README.md) defines everything else; start with `docs/README.md`.
3. **Open decisions** are in [`docs/09-open-issues.md`](docs/09-open-issues.md). Do not resolve one silently in code: update the issue.

## Tracks

Tracks: **R** (Rust reference, M0–M5) and **A** (reference application, A0–A6); track **C** is deferred to Phase 3. Find your track and milestone in [docs/08-roadmap.md](docs/08-roadmap.md), check that its entry gate is met on `main`, and follow §6: one track's directories per PR, branch prefix `m<N>/` or `a<N>/`, the Rust crates as the reference.

Do not edit the State and PRs columns of a plan's work-package table (the coordinator writes them; title a work-package PR `feat(m3/wp-3b): …`, [pr-body.md](docs/process/pr-body.md)). Never write requirement counts into a plan (`cargo xtask trace --scope <id>`). Before calling a work package done, go through its checklist in [docs/milestones/checklists.md](docs/milestones/checklists.md).

## Decision rights

[docs/process/decision-rights.md](docs/process/decision-rights.md) says who decides what. In short:

- **Decide yourself**, and list it under "Judgement calls" in your report: how to build what an accepted plan decision says; a Blue Book reading the normative text settles (cite the clause); new vector cases and the layout of unreleased vector files; fixes to review findings that change no behaviour, spec, or ABI.
- **Leave to the maintainer**, through the coordinator, as one question with a recommended option: a reading the Blue Book does not settle (as an OI), scope and plan decisions, C ABI and public API conventions, new dependencies, releases, tags, visibility and anything external, approvals, and merges to `main`. Meanwhile, continue the work the answer does not touch.
- **Never ask** about what a rule of this file, a checklist, or a check already decides.

## Independent review

Reviewers follow [docs/process/review.md](docs/process/review.md), the one copy of the protocol.

## Rules no check enforces (Layers 0–2)

- `no_std`; no I/O, clocks, randomness, logging, or global state; engines are sans-I/O and deterministic (GEN-030–032).
- **Panic-free** (GEN-020): the lints catch `unwrap`, indexing and arithmetic; they do not catch every panic path. An exerciser link error naming `aldis_panic_is_unreachable` means a panic path is reachable in `aldis-core`: remove it.
- **Parse, don't validate** (RS-020): make invalid values unrepresentable. Give every field with a restricted range its own type with a checked constructor, keep fields private when a constraint spans them, and check untrusted input once, where it becomes the type. A constructor checks only a relation between already-valid values, such as a value fitting a format; add no `validate`/`check` function that re-examines what the type could guarantee.
- **No clamped slice fallbacks or wire input defaults:** an untrusted wire input that is too short or out of range returns an explicit error (`Incomplete`, `LengthMismatch`, `InvalidField`, `Malformed`), never a clamped range, `unwrap_or_default`, or `unwrap_or(0)`; an absent optional or variable-length field may yield an empty slice or 0, and a clamp on a range the code built itself is not a fallback. The reviewer enforces it.
- **State machines** (RS-021): make invalid states and transitions unrepresentable. Where the caller's method call chooses the next state (lifecycles, two-phase writers, builders), use typestate. Where input data or an external event chooses it (COP-1, step-function engines such as segmentation, reassembly, CLTU packing), use a closed `enum` with an exhaustive `match`, no wildcard arm; generate COP-1's transition function from the state table or check it against it.

## Checks

Run `cargo xtask check --quick` after each edit, with zero warnings. The default tier runs only in the first round and the final round; an intermediate fix round or merge-only round runs `--quick` plus `cargo test` of what changed. Rounds, tiers, and resuming a failed run (`--from`): [agent-brief.md](docs/process/agent-brief.md) items 2 and 3; `--full` is optional. Report which tier passed; no tier is the gate.

**Implementation agents commit and stop**, unless the brief grants the gate ([agent brief](docs/process/agent-brief.md)). The coordinator runs the merge gate and the merge trains ([coordinator.md](docs/process/coordinator.md)).

- Never edit a generated file (`include/aldis/aldis.h`, `crates/aldis-ffi/src/status.rs`, field codes, SANA tables, vector schema, refapp scenario files); change its source and run `cargo xtask fix`.
- A new test module in a crate the coverage gate measures gets `#[cfg_attr(coverage_nightly, coverage(off))]` (`xtask/` needs none).
- A check in `xtask/src/verify/checks.rs` declares its inputs; when it starts reading a new path, add the path in the same change.
- Relax a clippy lint only with a local `#[expect(clippy::<lint>, reason = "…")]` on the smallest item (RS-016). Do not enable the whole `nursery` group; adopt a nursery lint only after checking its findings, and record it in [rust-lints.md](docs/profiles/rust-lints.md) (RS-019).
- The agent definitions in `.claude/agents/` and `docs/process/` change only by a pull request: propose a lesson as one, or in your report under "For the maintainer".

## Conventions

- **Traceability:** every public item's docs cite the CCSDS clause and requirement IDs it implements (RS-015). Every test names the requirements it covers with a `// req: <ID>` comment (VER-020).
- **Verification records:** only a requirement whose evidence is a person's judgement has a record, in [docs/verification/records/](docs/verification/records/) ([format](docs/verification/analysis-records.md)); change only the one you touch. A new check-to-requirement pair goes in `CHECK_REQUIREMENTS` in `xtask/src/trace.rs`.
- **Test vectors** in [`test-vectors/`](test-vectors/README.md) are shared across the Rust and C ABI implementations; follow [docs/07-test-vectors.md](docs/07-test-vectors.md). Record provenance, and never edit a released vector: add a new case and mark the old one `superseded`. An independent model (TV-023) is written blind, from the Blue Book and `docs/` only: give a model-writing agent no implementation paths.
- **Pull request body:** Summary, Evidence, Merge Danger; link decisions, never restate them ([pr-body.md](docs/process/pr-body.md)).
- **Errors, status codes, field codes** come from the catalogue of [docs/03-api-model.md](docs/03-api-model.md) §9 and §9.1. Do not renumber error codes. C ABI names use the `aldis_` / `ALDIS_` prefix ([docs/profiles/c-abi.md](docs/profiles/c-abi.md)).
- **Agent comments:** end every comment or review an agent posts on GitHub with the line `🤖 Generated with [Claude Code](https://claude.com/claude-code)`. It tells agent comments from the maintainer's (`cargo xtask metrics`).
- **Search boundaries:** search only the active workspace; prefer `git grep` and `git ls-files` over recursive walks. Never inspect external application or cache directories (e.g. `~/.gemini/`, `~/.cache`), `.git/`, or `.claude/worktrees/`.
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
| Requirements and design; conformance data | `docs/`; `test-vectors/` |
