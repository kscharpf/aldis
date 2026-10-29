//! Property tests of `aldis-uslp` (VER-023), drawing their inputs from the shared strategies of
//! `aldis-testkit` (VER-054). They are an integration test so that the strategies and the tests
//! see the same copy of the crate (M2 plan D6).
#![expect(
    clippy::arithmetic_side_effects,
    reason = "test bookkeeping on lengths the strategies bound (RS-018)"
)]

use aldis_core::oid::OidGenerator;
use aldis_core::{DecodeMode, ErrorKind};
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::uslp::{
    FrameParts, data_unit, uslp_channels, uslp_frame, uslp_frame_decoder_input,
};
use aldis_testkit::strategies::{self, octets};
use aldis_uslp::{
    Delimited, FrameContent, FrameDelimiter, UslpAnomaly, UslpFrame, UslpFrameWriter, encode,
    encode_oid, next_frame, validate_frame,
};

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/properties.txt"
);

proptest! {
    #![proptest_config(strategies::config(256, REGRESSIONS))]

    #[test]
    fn encode_then_decode_round_trips(f in uslp_frame()) {
        // req: API-020, API-021, USLP-002, USLP-003, USLP-004, USLP-005, USLP-009, USLP-010, VER-023
        let t = &f.channels.table;
        let d = UslpFrame::decode(&f.octets, t, DecodeMode::Strict).unwrap();
        prop_assert_eq!(d.len(), f.octets.len());
        prop_assert_eq!(d.tfdz(), f.tfdz.as_slice());
        prop_assert_eq!(UslpFrame::peek_length(&f.octets, t), Ok(f.octets.len()));
        prop_assert!(d.security_header().iter().chain(d.security_trailer()).all(|&b| b == 0));
        prop_assert!(d.anomalies().is_empty());
        match &f.parts {
            FrameParts::Full { fields, tfdf, insert_zone, ocf } => {
                let full = d.full().unwrap();
                let h = full.header();
                prop_assert_eq!((h.spacecraft_id(), h.vc_id(), h.map_id()), (fields.spacecraft_id, fields.vc_id, fields.map_id));
                prop_assert_eq!((h.kind(), h.vcf_count().value()), (fields.kind, fields.vcf_count));
                prop_assert_eq!(h.source_or_destination(), fields.source_or_destination);
                prop_assert_eq!(full.tfdf_header(), *tfdf);
                prop_assert_eq!(full.insert_zone(), insert_zone.as_slice());
                prop_assert_eq!(full.ocf(), *ocf);
                prop_assert_eq!(&h.octets()[..h.len()], &f.octets[..h.len()]);
            }
            FrameParts::Truncated { channel, tfdf } => {
                prop_assert_eq!(d.channel(), *channel);
                prop_assert_eq!(d.tfdf_header(), *tfdf);
                prop_assert_eq!(&d.truncated().unwrap().header_octets()[..], &f.octets[..4]);
            }
            FrameParts::Oid { fields, skip, .. } => {
                prop_assert_eq!(d.vc_id(), fields.vc_id);
                let mut g = OidGenerator::new();
                let mut expect = vec![0_u8; skip + f.tfdz.len()];
                g.fill(&mut expect);
                prop_assert_eq!(&expect[*skip..], f.tfdz.as_slice());
            }
        }
        // The same frame passes the USLP Frame Validation Checks (USLP-014).
        prop_assert_eq!(validate_frame(&f.octets, t, DecodeMode::Strict).map(|v| v.len()), Ok(f.octets.len()));
    }

    #[test]
    fn writers_agree(f in uslp_frame(), fill in any::<u8>()) {
        // req: API-020, API-022, API-023, GEN-014, GEN-042
        let t = &f.channels.table;
        match &f.parts {
            FrameParts::Full { fields, tfdf, insert_zone, ocf } => {
                let content = FrameContent { tfdf: *tfdf, insert_zone, ocf: *ocf };
                let mut two = vec![fill; f.octets.len()];
                let mut w = UslpFrameWriter::begin(&mut two, t, *fields, content, f.tfdz.len()).unwrap();
                w.tfdz_mut().unwrap().copy_from_slice(&f.tfdz);
                prop_assert_eq!(w.finish(f.tfdz.len()), Ok(f.octets.len()));
                prop_assert_eq!(&two, &f.octets);
                // One octet short: nothing is written (API-022).
                let mut short = vec![fill; f.octets.len() - 1];
                let e = encode(t, *fields, content, &f.tfdz, &mut short).unwrap_err();
                prop_assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
                prop_assert!(short.iter().all(|&b| b == fill));
            }
            FrameParts::Oid { fields, insert_zone, ocf, .. } => {
                // A rejected OID frame leaves the generator and the buffer unchanged (GEN-042).
                let mut g = OidGenerator::new();
                let mut short = vec![fill; f.octets.len() - 1];
                let e = encode_oid(t, *fields, insert_zone, *ocf, &mut g, &mut short).unwrap_err();
                prop_assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
                prop_assert_eq!(g, OidGenerator::new());
                prop_assert!(short.iter().all(|&b| b == fill));
            }
            FrameParts::Truncated { .. } => {}
        }
    }

    #[test]
    fn decoder_never_panics((c, v) in uslp_frame_decoder_input()) {
        // req: GEN-020, GEN-042, API-030, API-031, API-032, USLP-001, CMN-040, VER-023
        let t = &c.table;
        let strict = UslpFrame::decode(&v, t, DecodeMode::Strict);
        let lenient = UslpFrame::decode(&v, t, DecodeMode::Lenient);
        if let Ok(s) = &strict {
            // Strict ⊆ lenient, with the same view and no anomaly.
            prop_assert!(s.anomalies().is_empty());
            let l = lenient.as_ref().unwrap();
            prop_assert_eq!(s.octets(), l.octets());
            prop_assert_eq!(s.tfdz(), l.tfdz());
        }
        if let Ok(f) = &lenient {
            prop_assert!(f.len() <= v.len() && f.octets() == &v[..f.len()]);
            // The layout covers the frame exactly and the fields lie where it says (CMN-040).
            prop_assert_eq!(f.layout().frame_len(), f.len());
            prop_assert_eq!(&f.octets()[f.layout().security_header()], f.security_header());
            prop_assert_eq!(&f.octets()[f.layout().security_trailer()], f.security_trailer());
            prop_assert!(f.layout().data().len() > f.tfdz().len());
            prop_assert_eq!(UslpFrame::peek_length(&v, t), Ok(f.len()));
            prop_assert!(validate_frame(f.octets(), t, DecodeMode::Lenient).is_ok());
            if f.anomalies().contains(UslpAnomaly::OidHeaderMismatch) {
                prop_assert_eq!(f.vc_id().get(), 63);
            }
        }
        match (validate_frame(&v, t, DecodeMode::Strict), UslpFrame::decode(&v, t, DecodeMode::Strict)) {
            (Ok(a), Ok(b)) => prop_assert_eq!(a, b),
            (Err(r), Ok(b)) => prop_assert!(b.len() < v.len(), "{:?}", r),
            (Ok(_), Err(e)) => prop_assert!(false, "validated but not decoded: {:?}", e),
            (Err(_), Err(_)) => {}
        }
    }

    #[test]
    fn delimiter_returns_the_frames(u in data_unit()) {
        // req: USLP-015, VER-023
        let t = &u.channels.table;
        let items: Vec<Delimited<'_>> = FrameDelimiter::new(&u.octets, t).collect();
        let frames: Vec<&[u8]> = items
            .iter()
            .filter_map(|i| match i { Delimited::Frame(f) => Some(*f), _ => None })
            .collect();
        let expected: Vec<&[u8]> = u.frames.iter().map(Vec::as_slice).collect();
        // The fill can, rarely, start a valid header; the frames before it are all found.
        prop_assert!(frames.len() >= expected.len());
        prop_assert_eq!(&frames[..expected.len()], &expected[..]);
        prop_assert_eq!(items.iter().map(Delimited::len).sum::<usize>(), u.octets.len());
    }

    #[test]
    fn delimiter_covers_any_octets(c in uslp_channels(), unit in octets(0..80)) {
        // req: USLP-015, GEN-020, GEN-022
        let mut rest = unit.as_slice();
        let mut taken = 0;
        while let Some(item) = next_frame(rest, &c.table) {
            prop_assert!(!item.is_empty() && item.octets() == &rest[..item.len()]);
            if let Delimited::Frame(f) = item {
                prop_assert_eq!(f[0] >> 4, 0b1100);
            }
            taken += item.len();
            rest = &rest[item.len()..];
        }
        prop_assert_eq!(taken, unit.len());
    }
}
