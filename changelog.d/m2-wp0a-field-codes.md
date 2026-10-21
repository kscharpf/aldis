### Added
- Field catalogue: every field identifier that `aldis-core`, `aldis-time`, `aldis-spp`, and
  `aldis-epp` name has a stable numeric code, listed in `docs/03-api-model.md` §9.1 (64 codes;
  API-082, OI-27, M2 plan D2).
- `aldis-core`: the module `field`, generated from the catalogue, with one `FieldId` constant per
  code and `field::ALL`. `aldis-time`, `aldis-spp`, and `aldis-epp` re-export the constants they
  use as `aldis_time::field`, `aldis_spp::field`, and `aldis_epp::field`.
- `aldis-core`: `FieldId::code()`, the field code that the C ABI will carry (CABI-021).
- `cargo xtask fields [--write]`: checks the catalogue and that `aldis_core::field` and
  `test-vectors/core/fields.json` match it, or regenerates both; part of `cargo xtask ci` and of
  the `spec` check of `cargo xtask verify`.
- `cargo xtask trace --scope m2`, without `--strict`, in `cargo xtask ci`, the `spec` check, and
  the nightly `ci.yml` (M2 plan D1).
- Test vectors: the error expectation of a codec case may give `field`, the code of the field the
  error names; the testkit runner and the exerciser compare it when present (TV-012, M2 plan D3).
  `test-vectors/core/fields.json` (schema `test-vectors/schema/fields.schema.json`) publishes the
  catalogue for the C++ and binding runners; `test-vectors/spp/field_codes.json` adds four cases
  with field codes.

### Changed
- **Breaking:** `aldis-core`: `FieldId` holds a code and a display name and is built only by the
  constants of `aldis_core::field`; `FieldId::new(name)` is removed. Equality and hashing use the
  code alone (API-082). `Error::out_of_range` takes a `FieldId` instead of a name.
- **Breaking:** errors that named `version` now name `packet_version` (`aldis-spp`, `aldis-epp`),
  the CDS day segment is `cds_day` instead of `day` (`aldis-time`), and the frame layout's data
  field is `frame_data_field` instead of `data` (`aldis-core`). Only the display text changes;
  compare fields by their constants.
- **Breaking:** `aldis-testkit`: `Outcome::Err` is a struct variant with `kind` and `field`.
