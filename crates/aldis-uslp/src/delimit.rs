//! Frame Delimiting and Fill Data Removal Procedure (732.1 §4.3.11.2; USLP-015, M2 plan D14,
//! D17).
//!
//! When the Synchronization and Channel Coding Sublayer below is TC Synchronization and Channel
//! Coding (§4.3.11.1.6), the input is the octets of one data unit that it delivered error-free
//! (§4.3.11.2.1). [`next_frame`] is stateless: it takes the octets not yet delimited and returns
//! the next item, whose length the caller skips, so the C ABI can wrap it without keeping a
//! pointer (M2 plan D14). [`FrameDelimiter`] iterates over a unit with it.
//!
//! A *valid header* (`docs/standards/uslp-frame.md` §3.2; OI-44 item 3) starts with at least 4
//! octets whose TFVN is `1100`, and either has the End of Frame Primary Header Flag 1, its length
//! being its VC's truncated frame length (Annex D1.3.2; OI-35 item 8), or has the flag 0, at
//! least 7 octets, and a Frame Length that declares at least the Primary Header with its VCF
//! Count, a 1-octet TFDF Header, and one TFDZ octet, on a VC that has no truncated frame length
//! (a VC that has one carries only truncated frames, OI-48). Fill Data of `01010101` octets has TFVN
//! `0101`, so it is never a valid header.

use crate::config::VcLookup;
use crate::header::{PRIMARY_HEADER_LEN, RawRest, RawStart, VERSION};

/// One item of a data unit (732.1 §4.3.11.2.2–4; USLP-015).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Delimited<'a> {
    /// The octets of one frame, as long as its header declares, to be passed to the USLP Frame
    /// Validation Checks (§4.3.11.2.2, §4.3.11.2.4 c).
    Frame(&'a [u8]),
    /// The rest of the unit, discarded: it starts with a valid header that declares more octets
    /// than remain (§4.3.11.2.3).
    IncompleteFrame {
        /// The discarded octets: all that remained.
        octets: &'a [u8],
        /// The frame length the header declares, or its VC's truncated frame length.
        declared_len: usize,
    },
    /// The rest of the unit, discarded: a truncated header whose SCID and VCID name no
    /// configured VC, or a VC without a truncated frame length (USLP-015; OI-35 item 8).
    NoTruncatedLength(&'a [u8]),
    /// The rest of the unit, discarded: a non-truncated header on a configured VC that has a
    /// truncated frame length and so carries only truncated frames, which a bit error in the
    /// End of Frame Primary Header Flag of a truncated frame makes (USLP-002, USLP-015; OI-48).
    /// The Frame Length of such a header is not trusted.
    NonTruncatedNotAllowed(&'a [u8]),
    /// The rest of the unit, discarded as Fill Data: it does not start with a valid header
    /// (§4.3.11.2.4 a–b).
    Fill(&'a [u8]),
}

impl<'a> Delimited<'a> {
    /// The octets of the item (USLP-015).
    #[must_use]
    pub const fn octets(&self) -> &'a [u8] {
        match *self {
            Delimited::Frame(octets)
            | Delimited::IncompleteFrame { octets, .. }
            | Delimited::NoTruncatedLength(octets)
            | Delimited::NonTruncatedNotAllowed(octets)
            | Delimited::Fill(octets) => octets,
        }
    }

    /// How many octets of the unit the item takes; never 0 (USLP-015).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.octets().len()
    }

    /// Always `false`: every item takes at least one octet (USLP-015).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }
}

/// What the header at the start of the octets declares (§3.2 of the standard document).
enum Declared {
    /// A frame of this many octets.
    Len(usize),
    /// A truncated header whose VC is unknown or has no truncated frame length.
    NoTruncatedLength,
    /// A non-truncated header on a VC that has a truncated frame length (OI-48).
    TruncatedOnly,
}

/// What the header at the start of `rest` declares, or `None` for no valid header.
fn declared_len<L: VcLookup + ?Sized>(rest: &[u8], channels: &L) -> Option<Declared> {
    let start = RawStart::parse(rest).filter(|s| s.version == VERSION)?;
    let ch = start.channel;
    let truncated_len = channels
        .vc(ch.spacecraft_id, ch.vc_id)
        .and_then(|vc| vc.truncated_frame_len())
        .map(crate::config::TruncatedFrameLen::get);
    if start.truncated {
        return Some(truncated_len.map_or(Declared::NoTruncatedLength, Declared::Len));
    }
    let raw = RawRest::parse(rest)?;
    let shortest = PRIMARY_HEADER_LEN
        .saturating_add(raw.vcf_count_len.octets())
        .saturating_add(2);
    if raw.frame_len < shortest {
        return None;
    }
    Some(if truncated_len.is_some() {
        Declared::TruncatedOnly
    } else {
        Declared::Len(raw.frame_len)
    })
}

/// The next item of the octets `rest` of a data unit that are not yet delimited, or `None` when
/// none remain (732.1 §4.3.11.2.2–4; USLP-015, M2 plan D14).
///
/// A [`Delimited::Frame`] is followed by the octets after it; every other item takes all of
/// `rest`.
#[must_use]
pub fn next_frame<'a, L: VcLookup + ?Sized>(rest: &'a [u8], channels: &L) -> Option<Delimited<'a>> {
    if rest.is_empty() {
        return None;
    }
    let item = match declared_len(rest, channels) {
        Some(Declared::Len(len)) => match rest.split_at_checked(len) {
            Some((frame, _)) => Delimited::Frame(frame),
            None => Delimited::IncompleteFrame {
                octets: rest,
                declared_len: len,
            },
        },
        Some(Declared::NoTruncatedLength) => Delimited::NoTruncatedLength(rest),
        Some(Declared::TruncatedOnly) => Delimited::NonTruncatedNotAllowed(rest),
        None => Delimited::Fill(rest),
    };
    Some(item)
}

/// An iterator over the frames and discarded octets of one data unit (732.1 §4.3.11.2;
/// USLP-015).
#[derive(Debug)]
pub struct FrameDelimiter<'a, 'c, L: VcLookup + ?Sized> {
    rest: &'a [u8],
    channels: &'c L,
}

impl<'a, 'c, L: VcLookup + ?Sized> FrameDelimiter<'a, 'c, L> {
    /// A delimiter over the octets of one error-free data unit, with the channels whose
    /// truncated frame lengths delimit truncated frames (732.1 §4.3.11.2.1; USLP-015).
    #[must_use]
    pub const fn new(unit: &'a [u8], channels: &'c L) -> FrameDelimiter<'a, 'c, L> {
        FrameDelimiter {
            rest: unit,
            channels,
        }
    }

    /// The octets not yet delimited (USLP-015).
    #[must_use]
    pub const fn remaining(&self) -> &'a [u8] {
        self.rest
    }
}

impl<'a, L: VcLookup + ?Sized> Iterator for FrameDelimiter<'a, '_, L> {
    type Item = Delimited<'a>;

    fn next(&mut self) -> Option<Delimited<'a>> {
        let item = next_frame(self.rest, self.channels)?;
        self.rest = self.rest.get(item.len()..).unwrap_or_default();
        Some(item)
    }
}

impl<L: VcLookup + ?Sized> core::iter::FusedIterator for FrameDelimiter<'_, '_, L> {}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Delimited, FrameDelimiter, next_frame};
    use crate::config::tests::{plain_variable, variable};

    /// A 10-octet frame on VC 8 MAP 0 of the variable channel: rule 111, TFDZ `AA`, FECF.
    const FRAME: [u8; 10] = [0xC0, 0x15, 0x51, 0x00, 0x00, 0x09, 0x00, 0xE0, 0xAA, 0x12];

    /// A 10-octet non-truncated frame on VC 4 MAP 0, which has a truncated frame length.
    const FRAME_VC4: [u8; 10] = [0xC0, 0x15, 0x50, 0x80, 0x00, 0x09, 0x00, 0xE0, 0xAA, 0x12];

    #[test]
    fn frames_incomplete_and_fill() {
        // req: USLP-015
        let t = variable();
        assert_eq!(next_frame(&[], &t), None);
        let truncated = [0xC0, 0x15, 0x58, 0x85, 0xE5, 1, 2, 3, 4, 5, 6, 7];
        let mut unit = FRAME.to_vec();
        unit.extend_from_slice(&truncated);
        unit.extend_from_slice(&FRAME[..9]);
        let mut d = FrameDelimiter::new(&unit, &t);
        assert_eq!(d.next(), Some(Delimited::Frame(&FRAME)));
        assert_eq!(d.remaining().len(), 21);
        let second = d.next().unwrap();
        assert_eq!(
            (second.octets(), second.len(), second.is_empty()),
            (&truncated[..], 12, false)
        );
        assert_eq!(
            d.next(),
            Some(Delimited::IncompleteFrame {
                octets: &FRAME[..9],
                declared_len: 10
            })
        );
        assert_eq!((d.next(), d.next(), d.remaining()), (None, None, &[][..]));
    }

    #[test]
    fn what_is_fill_and_what_has_no_length() {
        // req: USLP-015
        let t = variable();
        for fill in [
            &[0x55_u8; 3][..],
            &[0x55; 8],
            &[0xC0, 0x15, 0x51, 0x00, 0x00, 0x07, 0x00, 0xE0],
            &[0xC0, 0x15, 0x51, 0x00, 0x00, 0x09],
            &[0xC0, 0x15, 0x51, 0x00, 0x00, 0x08, 0x01, 0xE0, 0, 0],
            &[0xD0, 0x15, 0x50, 0x81],
        ] {
            assert_eq!(
                next_frame(fill, &t),
                Some(Delimited::Fill(fill)),
                "{fill:02X?}"
            );
        }
        for none in [
            &[0xC0_u8, 0x15, 0x50, 0xC1, 0][..],
            &[0xC0, 0x16, 0x50, 0x81],
        ] {
            let item = next_frame(none, &t);
            assert_eq!(
                item,
                Some(Delimited::NoTruncatedLength(none)),
                "{none:02X?}"
            );
            assert_eq!(item.map(|i| i.octets()), Some(none));
        }
    }

    #[test]
    fn a_vc_with_a_truncated_length_carries_only_truncated_frames() {
        // req: USLP-015, USLP-002
        let t = variable();
        // A non-truncated header on VC 4, which has a truncated frame length: its Frame Length is
        // not trusted, so the rest of the unit is discarded.
        let mut unit = FRAME_VC4.to_vec();
        unit.extend_from_slice(&FRAME);
        assert_eq!(
            next_frame(&unit, &t),
            Some(Delimited::NonTruncatedNotAllowed(&unit))
        );
        let item = next_frame(&FRAME_VC4, &t).unwrap();
        assert_eq!((item.len(), item.octets()), (10, &FRAME_VC4[..]));
        // The same header with a Frame Length too short to be valid is Fill, as on any VC.
        let short = [0xC0, 0x15, 0x50, 0x80, 0x00, 0x07, 0x00, 0xE0];
        assert_eq!(next_frame(&short, &t), Some(Delimited::Fill(&short)));
        // A channel without truncated frame lengths delimits the same octets as a frame.
        let p = plain_variable();
        assert_eq!(
            next_frame(&FRAME_VC4, &p),
            Some(Delimited::Frame(&FRAME_VC4))
        );
        // An unknown VC is not refused here: the validation checks reject it (reason 5).
        let unknown = [0xC0, 0x15, 0x50, 0x60, 0x00, 0x09, 0x00, 0xE0, 0xAA, 0x12];
        assert_eq!(next_frame(&unknown, &t), Some(Delimited::Frame(&unknown)));
    }
}
