### Added
- `aldis-core`: the module `ocf`, the Operational Control Field of TM, AOS, and USLP frames
  (CMN-010, CMN-011; M2 plan WP-1). `Ocf::decode` classifies four octets by bits 0–1 into
  `Ocf::Clcw(Type1Report)`, `Ocf::ProjectSpecific(ProjectSpecificReport)`, or
  `Ocf::SdlsFsr(FsrReport)` (132.0 §4.1.5.4–5, 732.0 §4.1.5.4–5, 732.1 §4.1.5.2.1–2); all four
  values of the two bits are assigned, so it cannot fail. Each report type checks its flag bits in
  `new` and `TryFrom<[u8; 4]>` and keeps the octets opaque; `Ocf::to_octets` gives the octets
  to write. `ocf::OCF_LEN` is `layout::OCF_LEN`.
- Field catalogue: codes 65 `ocf_type_flag` and 66 `ocf_report_use`, named by the
  `InvalidField` errors of the report constructors (`docs/03-api-model.md` §9.1, API-082).
- Test vectors: `test-vectors/core/ocf.json`, 20 hand-derived cases of classification and of the
  report constructors, with field codes; run by `aldis-core`'s vector suite and by the
  exerciser on the three bare-metal targets.
- Fuzz target `ocf_classify`, in the nightly matrix (VER-022).
