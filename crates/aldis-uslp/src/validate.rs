//! USLP Frame Validation Check Procedure (732.1 §4.3.11.3; USLP-014, M2 plan D14, D17).
//!
//! When the Synchronization and Channel Coding Sublayer below is TC Synchronization and Channel
//! Coding (§4.3.11.1.6), [`validate_frame`] applies the tests of §4.3.11.3.4 a–e to the octets
//! of one frame, as the Frame Delimiting and Fill Data Removal Procedure delimits them
//! ([`delimit`](crate::delimit)), together with the decoder's checks of the TFDF Header. It runs
//! the same code as [`UslpFrame::decode`] and reports each failure with a [`Reason`] whose
//! numeric code is stable (`docs/standards/uslp-frame.md` §3.1).

use core::ops::Deref;

use aldis_core::{DecodeMode, Error, Lenient};

use crate::anomaly::UslpAnomaly;
use crate::config::VcLookup;
use crate::view::{UslpFrame, parse};

/// Why a frame failed the USLP Frame Validation Checks or the decoder (USLP-014).
///
/// 732.1 §4.3.11.3.4; M2 plan D14. The codes are stable and shared with the C ABI;
/// `docs/standards/uslp-frame.md` §3.1 gives the order in which they are checked.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reason {
    /// 1, test (a): the TFVN is not `1100` (§4.1.2.2.2; USLP-001).
    UnexpectedVersion,
    /// 2, test (b): the SCID names no Master Channel of the Physical Channel, so the MCID is not
    /// expected.
    UnexpectedSpacecraft,
    /// 3, test (c): Bypass/Sequence Control Flag `0` with Protocol Control Command Flag `1`
    /// (Table 4-1; USLP-001).
    ReservedFlags,
    /// 4, test (c), strict mode only: the Reserve Spares are not `00` (§4.1.2.9.2).
    ReservedSpare,
    /// 5, test (c): the VCID is not valid on the Master Channel.
    InvalidVcid,
    /// 6, test (c): the MAP ID is not valid on the VC.
    InvalidMapId,
    /// 7, test (c): a truncated frame on a VC without a truncated frame length (Annex D1.2.1;
    /// USLP-002).
    TruncatedFrameNotAllowed,
    /// 8, test (c): the VCF Count Length is not the VC's for the frame's quality of service (§4.1.2.11.4;
    /// USLP-004).
    VcfCountLengthMismatch,
    /// 9, test (c): the OCF Flag disagrees with the VC's OCF use (§4.1.5.3–4).
    OcfFlagMismatch,
    /// 10, test (c): on a fixed-length Physical Channel, the Frame Length is not the configured
    /// frame length (§4.1.2.7; USLP-003).
    FixedLengthMismatch,
    /// 11, test (c): on a variable-length Physical Channel, the Frame Length exceeds the
    /// maximum (Table 5-1 note 2; USLP-003).
    FrameTooLong,
    /// 12, test (c): the Frame Length leaves no TFDZ octet after the fields the frame has
    /// (CMN-041; OI-44 item 2).
    FrameTooShort,
    /// 13, test (d): the octets present disagree with the Frame Length, or with the truncated
    /// frame length (§4.3.11.3.4 d).
    LengthMismatch,
    /// 14, test (e): the FECF does not match the recomputed CRC (§4.1.6; Annex B).
    FecfMismatch,
    /// 15: the TFDZ Construction Rule is of the other TFDZ type than the VC's (USLP-016).
    ConstructionRuleMismatch,
    /// 16: the First Header or Last Valid Octet Pointer lies beyond the TFDZ (USLP-006).
    PointerBeyondZone,
    /// 17, strict mode only: a frame on VCID 63 whose OID header fields deviate (§4.1.4.1.8–9;
    /// OI-44 item 1).
    OidHeaderMismatch,
    /// 18, test (c): a non-truncated frame on a VC that has a truncated frame length, which
    /// carries only truncated frames (Annex D1.2.1, D1.2.4 notes; USLP-002; OI-48).
    NonTruncatedFrameNotAllowed,
}

impl Reason {
    /// Every reason, in code order.
    pub const ALL: [Reason; 18] = [
        Reason::UnexpectedVersion,
        Reason::UnexpectedSpacecraft,
        Reason::ReservedFlags,
        Reason::ReservedSpare,
        Reason::InvalidVcid,
        Reason::InvalidMapId,
        Reason::TruncatedFrameNotAllowed,
        Reason::VcfCountLengthMismatch,
        Reason::OcfFlagMismatch,
        Reason::FixedLengthMismatch,
        Reason::FrameTooLong,
        Reason::FrameTooShort,
        Reason::LengthMismatch,
        Reason::FecfMismatch,
        Reason::ConstructionRuleMismatch,
        Reason::PointerBeyondZone,
        Reason::OidHeaderMismatch,
        Reason::NonTruncatedFrameNotAllowed,
    ];

    /// The stable numeric code, 1–18 (`docs/standards/uslp-frame.md` §3.1; M2 plan D14).
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Reason::UnexpectedVersion => 1,
            Reason::UnexpectedSpacecraft => 2,
            Reason::ReservedFlags => 3,
            Reason::ReservedSpare => 4,
            Reason::InvalidVcid => 5,
            Reason::InvalidMapId => 6,
            Reason::TruncatedFrameNotAllowed => 7,
            Reason::VcfCountLengthMismatch => 8,
            Reason::OcfFlagMismatch => 9,
            Reason::FixedLengthMismatch => 10,
            Reason::FrameTooLong => 11,
            Reason::FrameTooShort => 12,
            Reason::LengthMismatch => 13,
            Reason::FecfMismatch => 14,
            Reason::ConstructionRuleMismatch => 15,
            Reason::PointerBeyondZone => 16,
            Reason::OidHeaderMismatch => 17,
            Reason::NonTruncatedFrameNotAllowed => 18,
        }
    }

    /// The reason with numeric code `code`, or `None` (M2 plan D14).
    #[must_use]
    pub fn from_code(code: u8) -> Option<Reason> {
        Reason::ALL.into_iter().find(|r| r.code() == code)
    }
}

/// A frame that failed the checks: the [`Reason`] and the decoder's [`Error`] (USLP-014,
/// GEN-041).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rejection {
    reason: Reason,
    error: Error,
}

impl Rejection {
    /// A rejection for `reason`, with the error the decoder reports for it.
    pub(crate) const fn new(reason: Reason, error: Error) -> Rejection {
        Rejection { reason, error }
    }

    /// Why the frame was discarded (USLP-014).
    #[must_use]
    pub const fn reason(&self) -> Reason {
        self.reason
    }

    /// The error of the failed check, with the field it names (GEN-041).
    #[must_use]
    pub const fn error(&self) -> Error {
        self.error
    }
}

impl From<Rejection> for Error {
    fn from(r: Rejection) -> Error {
        r.error
    }
}

/// A frame that passed the USLP Frame Validation Checks (732.1 §4.3.11.3; USLP-014, API-016).
///
/// [`validate_frame`] returns it. It dereferences to the [`UslpFrame`]; holding one is the proof
/// that the octets were exactly one frame and passed tests a–e (RS-023 `Checked`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedUslpFrame<'a> {
    frame: UslpFrame<'a>,
}

impl<'a> ValidatedUslpFrame<'a> {
    /// The frame that exact-length parsing returned.
    pub(crate) const fn new(frame: UslpFrame<'a>) -> ValidatedUslpFrame<'a> {
        ValidatedUslpFrame { frame }
    }

    /// The frame, without the proof.
    #[must_use]
    pub const fn into_frame(self) -> UslpFrame<'a> {
        self.frame
    }
}

impl<'a> Deref for ValidatedUslpFrame<'a> {
    type Target = UslpFrame<'a>;

    fn deref(&self) -> &UslpFrame<'a> {
        &self.frame
    }
}

/// Applies the USLP Frame Validation Checks to `frame`, the octets of exactly one frame (732.1
/// §4.3.11.3; USLP-014, API-016), in strict mode.
///
/// The checks run in the order of `docs/standards/uslp-frame.md` §3.1: (a) the TFVN; (b) the
/// SCID; (c) the flags, the Reserve Spares, the VCID and MAP ID, a truncated frame
/// on its VC, the VCF Count Length, the OCF Flag, and the Frame Length against the channel;
/// (d) the octets present against the Frame Length, or a truncated frame's against its VC's
/// truncated frame length; (e) the FECF; then the TFDF Header's rule and pointer and the OID
/// header. A frame that passes is returned as a [`ValidatedUslpFrame`].
///
/// # Errors
/// The first check that fails, as a [`Rejection`].
pub fn validate_frame<'a, L: VcLookup + ?Sized>(
    frame: &'a [u8],
    channels: &L,
) -> Result<ValidatedUslpFrame<'a>, Rejection> {
    parse(frame, channels, DecodeMode::Strict, true).map(|p| ValidatedUslpFrame::new(p.frame))
}

/// Applies the checks of [`validate_frame`] in lenient mode: nonzero Reserve Spares and a
/// deviating OID header are tolerated and reported as [`UslpAnomaly`] flags (USLP-014,
/// API-016, API-032).
///
/// # Errors
/// The first check that fails, as a [`Rejection`].
pub fn validate_frame_lenient<'a, L: VcLookup + ?Sized>(
    frame: &'a [u8],
    channels: &L,
) -> Result<Lenient<ValidatedUslpFrame<'a>, UslpAnomaly>, Rejection> {
    parse(frame, channels, DecodeMode::Lenient, true)
        .map(|p| Lenient::new(ValidatedUslpFrame::new(p.frame), p.anomalies))
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Reason, Rejection, validate_frame};
    use crate::config::tests::variable;
    use crate::field;
    use aldis_core::{Error, ErrorKind};

    #[test]
    fn reason_codes_are_stable() {
        // req: USLP-014
        for (i, r) in Reason::ALL.into_iter().enumerate() {
            let code = u8::try_from(i + 1).unwrap();
            assert_eq!(r.code(), code);
            assert_eq!(Reason::from_code(code), Some(r));
        }
        assert_eq!(Reason::from_code(0), None);
        assert_eq!(Reason::from_code(19), None);
    }

    #[test]
    fn rejections_carry_reason_and_error() {
        // req: USLP-014, GEN-041
        let t = variable();
        let r = validate_frame(&[0xC1, 0x55], &t).unwrap_err();
        assert_eq!(r.reason(), Reason::LengthMismatch);
        assert_eq!(
            (r.error().kind(), r.error().field()),
            (ErrorKind::LengthMismatch, Some(field::FRAME_LEN))
        );
        assert_eq!(Error::from(r), r.error());
        let x = Rejection::new(Reason::InvalidVcid, Error::new(ErrorKind::UnknownChannel));
        assert_eq!(x.reason().code(), 5);
    }
}
