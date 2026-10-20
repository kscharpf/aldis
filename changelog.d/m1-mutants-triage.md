### Changed
- `aldis-core` tests: five mutants that survived the M1 pre-release mutation run are now caught
  (`FixedVec::eq`, `SeqCount::get`, both `FrameSeqNum` conversions, `PacketVersion::get`), and
  `docs/verification/mutants-triage.md` records the full run over all four M1 crates (VER-025).
