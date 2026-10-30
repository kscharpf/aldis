# aldis-spp

Codec for the CCSDS 133.0-B-2 Space Packet Protocol: decoding, encoding, idle packets, sequence counting, the Octet String service, and segmentation of user data.

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2. It depends on `aldis-core`, and on `aldis-time` only with the feature `time`. That optional dependency is the one Layer 1 → Layer 1 dependency GEN-002 allows. The codec functions are stateless; the only state is in the sequence counter, tracker, segmenter, and reassembler values the caller owns.

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
