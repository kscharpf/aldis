//! Frame Error Control Field CRC-16 (CMN-001…CMN-004).
//!
//! Defined identically in 132.0-B-3 §4.1.6, 232.0-B-4 §4.1.4, 732.0-B-5 §4.1.6, and 732.1-B-3
//! Annex B: generator G(X) = X¹⁶ + X¹² + X⁵ + 1 (`0x1021`), register preset to all ones, MSB
//! first, no reflection, no final XOR (catalogue name CRC-16/IBM-3740). The FECF is the final
//! 16 bits of the frame, MSB first.
//!
//! The bitwise implementation is the default. Feature `crc-table` selects a 256-entry table in
//! read-only memory; both produce identical results (CMN-004).

use crate::error::{Error, ErrorKind, Result};

const POLY: u16 = 0x1021;
const PRESET: u16 = 0xFFFF;

/// One bitwise CRC step for one octet.
const fn step_bitwise(mut crc: u16, octet: u8) -> u16 {
    crc ^= (octet as u16) << 8;
    let mut bit = 0_u8;
    while bit < 8 {
        crc = if crc & 0x8000 != 0 {
            (crc << 1) ^ POLY
        } else {
            crc << 1
        };
        bit = bit.wrapping_add(1);
    }
    crc
}

/// CRC-16 of `data`, computed bit by bit.
#[must_use]
pub const fn crc16_bitwise(data: &[u8]) -> u16 {
    let mut crc = PRESET;
    let mut rest = data;
    while let [octet, tail @ ..] = rest {
        crc = step_bitwise(crc, *octet);
        rest = tail;
    }
    crc
}

/// Lookup table: the CRC register contribution of each possible leading octet.
#[cfg(feature = "crc-table")]
static TABLE: [u16; 256] = {
    let mut table = [0_u16; 256];
    let mut i = 0_usize;
    while i < 256 {
        #[expect(clippy::cast_possible_truncation, reason = "i < 256 fits in u8")]
        let entry = step_bitwise(0, i as u8);
        #[expect(clippy::indexing_slicing, reason = "i < 256, the table length")]
        {
            table[i] = entry;
        }
        i = i.wrapping_add(1);
    }
    table
};

/// CRC-16 of `data`, computed with the 256-entry table.
#[cfg(feature = "crc-table")]
#[must_use]
pub fn crc16_table(data: &[u8]) -> u16 {
    data.iter().fold(PRESET, |crc, &octet| {
        let index = usize::from((crc >> 8) as u8 ^ octet);
        #[expect(
            clippy::indexing_slicing,
            reason = "a u8 index is always within the 256-entry table"
        )]
        let entry = TABLE[index];
        (crc << 8) ^ entry
    })
}

/// CRC-16 of `data` with the implementation selected at build time (CMN-001, CMN-004).
#[must_use]
pub fn crc16(data: &[u8]) -> u16 {
    #[cfg(feature = "crc-table")]
    {
        crc16_table(data)
    }
    #[cfg(not(feature = "crc-table"))]
    {
        crc16_bitwise(data)
    }
}

/// Splits a frame into the covered octets and its trailing FECF.
fn split_fecf(frame: &[u8]) -> Result<(&[u8], u16)> {
    let body_len = frame.len().checked_sub(2).ok_or(Error::incomplete(2))?;
    let (body, fecf) = frame.split_at(body_len);
    Ok((
        body,
        u16::from_be_bytes([
            fecf.first().copied().unwrap_or(0),
            fecf.get(1).copied().unwrap_or(0),
        ]),
    ))
}

/// Verifies the FECF in the last two octets of `frame` by recomputing the CRC over the rest
/// (CMN-003, method 1).
///
/// # Errors
/// `Incomplete` if `frame` is shorter than 2 octets; `CrcMismatch` if the FECF is wrong.
pub fn verify_fecf(frame: &[u8]) -> Result<()> {
    let (body, fecf) = split_fecf(frame)?;
    if crc16(body) == fecf {
        Ok(())
    } else {
        Err(Error::new(ErrorKind::CrcMismatch))
    }
}

/// The error-detection syndrome over a whole frame including its FECF: zero exactly when no
/// error is detected (132.0 §4.1.6.3; CMN-003, method 2).
#[must_use]
pub fn syndrome(frame: &[u8]) -> u16 {
    crc16(frame)
}

/// Writes the FECF over all but the last two octets into the last two octets of `frame`.
///
/// # Errors
/// `BufferTooSmall` if `frame` is shorter than 2 octets.
pub fn write_fecf(frame: &mut [u8]) -> Result<()> {
    let body_len = frame
        .len()
        .checked_sub(2)
        .ok_or(Error::buffer_too_small(2))?;
    let (body, fecf) = frame.split_at_mut(body_len);
    fecf.copy_from_slice(&crc16(body).to_be_bytes());
    Ok(())
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "test bookkeeping on small bounded values (RS-018)"
)]
mod tests {
    use super::{crc16, crc16_bitwise, syndrome, verify_fecf, write_fecf};
    use crate::error::ErrorKind;
    use proptest::prelude::*;

    #[test]
    fn check_value() {
        // req: CMN-001, CMN-002
        const _: () = assert!(crc16_bitwise(b"123456789") == 0x29B1);
        assert_eq!(crc16_bitwise(b"123456789"), 0x29B1);
        assert_eq!(crc16(b"123456789"), 0x29B1);
        assert_eq!(crc16(&[]), 0xFFFF);
    }

    #[test]
    fn fecf_write_verify_and_syndrome() {
        // req: CMN-003
        let mut frame = *b"123456789\0\0";
        write_fecf(&mut frame).unwrap();
        assert_eq!(&frame[9..], &[0x29, 0xB1]);
        assert_eq!(verify_fecf(&frame), Ok(()));
        assert_eq!(syndrome(&frame), 0);
        frame[0] ^= 0x01;
        assert_eq!(
            verify_fecf(&frame).unwrap_err().kind(),
            ErrorKind::CrcMismatch
        );
        assert_ne!(syndrome(&frame), 0);
        assert_eq!(
            verify_fecf(&[0x00]).unwrap_err().kind(),
            ErrorKind::Incomplete
        );
        assert_eq!(
            write_fecf(&mut [0x00]).unwrap_err().kind(),
            ErrorKind::BufferTooSmall
        );
    }

    proptest! {
        #[test]
        fn both_verification_methods_agree(data in proptest::collection::vec(any::<u8>(), 0..300), flip in any::<u16>()) {
            // req: CMN-003
            let mut frame = data.clone();
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
        fn table_equals_bitwise(data in proptest::collection::vec(any::<u8>(), 0..300)) {
            // req: CMN-004
            prop_assert_eq!(super::crc16_table(&data), crc16_bitwise(&data));
        }
    }
}
