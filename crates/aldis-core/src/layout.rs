//! Frame field layout calculator (CMN-040, CMN-041).
//!
//! All four data link protocols place their fields in the same order, so one calculator
//! serves them all (132.0 §4.1, §6.3; 232.0 §4.1, §6.3; 732.0 §4.1, §6.3; 732.1 §4.1, §6.3):
//!
//! ```text
//! primary header ‖ secondary ‖ security header ‖ data ‖ security trailer ‖ OCF ‖ FECF
//! ```
//!
//! The *secondary* slot is protocol-specific: the TM Transfer Frame Secondary Header, the AOS
//! or USLP Insert Zone, or the TC Segment Header. TC frames have no OCF.

use core::ops::Range;

use crate::error::{Error, ErrorKind, Result};
use crate::field;
use crate::security::SecurityLayout;

/// Length of the Operational Control Field, in octets.
pub const OCF_LEN: usize = 4;
/// Length of the Frame Error Control Field, in octets.
pub const FECF_LEN: usize = 2;

/// Inputs to the layout of one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayoutSpec {
    /// Total frame length in octets.
    pub frame_len: usize,
    /// Primary header length in octets.
    pub primary_header_len: usize,
    /// Length of the protocol-specific secondary slot (FSH, Insert Zone, or Segment Header).
    pub secondary_len: usize,
    /// Security Header and Trailer lengths (API-070).
    pub security: SecurityLayout,
    /// Whether the OCF is present.
    pub ocf: bool,
    /// Whether the FECF is present.
    pub fecf: bool,
}

impl LayoutSpec {
    /// The octets of every field except the data field: the primary header, the secondary slot,
    /// the Security Header and Trailer, the OCF, and the FECF (CMN-040). The frame length does
    /// not matter.
    #[must_use]
    pub const fn overhead(&self) -> usize {
        self.primary_header_len
            .saturating_add(self.secondary_len)
            .saturating_add(self.security.header_len())
            .saturating_add(self.security.trailer_len())
            .saturating_add(if self.ocf { OCF_LEN } else { 0 })
            .saturating_add(if self.fecf { FECF_LEN } else { 0 })
    }

    /// The same spec for a frame whose data field holds `data_len` octets: its frame length is
    /// [`overhead`](Self::overhead) plus `data_len` (CMN-040, issue #154).
    #[must_use]
    pub const fn with_data_len(self, data_len: usize) -> LayoutSpec {
        LayoutSpec {
            frame_len: self.overhead().saturating_add(data_len),
            ..self
        }
    }

    /// Checks that a frame of this spec has room for data: after a data-field header of
    /// `data_header_len` octets (0 for none), at least one octet is left (CMN-041, API-070).
    ///
    /// # Errors
    /// `InvalidConfig` (`frame_data_field`) if the other fields, and the header, leave no
    /// octet for the data.
    pub fn check_room(&self, data_header_len: usize) -> Result<()> {
        FrameLayout::compute(self)?
            .with_data_header(data_header_len)
            .map(|_| ())
    }
}

/// Contiguous, non-overlapping octet ranges covering a frame exactly, in wire order (CMN-040).
///
/// Built only by [`FrameLayout::compute`], so holding one proves the ranges start at 0, follow
/// each other without gaps, and leave a non-empty data field (CMN-041, RS-020).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FrameLayout {
    primary_header: Range<usize>,
    secondary: Range<usize>,
    security_header: Range<usize>,
    data: Range<usize>,
    data_header_len: usize,
    security_trailer: Range<usize>,
    ocf: Range<usize>,
    fecf: Range<usize>,
}

const fn take(at: &mut usize, len: usize) -> Range<usize> {
    let start = *at;
    *at = start.saturating_add(len);
    start..*at
}

impl FrameLayout {
    /// Computes the layout.
    ///
    /// # Errors
    /// `InvalidConfig` (`field::FRAME_DATA_FIELD`) if the other fields leave no octet for the data field
    /// (CMN-041).
    pub fn compute(spec: &LayoutSpec) -> Result<FrameLayout> {
        let trailer_len = spec
            .security
            .trailer_len()
            .saturating_add(if spec.ocf { OCF_LEN } else { 0 })
            .saturating_add(if spec.fecf { FECF_LEN } else { 0 });
        let head_len = spec
            .primary_header_len
            .saturating_add(spec.secondary_len)
            .saturating_add(spec.security.header_len());
        let data_len = spec
            .frame_len
            .checked_sub(head_len)
            .and_then(|n| n.checked_sub(trailer_len))
            .filter(|&n| n > 0)
            .ok_or(Error::new(ErrorKind::InvalidConfig).with_field(field::FRAME_DATA_FIELD))?;
        let mut at = 0;
        Ok(FrameLayout {
            primary_header: take(&mut at, spec.primary_header_len),
            secondary: take(&mut at, spec.secondary_len),
            security_header: take(&mut at, spec.security.header_len()),
            data: take(&mut at, data_len),
            data_header_len: 0,
            security_trailer: take(&mut at, spec.security.trailer_len()),
            ocf: take(&mut at, if spec.ocf { OCF_LEN } else { 0 }),
            fecf: take(&mut at, if spec.fecf { FECF_LEN } else { 0 }),
        })
    }

    /// Computes the layout of a frame whose data field holds `data_len` octets, with the other
    /// fields as `spec` says (its frame length is ignored; CMN-040, issue #154).
    ///
    /// # Errors
    /// `InvalidConfig` (`frame_data_field`) if `data_len` is 0 (CMN-041).
    pub fn compute_for_data(spec: &LayoutSpec, data_len: usize) -> Result<FrameLayout> {
        FrameLayout::compute(&spec.with_data_len(data_len))
    }

    /// The same layout whose data field starts with a header of `header_len` octets (the TFDF
    /// Header of USLP, the `M_PDU` or `B_PDU` header of AOS) before the data it carries
    /// (CMN-040, issue #154).
    ///
    /// # Errors
    /// `InvalidConfig` (`frame_data_field`) unless the header leaves at least one octet of the
    /// data field (CMN-041).
    pub fn with_data_header(self, header_len: usize) -> Result<FrameLayout> {
        if header_len < self.data.len() {
            Ok(FrameLayout {
                data_header_len: header_len,
                ..self
            })
        } else {
            Err(Error::new(ErrorKind::InvalidConfig).with_field(field::FRAME_DATA_FIELD))
        }
    }

    /// The header at the start of the data field; empty unless
    /// [`with_data_header`](Self::with_data_header) set one (CMN-040).
    #[must_use]
    pub const fn data_header(&self) -> Range<usize> {
        self.data.start..self.data.start.saturating_add(self.data_header_len)
    }

    /// The data field after its header, never empty (CMN-040, CMN-041).
    #[must_use]
    pub const fn data_zone(&self) -> Range<usize> {
        self.data.start.saturating_add(self.data_header_len)..self.data.end
    }

    /// Primary header (CMN-040).
    #[must_use]
    pub fn primary_header(&self) -> Range<usize> {
        self.primary_header.clone()
    }

    /// Secondary header, Insert Zone, or Segment Header; empty if absent (CMN-040).
    #[must_use]
    pub fn secondary(&self) -> Range<usize> {
        self.secondary.clone()
    }

    /// Security Header; empty without SDLS (CMN-040, API-070).
    #[must_use]
    pub fn security_header(&self) -> Range<usize> {
        self.security_header.clone()
    }

    /// Data field, never empty (CMN-040, CMN-041).
    #[must_use]
    pub fn data(&self) -> Range<usize> {
        self.data.clone()
    }

    /// Security Trailer; empty without SDLS (CMN-040, API-070).
    #[must_use]
    pub fn security_trailer(&self) -> Range<usize> {
        self.security_trailer.clone()
    }

    /// Operational Control Field; empty if absent (CMN-040).
    #[must_use]
    pub fn ocf(&self) -> Range<usize> {
        self.ocf.clone()
    }

    /// Frame Error Control Field; empty if absent (CMN-040).
    #[must_use]
    pub fn fecf(&self) -> Range<usize> {
        self.fecf.clone()
    }

    /// Frame length: the end of the last range (CMN-040).
    #[must_use]
    pub const fn frame_len(&self) -> usize {
        self.fecf.end
    }

    /// The ranges in wire order (CMN-040).
    #[must_use]
    pub fn ranges(&self) -> [Range<usize>; 7] {
        [
            self.primary_header.clone(),
            self.secondary.clone(),
            self.security_header.clone(),
            self.data.clone(),
            self.security_trailer.clone(),
            self.ocf.clone(),
            self.fecf.clone(),
        ]
    }
}

/// The octets of a frame together with the layout they were checked against (CMN-040, RS-020,
/// issue #154).
///
/// [`new`](Self::new) checks once that the frame is as long as the layout, so every range of the
/// layout lies inside the frame and the accessors cut it without a per-field check. A decoder
/// holds this instead of one slice for each field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FramedBytes<'a> {
    frame: &'a [u8],
    layout: FrameLayout,
}

impl<'a> FramedBytes<'a> {
    /// The frame `frame` laid out as `layout`.
    ///
    /// # Errors
    /// `LengthMismatch` (`frame_len`) unless `frame` is exactly `layout.frame_len()` octets.
    pub const fn new(frame: &'a [u8], layout: FrameLayout) -> Result<FramedBytes<'a>> {
        if frame.len() == layout.frame_len() {
            Ok(FramedBytes { frame, layout })
        } else {
            Err(Error::new(ErrorKind::LengthMismatch).with_field(field::FRAME_LEN))
        }
    }

    /// The whole frame.
    #[must_use]
    pub const fn frame(&self) -> &'a [u8] {
        self.frame
    }

    /// The layout the frame was checked against.
    #[must_use]
    pub const fn layout(&self) -> &FrameLayout {
        &self.layout
    }

    /// The octets in `range`, a range of the layout, which lies inside the frame because
    /// [`new`](Self::new) checked the frame against the layout. Private, so that no other range
    /// reaches it.
    fn cut(&self, range: Range<usize>) -> &'a [u8] {
        self.frame.get(range).unwrap_or_default()
    }

    /// The primary header octets.
    #[must_use]
    pub fn primary_header(&self) -> &'a [u8] {
        self.cut(self.layout.primary_header())
    }

    /// The secondary slot octets (FSH, Insert Zone, or Segment Header); empty if absent.
    #[must_use]
    pub fn secondary(&self) -> &'a [u8] {
        self.cut(self.layout.secondary())
    }

    /// The Security Header octets; empty without SDLS.
    #[must_use]
    pub fn security_header(&self) -> &'a [u8] {
        self.cut(self.layout.security_header())
    }

    /// The data field octets, header included.
    #[must_use]
    pub fn data(&self) -> &'a [u8] {
        self.cut(self.layout.data())
    }

    /// The header at the start of the data field; empty unless the layout has one.
    #[must_use]
    pub fn data_header(&self) -> &'a [u8] {
        self.cut(self.layout.data_header())
    }

    /// The data field after its header.
    #[must_use]
    pub fn data_zone(&self) -> &'a [u8] {
        self.cut(self.layout.data_zone())
    }

    /// The Security Trailer octets; empty without SDLS.
    #[must_use]
    pub fn security_trailer(&self) -> &'a [u8] {
        self.cut(self.layout.security_trailer())
    }

    /// The OCF octets; empty if absent.
    #[must_use]
    pub fn ocf(&self) -> &'a [u8] {
        self.cut(self.layout.ocf())
    }

    /// The FECF octets; empty if absent.
    #[must_use]
    pub fn fecf(&self) -> &'a [u8] {
        self.cut(self.layout.fecf())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{FrameLayout, FramedBytes, LayoutSpec};
    use crate::error::ErrorKind;
    use crate::field;
    use crate::security::SecurityLayout;

    #[test]
    fn check_room_needs_a_data_octet_after_the_header() {
        // req: CMN-041
        let spec = LayoutSpec {
            frame_len: 10,
            primary_header_len: 6,
            secondary_len: 0,
            security: SecurityLayout::NONE,
            ocf: false,
            fecf: true,
        };
        // Two octets are left for the data field.
        assert_eq!(spec.check_room(0), Ok(()));
        assert_eq!(spec.check_room(1), Ok(()));
        for e in [spec.check_room(2), spec.with_data_len(0).check_room(0)] {
            let e = e.unwrap_err();
            assert_eq!(
                (e.kind(), e.field()),
                (ErrorKind::InvalidConfig, Some(field::FRAME_DATA_FIELD))
            );
        }
    }

    #[test]
    fn tm_frame_with_everything() {
        // req: CMN-040
        let spec = LayoutSpec {
            frame_len: 1115,
            primary_header_len: 6,
            secondary_len: 8,
            security: SecurityLayout::new(6, 8).unwrap(),
            ocf: true,
            fecf: true,
        };
        let l = FrameLayout::compute(&spec).unwrap();
        assert_eq!(
            l.ranges(),
            [
                0..6,
                6..14,
                14..20,
                20..1101,
                1101..1109,
                1109..1113,
                1113..1115
            ]
        );
    }

    #[test]
    fn rejects_empty_data_field() {
        // req: CMN-041
        let spec = LayoutSpec {
            frame_len: 12,
            primary_header_len: 6,
            secondary_len: 0,
            security: SecurityLayout::NONE,
            ocf: true,
            fecf: true,
        };
        assert_eq!(
            FrameLayout::compute(&spec).unwrap_err().kind(),
            ErrorKind::InvalidConfig
        );
        let spec = LayoutSpec {
            frame_len: 13,
            ..spec
        };
        assert_eq!(FrameLayout::compute(&spec).unwrap().data(), 6..7);
        let spec = LayoutSpec {
            frame_len: 3,
            ..spec
        };
        let e = FrameLayout::compute(&spec).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidConfig, Some(field::FRAME_DATA_FIELD))
        );
    }

    fn spec() -> LayoutSpec {
        LayoutSpec {
            frame_len: 0,
            primary_header_len: 6,
            secondary_len: 2,
            security: SecurityLayout::new(3, 4).unwrap(),
            ocf: true,
            fecf: true,
        }
    }

    #[test]
    fn a_layout_for_a_data_length() {
        // req: CMN-040, CMN-041
        assert_eq!(spec().overhead(), 6 + 2 + 3 + 4 + 4 + 2);
        let bare = LayoutSpec {
            ocf: false,
            fecf: false,
            security: SecurityLayout::NONE,
            secondary_len: 0,
            ..spec()
        };
        assert_eq!(bare.overhead(), 6);
        assert_eq!(spec().with_data_len(10).frame_len, 31);
        let l = FrameLayout::compute_for_data(&spec(), 10).unwrap();
        assert_eq!((l.data().len(), l.frame_len()), (10, 31));
        // The frame length of the spec is ignored.
        let other = LayoutSpec {
            frame_len: 999,
            ..spec()
        };
        assert_eq!(FrameLayout::compute_for_data(&other, 10).unwrap(), l);
        assert_eq!(
            FrameLayout::compute_for_data(&spec(), 0)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidConfig
        );
    }

    #[test]
    fn a_layout_carries_the_header_of_its_data_field() {
        // req: CMN-040, CMN-041
        let l = FrameLayout::compute_for_data(&spec(), 10).unwrap();
        assert_eq!(
            (l.data_header(), l.data_zone()),
            (l.data().start..l.data().start, l.data())
        );
        let h = l.clone().with_data_header(3).unwrap();
        assert_eq!(h.data_header().len(), 3);
        assert_eq!(h.data_zone().len(), 7);
        assert_eq!(h.data_header().end, h.data_zone().start);
        assert_eq!(h.data(), l.data());
        assert!(l.clone().with_data_header(9).is_ok());
        let e = l.with_data_header(10).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidConfig, Some(field::FRAME_DATA_FIELD))
        );
    }

    #[test]
    fn framed_bytes_cut_the_frame_at_its_layout() {
        // req: CMN-040, RS-020
        let l = FrameLayout::compute_for_data(&spec(), 10)
            .unwrap()
            .with_data_header(2)
            .unwrap();
        let frame: [u8; 31] = core::array::from_fn(|i| u8::try_from(i).unwrap());
        let f = FramedBytes::new(&frame, l.clone()).unwrap();
        assert_eq!(f.frame(), &frame[..]);
        assert_eq!(f.layout(), &l);
        assert_eq!(f.primary_header(), &frame[0..6]);
        assert_eq!(f.secondary(), &frame[6..8]);
        assert_eq!(f.security_header(), &frame[8..11]);
        assert_eq!(f.data(), &frame[11..21]);
        assert_eq!(f.data_header(), &frame[11..13]);
        assert_eq!(f.data_zone(), &frame[13..21]);
        assert_eq!(f.security_trailer(), &frame[21..25]);
        assert_eq!(f.ocf(), &frame[25..29]);
        assert_eq!(f.fecf(), &frame[29..31]);
        let long = [0u8; 32];
        let e = FramedBytes::new(&long, l.clone()).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::LengthMismatch, Some(field::FRAME_LEN))
        );
        assert_eq!(
            FramedBytes::new(&frame[..30], l).unwrap_err().kind(),
            ErrorKind::LengthMismatch
        );
    }
}
