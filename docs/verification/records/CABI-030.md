# Record: CABI-030

| | |
|---|---|
| Requirements | CABI-030 |
| Method | A |
| Check | Verify check `header`: `capacities.rs` regenerated |

`cargo xtask header` (verify check `header`) regenerates `crates/aldis-ffi/src/capacities.rs` from `crates/aldis-ffi/capacities.toml` and fails when it differs; the header publishes `ALDIS_SPP_TRACKER_MAX_APIDS`, and `aldis_limits` reports it (`limits.rs` test)

Since M2 WP-3 module 8 (branch `m2/tc-c-abi`) the header also publishes `ALDIS_TC_MAX_MAPS` (`[tc] max_maps`, the MAPs one TC call takes with a master channel), which `aldis_limits` reports as `tc_max_maps` (`limits.rs` test, C smoke test).

Since M2 WP-5 module 9 (branch `m2/uslp-c-abi`) the header also publishes `ALDIS_USLP_MAX_VCS` and `ALDIS_USLP_MAX_MAPS` (`[uslp] max_vcs` and `max_maps`, the channel table of the `aldis_uslp_*` calls), which `aldis_limits` reports (`limits.rs` test, the C smoke test).
