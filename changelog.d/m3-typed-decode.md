### Changed
- **Breaking:** the decoders of every codec crate have separate strict and lenient entry points instead of a `DecodeMode` argument (API-030): `decode` is strict and returns the view alone, and `decode_lenient` (`decode_with_lenient`, `decode_unchecked_lenient`, `decode_with_pfield_lenient`, `decode_implicit_lenient`, `validate_frame_lenient`, `LenientFrameStream`) returns the view in the new `aldis_core::Lenient`, which holds the `AnomalySet`. The views no longer have an `anomalies` accessor or field. The C ABI keeps its `mode` argument and `anomalies` fields.
- **Breaking:** `TmFrame::validate` and `EncapsulationPacket::validate` take the view by value and return `ValidatedTmFrame` and `ValidatedEncapsulationPacket` instead of `Result<()>` (API-016, RS-020). `validate_frame` of `aldis-tc` and `aldis-uslp` returns `ValidatedTcFrame` and `ValidatedUslpFrame`. The frame streams of `aldis-tc` and `aldis-uslp` yield validated frames, and `Farm1Frame::from_tc` takes a `ValidatedTcFrame` (API-016).
- **Breaking:** `TmFrame::decode_unchecked` returns an `UncheckedTmFrame`, which becomes a `TmFrame` only through `check_fecf` (API-015, TM-005). This is the receiving half of the RS-023 typestate (`Checked`, `Validated`); `Plain` and the engine inputs remain for the engine work packages.
- `aldis-spp`: a Space Packet no longer stores its decode mode; `split` is strict and `split_lenient` is lenient (SPP-008).

### Added
- `aldis_core::Lenient::try_map`, so a lenient decode followed by `check_fecf` or `validate` keeps its anomalies (API-032).
- **Breaking:** the C function `aldis_tm_validate` has a new `anomalies` out parameter before `err`, which receives the `ALDIS_TM_ANOMALY_*` flags of the frame it accepted; the ABI minor version is 7 (CABI-060).
- `aldis_core::Lenient` and `DecodeMode::dispatch`, for code that carries the mode as a value.
- Requirements API-015 and API-016; API-030 to API-032 restated for the two entry points.
