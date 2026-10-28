### Added
- `aldis-tc`: in-place `FrameStream` iterator unifying frame delimiting, fill handling, and channel validation over ingested data units with `DiscardReason` and `IntakeItem` (TC-020, TC-021).
- `aldis-uslp`: in-place `FrameStream` iterator unifying frame delimiting, fill handling, and channel validation over ingested data units with `DiscardReason` and `IntakeItem` (USLP-014, USLP-015).
- `aldis-ffi`: unified stream reception C ABI entry points `aldis_tc_stream_next` and `aldis_uslp_stream_next` with associated types and status codes (CABI-001).

### Removed
- `aldis-ffi`: eliminated legacy delimiter and validate entry points (`aldis_tc_next_frame`, `aldis_tc_validate`, `aldis_uslp_next_frame`, `aldis_uslp_validate`) and delimited types in favor of the single-pass stream intake APIs (`aldis_tc_stream_next`, `aldis_uslp_stream_next`).

