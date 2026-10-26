### Fixed
- `aldis-ffi`: prove non-null pointer invariants to static analysis (CodeQL alerts #1–#11, #127–#129)
  by using `NonNull` in `abi::call`, `abi::write_detail`, and `handle::Handle`, validating nullness
  before casting in `abi::config` and `abi::configs`, and reordering null checks before slot pointer
  derivation in `handle::init`. No public ABI or behavior changes.
