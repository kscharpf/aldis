# Inspection: GEN-022 and GEN-023 in `aldis-aos`

| | |
|---|---|
| Requirements | GEN-022 (every loop bounded by an input length, a compile-time capacity, or a constant; no recursion), GEN-023 (time bounded by O(input length + capacities); nothing blocks or waits) |
| Scope | `crates/aldis-aos/src/*.rs`, library code only (test modules excluded), on branch `m2/aos-fhec` (M2 WP-4, module 1, based on `main` 8647d69). Only `fhec.rs` holds code; `anomaly.rs`, `bpdu.rs`, `config.rs`, `header.rs`, `mpdu.rs`, `view.rs`, and `writer.rs` are module headers only, and `lib.rs` declares the modules and re-exports two `fhec` types |
| Inspected | 2026-10-20, prepared by Claude Code: first inspection. Every loop, every call between the crate's own functions, and every blocking construct checked. No unbounded loop, no recursion |
| Approved | 2026-10-21, maintainer |
| Repeat | when `aldis-aos` gains a loop or a call between its own functions (each later WP-4 module), and before each release |

## Loops

Every loop in library code, with its bound. Copies and comparisons of fixed-size arrays (the destructuring of the 6- and 8-octet headers, `[s6, s7, s8, s9] == [Gf::ZERO; 4]`, derived `PartialEq`/`Hash`) run over a constant length and are not listed. The table lookups `Gf::alpha_pow` (index `e % 15`) and `Gf::log` (`get`) contain no loop.

| Location | Loop | Bound |
|---|---|---|
| `fhec.rs` `encode` | over the six systematic symbols (division by g(x) in a shift register) | constant: 6 |
| `fhec.rs` `decode`, syndromes | `ROOTS.map` over the four roots, each a `fold` over the ten transmitted symbols (Horner's rule) | constant: 4 × 10 |
| `fhec.rs` `decode`, root search | over the ten transmitted symbols (`iter_mut().rev()`), evaluating the locator and, at a root, Forney's formula | constant: 10 (GEN-022, M2 plan D10: the transmitted positions only) |

## Recursion

No function calls itself, directly or through another `aldis-aos` function. The internal call graph is acyclic and at most four functions deep, for example `write` → `encode` → `Gf::mul` → `Gf::log` (or `Gf::alpha_pow`). Other calls: `decode` → `word` → `Gf::high`, `Gf::low`; `decode` and `encode` → `Gf::add`, `Gf::mul`, `Gf::octet`; `decode` → `Gf::inv` → `Gf::log`, `Gf::alpha_pow`; `decode` → `CorrectedSymbols::get`. The only call out of the crate is `aldis_core::Error::new`.

## Blocking

There is no I/O, locking, atomics, sleep, or spin-wait. `std` and `alloc` are only behind features (crate-root `extern crate` declarations) and in test modules. `encode`, `write`, and `decode` take fixed-size arrays and run in constant time; `CorrectedHeader` and `CorrectedSymbols` accessors are O(1).
