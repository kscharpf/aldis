### Added
- `test-vectors/cop1/`: eight generated files (`fop1_table.json`, `farm1_table.json`, six by topic) with one scenario per cell of CCSDS 232.1 Table 5-1 (FOP-1, 300 cells) and Table 6-1 (FARM-1, 33 cells), "Not applicable" cells asserting `InvalidState`, and scenarios for the normal run, retransmission, the sliding window, the timer, suspension, and the FARM-1 window (COP-001, COP-002, COP-005, TV-011).
- `test-vectors/models/cop1.py`: the independent model of FOP-1 and FARM-1 behind those scenarios, written blind from the Blue Books (TV-023).

### Changed
- `docs/09-open-issues.md`: OI-59 (FOP-1 start-up values), OI-60 (NN(R) after a purge), OI-61 (the two Accepts of E21 and E45), resolved by the maintainer; `docs/standards/cop1.md` records the Abort Request action, event precedence, and `StateChanged` rules.
- `cargo xtask trace` ignores vectors that no runner runs yet; OI-62 (E15 precedence) opened.
- `aldis-testkit`: an engine-scenario file holds many scenarios with named, shared setups (`ScenarioFile::resolve`); `expect_timeout_us` is absent, `null` (no deadline), or a value. `cargo xtask vectors generated` fails when `test-vectors/cop1/` differs from what the model writes.
