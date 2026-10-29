### Changed
- `xtask`: `cargo xtask loops` is a check again, in `cargo xtask ci` and the verify check `spec`. A loop over a slice, an array, or a closed range is bounded by its type; every `while`, `while let`, `loop`, and open or endless iteration needs a `// bound: <reason>` comment directly above it, and a comment that is not above a loop fails (GEN-022).
- `docs/verification/records/GEN-022.md`, `docs/06-verification.md`, `docs/verification/assurance-case.md`, `AGENTS.md`: GEN-022 loop bounds are checked by `cargo xtask loops`; recursion stays with the `stack` check; no inspection tables to maintain.
- `aldis-core`, `aldis-ffi`: five loops annotated with their bounds (no behaviour change).
- `docs/`: removed the inspection-approval residue: `records/VER-061.md`, the decision-rights row for approving inspection records, and the glossary entry "Inspection record"; the VER-061 row is marked Withdrawn (the ID is not reused).
- `xtask`: `trace --scope` leaves requirements marked Withdrawn or Deleted out of the scope.
