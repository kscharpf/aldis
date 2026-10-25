//! Bit errors in a frame, for the bit-flip completeness suites of the codecs with an FECF
//! (TM, TC, AOS, USLP; M2 plan WP-7, VER-023, VER-064).
//!
//! The suites flip every bit, and every pair of bits, of valid frames, and check the decoder's
//! outcome for each. This module enumerates the errors deterministically, runs the long
//! enumerations on every core of the host, and states the length up to which the CRC-16 of the
//! FECF detects every double-bit error.
//!
//! # The double-bit bound
//!
//! The FECF of all four standards is the CRC-16 with the generating polynomial
//! G(X) = X¹⁶ + X¹² + X⁵ + 1 (132.0 §4.1.6.2.2, 232.0 §4.1.4.2.2, 732.0 §4.1.6.2.2,
//! 732.1 Annex B1.1.3). None of the four books states which errors it is certain to detect, so the
//! bound is computed here from the polynomial, by arithmetic over GF(2) that shares no code with
//! `aldis-core`'s CRC:
//!
//! - The FECF syndrome of a received block is that of its error pattern E(X) times X¹⁶, modulo
//!   G(X) (§4.1.6.3 of 132.0 and 732.0): the preset term cancels, and X is prime to G(X). An
//!   error is undetected exactly when G(X) divides E(X).
//! - X + 1 divides G(X) (G(1) = 0), so G(X) divides no error of odd weight: every such error,
//!   every single-bit error among them, is detected in a block of any length.
//! - A double-bit error at bits i < j is E(X) = Xⁱ(X^(j−i) + 1). G(X) divides it exactly when
//!   the order e of X modulo G(X) divides j − i. [`crc16_order`] computes e = 32 767 (G(X) is
//!   X + 1 times a primitive polynomial of degree 15, whose order is 2¹⁵ − 1).
//! - So every double-bit error is detected in a block of at most e bits, the FECF included, and
//!   in no longer one: [`CRC16_DOUBLE_BIT_BITS`] = 32 767 bits, which is
//!   [`CRC16_DOUBLE_BIT_OCTETS`] = 4 095 whole octets. The longest TM frame (2 048 octets,
//!   131.0 §11.3) and TC frame (1 024 octets, 232.0 §4.1.2.7.3) lie within it; AOS and USLP
//!   frames may be up to 65 536 octets (131.0 §11.3, 732.1 §4.1.2.7), and above 4 095 octets a
//!   double-bit error 32 767 bits apart passes the FECF.
//!
//! The same numbers were computed first with a separate Python program, which
//! `docs/verification/derivations/crc16-bit-flips.md` records.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// The generating polynomial of the FECF, X¹⁶ + X¹² + X⁵ + 1, one bit per coefficient
/// (132.0 §4.1.6.2.2).
pub const CRC16_GENERATOR: u32 = (1 << 16) | (1 << 12) | (1 << 5) | 1;

/// The longest block, in bits and FECF included, in which the CRC-16 detects every double-bit
/// error: the order of X modulo [`CRC16_GENERATOR`] (module docs).
pub const CRC16_DOUBLE_BIT_BITS: usize = 32_767;

/// [`CRC16_DOUBLE_BIT_BITS`] in whole octets: the longest frame in which every double-bit error
/// is detected.
pub const CRC16_DOUBLE_BIT_OCTETS: usize = CRC16_DOUBLE_BIT_BITS / 8;

/// `a` modulo `m`, both polynomials over GF(2) with one bit per coefficient.
const fn poly_mod(mut a: u64, m: u64) -> u64 {
    let degree = m.ilog2();
    while a != 0 && a.ilog2() >= degree {
        a ^= m << (a.ilog2() - degree);
    }
    a
}

/// The order of X modulo `generator`: the least e > 0 with Xᵉ ≡ 1, or `None` if none is found
/// below 2²⁰ (a generator divisible by X has none).
#[must_use]
pub fn order_of_x(generator: u32) -> Option<usize> {
    let m = u64::from(generator);
    let mut power = poly_mod(2, m);
    for e in 1..(1_usize << 20) {
        if power == 1 {
            return Some(e);
        }
        power = poly_mod(power << 1, m);
    }
    None
}

/// The order of X modulo the FECF's generator, computed (module docs).
///
/// # Panics
/// Never: the generator has an order.
#[must_use]
pub fn crc16_order() -> usize {
    order_of_x(CRC16_GENERATOR).expect("G(X) is prime to X")
}

/// Whether the CRC-16 detects every double-bit error in a frame of `octets` octets.
#[must_use]
pub const fn crc16_detects_every_double(octets: usize) -> bool {
    octets <= CRC16_DOUBLE_BIT_OCTETS
}

/// `frame` with bit `bit` inverted; bit 0 is the most significant bit of the first octet, as in
/// the CCSDS bit numbering (132.0 §1.6.2).
///
/// # Panics
/// If `bit` lies outside `frame`.
pub const fn flip(frame: &mut [u8], bit: usize) {
    frame[bit / 8] ^= 0x80 >> (bit % 8);
}

/// Calls `check(bit, flipped)` for every single-bit error of `frame`, in bit order; `flipped` is
/// `frame` with that bit inverted.
pub fn each_single(frame: &[u8], mut check: impl FnMut(usize, &[u8])) {
    let mut work = frame.to_vec();
    for bit in 0..frame.len() * 8 {
        flip(&mut work, bit);
        check(bit, &work);
        flip(&mut work, bit);
    }
}

/// Calls `check(i, j, flipped)` for every double-bit error `i < j` of `frame` whose first bit
/// `i` is in `first`; `flipped` is `frame` with both bits inverted.
pub fn each_double(
    frame: &[u8],
    first: impl IntoIterator<Item = usize>,
    mut check: impl FnMut(usize, usize, &[u8]),
) {
    let bits = frame.len() * 8;
    let mut work = frame.to_vec();
    for i in first {
        flip(&mut work, i);
        for j in i + 1..bits {
            flip(&mut work, j);
            check(i, j, &work);
            flip(&mut work, j);
        }
        flip(&mut work, i);
    }
}

/// The representative double-bit errors of an `n`-bit frame, for the gated runs over long
/// frames.
///
/// First bits: the first two bits of each of the first eight octets (the header), the middle
/// bit, the first bit of the last four octets, the first and last bits of the FECF, and the
/// last bit but one. Second bits, for each first bit `i`: the `near` bits after it, every
/// `stride`-th bit after it, `i` + 32 767 (the separation the CRC-16 misses, module docs), and
/// the 16 bits of the FECF. Sorted, without repeats.
///
/// # Panics
/// If `stride` is 0.
#[must_use]
pub fn sampled_pairs(n: usize, near: usize, stride: usize) -> Vec<(usize, usize)> {
    let mut firsts: Vec<usize> = (0..8).flat_map(|o| [o * 8, o * 8 + 1]).collect();
    firsts.extend([n / 2, n.saturating_sub(32), n.saturating_sub(16)]);
    firsts.extend([n.saturating_sub(1), n.saturating_sub(2)]);
    let mut pairs = Vec::new();
    for &i in firsts.iter().filter(|&&i| i + 1 < n) {
        let close = i + 1..(i + 1 + near).min(n);
        let strided = (i + 1..n).step_by(stride);
        let apart = (i + CRC16_DOUBLE_BIT_BITS..n).take(1);
        let fecf = n.saturating_sub(16).max(i + 1)..n;
        pairs.extend(
            close
                .chain(strided)
                .chain(apart)
                .chain(fecf)
                .map(|j| (i, j)),
        );
    }
    pairs.sort_unstable();
    pairs.dedup();
    pairs
}

/// The representative single-bit errors of an `n`-bit frame, for the gated runs over long
/// frames: each of the first and last `edge` bits, and every `stride`-th bit. Sorted, without
/// repeats.
///
/// # Panics
/// If `stride` is 0.
#[must_use]
pub fn sampled_bits(n: usize, edge: usize, stride: usize) -> Vec<usize> {
    let mut bits: Vec<usize> = (0..edge.min(n))
        .chain(n.saturating_sub(edge)..n)
        .chain((0..n).step_by(stride))
        .collect();
    bits.sort_unstable();
    bits.dedup();
    bits
}

/// Calls `check(bit, flipped)` for each bit of `bits`, as [`each_single`] does.
///
/// # Panics
/// If a bit lies outside `frame`.
pub fn each_bit(frame: &[u8], bits: &[usize], mut check: impl FnMut(usize, &[u8])) {
    let mut work = frame.to_vec();
    for &bit in bits {
        flip(&mut work, bit);
        check(bit, &work);
        flip(&mut work, bit);
    }
}

/// Calls `check(i, j, flipped)` for each pair of `pairs`, as [`each_double`] does.
///
/// # Panics
/// If a bit lies outside `frame`.
pub fn each_pair(
    frame: &[u8],
    pairs: &[(usize, usize)],
    mut check: impl FnMut(usize, usize, &[u8]),
) {
    let mut work = frame.to_vec();
    for &(i, j) in pairs {
        flip(&mut work, i);
        flip(&mut work, j);
        check(i, j, &work);
        flip(&mut work, j);
        flip(&mut work, i);
    }
}

/// [`each_single`] with the bits shared among the host's cores, for the exhaustive single-bit
/// runs over the longest frames (nightly).
///
/// `check` returns `false` for a failure; the run stops at the first failure and returns its
/// bit.
pub fn find_single(frame: &[u8], check: impl Fn(usize, &[u8]) -> bool + Sync) -> Option<usize> {
    run_parallel(frame, false, &|i, _, bytes| check(i, bytes)).map(|(i, _)| i)
}

/// [`each_double`] over every first bit, shared among the host's cores, for the exhaustive
/// double-bit runs over long frames (nightly).
///
/// `check` returns `false` for a failure; the run stops at the first failure and returns its
/// bits.
pub fn find_double(
    frame: &[u8],
    check: impl Fn(usize, usize, &[u8]) -> bool + Sync,
) -> Option<(usize, usize)> {
    run_parallel(frame, true, &check)
}

/// The parallel runs: every single-bit error (`check` gets `j` = `i`), or with `doubles` every
/// double-bit error.
fn run_parallel(
    frame: &[u8],
    doubles: bool,
    check: &(dyn Fn(usize, usize, &[u8]) -> bool + Sync),
) -> Option<(usize, usize)> {
    let bits = frame.len() * 8;
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let failed = AtomicBool::new(false);
    let found = Mutex::new(None);
    std::thread::scope(|s| {
        for t in 0..threads {
            let (failed, found) = (&failed, &found);
            s.spawn(move || {
                let fail = |i, j| {
                    failed.store(true, Ordering::Relaxed);
                    *found.lock().expect("not poisoned") = Some((i, j));
                };
                // Interleaved first bits: the pairs of a low first bit are the most numerous.
                let mut work = frame.to_vec();
                for i in (t..bits).step_by(threads) {
                    if failed.load(Ordering::Relaxed) {
                        return;
                    }
                    flip(&mut work, i);
                    if !doubles && !check(i, i, &work) {
                        return fail(i, i);
                    }
                    for j in (i + 1..bits).filter(|_| doubles) {
                        flip(&mut work, j);
                        if !check(i, j, &work) {
                            return fail(i, j);
                        }
                        flip(&mut work, j);
                    }
                    flip(&mut work, i);
                }
            });
        }
    });
    found.into_inner().expect("not poisoned")
}

/// A deterministic filler: `len` octets of a linear congruential sequence from `seed`, so that
/// the suites' frames hold varied, reproducible data.
#[must_use]
pub fn filler(len: usize, seed: u32) -> Vec<u8> {
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            state.to_be_bytes()[1]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_double_bit_bound_is_the_order_of_x() {
        // req: CMN-001, VER-023, VER-064
        assert_eq!(crc16_order(), CRC16_DOUBLE_BIT_BITS);
        assert_eq!(CRC16_DOUBLE_BIT_OCTETS, 4095);
        // X + 1 divides G(X): its value at X = 1 (the parity of its weight) is 0.
        assert_eq!(CRC16_GENERATOR.count_ones() % 2, 0);
        // The quotient by X + 1 is primitive of degree 15: its order is 2^15 - 1.
        let quotient = 0b1111_0000_0001_1111_u32;
        assert_eq!(poly_mod(u64::from(CRC16_GENERATOR), u64::from(quotient)), 0);
        assert_eq!(order_of_x(quotient), Some((1 << 15) - 1));
        assert_eq!(order_of_x(0b10), None);
        assert!(crc16_detects_every_double(4095) && !crc16_detects_every_double(4096));
    }

    #[test]
    fn errors_are_enumerated_once_each() {
        // req: VER-023, VER-064
        let frame = [0x00, 0xFF];
        let mut singles = Vec::new();
        each_single(&frame, |bit, f| singles.push((bit, f.to_vec())));
        assert_eq!(singles.len(), 16);
        assert_eq!(singles[0].1, [0x80, 0xFF]);
        assert_eq!(singles[15].1, [0x00, 0xFE]);
        let mut doubles = 0;
        each_double(&frame, 0..16, |i, j, f| {
            assert!(i < j);
            let weight: u32 = f.iter().zip(frame).map(|(a, b)| (a ^ b).count_ones()).sum();
            assert_eq!(weight, 2);
            doubles += 1;
        });
        assert_eq!(doubles, 16 * 15 / 2);
        assert_eq!(find_double(&frame, |_, _, _| true), None);
        assert_eq!(
            find_double(&frame, |i, j, _| (i, j) != (3, 9)),
            Some((3, 9))
        );
        assert_eq!(find_single(&frame, |_, _| true), None);
        assert_eq!(
            find_single(&frame, |i, f| i != 5 || f != [0x04, 0xFF]),
            Some(5)
        );
        assert_eq!(filler(4, 7), filler(4, 7));
        assert_ne!(filler(4, 7), filler(4, 8));
    }

    #[test]
    fn sampled_pairs_are_inside_and_ordered() {
        // req: VER-023, VER-064
        let frame = [0x00, 0xFF];
        let pairs = sampled_pairs(16, 32, 97);
        assert!(pairs.windows(2).all(|w| w[0] < w[1]));
        assert!(pairs.iter().all(|&(i, j)| i < j && j < 16));
        assert!(pairs.contains(&(0, 1)) && pairs.contains(&(14, 15)));
        let mut seen = 0;
        each_pair(&frame, &pairs, |_, _, f| {
            assert_ne!(f, frame);
            seen += 1;
        });
        assert_eq!(seen, pairs.len());
        let long = sampled_pairs(16_384, 32, 97);
        assert!(long.len() < 4000, "{}", long.len());
        let longest = sampled_pairs(65_536 * 8, 4, 65_536 * 2);
        assert!(longest.contains(&(0, CRC16_DOUBLE_BIT_BITS)));
        assert!(longest.len() < 600, "{}", longest.len());
        assert_eq!(sampled_bits(32, 2, 10), [0, 1, 10, 20, 30, 31]);
        let mut seen = Vec::new();
        each_bit(&frame, &[3, 12], |bit, f| seen.push((bit, f.to_vec())));
        assert_eq!(seen, [(3, vec![0x10, 0xFF]), (12, vec![0x00, 0xF7])]);
    }
}
