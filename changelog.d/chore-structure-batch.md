### Added
- `cargo xtask consistency`, part of `cargo xtask ci` and the nightly `ci.yml` job `spec`: fails
  when the registrations disagree. Every Layer 0–2 crate that declares a function is in the
  coverage gate, the `features` check, the nightly `mutants` job, and the exerciser (with a
  vector group), and no skeleton is; every crate has a README linked from `README.md` and a
  facade feature in `full`; the fuzz targets of `fuzz/Cargo.toml` and the nightly fuzz matrix
  are the same; every vector file is listed in `test-vectors/README.md`, named by a test in
  `crates/*/tests/`, and embedded by the exerciser when its crate is exercised.

### Changed
- `docs/verification/analysis-records.md` is split into one file per record in
  `docs/verification/records/`, named after the first requirement ID it covers, so that a pull
  request that changes one requirement's record touches one small file. `cargo xtask trace` reads
  every file there and fails on a malformed record, one named after another ID, or a requirement
  with two records (VER-020); the trace results are unchanged. `analysis-records.md` describes
  the format.
- Milestone plans carry a work-package status table (WP, State, PRs, Notes), one row per work
  package or sub-part, and quote no requirement counts: `cargo xtask trace --scope <id>` reports
  them ([docs/08-roadmap.md](docs/08-roadmap.md) §6 rule 1, `AGENTS.md`). The M2 plan's status
  paragraph becomes a status sentence, the table, and a short history; the C0 and A0 status
  paragraphs carry no counts and stay as they are.

### Fixed
- `cargo xtask features` checks `aldis-aos` too (GEN-006): it joined the coverage gate in M2
  WP-4 but not the `features` list, which `cargo xtask consistency` reported. Its `alloc` and
  `std` features are additive.
