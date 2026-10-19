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

use crate::error::{Error, ErrorKind, FieldId, Result};
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
    /// `InvalidConfig` (field `data`) if the other fields leave no octet for the data field
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
            .ok_or(Error::new(ErrorKind::InvalidConfig).with_field(FieldId::new("data")))?;
        let mut at = 0;
        Ok(FrameLayout {
            primary_header: take(&mut at, spec.primary_header_len),
            secondary: take(&mut at, spec.secondary_len),
            security_header: take(&mut at, spec.security.header_len()),
            data: take(&mut at, data_len),
            security_trailer: take(&mut at, spec.security.trailer_len()),
            ocf: take(&mut at, if spec.ocf { OCF_LEN } else { 0 }),
            fecf: take(&mut at, if spec.fecf { FECF_LEN } else { 0 }),
        })
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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "test bookkeeping on small bounded values (RS-018)"
)]
mod tests {
    use super::{FrameLayout, LayoutSpec};
    use crate::error::ErrorKind;
    use crate::security::SecurityLayout;
    use proptest::prelude::*;

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
        assert_eq!(
            FrameLayout::compute(&spec).unwrap_err().kind(),
            ErrorKind::InvalidConfig
        );
    }

    proptest! {
        #[test]
        fn ranges_are_contiguous_and_cover_the_frame(
            frame_len in 0_usize..2000, ph in 0_usize..16, sec in 0_usize..70,
            sh in 0_usize..=64, st in 0_usize..40, ocf in any::<bool>(), fecf in any::<bool>(),
        ) {
            // req: CMN-040, CMN-041
            let spec = LayoutSpec { frame_len, primary_header_len: ph, secondary_len: sec,
                                    security: SecurityLayout::new(sh, st).unwrap(), ocf, fecf };
            let fixed = ph + sec + sh + st + if ocf { 4 } else { 0 } + if fecf { 2 } else { 0 };
            match FrameLayout::compute(&spec) {
                Ok(l) => {
                    let r = l.ranges();
                    prop_assert_eq!(r[0].start, 0);
                    for w in r.windows(2) { prop_assert_eq!(w[0].end, w[1].start); }
                    prop_assert_eq!(r[6].end, frame_len);
                    prop_assert_eq!(l.data().len(), frame_len - fixed);
                    prop_assert_eq!(l.frame_len(), frame_len);
                    prop_assert!(!l.data().is_empty());
                }
                Err(e) => {
                    prop_assert_eq!(e.kind(), ErrorKind::InvalidConfig);
                    prop_assert!(frame_len <= fixed);
                }
            }
        }
    }
}
