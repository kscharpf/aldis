//! Arbitrary octets through `TcFrame::decode`, `peek_length`, and `validate_frame` on channels
//! drawn from the input: no panic; strict ⊆ lenient; `peek_length` agrees with `decode`; the
//! Frame Validation Checks accept exactly the decoded frame; and a strict decode re-encodes, with
//! its Security Header and Trailer, to the same octets (TC-001…TC-006, TC-021, TC-023, GEN-020,
//! GEN-042).
#![no_main]

// req: TC-001, TC-002, TC-003, TC-004, TC-005, TC-006, TC-021, TC-023, API-020, GEN-020, GEN-042, VER-022

use aldis_core::crc::crc16;
use aldis_core::security::SecurityLayout;
use aldis_core::{DecodeMode, ErrorKind};
use aldis_tc::{
    FrameData, MapConfig, PhysicalChannelConfig, SegmentHeaderUse, TcFrame, TcFrameWriter, TcMapId,
    TcSpacecraftId, TcVcId, VcConfig, VcTable, encode, validate_frame,
};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

/// SCID 0x155: VC 1 without Segment Headers, VC 2 with MAPs 0, 5, 9; lengths and SDLS from `u`.
fn channels(u: &mut Unstructured<'_>) -> Result<Option<VcTable<2, 3>>> {
    let scid = TcSpacecraftId::new(0x155).unwrap();
    let security = |u: &mut Unstructured<'_>| -> Result<SecurityLayout> {
        Ok(SecurityLayout::new(u.int_in_range(0..=6)?, u.int_in_range(0..=4)?).unwrap())
    };
    let Ok(pc) = PhysicalChannelConfig::new(u.int_in_range(6..=1024)?, u.arbitrary()?) else {
        return Ok(None);
    };
    let plain = SegmentHeaderUse::Absent(security(u)?);
    let vcs = [
        VcConfig::new(scid, TcVcId::new(1).unwrap(), 1024, plain).unwrap(),
        VcConfig::new(
            scid,
            TcVcId::new(2).unwrap(),
            1024,
            SegmentHeaderUse::Present,
        )
        .unwrap(),
    ];
    let mut maps = Vec::new();
    for m in [0, 5, 9] {
        maps.push(MapConfig::new(
            scid,
            TcVcId::new(2).unwrap(),
            TcMapId::new(m).unwrap(),
            security(u)?,
        ));
    }
    Ok(VcTable::new(pc, &vcs, &maps).ok())
}

fn run(u: &mut Unstructured<'_>) -> Result<()> {
    let Some(table) = channels(u)? else {
        return Ok(());
    };
    let (aim, fix_fecf): (bool, bool) = (u.arbitrary()?, u.arbitrary()?);
    let mut bytes = u.bytes(u.len())?.to_vec();
    // Often aim the header at a configured channel, and give the frame a correct FECF.
    if aim && bytes.len() >= 5 {
        bytes[0] = (bytes[0] & 0xFC) | 0x01;
        bytes[1] = 0x55;
        bytes[2] = (bytes[2] & 0x03) | if bytes[2] & 0x80 == 0 { 0x04 } else { 0x08 };
    }
    if fix_fecf && bytes.len() >= 7 {
        let n = (usize::from(u16::from_be_bytes([bytes[2] & 3, bytes[3]])) + 1).min(bytes.len());
        if n >= 7 {
            let crc = crc16(&bytes[..n - 2]).to_be_bytes();
            bytes[n - 2..n].copy_from_slice(&crc);
        }
    }
    let input = bytes.clone();
    let strict = TcFrame::decode(&bytes, &table, DecodeMode::Strict);
    let lenient = TcFrame::decode(&bytes, &table, DecodeMode::Lenient);
    assert_eq!(bytes, input, "decode changed its input");
    if let Ok(s) = &strict {
        assert!(s.anomalies().is_empty());
        assert_eq!(lenient.as_ref().map(TcFrame::header), Ok(s.header()));
    }
    match &lenient {
        Ok(f) => {
            assert_eq!(f.octets(), &bytes[..f.len()]);
            assert_eq!(TcFrame::peek_length(&bytes), Ok(f.len()));
            let v = validate_frame(f.octets(), &table, DecodeMode::Lenient).unwrap();
            assert_eq!(&v, f);
            if f.len() < bytes.len() {
                assert!(validate_frame(&bytes, &table, DecodeMode::Lenient).is_err());
            }
        }
        Err(e) => {
            assert!(
                validate_frame(&bytes, &table, DecodeMode::Lenient).is_err(),
                "{e:?}"
            );
            if e.kind() == ErrorKind::Incomplete {
                assert!(bytes.len() < TcFrame::peek_length(&bytes).unwrap_or(usize::MAX));
            }
        }
    }
    // A strict decode re-encodes to the same octets (API-020).
    if let Ok(f) = strict {
        let mut out = vec![0_u8; f.len()];
        let n = if let Some(cmd) = f.control_command() {
            encode(
                &table,
                f.header().fields(),
                FrameData::Control(cmd),
                &mut out,
            )
        } else {
            let data = f.data_field();
            let mut w = TcFrameWriter::begin(
                &mut out,
                &table,
                f.header().fields(),
                f.segment_header(),
                data.len(),
            )
            .unwrap();
            w.user_data_mut().copy_from_slice(data);
            w.security_header_mut().copy_from_slice(f.security_header());
            w.security_trailer_mut()
                .copy_from_slice(f.security_trailer());
            w.finish(data.len())
        };
        assert_eq!(n, Ok(f.len()));
        assert_eq!(out, f.octets());
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = run(&mut Unstructured::new(data));
});
