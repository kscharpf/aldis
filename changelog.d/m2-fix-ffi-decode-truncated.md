### Fixed
- fuzz: update the USLP `ffi_decode` harness to assert `ALDIS_USLP_KIND_EXPEDITED` for truncated
  frames (OI-52), gate `truncated_frame_len` on variable-length channels to restore coverage of full
  frames (OI-48), and add regression vector `uslp-trunc-056` for issue #124 (VER-022, TV-020).
