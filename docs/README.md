# Aldis: Specification

**Status:** Draft 0.1 · **Scope:** v1 (core packets and links) · **Applies to:** Rust reference implementation, C ABI, and future native C/C++ ports

This directory holds the specification for a family of libraries that add CCSDS protocol support to spacecraft flight software and ground systems. The specification is **language-neutral**. Language-specific *profiles* define how each implementation exposes it. A shared **conformance test-vector suite** keeps every implementation behaving the same.

## Reading order

| # | Document | What it defines |
|---|---|---|
| 00 | [Introduction](00-introduction.md) | Purpose, v1 scope, non-goals, terminology, requirement conventions |
| 01 | [Architecture](01-architecture.md) | Layers (codecs → engines → framework), crate map, dependency rules, data flows |
| 02 | [Design rules](02-design-rules.md) | `no_std`/no-heap, panic freedom, bounded resources, error model, bit conventions |
| 03 | [API model](03-api-model.md) | Language-neutral concepts: views, writers, configs, sans-I/O engines, security hooks, error catalogue |
| 04 | [Engines](04-engines.md) | Stateful protocol behavior: packet multiplexing and extraction, channel multiplexing, segmentation |
| 05 | [Framework](05-framework.md) | Optional layer: transport/clock traits, run loop, reference adapters |
| 06 | [Verification](06-verification.md) | Mission-grade assurance: lints, panic-freedom, fuzzing, coverage, traceability |
| 07 | [Test vectors](07-test-vectors.md) | Shared, language-neutral conformance vector format |
| 08 | [Roadmap](08-roadmap.md) | Milestones for v1, later standards, language rollout |
| 09 | [Open issues](09-open-issues.md) | Decisions and verifications still pending |

### Milestone plans ([`milestones/`](milestones/))

| Document | Milestone |
|---|---|
| [M0 Foundations](milestones/m0-foundations.md) | Repository, CI, verification tooling, `aldis-core` |

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

## Source material

- Standards: the CCSDS Blue Books. They are © CCSDS and are **not included in this repository**. [`specs/README.md`](../specs/README.md) explains how to download them into the git-ignored `specs/pdf/` (`specs/fetch.sh`), with checksums in [`specs/manifest.tsv`](../specs/manifest.tsv).
- Every requirement in this specification cites the CCSDS clause it implements. Where the Blue Book and this document disagree, **the Blue Book governs** and the discrepancy is a defect in this document.
