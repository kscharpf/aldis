//! Arbitrary fields and data through `encode` and the two-phase writer, then `decode`:
//! `decode(encode(x)) == x`, both writers agree, and a rejected encode leaves the output buffer
//! unchanged (TC-004, TC-005, TC-007, TC-008, API-020…API-023, GEN-042).
#![no_main]

// req: TC-004, TC-005, TC-007, TC-008, API-020, API-021, API-022, API-023, GEN-042, VER-022

use aldis_core::DecodeMode;
use aldis_core::ids::FrameSeqNum;
use aldis_core::security::{FrameCtx, PreFilledSecurity, SecurityLayout};
use aldis_tc::{
    ControlCommand, FrameData, FrameFields, FrameType, MapConfig, PhysicalChannelConfig,
    SegmentHeader, SegmentHeaderUse, SequenceFlags, TcFrame, TcFrameWriter, TcMapId,
    TcSpacecraftId, TcVcId, VcConfig, VcTable, encode,
};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

fn run(u: &mut Unstructured<'_>) -> Result<()> {
    let scid = TcSpacecraftId::new(0x155).unwrap();
    let Ok(pc) = PhysicalChannelConfig::new(u.int_in_range(6..=1024)?, u.arbitrary()?) else {
        return Ok(());
    };
    let plain = SegmentHeaderUse::Absent(
        SecurityLayout::new(u.int_in_range(0..=6)?, u.int_in_range(0..=4)?).unwrap(),
    );
    let vcs = [
        VcConfig::new(
            scid,
            TcVcId::new(1).unwrap(),
            u.int_in_range(6..=1024)?,
            plain,
        )
        .unwrap(),
        VcConfig::new(
            scid,
            TcVcId::new(2).unwrap(),
            1024,
            SegmentHeaderUse::Present,
        )
        .unwrap(),
    ];
    let maps = [MapConfig::new(
        scid,
        TcVcId::new(2).unwrap(),
        TcMapId::new(5).unwrap(),
        SecurityLayout::new(u.int_in_range(0..=6)?, u.int_in_range(0..=4)?).unwrap(),
    )];
    let Ok(table) = VcTable::<2, 1>::new(pc, &vcs, &maps) else {
        return Ok(());
    };
    // Raw identifiers: some name no configured channel, and the encoder must say so.
    let fields = FrameFields {
        frame_type: match u.int_in_range(0..=2)? {
            0 => FrameType::Ad,
            1 => FrameType::Bd,
            _ => FrameType::Bc,
        },
        spacecraft_id: TcSpacecraftId::new(if u.arbitrary()? { 0x155 } else { 0x156 }).unwrap(),
        vc_id: TcVcId::new(u.int_in_range(0..=3)?).unwrap(),
        sequence_number: FrameSeqNum::new(u.arbitrary()?),
    };
    let segment = if u.arbitrary()? {
        Some(SegmentHeader {
            sequence_flags: SequenceFlags::from_bits(u.arbitrary()?),
            map_id: TcMapId::new(u.int_in_range(4..=6)?).unwrap(),
        })
    } else {
        None
    };
    let len = usize::from(u.int_in_range(0_u16..=1030)?);
    let user = u.bytes(len.min(u.len()))?.to_vec();
    let data = match u.int_in_range(0..=2)? {
        0 => FrameData::Control(ControlCommand::Unlock),
        1 => FrameData::Control(ControlCommand::SetVr(FrameSeqNum::new(u.arbitrary()?))),
        _ => FrameData::Fdu {
            segment_header: segment,
            user_data: &user,
        },
    };
    let cap = usize::from(u.int_in_range(0_u16..=1100)?);
    let mut out = vec![0xA5_u8; cap];
    match encode(&table, fields, data, &mut out) {
        Err(_) => assert!(
            out.iter().all(|&b| b == 0xA5),
            "rejected encode wrote output"
        ),
        Ok(n) => {
            let f = TcFrame::decode(&out[..n], &table, DecodeMode::Strict).unwrap();
            assert_eq!(f.len(), n);
            assert_eq!(f.header().fields(), fields);
            match data {
                FrameData::Control(cmd) => assert_eq!(f.control_command(), Some(cmd)),
                FrameData::Fdu {
                    segment_header,
                    user_data,
                } => {
                    assert_eq!(f.segment_header(), segment_header);
                    assert_eq!(f.data_field(), user_data);
                    two_phase(&table, fields, segment_header, &user, &out[..n]);
                }
            }
        }
    }
    Ok(())
}

/// The two-phase writer gives the frame of `encode` (API-023).
fn two_phase(
    table: &VcTable<2, 1>,
    fields: FrameFields,
    segment_header: Option<SegmentHeader>,
    user: &[u8],
    frame: &[u8],
) {
    let mut two = vec![0x5A_u8; frame.len()];
    let mut w = TcFrameWriter::begin(&mut two, table, fields, segment_header, user.len()).unwrap();
    w.user_data_mut().copy_from_slice(user);
    let ctx = FrameCtx::Tc {
        scid: fields.spacecraft_id,
        vcid: fields.vc_id,
        map_id: None,
    };
    let done = w
        .protect(user.len(), &mut PreFilledSecurity, &ctx)
        .unwrap()
        .finalize()
        .unwrap();
    assert_eq!(done.len(), frame.len());
    assert_eq!(two, frame);
}

fuzz_target!(|data: &[u8]| {
    let _ = run(&mut Unstructured::new(data));
});
