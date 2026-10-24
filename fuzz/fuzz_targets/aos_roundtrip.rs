//! Arbitrary virtual channel tables, frame fields, and contents through `encode`, the two-phase
//! writer, and the OID writer, then `AosFrame::decode`: `decode(encode(x)) == x`, both encoders
//! agree, up to two header symbols are corrected, and a rejected encode leaves the output buffer
//! and the OID generator unchanged (API-020…API-022, AOS-014, AOS-016, AOS-019, GEN-042).
#![no_main]

// req: API-020, API-021, API-022, API-023, AOS-013, AOS-014, AOS-015, AOS-016, AOS-019, CMN-021, GEN-042, VER-022

use aldis_aos::{
    AosEdition, AosFrame, AosFrameContent, AosFrameFields, AosFrameWriter,
    AosPhysicalChannelConfig, AosVirtualChannelConfig, BitstreamPointer, DataFieldContent,
    DataUnit, FrameCountCycle, PacketPointer, PduHeader, VcFrameCount, VcTable, encode,
};
use aldis_core::DecodeMode;
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::ocf::Ocf;
use aldis_core::oid::OidGenerator;
use aldis_core::security::SecurityLayout;
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

/// A physical channel and a table of its virtual channels drawn from the input; `None` if the
/// input's choices are rejected.
fn table(u: &mut Unstructured<'_>) -> Result<Option<(AosPhysicalChannelConfig, VcTable<4>)>> {
    let edition = if u.ratio(1, 4)? {
        AosEdition::Issue4Compatible
    } else {
        AosEdition::Issue5
    };
    let Ok(pc) = AosPhysicalChannelConfig::new(
        u.int_in_range(0..=300)?,
        u.arbitrary()?,
        u.int_in_range(0..=8)?,
        u.arbitrary()?,
        edition,
    ) else {
        return Ok(None);
    };
    let mut vcs = Vec::new();
    for _ in 0..u.int_in_range(1..=4)? {
        let content = match u.int_in_range(0..=3)? {
            0 => DataFieldContent::Mpdu,
            1 => DataFieldContent::Bpdu,
            2 => DataFieldContent::VcaSdu,
            _ => DataFieldContent::IdleData,
        };
        let scid = SpacecraftId::new(u.int_in_range(0..=1023)?).unwrap();
        let Ok(mut vc) = AosVirtualChannelConfig::new(
            scid,
            VcId::new(u.int_in_range(0..=63)?).unwrap(),
            content,
        ) else {
            continue;
        };
        if u.ratio(1, 4)? {
            vc = vc.with_ocf();
        }
        let security = SecurityLayout::new(u.int_in_range(0..=8)?, u.int_in_range(0..=8)?).unwrap();
        let mut with = vcs.clone();
        with.push(vc.with_security(security));
        if VcTable::<4>::new(pc, &with).is_ok() {
            vcs = with;
        }
    }
    Ok(VcTable::new(pc, &vcs).ok().map(|vcs_table| (pc, vcs_table)))
}

fn pdu(u: &mut Unstructured<'_>) -> Result<PduHeader> {
    Ok(match u.int_in_range(0..=6)? {
        0 => PduHeader::None,
        1 => PduHeader::Mpdu(PacketPointer::NoPacketStart),
        2 => PduHeader::Mpdu(PacketPointer::OnlyIdleData),
        3 => PduHeader::Mpdu(PacketPointer::StartsAt(u.int_in_range(0..=300)?)),
        4 => PduHeader::Bpdu(BitstreamPointer::AllValid),
        5 => PduHeader::Bpdu(BitstreamPointer::OnlyIdle),
        _ => PduHeader::Bpdu(BitstreamPointer::ValidBits(u.int_in_range(0..=2400)?)),
    })
}

/// Fields of a VC of `vcs_table` most of the time, any fields otherwise.
fn fields(u: &mut Unstructured<'_>, vcs_table: &VcTable<4>) -> Result<AosFrameFields> {
    let vcs: Vec<_> = vcs_table.vcs().copied().collect();
    let (scid, vcid) = if u.ratio(7, 8)? {
        let vc = u.choose(&vcs)?;
        (vc.scid(), vc.vcid())
    } else {
        (
            SpacecraftId::new(u.int_in_range(0..=1023)?).unwrap(),
            VcId::new(u.int_in_range(0..=63)?).unwrap(),
        )
    };
    let cycle = if u.arbitrary()? {
        FrameCountCycle::new(u.int_in_range(0..=15)?)
    } else {
        None
    };
    Ok(AosFrameFields {
        scid,
        vcid,
        vc_frame_count: VcFrameCount::new(u.int_in_range(0..=0xFF_FFFF)?).unwrap(),
        replay_flag: u.arbitrary()?,
        cycle,
        pdu: pdu(u)?,
    })
}

fn run(u: &mut Unstructured<'_>) -> Result<()> {
    let Some((pc, vcs_table)) = table(u)? else {
        return Ok(());
    };
    let fields = fields(u, &vcs_table)?;
    // Lengths near the configured ones, so that most encodes succeed.
    let layout = vcs_table.frame_layout(fields.scid, fields.vcid);
    let header = match fields.pdu {
        PduHeader::None => 0,
        PduHeader::Mpdu(_) | PduHeader::Bpdu(_) => 2,
    };
    let part = |u: &mut Unstructured<'_>, want: usize| -> Result<Vec<u8>> {
        let len = if u.ratio(15, 16)? {
            want
        } else {
            u.int_in_range(0..=want + 2)?
        };
        Ok(u.bytes(len.min(u.len()))?.to_vec())
    };
    let sizes = layout.as_ref().map_or([0; 4], |l| {
        [
            l.secondary().len(),
            l.security_header().len(),
            l.data().len().saturating_sub(header),
            l.security_trailer().len(),
        ]
    });
    let iz = part(u, sizes[0])?;
    let sh = part(u, sizes[1])?;
    let zone = part(u, sizes[2])?;
    let st = part(u, sizes[3])?;
    let has_ocf = layout.as_ref().is_some_and(|l| !l.ocf().is_empty());
    let ocf = if u.ratio(15, 16)? == has_ocf {
        Some(Ocf::decode(u.arbitrary()?))
    } else {
        None
    };
    let content = AosFrameContent {
        insert_zone: &iz,
        security_header: &sh,
        zone: &zone,
        security_trailer: &st,
        ocf,
    };
    let cap = if u.ratio(7, 8)? {
        pc.frame_len()
    } else {
        u.int_in_range(0..=320)?
    };
    let mut out = vec![0xA5_u8; cap];
    match encode(&pc, &vcs_table, &fields, &content, &mut out) {
        Err(_) => assert!(
            out.iter().all(|&b| b == 0xA5),
            "rejected encode wrote output"
        ),
        Ok(n) => {
            let f = AosFrame::decode(&out[..n], &pc, &vcs_table, DecodeMode::Strict).unwrap();
            assert_eq!((f.scid(), f.vcid()), (fields.scid, fields.vcid));
            assert_eq!(f.vc_frame_count(), fields.vc_frame_count);
            assert_eq!(
                (f.replay_flag(), f.vc_frame_count_cycle()),
                (fields.replay_flag, fields.cycle)
            );
            let decoded_pdu = match f.data_unit() {
                DataUnit::Mpdu(m) => PduHeader::Mpdu(m.pointer()),
                DataUnit::Bpdu(b) => PduHeader::Bpdu(b.pointer()),
                DataUnit::VcaSdu(_) | DataUnit::IdleData(_) => PduHeader::None,
            };
            assert_eq!(decoded_pdu, fields.pdu);
            assert_eq!(f.insert_zone(), iz.as_slice());
            assert_eq!(f.security_header(), sh.as_slice());
            assert_eq!(f.data_unit().zone(), zone.as_slice());
            assert_eq!(f.security_trailer(), st.as_slice());
            assert_eq!(f.ocf(), ocf);
            assert!(f.anomalies().is_empty());
            let mut two = vec![0x5A_u8; n];
            let mut w = AosFrameWriter::begin(&mut two, &pc, &vcs_table, &fields).unwrap();
            w.insert_zone_mut().copy_from_slice(&iz);
            w.security_header_mut().copy_from_slice(&sh);
            w.zone_mut().copy_from_slice(&zone);
            w.security_trailer_mut().copy_from_slice(&st);
            if let Some(ocf) = ocf {
                w.set_ocf(ocf).unwrap();
            }
            assert_eq!(w.finish(), n);
            assert_eq!(two[..n], out[..n]);
            corrected(u, &pc, &vcs_table, &out[..n], &f)?;
        }
    }
    oid_frame(u, &pc, &vcs_table, fields.scid, cap)
}

/// With the FHEC, up to two header symbols changed are corrected back to the same view
/// (AOS-013, AOS-014).
fn corrected(
    u: &mut Unstructured<'_>,
    pc: &AosPhysicalChannelConfig,
    vcs_table: &VcTable<4>,
    frame: &[u8],
    clean: &AosFrame<'_>,
) -> Result<()> {
    if !pc.fhec() {
        return Ok(());
    }
    let mut bad = frame.to_vec();
    let a: usize = u.int_in_range(0..=9)?;
    let b: usize = u.int_in_range(0..=9)?;
    for (symbol, value) in [(a, u.int_in_range(1..=15)?), (b, u.int_in_range(0..=15)?)] {
        let octet = [0, 1, 5, 6, 7][symbol / 2];
        bad[octet] ^= if symbol % 2 == 0 { value << 4 } else { value };
    }
    let f = AosFrame::decode(&bad, pc, vcs_table, DecodeMode::Strict).unwrap();
    assert_eq!(f.header().to_octets(), clean.header().to_octets());
    assert_eq!(f.data_unit(), clean.data_unit());
    Ok(())
}

/// OID frames: the generator advances by exactly the data field, or not at all (AOS-016,
/// CMN-021, GEN-042).
fn oid_frame(
    u: &mut Unstructured<'_>,
    pc: &AosPhysicalChannelConfig,
    vcs_table: &VcTable<4>,
    scid: SpacecraftId<10>,
    cap: usize,
) -> Result<()> {
    let mut oid = OidGenerator::new();
    let mut reference = OidGenerator::new();
    let skip: u8 = u.arbitrary()?;
    let mut skipped = vec![0_u8; usize::from(skip)];
    oid.fill(&mut skipped);
    reference.fill(&mut skipped);
    let before = oid.clone();
    let count = VcFrameCount::new(u.int_in_range(0..=0xFF_FFFF)?).unwrap();
    let mut frame = vec![0_u8; cap];
    match AosFrameWriter::begin_oid(
        &mut frame,
        pc,
        vcs_table,
        &AosFrameFields::oid(scid, count),
        &mut oid,
    ) {
        Err(_) => assert_eq!(oid, before, "a rejected OID frame advanced the generator"),
        Ok(w) => {
            let n = w.finish();
            let f = AosFrame::decode(&frame[..n], pc, vcs_table, DecodeMode::Strict).unwrap();
            assert!(f.is_oid());
            let mut expected = vec![0_u8; f.data_field().len()];
            reference.fill(&mut expected);
            assert_eq!(f.data_unit(), DataUnit::IdleData(&expected));
            assert_eq!(oid, reference);
        }
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = run(&mut Unstructured::new(data));
});
