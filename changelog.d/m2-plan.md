### Added
- Plan for milestone M2, frame codecs ([docs/milestones/m2-frame-codecs.md](docs/milestones/m2-frame-codecs.md)): WP-0 with the C ABI of the M1 codecs, the field-code catalogue, and the shared property-test strategies, after which C1 can start; then `aldis-tm`, `aldis-tc`, `aldis-aos`, and `aldis-uslp`, each with its C ABI functions, vectors through the Rust API and the C ABI, and the M1 gates. Decisions D1–D17 are proposed. Linked from the roadmap and the docs index.
- Open issues OI-33 (frame anomaly flags and API-032), OI-34 (frame requirements that cross crates under GEN-002), and OI-35 (USLP and AOS readings found while planning M2) in [docs/09-open-issues.md](docs/09-open-issues.md); OI-04 records the reading of 732.0-B-4, which confirms AOS-002, AOS-003, and the FHEC for `Issue4Compatible`.
- USLP-014 (USLP Frame Validation Checks, 732.1 §4.3.11.3) and USLP-015 (Frame Delimiting and Fill
  Data Removal, 732.1 §4.3.11.2) in [docs/standards/uslp-frame.md](docs/standards/uslp-frame.md),
  for a physical channel over TC Synchronization and Channel Coding.
- `specs/manifest.tsv`: CCSDS 732.0-B-4 (Silver Book) in the `ref` group, for `Issue4Compatible`.
