# aldis-link

Sans-I/O sending and receiving engines for CCSDS packets and frames: packet-to-frame generation, Master and Virtual Channel multiplexing, frame demultiplexing, packet extraction, and segmentation and reassembly.

## Where it sits

Layer 2 (engines) of the [architecture](../../docs/01-architecture.md) §2. It depends on `aldis-core`, and on each of `aldis-spp`, `aldis-epp`, `aldis-tm`, `aldis-tc`, `aldis-aos`, and `aldis-uslp` only with the feature of the same name. The `PacketGenerator` and the `PacketExtractor` (M4 WP-3) need `spp` and `epp`, and a protocol of `tm`, `aos`, or `uslp`; the shared types and the protocol trait need none.
