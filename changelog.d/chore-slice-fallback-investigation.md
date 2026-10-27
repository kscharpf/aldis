### Added
- `aldis-core`: allocated `ErrorKind::InternalError = 18` for reporting violations of internal invariants and data consistency checks (API-001, GEN-020).
- `aldis-ffi`: exposed `ALDIS_ERR_INTERNAL_ERROR = -18` in C ABI (CABI-001).

### Changed
- `aldis-core`, `aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp`, `aldis-time`, `aldis-spp`, `aldis-epp`, `aldis-ffi`: eliminated silent `.unwrap_or_default()` and default-fallback slice indexing patterns. Replaced untrusted wire input fallbacks with explicit errors (`Incomplete`, `LengthMismatch`, `InvalidField`) and invariant-backed internal accessors with `ErrorKind::InternalError` or mathematically proven clamped slicing (`self.len.min(N)`).
