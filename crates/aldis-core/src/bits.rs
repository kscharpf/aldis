//! Big-endian bit-field read/write primitives (GEN-050, GEN-051).
//!
//! CCSDS convention (e.g. 133.0-B-2 §1.6.3): Bit 0 of a field is the first transmitted bit
//! and the most significant bit; multi-octet fields are big-endian on the wire. All codecs
//! access wire fields through this module, the only place in `aldis-core` where shifts and
//! integer casts are used (VER-001 exception, each justified with `#[expect]`).
//!
//! A field is addressed by its bit offset from the start of the buffer and its width in bits
//! (1–64). Every function is total: out-of-range arguments return [`ErrorKind::OutOfRange`](crate::error::ErrorKind::OutOfRange)
//! and never panic (GEN-020).

use crate::error::{Error, Result, as_u64};
use crate::field;

/// Octets covered by a field, and the shift that right-aligns it.
struct Span {
    first: usize,
    last: usize,
    shift: u32,
    mask: u64,
}

impl Span {
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "`span_bits` is at most 72 (a 64-bit field spans at most 9 octets), so it fits in u32"
    )]
    fn new(buf_len: usize, bit_offset: usize, width: u8) -> Result<Span> {
        if width == 0 {
            return Err(Error::out_of_range(field::WIDTH, 1, 0));
        }
        if width > 64 {
            return Err(Error::out_of_range(field::WIDTH, 64, u64::from(width)));
        }
        let total_bits = buf_len.saturating_mul(8);
        let width_bits = usize::from(width);
        let end_bit = bit_offset
            .checked_add(width_bits)
            .filter(|&end| end <= total_bits)
            .ok_or_else(|| {
                if let Some(max_offset) = total_bits.checked_sub(width_bits) {
                    Error::out_of_range(field::BIT_OFFSET, as_u64(max_offset), as_u64(bit_offset))
                } else {
                    Error::out_of_range(field::BUF_BITS, as_u64(width_bits), as_u64(total_bits))
                }
            })?;
        // end_bit >= width >= 1 and end_bit <= total_bits, so all of these are in range.
        let first = bit_offset >> 3;
        let last = end_bit.wrapping_sub(1) >> 3;
        let span_bits = last.wrapping_sub(first).wrapping_add(1).wrapping_mul(8);
        let lead = bit_offset & 7;
        let shift = span_bits
            .wrapping_sub(lead)
            .wrapping_sub(usize::from(width)) as u32;
        let mask = u64::MAX >> (64_u8.wrapping_sub(width));
        Ok(Span {
            first,
            last,
            shift,
            mask,
        })
    }
}

/// Reads the `width`-bit field at `bit_offset` as an unsigned value.
///
/// # Errors
/// `OutOfRange` if `width` is not 1–64 or the field extends past the buffer.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "the value is masked to `width` <= 64 bits before narrowing to u64"
)]
pub fn get(buf: &[u8], bit_offset: usize, width: u8) -> Result<u64> {
    let span = Span::new(buf.len(), bit_offset, width)?;
    let octets = buf.get(span.first..=span.last).unwrap_or_default();
    let acc = octets
        .iter()
        .fold(0_u128, |acc, &b| (acc << 8) | u128::from(b));
    Ok((acc >> span.shift) as u64 & span.mask)
}

/// Writes `value` into the `width`-bit field at `bit_offset`, leaving all other bits unchanged.
///
/// # Errors
/// `OutOfRange` if `width` is not 1–64, the field extends past the buffer, or `value` does not
/// fit in `width` bits. On error the buffer is unchanged (GEN-042).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "each octet written is the low 8 bits of the accumulator, extracted deliberately"
)]
pub fn set(buf: &mut [u8], bit_offset: usize, width: u8, value: u64) -> Result<()> {
    let span = Span::new(buf.len(), bit_offset, width)?;
    if value & !span.mask != 0 {
        return Err(Error::out_of_range(field::VALUE, span.mask, value));
    }
    let octets = buf.get_mut(span.first..=span.last).unwrap_or_default();
    let acc = octets
        .iter()
        .fold(0_u128, |acc, &b| (acc << 8) | u128::from(b));
    let field_mask = u128::from(span.mask) << span.shift;
    let mut acc = (acc & !field_mask) | (u128::from(value) << span.shift);
    for octet in octets.iter_mut().rev() {
        *octet = acc as u8;
        acc >>= 8;
    }
    Ok(())
}

const fn octet_err(buf_len: usize, offset: usize, width: usize) -> Error {
    if let Some(max_offset) = buf_len.checked_sub(width) {
        Error::out_of_range(field::OFFSET, as_u64(max_offset), as_u64(offset))
    } else {
        Error::out_of_range(field::BUF_LEN, as_u64(width), as_u64(buf_len))
    }
}

macro_rules! octet_int {
    ($read_doc:literal, $read:ident, $write_doc:literal, $write:ident, $ty:ty, $n:literal) => {
        #[doc = $read_doc]
        ///
        /// # Errors
        /// `OutOfRange` if the octets are not inside the buffer.
        pub fn $read(buf: &[u8], offset: usize) -> Result<$ty> {
            let bytes = offset
                .checked_add($n)
                .and_then(|end| buf.get(offset..end))
                .and_then(|s| <[u8; $n]>::try_from(s).ok())
                .ok_or_else(|| octet_err(buf.len(), offset, $n))?;
            Ok(<$ty>::from_be_bytes(bytes))
        }

        #[doc = $write_doc]
        ///
        /// # Errors
        /// `OutOfRange` if the octets are not inside the buffer.
        pub fn $write(buf: &mut [u8], offset: usize, value: $ty) -> Result<()> {
            let len = buf.len();
            let dest = offset
                .checked_add($n)
                .and_then(|end| buf.get_mut(offset..end))
                .ok_or_else(|| octet_err(len, offset, $n))?;
            dest.copy_from_slice(&value.to_be_bytes());
            Ok(())
        }
    };
}

octet_int!(
    "Reads a big-endian `u16` starting at octet `offset` (GEN-050, GEN-051).",
    read_u16,
    "Writes a big-endian `u16` starting at octet `offset` (GEN-050, GEN-051).",
    write_u16,
    u16,
    2
);
octet_int!(
    "Reads a big-endian `u32` starting at octet `offset` (GEN-050, GEN-051).",
    read_u32,
    "Writes a big-endian `u32` starting at octet `offset` (GEN-050, GEN-051).",
    write_u32,
    u32,
    4
);
octet_int!(
    "Reads a big-endian `u64` starting at octet `offset` (GEN-050, GEN-051).",
    read_u64,
    "Writes a big-endian `u64` starting at octet `offset` (GEN-050, GEN-051).",
    write_u64,
    u64,
    8
);

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{get, read_u16, read_u32, read_u64, set, write_u16, write_u32, write_u64};
    use crate::error::{ErrorDetail, ErrorKind};
    use crate::field;

    #[test]
    fn space_packet_header_fields() {
        // req: GEN-050, GEN-051
        let h = [0x18, 0x01, 0xC0, 0x00, 0x00, 0x00];
        assert_eq!(get(&h, 0, 3), Ok(0));
        assert_eq!(get(&h, 3, 1), Ok(1));
        assert_eq!(get(&h, 4, 1), Ok(1));
        assert_eq!(get(&h, 5, 11), Ok(1));
        assert_eq!(get(&h, 16, 2), Ok(3));
    }

    #[test]
    fn rejects_bad_arguments_without_writing() {
        // req: GEN-020, GEN-042
        let mut b = [0xAA_u8; 2];
        for (offset, width, value) in [
            (0, 0, 0),
            (0, 65, 0),
            (9, 8, 0),
            (usize::MAX, 8, 0),
            (0, 4, 16),
        ] {
            let e = set(&mut b, offset, width, value).unwrap_err();
            assert_eq!(e.kind(), ErrorKind::OutOfRange);
            assert_eq!(b, [0xAA, 0xAA]);
        }
        assert_eq!(
            get(&b, 0, 0).unwrap_err().detail(),
            ErrorDetail::Expected {
                expected: 1,
                found: 0
            }
        );
        assert_eq!(
            get(&b, 0, 65).unwrap_err().detail(),
            ErrorDetail::Expected {
                expected: 64,
                found: 65
            }
        );
        assert_eq!(
            get(&b, 9, 8).unwrap_err().detail(),
            ErrorDetail::Expected {
                expected: 8,
                found: 9
            }
        );
        let short_bits = get(&[0], 0, 16).unwrap_err();
        assert_eq!(short_bits.field(), Some(field::BUF_BITS));
        assert_eq!(
            short_bits.detail(),
            ErrorDetail::Expected {
                expected: 16,
                found: 8
            }
        );
        assert_eq!(
            get(&b, usize::MAX, 1).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
        assert_eq!(get(&[], 0, 1).unwrap_err().kind(), ErrorKind::OutOfRange);
    }

    #[test]
    fn octet_helpers() {
        // req: GEN-050, GEN-051
        let mut b = [0_u8; 9];
        write_u16(&mut b, 1, 0xABCD).unwrap();
        assert_eq!(&b[..3], &[0x00, 0xAB, 0xCD]);
        assert_eq!(read_u16(&b, 1), Ok(0xABCD));
        let e16 = read_u16(&b, 8).unwrap_err();
        assert_eq!(e16.kind(), ErrorKind::OutOfRange);
        assert_eq!(
            e16.detail(),
            ErrorDetail::Expected {
                expected: 7,
                found: 8
            }
        );
        assert_eq!(
            write_u16(&mut b, usize::MAX, 0).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );

        write_u32(&mut b, 1, 0x1234_5678).unwrap();
        assert_eq!(read_u32(&b, 1), Ok(0x1234_5678));
        assert_eq!(read_u32(&b, 6).unwrap_err().kind(), ErrorKind::OutOfRange);
        let short32 = read_u32(&[0; 2], 0).unwrap_err();
        assert_eq!(short32.kind(), ErrorKind::OutOfRange);
        assert_eq!(
            short32.detail(),
            ErrorDetail::Expected {
                expected: 4,
                found: 2
            }
        );
        assert_eq!(
            write_u32(&mut [0; 2], 0, 0).unwrap_err().detail(),
            ErrorDetail::Expected {
                expected: 4,
                found: 2
            }
        );
        assert_eq!(
            write_u32(&mut b, usize::MAX, 0).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );

        write_u64(&mut b, 1, 0x0123_4567_89AB_CDEF).unwrap();
        assert_eq!(read_u64(&b, 1), Ok(0x0123_4567_89AB_CDEF));
        assert_eq!(read_u64(&b, 2).unwrap_err().kind(), ErrorKind::OutOfRange);
        assert_eq!(
            write_u64(&mut b, usize::MAX, 0).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
    }
}
