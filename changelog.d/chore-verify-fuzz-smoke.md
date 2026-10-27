### Added
- `cargo xtask fuzz smoke`: fast smoke iteration (`-runs=1 -max_total_time=2`) across all fuzz targets with corpus seeds when present, preventing stale harness assertions from passing merge gates (#135).
- Verify check `fuzz-smoke`: gating verification check running `cargo xtask fuzz smoke` on changes to Rust crates, fuzz targets, or workspace dependencies (#135).
