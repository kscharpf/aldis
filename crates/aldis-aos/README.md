# aldis-aos

Frame codec for the CCSDS 732.0-B-5 AOS Space Data Link Protocol: AOS Transfer Frames, the Frame Header Error Control, the M_PDU, and the B_PDU, in Issue 5 and in the wire format of Issue 4 and earlier.

**Status:** implemented in Milestone 2, work package 4 ([plan](../../docs/milestones/m2-frame-codecs.md)), modules 1–7: the FHEC, configuration and editions, the Primary Header, the frame view, the M_PDU and B_PDU, the writers, and the anomaly flags; and module 8, its C ABI functions `aldis_aos_*` in `aldis-ffi`.

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2. It depends only on `aldis-core`, is stateless, and does no I/O. Frame counts and the OID generator's state belong to the caller, the engines of Milestone 4.

## What it provides

- The edition (`AosEdition::Issue5`, the default, or `Issue4Compatible`; AOS-001, GEN-062) and the managed parameters of 732.0 §5: `AosPhysicalChannelConfig` (frame length, FHEC, Insert Zone, FECF, edition) and `AosVirtualChannelConfig` (SCID, VCID 0–62, data field content, `VC_OCF`, and the SDLS Security Header and Trailer of §6.3), gathered in a fixed-capacity `VcTable<N>` that implements the `VcLookup` trait. Building them rejects what 732.0 forbids, including VCID 63 for a data VC, an SCID too wide for Issue 4, and the B_PDU limits of AOS-018.
- `AosPrimaryHeader::decode`, which corrects up to two symbol errors with the FHEC before any field is read and reads the SCID as the edition defines it (AOS-002, AOS-010, AOS-014), and the `fhec` module itself (AOS-013).
- A zero-copy decoder, `AosFrame::decode`, that takes the physical channel and a `VcLookup`, checks the FECF over the corrected header (732.0 §4.3.7.2–3), and gives every field: the Insert Zone, the security slices, the data field as a `DataUnit` (`Mpdu`, `Bpdu`, `VcaSdu`, or `IdleData` for OID frames on VCID 63), the OCF as an `aldis_core::ocf::Ocf`, the FECF, and `corrected_symbols()`; and `peek_length`. `Mpdu::decode` and `Bpdu::decode` classify their pointers in both editions (AOS-003, AOS-017).
- Encoders: the one-shot `encode`, the two-phase `AosFrameWriter`, and `AosFrameWriter::begin_oid` for OID frames on VCID 63 from an `OidGenerator` (AOS-016). They compute the FHEC, the M_PDU or B_PDU Header, and the FECF themselves (AOS-019). `AosFrameWriter::resume` takes up a frame `begin` or `begin_oid` started, for a caller that keeps no writer between the two steps, such as the C ABI's `aldis_aos_writer_finish` (API-023).
- Strict and lenient decoding (`aldis_core::DecodeMode`); a lenient decode reports the deviations it accepted as `AosAnomaly` flags.

The module table in [`src/lib.rs`](src/lib.rs) maps every module to its 732.0 clauses and requirement IDs.

## Features

None by default (GEN-005). `alloc` and `std` are strictly additive: no change in behaviour or wire output (GEN-006).

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

## Verification

- Unit tests in every module; property tests in `tests/properties.rs` drawing from `aldis_testkit::strategies::aos` (VER-023, VER-054); the FHEC checked against the independent table model `aldis_testkit::oracle::fhec` on all 65 536 syndromes.
- Vectors in [`test-vectors/aos/`](../../test-vectors/aos/), computed by the independent models [`aos_fhec.py`](../../test-vectors/models/aos_fhec.py) and [`aos_frame.py`](../../test-vectors/models/aos_frame.py), with the hand derivations in [aos-fhec.md](../../docs/verification/derivations/aos-fhec.md) and [aos-frame.md](../../docs/verification/derivations/aos-frame.md); the fuzz targets `aos_fhec_model`, `aos_decode`, and `aos_roundtrip`; the exerciser on three bare-metal targets; 100 % coverage (VER-024); the [GEN-022/023 inspection](../../docs/verification/inspections/gen-022-023-aldis-aos.md).

## Specification

- [docs/standards/aos-frame.md](../../docs/standards/aos-frame.md): requirement area AOS.
- [docs/standards/common-link-features.md](../../docs/standards/common-link-features.md): the FECF, OID, counters, and field placement, implemented in `aldis-core`.
- [docs/09-open-issues.md](../../docs/09-open-issues.md): OI-04 (the editions, resolved), OI-35 (FHEC miscorrection, the B_PDU limits), OI-43 (frames on VCID 63).
- [docs/README.md](../../docs/README.md): the index of the specification.
