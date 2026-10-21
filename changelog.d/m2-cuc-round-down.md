### Changed
- **Breaking:** `aldis-time`: `Cuc::from_seconds_nanos`, and the C ABI function
  `aldis_time_cuc_from_ns`, round the fraction down instead of up (TIME-011, OI-40), as
  `CucValue::to_seconds_nanos` already did. The coarse time is now always the given seconds:
  999 999 999 ns no longer carries into the next second, and a full coarse time no longer fails with
  `OutOfRange` (`coarse`) on a carry. The round trip `CucValue → ns → CucValue` is exact if and only
  if the value is a multiple of 2⁻⁹ s, so always for 0 and 1 fine octets but no longer for every
  value of 2 or 3. The `cuc_from_ns` vectors `time-cuc-020`, `-022`, `-023`, `-025`, and `-026` are
  superseded by `time-cuc-029`…`-033`, and `time-cuc-034` is added. The cross-check discrepancy D4
  with `spacepackets` is closed (docs/verification/cross-checks.md).
