### Added
- `aldis-cop1`: the FARM-1 engine (M3 WP-3; COP-002, COP-006, COP-030…COP-036). `Farm1::step` is a pure step function over plain-data state that executes the cells of `FARM1_TABLE` (232.1 Table 6-1) with exhaustive matching, returns the next state and at most `FARM1_MAX_ACTIONS` actions (derived from the table at compile time), and writes an accepted FDU to a caller-supplied output area; `Farm1::clcw` is the pure E11 query. `Farm1Window` checks W, PW, and NW against 232.1 §6.1.8 and Table 7-2. Also `Actions` and `OutRange`, the action list and output-area range shared by the engines (API-057, API-061).
- `aldis-tc`: `FarmBCounter::next`, the increment modulo 4 (232.1 §6.1.6).
- `docs/09-open-issues.md`: OI-58, the start-up values of FARM-1.
- `exerciser`: calls the FARM-1 API; registers the `cop1` vector group, empty until M3 WP-2.
