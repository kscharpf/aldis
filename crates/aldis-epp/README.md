# aldis-epp

Codec for the CCSDS 133.1-B-3 Encapsulation Packet Protocol: decoding and encoding Encapsulation Packets with all four header lengths, idle packets, and the managed parameters.

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2. It depends only on `aldis-core`, is stateless, and does no I/O.

## Example

Encode a packet of mission-specific data and decode it again:

```rust
use aldis_core::DecodeMode;
use aldis_epp::{
    EncapsulationFields, EncapsulationPacket, Epi, EpiSet, EppConfig, ExtendedEpiSet,
    HeaderSize, encode,
};

fn main() -> Result<(), aldis_core::Error> {
    // Managed parameters: data units of 1 to 1024 octets, and only the mission-specific EPI.
    let config = EppConfig::new(
        1,
        1024,
        EpiSet::EMPTY.with(Epi::MISSION_SPECIFIC),
        ExtendedEpiSet::EMPTY,
    )?;
    let unit = config.data_unit(EncapsulationFields::new(Epi::MISSION_SPECIFIC), b"payload")?;

    let mut buf = [0_u8; 64];
    let len = encode(&unit, HeaderSize::Auto, &mut buf)?;

    // The decoded view borrows `buf`; `validate` applies the managed parameters.
    let packet = EncapsulationPacket::decode(&buf[..len], DecodeMode::Strict)?;
    packet.validate(&config)?;
    assert_eq!(packet.epi(), Epi::MISSION_SPECIFIC);
    assert_eq!(packet.data(), b"payload");
    assert_eq!(packet.len(), len);
    Ok(())
}
```
