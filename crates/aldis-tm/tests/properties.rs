//! Property tests of `aldis-tm` (VER-023), drawing their inputs from the shared strategies of
//! `aldis-testkit` (VER-054). They are an integration test so that the strategies and the tests
//! see the same copy of the crate (M2 plan D6).
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "property helpers outside #[test] fail loudly (RS-018)"
)]

use aldis_core::security::{FrameCtx, NoSecurity};
use aldis_core::{DecodeMode, ErrorKind};
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::proptest::test_runner::TestCaseError;
use aldis_testkit::strategies::tm::{tm_frame, tm_frame_decoder_input};
use aldis_testkit::strategies::{self, octets};
use aldis_tm::{
    DataFieldStatus, TmFrame, TmFrameContent, TmFrameFields, TmFrameWriter, TmMasterChannelConfig,
    encode,
};

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/properties.txt"
);

/// A strict decode against the master channel re-encodes to exactly the octets decoded, and
/// lenient decodes it the same (API-020, API-010, GEN-020).
fn strict_re_encode_property(
    mc: &TmMasterChannelConfig,
    bytes: &[u8],
) -> Result<(), TestCaseError> {
    let Ok(f) = TmFrame::decode_with(bytes, mc.pc(), mc, DecodeMode::Strict) else {
        return Ok(());
    };
    if f.validate(mc).is_err() {
        return Ok(());
    }
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
    let n = encode(mc, &fields, &content, &mut out).unwrap();
    prop_assert_eq!(&out[..n], bytes);
    let lenient = TmFrame::decode_with(bytes, mc.pc(), mc, DecodeMode::Lenient).unwrap();
    prop_assert_eq!(lenient, f);
    Ok(())
}

/// Lenient accepts whatever strict accepts, and more only with an anomaly flag; strict and
/// lenient reject with the same error otherwise (API-030, API-032, GEN-020).
fn lenient_superset_property(
    mc: &TmMasterChannelConfig,
    bytes: &[u8],
) -> Result<(), TestCaseError> {
    let strict = TmFrame::decode_with(bytes, mc.pc(), mc, DecodeMode::Strict);
    let lenient = TmFrame::decode_with(bytes, mc.pc(), mc, DecodeMode::Lenient);
    match (strict, lenient) {
        (Ok(s), Ok(l)) => prop_assert_eq!(s, l),
        (Ok(_), Err(e)) => prop_assert!(false, "lenient rejected a strict frame: {e}"),
        (Err(s), Ok(l)) => {
            prop_assert!(matches!(
                s.kind(),
                ErrorKind::ReservedValue | ErrorKind::InvalidField
            ));
            prop_assert!(!l.anomalies().is_empty());
        }
        (Err(s), Err(l)) => prop_assert_eq!(s, l),
    }
    Ok(())
}

/// Every slice of a decoded view lies inside the input, in wire order, and the fields cover the
/// frame exactly (API-010, CMN-040).
fn slices_inside_property(mc: &TmMasterChannelConfig, bytes: &[u8]) -> Result<(), TestCaseError> {
    let Ok(f) = TmFrame::decode_unchecked(bytes, mc.pc(), mc, DecodeMode::Lenient) else {
        return Ok(());
    };
    prop_assert_eq!(f.len(), bytes.len());
    prop_assert_eq!(f.layout().frame_len(), bytes.len());
    prop_assert!(!f.data_field().is_empty());
    let mut at = 0;
    for range in f.layout().ranges() {
        prop_assert_eq!(range.start, at);
        at = range.end;
    }
    prop_assert_eq!(at, bytes.len());
    if let DataFieldStatus::Packets(aldis_tm::PacketPointer::StartsAt(n)) = f.data_field_status() {
        prop_assert!(usize::from(n.get()) < f.data_field().len());
    }
    Ok(())
}

proptest! {
    #![proptest_config(strategies::config(256, REGRESSIONS))]

    #[test]
    fn encode_then_decode_round_trips(f in tm_frame()) {
        // req: API-020, TM-009, TM-001, TM-004, TM-006, VER-023
        let d = TmFrame::decode_with(&f.octets, f.mc.pc(), &f.mc, DecodeMode::Strict).unwrap();
        prop_assert_eq!(d.vcid(), f.fields.vcid);
        prop_assert_eq!(d.scid(), f.mc.scid());
        prop_assert_eq!(d.mc_frame_count(), f.fields.mc_frame_count);
        prop_assert_eq!(d.vc_frame_count(), f.fields.vc_frame_count);
        prop_assert_eq!(d.data_field_status(), f.fields.status);
        prop_assert_eq!(
            d.secondary_header_data().unwrap_or_default(),
            f.secondary_header_data.as_slice()
        );
        prop_assert_eq!(d.security_header(), f.security_header.as_slice());
        prop_assert_eq!(d.data_field(), f.data.as_slice());
        prop_assert_eq!(d.security_trailer(), f.security_trailer.as_slice());
        prop_assert_eq!(d.ocf(), f.ocf);
        prop_assert!(d.anomalies().is_empty());
        prop_assert_eq!(d.validate(&f.mc), Ok(()));
        prop_assert_eq!(TmFrame::peek_length(&f.octets, f.mc.pc()), Ok(f.octets.len()));
        prop_assert_eq!(TmFrame::decode_unchecked(&f.octets, f.mc.pc(), &f.mc, DecodeMode::Strict), Ok(d));
    }

    #[test]
    fn two_phase_writer_matches_encode(f in tm_frame()) {
        // req: API-020, API-023, TM-009, VER-023
        let mut out = vec![0_u8; f.octets.len()];
        let mut w = TmFrameWriter::begin(&mut out, &f.mc, &f.fields).unwrap();
        w.secondary_header_data_mut().copy_from_slice(&f.secondary_header_data);
        w.security_header_mut().copy_from_slice(&f.security_header);
        w.data_field_mut().copy_from_slice(&f.data);
        w.security_trailer_mut().copy_from_slice(&f.security_trailer);
        if let Some(ocf) = f.ocf {
            w.set_ocf(ocf).unwrap();
        }
        let ctx = FrameCtx::Tm { scid: f.mc.scid(), vcid: f.fields.vcid };
        prop_assert_eq!(w.protect(&mut NoSecurity, &ctx).unwrap().finalize().len(), f.octets.len());
        prop_assert_eq!(out, f.octets);
    }

    #[test]
    fn rejected_encode_leaves_the_buffer(f in tm_frame(), short in 1_usize..9) {
        // req: API-022, GEN-042, VER-023
        let len = f.octets.len().saturating_sub(short);
        let mut out = vec![0xEE_u8; len];
        let e = encode(&f.mc, &f.fields, &f.content(), &mut out).unwrap_err();
        prop_assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        prop_assert!(out.iter().all(|&b| b == 0xEE));
    }

    #[test]
    fn decoders_never_panic((mc, bytes) in tm_frame_decoder_input()) {
        // req: GEN-020, API-010, API-030, API-032, VER-023
        strict_re_encode_property(&mc, &bytes)?;
        lenient_superset_property(&mc, &bytes)?;
        slices_inside_property(&mc, &bytes)?;
    }

    #[test]
    fn arbitrary_octets_never_panic((mc, _) in tm_frame_decoder_input(), bytes in octets(0..64)) {
        // req: GEN-020, VER-023
        for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
            let _ = TmFrame::decode_with(&bytes, mc.pc(), &mc, mode);
            let _ = TmFrame::decode(&bytes, mc.pc(), mode);
        }
        let _ = TmFrame::peek_length(&bytes, mc.pc());
    }
}
