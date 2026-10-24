//! Frame Delimiting and Fill Removal Procedure (232.0 §4.4.9.2; TC-020, M2 plan D14).
//!
//! The input is the octets of one data unit that the Channel Coding Sublayer declared clean,
//! for example the decoded codeblocks of one CLTU (§4.4.9.2.3). A decoding failure discards
//! every frame of the CLTU (232.0 Cor. 1 §6.5.2.2), which the caller does by never passing a
//! unit that failed. [`next_frame`] is stateless: it takes the octets not yet delimited and
//! returns the next item, whose length the caller skips, so the C ABI can wrap it without
//! keeping a pointer (M2 plan D14). [`FrameDelimiter`] iterates over a unit with it.
//!
//! A *valid header* starts with five octets whose Transfer Frame Version Number is `00` and
//! whose Frame Length declares a frame of at least six octets (232.0 §4.1.1, §4.1.2.2.2,
//! §4.1.2.7). Fill Data of `01010101` octets (§4.4.9.2.6 note 2) has version `01`, so it is
//! never a valid header.

use crate::header::{MIN_FRAME_LEN, RawHeader};

/// One item of a data unit (232.0 §4.4.9.2.4–6; TC-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Delimited<'a> {
    /// The octets of one frame, as long as its Frame Length declares, to be passed to the
    /// Frame Validation Checks (§4.4.9.2.4, §4.4.9.2.6 c).
    Frame(&'a [u8]),
    /// The rest of the unit, discarded: it starts with a valid header that declares more
    /// octets than remain (§4.4.9.2.5).
    TruncatedFrame {
        /// The discarded octets: all that remained.
        octets: &'a [u8],
        /// The frame length the header declares.
        declared_len: usize,
    },
    /// The rest of the unit, discarded as Fill Data: it does not start with a valid header
    /// (§4.4.9.2.6 a–b).
    Fill(&'a [u8]),
}

impl<'a> Delimited<'a> {
    /// The octets of the item (TC-020).
    #[must_use]
    pub const fn octets(&self) -> &'a [u8] {
        match *self {
            Delimited::Frame(octets)
            | Delimited::TruncatedFrame { octets, .. }
            | Delimited::Fill(octets) => octets,
        }
    }

    /// How many octets of the unit the item takes; never 0 (TC-020).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.octets().len()
    }

    /// Always `false`: every item takes at least one octet (TC-020).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }
}

/// The next item of the octets `rest` of a data unit that are not yet delimited, or `None` when
/// none remain (232.0 §4.4.9.2.4–6; TC-020, M2 plan D14).
///
/// A [`Delimited::Frame`] is followed by the octets after it; a [`TruncatedFrame`] or
/// [`Fill`] takes all of `rest`.
///
/// [`TruncatedFrame`]: Delimited::TruncatedFrame
/// [`Fill`]: Delimited::Fill
#[must_use]
pub fn next_frame(rest: &[u8]) -> Option<Delimited<'_>> {
    if rest.is_empty() {
        return None;
    }
    let item = match RawHeader::parse(rest) {
        Some(h) if h.version == 0 && h.frame_len >= MIN_FRAME_LEN => {
            match rest.split_at_checked(h.frame_len) {
                Some((frame, _)) => Delimited::Frame(frame),
                None => Delimited::TruncatedFrame {
                    octets: rest,
                    declared_len: h.frame_len,
                },
            }
        }
        _ => Delimited::Fill(rest),
    };
    Some(item)
}

/// An iterator over the frames and discarded octets of one data unit (232.0 §4.4.9.2; TC-020).
///
/// ```
/// use aldis_tc::delimit::{Delimited, FrameDelimiter};
///
/// // An Unlock frame without FECF, then three octets of fill.
/// let unit = [0x31, 0x55, 0x04, 0x05, 0x00, 0x00, 0x55, 0x55, 0x55];
/// let items: Vec<_> = FrameDelimiter::new(&unit).collect();
/// assert_eq!(items, [Delimited::Frame(&unit[..6]), Delimited::Fill(&unit[6..])]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameDelimiter<'a> {
    rest: &'a [u8],
}

impl<'a> FrameDelimiter<'a> {
    /// A delimiter over the octets of one clean data unit (232.0 §4.4.9.2.3; TC-020).
    #[must_use]
    pub const fn new(unit: &'a [u8]) -> FrameDelimiter<'a> {
        FrameDelimiter { rest: unit }
    }

    /// The octets not yet delimited (TC-020).
    #[must_use]
    pub const fn remaining(&self) -> &'a [u8] {
        self.rest
    }
}

impl<'a> Iterator for FrameDelimiter<'a> {
    type Item = Delimited<'a>;

    fn next(&mut self) -> Option<Delimited<'a>> {
        let item = next_frame(self.rest)?;
        self.rest = self.rest.get(item.len()..).unwrap_or_default();
        Some(item)
    }
}

impl core::iter::FusedIterator for FrameDelimiter<'_> {}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Delimited, FrameDelimiter, next_frame};

    const UNLOCK: [u8; 6] = [0x31, 0x55, 0x04, 0x05, 0x00, 0x00];

    #[test]
    fn frames_truncation_and_fill() {
        // req: TC-020
        assert_eq!(next_frame(&[]), None);
        let mut unit = UNLOCK.to_vec();
        unit.extend_from_slice(&UNLOCK);
        unit.extend_from_slice(&[0x01, 0x55, 0x04, 0x09, 0x00, 0xAA]);
        let mut d = FrameDelimiter::new(&unit);
        assert_eq!(d.next(), Some(Delimited::Frame(&UNLOCK)));
        assert_eq!(d.remaining().len(), 12);
        let second = d.next().unwrap();
        assert_eq!(
            (second.octets(), second.len(), second.is_empty()),
            (&UNLOCK[..], 6, false)
        );
        let last = d.next().unwrap();
        assert_eq!(
            last,
            Delimited::TruncatedFrame {
                octets: &unit[12..],
                declared_len: 10
            }
        );
        assert_eq!((d.next(), d.next(), d.remaining()), (None, None, &[][..]));
    }

    #[test]
    fn what_is_fill() {
        // req: TC-020
        for fill in [
            &[0x55_u8; 4][..],
            &[0x55; 6],
            &[0x01, 0x55, 0x04, 0x04, 0x00, 0x00],
            &[0x00; 5],
            &[0xC1, 0x55, 0x04, 0x05, 0x00, 0x00],
        ] {
            assert_eq!(next_frame(fill), Some(Delimited::Fill(fill)), "{fill:02X?}");
        }
    }
}
