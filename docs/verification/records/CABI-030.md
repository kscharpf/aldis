# Record: CABI-030

| | |
|---|---|
| Requirements | CABI-030 |
| Method | A |
| Check | Verify check `header`: `file:crates/aldis-ffi/src/capacities.rs` regenerated |

`cargo xtask header` (verify check `header`) regenerates `file:crates/aldis-ffi/src/capacities.rs` from `file:crates/aldis-ffi/capacities.toml` and fails when it differs; the header publishes `ALDIS_SPP_TRACKER_MAX_APIDS`, `ALDIS_TC_MAX_MAPS` (`[tc] max_maps`, the MAPs one TC call takes with a master channel), and `ALDIS_USLP_MAX_VCS` and `ALDIS_USLP_MAX_MAPS` (`[uslp] max_vcs` and `max_maps`, the channel table of the aldis_uslp_* calls). The C smoke test (`file:crates/aldis-ffi/tests/c/smoke.c`) compiles against them.

There is no run-time report of the capacities (OI-57 convention 5): the header is the only source.
