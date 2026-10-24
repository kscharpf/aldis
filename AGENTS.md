# AGENTS.md

Guide for coding agents working on **Aldis**. Read [README.md](README.md) first for the project overview, crate layout, and build commands. This file adds the rules you need before changing anything.

## Sources of truth

1. **CCSDS Blue Books** are authoritative for wire behavior. They are not in the repository. Run `specs/fetch.sh` to download them into `specs/pdf/` (see [specs/README.md](specs/README.md)). If this repository disagrees with a Blue Book, the Blue Book wins: fix the spec or the code, never the other way round. **Never commit the PDFs or text copied from them.** Cite them by document number and clause.
2. **The specification** in [`docs/`](docs/README.md) defines everything else. Start with `docs/README.md`. Every requirement has an ID (e.g. `SPP-002`, `GEN-020`) and cites its CCSDS clause.
3. **Open decisions** are tracked in [`docs/09-open-issues.md`](docs/09-open-issues.md). Do not silently resolve an open issue in code: update the issue.

Terms and acronyms: [`docs/glossary.md`](docs/glossary.md).

Never write protocol values from memory. Look them up in the Blue Book. Registry values (UPIDs, EPIs, PVNs, time systems) come from SANA (OI-06).

## Tracks

v1 is built in three tracks: **R** (Rust reference, milestones M0–M6), **C** (native C++, C0–C5), and **A** (reference application, A0–A6). Before starting work, find your track and milestone in [docs/08-roadmap.md](docs/08-roadmap.md), check that its entry gate is met on `main`, and follow the parallel-work rules in §6: one track's directories per PR, branch prefix `m<N>/`, `c<N>/`, or `a<N>/`, and the Rust crates as the reference when implementations disagree.

A milestone plan carries a work-package status table, one row per WP or sub-part (WP, State, PRs, Notes); update only your WP's row. Never write requirement counts into a plan: `cargo xtask trace --scope <id>` reports them (roadmap §6 rule 1). Before calling a work package done, go through its checklist in [docs/milestones/checklists.md](docs/milestones/checklists.md) (adding a codec module, a crate, a vector file, a fuzz target, C ABI functions; a spec-readings PR; a milestone exit); `cargo xtask consistency` checks the registrations it marks.

The rules and checks below are written for the Rust crates. The native C++ code follows [docs/profiles/cpp.md](docs/profiles/cpp.md) instead; its checks are set up in milestone C0.

## Decision rights

[docs/process/decision-rights.md](docs/process/decision-rights.md) says who decides what: the maintainer, the coordinating session that dispatches agents and opens pull requests, or the agent of a work package. In short:

- **Decide yourself**, and list it under "Judgement calls" in your report: how to build what an accepted plan decision says; a Blue Book reading the normative text settles (cite the clause); new vector cases, and the layout of vector files not yet released; fixes to review findings that change no behaviour, spec, or ABI.
- **Leave to the maintainer**, through the coordinator, as one question with a recommended option: a reading the Blue Book does not settle (as an OI), scope and plan decisions, C ABI and public API conventions, new dependencies, releases, tags, visibility and anything external, approvals, and merges to `main`. Meanwhile, continue the work the answer does not touch.
- **Never ask** about what a rule of this file, a checklist, or a check already decides.

Every maintainer decision gets a line in the [decision log](docs/process/decisions.md) (VER-062), which `cargo xtask decisions` checks.

## Independent review

A reviewer of a pull request does **not** re-run the verification checks: `cargo build`, `cargo check`, `cargo test`, `cargo clippy`, `cargo fmt`, `cargo xtask ci`, coverage, the exerciser, fuzzing, or `cargo xtask verify`. They are the merge gate's job (VER-056…VER-058), and the gate already ran them on the head commit. Read the evidence instead: `cargo xtask verify check <head>` re-checks the published record, and a check that does not gate (`cpp-leon3`, `rust-sparc-leon3`) is read from the record, not assumed to pass. `kani` is nightly-only: a pull request's record shows it as not run ("nightly only (VER-059)"), and its results are in the nightly run on `main`, whose failure opens the nightly issue.

Spend the review on what no check covers: the reading of the Blue Book behind each decision (cite the clause), the requirements and whether the tests and vectors really cover them, the soundness of every `unsafe` block, the shape of the public API and C ABI, and the decisions and judgement calls the pull request lists. Read every suppression the change adds, because the gate passes *because of* it: each `#[expect(…, reason = "…")]` (RS-016), `// NOLINT(<check>): <reason>` (CPP-042), entry of `coverage-waivers.toml` (VER-024), and mutant triaged as equivalent in [mutants-triage.md](docs/verification/mutants-triage.md) (VER-025). Check that the reason is true and the region is the smallest one; a reason that does not hold is a finding. Report findings as [decision-rights.md](docs/process/decision-rights.md) §2.6 describes, each with the file and line and why it matters; to show a defect, a failing test or vector case is the clearest form.

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
cargo xtask verify --publish        # every check, in the pinned image, stopping at the first failure; then the signed git note and status aldis/verify
cargo xtask verify check            # re-check the published record of HEAD and its signature; it fails while HEAD lacks the tip of main
```

`--publish` signs the record with the SSH key that git signs with, and the gate accepts only a record signed by a key that `.github/allowed_signers` lists on `main` (VER-064). Set the key up once per clone (`gpg.format ssh`, `user.signingKey` naming the public key whose private half is in your SSH agent); an agent never creates, reads, or handles a private key, and never edits `.github/allowed_signers`, which changes only in a reviewed pull request of its own.

If you cannot run it locally, start the same run on a GitHub-hosted runner instead (VER-060). It takes 20 to 30 minutes when it has to build the image, and less when a workflow has already pushed the image for the same pins to `ghcr.io/kscharpf/aldis-verify` (VER-060). It publishes the same record, unsigned, and names the run it came from; an allowed signer then countersigns it, which checks the run and its artefact and sets the status:

```sh
git push
gh workflow run verify.yml --ref <branch>   # or "Run workflow" on the Actions tab
gh run watch                                # then: cargo xtask verify sign <commit>; cargo xtask verify check
```

`cargo xtask verify` needs podman or Docker Engine with about 20 GB of free disk and 6 GB of memory for the engine's VM (`podman machine set --memory 6144`). The first run builds the image (several GB of downloads). A podman VM keeps the disk space the build frees until it is trimmed: run `podman machine ssh sudo fstrim -av` after the build (it returned about 20 GB on macOS). It refuses a dirty working tree, runs every check in a fresh checkout of HEAD, and writes `target/verify/<commit>/record.json` and one log per check. `cargo xtask verify list` names the checks, and `--only <check>,...` runs some of them while you work; such a record never passes the gate. The checks run quickest first. With `--publish` the run stops at the first gating check that fails, since it could not publish, and records the rest as not run (`--no-fail-fast` runs them all; `--fail-fast` stops a run without `--publish`). When the input rule skips or reuses every gating check, the run starts no container and pulls or builds no image. If `main` moves during a passing run, `--publish` still publishes the record, which a later commit can reuse, but sets no `aldis/verify` status: merge `main` and verify again. The nightly-only check `kani` runs only with `--all`, as the nightly run does (VER-059). Runs on one machine share the engine's cache volume and take turns: a run started while another is in progress waits for it to finish. A record is tied to the commit's tree, so any new commit, including a merge from `main`, needs a new run.

While you work, run these. All must pass with **zero warnings**:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-features --all-targets
cargo test --workspace --all-features
cargo clippy -p aldis --no-default-features --features full --target thumbv7em-none-eabihf
cargo xtask ci        # spec lint, links, decision log, layering, registrations (consistency), vectors, schema, generated files (field codes, C header), traces, changelog, C++ opt-outs
```

When you change `aldis-core`, also run:

```sh
cargo xtask coverage                                   # 100 % regions/functions/lines/branches (VER-024)
(cd exerciser && cargo build --release --target thumbv7em-none-eabihf)  # panic-freedom link check (VER-010)
cargo clippy --manifest-path fuzz/Cargo.toml --all-targets               # fuzz targets still compile
```

A failed exerciser link with `undefined symbol: aldis_panic_is_unreachable` means a panic path is reachable in `aldis-core`. Remove it; do not work around the check. Mark new test modules `#[cfg_attr(coverage_nightly, coverage(off))]`.

`cargo xtask ci` checks every generated file (`fields`, `header`), so it needs the pinned `cbindgen` (`cargo install --locked cbindgen@0.29.4`) in any work package; without it, `ci` reports the missing tool as a problem. When you change `aldis-ffi` (the C ABI) or a crate it wraps, regenerate and check the header with it ([crates/aldis-ffi/README.md](crates/aldis-ffi/README.md)), and never edit `include/aldis/aldis.h` by hand:

```sh
cargo xtask header --write && cargo xtask header   # CABI-002, CABI-003
cargo xtask header c-smoke cc c++                  # host libraries, C99 and C++11 (CABI-001)
```

When you change `cpp/` (the native C++ implementation, [C++ profile](docs/profiles/cpp.md)), use the toolchains pinned in `cpp/toolchains.lock`; every preset refuses another compiler. Also run:

```sh
(cd cpp && cmake --preset host-gcc && cmake --build --preset host-gcc && ctest --preset host-gcc)  # and host-clang
cargo xtask cpp unsafe-optouts                 # bounds opt-outs only in aldis/core/bytes.hpp (CPP-031)
cargo xtask cpp layers target/cpp/host-gcc     # CMake target graph (CPP-010)
```

`cargo xtask verify` also runs clang-format, clang-tidy, the sanitizers, s390x, and the bare-metal builds. Each check declares its inputs in `xtask/src/verify/checks.rs` and is skipped when none of them changed against the merge base with `main` (`cargo xtask verify list` shows which checks HEAD needs; `--all` runs every check): a change to documentation only (`docs/`, `changelog.d/`, the root `*.md`, `specs/README.md`, `specs/manifest.tsv`) or to the GitHub configuration that no check reads (`.github/workflows/`, `.github/actions/setup-rust/`, `.github/actions/setup-cpp/action.yml`, the pull-request template, `CODEOWNERS`, `dependabot.yml`, `allowed_signers`) or to `.gitignore` runs only `spec`, a change to `xtask/` runs everything, and so does a path that no list names. After you merge `main` into a branch whose head has a published record, a check whose inputs the commits from `main` did not touch is `reused` from that record instead of run. When a check starts reading a new path, add the path to its inputs in the same change (as `CPP` for the C++ build). Suppress a warning or clang-tidy finding only on the smallest region, naming the check (CPP-042).

When you change `refapp/` (track A), also run, from `refapp/` (see [refapp/README.md](refapp/README.md)):

```sh
cargo fmt --all --check && cargo clippy --workspace --all-targets && cargo test --workspace
cargo build -p refapp-flight --target thumbv7em-none-eabihf
```

Clippy runs with the `all` and `pedantic` groups denied, plus restriction lints and selected nursery lints ([docs/profiles/rust.md](docs/profiles/rust.md) §5). Do not enable the whole `nursery` group; adopt a nursery lint only after checking its findings, and record the decision there (RS-019). To relax a lint, use a local `#[expect(clippy::<lint>, reason = "…")]` on the smallest item (RS-016). Never use crate-wide `allow`.

## Conventions

- **Traceability:** every public item's docs cite the CCSDS clause and requirement IDs it implements (RS-015). Every test names the requirements it covers with a `// req: <ID>` comment (VER-020).
- **Verification records:** requirements verified by a check of `cargo xtask verify` or by review (method A/I) each have a record file in [docs/verification/records/](docs/verification/records/), named after its first requirement ID, in the format of [analysis-records.md](docs/verification/analysis-records.md). Change only the record you touch; there is no index.
- **Inspections (GEN-022/023):** when your change adds or changes a loop, a call between a crate's own functions, or a blocking construct in library code, revise that crate's record in [docs/verification/inspections/](docs/verification/inspections/) in the same change: update the loop table and call graph, and add a `Revised` row "<date>, <branch> (<WP>), prepared by <you>, pending exit review: …". Do not ask for or wait for a maintainer approval, and never edit an `Approved` row: the maintainer approves every pending revision once, in the milestone exit review or before a release (VER-061).
- **Test vectors** in [`test-vectors/`](test-vectors/README.md) are shared with future C/C++ and binding implementations. Follow [docs/07-test-vectors.md](docs/07-test-vectors.md). Record provenance, and never edit a released vector: add a new case and mark the old one `superseded`.
- **Changelog:** never edit `CHANGELOG.md` in a pull request. Add one fragment file to [`changelog.d/`](changelog.d/README.md), named after the branch; `cargo xtask changelog check` validates it.
- **Errors** use the stable catalogue in [docs/03-api-model.md](docs/03-api-model.md) §9. Do not renumber error codes.
- **Field codes** come from the catalogue of [docs/03-api-model.md](docs/03-api-model.md) §9.1. A field of one new crate takes the next code of that crate's block of 50, reserved when its plan is accepted; a field shared by several crates, or of a crate without a block, takes the next code of the common block 107–149. Because each new crate has its own block, codes need no renumbering at merge (roadmap §6 rule 7 still settles a clash in one block). `cargo xtask fields` checks the blocks.
- **C ABI names** use the `aldis_` / `ALDIS_` prefix ([docs/profiles/c-abi.md](docs/profiles/c-abi.md)).
- **Process metrics:** `cargo xtask metrics` measures PR cycle time, the stages of each PR (raw and in the maintainer's active time), the process-created waits, the process overhead (the shares of pull requests, commits, and verify minutes spent on process rather than product, and the bookkeeping-only commits), verify cost, merge conflicts, and review outcomes; the nightly report is the pinned issue "Process metrics" ([docs/06-verification.md](docs/06-verification.md) §9, VER-063).
- **Agent comments:** end every comment or review an agent posts on GitHub with the line `🤖 Generated with [Claude Code](https://claude.com/claude-code)`. Agents post under the maintainer's account, and `cargo xtask metrics` tells their words apart by this marker.
- **Spec changes:** when behavior changes, update the requirement table in the same change. New requirements get the next free ID in their area; IDs are never reused.

## Where things go

| Change | Location |
|---|---|
| Shared algorithm or type (CRC, counters, IDs, security hooks) | `crates/aldis-core` |
| Codec for one standard | `crates/aldis-<std>` (`time`, `spp`, `epp`, `tm`, `tc`, `aos`, `uslp`) |
| Stateful protocol behavior | `crates/aldis-link`, or `crates/aldis-cop1` for COP-1 |
| Transports, run loop, adapters | `crates/aldis-framework` |
| C interface | `crates/aldis-ffi` (capacities in `capacities.toml`) |
| Native C++ implementation (track C) | `cpp/` |
| Differential tests, Rust vs. native (VER-053) | `difftest/` |
| Reference application (track A) | `refapp/` |
| Requirements and design | `docs/` |
| Conformance data | `test-vectors/` |
| Native C++ implementation (track C) | `cpp/` ([cpp/README.md](cpp/README.md)) |
