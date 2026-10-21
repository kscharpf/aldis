### Changed
- `docs/verification/analysis-records.md` is split into one file per record in
  `docs/verification/records/`, named after the first requirement ID it covers, so that a pull
  request that changes one requirement's record touches one small file. `cargo xtask trace` reads
  every file there and fails on a malformed record, one named after another ID, or a requirement
  with two records (VER-020); the trace results are unchanged. `analysis-records.md` describes
  the format.
