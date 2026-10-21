//! Frame Header Error Control: the shortened Reed-Solomon code over GF(2⁴) (AOS-013, AOS-014).
//!
//! 732.0-B-5 EC1 §4.1.2.6.5 specifies a Reed-Solomon (15,11) code over GF(2⁴), shortened by five
//! symbols and converted to GF(2) to form a binary (40,24) code (AOS-013). Counted in
//! transmitted symbols it is (10,6), the name 732.0-B-4 §4.1.2.6.5 gives the same code (OI-04,
//! OI-35 item 6, M2 plan D11), so one implementation serves both editions. Its parameters, all
//! from §4.1.2.6.5:
//!
//! | Parameter | Value | Clause |
//! |---|---|---|
//! | Bits per symbol | J = 4 | a |
//! | Correction capability | E = 2 symbols | b |
//! | Field generator | F(x) = x⁴ + x + 1 over GF(2) | c |
//! | Code generator | g(x) = (x + α⁶)(x + α⁷)(x + α⁸)(x + α⁹) = x⁴ + α³x³ + αx² + α³x + 1, with F(α) = 0 | d |
//! | Symbol bits | the left bit of a symbol is sent first (α³ = `1000` is sent as 1, 0, 0, 0) | e |
//! | Virtual fill | five zero symbols, not transmitted, at the start of the codeword | f |
//! | Form | systematic | g |
//! | Mapping | symbols 1–5 virtual fill; 6–9 header bits 0–15; 10–11 bits 40–47; 12–15 (parity) bits 48–63 | h |
//!
//! The header bits 0–15 and 40–47 are the Master Channel Identifier, the Virtual Channel
//! Identifier, the Signaling Field, and the Virtual Channel Frame Count Cycle; the Virtual
//! Channel Frame Count (bits 16–39) is not protected (§4.1.2.6.1 notes 1 and 2). Bit 0 is the
//! first transmitted bit and the most significant (§1.6.3), so a symbol is the nibble of its
//! four header bits read most significant first.
//!
//! **Polynomial order.** Symbol k (1–15) is the coefficient of x^(15−k), so the first symbol
//! sent has the highest degree. §4.1.2.6.5 does not state the order, and need not: g(x) is its
//! own reciprocal (coefficients 1, α³, α, α³, 1), so the set of codewords is the same for either
//! order, and with the virtual fill and the systematic symbols fixed the parity symbols are
//! unique.
//!
//! **Decoding** ([`decode`]): the four syndromes S₆…S₉ (the received codeword at α⁶…α⁹); the
//! error locator Λ(x) of at most two errors in closed form (Peterson); a search for its roots
//! over the ten transmitted positions only, in a loop of fixed length (GEN-022); and the error
//! values by Forney's formula. A locator without as many distinct roots among the transmitted
//! positions as its degree, which includes one with a root in the virtual fill, means more than
//! two symbol errors: [`ErrorKind::HeaderUncorrectable`]. Three or more symbol errors can also
//! look like one or two and be miscorrected; the code corrects at most two (§4.1.2.6.5 note 3,
//! OI-35 item 5).
//!
//! GF(2⁴) arithmetic uses log and antilog tables of 15 entries.

use aldis_core::{Error, ErrorKind};

/// α^i for i = 0…14, as the 4-bit vector of §4.1.2.6.5 d (`1100` for α⁶), with
/// F(α) = α⁴ + α + 1 = 0, so α⁴ = α + 1. A unit test regenerates it from F(x).
const ANTILOG: [u8; 15] = [
    0b0001, 0b0010, 0b0100, 0b1000, 0b0011, 0b0110, 0b1100, 0b1011, 0b0101, 0b1010, 0b0111, 0b1110,
    0b1111, 0b1101, 0b1001,
];

/// The logarithm to base α of x = 1…15, at index x − 1: the inverse of [`ANTILOG`] (unit test).
const LOG: [u8; 15] = [0, 1, 4, 2, 8, 5, 10, 3, 14, 9, 7, 6, 13, 11, 12];

/// An element of GF(2⁴): a 4-bit symbol. The value is below 16 by construction: it comes from
/// a nibble or from [`ANTILOG`], and the sum of two elements is their XOR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Gf(u8);

impl Gf {
    const ZERO: Gf = Gf(0);
    const ONE: Gf = Gf(1);

    /// The symbol in the upper nibble of an octet (the first sent, §4.1.2.6.5 e).
    const fn high(octet: u8) -> Gf {
        Gf(octet >> 4)
    }

    /// The symbol in the lower nibble of an octet.
    const fn low(octet: u8) -> Gf {
        Gf(octet & 0x0F)
    }

    /// The octet holding `high` then `low`.
    const fn octet(high: Gf, low: Gf) -> u8 {
        high.0 << 4 | low.0
    }

    /// Addition (and subtraction) in GF(2⁴).
    const fn add(self, other: Gf) -> Gf {
        Gf(self.0 ^ other.0)
    }

    /// α^(e mod 15).
    fn alpha_pow(e: u8) -> Gf {
        let i = usize::from(e % 15);
        #[expect(clippy::indexing_slicing, reason = "e % 15 < 15, the table length")]
        Gf(ANTILOG[i])
    }

    /// The logarithm to base α of a non-zero element; `None` for zero, which has no logarithm.
    fn log(self) -> Option<u8> {
        // Zero wraps to `usize::MAX`, outside the table; 1…15 map to 0…14.
        LOG.get(usize::from(self.0).wrapping_sub(1)).copied()
    }

    /// Multiplication: α^(log a + log b), or zero if either factor is zero.
    fn mul(self, other: Gf) -> Gf {
        match (self.log(), other.log()) {
            // Both logarithms are at most 14, so the sum cannot wrap.
            (Some(a), Some(b)) => Gf::alpha_pow(a.wrapping_add(b)),
            _ => Gf::ZERO,
        }
    }

    /// The multiplicative inverse α^(15 − log a); zero for zero, which has none.
    fn inv(self) -> Gf {
        match self.log() {
            // The logarithm is at most 14, so the difference cannot wrap.
            Some(a) => Gf::alpha_pow(15_u8.wrapping_sub(a)),
            None => Gf::ZERO,
        }
    }
}

/// The coefficients of x³, x², and x of g(x) = x⁴ + α³x³ + αx² + α³x + 1 (§4.1.2.6.5 d); the
/// coefficients of x⁴ and x⁰ are 1.
const G3: Gf = Gf(0b1000);
const G2: Gf = Gf(0b0010);
const G1: Gf = Gf(0b1000);

/// The roots α⁶, α⁷, α⁸, α⁹ of g(x), at which the syndromes are taken (§4.1.2.6.5 d).
const ROOTS: [Gf; 4] = [Gf(0b1100), Gf(0b1011), Gf(0b0101), Gf(0b1010)];

/// α⁻¹ = α¹⁴, the step of the root search from one transmitted position to the next.
const ALPHA_INV: Gf = Gf(0b1001);

/// The ten transmitted symbols of a primary header with FHEC, symbols 6–15 of §4.1.2.6.5 h in
/// order: octets 0, 1, and 5 (bits 0–15 and 40–47), then octets 6 and 7 (the FHEC, bits 48–63).
/// Symbol 6 + k is the coefficient of x^(9 − k).
type Word = [Gf; 10];

const fn word(header: [u8; 8]) -> Word {
    let [o0, o1, _, _, _, o5, o6, o7] = header;
    [
        Gf::high(o0),
        Gf::low(o0),
        Gf::high(o1),
        Gf::low(o1),
        Gf::high(o5),
        Gf::low(o5),
        Gf::high(o6),
        Gf::low(o6),
        Gf::high(o7),
        Gf::low(o7),
    ]
}

/// The number of symbols [`decode`] corrected: at most two (§4.1.2.6.5 b; AOS-014, M2 plan D9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CorrectedSymbols {
    /// The header was a codeword.
    Zero,
    /// One symbol was corrected.
    One,
    /// Two symbols were corrected.
    Two,
}

impl CorrectedSymbols {
    /// The count: 0, 1, or 2 (AOS-014).
    #[must_use]
    pub const fn get(self) -> u8 {
        match self {
            CorrectedSymbols::Zero => 0,
            CorrectedSymbols::One => 1,
            CorrectedSymbols::Two => 2,
        }
    }
}

/// A primary header that passed the FHEC check, corrected where needed (732.0 §4.1.2.6,
/// §4.3.7.2; AOS-014).
///
/// The octets are the received ones with at most two symbols corrected, the FHEC included;
/// octets 2–4, the Virtual Channel Frame Count, which the code does not protect, are always the
/// received ones (§4.1.2.6.1 note 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CorrectedHeader {
    octets: [u8; 8],
    corrected: CorrectedSymbols,
}

impl CorrectedHeader {
    /// The eight header octets after correction: a codeword of the FHEC code (AOS-013).
    #[must_use]
    pub const fn octets(&self) -> [u8; 8] {
        self.octets
    }

    /// How many symbols were corrected (AOS-014, M2 plan D9).
    #[must_use]
    pub const fn corrected_symbols(&self) -> CorrectedSymbols {
        self.corrected
    }
}

/// The FHEC of a primary header: the four parity symbols, as octets 6 and 7, computed from the
/// six systematic symbols in octets 0, 1, and 5 (732.0 §4.1.2.6.5; AOS-013, AOS-019).
///
/// `header` is octets 0–5; octets 2–4 are not covered. The parity symbols are the remainder of
/// m(x)·x⁴ divided by g(x), where m(x) has the systematic symbols as coefficients, the first one
/// highest; the five symbols of virtual fill are zero and do not change the remainder.
#[must_use]
pub fn encode(header: &[u8; 6]) -> [u8; 2] {
    let [o0, o1, _, _, _, o5] = *header;
    let systematic = [
        Gf::high(o0),
        Gf::low(o0),
        Gf::high(o1),
        Gf::low(o1),
        Gf::high(o5),
        Gf::low(o5),
    ];
    // Division by g(x) in a shift register: r3 is the coefficient of x³ of the remainder.
    let (mut r3, mut r2, mut r1, mut r0) = (Gf::ZERO, Gf::ZERO, Gf::ZERO, Gf::ZERO);
    for m in systematic {
        let feedback = m.add(r3);
        r3 = r2.add(feedback.mul(G3));
        r2 = r1.add(feedback.mul(G2));
        r1 = r0.add(feedback.mul(G1));
        // The coefficient of x⁰ of g(x) is 1.
        r0 = feedback;
    }
    // Symbols 12–15: the remainder, highest coefficient first (§4.1.2.6.5 h).
    [Gf::octet(r3, r2), Gf::octet(r1, r0)]
}

/// Writes the FHEC of octets 0–5 of `header` into its octets 6 and 7 (732.0 §4.1.2.6.5;
/// AOS-013, AOS-019).
pub fn write(header: &mut [u8; 8]) {
    let [o0, o1, o2, o3, o4, o5, _, _] = *header;
    let [f0, f1] = encode(&[o0, o1, o2, o3, o4, o5]);
    *header = [o0, o1, o2, o3, o4, o5, f0, f1];
}

/// Checks a received primary header with FHEC and corrects up to two symbol errors
/// (732.0 §4.1.2.6.5, §4.3.7.2; AOS-013, AOS-014).
///
/// The received octets are not changed: the result is a corrected copy, from which the header
/// fields are then read. Octets 2–4 are copied unchanged. The correction is reported in both
/// decoding modes, by [`CorrectedHeader::corrected_symbols`], and never as an anomaly (M2 plan
/// D9).
///
/// **Order with the FECF.** The All Frames Reception Function first uses the FHEC to correct the
/// header (§4.3.7.2), then recomputes the CRC over the Transfer Frame (§4.3.7.3). The FECF was
/// computed by the sender over the uncorrupted header, so a caller that checks a configured
/// FECF does so over the frame with the [corrected octets](CorrectedHeader::octets) in place of
/// the received header (AOS-014).
///
/// `HeaderUncorrectable` is returned only for the error patterns the decoder detects: the code
/// has minimum distance 5, so three or more symbol errors can be miscorrected to another
/// codeword and reported as one or two corrections (§4.1.2.6.5 note 3; OI-35 item 5).
///
/// # Errors
/// [`ErrorKind::HeaderUncorrectable`] if no pattern of at most two symbol errors explains the
/// syndromes.
pub fn decode(received: &[u8; 8]) -> Result<CorrectedHeader, Error> {
    let mut symbols = word(*received);
    let [s6, s7, s8, s9] = ROOTS.map(|root| {
        symbols
            .iter()
            .fold(Gf::ZERO, |acc, &symbol| acc.mul(root).add(symbol))
    });
    if [s6, s7, s8, s9] == [Gf::ZERO; 4] {
        return Ok(CorrectedHeader {
            octets: *received,
            corrected: CorrectedSymbols::Zero,
        });
    }
    // With error locations X and values Y, S_j = Σ Y·X^j. The locator
    // Λ(x) = 1 + Λ1·x + Λ2·x² has the roots X⁻¹ and satisfies
    // S8 + Λ1·S7 + Λ2·S6 = 0 and S9 + Λ1·S8 + Λ2·S7 = 0 (Newton's identities).
    let det = s7.mul(s7).add(s6.mul(s8));
    let (l1, l2, degree) = if det == Gf::ZERO {
        // A single error: S7 = X·S6, S8 = X·S7, S9 = X·S8, so Λ(x) = 1 + X·x.
        let l1 = s7.mul(s6.inv());
        if s6 == Gf::ZERO || s9 != l1.mul(s8) {
            return Err(Error::new(ErrorKind::HeaderUncorrectable));
        }
        (l1, Gf::ZERO, CorrectedSymbols::One)
    } else {
        // Two errors: Cramer's rule on the two identities.
        let inv = det.inv();
        let l1 = s7.mul(s8).add(s6.mul(s9)).mul(inv);
        let l2 = s8.mul(s8).add(s7.mul(s9)).mul(inv);
        (l1, l2, CorrectedSymbols::Two)
    };
    // Forney with the first root α⁶: Y = X^(1−6)·Ω(X⁻¹)/Λ'(X⁻¹), where
    // Ω(x) = S(x)·Λ(x) mod x² = S6 + (S7 + S6·Λ1)·x and, in characteristic 2, Λ'(x) = Λ1.
    let omega1 = s7.add(s6.mul(l1));
    let l1_inv = l1.inv();
    // Root search over the ten transmitted positions only: position p (degree p, symbol 15 − p)
    // has X = α^p; `z` is X⁻¹. A root in the virtual fill (degrees 10–14) is never found, so it
    // leaves the count short of the degree.
    let mut roots = 0_u8;
    let mut z = Gf::ONE;
    for symbol in symbols.iter_mut().rev() {
        if Gf::ONE.add(l1.mul(z)).add(l2.mul(z).mul(z)) == Gf::ZERO {
            let z2 = z.mul(z);
            let z5 = z2.mul(z2).mul(z);
            let value = z5.mul(s6.add(omega1.mul(z))).mul(l1_inv);
            *symbol = symbol.add(value);
            // At most ten iterations: the count cannot wrap.
            roots = roots.wrapping_add(1);
        }
        z = z.mul(ALPHA_INV);
    }
    if roots != degree.get() {
        return Err(Error::new(ErrorKind::HeaderUncorrectable));
    }
    let [c0, c1, c2, c3, c4, c5, c6, c7, c8, c9] = symbols;
    let [_, _, o2, o3, o4, _, _, _] = *received;
    Ok(CorrectedHeader {
        octets: [
            Gf::octet(c0, c1),
            Gf::octet(c2, c3),
            o2,
            o3,
            o4,
            Gf::octet(c4, c5),
            Gf::octet(c6, c7),
            Gf::octet(c8, c9),
        ],
        corrected: degree,
    })
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{ANTILOG, CorrectedSymbols, G1, G2, G3, Gf, LOG, ROOTS, decode, encode, write};
    use aldis_core::ErrorKind;
    use aldis_testkit::oracle::fhec as model;

    /// A deterministic sequence of headers (64-bit LCG), for sampled checks.
    fn headers(n: usize) -> impl Iterator<Item = [u8; 8]> {
        let mut x = 0x2545_F491_4F6C_DD1D_u64;
        (0..n).map(move |_| {
            x = x
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            x.to_be_bytes()
        })
    }

    fn codeword(h: [u8; 8]) -> [u8; 8] {
        let mut c = h;
        write(&mut c);
        c
    }

    #[test]
    fn tables_regenerate_from_the_field_polynomial() {
        // req: AOS-013
        // α^(i+1) = α·α^i, reduced by F(x) = x⁴ + x + 1 (§4.1.2.6.5 c).
        let mut x = 1_u8;
        for (i, &entry) in ANTILOG.iter().enumerate() {
            assert_eq!(entry, x, "alpha^{i}");
            assert_eq!(usize::from(LOG[usize::from(x) - 1]), i, "log {x}");
            x <<= 1;
            if x & 0x10 != 0 {
                x ^= 0b1_0011;
            }
        }
        assert_eq!(x, 1, "alpha^15 = 1");
        // The values printed in §4.1.2.6.5 d.
        assert_eq!(
            (ANTILOG[0], ANTILOG[1], ANTILOG[3]),
            (0b0001, 0b0010, 0b1000)
        );
        assert_eq!(
            ROOTS.map(|r| r.0),
            [ANTILOG[6], ANTILOG[7], ANTILOG[8], ANTILOG[9]]
        );
        assert_eq!(ROOTS.map(|r| r.0), [0b1100, 0b1011, 0b0101, 0b1010]);
        assert_eq!(super::ALPHA_INV.0, ANTILOG[14]);
    }

    #[test]
    fn field_arithmetic_agrees_with_the_carry_less_product() {
        // req: AOS-013
        for a in 0..16 {
            for b in 0..16 {
                assert_eq!(Gf(a).mul(Gf(b)).0, model::mul(a, b), "{a} * {b}");
            }
            let inv = Gf(a).inv();
            if a == 0 {
                assert_eq!(inv, Gf::ZERO);
            } else {
                assert_eq!(Gf(a).mul(inv), Gf::ONE, "{a}^-1");
            }
        }
    }

    #[test]
    fn generator_is_the_product_of_its_four_roots() {
        // req: AOS-013
        // (x + α⁶)(x + α⁷)(x + α⁸)(x + α⁹), coefficients highest first.
        let mut g = [Gf::ONE, Gf::ZERO, Gf::ZERO, Gf::ZERO, Gf::ZERO];
        for (n, root) in ROOTS.into_iter().enumerate() {
            // Multiply the degree-n product by (x + root).
            let mut next = [Gf::ZERO; 5];
            for i in 0..=n {
                next[i] = next[i].add(g[i]);
                next[i + 1] = next[i + 1].add(g[i].mul(root));
            }
            g = next;
        }
        assert_eq!(g, [Gf::ONE, G3, G2, G1, Gf::ONE]);
        // §4.1.2.6.5 d: g(x) = x⁴ + α³x³ + αx² + α³x + 1.
        assert_eq!(
            [G3, G2, G1],
            [Gf(ANTILOG[3]), Gf(ANTILOG[1]), Gf(ANTILOG[3])]
        );
    }

    #[test]
    fn encoder_agrees_with_the_model() {
        // req: AOS-013
        // The code is GF(2)-linear, so the 24 unit vectors of the systematic bits and zero
        // decide it; a sample of other headers checks the encoder is linear.
        assert_eq!(encode(&[0; 6]), [0, 0]);
        for bit in 0..24 {
            let mut h = [0_u8; 6];
            let octet = [0, 1, 5][bit / 8];
            h[octet] = 0x80 >> (bit % 8);
            assert_eq!(encode(&h), model::encode(&h), "bit {bit}");
        }
        for h in headers(4096) {
            let six = [h[0], h[1], h[2], h[3], h[4], h[5]];
            assert_eq!(encode(&six), model::encode(&six), "{h:02X?}");
            // Octets 2–4 are not covered (§4.1.2.6.1 note 2).
            assert_eq!(encode(&six), encode(&[h[0], h[1], 0, 0, 0, h[5]]));
        }
    }

    #[test]
    fn write_fills_octets_6_and_7_only() {
        // req: AOS-013
        let mut h = [0x41, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xEE, 0xEE];
        write(&mut h);
        let f = encode(&[0x41, 0x23, 0x45, 0x67, 0x89, 0xAB]);
        assert_eq!(h, [0x41, 0x23, 0x45, 0x67, 0x89, 0xAB, f[0], f[1]]);
    }

    #[test]
    fn model_table_holds_every_pattern_of_at_most_two_symbol_errors() {
        // req: AOS-013, AOS-014
        // 10·15 + 45·15² = 10 275 patterns, plus the zero pattern, each with its own syndrome.
        assert_eq!(model::ERROR_PATTERNS, 10_275);
        assert_eq!(model::patterns(), 10_276);
        assert_eq!(model::correctable_syndromes(), 10_276);
    }

    #[test]
    fn decoder_agrees_with_the_model_on_all_65536_syndromes() {
        // req: AOS-013, AOS-014, GEN-020
        // With the systematic symbols fixed, the syndrome map of the 16 FHEC bits is a
        // bijection (the model asserts it), so the 65 536 FHEC values give every syndrome once.
        for base in [[0_u8; 8], [0x7F, 0xFF, 0x12, 0x34, 0x56, 0xC5, 0, 0]] {
            let (mut correctable, mut uncorrectable) = (0, 0);
            for fhec in 0..=u16::MAX {
                let [f0, f1] = fhec.to_be_bytes();
                let h = [base[0], base[1], base[2], base[3], base[4], base[5], f0, f1];
                match (decode(&h), model::decode(&h)) {
                    (Ok(c), Some((octets, weight))) => {
                        assert_eq!(c.octets(), octets, "{h:02X?}");
                        assert_eq!(c.corrected_symbols().get(), weight, "{h:02X?}");
                        assert_eq!(c.octets()[2..5], h[2..5], "VC frame count altered");
                        correctable += 1;
                    }
                    (Err(e), None) => {
                        assert_eq!(e.kind(), ErrorKind::HeaderUncorrectable);
                        assert_eq!(e.field(), None);
                        uncorrectable += 1;
                    }
                    (d, m) => panic!("{h:02X?}: decoder {d:?}, model {m:?}"),
                }
            }
            assert_eq!((correctable, uncorrectable), (10_276, 65_536 - 10_276));
        }
    }

    #[test]
    fn every_pattern_of_one_or_two_symbol_errors_is_corrected() {
        // req: AOS-013, AOS-014
        let mut patterns = 0;
        for original in headers(3).map(codeword).chain([codeword([0; 8])]) {
            assert_eq!(
                decode(&original).map(|c| (c.octets(), c.corrected_symbols())),
                Ok((original, CorrectedSymbols::Zero))
            );
            for i in 0..model::SYMBOLS {
                for v in 1..16 {
                    let mut e = [0; model::SYMBOLS];
                    e[i] = v;
                    let c = decode(&model::with_errors(&original, &e)).unwrap();
                    assert_eq!(
                        (c.octets(), c.corrected_symbols()),
                        (original, CorrectedSymbols::One)
                    );
                    patterns += 1;
                    for j in i + 1..model::SYMBOLS {
                        for w in 1..16 {
                            e[j] = w;
                            let c = decode(&model::with_errors(&original, &e)).unwrap();
                            assert_eq!(
                                (c.octets(), c.corrected_symbols()),
                                (original, CorrectedSymbols::Two)
                            );
                            patterns += 1;
                        }
                        e[j] = 0;
                    }
                }
            }
        }
        assert_eq!(patterns, 4 * model::ERROR_PATTERNS);
    }

    #[test]
    fn errors_in_the_frame_count_are_neither_seen_nor_changed() {
        // req: AOS-014
        let original = codeword([0x40, 0x01, 0x00, 0x00, 0x00, 0x00, 0, 0]);
        let mut h = original;
        h[2..5].copy_from_slice(&[0xFF, 0xA5, 0x5A]);
        let c = decode(&h).unwrap();
        assert_eq!(
            (c.octets(), c.corrected_symbols()),
            (h, CorrectedSymbols::Zero)
        );
    }

    #[test]
    fn corrected_symbol_counts() {
        // req: AOS-014
        assert_eq!(
            [
                CorrectedSymbols::Zero,
                CorrectedSymbols::One,
                CorrectedSymbols::Two
            ]
            .map(CorrectedSymbols::get),
            [0, 1, 2]
        );
    }
}
