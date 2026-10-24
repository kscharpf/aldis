# Record: CABI-030

| | |
|---|---|
| Requirements | CABI-030 |
| Method | A |
| Check | Verify check `header`: `capacities.rs` regenerated |

`cargo xtask header` (verify check `header`) regenerates `crates/aldis-ffi/src/capacities.rs` from `crates/aldis-ffi/capacities.toml` and fails when it differs; the header publishes `ALDIS_SPP_TRACKER_MAX_APIDS` and, since M2 WP-5 module 9, `ALDIS_USLP_MAX_VCS` and `ALDIS_USLP_MAX_MAPS` (`[uslp]`, the channel table of the `aldis_uslp_*` calls), and `aldis_limits` reports them (`limits.rs` test, the C smoke test)
