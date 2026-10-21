//! Independent oracles that library code is checked against in tests and fuzzing.
//!
//! An oracle shares no code with the implementation it checks. It is written from the Blue
//! Book text by the simplest means that is obviously correct, however slow, and works on plain
//! octets, so that a crate's unit tests can use it without sharing a type with it.

/// The table model of the AOS Frame Header Error Control (732.0-B-5 EC1 §4.1.2.6; AOS-013,
/// AOS-014; M2 plan D10).
///
/// The code is the Reed-Solomon (15,11) code over GF(2⁴) with field generator
/// F(x) = x⁴ + x + 1 and the roots α⁶, α⁷, α⁸, α⁹ of g(x) (§4.1.2.6.5 c, d). A codeword is
/// symbols 1–15 in the Blue Book's numbering (§4.1.2.6.5 h): five symbols of zero virtual fill,
/// the six systematic symbols (header bits 0–15 and 40–47), and the four parity symbols (bits
/// 48–63), each symbol transmitted from its left bit (§4.1.2.6.5 e, f). Symbol k is the
/// coefficient of x^(15−k).
///
/// The model needs no log tables, no division by g(x), and no error-locator algebra:
/// multiplication is the carry-less product reduced by F(x); a header's syndrome is the
/// codeword polynomial evaluated at the four roots; the encoder looks up the parity symbols
/// whose syndrome cancels that of the systematic symbols (the syndrome map of the four parity
/// symbols is a bijection, since any four columns of the parity check matrix of an MDS code are
/// independent); and the decoder is a table from the syndrome of every pattern of at most two
/// symbol errors over the ten transmitted symbols to that pattern.
pub mod fhec {
    use std::sync::OnceLock;

    /// F(x) = x⁴ + x + 1 (§4.1.2.6.5 c).
    const FIELD_POLY: u8 = 0b1_0011;

    /// The number of transmitted symbols: six systematic and four parity (§4.1.2.6.5 h).
    pub const SYMBOLS: usize = 10;

    /// Patterns of one or two symbol errors over the ten transmitted symbols:
    /// 10 · 15 + 45 · 15² (M2 plan D10).
    pub const ERROR_PATTERNS: usize = 10 * 15 + 45 * 15 * 15;

    /// Carry-less product of two 4-bit symbols reduced by F(x).
    #[must_use]
    pub fn mul(a: u8, b: u8) -> u8 {
        // Symbols are 4-bit, so the product before reduction has at most 7 bits.
        let mut r = 0_u8;
        for i in 0..4 {
            if b >> i & 1 == 1 {
                r ^= (a & 15) << i;
            }
        }
        for i in (4..8).rev() {
            if r >> i & 1 == 1 {
                r ^= FIELD_POLY << (i - 4);
            }
        }
        r
    }

    /// α^n, with α = x (`0010`, §4.1.2.6.5 d).
    #[must_use]
    pub fn alpha_pow(n: u32) -> u8 {
        (0..n).fold(1, |acc, _| mul(acc, 0b0010))
    }

    /// The ten transmitted symbols of an 8-octet primary header, in transmission order
    /// (symbols 6–15).
    #[must_use]
    pub const fn symbols(h: &[u8; 8]) -> [u8; SYMBOLS] {
        [
            h[0] >> 4,
            h[0] & 15,
            h[1] >> 4,
            h[1] & 15,
            h[5] >> 4,
            h[5] & 15,
            h[6] >> 4,
            h[6] & 15,
            h[7] >> 4,
            h[7] & 15,
        ]
    }

    /// The syndrome of ten transmitted symbols, packed as four nibbles (root α⁶ first): the
    /// 15-symbol codeword with its virtual fill, evaluated at α⁶…α⁹ by Horner's rule.
    #[must_use]
    pub fn syndrome(word: &[u8; SYMBOLS]) -> u16 {
        let full = [0_u8; 5].iter().chain(word.iter());
        let mut packed = 0_u16;
        for j in 6..=9 {
            let x = alpha_pow(j);
            let s = full.clone().fold(0, |acc, &c| mul(acc, x) ^ c);
            packed = packed << 4 | u16::from(s);
        }
        packed
    }

    /// The oracle's tables, built once.
    struct Tables {
        /// Syndrome → the error nibbles of the unique pattern of weight ≤ 2 with it.
        correct: Vec<Option<[u8; SYMBOLS]>>,
        /// Syndrome → the 16 parity bits whose syndrome it is.
        parity: Vec<u16>,
        /// Patterns enumerated, the zero pattern included.
        patterns: usize,
    }

    fn tables() -> &'static Tables {
        static TABLES: OnceLock<Tables> = OnceLock::new();
        TABLES.get_or_init(|| {
            let mut correct = vec![None; 1 << 16];
            let mut patterns = 0;
            let mut add = |e: [u8; SYMBOLS]| {
                let s = usize::from(syndrome(&e));
                assert!(correct[s].is_none(), "two patterns share syndrome {s:04X}");
                correct[s] = Some(e);
                patterns += 1;
            };
            add([0; SYMBOLS]);
            for i in 0..SYMBOLS {
                for v in 1..16 {
                    let mut e = [0; SYMBOLS];
                    e[i] = v;
                    add(e);
                }
            }
            for i in 0..SYMBOLS {
                for j in i + 1..SYMBOLS {
                    for v in 1..16 {
                        for w in 1..16 {
                            let mut e = [0; SYMBOLS];
                            e[i] = v;
                            e[j] = w;
                            add(e);
                        }
                    }
                }
            }
            let mut parity: Vec<Option<u16>> = vec![None; 1 << 16];
            for p in 0..=u16::MAX {
                let [hi, lo] = p.to_be_bytes();
                let word = [0, 0, 0, 0, 0, 0, hi >> 4, hi & 15, lo >> 4, lo & 15];
                let slot = &mut parity[usize::from(syndrome(&word))];
                assert!(slot.is_none(), "the parity map is not a bijection");
                *slot = Some(p);
            }
            let parity = parity.into_iter().map(|p| p.expect("bijection")).collect();
            Tables {
                correct,
                parity,
                patterns,
            }
        })
    }

    /// The number of patterns the table holds, the zero pattern included: 10 276 when no two
    /// patterns share a syndrome (building the table asserts that they do not).
    #[must_use]
    pub fn patterns() -> usize {
        tables().patterns
    }

    /// The number of syndromes with a table entry.
    #[must_use]
    pub fn correctable_syndromes() -> usize {
        tables().correct.iter().filter(|e| e.is_some()).count()
    }

    /// The FHEC (octets 6–7) of the first six octets of a primary header.
    #[must_use]
    pub fn encode(h: &[u8; 6]) -> [u8; 2] {
        let word = [
            h[0] >> 4,
            h[0] & 15,
            h[1] >> 4,
            h[1] & 15,
            h[5] >> 4,
            h[5] & 15,
            0,
            0,
            0,
            0,
        ];
        tables().parity[usize::from(syndrome(&word))].to_be_bytes()
    }

    /// The model's decision for a received 8-octet primary header: the corrected header and
    /// the number of corrected symbols, or `None` when no pattern of at most two symbol errors
    /// explains the syndrome.
    #[must_use]
    pub fn decode(h: &[u8; 8]) -> Option<([u8; 8], u8)> {
        let e = tables().correct[usize::from(syndrome(&symbols(h)))]?;
        let c: Vec<u8> = symbols(h).iter().zip(e).map(|(s, e)| s ^ e).collect();
        let mut out = *h;
        out[0] = c[0] << 4 | c[1];
        out[1] = c[2] << 4 | c[3];
        out[5] = c[4] << 4 | c[5];
        out[6] = c[6] << 4 | c[7];
        out[7] = c[8] << 4 | c[9];
        let weight = e.iter().fold(0, |n, &v| n + u8::from(v != 0));
        Some((out, weight))
    }

    /// Applies an error pattern (one nibble per transmitted symbol) to a header.
    #[must_use]
    pub fn with_errors(h: &[u8; 8], e: &[u8; SYMBOLS]) -> [u8; 8] {
        let mut out = *h;
        for (octet, pair) in [0, 1, 5, 6, 7].into_iter().zip(e.chunks(2)) {
            out[octet] ^= pair[0] << 4 | pair[1];
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::fhec;

    #[test]
    fn fhec_oracle_matches_the_blue_book_parameters() {
        // req: AOS-013
        // §4.1.2.6.5 d: α⁶ = 1100, α⁷ = 1011, α⁸ = 0101, α⁹ = 1010, α³ = 1000, α¹⁵ = 1.
        let printed = [
            (6, 0b1100),
            (7, 0b1011),
            (8, 0b0101),
            (9, 0b1010),
            (3, 0b1000),
        ];
        for (n, v) in printed {
            assert_eq!(fhec::alpha_pow(n), v, "alpha^{n}");
        }
        assert_eq!(fhec::alpha_pow(15), 1);
        assert_eq!(fhec::patterns(), fhec::ERROR_PATTERNS + 1);
        assert_eq!(fhec::correctable_syndromes(), 10_276);
        // A codeword has a zero syndrome, and the zero header encodes to zero.
        let h = [0x40, 0x00, 0, 0, 0, 0];
        let f = fhec::encode(&h);
        let full = [h[0], h[1], h[2], h[3], h[4], h[5], f[0], f[1]];
        assert_eq!(fhec::syndrome(&fhec::symbols(&full)), 0);
        assert_eq!(fhec::encode(&[0; 6]), [0, 0]);
    }
}
