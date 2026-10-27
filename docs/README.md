# Aldis: Specification

**Status:** Draft 0.1 · **Scope:** v1 (core packets and links) · **Applies to:** Rust reference implementation, C ABI, native C++ implementation, and future native C port

This directory holds the specification for a family of libraries that add CCSDS protocol support to spacecraft flight software and ground systems. The specification is **language-neutral**. Language-specific *profiles* define how each implementation exposes it. A shared **conformance test-vector suite** keeps every implementation behaving the same.

## Reading order

| # | Document | What it defines |
|---|---|---|
| 00 | [Introduction](00-introduction.md) | Purpose, v1 scope, non-goals, terminology, requirement conventions |
| — | [Glossary](glossary.md) | Acronyms and abbreviations (OCF, CLCW, FHP, …) with their Blue Book clauses, Aldis terms (tracks, WPs, provenance, verification run, …), and the Blue Book document numbers. Keep it open while reading the rest |
| 01 | [Architecture](01-architecture.md) | Layers (codecs → engines → framework), crate map, dependency rules, data flows |
| 02 | [Design rules](02-design-rules.md) | `no_std`/no-heap, panic freedom, bounded resources, error model, bit conventions |
| — | [Hardware assumptions](hardware-assumptions.md) | What Aldis assumes of the platform: memory protected against single-event upsets (GEN-015), and what an upset in unprotected memory does |
| 03 | [API model](03-api-model.md) | Language-neutral concepts: views, writers, configs, sans-I/O engines, security hooks, error catalogue |
| 04 | [Engines](04-engines.md) | Stateful protocol behavior: packet multiplexing and extraction, channel multiplexing, segmentation |
| 05 | [Framework](05-framework.md) | Optional layer: transport/clock traits, run loop, reference adapters |
| 06 | [Verification](06-verification.md) | Mission-grade assurance: lints, panic-freedom, fuzzing, coverage, traceability |
| 07 | [Test vectors](07-test-vectors.md) | Shared, language-neutral conformance vector format |
| 08 | [Roadmap](08-roadmap.md) | The three v1 tracks (Rust reference, native C++, reference application): milestones, entry gates, releases, rules for parallel work; later standards; language rollout |
| 09 | [Open issues](09-open-issues.md) | Decisions and verifications still pending |

### Verification records

[`verification/analysis-records.md`](verification/analysis-records.md): how requirements verified by analysis or inspection (method A/I) are recorded, one file per record in [`verification/records/`](verification/records/), read by `cargo xtask trace`.

[`verification/hazards.md`](verification/hazards.md) and [`verification/assurance-case.md`](verification/assurance-case.md): the hazard and failure-mode analysis, one file per frame codec in [`verification/hazards/`](verification/hazards/), and the assurance case that argues from the library's top claims to its evidence (VER-065, VER-066); [`verification/assurance-evidence.md`](verification/assurance-evidence.md) specifies how `cargo xtask assurance` checks their references.

[`verification/formal-proofs.md`](verification/formal-proofs.md): the Kani proof harnesses of the formal-verification spike, what they prove and up to which bounds, and the limits found (OI-42).

### Process ([`process/`](process/))

| Document | What it defines |
|---|---|
| [Decision rights](process/decision-rights.md) | Who decides what: the maintainer, the coordinating session, or the agent of a work package, per category (spec readings, scope, plans, API and ABI, vectors, reviews, process, dependencies, releases, merges, housekeeping, approvals); how to ask the maintainer, and what continues meanwhile (VER-062) |
| [Decision log](process/decisions.md) | One line per maintainer decision, with its date, category, and the pull request or issue that records it; checked by `cargo xtask decisions` (VER-062) |
| [Independent review](process/review.md) | Protocol for independent PR review: verification evidence over re-running checks, suppression verification, unautomated review focus, and citation of code file:line |

### Reference application

[`reference-application.md`](reference-application.md): the scenario of the end-to-end flight↔ground application in `refapp/` (track A, area APP): roles, APIDs, link plan, time, link model and faults, golden-trace format, node interface, and implementation rules.

### Milestone plans ([`milestones/`](milestones/))

[Work-package checklists](milestones/checklists.md): the definition of done for recurring work (codec modules, crates, vector files, fuzz targets, C ABI functions, spec-readings PRs, milestone exits), and which items `cargo xtask consistency` checks.

| Document | Milestone |
|---|---|
| [M0 Foundations](milestones/m0-foundations.md) | Repository, CI, verification tooling, `aldis-core` |
| [M1 Packets and time](milestones/m1-packets-time.md) | `aldis-time`, `aldis-spp`, `aldis-epp` (complete) |
| [M2 Frame codecs](milestones/m2-frame-codecs.md) | C ABI for the M1 codecs, field codes, shared property-test strategies; `aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp` (planned) |
| [C0 Foundations (native C++)](milestones/c0-foundations.md) | Native C++ track foundations (retired; deferred to Phase 3) |
| [A0 Scenario](milestones/a0-scenario.md) | Reference application: scenario specification (area APP) and `refapp/` skeleton (plan approved) |
| [ACFS cFS flight side](milestones/acfs-flight-side.md) | Reference application: a cFS flight side over the C ABI, reproducing the golden traces in four phases (plan approved; decisions D1–D19 accepted) |

### Per-standard specifications ([`standards/`](standards/))

| Document | CCSDS source |
|---|---|
| [Common link features](standards/common-link-features.md) | FECF CRC-16, OCF, OID idle pattern, frame counts, SDLS field placement (shared by 132.0 / 232.0 / 732.0 / 732.1) |
| [Time codes](standards/time-codes.md) | 301.0-B-4 |
| [Space Packet](standards/space-packet.md) | 133.0-B-2 |
| [Encapsulation Packet](standards/encapsulation-packet.md) | 133.1-B-3 |
| [TM frames](standards/tm-frame.md) | 132.0-B-3 |
| [TC frames and CLCW](standards/tc-frame.md) | 232.0-B-4 (+Cor. 1) |
| [AOS frames](standards/aos-frame.md) | 732.0-B-5 |
| [USLP frames](standards/uslp-frame.md) | 732.1-B-3 |
| [COP-1](standards/cop1.md) | 232.1-B-2 (+Cor. 1) |

### Profiles ([`profiles/`](profiles/))

| Document | Defines |
|---|---|
| [Rust profile](profiles/rust.md) | Crate names, API shapes, feature flags, MSRV, lint configuration |
| [C ABI profile](profiles/c-abi.md) | C linkage, naming, memory model (caller-owned storage), status codes, header generation, C++ wrapper guidance, rules for native C/C++ ports |
| [C++ profile](profiles/cpp.md) | Native C++ implementation: language subset, API shapes, build, lint configuration, verification, differential testing against the Rust reference (draft) |

## Source material

- Standards: the CCSDS Blue Books. They are © CCSDS and are **not included in this repository**. [`specs/README.md`](../specs/README.md) explains how to download them into the git-ignored `specs/pdf/` (`specs/fetch.sh`), with checksums in [`specs/manifest.tsv`](../specs/manifest.tsv).
- Every requirement in this specification cites the CCSDS clause it implements. Where the Blue Book and this document disagree, **the Blue Book governs** and the discrepancy is a defect in this document.
