# aldis-tm

Frame codec for the CCSDS 132.0-B-3 TM Space Data Link Protocol: TM Transfer Frames.

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2. It depends only on `aldis-core`, is stateless, and does no I/O. Frame counts and the OID generator's state belong to the caller, the engines of Milestone 4.

## Features

None by default (GEN-005). `alloc` and `std` are strictly additive: no change in behaviour or wire output (GEN-006).

## Example

Encode a frame on a virtual channel with an OCF and decode it again:

```rust
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::ocf::Ocf;
use aldis_tm::{
    DataFieldContent, DataFieldStatus, PacketOffset, PacketPointer, TmFrame, TmFrameContent,
    TmFrameFields, TmMasterChannelConfig, TmPhysicalChannelConfig, TmVirtualChannelConfig, encode,
};

fn main() -> Result<(), aldis_core::Error> {
    // 32-octet frames with an FECF; spacecraft 42; VC 1 carries packets and an OCF.
    let pc = TmPhysicalChannelConfig::new(32, true)?;
    let vc1 = VcId::try_from(1_u8)?;
    let mc = TmMasterChannelConfig::new(pc, SpacecraftId::try_from(42_u16)?, None, false)
        .with_vc(TmVirtualChannelConfig::new(vc1, DataFieldContent::Packets).with_ocf())?;

    // A packet starts at the first data octet; the data field is 32 - 6 - 4 - 2 = 20 octets.
    let start = PacketPointer::StartsAt(PacketOffset::try_from(0)?);
    let fields = TmFrameFields::new(vc1, 7, 3, DataFieldStatus::Packets(start));
    let data = [0xAB_u8; 20];
    let content = TmFrameContent {
        ocf: Some(Ocf::decode([0x01, 0x14, 0x0C, 0x2A])),
        ..TmFrameContent::new(&data)
    };
    let mut buf = [0_u8; 32];
    let len = encode(&mc, &fields, &content, &mut buf)?;

    // The view borrows `buf`; `validate` checks it against the master channel and returns it as a `ValidatedTmFrame`.
    let frame = TmFrame::decode_with(&buf[..len], mc.pc(), &mc)?.validate(&mc)?;
    assert_eq!(frame.vcid(), vc1);
    assert_eq!(frame.data_field(), &data);
    assert_eq!(frame.ocf(), content.ocf);
    Ok(())
}
```

## Guarantees

- `no_std`, no heap allocation, and no `unsafe` (GEN-010, GEN-070).
- No panic on any input (GEN-020). Checked by the bare-metal panic-freedom link check in [`exerciser/`](../../exerciser/) (VER-010) and by the fuzz targets `tm_decode` and `tm_roundtrip` in [`fuzz/`](../../fuzz/fuzz_targets/) (VER-022).
- No clock, I/O, or global state (GEN-031, GEN-032).

The shared conformance vectors are in [`test-vectors/tm/`](../../test-vectors/tm/), computed by the independent model [`test-vectors/models/tm_frame.py`](../../test-vectors/models/tm_frame.py), with one decode and one encode worked by hand in [docs/verification/derivations/tm-frame.md](../../docs/verification/derivations/tm-frame.md). 100 % coverage (VER-024); the bounded-loop analysis record is [GEN-022](../../docs/verification/records/GEN-022.md). No independent implementation has been run against the TM vectors yet ([cross-checks.md](../../docs/verification/cross-checks.md), M2 plan D15).

## Specification

- [docs/standards/tm-frame.md](../../docs/standards/tm-frame.md): requirement area TM, with the 132.0 clause for each requirement, and the anomaly flags.
- [docs/standards/common-link-features.md](../../docs/standards/common-link-features.md): the FECF, the OCF, OID, counters, and field placement, implemented in `aldis-core`.
- [docs/03-api-model.md](../../docs/03-api-model.md): the codec contract (views, writers, decode modes, errors, field codes) shared by every codec.
- [docs/README.md](../../docs/README.md): the index of the specification.
