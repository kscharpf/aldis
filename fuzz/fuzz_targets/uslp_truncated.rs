//! Truncated frames (732.1 Annex D) against an independent reading of the 4-octet header: on a
//! VC whose truncated frame length and SDLS use come from the input, octets with the End of
//! Frame Primary Header Flag set decode exactly when the VC is configured and the rule is
//! `011`–`111`, into the fields the header bits give, the 6/16 security fields when the VC uses
//! SDLS, and the rest as TFDZ; the writer reproduces the frame; and a data unit of such frames,
//! then fill, delimits back into them (USLP-002, USLP-012, USLP-015, USLP-016, API-075,
//! GEN-020).
#![no_main]

// req: USLP-002, USLP-012, USLP-015, USLP-016, API-075, GEN-020, VER-022

use aldis_core::security::SecurityLayout;
use aldis_uslp::{
    Delimited, FrameDelimiter, MapConfig, PhysicalChannelConfig, SecurityScope, TransferFrameType,
    TruncatedFrameLen, UslpFrame, UslpMapId, UslpSpacecraftId, UslpVcId, VcConfig, VcParams,
    VcTable, encode_truncated,
};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

fn run(u: &mut Unstructured<'_>) -> Result<()> {
    let scid = UslpSpacecraftId::new(u.arbitrary()?).unwrap();
    let vcid = UslpVcId::new(u.int_in_range(0..=62)?).unwrap();
    let map = UslpMapId::new(u.int_in_range(0..=15)?).unwrap();
    let sdls: bool = u.arbitrary()?;
    let len: usize = u.int_in_range(if sdls { 28..=32 } else { 6..=32 })?;
    let params = VcParams {
        truncated_frame_len: Some(TruncatedFrameLen::new(len).unwrap()),
        security: SecurityScope::PerMap,
        ..VcParams::new(TransferFrameType::Variable)
    };
    let layout = if sdls {
        SecurityLayout::new(u.int_in_range(1..=8)?, u.int_in_range(0..=8)?).unwrap()
    } else {
        SecurityLayout::NONE
    };
    let pc =
        PhysicalChannelConfig::new(TransferFrameType::Variable, 512, 0, u.arbitrary()?).unwrap();
    let table: VcTable<1, 1> = VcTable::new(
        pc,
        &[VcConfig::new(scid, vcid, params).unwrap()],
        &[MapConfig::new(scid, vcid, map, layout)],
    )
    .unwrap();
    let count: usize = u.int_in_range(1..=4)?;
    let mut unit = Vec::new();
    let mut frames = Vec::new();
    for _ in 0..count {
        let mut frame = u.bytes(len)?.to_vec();
        if frame.len() < len {
            return Ok(());
        }
        // The header the model writes: TFVN 1100, the SCID, a Source-or-Destination bit from
        // the input, the VCID, the MAP ID, and End of Frame Primary Header Flag 1 (Annex D1.3).
        let word = (0xC_u32 << 28)
            | (u32::from(scid.get()) << 12)
            | (u32::from(frame[2] >> 3 & 1) << 11)
            | (u32::from(vcid.get()) << 5)
            | (u32::from(map.get()) << 1)
            | 1;
        frame[..4].copy_from_slice(&word.to_be_bytes());
        let (hlen, tlen) = if sdls { (6, 16) } else { (0, 0) };
        let tfdf = frame[4 + hlen];
        let decoded = UslpFrame::decode(&frame, &table);
        if tfdf >> 5 < 3 {
            assert!(decoded.is_err(), "rule {:03b} accepted", tfdf >> 5);
            continue;
        }
        let dec = decoded.unwrap();
        let tr = dec.truncated().unwrap();
        assert_eq!(tr.header_octets(), frame[..4]);
        assert_eq!(dec.security_header(), &frame[4..4 + hlen]);
        assert_eq!(dec.security_trailer(), &frame[len - tlen..]);
        assert_eq!(dec.tfdz(), &frame[5 + hlen..len - tlen]);
        assert_eq!(dec.tfdf_header().upid.bits(), tfdf & 0x1F);
        assert_eq!(dec.tfdf_header().rule.rule().bits(), tfdf >> 5);
        assert_eq!(UslpFrame::peek_length(&frame, &table), Ok(len));
        let mut out = [0_u8; 32];
        let written =
            encode_truncated(&table, tr.channel(), tr.tfdf_header(), tr.tfdz(), &mut out).unwrap();
        assert_eq!(written, len);
        assert_eq!(
            &out[..4 + hlen],
            &[&frame[..4], &[0_u8; 6][..hlen]].concat()[..]
        );
        assert_eq!(&out[4 + hlen..len - tlen], &frame[4 + hlen..len - tlen]);
        frames.push(frame.clone());
        unit.extend_from_slice(&frame);
    }
    let fill = u.int_in_range(0..=6)?;
    unit.extend(std::iter::repeat_n(0x55, fill));
    let items: Vec<Delimited<'_>> = FrameDelimiter::new(&unit, &table).collect();
    let found: Vec<&[u8]> = items
        .iter()
        .filter_map(|i| match i {
            Delimited::Frame(f) => Some(*f),
            _ => None,
        })
        .collect();
    assert_eq!(found, frames.iter().map(Vec::as_slice).collect::<Vec<_>>());
    assert_eq!(items.iter().map(Delimited::len).sum::<usize>(), unit.len());
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = run(&mut Unstructured::new(data));
});
