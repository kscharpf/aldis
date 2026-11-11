//! Arbitrary octets through `TcFrame::decode`, `peek_length`, and `check_frame` on channels
//! drawn from the input: no panic; strict ⊆ lenient; `peek_length` agrees with `decode`; the
//! Frame Validation Checks accept the decoded frame of a configured channel and reject that of an
//! unknown one; and a strict decode of a configured channel re-encodes, with its Security Header
//! and Trailer, to the same octets (TC-001…TC-006, TC-021, TC-023, GEN-020, GEN-042).
#![no_main]

// req: TC-001, TC-002, TC-003, TC-004, TC-005, TC-006, TC-021, TC-023, API-020, GEN-020, GEN-042, VER-022

use aldis_core::DecodeMode;
use aldis_core::ErrorKind;
use aldis_core::crc::crc16;
use aldis_core::lookup::MapLookup;
use aldis_core::security::{FrameCtx, PreFilledSecurity, SecurityLayout};
use aldis_tc::{
    FrameData, MapConfig, PhysicalChannelConfig, Reason, SegmentHeaderUse, TcFrame, TcMapId,
    TcPlan, TcSpacecraftId, TcVcId, VcConfig, VcTable, check_frame, encode,
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
    let strict = TcFrame::decode(&bytes, &table);
    let lenient = TcFrame::decode_lenient(&bytes, &table);
    assert_eq!(bytes, input, "decode changed its input");
    if let Ok(s) = &strict {
        assert_eq!(lenient.as_ref().map(|l| l.header()), Ok(s.header()));
        assert!(lenient.as_ref().is_ok_and(|l| l.anomalies().is_empty()));
    }
    match &lenient {
        Ok(f) => {
            assert_eq!(f.octets(), &bytes[..f.len()]);
            assert_eq!(TcFrame::peek_length(&bytes), Ok(f.len()));
            // The decoder reads a frame of an unknown channel; the checks reject it (RS-023).
            match check_frame(f.octets(), &table, DecodeMode::Lenient) {
                Ok(v) => assert_eq!(&**v, &**f),
                Err(r) => assert!(
                    matches!(
                        r.reason(),
                        Reason::UnexpectedSpacecraft | Reason::InvalidVcid
                    ) && r.error().kind() == ErrorKind::UnknownChannel,
                    "{r:?}"
                ),
            }
            if f.len() < bytes.len() {
                assert!(check_frame(&bytes, &table, DecodeMode::Lenient).is_err());
            }
        }
        Err(e) => {
            assert!(
                check_frame(&bytes, &table, DecodeMode::Lenient).is_err(),
                "{e:?}"
            );
            if e.kind() == ErrorKind::Incomplete {
                assert!(bytes.len() < TcFrame::peek_length(&bytes).unwrap_or(usize::MAX));
            }
        }
    }
    // A strict decode of a frame of a configured channel re-encodes to the same octets (API-020).
    // The Err arms above fail on any rejection but an unknown channel, so "validates Ok" here
    // means "a configured Spacecraft ID and VC". A MAP ID the channels do not hold passes the
    // checks (TC-022, OI-98) and cannot be written (`UnknownChannel`), so it is left out (#433).
    if let Ok(f) = strict
        && check_frame(f.octets(), &table, DecodeMode::Lenient).is_ok()
        && f.segment_header().is_none_or(|sh| {
            let fields = f.header().fields();
            table
                .map(fields.spacecraft_id, fields.vc_id, sh.map_id)
                .is_some()
        })
    {
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
            let mut w = TcPlan::begin(
                &mut out,
                &table,
                f.header().fields(),
                f.segment_header(),
                data.len(),
            )
            .unwrap();
            w.data_mut().copy_from_slice(data);
            w.security_header_mut().copy_from_slice(f.security_header());
            w.security_trailer_mut()
                .copy_from_slice(f.security_trailer());
            let fields = f.header().fields();
            let ctx = FrameCtx::Tc {
                scid: fields.spacecraft_id,
                vcid: fields.vc_id,
                map_id: f.segment_header().map(|s| s.map_id),
            };
            w.protect(data.len(), &mut PreFilledSecurity, &ctx)
                .map(|w| w.finalize().len())
        };
        assert_eq!(n, Ok(f.len()));
        assert_eq!(out, f.octets());
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = run(&mut Unstructured::new(data));
});
