### Added
- `crosscheck/` (M1 WP-4, VER-040): checks `time/pfield`, `time/cuc`, `time/cds`, ASCII
  formatting in `time/ascii`, and the Time Code Field of `spp/secondary_header_time` against
  `spacepackets` 0.18.0, with no disagreement. Four differences in `spacepackets` are resolved
  against 301.0 and reported as known discrepancies under the exact condition each describes: the
  CUC fine-time divisor, the reserved CDS sub-millisecond code, the ignored P-field extension
  flag, and the rounding from nanoseconds to fine time (`docs/verification/cross-checks.md` §3).
  §4 there records, for each feature `spacepackets` does not cover, what was searched and which
  candidates qualify.
- `docs/09-open-issues.md`: OI-32 records the independent implementations M1 found but does not
  run (VER-040), targeted at M2.
