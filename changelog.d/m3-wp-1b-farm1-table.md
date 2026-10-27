### Added
- `crates/aldis-cop1/src/notify.rs`: defined common COP-1 types `Directive`, `AlertReason`, `Alert`, `TransferNotification`, `DirectiveNotification`, and `TimeoutType` (CCSDS 232.1-B-2 Section 4, COP-004).
- `crates/aldis-cop1/src/farm1_table.rs`: represented FARM-1 State Table 6-1 as `const` data with closed enums `Farm1State`, `Farm1Event`, and `Farm1Action` (COP-002, COP-003, RS-021).
- `crates/aldis-cop1/tests/farm1_table.rs`: added exhaustive 33-cell unit tests covering all states S1–S3 across events E1–E11 against CCSDS 232.1-B-2 Table 6-1 and `docs/verification/reviews/cop1-tables.md` (COP-002, COP-003, COP-004, RS-021).
