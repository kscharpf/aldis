### Added
- [docs/verification/reviews/m2-vectors.md](docs/verification/reviews/m2-vectors.md): the second review of
  the hand-derived TM, TC, AOS, and USLP vectors (M2 plan WP-6), blind, with the maintainer's decisions
  of 2026-10-25, the resolution of each finding, and the list of follow-up cases.
- TC-009: the TC writer builds a Type-C frame from a Control Command only and a Type-D frame from a
  Frame Data Unit only (232.0-B-4 §4.1.2.3.2.2, Table 4-1); the three vector cases that pin it cite it.
- OI-46: the USLP VCF Count Length is read per QoS (Table 5-3), resolved.

### Changed
- `aldis-tm`, `aldis-ffi`: `TmFrameWriter::begin_oid` and `aldis_tm_oid_writer_begin` (both generator
  forms) reject a VC that uses SDLS with `InvalidField` naming `vc_id`, after the `sync_flag` check and
  writing nothing (TM-010). No signature changes.
- `aldis-uslp`: `VcTable`'s `VcLookup::vc` answers VCID 63 of a known Spacecraft ID on a fixed-length
  Physical Channel without an entry, as a fixed-length VC with the defaults of `VcParams::new`; an entry
  still overrides them, and VCID 63 stays invalid on a variable-length channel (USLP-009,
  `docs/standards/uslp-frame.md` §2.1). No signature changes.
- API-031 lists two exceptions (a CLCW with COP in Effect other than `01`; OID data, which a stateless
  decoder cannot check); API-071 limits the Security Trailer to 65 535 octets; the OID sources of
  CMN-020…022 include the AOS and USLP Blue Books.
- Test vectors (unreleased): descriptions, requirement citations, and provenance notes of the `tm/`,
  `tc/`, `aos/`, `uslp/`, and `core/ocf.json` files, regenerated from their models. `tm-oid-002` and
  `uslp-oid-019` are superseded by `tm-oid-005` and `uslp-oid-042`. The follow-up cases of the review (TM 8, TC 29, AOS 1, USLP 31) were added by a blind second model writer, and the models' `--write` generated them.

### Fixed
- `docs/standards/uslp-frame.md` §2.1 cited 732.1-B-3 Table 5-2 note 2 for the opposite of what it
  says; API-032 named the TM Packet Order Flag with Sync Flag = 1 where Sync Flag = 0 is the anomaly.
