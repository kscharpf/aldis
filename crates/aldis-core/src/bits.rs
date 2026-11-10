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

/// A field of a header of `N` octets at bit `OFFSET`, `WIDTH` bits wide, addressed as [`get`](fn@get)
/// and [`set`] do (GEN-050, GEN-051).
///
/// The three parameters are checked when the code is compiled: a field that is not inside its
/// header, or not 1 to 64 bits wide, does not compile. So the operations have no error to
/// return and no panic path, and the narrowing accessors ([`get_u8`](Self::get_u8) and the
/// others) accept only a field that fits their type. A codec names each field of a header once
/// as a type alias and reads and writes it through these functions. A field of a buffer of
/// unknown length is read with [`get`](fn@get).
///
/// Each operation touches every octet of the header once, with no 128-bit accumulator, so the
/// proof of a header codec stays small (OI-42, VER-059).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field<const N: usize, const OFFSET: usize, const WIDTH: u8>;

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::arithmetic_side_effects,
    reason = "the module's shifts and narrowing casts (VER-001 exception): every shift is below 8 or the width, every cast keeps at most 8 bits, and the loop counter is at most N"
)]
impl<const N: usize, const OFFSET: usize, const WIDTH: u8> Field<N, OFFSET, WIDTH> {
    /// Past the header or of a width outside 1 to 64: a build error where it is used.
    const INSIDE: () = assert!(
        WIDTH >= 1 && WIDTH <= 64 && OFFSET + (WIDTH as usize) <= N * 8,
        "the field is not a 1 to 64 bit field of the header"
    );

    /// The bit range `lo..hi` that octet `k` holds of the field, as the number of bits and the
    /// shift that right-aligns them in the octet; `None` for an octet with none of it.
    const fn part(k: usize) -> Option<(u8, u8)> {
        let () = Self::INSIDE;
        let start = k * 8;
        let lo = if OFFSET > start { OFFSET } else { start };
        let end = OFFSET + WIDTH as usize;
        let hi = if end < start + 8 { end } else { start + 8 };
        if hi > lo {
            Some(((hi - lo) as u8, (start + 8 - hi) as u8))
        } else {
            None
        }
    }

    /// The value of the field, right-aligned (GEN-051).
    #[must_use]
    pub const fn get(header: &[u8; N]) -> u64 {
        let mut acc = 0_u64;
        let mut rest: &[u8] = header;
        let mut k = 0;
        // bound: one step for each of the N octets of the header
        while let [octet, tail @ ..] = rest {
            if let Some((take, shift)) = Self::part(k) {
                let bits = (*octet >> shift) & (0xFF_u8 >> (8 - take));
                acc = (acc << take) | bits as u64;
            }
            rest = tail;
            k += 1;
        }
        acc
    }

    /// The value of a field of at most 8 bits (GEN-051).
    #[must_use]
    pub const fn get_u8(header: &[u8; N]) -> u8 {
        const { assert!(WIDTH <= 8, "the field is wider than 8 bits") };
        Self::get(header) as u8
    }

    /// The value of a field of at most 16 bits (GEN-051).
    #[must_use]
    pub const fn get_u16(header: &[u8; N]) -> u16 {
        const { assert!(WIDTH <= 16, "the field is wider than 16 bits") };
        Self::get(header) as u16
    }

    /// The value of a field of at most 32 bits (GEN-051).
    #[must_use]
    pub const fn get_u32(header: &[u8; N]) -> u32 {
        const { assert!(WIDTH <= 32, "the field is wider than 32 bits") };
        Self::get(header) as u32
    }

    /// Whether a one-bit field is set (GEN-051).
    #[must_use]
    pub const fn flag(header: &[u8; N]) -> bool {
        const { assert!(WIDTH == 1, "the field is not one bit") };
        Self::get(header) != 0
    }

    /// Writes `value` into the field and leaves the other bits as they are; the bits of `value`
    /// above the width are dropped, so a value type that bounds the field comes first
    /// (GEN-051).
    pub const fn put(header: &mut [u8; N], value: u64) {
        let mut rest: &mut [u8] = header;
        let mut k = 0;
        // bound: one step for each of the N octets of the header
        while let [octet, tail @ ..] = rest {
            if let Some((take, shift)) = Self::part(k) {
                let mask = 0xFF_u8 >> (8 - take);
                // The bits of the value that lie below this octet's part of the field.
                let below = OFFSET + WIDTH as usize - (k * 8 + 8 - shift as usize);
                let part = ((value >> below) as u8) & mask;
                *octet = (*octet & !(mask << shift)) | (part << shift);
            }
            rest = tail;
            k += 1;
        }
    }

    /// Writes a value of the type that bounds a field of at most 8 bits (GEN-051).
    pub const fn put_u8(header: &mut [u8; N], value: u8) {
        const { assert!(WIDTH <= 8, "the field is wider than 8 bits") };
        Self::put(header, value as u64);
    }

    /// Writes a value of the type that bounds a field of at most 16 bits (GEN-051).
    pub const fn put_u16(header: &mut [u8; N], value: u16) {
        const { assert!(WIDTH <= 16, "the field is wider than 16 bits") };
        Self::put(header, value as u64);
    }

    /// Writes a value of the type that bounds a field of at most 32 bits (GEN-051).
    pub const fn put_u32(header: &mut [u8; N], value: u32) {
        const { assert!(WIDTH <= 32, "the field is wider than 32 bits") };
        Self::put(header, value as u64);
    }

    /// Writes a one-bit field (GEN-051).
    pub const fn put_flag(header: &mut [u8; N], value: bool) {
        const { assert!(WIDTH == 1, "the field is not one bit") };
        Self::put(header, value as u64);
    }
}

/// The unsigned big-endian integer of `octets`: the last 8 if there are more (GEN-051).
///
/// For a field of whole octets whose number the caller has already fixed, such as the VCF Count
/// of a USLP header.
#[must_use]
#[expect(
    clippy::as_conversions,
    reason = "lossless widening u8 -> u64 in `const` context (VER-001 exception)"
)]
pub const fn be_uint(octets: &[u8]) -> u64 {
    let mut acc = 0_u64;
    let mut rest = octets;
    // bound: one step for each octet of the slice
    while let [octet, tail @ ..] = rest {
        acc = (acc << 8) | *octet as u64;
        rest = tail;
    }
    acc
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
    use super::{Field, get, read_u16, read_u32, read_u64, set, write_u16, write_u32, write_u64};
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
    fn fixed_fields_agree_with_the_checked_primitives() {
        // req: GEN-050, GEN-051
        type Scid = Field<6, 2, 10>;
        type Wide = Field<9, 3, 64>;
        type Bit = Field<2, 15, 1>;
        let h = [0x1B, 0xCD, 0xE5, 0x07, 0xFF, 0x81];
        assert_eq!(Scid::get(&h), get(&h, 2, 10).unwrap());
        assert_eq!(u64::from(Scid::get_u16(&h)), Scid::get(&h));
        let w = [0xA5, 1, 2, 3, 4, 5, 6, 7, 0x9F];
        assert_eq!(Wide::get(&w), get(&w, 3, 64).unwrap());
        assert!(Bit::flag(&[0, 1]));
        assert_eq!(Field::<1, 2, 5>::get_u8(&[0b0011_1100]), 0b11110);
        assert_eq!(Field::<5, 8, 32>::get_u32(&[0, 1, 2, 3, 4]), 0x0102_0304);
        let mut b = [0_u8; 2];
        Bit::put_flag(&mut b, true);
        assert_eq!(b, [0, 1]);
        let mut c = [0_u8; 3];
        Field::<3, 4, 8>::put_u8(&mut c, 0xAB);
        Field::<3, 12, 12>::put_u16(&mut c, 0xCDE);
        assert_eq!(c, [0x0A, 0xBC, 0xDE]);
        let mut d = [0_u8; 5];
        Field::<5, 4, 32>::put_u32(&mut d, 0x0102_0304);
        assert_eq!(d, [0x00, 0x10, 0x20, 0x30, 0x40]);
        macro_rules! round {
            ($offset:literal, $width:literal) => {
                type F = Field<6, $offset, $width>;
                for value in [0_u64, 1, 0x2A5, 0xFFFF_FFFF_FFFF] {
                    let value = value & (u64::MAX >> (64 - $width));
                    let mut a = [0xA5_u8, 0x5A, 0xFF, 0x00, 0x81, 0x7E];
                    let mut c = a;
                    F::put(&mut a, value);
                    set(&mut c, $offset, $width, value).unwrap();
                    assert_eq!(a, c);
                    assert_eq!(F::get(&a), value);
                }
            };
        }
        {
            round!(0, 1);
        }
        {
            round!(3, 7);
        }
        {
            round!(5, 11);
        }
        {
            round!(8, 16);
        }
        {
            round!(13, 3);
        }
        {
            round!(0, 48);
        }
        assert_eq!(super::be_uint(&[]), 0);
        assert_eq!(super::be_uint(&[1, 2, 3]), 0x01_02_03);
        assert_eq!(super::be_uint(&[9, 1, 2, 3, 4, 5, 6, 7, 8]), 0x0102_0304_0506_0708);
        let mut h = [0xFF_u8; 2];
        Field::<2, 4, 4>::put(&mut h, 0x1F5);
        assert_eq!(h, [0xF5, 0xFF], "bits above the width are dropped");
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
