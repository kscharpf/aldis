### Fixed
- `cargo xtask sana check-live` and `sana import` follow the pager of a SANA registry page (25 rows a page; the `per_page` query is ignored) and fail unless the rows of all pages match the record count the pages state. The nightly job `sana-registries` failed on the 32 records of `uslp_protocol_id` (#155).
