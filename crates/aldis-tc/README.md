# aldis-tc

Frame codec for the CCSDS 232.0-B-4 TC Space Data Link Protocol: TC Transfer Frames, the Segment Header, Control Commands, the CLCW, frame delimiting, and frame validation.

**Status:** skeleton. The modules are declared but not implemented; they are implemented in Milestone 2 ([roadmap](../../docs/08-roadmap.md) §1).

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2. It depends only on `aldis-core`. The COP-1 engines in `aldis-cop1` build on it.

## Specification

- [docs/standards/tc-frame.md](../../docs/standards/tc-frame.md): requirement area TC.
- [docs/standards/common-link-features.md](../../docs/standards/common-link-features.md): the FECF, counters, and field placement, implemented in `aldis-core`.
- [docs/README.md](../../docs/README.md): the index of the specification.
