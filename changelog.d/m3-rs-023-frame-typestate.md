### Changed
- `aldis-tm`, `aldis-tc`, `aldis-aos`, `aldis-uslp`: the two-phase frame writers are a finishing typestate (RS-023, API-074). `begin` returns the writer `Unprotected`; `protect(hook, ctx)` (SDLS) or `unsecured()` (no SDLS) gives it `Protected`; `finalize()` computes the FHEC (AOS) and the FECF last and gives it `Finalized`, the only state with `octets`, `into_octets`, and `len`. `finish` is removed. For TC and USLP, `protect` and `unsecured` take the User Data or TFDZ length that `finish` took. The AOS writer no longer computes the FHEC in `begin`.

### Added
- `aldis-core`: module `finishing` with the sealed `FrameState` and its states `Unprotected`, `Protected`, and `Finalized`, and `require_unsecured` (RS-023).
- `aldis-core`: `SecurityHook::layout` (default `None`; `NoSecurity` is `Some(NONE)`), `finishing::require_layout`, and the test-only `PreFilledSecurity`. A writer's `protect` rejects a hook whose layout differs from the channel's, so `NoSecurity` cannot be applied to a channel with SDLS fields.

### Changed
- `aldis-tc`, `aldis-uslp`: `finalize` returns a `Result` (`InternalError` instead of an ignored CRC result), and a failed `protect` restores the unfinished header.
