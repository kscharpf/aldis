### Fixed
- `aldis-spp`: a time secondary header cut off inside its time code now reports a `LengthMismatch`
  `expected` length that includes the Ancillary Data Field (a lower bound inside a P-field
  extension). SPP-008.
