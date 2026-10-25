//! Kani proof harnesses (spike, docs/verification/formal-proofs.md, OI-42).
//!
//! Compiled only by `cargo kani` (`cfg(kani)`); not part of the library, the tests, or the
//! coverage run. Each harness states its bound and why the unwinding depth suffices.

use crate::counter::{Continuity, ModCounter};
use crate::crc::{PRESET, crc16, crc16_bitwise, crc16_update, syndrome, verify_fecf, write_fecf};
use crate::error::ErrorKind;
use crate::{ErrorDetail, bits};

/// Longest CRC input the harnesses cover, in octets.
const CRC_MAX: usize = 8;

/// Reference CRC-16 written from the definition in docs/standards/common-link-features.md §1
/// (132.0 §4.1.6): a 16-bit shift register preset to all ones, fed one message bit at a time,
/// MSB first; when the bit shifted out differs from the message bit, the register is XORed with
/// the generator 0x1021. No reflection, no final XOR. Deliberately a different formulation from
/// `crc::step_bitwise`, which XORs a whole octet into the register first.
fn reference_crc(data: &[u8]) -> u16 {
    let mut reg: u16 = 0xFFFF;
    for &octet in data {
        for i in 0..8 {
            let bit = (octet >> (7 - i)) & 1;
            let top = ((reg >> 15) & 1) as u8;
            reg <<= 1;
            if top ^ bit == 1 {
                reg ^= 0x1021;
            }
        }
    }
    reg
}

/// A symbolic slice of at most `CRC_MAX` octets: any length, any content.
fn any_prefix(buf: &[u8; CRC_MAX]) -> &[u8] {
    let len: usize = kani::any();
    kani::assume(len <= CRC_MAX);
    &buf[..len]
}

/// CRC-16 agrees with the reference for every input of 0–8 octets, and the incremental form
/// agrees with the one-shot form at every split point. Unwind 9: the octet loops run at most
/// 8 times and the bit loops 8 times, plus one for the exit test.
#[kani::proof]
#[kani::unwind(9)]
fn crc16_matches_reference() {
    // req: CMN-001, CMN-004, GEN-020
    let buf: [u8; CRC_MAX] = kani::any();
    let data = any_prefix(&buf);
    let expected = reference_crc(data);
    assert_eq!(crc16_bitwise(data), expected);
    assert_eq!(crc16(data), expected);
    #[cfg(feature = "crc-table")]
    assert_eq!(crate::crc::crc16_table(data), expected);
    let split: usize = kani::any();
    kani::assume(split <= data.len());
    let (head, tail) = data.split_at(split);
    assert_eq!(crc16_update(crc16_update(PRESET, head), tail), expected);
}

/// The two FECF verification methods of CMN-003 agree on every frame of 0–8 octets, and a frame
/// with a written FECF passes both. Unwind 9 as above.
#[kani::proof]
#[kani::unwind(9)]
fn fecf_methods_agree() {
    // req: CMN-003, GEN-020
    let mut buf: [u8; CRC_MAX] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= CRC_MAX);
    let frame = &mut buf[..len];
    match verify_fecf(frame) {
        Ok(()) => assert_eq!(syndrome(frame), 0),
        Err(e) if len < 2 => assert_eq!(e.kind(), ErrorKind::Incomplete),
        Err(e) => {
            assert_eq!(e.kind(), ErrorKind::CrcMismatch);
            assert_ne!(syndrome(frame), 0);
        }
    }
    if write_fecf(frame).is_ok() {
        assert_eq!(verify_fecf(frame), Ok(()));
        assert_eq!(syndrome(frame), 0);
    } else {
        assert!(len < 2);
    }
}

/// The CMN-031 classification, written from the requirement text with the modulus 2^bits as a
/// `u128` so that no step wraps.
fn reference_classify(bits: u8, prev: u64, next: u64) -> Continuity {
    let modulus: u128 = 1 << bits;
    let d = (u128::from(next) + modulus - u128::from(prev)) % modulus;
    if d == 0 {
        Continuity::Repeat
    } else if d == 1 {
        Continuity::InSequence
    } else if d <= modulus / 2 {
        Continuity::Gap {
            missing: (d - 1) as u64,
        }
    } else {
        Continuity::Backwards
    }
}

/// Every counter width 1–64 and every pair of counts: `new` accepts exactly the values that fit,
/// `classify` and `distance_to` agree with the reference, and `next` wraps modulo 2^bits. No
/// loops: every input is covered.
#[kani::proof]
fn counter_classification() {
    // req: CMN-030, CMN-031, GEN-024, GEN-020
    let bits: u8 = kani::any();
    let prev: u64 = kani::any();
    let next: u64 = kani::any();
    let fits = |v: u64| bits >= 64 || v < (1_u64 << bits);
    match ModCounter::new(bits, prev) {
        Err(e) => {
            assert_eq!(e.kind(), ErrorKind::OutOfRange);
            assert!(bits == 0 || bits > 64 || !fits(prev));
        }
        Ok(c) => {
            assert!((1..=64).contains(&bits) && fits(prev));
            assert_eq!((c.bits(), c.value()), (bits, prev));
            let modulus: u128 = 1 << bits;
            assert_eq!(
                u128::from(c.next().value()),
                (u128::from(prev) + 1) % modulus
            );
            match c.classify(next) {
                Err(e) => {
                    assert_eq!(e.kind(), ErrorKind::OutOfRange);
                    assert!(!fits(next));
                    assert!(c.distance_to(next).is_err());
                }
                Ok(class) => {
                    assert!(fits(next));
                    assert_eq!(class, reference_classify(bits, prev, next));
                    let d = (u128::from(next) + modulus - u128::from(prev)) % modulus;
                    assert_eq!(c.distance_to(next).map(u128::from), Ok(d));
                }
            }
        }
    }
}

/// Longest buffer for the bit-field harnesses: a 64-bit field at bit offset 7 spans 9 octets.
const BITS_MAX: usize = 9;

/// Bit `i` of `buf`, bit 0 being the MSB of octet 0 (133.0 §1.6.3), from the definition.
fn bit_at(buf: &[u8], i: usize) -> u64 {
    u64::from((buf[i / 8] >> (7 - i % 8)) & 1)
}

/// `bits::get` and `bits::set` on every buffer of 0–9 octets, every offset, width, and value:
/// both accept exactly the fields inside the buffer with width 1–64 (and `set` the values that
/// fit), `get` reads the field bit by bit as the definition says, `set` then `get` returns the
/// value, `set` leaves every other bit unchanged, and a failed `set` changes nothing. Unwind 73:
/// the reference loops run over at most 9 × 8 = 72 bits; the library folds over at most 9 octets.
#[kani::proof]
#[kani::unwind(73)]
fn bit_fields() {
    // req: GEN-050, GEN-051, GEN-042, GEN-020
    let original: [u8; BITS_MAX] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= BITS_MAX);
    let offset: usize = kani::any();
    let width: u8 = kani::any();
    let value: u64 = kani::any();
    let inside = (1..=64).contains(&width)
        && offset
            .checked_add(usize::from(width))
            .is_some_and(|end| end <= len * 8);

    let got = bits::get(&original[..len], offset, width);
    assert_eq!(got.is_ok(), inside);
    if let Ok(v) = got {
        let mut expected = 0_u64;
        for i in 0..usize::from(width) {
            expected = (expected << 1) | bit_at(&original, offset + i);
        }
        assert_eq!(v, expected);
    }

    let mut buf = original;
    let fits = width >= 64 || (width < 64 && value >> width == 0);
    match bits::set(&mut buf[..len], offset, width, value) {
        Err(e) => {
            assert_eq!(e.kind(), ErrorKind::OutOfRange);
            assert!(!inside || !fits);
            assert_eq!(buf, original);
        }
        Ok(()) => {
            assert!(inside && fits);
            assert_eq!(bits::get(&buf[..len], offset, width), Ok(value));
            for i in 0..BITS_MAX * 8 {
                if i < offset || i >= offset + usize::from(width) {
                    assert_eq!(bit_at(&buf, i), bit_at(&original, i));
                }
            }
        }
    }
}

/// The octet integer helpers on every buffer of 0–9 octets and every offset: they accept exactly
/// the offsets with the integer inside the buffer, read big-endian, and write then read back.
/// Unwind 10: the library copies at most 8 octets.
#[kani::proof]
#[kani::unwind(10)]
fn octet_integers() {
    // req: GEN-050, GEN-051, GEN-020
    let original: [u8; BITS_MAX] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= BITS_MAX);
    let offset: usize = kani::any();
    let value: u32 = kani::any();
    let inside = offset.checked_add(4).is_some_and(|end| end <= len);
    let read = bits::read_u32(&original[..len], offset);
    assert_eq!(read.is_ok(), inside);
    if let Ok(v) = read {
        let o = &original[offset..offset + 4];
        assert_eq!(v, u32::from_be_bytes([o[0], o[1], o[2], o[3]]));
    } else {
        let err = read.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::OutOfRange);
        assert!(matches!(err.detail(), ErrorDetail::Expected { .. }));
    }
    let mut buf = original;
    let written = bits::write_u32(&mut buf[..len], offset, value);
    assert_eq!(written.is_ok(), inside);
    if inside {
        assert_eq!(bits::read_u32(&buf[..len], offset), Ok(value));
    } else {
        assert_eq!(buf, original);
    }
}

/// GEN-022 (loop `error.rs ErrorKind::from_code #1` of the inspection record): the lookup runs
/// over the constant catalogue `ErrorKind::ALL` (17 kinds) for every code. Unwind 18: 17
/// iterations and the exit test; the unwinding assertion fails the proof if any code needed more.
#[kani::proof]
#[kani::unwind(18)]
fn error_kind_lookup_is_bounded() {
    // req: GEN-022, GEN-020
    let code: u16 = kani::any();
    match ErrorKind::from_code(code) {
        Some(kind) => assert_eq!(kind.code(), code),
        None => assert!(ErrorKind::ALL.iter().all(|k| k.code() != code)),
    }
}
