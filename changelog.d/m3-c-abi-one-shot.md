### Added
- `aldis-tm`, `aldis-aos`: the one-shot Only Idle Data encoders `encode_oid` and `encode_oid_galois` (TM-012, AOS-021).
- `aldis-ffi`: `aldis_tm_encode_oid` and `aldis_aos_encode_oid`, the one-shot Only Idle Data frame encoders, which take an `aldis_oid_generator_t` as `aldis_uslp_encode_oid` does (TM-012, AOS-021).

### Changed
- `aldis-ffi`: **Breaking:** the ABI version is 0.2.0, since the two-phase writers are gone.
- `docs/`: API-023 describes the Rust typestate writers only; the C ABI keeps no state between calls (OI-57). OI-50 is superseded and the stateless-finish rows of `docs/profiles/c-abi.md` are withdrawn.

### Removed
- `aldis-ffi`: **Breaking:** the two-phase writer functions `aldis_{tm,aos,tc,uslp,spp,epp}_writer_begin` and `_writer_finish`, `aldis_tm_oid_writer_begin`, `aldis_tm_oid_writer_finish`, and `aldis_aos_oid_writer_begin`, and the structs `aldis_spp_writer_layout_t` and `aldis_epp_writer_layout_t` (OI-57 convention 1). Frames and packets are built with the one-shot encoders. `aldis_tm_layout_t`, `aldis_aos_layout_t`, `aldis_tc_layout_t`, and `aldis_uslp_layout_t` stay: they are the layouts of decoded frames.
- `aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp`: **Breaking:** `TmFrameWriter::resume`, `TcFrameWriter::resume`, `AosFrameWriter::resume`, and `UslpFrameWriter::resume`, which existed only for the C stateless finish. The typestate `begin` and `finish` stay.
