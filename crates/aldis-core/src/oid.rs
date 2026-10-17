//! Only-Idle-Data (OID) pseudo-noise generator (CMN-020…CMN-023).
//!
//! TM (132.0-B-3 §4.1.4.6.2, Annex D), AOS (732.0-B-5 §4.1.4.1.5.2), and USLP (732.1-B-3
//! §4.1.4.1.10) fill OID frame data fields with the output of a 32-cell LFSR with polynomial
//! D⁰ + D¹ + D² + D²² + D³². The generator is initialized once at device start-up and never
//! restarted: the sequence continues across OID frames (CMN-021). The first output octets are
//! `FF FF FF FF 6D B6 D8 61 45 1F …` (132.0 Annex D).

/// Fibonacci form (CMN-020): output xₙ, with x₀ … x₃₁ = 1 and
/// xₙ = xₙ₋₁ ⊕ xₙ₋₂ ⊕ xₙ₋₂₂ ⊕ xₙ₋₃₂ for n ≥ 32. This is the reference implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OidGenerator {
    /// The last 32 sequence bits: bit 31 = xₙ₋₃₂ (the next output), bit 0 = xₙ₋₁.
    register: u32,
}

impl OidGenerator {
    /// The generator in its device start-up state (all-ones seed).
    #[must_use]
    pub const fn new() -> OidGenerator {
        OidGenerator { register: u32::MAX }
    }

    /// The next bit of the sequence.
    pub fn next_bit(&mut self) -> u8 {
        let r = self.register;
        let out = r >> 31;
        let new = (r ^ (r >> 1) ^ (r >> 21) ^ (r >> 31)) & 1;
        self.register = (r << 1) | new;
        u8::from(out == 1)
    }

    /// The next octet, MSB first.
    pub fn next_octet(&mut self) -> u8 {
        (0..8).fold(0, |octet, _| (octet << 1) | self.next_bit())
    }

    /// Fills `buf` with the next `buf.len()` octets and advances by exactly that many (CMN-023).
    pub fn fill(&mut self, buf: &mut [u8]) {
        for octet in buf {
            *octet = self.next_octet();
        }
    }
}

impl Default for OidGenerator {
    fn default() -> OidGenerator {
        OidGenerator::new()
    }
}

/// Galois form (CMN-022), seeded with `0b00000000001111111111111111111101` (132.0 Annex D,
/// figure D-2): output the low bit, shift right, and XOR `0x8020_0003` when the output bit is 1.
/// The wiring was derived by searching for the configuration that reproduces the Annex D
/// sequence, then checked equal to [`OidGenerator`] over 10⁷ bits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OidGeneratorGalois {
    register: u32,
}

impl OidGeneratorGalois {
    const SEED: u32 = 0b0000_0000_0011_1111_1111_1111_1111_1101;
    const TAPS: u32 = 0x8020_0003;

    /// The generator in its device start-up state.
    #[must_use]
    pub const fn new() -> OidGeneratorGalois {
        OidGeneratorGalois {
            register: OidGeneratorGalois::SEED,
        }
    }

    /// The next bit of the sequence.
    pub fn next_bit(&mut self) -> u8 {
        let out = self.register & 1;
        self.register >>= 1;
        if out == 1 {
            self.register ^= OidGeneratorGalois::TAPS;
        }
        u8::from(out == 1)
    }

    /// The next octet, MSB first.
    pub fn next_octet(&mut self) -> u8 {
        (0..8).fold(0, |octet, _| (octet << 1) | self.next_bit())
    }

    /// Fills `buf` with the next `buf.len()` octets (CMN-023).
    pub fn fill(&mut self, buf: &mut [u8]) {
        for octet in buf {
            *octet = self.next_octet();
        }
    }
}

impl Default for OidGeneratorGalois {
    fn default() -> OidGeneratorGalois {
        OidGeneratorGalois::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{OidGenerator, OidGeneratorGalois};

    const ANNEX_D: [u8; 20] = [
        0xFF, 0xFF, 0xFF, 0xFF, 0x6D, 0xB6, 0xD8, 0x61, 0x45, 0x1F, 0x11, 0xF1, 0x97, 0x16, 0x72,
        0x3C, 0xBE, 0x7E, 0x00, 0xB1,
    ];

    #[test]
    fn both_forms_reproduce_annex_d() {
        // req: CMN-020, CMN-022
        let mut fib = [0_u8; 20];
        OidGenerator::new().fill(&mut fib);
        assert_eq!(fib, ANNEX_D);
        let mut gal = [0_u8; 20];
        OidGeneratorGalois::default().fill(&mut gal);
        assert_eq!(gal, ANNEX_D);
    }

    #[test]
    fn sequence_continues_across_fills() {
        // req: CMN-021, CMN-023
        let mut g = OidGenerator::default();
        let (mut a, mut b) = ([0_u8; 7], [0_u8; 13]);
        g.fill(&mut a);
        g.fill(&mut b);
        assert_eq!(a, ANNEX_D[..7]);
        assert_eq!(b, ANNEX_D[7..]);
        g.fill(&mut []);
        assert_ne!(g, OidGenerator::new());
    }

    #[test]
    fn forms_are_identical_over_a_long_run() {
        // req: CMN-022
        let (mut fib, mut gal) = (OidGenerator::new(), OidGeneratorGalois::new());
        for _ in 0..1_000_000 {
            assert_eq!(fib.next_octet(), gal.next_octet());
        }
    }
}
