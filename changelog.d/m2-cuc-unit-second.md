### Changed
- TIME-011 and the docs of `CucValue::to_seconds_nanos`, `Cuc::from_seconds_nanos`,
  `aldis_time_cuc_to_ns`, and `aldis_time_cuc_from_ns` state that the conversions take the basic
  time unit to be the second (301.0 §1.2, §3.2.1). For a Level 2 code they give the time in seconds
  only if the agency's unit is the second (#82). Behaviour is unchanged.
