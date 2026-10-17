//! Modular frame and sequence counters with gap classification (CMN-030, CMN-031, GEN-024).
//!
//! Used for TM MC/VC frame counts (8 bits, 132.0 §4.1.2.5–6), AOS VC frame counts (24 bits,
//! or 28 with the cycle field, 732.0 §4.1.2.4–5), USLP VCF counts (8–56 bits, 732.1
//! §4.1.2.12), and Space Packet sequence counts (14 bits, 133.0 §4.1.3.4.3). The width is a
//! run-time value because USLP signals the count length in every frame header.

use crate::error::{Error, Result};

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

impl ModCounter {
    /// A counter of `bits` width holding `value`.
    ///
    /// # Errors
    /// `OutOfRange` if `bits` is not 1–64 or `value` needs more than `bits` bits.
    pub fn new(bits: u8, value: u64) -> Result<ModCounter> {
        if bits == 0 || bits > 64 {
            return Err(Error::out_of_range("bits", 64, u64::from(bits)));
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
        let later = ModCounter::new(self.bits, later)?;
        Ok(later.value.wrapping_sub(self.value) & mask(self.bits))
    }

    /// Classifies `next` as received after `self` (CMN-031): distance 1 in sequence,
    /// 0 repeat, 2 … modulus/2 gap, otherwise backwards.
    ///
    /// # Errors
    /// `OutOfRange` if `next` needs more than `bits` bits.
    pub fn classify(self, next: u64) -> Result<Continuity> {
        let d = self.distance_to(next)?;
        let half = 1_u64 << (self.bits.wrapping_sub(1));
        Ok(match d {
            0 => Continuity::Repeat,
            1 => Continuity::InSequence,
            d if d <= half => Continuity::Gap {
                missing: d.wrapping_sub(1),
            },
            _ => Continuity::Backwards,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Continuity, ModCounter};
    use crate::error::ErrorKind;

    #[test]
    fn exhaustive_8_bit() {
        // req: CMN-030, CMN-031
        for prev in 0..=255_u64 {
            let c = ModCounter::new(8, prev).unwrap();
            for next in 0..=255_u64 {
                let d = (next + 256 - prev) % 256;
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
        for (bits, value) in [(0, 0), (65, 0), (8, 256)] {
            assert_eq!(
                ModCounter::new(bits, value).unwrap_err().kind(),
                ErrorKind::OutOfRange
            );
        }
        assert_eq!(
            ModCounter::new(8, 1)
                .unwrap()
                .classify(300)
                .unwrap_err()
                .kind(),
            ErrorKind::OutOfRange
        );
    }
}
