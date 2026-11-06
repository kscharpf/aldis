//! The two sequences over whole periods (SYNC-005, SYNC-020): bit `i` of a buffer of zeros is
//! bit `i` of the sequence, so two periods and the 40 bits of the note show the period.

#![expect(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "test bookkeeping on lengths the test fixes (RS-018)"
)]

use aldis_sync::{RandomizerSequence, randomize};

const fn bit(buffer: &[u8], i: usize) -> u8 {
    (buffer[i / 8] >> (7 - i % 8)) & 1
}

#[test]
fn the_sequences_repeat_after_their_period_and_not_before() {
    // req: SYNC-005, SYNC-020
    for s in [
        RandomizerSequence::Period131071,
        RandomizerSequence::Period255,
    ] {
        let period = usize::try_from(s.period()).unwrap();
        let mut b = vec![0_u8; (2 * period + 40).div_ceil(8)];
        randomize(s, &mut b);
        for i in 0..(period + 40) {
            assert_eq!(bit(&b, i), bit(&b, i + period), "{s:?} bit {i}");
        }
        // The first 40 bits do not recur at any shorter distance.
        assert!((1..period).all(|p| (0..40).any(|i| bit(&b, i) != bit(&b, i + p))));
    }
}

#[test]
fn a_buffer_of_four_periods_wraps_at_a_bit_that_is_not_an_octet_boundary() {
    // req: SYNC-005, SYNC-004
    // 65536 octets are 524288 bits: four periods of 131071 bits and 4 bits.
    let mut long = vec![0_u8; 65536];
    randomize(RandomizerSequence::Period131071, &mut long);
    let mut short = vec![0_u8; 3];
    randomize(RandomizerSequence::Period131071, &mut short);
    // Bit 131071 of the long buffer is bit 0 of the sequence again.
    for i in 0..24 {
        assert_eq!(bit(&long, 131_071 + i), bit(&short, i));
    }
}
