# aldis

The facade crate: it re-exports the Aldis crates as modules, each behind a Cargo feature, so that an application depends on one crate and enables only the standards it uses.

**Status:** in place since Milestone 0. The re-exported crates for time codes, Space Packets, Encapsulation Packets, and the four frame codecs (TM, TC, AOS, USLP) are implemented (release `v0.3.0-m2`); the link, COP-1, and framework crates are skeletons until their milestones ([roadmap](../../docs/08-roadmap.md) §1).

## Where it sits

Outside the layers of the [architecture](../../docs/01-architecture.md) §2: it depends on every library crate, each one optional, and contains no code of its own. No feature is enabled by default, and with none it builds for a `no_std` target (GEN-005). A mission that enables only `spp` compiles only `aldis-core` and `aldis-spp`, and has only those to qualify (GEN-002, GEN-003).

## Features

| Feature | Module | Crate | Status |
|---|---|---|---|
| `core` | `aldis::core` | [`aldis-core`](../aldis-core/README.md) | Implemented (M0) |
| `time` | `aldis::time` | [`aldis-time`](../aldis-time/README.md) | Implemented (M1) |
| `spp` | `aldis::spp` | [`aldis-spp`](../aldis-spp/README.md), without its feature `time` | Implemented (M1) |
| `spp-time` | `aldis::spp` and `aldis::time` | `aldis-spp` with its feature `time`: the typed time secondary header (SPP-008) | Implemented (M1) |
| `epp` | `aldis::epp` | [`aldis-epp`](../aldis-epp/README.md) | Implemented (M1) |
| `tm` | `aldis::tm` | [`aldis-tm`](../aldis-tm/README.md) | Implemented (M2) |
| `tc` | `aldis::tc` | [`aldis-tc`](../aldis-tc/README.md) | Implemented (M2) |
| `aos` | `aldis::aos` | [`aldis-aos`](../aldis-aos/README.md) | Implemented (M2) |
| `uslp` | `aldis::uslp` | [`aldis-uslp`](../aldis-uslp/README.md) | Implemented (M2) |
| `cop1` | `aldis::cop1` | [`aldis-cop1`](../aldis-cop1/README.md) | Skeleton (M3) |
| `link` | `aldis::link` | [`aldis-link`](../aldis-link/README.md) | Skeleton (M4) |
| `framework` | `aldis::framework` | [`aldis-framework`](../aldis-framework/README.md) | Skeleton (M6) |
| `alloc`, `std` | — | The same feature of every enabled crate; strictly additive (GEN-006) | — |
| `full` | — | Every crate above, `spp-time`, and the cross-crate features of `aldis-link` and `aldis-cop1` | — |

The crate documentation in [`src/lib.rs`](src/lib.rs) gives the standard and the requirement IDs behind each feature.

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

## Guarantees

The facade adds no code: it has the guarantees of the crates it re-exports (`no_std`, no heap, no panics, and no `unsafe`; GEN-010, GEN-020, GEN-070). The bare-metal check builds it with the feature `full` (`cargo clippy -p aldis --no-default-features --features full --target thumbv7em-none-eabihf`, see the root [README](../../README.md#building)). The tests in [`tests/`](tests/) check the `spp-time` feature combination.

## Specification

- [docs/01-architecture.md](../../docs/01-architecture.md) §2: the crate map and the dependency rules GEN-001…GEN-004.
- [docs/profiles/rust.md](../../docs/profiles/rust.md) §2: crates and features.
- [docs/README.md](../../docs/README.md): the index of the specification.
