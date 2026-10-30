# aldis

The facade crate: it re-exports the Aldis crates as modules, each behind a Cargo feature, so that an application depends on one crate and enables only the standards it uses.

## Where it sits

Outside the layers of the [architecture](../../docs/01-architecture.md) §2: it depends on every library crate, each one optional, and contains no code of its own. No feature is enabled by default, and with none it builds for a `no_std` target (GEN-005). A mission that enables only `spp` compiles only `aldis-core` and `aldis-spp`, and has only those to qualify (GEN-002, GEN-003).

## Example

With `aldis = { …, features = ["core", "spp"] }` in `Cargo.toml`, decode a Space Packet through the facade:

```rust
use aldis::core::DecodeMode;
use aldis::spp::SpacePacket;

fn main() -> Result<(), aldis::core::Error> {
    // Vector spp-dec-001 of test-vectors/spp/decode_valid.json: a telemetry packet,
    // APID 0x123, unsegmented, sequence count 5, three octets of data.
    let bytes = [0x01, 0x23, 0xC0, 0x05, 0x00, 0x02, 0x01, 0x02, 0x03];
    let packet = SpacePacket::decode(&bytes, DecodeMode::Strict)?;
    assert_eq!(packet.apid().get(), 0x123);
    assert_eq!(packet.sequence_count().get(), 5);
    assert_eq!(packet.data_field(), [0x01, 0x02, 0x03]);
    Ok(())
}
```
