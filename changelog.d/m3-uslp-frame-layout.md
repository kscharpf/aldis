### Added
- `aldis-uslp`: `UslpFrame::layout`, `UslpFullFrame::layout`, `UslpTruncatedFrame::layout`, and `UslpFrameWriter::layout` return the `FrameLayout` of the frame, as the TM, TC, and AOS views do (CMN-040).

### Changed
- **Breaking:** `aldis-uslp`: `UslpFrameWriter::security_header_mut`, `tfdz_mut`, and `security_trailer_mut` return `Result<&mut [u8]>`, and `InternalError` replaces an empty slice for a field outside the buffer (GEN-020).
- **Breaking:** `aldis-uslp`: `UslpFrame`, `UslpFullFrame`, and `UslpTruncatedFrame` are no longer `Copy`, because they hold their `FrameLayout`.
- `aldis-uslp`: the decoder, the writers, and the configuration check compute the frame overhead with `aldis_core::layout::FrameLayout` instead of three separate sums; the decoder and the truncated decoder no longer cut the frame with clamped slices, and report `InternalError` for a field outside the frame instead of an empty field (GEN-020). Frames, errors, and reasons are unchanged.
- `aldis-ffi`: the USLP functions take the field offsets of `aldis_uslp_layout_t` from the frame layout instead of subtracting pointer addresses (CABI-012). The C ABI is unchanged.
- `aldis-ffi`: the USLP layout conversion reports `InternalError` for a TFDF Header longer than the data field instead of a zero-length TFDZ (CABI-012).
