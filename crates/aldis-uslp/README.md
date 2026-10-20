# aldis-uslp

Frame codec for the CCSDS 732.1-B-3 Unified Space Data Link Protocol: USLP Transfer Frames, including truncated frames, and the Transfer Frame Data Field header.

**Status:** skeleton. The modules are declared but not implemented; they are implemented in Milestone 2 ([roadmap](../../docs/08-roadmap.md) §1).

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2. It depends only on `aldis-core`.

## Specification

- [docs/standards/uslp-frame.md](../../docs/standards/uslp-frame.md): requirement area USLP.
- [docs/standards/common-link-features.md](../../docs/standards/common-link-features.md): the FECF, OID, counters, and field placement, implemented in `aldis-core`.
- [docs/09-open-issues.md](../../docs/09-open-issues.md): OI-05 (the TFDF header length, to be verified); OI-06 (SANA registries, needed for the UPID values).
- [docs/README.md](../../docs/README.md): the index of the specification.
