# aldis-epp

Codec for the CCSDS 133.1-B-3 Encapsulation Packet Protocol: decoding and encoding Encapsulation Packets with all four header lengths, idle packets, and the managed parameters.

**Status:** implemented in Milestone 1, release `v0.2.0-m1` ([plan](../../docs/milestones/m1-packets-time.md)); C ABI functions `aldis_epp_*` implemented in Milestone 2 in `aldis-ffi` ([M2 plan](../../docs/milestones/m2-frame-codecs.md) WP-0c, release `v0.3.0-m2`).

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2. It depends only on `aldis-core`, is stateless, and does no I/O.

## What it provides

- A zero-copy decoder, `EncapsulationPacket`, that borrows the input buffer and gives the header fields and the data (133.1 §4.1). `EncapsulationPacket::peek_length` reads the total length from the header alone, for splitting a stream into packets.
- Encoders: the one-shot `encode`, the two-phase `EncapsulationWriter`, and `idle` for Encapsulation Idle Packets (133.1 §4.1, §4.2.1). `HeaderSize` chooses the header length, or lets the encoder pick the smallest that fits (133.1 §4.1.2.1.1).
- Range-checked header fields (`Epi`, `EpiExtension`, `UserDefined`, `HeaderLen`) and the owned header `EncapsulationHeader` (133.1 §4.1.2).
- The managed parameters in `EppConfig`: data unit length limits and the sets of valid EPIs and extended EPIs (133.1 §5 Table 5-1). The sending end builds a checked `DataUnit` from it, and the receiving end checks a decoded packet with `EncapsulationPacket::validate`.
- Strict and lenient decoding (`aldis_core::DecodeMode`); a lenient decode reports what it accepted as `EppAnomaly` flags.

It embeds no SANA registry. Which Encapsulation Protocol IDs are valid is a managed parameter the caller supplies in `EppConfig` (EPP-010; M1 decision D4, OI-06), so no EPI value assigned by SANA is written into the code.

The module table in [`src/lib.rs`](src/lib.rs) maps every module to its 133.1 clauses and requirement IDs.

## Features

None by default (GEN-005). `alloc` and `std` are strictly additive: no change in behaviour or wire output (GEN-006).

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

## Guarantees

- `no_std`, no heap allocation, and no `unsafe` (GEN-010, GEN-070).
- No panic on any input (GEN-020). Checked by the bare-metal panic-freedom link check in [`exerciser/`](../../exerciser/) (VER-010) and by the fuzz targets `epp_*` in [`fuzz/`](../../fuzz/fuzz_targets/) (VER-022).
- No clock, I/O, or global state (GEN-031, GEN-032).

The evidence for the release is the exit table of the [M1 plan](../../docs/milestones/m1-packets-time.md) §4: coverage, fuzzing, mutation testing, and vectors on the host, on s390x, and under QEMU. No independent implementation has been run against the EPP vectors yet ([cross-checks.md](../../docs/verification/cross-checks.md) §4.5, OI-32); they are hand-derived, and their second review is [m1-vectors.md](../../docs/verification/reviews/m1-vectors.md). The bounded-loop analysis record is [GEN-022](../../docs/verification/records/GEN-022.md). The shared conformance vectors are in [`test-vectors/epp/`](../../test-vectors/epp/).

## Specification

- [docs/standards/encapsulation-packet.md](../../docs/standards/encapsulation-packet.md): requirement area EPP, with the 133.1 clause for each requirement.
- [docs/03-api-model.md](../../docs/03-api-model.md): the codec contract (views, writers, decode modes, errors) shared by every codec.
- [docs/09-open-issues.md](../../docs/09-open-issues.md): OI-06 (SANA registries, open); OI-17 (resolved) records the EPP readings taken in M1; OI-32 (independent implementations not yet run).
- [docs/README.md](../../docs/README.md): the index of the specification.
