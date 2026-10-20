# aldis-cop1

The CCSDS 232.1-B-2 Communications Operation Procedure-1 as sans-I/O engines: FOP-1 at the sending end and FARM-1 at the receiving end, one instance of each per Virtual Channel.

**Status:** skeleton. The modules are declared but not implemented; they are implemented in Milestone 3 ([roadmap](../../docs/08-roadmap.md) §1).

## Where it sits

Layer 2 (engines) of the [architecture](../../docs/01-architecture.md) §2. It depends on `aldis-core` and `aldis-tc`, and on `aldis-uslp` with the feature `uslp`. As a library, the application calls the engines directly; `aldis-framework` uses the same engines.

## Specification

- [docs/standards/cop1.md](../../docs/standards/cop1.md): requirement area COP.
- [docs/04-engines.md](../../docs/04-engines.md): the sans-I/O contract every engine follows.
- [docs/README.md](../../docs/README.md): the index of the specification.
