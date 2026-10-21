### Added
- `aldis-ffi`: the C ABI of the M1 codecs (M2 plan WP-0c, D4; CABI-001…004, CABI-010…012,
  CABI-020…022, CABI-033): `aldis_spp_*`, `aldis_time_*`, and `aldis_epp_*` decoders, encoders,
  two-phase writers as two stateless calls (`_writer_begin`, `_writer_finish`), idle builders,
  `peek_length`, conversions, and configuration checks. Every pointer and length is checked;
  views are POD structs with offset/length pairs; configuration structs start with
  `struct_size`; `aldis_error_detail_t` carries the error kind, the field code, and the detail.
  The `ALDIS_FIELD_*` constants are generated from the field catalogue by `cargo xtask fields`.
- The generated header `include/aldis/aldis.h`, made by the pinned `cbindgen` 0.29.4 (MPL-2.0)
  with `cargo xtask header --write`; `cargo xtask header` and the new verify check `header`
  fail on a stale header, and compile and run a C program with the static and shared libraries
  as C99 and C++11 with the pinned GCC and Clang (CABI-001…003). The verification image
  installs `cbindgen`.
- `capacities.toml` `[spp] tracker_max_apids`, published as `ALDIS_SPP_TRACKER_MAX_APIDS` and
  reported by `aldis_limits` (CABI-030).
- An `aldis-ffi` integration test runs every live case of `test-vectors/{time,spp,epp}/`
  through the C ABI with the testkit runner.
