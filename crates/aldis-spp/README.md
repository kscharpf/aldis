# aldis-spp

Codec for the CCSDS 133.0-B-2 Space Packet Protocol: decoding, encoding, idle packets, sequence counting, the Octet String service, and segmentation of user data.

**Status:** implemented in Milestone 1, release `v0.2.0-m1` ([plan](../../docs/milestones/m1-packets-time.md)). The C ABI functions for this crate follow in Milestone 2, work package 0 ([roadmap](../../docs/08-roadmap.md) §1, OI-21).

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2. It depends on `aldis-core`, and on `aldis-time` only with the feature `time`. That optional dependency is the one Layer 1 → Layer 1 dependency GEN-002 allows. The codec functions are stateless; the only state is in the sequence counter, tracker, segmenter, and reassembler values the caller owns.

## What it provides

- A zero-copy decoder, `SpacePacket`, that borrows the input buffer and gives the primary header fields and the Packet Data Field (133.0 §4.1.2, §4.1.3). `SpacePacket::peek_length` reads the total length from the header alone, for splitting a stream into packets.
- Encoders: the one-shot `encode` and the two-phase `SpacePacketWriter`, which reserves the header and lets the caller fill the data field in place (133.0 §4.1). Both take parts already checked by `SppConfig`, which holds the managed parameter Maximum Packet Length (133.0 §5.2 Table 5-1). The Secondary Header Flag and the Packet Data Length are derived, never set by hand.
- Secondary headers through a caller-supplied `SecondaryHeaderLayout` per APID (133.0 §4.1.4.2). Without the feature `time` a secondary header is opaque octets; with it, `TimeHeaderLayout` decodes a time code there (133.0 §4.1.4.2.2).
- Idle Packets (`IdlePacket`, 133.0 §4.1.3.3.4.4), the Packet Sequence Count (`SequenceCounter` for one APID, `SequenceTracker` for up to a fixed number of APIDs, 133.0 §4.1.3.4.3, §4.3.2.2), the Octet String service (`OctetStringEncoder`, 133.0 §4.2.2), and segmentation and reassembly of user data with the Sequence Flags (`Segmenter`, `Reassembler`, 133.0 §4.1.3.4.2 note).
- Strict and lenient decoding (`aldis_core::DecodeMode`); a lenient decode reports what it accepted as `SppAnomaly` flags.

It keeps no table of secondary-header layouts: the caller looks the layout up for the APID and passes it in (OI-30). Packets are carried in frames by the link engines of `aldis-link`, not here.

The module table in [`src/lib.rs`](src/lib.rs) maps every module to its 133.0 clauses and requirement IDs.

## Features

None by default (GEN-005).

| Feature | Effect |
|---|---|
| `time` | The typed time secondary header, `SecondaryHeaderLayout::Time` with `TimeHeaderLayout` (SPP-008). Adds the dependency on `aldis-time` |
| `alloc`, `std` | Strictly additive: no change in behaviour or wire output (GEN-006) |

## Example

Encode a telemetry packet and decode it again:

```rust
use aldis_core::DecodeMode;
use aldis_core::ids::{Apid, SeqCount};
use aldis_spp::{
    PRIMARY_HEADER_LEN, PacketFields, PacketType, SequenceFlags, SpacePacket, SppConfig, encode,
};

fn main() -> Result<(), aldis_core::Error> {
    let fields = PacketFields {
        packet_type: PacketType::Telemetry,
        apid: Apid::try_from(0x123)?,
        sequence_flags: SequenceFlags::Unsegmented,
        sequence_count: SeqCount::try_from(42)?,
    };
    // No secondary header, five octets of user data, checked against the default
    // Maximum Packet Length.
    let packet = SppConfig::DEFAULT.packet(fields, &[], b"hello")?;

    let mut buf = [0_u8; 64];
    let len = encode(&packet, &mut buf)?;
    assert_eq!(len, PRIMARY_HEADER_LEN + 5);

    // The decoded view borrows `buf`.
    let decoded = SpacePacket::decode(&buf[..len], DecodeMode::Strict)?;
    assert_eq!(decoded.apid().get(), 0x123);
    assert_eq!(decoded.sequence_count().get(), 42);
    assert!(!decoded.has_secondary_header());
    assert_eq!(decoded.data_field(), b"hello");
    Ok(())
}
```

## Guarantees

- `no_std`, no heap allocation, and no `unsafe` (GEN-010, GEN-070).
- No panic on any input (GEN-020). Checked by the bare-metal panic-freedom link check in [`exerciser/`](../../exerciser/) (VER-010) and by the fuzz targets `spp_*` in [`fuzz/`](../../fuzz/fuzz_targets/) (VER-022).
- No clock, I/O, or global state (GEN-031, GEN-032).

The evidence for the release is the exit table of the [M1 plan](../../docs/milestones/m1-packets-time.md) §4: coverage, fuzzing, mutation testing, vectors on the host, on s390x, and under QEMU, and the cross-check against an independent implementation ([cross-checks.md](../../docs/verification/cross-checks.md)). The bounded-loop inspection is [gen-022-023-aldis-spp.md](../../docs/verification/inspections/gen-022-023-aldis-spp.md). The shared conformance vectors are in [`test-vectors/spp/`](../../test-vectors/spp/).

## Specification

- [docs/standards/space-packet.md](../../docs/standards/space-packet.md): requirement area SPP, with the 133.0 clause for each requirement.
- [docs/03-api-model.md](../../docs/03-api-model.md): the codec contract (views, writers, decode modes, errors) shared by every codec.
- [docs/09-open-issues.md](../../docs/09-open-issues.md): OI-30 (resolved) records the Space Packet readings taken in M1; OI-32 lists independent implementations not yet run, including one for reassembly.
- [docs/README.md](../../docs/README.md): the index of the specification.
