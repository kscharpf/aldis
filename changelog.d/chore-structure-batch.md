### Added
- `cargo xtask consistency`, part of `cargo xtask ci` and the nightly `ci.yml` job `spec`: fails
  when the registrations disagree. Every Layer 0–2 crate that declares a function is in the
  coverage gate, the `features` check, the nightly `mutants` job, and the exerciser (with a
  vector group), and no skeleton is; every crate has a README linked from `README.md` and a
  facade feature in `full`; the fuzz targets of `fuzz/Cargo.toml` and the nightly fuzz matrix
  are the same; every vector file is listed in `test-vectors/README.md`, named by a test in
  `crates/*/tests/`, and embedded by the exerciser when its crate is exercised.
- The independent models of the hand-derived vectors are committed under `test-vectors/models/`
  (new TV-023, [docs/07-test-vectors.md](docs/07-test-vectors.md) §4): `aos_fhec.py` for
  `test-vectors/aos/fhec_*.json` and `core_ocf.py` for `test-vectors/core/ocf.json`, each with
  `--check` (recompute every expected value; exit 1 on a difference) and `--write`.
  `cargo xtask vectors validate` runs every model that a `provenance_note` names, which needs
  `python3` (installed in the verification image). The provenance notes of those six files,
  not released yet, name their model. `test-vectors/models/README.md` records that the M1
  models were not kept. TV-023 is in the M2 exit criteria.
- `docs/milestones/checklists.md`: the definition of done for the recurring kinds of work (a
  codec module, a crate, a vector file, a fuzz target, C ABI functions, a spec-readings pull
  request, a milestone exit), each item citing the requirement or check that enforces it and
  marking those `cargo xtask consistency` checks. Linked from `AGENTS.md`, `docs/README.md`, and
  roadmap §6 rule 1.

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
- GEN-022/023 inspections are approved once per milestone (maintainer decision 2026-10-21; new
  VER-061, [docs/06-verification.md](docs/06-verification.md) §8): the change that adds a loop
  or a call revises the crate's record in `docs/verification/inspections/`, prepared by its
  author and marked pending exit review, and the maintainer approves every revision since the
  last approval at the milestone exit review or before a release. No pull request waits for an
  approval. `AGENTS.md` and the M2 plan (WP-7 and a new §4 exit row) follow; past approvals
  stand.

### Fixed
- `cargo xtask features` checks `aldis-aos` too (GEN-006): it joined the coverage gate in M2
  WP-4 but not the `features` list, which `cargo xtask consistency` reported. Its `alloc` and
  `std` features are additive.
