# aldis-tm

Frame codec for the CCSDS 132.0-B-3 TM Space Data Link Protocol: TM Transfer Frames.

**Status:** implemented in Milestone 2, work package 2 ([plan](../../docs/milestones/m2-frame-codecs.md)), modules 1–6: configuration, header and view, anomalies, writers, OID frames, and validation. Its C ABI functions (module 7) follow in a separate pull request.

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2. It depends only on `aldis-core`, is stateless, and does no I/O. Frame counts and the OID generator's state belong to the caller, the engines of Milestone 4.

## What it provides

- The managed parameters of 132.0 §5: `TmPhysicalChannelConfig` (frame length, FECF), `TmMasterChannelConfig` (SCID, MC_FSH, MC_OCF, and up to eight virtual channels), and `TmVirtualChannelConfig` (data field content, VC_FSH, VC_OCF, and the SDLS Security Header and Trailer of §6.3). Building a master channel rejects what 132.0 forbids, and computes the layout of every VC's frames.
- A zero-copy decoder, `TmFrame`, that borrows the frame and gives every field (132.0 §4.1): `decode` for a channel without SDLS, `decode_with` taking each VC's security layout from a `VcLookup` (a master channel, or a slice of them), `decode_unchecked` without the FECF check for diagnostics, and `peek_length`. The OCF is an `aldis_core::ocf::Ocf`; a CLCW in it is decoded with `aldis-tc`.
- `TmFrame::validate`, which checks a decoded frame against its master channel: SCID, VCID, OCF and Secondary Header flags and lengths, the Synchronization Flag against the data field content, and the security layout.
- Encoders: the one-shot `encode`, the two-phase `TmFrameWriter`, and `TmFrameWriter::begin_oid` for Only Idle Data frames from an `OidGenerator` (132.0 §4.1.4.6). They compute the Secondary Header Identification Field, the OCF Flag, and the FECF themselves.
- Strict and lenient decoding (`aldis_core::DecodeMode`); a lenient decode reports the Packet Order Flag and Segment Length Identifier deviations it accepted as `TmAnomaly` flags.

The module table in [`src/lib.rs`](src/lib.rs) maps every module to its 132.0 clauses and requirement IDs.

## Features

None by default (GEN-005). `alloc` and `std` are strictly additive: no change in behaviour or wire output (GEN-006).

## Example

Encode a frame on a virtual channel with an OCF and decode it again:

```rust
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::ocf::Ocf;
use aldis_core::DecodeMode;
use aldis_tm::{
    DataFieldContent, DataFieldStatus, PacketPointer, TmFrame, TmFrameContent, TmFrameFields,
    TmMasterChannelConfig, TmPhysicalChannelConfig, TmVirtualChannelConfig, encode,
};

fn main() -> Result<(), aldis_core::Error> {
    // 32-octet frames with an FECF; spacecraft 42; VC 1 carries packets and an OCF.
    let pc = TmPhysicalChannelConfig::new(32, true)?;
    let vc1 = VcId::try_from(1_u8)?;
    let mc = TmMasterChannelConfig::new(pc, SpacecraftId::try_from(42_u16)?, None, false)
        .with_vc(TmVirtualChannelConfig::new(vc1, DataFieldContent::Packets).with_ocf())?;

    // A packet starts at the first data octet; the data field is 32 - 6 - 4 - 2 = 20 octets.
    let fields = TmFrameFields::new(vc1, 7, 3, DataFieldStatus::Packets(PacketPointer::StartsAt(0)));
    let data = [0xAB_u8; 20];
    let content = TmFrameContent {
        ocf: Some(Ocf::decode([0x01, 0x14, 0x0C, 0x2A])),
        ..TmFrameContent::new(&data)
    };
    let mut buf = [0_u8; 32];
    let len = encode(&mc, &fields, &content, &mut buf)?;

    // The view borrows `buf`; `validate` checks it against the master channel.
    let frame = TmFrame::decode_with(&buf[..len], mc.pc(), &mc, DecodeMode::Strict)?;
    frame.validate(&mc)?;
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

The shared conformance vectors are in [`test-vectors/tm/`](../../test-vectors/tm/), computed by the independent model [`test-vectors/models/tm_frame.py`](../../test-vectors/models/tm_frame.py), with one decode and one encode worked by hand in [docs/verification/derivations/tm-frame.md](../../docs/verification/derivations/tm-frame.md). 100 % coverage (VER-024); the bounded-loop inspection is [gen-022-023-aldis-tm.md](../../docs/verification/inspections/gen-022-023-aldis-tm.md). No independent implementation has been run against the TM vectors yet ([cross-checks.md](../../docs/verification/cross-checks.md), M2 plan D15).

## Specification

- [docs/standards/tm-frame.md](../../docs/standards/tm-frame.md): requirement area TM, with the 132.0 clause for each requirement, and the anomaly flags.
- [docs/standards/common-link-features.md](../../docs/standards/common-link-features.md): the FECF, the OCF, OID, counters, and field placement, implemented in `aldis-core`.
- [docs/03-api-model.md](../../docs/03-api-model.md): the codec contract (views, writers, decode modes, errors, field codes) shared by every codec.
- [docs/README.md](../../docs/README.md): the index of the specification.
