### Fixed
- `cargo xtask sana check-live` and `sana import` follow the pager of a SANA registry page (25 rows a page; the `per_page` query is ignored), sort the rows by value, and drop duplicates. The nightly job `sana-registries` failed on the 32 records of `uslp_protocol_id` (#155). Fetching fails only without data; the comparison with the snapshot is the check.
