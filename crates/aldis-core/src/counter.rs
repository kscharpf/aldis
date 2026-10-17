//! Modular frame and sequence counters with gap classification (CMN-030, CMN-031, GEN-024).
//!
//! Used for TM MC/VC frame counts (8 bits, 132.0 §4.1.2.5–6), AOS VC frame counts (24 bits,
//! or 28 with the cycle field, 732.0 §4.1.2.4–5), USLP VCF counts (8–56 bits, 732.1
//! §4.1.2.12), and Space Packet sequence counts (14 bits, 133.0 §4.1.3.4.3). The width is a
//! run-time value because USLP signals the count length in every frame header.

use crate::error::{Error, Result, u8_to_u64};

/// Continuity of a received count relative to the previous one (CMN-031).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Continuity {
    /// Forward distance 1.
    InSequence,
    /// Forward distance 2 … modulus/2; `missing` = distance − 1 counts were skipped.
    Gap {
        /// Number of counts skipped.
        missing: u64,
    },
    /// Forward distance 0: the same count again.
    Repeat,
    /// Any other distance: the counter went backwards or was reset.
    Backwards,
}

/// A counter value of 1–64 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModCounter {
    bits: u8,
    value: u64,
}

const fn mask(bits: u8) -> u64 {
    u64::MAX >> (64_u8.wrapping_sub(bits))
}

/// Constructs a counter from already-validated `bits` (1–64) and `value` (`<= mask(bits)`).
pub(crate) const fn from_valid(bits: u8, value: u64) -> ModCounter {
    ModCounter { bits, value }
}

/// Classifies `next` as received after `prev` (both masked to `bits`, 1–64; CMN-031).
pub(crate) const fn classify_masked(bits: u8, prev: u64, next: u64) -> Continuity {
    let d = next.wrapping_sub(prev) & mask(bits);
    let half = 1_u64 << (bits.wrapping_sub(1));
    match d {
        0 => Continuity::Repeat,
        1 => Continuity::InSequence,
        d if d <= half => Continuity::Gap {
            missing: d.wrapping_sub(1),
        },
        _ => Continuity::Backwards,
    }
}

impl ModCounter {
    /// A counter of `bits` width holding `value`.
    ///
    /// # Errors
    /// `OutOfRange` if `bits` is not 1–64 or `value` needs more than `bits` bits.
    pub fn new(bits: u8, value: u64) -> Result<ModCounter> {
        if bits == 0 {
            return Err(Error::out_of_range("bits", 1, 0));
        }
        if bits > 64 {
            return Err(Error::out_of_range("bits", 64, u8_to_u64(bits)));
        }
        if value & !mask(bits) != 0 {
            return Err(Error::out_of_range("value", mask(bits), value));
        }
        Ok(ModCounter { bits, value })
    }

    /// The width in bits.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.bits
    }

    /// The current value.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.value
    }

    /// The next value, modulo 2^bits.
    #[must_use]
    pub const fn next(self) -> ModCounter {
        ModCounter {
            bits: self.bits,
            value: self.value.wrapping_add(1) & mask(self.bits),
        }
    }

    /// Advances the counter by one, modulo 2^bits.
    pub fn increment(&mut self) {
        *self = self.next();
    }

    /// Forward distance from `self` to `later`, modulo 2^bits.
    ///
    /// # Errors
    /// `OutOfRange` if `later` needs more than `bits` bits.
    pub fn distance_to(self, later: u64) -> Result<u64> {
        ModCounter::new(self.bits, later)
            .map(|later| later.value.wrapping_sub(self.value) & mask(self.bits))
    }

    /// Classifies `next` as received after `self` (CMN-031): distance 1 in sequence,
    /// 0 repeat, 2 … modulus/2 gap, otherwise backwards.
    ///
    /// # Errors
    /// `OutOfRange` if `next` needs more than `bits` bits.
    pub fn classify(self, next: u64) -> Result<Continuity> {
        ModCounter::new(self.bits, next)
            .map(|later| classify_masked(self.bits, self.value, later.value))
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "test bookkeeping on bounded values and u128 modular model (RS-018)"
)]
mod tests {
    use super::{Continuity, ModCounter};
    use crate::error::{ErrorDetail, ErrorKind};
    use proptest::prelude::*;

    #[test]
    fn exhaustive_8_bit() {
        // req: CMN-030, CMN-031
        for prev in 0..=255_u64 {
            let c = ModCounter::new(8, prev).unwrap();
            for next in 0..=255_u64 {
                let d = (next + 256 - prev) % 256;
                assert_eq!(c.distance_to(next), Ok(d));
                let expected = match d {
                    0 => Continuity::Repeat,
                    1 => Continuity::InSequence,
                    2..=128 => Continuity::Gap { missing: d - 1 },
                    _ => Continuity::Backwards,
                };
                assert_eq!(c.classify(next), Ok(expected), "prev {prev} next {next}");
            }
        }
    }

    #[test]
    fn widths_wrap_and_validate() {
        // req: CMN-030, GEN-024
        let mut c = ModCounter::new(64, u64::MAX).unwrap();
        c.increment();
        assert_eq!((c.bits(), c.value()), (64, 0));
        assert_eq!(ModCounter::new(1, 1).unwrap().next().value(), 0);
        assert_eq!(ModCounter::new(14, 16383).unwrap().next().value(), 0);
        assert_eq!(
            ModCounter::new(64, 0).unwrap().classify(1 << 63),
            Ok(Continuity::Gap {
                missing: (1 << 63) - 1
            })
        );
        assert_eq!(
            ModCounter::new(64, 0).unwrap().classify((1 << 63) + 1),
            Ok(Continuity::Backwards)
        );
        for (bits, value, expected, found) in [(0, 0, 1, 0), (65, 0, 64, 65), (8, 256, 255, 256)] {
            let e = ModCounter::new(bits, value).unwrap_err();
            assert_eq!(e.kind(), ErrorKind::OutOfRange);
            assert_eq!(e.detail(), ErrorDetail::Expected { expected, found });
        }
        assert_eq!(
            ModCounter::new(8, 1)
                .unwrap()
                .distance_to(300)
                .unwrap_err()
                .kind(),
            ErrorKind::OutOfRange
        );
        assert_eq!(
            ModCounter::new(8, 1)
                .unwrap()
                .classify(300)
                .unwrap_err()
                .kind(),
            ErrorKind::OutOfRange
        );
    }

    proptest! {
        #[test]
        fn wider_counters_match_u128_model(bits in 1_u8..=64, raw_prev in any::<u64>(), raw_next in any::<u64>()) {
            // req: CMN-030, CMN-031, GEN-024, VER-023
            let modulus = 1_u128 << bits;
            let mask = u64::MAX >> (64 - bits);
            let prev = raw_prev & mask;
            let next = raw_next & mask;
            let c = ModCounter::new(bits, prev).unwrap();
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
    }
}
