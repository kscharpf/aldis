//! Property tests of `aldis-spp` (VER-023), drawing their inputs from the shared strategies of
//! `aldis-testkit` (VER-054). They are an integration test so that the strategies and the tests
//! see the same copy of the crate (M2 plan D6).
#![expect(
    clippy::arithmetic_side_effects,
    reason = "test bookkeeping on small bounded counts (RS-018)"
)]

use aldis_core::ErrorKind;
use aldis_core::{DecodeMode, ids::SeqCount};
use aldis_spp::{
    Outcome, Reassembler, Segmenter, SequenceCounter, SequenceTracker, SpacePacket, SppConfig,
    Status, Tracked, encode,
};
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::spp::{
    segmentation, space_packet, space_packet_decoder_input, tracker_events,
};
use aldis_testkit::strategies::{self};

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/properties.txt"
);

proptest! {
    #![proptest_config(strategies::config(256, REGRESSIONS))]

    #[test]
    fn encode_then_decode_round_trips(p in space_packet()) {
        // req: API-020, API-021, SPP-002, SPP-005, VER-023
        let mut out = [0_u8; 64];
        let n = encode(&p.data(), &mut out).unwrap();
        let d = SpacePacket::decode(&out[..n], DecodeMode::Strict).unwrap();
        prop_assert_eq!(d.header().fields(), p.fields);
        prop_assert_eq!(d.has_secondary_header(), !p.secondary_header.is_empty());
        let data = p.data_field();
        prop_assert_eq!(d.data_field(), data.as_slice());
        prop_assert_eq!(d.header().octets(), <[u8; 6]>::try_from(&out[..6]).unwrap());
    }

    #[test]
    fn tracker_matches_a_model(events in tracker_events(4, 60)) {
        // req: SPP-011, CMN-031, VER-023
        let mut t = SequenceTracker::<3>::new();
        let mut model: [Option<SeqCount>; 4] = [None; 4];
        for e in events {
            let slot = usize::from(e.apid.get());
            if e.forget {
                prop_assert_eq!(t.forget(e.apid), model[slot].take().is_some());
                continue;
            }
            let r = t.track(e.apid, e.count);
            match model[slot] {
                Some(prev) => {
                    prop_assert_eq!(r, Ok(Tracked::Next(prev.classify(e.count))));
                    model[slot] = Some(e.count);
                }
                None if model.iter().flatten().count() == 3 => {
                    prop_assert_eq!(r.unwrap_err().kind(), ErrorKind::CapacityExceeded);
                }
                None => {
                    prop_assert_eq!(r, Ok(Tracked::First));
                    model[slot] = Some(e.count);
                }
            }
            prop_assert_eq!(t.len(), model.iter().flatten().count());
        }
    }

    #[test]
    fn segment_then_reassemble_round_trips(s in segmentation()) {
        // req: SPP-013, VER-023
        let cfg = SppConfig::DEFAULT;
        let (sdu, sec, max_user) = (&s.sdu, &s.secondary_header, s.max_user);
        let mut seg = Segmenter::new(&cfg, s.packet_type, s.apid, sdu, max_user).unwrap();
        let mut counter = SequenceCounter::new(s.start);
        let mut buf = vec![0_u8; sdu.len()];
        let mut ra = Reassembler::new(s.apid, &mut buf);
        let mut out = [0_u8; 64];
        let mut last = None;
        let mut packets = 0;
        while let Some(n) = seg.next_packet(&mut counter, sec, &mut out).unwrap() {
            let p = SpacePacket::decode(&out[..n], DecodeMode::Strict).unwrap();
            let user = p.data_field().get(sec.len()..).unwrap();
            prop_assert!(!user.is_empty() && user.len() <= max_user);
            last = Some(ra.push(&p.header(), user).unwrap());
            packets += 1;
        }
        prop_assert_eq!(packets, sdu.len().div_ceil(max_user));
        prop_assert_eq!(last, Some(Outcome { discarded: None, status: Status::Complete(sdu.len()) }));
        prop_assert_eq!(ra.sdu(), sdu.as_slice());
    }

    #[test]
    fn a_rejected_segment_changes_nothing(s in segmentation(), fill in any::<u8>()) {
        // req: SPP-013, API-022
        let cfg = SppConfig::DEFAULT;
        let sec = s.secondary_header.as_slice();
        let mut seg = Segmenter::new(&cfg, s.packet_type, s.apid, &s.sdu, s.max_user).unwrap();
        let mut counter = SequenceCounter::new(s.start);
        while !seg.is_done() {
            // One octet short of the next segment: the primary header, the secondary header,
            // and the next piece of user data.
            let len = 6 + sec.len() + seg.remaining().min(s.max_user);
            let mut short = vec![fill; len - 1];
            let (before, count) = (seg, counter.peek());
            let e = seg.next_packet(&mut counter, sec, &mut short).unwrap_err();
            prop_assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
            prop_assert!(short.iter().all(|&b| b == fill), "no partial segment written");
            prop_assert_eq!((seg, counter.peek()), (before, count));
            let mut out = vec![0_u8; len];
            prop_assert_eq!(seg.next_packet(&mut counter, sec, &mut out), Ok(Some(len)));
        }
    }
}

proptest! {
    #![proptest_config(strategies::config(1024, REGRESSIONS))]

    #[test]
    fn lenient_accepts_whatever_strict_accepts(bytes in space_packet_decoder_input()) {
        // req: GEN-020, API-010, API-031, SPP-003
        let strict = SpacePacket::decode(&bytes, DecodeMode::Strict);
        let lenient = SpacePacket::decode(&bytes, DecodeMode::Lenient);
        if let Ok(s) = strict {
            let l = lenient.unwrap();
            prop_assert_eq!((l.header(), l.data_field(), l.anomalies()), (s.header(), s.data_field(), s.anomalies()));
            prop_assert!(s.anomalies().is_empty());
        }
        if let Ok(l) = lenient {
            prop_assert_eq!(SpacePacket::peek_length(&bytes), Ok(l.len()));
            prop_assert!(l.len() <= bytes.len());
        }
    }
}
