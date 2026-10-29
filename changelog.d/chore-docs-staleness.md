### Changed
- `cargo xtask assurance`: a code span that names an `aldis_` symbol, a `.rs`, `.h`, or `.toml` path, or a `::` path is an error in the hazard analysis, the assurance case, the verification records, the cross-checks, and the formal-proofs file (VER-065, VER-066); the new reference kind `sym:` resolves an `aldis_` name against the C header.
- Documentation that restated code is removed: wire diagrams, configuration tables, and vector lists of `docs/standards/`, the API sketches of the Rust profile, the illustrative C of the C ABI profile, and the status text of the crate READMEs. Dated reviews carry a banner.

### Fixed
- Dangling names in the hazard analysis, the assurance evidence rules, and the USLP standard (`aldis_tc_stream_next`, `truncated_frame_len`), and mentions of the removed `resume` and C two-phase writers.
