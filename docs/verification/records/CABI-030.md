# Record: CABI-030

| | |
|---|---|
| Requirements | CABI-030 |
| Method | A |
| Check | Verify check `header`: `capacities.rs` regenerated |

`cargo xtask header` (verify check `header`) regenerates `crates/aldis-ffi/src/capacities.rs` from `crates/aldis-ffi/capacities.toml` and fails when it differs; the header publishes `ALDIS_SPP_TRACKER_MAX_APIDS`, and `aldis_limits` reports it (`limits.rs` test)
