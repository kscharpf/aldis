# aldis-aos

Frame codec for the CCSDS 732.0-B-5 AOS Space Data Link Protocol: AOS Transfer Frames, the Frame Header Error Control, the M_PDU, and the B_PDU, in Issue 5 and in the wire format of Issue 4 and earlier.

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2. It depends only on `aldis-core`, is stateless, and does no I/O. Frame counts and the OID generator's state belong to the caller, the engines of Milestone 4.

## Example

Encode a frame with the FHEC and an M_PDU and decode it again, after a header symbol was hit:

```rust
use aldis_aos::{
    AosEdition, AosFrame, AosFrameContent, AosFrameFields, AosPhysicalChannelConfig,
    AosVirtualChannelConfig, CorrectedSymbols, DataFieldContent, DataUnit, PacketOffset,
    PacketPointer, PduHeader, VcFrameCount, VcTable, encode,
};
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::DecodeMode;

fn main() -> Result<(), aldis_core::Error> {
    // 32-octet frames with the FHEC and the FECF; spacecraft 42, VC 1 carries M_PDUs.
    let pc = AosPhysicalChannelConfig::new(32, true, 0, true, AosEdition::Issue5)?;
    let (scid, vcid) = (SpacecraftId::try_from(42)?, VcId::try_from(1)?);
    let vc = AosVirtualChannelConfig::new(scid, vcid, DataFieldContent::Mpdu)?;
    let table = VcTable::<4>::new(pc, &[vc])?;

    // 32 − 8 (header) − 2 (FECF) = 22 data octets: the M_PDU Header and 20.
    let zone = [0x5A; 20];
    let fields = AosFrameFields::new(
        scid,
        vcid,
        VcFrameCount::try_from(7)?,
        PduHeader::Mpdu(PacketPointer::StartsAt(PacketOffset::try_from(0)?)),
    );
    let mut frame = [0_u8; 32];
    let n = encode(&pc, &table, &fields, &AosFrameContent::new(&zone), &mut frame)?;
    assert_eq!(n, 32);

    frame[1] ^= 0x01; // a bit of the VCID
    let f = AosFrame::decode(&frame, &pc, &table, DecodeMode::Strict)?;
    assert_eq!((f.vcid(), f.corrected_symbols()), (vcid, CorrectedSymbols::One));
    assert!(matches!(f.data_unit(), DataUnit::Mpdu(m) if m.packet_zone() == zone));
    Ok(())
}
```
