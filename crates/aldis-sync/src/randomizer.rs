//! The pseudo-randomizer of 131.0 §10 and its two sequences (SYNC-004, SYNC-005, SYNC-020).
//!
//! The randomizer exclusive-ORs the bits of a unit with a fixed pseudo-random sequence
//! (§10.2.1), the first bit of the unit with the first bit of the sequence (§10.3.2); within an
//! octet the first bit is the most significant. The sequence restarts at every unit
//! (§10.4.3), so [`randomize`] is a function of the buffer alone and is its own inverse
//! (§10.3.3). The caller randomizes the frame before it attaches the ASM, and derandomizes the
//! unit the search engine returns (SYNC-006).
//!
//! Both sequences are linear recurrences, so a register of the last 17 (or 8) bits generates
//! them, and the register runs on across the end of a period, where the sequence repeats.

use aldis_core::{Error, ErrorKind, Result, field};

/// The pseudo-random sequence the randomizer uses, chosen by the caller; there is no default
/// (SYNC-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RandomizerSequence {
    /// The sequence of §10.4.1: 131071 bits, h(x) = x¹⁷ + x¹⁴ + 1, started from
    /// `11000111000111000` (SYNC-005).
    Period131071,
    /// The legacy sequence of §10.4.2: 255 bits, h(x) = x⁸ + x⁷ + x⁵ + x³ + 1, started from
    /// all ones (SYNC-020).
    Period255,
}

/// The seed of the 131071-bit sequence: bit `i` is a(i), the digits `11000111000111000` read
/// from the right (SYNC-005).
const SEED_131071: u32 = 0b1_1000_1110_0011_1000;
/// The seed of the 255-bit sequence: b(0)…b(7) all ones (SYNC-020).
const SEED_255: u32 = 0xFF;

impl RandomizerSequence {
    /// The sequence of the given period in bits, 131071 or 255: the choice a C ABI caller
    /// passes (SYNC-020).
    ///
    /// # Errors
    /// `InvalidConfig` naming `randomizer_sequence` for any other value (SYNC-020).
    pub const fn from_period(bits: u32) -> Result<RandomizerSequence> {
        match bits {
            131_071 => Ok(RandomizerSequence::Period131071),
            255 => Ok(RandomizerSequence::Period255),
            _ => Err(Error::new(ErrorKind::InvalidConfig).with_field(field::RANDOMIZER_SEQUENCE)),
        }
    }

    /// The length of the sequence in bits (131.0 §10.4.1, §10.4.2; SYNC-005, SYNC-020).
    #[must_use]
    pub const fn period(self) -> u32 {
        match self {
            RandomizerSequence::Period131071 => 131_071,
            RandomizerSequence::Period255 => 255,
        }
    }

    /// The register at the start of a unit: bit 0 is the oldest bit, the next to leave.
    const fn seed(self) -> u32 {
        match self {
            RandomizerSequence::Period131071 => SEED_131071,
            RandomizerSequence::Period255 => SEED_255,
        }
    }

    /// The bit that enters the register, from the bits in it (SYNC-005, SYNC-020).
    ///
    /// For 131071 the register holds a(n−17)…a(n−1) at bits 0…16, and a(n) = a(n−3) ⊕ a(n−17).
    /// For 255 it holds b(n−8)…b(n−1) at bits 0…7, and
    /// b(n) = b(n−1) ⊕ b(n−3) ⊕ b(n−5) ⊕ b(n−8).
    const fn feedback(self, reg: u32) -> u32 {
        match self {
            RandomizerSequence::Period131071 => (reg ^ (reg >> 14)) & 1,
            RandomizerSequence::Period255 => (reg ^ (reg >> 3) ^ (reg >> 5) ^ (reg >> 7)) & 1,
        }
    }

    /// The width of the register in bits.
    const fn width(self) -> u32 {
        match self {
            RandomizerSequence::Period131071 => 17,
            RandomizerSequence::Period255 => 8,
        }
    }
}

/// Exclusive-ORs `buffer` in place, bit for bit, with `sequence`, the first bit of the buffer
/// with the first bit of the sequence (131.0 §10.2.1, §10.3.2–§10.3.3, §10.4.3; SYNC-004).
///
/// One function serves both directions: applying it twice restores the buffer. It accepts a
/// buffer of any length, including 0, and does not fail. The sequence restarts at the first
/// octet of every call; a buffer longer than a period runs on into the next period
/// (SYNC-005).
pub fn randomize(sequence: RandomizerSequence, buffer: &mut [u8]) {
    let mut reg = sequence.seed();
    let top = sequence.width().saturating_sub(1);
    for octet in buffer {
        let mut mask = 0_u8;
        for _ in 0..8 {
            // The oldest bit of the register is the next bit of the sequence.
            mask = (mask << 1) | u8::from(reg & 1 == 1);
            reg = (reg >> 1) | sequence.feedback(reg).wrapping_shl(top);
        }
        *octet ^= mask;
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{RandomizerSequence, randomize};
    use aldis_core::{ErrorKind, field};

    const SEQUENCES: [RandomizerSequence; 2] = [
        RandomizerSequence::Period131071,
        RandomizerSequence::Period255,
    ];

    /// The first 40 bits of the sequence as five zero octets give them (§10.4.3 note 2).
    fn first_40(sequence: RandomizerSequence) -> [u8; 5] {
        let mut b = [0; 5];
        randomize(sequence, &mut b);
        b
    }

    #[test]
    fn the_131071_sequence_starts_as_the_note_says() {
        // req: SYNC-005
        // 0001 1100 0111 0001 1011 1001 0001 1011 1010 1001
        assert_eq!(
            first_40(RandomizerSequence::Period131071),
            [0x1C, 0x71, 0xB9, 0x1B, 0xA9]
        );
    }

    #[test]
    fn the_255_sequence_starts_with_ones() {
        // req: SYNC-020
        assert_eq!(
            first_40(RandomizerSequence::Period255),
            [0xFF, 0x48, 0x0E, 0xC0, 0x9A]
        );
    }

    #[test]
    fn applying_it_twice_restores_the_buffer() {
        // req: SYNC-004
        for s in SEQUENCES {
            let mut original = [0_u8; 300];
            for (i, o) in original.iter_mut().enumerate() {
                *o = i.to_le_bytes()[0] ^ 0x5A;
            }
            let mut b = original;
            randomize(s, &mut b);
            assert_ne!(b, original);
            randomize(s, &mut b);
            assert_eq!(b, original);
            let mut empty: [u8; 0] = [];
            randomize(s, &mut empty);
        }
    }

    #[test]
    fn a_call_depends_on_its_buffer_alone() {
        // req: SYNC-004
        let mut a = [0_u8; 6];
        randomize(RandomizerSequence::Period131071, &mut a);
        let mut b = [0_u8; 6];
        randomize(RandomizerSequence::Period131071, &mut b[..3]);
        randomize(RandomizerSequence::Period131071, &mut b[3..]);
        assert_eq!(a[..3], b[..3]);
        assert_eq!(b[3..], a[..3], "the sequence restarts at every call");
    }

    #[test]
    fn the_choice_is_a_period_in_bits() {
        // req: SYNC-020
        assert_eq!(
            RandomizerSequence::from_period(131_071),
            Ok(RandomizerSequence::Period131071)
        );
        assert_eq!(
            RandomizerSequence::from_period(255),
            Ok(RandomizerSequence::Period255)
        );
        // The period of each choice is the number the caller passes to choose it.
        assert_eq!(RandomizerSequence::Period131071.period(), 131_071);
        assert_eq!(RandomizerSequence::Period255.period(), 255);
        for bad in [0, 1, 254, 256, 131_070, u32::MAX] {
            let e = RandomizerSequence::from_period(bad).unwrap_err();
            assert_eq!(e.kind(), ErrorKind::InvalidConfig);
            assert_eq!(e.field(), Some(field::RANDOMIZER_SEQUENCE));
        }
    }
}
