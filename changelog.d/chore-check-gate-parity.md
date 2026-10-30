### Changed
- `cargo xtask check` runs the formatting of `exerciser/`, `fuzz/`, and `crosscheck/` when they change, and `cargo doc` with `RUSTDOCFLAGS=-D warnings`, as the merge gate does, so a change that passes `check` no longer fails the gate on either. Tests keep the format workspaces, the doc command, and its flags equal to the gate's (VER-004, VER-001).
- `cargo xtask consistency` checks that every `aldis_` and `ALDIS_` name in `docs/profiles/c-abi.md` and `crates/aldis-ffi/README.md` is declared by `include/aldis/aldis.h`, or is listed with its reason in `NOT_IN_HEADER` (CABI-002). The `c-smoke` step compiles the usage example of the ffi README against the header (CABI-003).

### Fixed
- `docs/profiles/c-abi.md`: two names of the USLP non-truncated discard that the header no longer declares (`ALDIS_USLP_REASON_NON_TRUNCATED_FRAME_NOT_ALLOWED`, `ALDIS_USLP_DELIMITED_NON_TRUNCATED_NOT_ALLOWED`) are now the discard reason `ALDIS_USLP_DISCARD_NON_TRUNCATED_FRAME_NOT_ALLOWED`, and the naming example `aldis_spp_header_t` is `aldis_spp_packet_t`.
