# aldis-tm

Frame codec for the CCSDS 132.0-B-3 TM Space Data Link Protocol: TM Transfer Frames.

**Status:** skeleton. The modules are declared but not implemented; they are implemented in Milestone 2 ([roadmap](../../docs/08-roadmap.md) §1).

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2. It depends only on `aldis-core`.

## Specification

- [docs/standards/tm-frame.md](../../docs/standards/tm-frame.md): requirement area TM.
- [docs/standards/common-link-features.md](../../docs/standards/common-link-features.md): the FECF, OID, counters, and field placement, implemented in `aldis-core`.
- [docs/README.md](../../docs/README.md): the index of the specification.
