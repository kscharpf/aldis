### Changed
- `aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp`: the two-phase frame writers are a finishing typestate (RS-023, API-074). `begin` returns the writer `Unprotected`; `protect(hook, ctx)` (SDLS) or `unsecured()` (no SDLS) gives it `Protected`; `finalize()` computes the FHEC (AOS) and the FECF last and gives it `Finalized`, the only state with `octets`, `into_octets`, and `len`. `finish` is removed. For TC and USLP, `protect` and `unsecured` take the User Data or TFDZ length that `finish` took. The AOS writer no longer computes the FHEC in `begin`.

### Added
- `aldis-core`: module `finishing` with the sealed `FrameState` and its states `Unprotected`, `Protected`, and `Finalized`, and `require_unsecured` (RS-023).
