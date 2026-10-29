# aldis-link

Sans-I/O sending and receiving engines for CCSDS packets and frames: packet-to-frame generation, Master and Virtual Channel multiplexing, frame demultiplexing, packet extraction, and segmentation and reassembly.

## Where it sits

Layer 2 (engines) of the [architecture](../../docs/01-architecture.md) §2. It depends on `aldis-core`, and on each of `aldis-spp`, `aldis-epp`, `aldis-tm`, `aldis-tc`, `aldis-aos`, and `aldis-uslp` only with the feature of the same name.

## Specification

- [docs/04-engines.md](../../docs/04-engines.md): requirement area LNK.
- [docs/09-open-issues.md](../../docs/09-open-issues.md): OI-06 (SANA registries); OI-11 (TM idle strategy).
- [docs/README.md](../../docs/README.md): the index of the specification.
