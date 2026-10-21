# Record: TIME-004

| | |
|---|---|
| Requirements | TIME-004 |
| Method | I |
| Check | Review of each PR that touches `crates/aldis-time` |

`aldis-time` has no dependency that reads a clock or time zone: it depends only on `aldis-core`, is `no_std`, and no function takes or returns host time. Reviewed in each PR that touches `crates/aldis-time`
