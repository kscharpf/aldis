# aldis

The facade crate: it re-exports the Aldis crates as modules, each behind a Cargo feature, so that an application depends on one crate and enables only the standards it uses.

## Where it sits

Outside the layers of the [architecture](../../docs/01-architecture.md) §2: it depends on every library crate, each one optional, and contains no code of its own. No feature is enabled by default, and with none it builds for a `no_std` target (GEN-005). A mission that enables only `spp` compiles only `aldis-core` and `aldis-spp`, and has only those to qualify (GEN-002, GEN-003).

## Example

With `aldis = { …, features = ["core", "spp"] }` in `Cargo.toml`, encode a Space Packet through the facade and decode it again. The root [README](../../README.md) shows this example, and `cargo xtask consistency` checks that the two are the same:

```rust
use aldis::core::ids::{Apid, SeqCount};
use aldis::spp::{
    PRIMARY_HEADER_LEN, PacketFields, PacketType, SequenceFlags, SpacePacket, SppConfig, encode,
};

fn main() -> Result<(), aldis::core::Error> {
    let fields = PacketFields {
        packet_type: PacketType::Telemetry,
        apid: Apid::try_from(0x123)?,
        sequence_flags: SequenceFlags::Unsegmented,
        sequence_count: SeqCount::try_from(42)?,
    };
    let packet = SppConfig::DEFAULT.packet(fields, &[], b"hello")?;

    let mut buf = [0_u8; 64];
    let len = encode(&packet, &mut buf)?;
    assert_eq!(len, PRIMARY_HEADER_LEN + 5);

    let decoded = SpacePacket::decode(&buf[..len])?;
    assert_eq!(decoded.apid().get(), 0x123);
    assert_eq!(decoded.sequence_count().get(), 42);
    assert_eq!(decoded.data_field(), b"hello");
    Ok(())
}
```
