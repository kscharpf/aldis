# AGENTS.md

Guide for coding agents working on **Aldis**. Read [README.md](README.md) first for the project overview, crate layout, and build commands. This file adds the rules you need before changing anything. This file holds what an agent must do or must not do; mechanics, rationale, and history go in [docs/](docs/README.md) and are linked.

## Sources of truth

1. **CCSDS Blue Books** are authoritative for wire behavior. They are not in the repository. Run `specs/fetch.sh`: it links `specs/pdf/` from the main checkout when that has them, else downloads them (see [specs/README.md](specs/README.md)). If this repository disagrees with a Blue Book, the Blue Book wins: fix the spec or the code, never the other way round. **Never commit the PDFs or text copied from them.** Cite them by document number and clause.
2. **The specification** in [`docs/`](docs/README.md) defines everything else. Start with `docs/README.md`. Every requirement has an ID (e.g. `SPP-002`, `GEN-020`) and cites its CCSDS clause.
3. **Open decisions** are tracked in [`docs/09-open-issues.md`](docs/09-open-issues.md). Do not silently resolve an open issue in code: update the issue.

Terms and acronyms: [`docs/glossary.md`](docs/glossary.md).

Never write protocol values from memory. Look them up in the Blue Book. Registry values (UPIDs, EPIs, PVNs, time systems) come from SANA (OI-06).

## Tracks

v1 is built in three tracks: **R** (Rust reference, milestones M0–M6), **C** (native C++, C0–C5), and **A** (reference application, A0–A6). Before starting work, find your track and milestone in [docs/08-roadmap.md](docs/08-roadmap.md), check that its entry gate is met on `main`, and follow the parallel-work rules in §6: one track's directories per PR, branch prefix `m<N>/`, `c<N>/`, or `a<N>/`, and the Rust crates as the reference when implementations disagree.

A milestone plan carries a work-package status table, one row per WP or sub-part (WP, State, PRs, Notes); do not edit its State and PRs columns: `cargo xtask status-table` derives them from GitHub, and the coordinator writes them (title a work-package PR `feat(m3/wp-3b): …`, see [pr-body.md](docs/process/pr-body.md)). Never write requirement counts into a plan: `cargo xtask trace --scope <id>` reports them (roadmap §6 rule 1). Before calling a work package done, go through its checklist in [docs/milestones/checklists.md](docs/milestones/checklists.md) (adding a codec module, a crate, an engine, a vector file, a fuzz target, C ABI functions; a spec-readings PR; a milestone exit); `cargo xtask consistency` checks the registrations it marks.

The rules and checks below are written for the Rust crates and the reference application.

## Decision rights

[docs/process/decision-rights.md](docs/process/decision-rights.md) says who decides what: the maintainer, the coordinating session that dispatches agents and opens pull requests, or the agent of a work package. In short:

- **Decide yourself**, and list it under "Judgement calls" in your report: how to build what an accepted plan decision says; a Blue Book reading the normative text settles (cite the clause); new vector cases, and the layout of vector files not yet released; fixes to review findings that change no behaviour, spec, or ABI.
- **Leave to the maintainer**, through the coordinator, as one question with a recommended option: a reading the Blue Book does not settle (as an OI), scope and plan decisions, C ABI and public API conventions, new dependencies, releases, tags, visibility and anything external, approvals, and merges to `main`. Meanwhile, continue the work the answer does not touch.
- **Never ask** about what a rule of this file, a checklist, or a check already decides.

## Independent review

Detailed protocol: [docs/process/review.md](docs/process/review.md).

- Don't re-run the gate's checks; read the published record (`cargo xtask verify check <head>`), including non-gating checks (`rust-sparc-leon3`; `kani` is nightly-only, VER-059).
- Read every suppression the change adds (`#[expect(…, reason = "…")]` RS-016, `coverage-waivers.toml` VER-024, and equivalent mutants in [mutants-triage.md](docs/verification/mutants-triage.md) VER-025) and verify the reason holds on the smallest region.
- Spend the review on what no check covers: the Blue Book readings (cite the clause), requirements and test/vector coverage, `unsafe` soundness, and the public API and C ABI shape.
- **Every statement about the implementation cites `path:line`**, quoting the code where the point depends on it; a review whose description of the code does not match the code does not count and is done again.
- Report mismatches between documentation and code, or between two documents, as one batch of nits at the end; the implementer fixes them in the same round as the code findings.

## Non-negotiable rules (Layers 0–2)

- `no_std`, **no heap**: fixed capacities via const generics (GEN-010, GEN-011).
- **No panics** on any input: no `unwrap`/`expect`, no unchecked indexing or arithmetic (GEN-020).
- No I/O, clocks, randomness, logging, or global state. Engines are sans-I/O and deterministic (GEN-030–032).
- No `unsafe` outside `aldis-ffi` (GEN-070). Every `unsafe` block there needs a `// SAFETY:` comment.
- No third-party runtime dependencies in Layers 0–2 (GEN-001).
- Layering: a crate may depend only on lower layers ([docs/01-architecture.md](docs/01-architecture.md) §2).
- **Parse, don't validate** (RS-020): make invalid values unrepresentable. Give every field with a restricted range its own type with a checked constructor, keep fields private when a constraint spans them, and check untrusted input once, where it becomes the type. A constructor may check only the relation between values that are each already valid, such as a value fitting a format. Do not add `validate`/`check` functions that re-examine a value the type could have guaranteed.
- **No clamped slice fallbacks or wire input defaults:** an untrusted wire input that is too short or out of range returns an explicit error (`Incomplete`, `LengthMismatch`, `InvalidField`, `Malformed`), never a clamped range, `unwrap_or_default`, or `unwrap_or(0)`; an absent optional or variable-length field may yield an empty slice or 0. The reviewer enforces it ([review.md](docs/process/review.md)).
- **State machines** (RS-021): make invalid states and transitions unrepresentable. Always use the typestate pattern where the caller's call sequence decides the next state (lifecycles, two-phase writers, builders, sessions). Where an external event decides it (COP-1's protocol states), use a closed `enum` whose transition function is generated from, or checked against, the state table, with an exhaustive `match` and no wildcard arm.

## Checks

While you work, run `cargo xtask check --quick` after each edit (format and clippy with the gate's lints, of the four workspaces, every error at once), `cargo xtask check` before each commit, and `cargo xtask check --full` once before the final commit; fix everything they report (**zero warnings**). The default tier stops at the first failure and prints what it ran and skipped: format, clippy, tests, the bare-metal clippy build, `cargo xtask ci`, then the extras for what differs from `origin/main`: the exerciser's clippy and build for any crate, and the extras of `crates/aldis-ffi` and `refapp/`. `--full` adds coverage, `stack`, `big-endian`, and the exerciser's QEMU run when the verification run would run them for the change (the checks declare their inputs, VER-056). No tier is the gate: the last line names the gate checks it did not run (`check: passed (full tier; …)` only for `--full`; the other tiers end `… tier ok (not a gate pass; …)`), and `cargo xtask verify` decides a merge (VER-068). Coverage and `stack` need the tooling nightly, `big-endian` needs the s390x target and QEMU, the exerciser run needs `qemu-system-arm`, the header checks need `cargo install --locked cbindgen@0.29.4`. `cargo xtask fix` runs the generators (field codes, vector schema, SANA tables, C header), then `consistency`; never edit a generated file (`include/aldis/aldis.h`, `crates/aldis-ffi/src/status.rs`) by hand. Generated files are committed and checked current by the `cargo xtask ci` steps `fields`, `status`, `header`, `sana`, and the vector checks (`CI` in `xtask/src/main.rs`; `fix()` there lists the generators). Change the source (the catalogue of `docs/03-api-model.md` §9 for status codes and §9.1 for field codes, the SANA tables, the vector schema), run `cargo xtask fix`, and commit the result.

**Implementation agents commit and stop**, unless the brief grants the gate ([agent brief](docs/process/agent-brief.md)). The coordinator runs the merge gate (`cargo xtask verify --publish`, VER-058) per [docs/06-verification.md](docs/06-verification.md) §7, brings open PRs up to date with `scripts/reverify <pr>...` (a head that needs no container is verified at once, others on the hosted run), and clears finished worktrees and `worktree-agent-*` branches with `scripts/prune-agent-branches` (dry run; `--apply` acts).

Dispatching an agent: [docs/process/agent-brief.md](docs/process/agent-brief.md). Whether a pull request can merge: `scripts/pr-status <n>`. After it merges: `scripts/cleanup-merged <branch>`.

A failed exerciser link with `undefined symbol: aldis_panic_is_unreachable` means a panic path is reachable in `aldis-core`. Remove it; do not work around the check. In the crates the coverage gate measures (Layers 0–2 and the reported `aldis-ffi`, VER-024), mark new test modules `#[cfg_attr(coverage_nightly, coverage(off))]`; `xtask/` is not measured and needs no mark.

Each check in `xtask/src/verify/checks.rs` declares its inputs (`cargo xtask verify list` shows which checks HEAD needs); when a check starts reading a new path, add that path to its inputs in the same change.

Clippy runs with the `all` and `pedantic` groups denied, plus restriction lints and selected nursery lints ([docs/profiles/rust.md](docs/profiles/rust.md) §5). Do not enable the whole `nursery` group; adopt a nursery lint only after checking its findings, and record the decision there (RS-019). To relax a lint, use a local `#[expect(clippy::<lint>, reason = "…")]` on the smallest item (RS-016). Never use crate-wide `allow`.

## Conventions

- **Traceability:** every public item's docs cite the CCSDS clause and requirement IDs it implements (RS-015). Every test names the requirements it covers with a `// req: <ID>` comment (VER-020).
- **Verification records:** a requirement verified by test, vector, or a check of `cargo xtask verify` has no prose record: `cargo xtask trace` and the verify record are the evidence (a new check-to-requirement pair goes in `CHECK_REQUIREMENTS` in `xtask/src/trace.rs`). A requirement whose evidence is a person's judgement (method I, or an analysis no check runs) has a record file in [docs/verification/records/](docs/verification/records/), in the format of [analysis-records.md](docs/verification/analysis-records.md); it names its tests and checks and does not describe them. Change only the record you touch; there is no index.
- **Loop bounds (GEN-022):** a loop over a slice, an array, or a closed range (`a..b`) needs nothing. Put `// bound: <the bound and why the loop reaches it>` directly above every `while`, `while let`, `loop`, and every iteration over an open range or `repeat`/`cycle`/`from_fn`/`successors`; `cargo xtask loops` fails without it (`loops list` prints the loops). No recursion: the `stack` check fails on a call cycle. There are no inspection tables to update.
- **Test vectors** in [`test-vectors/`](test-vectors/README.md) are shared across the Rust and C ABI implementations. Follow [docs/07-test-vectors.md](docs/07-test-vectors.md). Record provenance, and never edit a released vector: add a new case and mark the old one `superseded`. An independent model (TV-023) is written blind: by an agent or person who has not read the implementation it checks, from the Blue Book and `docs/` only; brief a model-writing agent that way, and give it no implementation paths.
- **Pull request body:** Summary, Evidence, Merge Danger; link decisions, never restate them ([docs/process/pr-body.md](docs/process/pr-body.md)).
- **Changelog:** never edit `CHANGELOG.md` in a pull request. It is generated at release from the PR titles (`cargo xtask changelog`), so the title is the entry: a conventional commit, `type(scope)!: description` ([pr-body.md](docs/process/pr-body.md)).
- **Errors** use the stable catalogue in [docs/03-api-model.md](docs/03-api-model.md) §9. Do not renumber error codes.
- **Error codes** of the C ABI are generated from the catalogue of [docs/03-api-model.md](docs/03-api-model.md) §9 by `cargo xtask status --write`.
- **Field codes** come from the catalogue of [docs/03-api-model.md](docs/03-api-model.md) §9.1; `cargo xtask fields` checks the per-crate and common blocks.
- **C ABI names** use the `aldis_` / `ALDIS_` prefix ([docs/profiles/c-abi.md](docs/profiles/c-abi.md)).
- **Agent comments:** end every comment or review an agent posts on GitHub with the line `🤖 Generated with [Claude Code](https://claude.com/claude-code)`. Agents post under the maintainer's account, and `cargo xtask metrics` tells their words apart by this marker.
- **Search boundaries:** confine all file and text searches strictly to the active repository workspace; prefer git-aware search tools (`git grep`, `git ls-files`) over recursive filesystem walks (`grep -r`, `find .`). Never inspect external application or cache directories (e.g. `~/.gemini/`, `~/.cache`), `.git/`, or internal session worktrees (`.claude/`).
- **GitHub CLI queries:** always query GitHub CLI commands with structured output flags (`--json` or `-q`), never raw interactive commands (e.g. use `gh run view --json status,conclusion,jobs` or `gh run list --workflow= -L 1`).
- **Spec changes:** when behavior changes, update the requirement table in the same change. New requirements get the next free ID in their area; IDs are never reused. Open issues follow the same rule: take the next free `OI-<number>`; if `spec-lint` reports a collision after a merge of `main`, renumber your own row and its references ([09](docs/09-open-issues.md)).

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
