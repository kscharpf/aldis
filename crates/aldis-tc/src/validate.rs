//! Frame Validation Check Procedure (232.0 §4.4.9.3; TC-021…TC-023, M2 plan D14).
//!
//! [`validate_frame`] applies the checks of 232.0 §4.4.9.3.3 a–e to the octets of one frame,
//! as the Frame Delimiting and Fill Removal Procedure delimits them
//! ([`delimit`](crate::delimit)), together with the checks of the decoder and the discard
//! rules for an invalid VCID and MAP ID (§4.4.7.3, §4.4.3.3). It runs the same code as
//! [`TcFrame::decode`] for both Type-A and Type-B frames, before any FARM-1 processing
//! (§4.4.9.3.1; TC-023), and reports each failure with a [`Reason`] whose numeric code is
//! stable (`docs/standards/tc-frame.md` §4).

use aldis_core::{DecodeMode, Error};

use crate::config::VcLookup;
use crate::view::{TcFrame, parse};

/// Why a frame failed the Frame Validation Checks (232.0 §4.4.9.3.3; TC-021, TC-022, M2 plan
/// D14). Listed in check order; the codes are stable and shared with the C ABI.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reason {
    /// 1, check (a): the Transfer Frame Version Number is not `00` (§4.1.2.2.2).
    UnexpectedVersion,
    /// 2, check (b): the Spacecraft ID is not a valid Spacecraft ID of the Physical Channel,
    /// so the MCID is not expected (Table 5-1).
    UnexpectedSpacecraft,
    /// 3, check (c): Bypass Flag `0` with Control Command Flag `1` (Table 4-1; TC-002).
    ReservedFlags,
    /// 4, check (c), strict mode only: the Reserved Spare is not `00` (§4.1.2.4.2; TC-003).
    ReservedSpare,
    /// 5, check (c): the VCID is not valid on the Master Channel (§4.4.7.3; TC-022).
    InvalidVcid,
    /// 6, check (c): the Frame Length exceeds the Maximum Transfer Frame Length of the
    /// Virtual or Physical Channel (Tables 5-1, 5-3).
    FrameTooLong,
    /// 7, check (c): the Frame Length leaves no data octet after the Segment Header, Security
    /// Header and Trailer, and FECF that the channel has (§4.1.1; CMN-041).
    FrameTooShort,
    /// 8, check (d): the Frame Length disagrees with the number of octets present.
    LengthMismatch,
    /// 9, check (e): the FECF does not match the recomputed CRC (§4.1.4.3; TC-006).
    FecfMismatch,
    /// 10: the MAP ID is not valid on the Virtual Channel (§4.4.3.3; TC-022).
    InvalidMapId,
    /// 11, strict mode only: a Type-C data field that is neither Unlock nor Set V(R)
    /// (§4.1.3.3.4; TC-005).
    ReservedControlCommand,
}

impl Reason {
    /// Every reason, in code order.
    pub const ALL: [Reason; 11] = [
        Reason::UnexpectedVersion,
        Reason::UnexpectedSpacecraft,
        Reason::ReservedFlags,
        Reason::ReservedSpare,
        Reason::InvalidVcid,
        Reason::FrameTooLong,
        Reason::FrameTooShort,
        Reason::LengthMismatch,
        Reason::FecfMismatch,
        Reason::InvalidMapId,
        Reason::ReservedControlCommand,
    ];

    /// The stable numeric code, 1–11 (`docs/standards/tc-frame.md` §4; M2 plan D14).
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Reason::UnexpectedVersion => 1,
            Reason::UnexpectedSpacecraft => 2,
            Reason::ReservedFlags => 3,
            Reason::ReservedSpare => 4,
            Reason::InvalidVcid => 5,
            Reason::FrameTooLong => 6,
            Reason::FrameTooShort => 7,
            Reason::LengthMismatch => 8,
            Reason::FecfMismatch => 9,
            Reason::InvalidMapId => 10,
            Reason::ReservedControlCommand => 11,
        }
    }

    /// The reason with numeric code `code`, or `None` (M2 plan D14).
    #[must_use]
    pub fn from_code(code: u8) -> Option<Reason> {
        Reason::ALL.into_iter().find(|r| r.code() == code)
    }
}

/// A frame that failed the Frame Validation Checks: the [`Reason`] and the decoder's
/// [`Error`] (TC-021, TC-022, GEN-041).
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

    /// Why the frame was discarded (TC-021, TC-022).
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

/// Applies the Frame Validation Checks to `frame`, the octets of exactly one frame (232.0
/// §4.4.9.3; TC-021…TC-023).
///
/// The checks run in the order of [`Reason`]: (a) version; (b) Spacecraft ID; (c) the frame
/// type flags, the Reserved Spare (strict mode), the VCID, and the Frame Length against the
/// channel's limits; (d) the Frame Length against `frame.len()`; (e) the FECF; then the MAP ID
/// of a Segment Header, the room left by the MAP's Security Header and Trailer, and, in strict
/// mode, the Control Command of a Type-C frame. Type-A and Type-B frames are checked alike
/// (TC-023). A frame that passes is returned decoded, with the anomalies a lenient mode
/// tolerated.
///
/// # Errors
/// The first check that fails, as a [`Rejection`].
pub fn validate_frame<'a, L: VcLookup + ?Sized>(
    frame: &'a [u8],
    channels: &L,
    mode: DecodeMode,
) -> Result<TcFrame<'a>, Rejection> {
    parse(frame, channels, mode, true)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Reason, Rejection, validate_frame};
    use crate::config::tests::standard;
    use crate::field;
    use aldis_core::{DecodeMode, Error, ErrorKind};

    #[test]
    fn reason_codes_are_stable() {
        // req: TC-021, TC-022
        for (i, r) in Reason::ALL.into_iter().enumerate() {
            let code = u8::try_from(i + 1).unwrap();
            assert_eq!(r.code(), code);
            assert_eq!(Reason::from_code(code), Some(r));
        }
        assert_eq!(Reason::from_code(0), None);
        assert_eq!(Reason::from_code(12), None);
    }

    #[test]
    fn rejections_carry_reason_and_error() {
        // req: TC-021, TC-023, GEN-041
        let t = standard(true);
        let r = validate_frame(&[0x01, 0x55, 0x04], &t, DecodeMode::Strict).unwrap_err();
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
