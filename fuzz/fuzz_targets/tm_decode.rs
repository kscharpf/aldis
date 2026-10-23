//! `TmFrame::decode_with`, `decode_unchecked`, and `peek_length` on a master channel and frame
//! octets drawn from the input, in both modes: no panic, every field inside the frame and the
//! fields covering it in wire order, strict ⊆ lenient, and a strict frame that matches its
//! master channel re-encodes to the same octets (GEN-020, API-010, API-020, API-030, CMN-040).
#![no_main]

// req: GEN-020, API-010, API-012, API-013, API-020, API-030, API-032, TM-001, TM-005, TM-008, TM-011, VER-022

use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::security::SecurityLayout;
use aldis_core::{DecodeMode, ErrorKind};
use aldis_tm::{
    DataFieldContent, DataFieldStatus, FshLen, PacketPointer, TmFrame, TmFrameContent,
    TmFrameFields, TmMasterChannelConfig, TmPhysicalChannelConfig, TmVirtualChannelConfig, encode,
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

fn run(u: &mut Unstructured<'_>) -> Result<()> {
    let Some(mc) = master_channel(u)? else {
        return Ok(());
    };
    let pc = mc.pc();
    let len = if u.ratio(7, 8)? {
        pc.frame_len()
    } else {
        u.int_in_range(0..=320)?
    };
    let bytes = u.bytes(len.min(u.len()))?;
    let strict = TmFrame::decode_with(bytes, pc, &mc, DecodeMode::Strict);
    let lenient = TmFrame::decode_with(bytes, pc, &mc, DecodeMode::Lenient);
    match (&strict, &lenient) {
        (Ok(s), Ok(l)) => assert_eq!(s, l),
        (Ok(_), Err(e)) => panic!("lenient rejected a strict frame: {e}"),
        (Err(s), Ok(l)) => {
            assert!(matches!(
                s.kind(),
                ErrorKind::ReservedValue | ErrorKind::InvalidField
            ));
            assert!(!l.anomalies().is_empty());
        }
        (Err(s), Err(l)) => assert_eq!(s, l),
    }
    let peek = TmFrame::peek_length(bytes, pc);
    if let Ok(f) = &lenient {
        assert_eq!(peek, Ok(f.len()));
        assert_eq!(
            TmFrame::decode_unchecked(bytes, pc, &mc, DecodeMode::Lenient).as_ref(),
            Ok(f)
        );
        let mut at = 0;
        for range in f.layout().ranges() {
            assert_eq!(range.start, at);
            at = range.end;
        }
        assert_eq!(at, bytes.len());
        assert_ne!(f.data_field().len(), 0);
        if let DataFieldStatus::Packets(PacketPointer::StartsAt(n)) = f.data_field_status() {
            assert!(usize::from(n) < f.data_field().len());
        }
    }
    if let Ok(f) = strict
        && f.validate(&mc).is_ok()
    {
        assert!(f.anomalies().is_empty());
        let fields = TmFrameFields::new(
            f.vcid(),
            f.mc_frame_count(),
            f.vc_frame_count(),
            f.data_field_status(),
        );
        let content = TmFrameContent {
            secondary_header_data: f.secondary_header_data().unwrap_or_default(),
            security_header: f.security_header(),
            data: f.data_field(),
            security_trailer: f.security_trailer(),
            ocf: f.ocf(),
        };
        let mut out = vec![0_u8; bytes.len()];
        assert_eq!(encode(&mc, &fields, &content, &mut out), Ok(bytes.len()));
        assert_eq!(out, bytes);
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = run(&mut Unstructured::new(data));
});
