# aldis-aos

Frame codec for the CCSDS 732.0-B-5 AOS Space Data Link Protocol: AOS Transfer Frames, the Frame Header Error Control, the M_PDU, and the B_PDU.

**Status:** in progress in Milestone 2 ([roadmap](../../docs/08-roadmap.md) §1, [M2 plan](../../docs/milestones/m2-frame-codecs.md) WP-4). The Frame Header Error Control (`fhec`: `encode`, `write`, `decode`; AOS-013, AOS-014) is implemented; the other modules are declared but not yet implemented.

## Verification

- Unit tests in `src/fhec.rs` check the decoder against the independent table model `aldis_testkit::oracle::fhec` on all 65 536 syndromes and correct every pattern of one or two symbol errors.
- Vectors in [`test-vectors/aos/`](../../test-vectors/aos/), with the hand derivation in [docs/verification/derivations/aos-fhec.md](../../docs/verification/derivations/aos-fhec.md); the fuzz target `aos_fhec_model`; the exerciser on three bare-metal targets; 100 % coverage (VER-024); the [GEN-022/023 inspection](../../docs/verification/inspections/gen-022-023-aldis-aos.md).

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2. It depends only on `aldis-core`.

## Specification

- [docs/standards/aos-frame.md](../../docs/standards/aos-frame.md): requirement area AOS.
- [docs/standards/common-link-features.md](../../docs/standards/common-link-features.md): the FECF, OID, counters, and field placement, implemented in `aldis-core`.
- [docs/09-open-issues.md](../../docs/09-open-issues.md): OI-04 (the M_PDU of AOS Issue 4 and earlier, to be verified).
- [docs/README.md](../../docs/README.md): the index of the specification.
