# aldis-aos

Frame codec for the CCSDS 732.0-B-5 AOS Space Data Link Protocol: AOS Transfer Frames, the Frame Header Error Control, the M_PDU, and the B_PDU.

**Status:** skeleton. The modules are declared but not implemented; they are implemented in Milestone 2 ([roadmap](../../docs/08-roadmap.md) §1).

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2. It depends only on `aldis-core`.

## Specification

- [docs/standards/aos-frame.md](../../docs/standards/aos-frame.md): requirement area AOS.
- [docs/standards/common-link-features.md](../../docs/standards/common-link-features.md): the FECF, OID, counters, and field placement, implemented in `aldis-core`.
- [docs/09-open-issues.md](../../docs/09-open-issues.md): OI-04 (the M_PDU of AOS Issue 4 and earlier, to be verified).
- [docs/README.md](../../docs/README.md): the index of the specification.
