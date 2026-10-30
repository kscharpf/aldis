//! Property tests of `aldis-tc` (VER-023), drawing their inputs from the shared strategies of
//! `aldis-testkit` (VER-054). They are an integration test so that the strategies and the tests
//! see the same copy of the crate (M2 plan D6).
#![expect(
    clippy::arithmetic_side_effects,
    reason = "test bookkeeping on lengths the strategies bound (RS-018)"
)]

use aldis_core::security::{FrameCtx, PreFilledSecurity};
use aldis_core::{DecodeMode, ErrorKind};
use aldis_tc::{
    Clcw, Delimited, FrameDelimiter, TcAnomaly, TcFrame, TcFrameWriter, encode, next_frame,
    validate_frame,
};
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::tc::{clcw, data_unit, tc_frame, tc_frame_decoder_input};
use aldis_testkit::strategies::{self, octets};

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/properties.txt"
);

proptest! {
    #![proptest_config(strategies::config(256, REGRESSIONS))]

    #[test]
    fn encode_then_decode_round_trips(f in tc_frame()) {
        // req: API-020, API-021, TC-001, TC-004, TC-005, TC-007, TC-008, VER-023
        let t = &f.channels.table;
        let d = TcFrame::decode(&f.octets, t, DecodeMode::Strict).unwrap();
        prop_assert_eq!(d.header().fields(), f.fields);
        prop_assert_eq!(d.segment_header(), f.segment_header);
        prop_assert_eq!(d.control_command(), f.control);
        prop_assert_eq!(d.len(), f.octets.len());
        prop_assert_eq!(d.header().frame_len(), f.octets.len());
        prop_assert_eq!(TcFrame::peek_length(&f.octets), Ok(f.octets.len()));
        if f.control.is_none() {
            prop_assert_eq!(d.data_field(), f.user_data.as_slice());
        }
        prop_assert!(d.security_header().iter().chain(d.security_trailer()).all(|&b| b == 0));
        prop_assert_eq!(d.header().primary_header_octets(), <[u8; 5]>::try_from(&f.octets[..5]).unwrap());
        // The same frame passes the Frame Validation Checks (TC-021, TC-023).
        prop_assert_eq!(validate_frame(&f.octets, t, DecodeMode::Strict).map(|v| v.len()), Ok(f.octets.len()));
    }

    #[test]
    fn writers_agree(f in tc_frame(), fill in any::<u8>()) {
        // req: API-020, API-022, API-023, GEN-014
        prop_assume!(f.control.is_none());
        let t = &f.channels.table;
        let mut two = vec![fill; f.octets.len()];
        let mut w = TcFrameWriter::begin(&mut two, t, f.fields, f.segment_header, f.user_data.len()).unwrap();
        w.user_data_mut().copy_from_slice(&f.user_data);
        let ctx = FrameCtx::Tc { scid: f.fields.spacecraft_id, vcid: f.fields.vc_id, map_id: None };
        let done = w.protect(f.user_data.len(), &mut PreFilledSecurity, &ctx).unwrap().finalize().unwrap();
        prop_assert_eq!(done.len(), f.octets.len());
        prop_assert_eq!(&two, &f.octets);
        // One octet short: nothing is written (API-022).
        let mut short = vec![fill; f.octets.len() - 1];
        let e = encode(t, f.fields, f.data(), &mut short).unwrap_err();
        prop_assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        prop_assert!(short.iter().all(|&b| b == fill));
    }

    #[test]
    fn decoder_never_panics((c, v) in tc_frame_decoder_input()) {
        // req: GEN-020, GEN-042, API-030, API-031, API-032, TC-001, VER-023
        let t = &c.table;
        let strict = TcFrame::decode(&v, t, DecodeMode::Strict);
        let lenient = TcFrame::decode(&v, t, DecodeMode::Lenient);
        if let Ok(s) = strict {
            // Strict ⊆ lenient, with the same view and no anomaly.
            prop_assert!(s.anomalies().is_empty());
            let l = lenient.unwrap();
            prop_assert_eq!(s.header(), l.header());
            prop_assert_eq!(s.data_field(), l.data_field());
        }
        if let Ok(f) = lenient {
            prop_assert!(f.len() <= v.len() && f.octets() == &v[..f.len()]);
            prop_assert_eq!(TcFrame::peek_length(&v), Ok(f.len()));
            // Exactly the frame passes validation in the same mode (TC-021 d).
            prop_assert!(validate_frame(f.octets(), t, DecodeMode::Lenient).is_ok());
            if !f.anomalies().contains(TcAnomaly::UnknownControlCommand) {
                prop_assert_eq!(f.control_command().is_some(), f.frame_type().control_command_flag());
            }
        }
        // Validation of exactly the input agrees with decoding when the input is one frame.
        match (validate_frame(&v, t, DecodeMode::Strict), TcFrame::decode(&v, t, DecodeMode::Strict)) {
            (Ok(a), Ok(b)) => prop_assert_eq!(a, b),
            (Err(r), Ok(b)) => prop_assert!(b.len() < v.len(), "{:?}", r),
            (Ok(_), Err(e)) => prop_assert!(false, "validated but not decoded: {:?}", e),
            (Err(_), Err(_)) => {}
        }
    }

    #[test]
    fn delimiter_returns_the_frames(u in data_unit()) {
        // req: TC-020, VER-023
        let items: Vec<Delimited<'_>> = FrameDelimiter::new(&u.octets).collect();
        let frames: Vec<&[u8]> = items
            .iter()
            .filter_map(|i| match i { Delimited::Frame(f) => Some(*f), _ => None })
            .collect();
        let expected: Vec<&[u8]> = u.frames.iter().map(Vec::as_slice).collect();
        prop_assert_eq!(frames, expected);
        prop_assert_eq!(items.iter().map(Delimited::len).sum::<usize>(), u.octets.len());
        prop_assert_eq!(items.len(), u.frames.len() + usize::from(!u.fill.is_empty()));
    }

    #[test]
    fn delimiter_covers_any_octets(unit in octets(0..80)) {
        // req: TC-020, GEN-020, GEN-022
        let mut rest = unit.as_slice();
        let mut taken = 0;
        while let Some(item) = next_frame(rest) {
            prop_assert!(!item.is_empty() && item.octets() == &rest[..item.len()]);
            if let Delimited::Frame(f) = item {
                // Version 00 and the length its Frame Length field declares (232.0 §4.1.2.7).
                prop_assert_eq!(f[0] >> 6, 0);
                prop_assert_eq!(usize::from(u16::from_be_bytes([f[2] & 3, f[3]])) + 1, f.len());
            }
            taken += item.len();
            rest = &rest[item.len()..];
        }
        prop_assert_eq!(taken, unit.len());
    }

    #[test]
    fn clcw_round_trips(c in clcw()) {
        // req: TC-030, TC-031, API-020, VER-023
        let (d, anomalies) = Clcw::decode(c.to_octets(), DecodeMode::Strict).unwrap();
        prop_assert_eq!(d, c);
        prop_assert!(anomalies.is_empty());
        prop_assert_eq!(c.to_word().to_be_bytes(), c.to_octets());
    }

    #[test]
    fn clcw_decodes_any_word(word in any::<[u8; 4]>()) {
        // req: TC-030, API-031, API-032, GEN-020
        let lenient = Clcw::decode(word, DecodeMode::Lenient);
        let strict = Clcw::decode(word, DecodeMode::Strict);
        match lenient {
            Ok((c, anomalies)) => {
                // The re-encoded word is the input with the spares cleared.
                let mut cleared = word;
                cleared[1] &= 0xFC;
                cleared[2] &= 0xFE;
                prop_assert_eq!(c.to_octets(), cleared);
                prop_assert_eq!(anomalies.is_empty(), cleared == word);
                prop_assert_eq!(strict.is_ok(), cleared == word);
            }
            Err(e) => {
                prop_assert!(word[0] & 0xE0 != 0, "{:?}", e);
                prop_assert_eq!(strict.unwrap_err(), e);
            }
        }
    }
}
