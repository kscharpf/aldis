//! Arbitrary master channels, frame fields, and contents through `encode`, the two-phase writer,
//! and the OID writer, then `decode_with`: `decode(encode(x)) == x`, both encoders agree, the
//! frame matches its master channel, and a rejected encode leaves the output buffer and the OID
//! generator unchanged (API-020…API-022, TM-009, TM-010, GEN-042).
#![no_main]

// req: API-020, API-021, API-022, API-023, TM-009, TM-010, TM-011, CMN-021, GEN-042, VER-022

use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::ocf::Ocf;
use aldis_core::oid::OidGenerator;
use aldis_core::security::{FrameCtx, PreFilledSecurity, SecurityLayout};
use aldis_tm::{
    DataFieldContent, DataFieldStatus, FirstHeaderPointer, FshLen, PacketOffset, PacketPointer,
    SegmentLengthId, TmFrame, TmFrameContent, TmFrameFields, TmFrameWriter, TmMasterChannelConfig,
    TmPhysicalChannelConfig, TmVirtualChannelConfig, encode,
};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

/// A master channel drawn from the input; `None` if the input's choices are rejected.
fn master_channel(u: &mut Unstructured<'_>) -> Result<Option<TmMasterChannelConfig>> {
    let Ok(pc) = TmPhysicalChannelConfig::new(u.int_in_range(0..=300)?, u.arbitrary()?) else {
        return Ok(None);
    };
    let scid = SpacecraftId::new(u.int_in_range(0..=1023)?).unwrap();
    let fsh = |u: &mut Unstructured<'_>| -> Result<Option<FshLen>> {
        Ok(if u.ratio(1, 4)? {
            FshLen::new(u.int_in_range(2..=64)?)
        } else {
            None
        })
    };
    let mut mc = TmMasterChannelConfig::new(pc, scid, fsh(u)?, u.ratio(1, 4)?);
    for _ in 0..u.int_in_range(1..=8)? {
        let content = if u.arbitrary()? {
            DataFieldContent::VcaSdu
        } else {
            DataFieldContent::Packets
        };
        let mut vc =
            TmVirtualChannelConfig::new(VcId::new(u.int_in_range(0..=7)?).unwrap(), content);
        if let Some(len) = fsh(u)? {
            vc = vc.with_fsh(len);
        }
        if u.ratio(1, 4)? {
            vc = vc.with_ocf();
        }
        let security = SecurityLayout::new(u.int_in_range(0..=8)?, u.int_in_range(0..=8)?).unwrap();
        if let Ok(next) = mc.clone().with_vc(vc.with_security(security)) {
            mc = next;
        }
    }
    Ok(Some(mc))
}

fn status(u: &mut Unstructured<'_>) -> Result<DataFieldStatus> {
    Ok(if u.arbitrary()? {
        DataFieldStatus::VcaSdu {
            packet_order_flag: u.arbitrary()?,
            segment_length_id: SegmentLengthId::new(u.int_in_range(0..=3)?).unwrap(),
            first_header_pointer: FirstHeaderPointer::new(u.int_in_range(0..=0x7FF)?).unwrap(),
        }
    } else {
        DataFieldStatus::Packets(match u.int_in_range(0..=3)? {
            0 => PacketPointer::NoPacketStart,
            1 => PacketPointer::OnlyIdleData,
            _ => PacketPointer::StartsAt(
                PacketOffset::new(u.int_in_range(0..=PacketOffset::MAX)?).unwrap(),
            ),
        })
    })
}

fn run(u: &mut Unstructured<'_>) -> Result<()> {
    let Some(mc) = master_channel(u)? else {
        return Ok(());
    };
    let fields = TmFrameFields::new(
        VcId::new(u.int_in_range(0..=7)?).unwrap(),
        u.arbitrary()?,
        u.arbitrary()?,
        status(u)?,
    );
    // Lengths near the configured ones, so that most encodes succeed.
    let layout = mc.frame_layout(fields.vcid).cloned();
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
            l.secondary().len().saturating_sub(1),
            l.security_header().len(),
            l.data().len(),
            l.security_trailer().len(),
        ]
    });
    let fsh = part(u, sizes[0])?;
    let sh = part(u, sizes[1])?;
    let data = part(u, sizes[2])?;
    let st = part(u, sizes[3])?;
    let has_ocf = layout.as_ref().is_some_and(|l| !l.ocf().is_empty());
    let ocf = if u.ratio(15, 16)? == has_ocf {
        Some(Ocf::decode(u.arbitrary()?))
    } else {
        None
    };
    let content = TmFrameContent {
        secondary_header_data: &fsh,
        security_header: &sh,
        data: &data,
        security_trailer: &st,
        ocf,
    };
    let cap = if u.ratio(7, 8)? {
        mc.pc().frame_len()
    } else {
        u.int_in_range(0..=320)?
    };
    let mut out = vec![0xA5_u8; cap];
    match encode(&mc, &fields, &content, &mut out) {
        Err(_) => assert!(
            out.iter().all(|&b| b == 0xA5),
            "rejected encode wrote output"
        ),
        Ok(n) => {
            let f = TmFrame::decode_with(&out[..n], mc.pc(), &mc).unwrap();
            assert_eq!(f.vcid(), fields.vcid);
            assert_eq!(f.scid(), mc.scid());
            assert_eq!(
                (f.mc_frame_count(), f.vc_frame_count()),
                (fields.mc_frame_count, fields.vc_frame_count)
            );
            assert_eq!(f.data_field_status(), fields.status);
            assert_eq!(
                f.secondary_header_data().unwrap_or_default(),
                fsh.as_slice()
            );
            assert_eq!(f.security_header(), sh.as_slice());
            assert_eq!(f.data_field(), data.as_slice());
            assert_eq!(f.security_trailer(), st.as_slice());
            assert_eq!(f.ocf(), ocf);
            assert!(f.validate(&mc).is_ok());
            let mut two = vec![0x5A_u8; n];
            let mut w = TmFrameWriter::begin(&mut two, &mc, &fields).unwrap();
            w.secondary_header_data_mut().copy_from_slice(&fsh);
            w.security_header_mut().copy_from_slice(&sh);
            w.data_field_mut().copy_from_slice(&data);
            w.security_trailer_mut().copy_from_slice(&st);
            if let Some(ocf) = ocf {
                w.set_ocf(ocf).unwrap();
            }
            let ctx = FrameCtx::Tm {
                scid: mc.scid(),
                vcid: fields.vcid,
            };
            assert_eq!(
                w.protect(&mut PreFilledSecurity, &ctx)
                    .unwrap()
                    .finalize()
                    .len(),
                n
            );
            assert_eq!(two[..n], out[..n]);
        }
    }
    oid_frame(u, &mc, fields.vcid, cap)
}

/// OID frames: the generator advances by exactly the data field, or not at all (TM-010,
/// CMN-021, GEN-042).
fn oid_frame(
    u: &mut Unstructured<'_>,
    mc: &TmMasterChannelConfig,
    vcid: VcId<3>,
    cap: usize,
) -> Result<()> {
    let mut oid = OidGenerator::new();
    let mut reference = OidGenerator::new();
    let skip: u8 = u.arbitrary()?;
    let mut skipped = vec![0_u8; usize::from(skip)];
    oid.fill(&mut skipped);
    reference.fill(&mut skipped);
    let before = oid.clone();
    let mut frame = vec![0_u8; cap];
    match TmFrameWriter::begin_oid(&mut frame, mc, vcid, 0, 0, &mut oid) {
        Err(_) => assert_eq!(oid, before, "a rejected OID frame advanced the generator"),
        Ok(w) => {
            let n = w.unsecured().unwrap().finalize().len();
            let f = TmFrame::decode_with(&frame[..n], mc.pc(), mc).unwrap();
            assert_eq!(
                f.data_field_status(),
                DataFieldStatus::Packets(PacketPointer::OnlyIdleData)
            );
            let mut expected = vec![0_u8; f.data_field().len()];
            reference.fill(&mut expected);
            assert_eq!(f.data_field(), expected.as_slice());
            assert_eq!(oid, reference);
        }
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = run(&mut Unstructured::new(data));
});
