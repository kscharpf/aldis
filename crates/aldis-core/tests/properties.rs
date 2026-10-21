//! Property tests of `aldis-core` (VER-023), drawing their inputs from the shared strategies of
//! `aldis-testkit` (VER-054). They are an integration test so that the strategies and the tests
//! see the same copy of the crate (M2 plan D6).
#![expect(
    clippy::arithmetic_side_effects,
    reason = "test models on small bounded values and a u128 modular model (RS-018)"
)]

use std::collections::VecDeque;

use aldis_core::bits::{get, set};
use aldis_core::container::{ByteQueue, FixedQueue};
use aldis_core::counter::Continuity;
use aldis_core::crc::{syndrome, verify_fecf, write_fecf};
use aldis_core::error::ErrorKind;
use aldis_core::layout::FrameLayout;
use aldis_core::ocf::{FsrReport, Ocf, ProjectSpecificReport, Type1Report};
use aldis_testkit::strategies::common::{
    QueueOp, bit_field_access, byte_queue_ops, fixed_queue_ops, layout_spec, mod_counter_and_value,
};
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::{self, octets};

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/properties.txt"
);

proptest! {
    #![proptest_config(strategies::config(256, REGRESSIONS))]

    #[test]
    fn set_then_get_round_trips_and_preserves_other_bits(access in bit_field_access()) {
        // req: GEN-050, GEN-051
        let (buf, offset, width, value) = (access.buf, access.offset, access.width, access.value);
        let mut out = buf.clone();
        if set(&mut out, offset, width, value).is_ok() {
            prop_assert_eq!(get(&out, offset, width), Ok(value));
            for bit in 0..buf.len() * 8 {
                if bit < offset || bit >= offset + usize::from(width) {
                    prop_assert_eq!(get(&out, bit, 1), get(&buf, bit, 1));
                }
            }
        } else {
            prop_assert_eq!(out, buf);
        }
    }

    #[test]
    fn byte_queue_matches_model(ops in byte_queue_ops(20, 200)) {
        // req: GEN-010, API-061
        let mut q: ByteQueue<37> = ByteQueue::new();
        let mut model: VecDeque<Vec<u8>> = VecDeque::new();
        let mut used = 0;
        for op in ops {
            match op {
                QueueOp::Push(data) => {
                    let need = data.len() + 4;
                    let r = q.push(&data);
                    if need > 37 {
                        prop_assert_eq!(r.unwrap_err().kind(), ErrorKind::OutOfRange);
                    } else if used + need > 37 {
                        prop_assert_eq!(r.unwrap_err().kind(), ErrorKind::WouldBlock);
                    } else {
                        prop_assert!(r.is_ok());
                        used += need;
                        model.push_back(data);
                    }
                }
                QueueOp::Pop => {
                    let mut out = [0_u8; 40];
                    let got = q.pop_into(&mut out).unwrap();
                    let expected = model.pop_front();
                    prop_assert_eq!(got.map(|n| out[..n].to_vec()), expected.clone());
                    used -= expected.map_or(0, |e| e.len() + 4);
                }
                QueueOp::Discard => {
                    let expected = model.pop_front();
                    prop_assert_eq!(q.discard(), expected.as_ref().map(Vec::len));
                    used -= expected.map_or(0, |e| e.len() + 4);
                }
            }
            prop_assert_eq!(q.records(), model.len());
            prop_assert_eq!(q.free(), 37 - used);
        }
    }

    #[test]
    fn fixed_queue_matches_model(ops in fixed_queue_ops(200)) {
        // req: GEN-010, API-051
        let mut q: FixedQueue<u8, 5> = FixedQueue::new();
        let mut model: VecDeque<u8> = VecDeque::new();
        for op in ops {
            match op {
                Some(v) if model.len() < 5 => { prop_assert_eq!(q.push(v), Ok(())); model.push_back(v); }
                Some(v) => prop_assert_eq!(q.push(v), Err(v)),
                None => prop_assert_eq!(q.pop(), model.pop_front()),
            }
            prop_assert_eq!(q.len(), model.len());
            prop_assert_eq!(q.peek(), model.front());
        }
    }

    #[test]
    fn wider_counters_match_u128_model((c, next) in mod_counter_and_value()) {
        // req: CMN-030, CMN-031, GEN-024, VER-023
        let modulus = 1_u128 << c.bits();
        let prev = c.value();
        prop_assert_eq!(u128::from(c.next().value()), (u128::from(prev) + 1) % modulus);
        let dist = (u128::from(next) + modulus - u128::from(prev)) % modulus;
        prop_assert_eq!(u128::from(c.distance_to(next).unwrap()), dist);
        let expected = match dist {
            0 => Continuity::Repeat,
            1 => Continuity::InSequence,
            d if d <= modulus / 2 => Continuity::Gap {
                missing: u64::try_from(d - 1).unwrap(),
            },
            _ => Continuity::Backwards,
        };
        prop_assert_eq!(c.classify(next), Ok(expected));
    }

    #[test]
    fn both_verification_methods_agree(data in octets(0..300), flip in any::<u16>()) {
        // req: CMN-003
        let mut frame = data;
        frame.extend_from_slice(&[0, 0]);
        write_fecf(&mut frame).unwrap();
        prop_assert_eq!(syndrome(&frame), 0);
        prop_assert!(verify_fecf(&frame).is_ok());
        let bit = usize::from(flip) % (frame.len() * 8);
        frame[bit / 8] ^= 0x80 >> (bit % 8);
        prop_assert_eq!(verify_fecf(&frame).is_ok(), syndrome(&frame) == 0);
    }

    #[cfg(feature = "crc-table")]
    #[test]
    fn table_equals_bitwise(data in octets(0..300)) {
        // req: CMN-004
        prop_assert_eq!(aldis_core::crc::crc16_table(&data), aldis_core::crc::crc16_bitwise(&data));
    }

    #[test]
    fn ranges_are_contiguous_and_cover_the_frame(spec in layout_spec()) {
        // req: CMN-040, CMN-041
        let fixed = spec.primary_header_len
            + spec.secondary_len
            + spec.security.header_len()
            + spec.security.trailer_len()
            + if spec.ocf { 4 } else { 0 }
            + if spec.fecf { 2 } else { 0 };
        let frame_len = spec.frame_len;
        match FrameLayout::compute(&spec) {
            Ok(l) => {
                let r = l.ranges();
                prop_assert_eq!(r[0].start, 0);
                for w in r.windows(2) { prop_assert_eq!(w[0].end, w[1].start); }
                prop_assert_eq!(r[6].end, frame_len);
                prop_assert_eq!(l.data().len(), frame_len - fixed);
                prop_assert_eq!(l.frame_len(), frame_len);
                prop_assert!(!l.data().is_empty());
            }
            Err(e) => {
                prop_assert_eq!(e.kind(), ErrorKind::InvalidConfig);
                prop_assert!(frame_len <= fixed);
            }
        }
    }
}

proptest! {
    #![proptest_config(strategies::config(256, REGRESSIONS))]

    #[test]
    fn ocf_classification_matches_the_flag_bits(octets in any::<[u8; 4]>()) {
        // req: CMN-010
        let ocf = Ocf::decode(octets);
        prop_assert_eq!(ocf.to_octets(), octets);
        let (type_flag, use_bit) = (octets[0] >> 7, (octets[0] >> 6) & 1);
        let expected = match (type_flag, use_bit) {
            (0, _) => Ocf::Clcw(Type1Report::new(octets).unwrap()),
            (_, 0) => Ocf::ProjectSpecific(ProjectSpecificReport::new(octets).unwrap()),
            _ => Ocf::SdlsFsr(FsrReport::new(octets).unwrap()),
        };
        prop_assert_eq!(ocf, expected);
        // Exactly one report type accepts the octets.
        let accepted = [
            Type1Report::new(octets).is_some(),
            ProjectSpecificReport::new(octets).is_some(),
            FsrReport::new(octets).is_some(),
        ];
        prop_assert_eq!(accepted.iter().filter(|&&a| a).count(), 1);
    }
}
