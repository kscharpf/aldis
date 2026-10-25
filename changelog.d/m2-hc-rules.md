### Changed
- **Breaking:** `aldis-tm`: `TmPhysicalChannelConfig::new` rejects a frame length above 4 095
  octets (`MAX_FRAME_LEN`) with `InvalidConfig` (`frame_len`, `expected` 4095), since above it the
  CRC-16 misses some double-bit errors (OI-51, VER-064, `docs/standards/tm-frame.md` TM-001). The
  C ABI follows: `aldis_tm_config_check` and every call with a physical channel.
- **Breaking:** `aldis-uslp`: a VC with a truncated frame length carries only truncated frames
  (OI-48; 732.1 Annex D1.2.1, `docs/standards/uslp-frame.md` USLP-002). `encode`,
  `UslpFrameWriter::begin` and `resume`, and the C calls `aldis_uslp_encode`,
  `aldis_uslp_writer_begin`, and `aldis_uslp_writer_finish` reject a non-truncated frame on it
  with `InvalidField` (`end_of_frame_primary_header_flag`); `UslpFrame::decode`, `validate_frame`
  (new reason 18, `Reason::NonTruncatedFrameNotAllowed`, `ALDIS_USLP_REASON_NON_TRUNCATED_FRAME_NOT_ALLOWED`),
  and `UslpFrame::peek_length` reject one with the same error, in both modes. `Delimited` has a
  new variant `NonTruncatedNotAllowed` (C: `ALDIS_USLP_DELIMITED_NON_TRUNCATED_NOT_ALLOWED`, 5),
  which discards the rest of the data unit. A bit error in the End of Frame Primary Header Flag
  can no longer make a frame pass as a truncated frame without an FECF (`hz:HZ-USLP-02`).
- **Breaking:** the C view of a decoded truncated USLP frame (`aldis_uslp_frame_t`) reports `kind`
  as `ALDIS_USLP_KIND_EXPEDITED`, the processing of Annex D1.3.1 note 3 (OI-52), where it was 0
  (sequence-controlled).
- `aldis-ffi`: the `panic-handler` feature executes a trapping instruction before it spins:
  `udf #0` on Arm, `ebreak` on RISC-V, `brk #0` on AArch64, `ud2` on x86 (OI-49;
  `docs/profiles/c-abi.md` §1). A target without stable inline assembly (SPARC V8) still spins.
