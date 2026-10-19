//! Packet Secondary Header layouts and the split of the Packet Data Field (133.0 §4.1.4;
//! SPP-007, OI-30).
//!
//! The layout is a managed parameter per APID (133.0 Table 5-1). `aldis-spp` keeps no APID
//! table: the caller looks the layout up and passes it to
//! [`SpacePacket::split`](crate::view::SpacePacket::split).

use core::num::NonZeroUsize;

use aldis_core::{Error, ErrorDetail, ErrorKind, FieldId, Result};

use crate::header::{invalid, len_u64};

/// The secondary header of one APID (133.0 §4.1.4.2.1.4–5, SPP-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SecondaryHeaderLayout {
    /// The APID's packets have no secondary header.
    Absent,
    /// A secondary header of a fixed number of octets whose contents the caller interprets.
    Opaque(NonZeroUsize),
}

impl SecondaryHeaderLayout {
    /// Splits a Packet Data Field whose Secondary Header Flag is `flag` (SPP-007).
    ///
    /// # Errors
    /// - `InvalidField` (`secondary_header_flag`) if the flag disagrees with the layout: set
    ///   with [`Absent`](Self::Absent), or clear with a secondary header. 133.0 §4.1.3.3.3.3
    ///   fixes the flag per APID, so this holds in both modes.
    /// - `LengthMismatch` (`secondary_header`) if `data` is shorter than the secondary header.
    pub(crate) fn split(self, flag: bool, data: &[u8]) -> Result<SplitPacket<'_>> {
        let len = match self {
            SecondaryHeaderLayout::Absent => 0,
            SecondaryHeaderLayout::Opaque(len) => len.get(),
        };
        if flag != (len != 0) {
            return Err(invalid("secondary_header_flag"));
        }
        let (secondary_header, user_data) = data.split_at_checked(len).ok_or(
            Error::new(ErrorKind::LengthMismatch)
                .with_field(FieldId::new("secondary_header"))
                .with_detail(ErrorDetail::Expected {
                    expected: len_u64(len),
                    found: len_u64(data.len()),
                }),
        )?;
        Ok(SplitPacket {
            secondary_header,
            user_data,
        })
    }
}

/// A Packet Data Field split by its layout, borrowing from the input (SPP-007, GEN-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitPacket<'a> {
    secondary_header: &'a [u8],
    user_data: &'a [u8],
}

impl<'a> SplitPacket<'a> {
    /// The Packet Secondary Header; empty with [`SecondaryHeaderLayout::Absent`] (133.0
    /// §4.1.4.2, SPP-007).
    #[must_use]
    pub const fn secondary_header(&self) -> &'a [u8] {
        self.secondary_header
    }

    /// The User Data Field; may be empty when a secondary header is present (133.0 §4.1.4.3,
    /// SPP-007).
    #[must_use]
    pub const fn user_data(&self) -> &'a [u8] {
        self.user_data
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::SecondaryHeaderLayout;
    use aldis_core::{ErrorDetail, ErrorKind, FieldId};
    use core::num::NonZeroUsize;

    #[test]
    fn split_rules() {
        // req: SPP-007, OI-30
        let two = SecondaryHeaderLayout::Opaque(NonZeroUsize::new(2).unwrap());
        let s = two.split(true, &[1, 2, 3]).unwrap();
        assert_eq!(
            (s.secondary_header(), s.user_data()),
            (&[1, 2][..], &[3][..])
        );
        assert_eq!(
            two.split(true, &[1, 2]).unwrap().user_data(),
            <&[u8]>::default()
        );
        let s = SecondaryHeaderLayout::Absent.split(false, &[9]).unwrap();
        assert_eq!(
            (s.secondary_header(), s.user_data()),
            (<&[u8]>::default(), &[9][..])
        );
        for (layout, flag) in [(SecondaryHeaderLayout::Absent, true), (two, false)] {
            let e = layout.split(flag, &[1, 2, 3]).unwrap_err();
            assert_eq!(
                (e.kind(), e.field().map(FieldId::name)),
                (ErrorKind::InvalidField, Some("secondary_header_flag"))
            );
        }
        let e = two.split(true, &[1]).unwrap_err();
        assert_eq!(
            (e.kind(), e.field().map(FieldId::name)),
            (ErrorKind::LengthMismatch, Some("secondary_header"))
        );
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 2,
                found: 1
            }
        );
    }
}
