### Added
- `crates/aldis-cop1/src/fop1_table.rs`: represented FOP-1 State Table 5-1 as `const` data with closed enums `Fop1State`, `Fop1Event` (50 variants), `Fop1Action`, `FrameKind`, and `BcCommand` (COP-001, COP-003, COP-004, RS-021).
- `crates/aldis-cop1/tests/fop1_table.rs`: added exhaustive 300-cell unit tests covering all states S1–S6 across events E1–E46 and E101–E104 against CCSDS 232.1-B-2 Table 5-1, Cor. 1, and `docs/verification/reviews/cop1-tables.md` (COP-001, COP-003, COP-004, COP-005, RS-021).
