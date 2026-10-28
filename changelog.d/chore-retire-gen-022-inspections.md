### Changed
- `docs/02-design-rules.md`: GEN-022 and GEN-023 verification method changed from Inspection (`I`) to Analysis (`A`), backed by static object-level call-graph cycle detection (`cargo xtask stack`, VER-011), `#![no_std]` architecture invariants, and formal proofs.
- `xtask`: `cargo xtask loops` removed from CI and verify checks; simplified to a diagnostic utility `cargo xtask loops [list]` that scans library crates for loops.

### Removed
- `docs/verification/inspections/`: retired all 9 `gen-022-023-*.md` Markdown inspection tables to eliminate maintenance churn and merge conflicts across parallel branches.
- `docs/06-verification.md`: deleted requirement VER-061 (per-PR inspection record maintenance).
