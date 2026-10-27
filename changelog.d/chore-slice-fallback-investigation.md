### Added
- `aldis-core`: allocated `ErrorKind::InternalError = 18` for reporting violations of internal invariants and data consistency checks (API-001, GEN-020).
- `aldis-ffi`: exposed `ALDIS_ERR_INTERNAL_ERROR = -18` in C ABI (CABI-001).

### Changed
- `aldis-core`, `aldis-tm`, `aldis-tc`, `aldis-uslp`, `aldis-time`: eliminated silent default-fallback slice indexing on untrusted wire inputs. Replaced wire input fallbacks with explicit errors (`Incomplete`, `LengthMismatch`, `InvalidField`, `Malformed`).
