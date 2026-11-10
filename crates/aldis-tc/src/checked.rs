//! Frame Validation Check Procedure (232.0 §4.4.9.3; TC-021…TC-023, M2 plan D14).
//!
//! [`check_frame`] applies the checks of 232.0 §4.4.9.3.3 a–e to the octets of one frame,
//! as the Frame Delimiting and Fill Removal Procedure delimits them
//! ([`delimit`](crate::delimit)), together with the checks of the decoder and the discard
//! rule for an invalid VCID (§4.4.7.3). A MAP ID the channels do not hold passes: the MAP
//! Demultiplexing Function discards the FDU after FARM-1 (§4.4.3.3, §4.4.5). It runs the same
//! code as [`TcFrame::decode`] for both Type-A and Type-B frames, before any FARM-1 processing
//! (§4.4.9.3.1; TC-023), and reports each failure with a [`Reason`] whose numeric code is
//! stable (`docs/standards/tc-frame.md` §4).

use aldis_core::checked::check_bytes;
use aldis_core::{AnomalySet, CheckBytes, Checked, DecodeMode, Lenient, Rejection};

use crate::anomaly::TcAnomaly;
use crate::config::{MapConfig, VcConfig, VcLookup};
use crate::view::{Channels, TcFrame, parse};

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
    /// 11, strict mode only: a Type-C data field that is neither Unlock nor Set V(R)
    /// (§4.1.3.3.4; TC-005).
    ReservedControlCommand,
}

impl Reason {
    /// Every reason, in code order.
    pub const ALL: [Reason; 10] = [
        Reason::UnexpectedVersion,
        Reason::UnexpectedSpacecraft,
        Reason::ReservedFlags,
        Reason::ReservedSpare,
        Reason::InvalidVcid,
        Reason::FrameTooLong,
        Reason::FrameTooShort,
        Reason::LengthMismatch,
        Reason::FecfMismatch,
        Reason::ReservedControlCommand,
    ];

    /// The stable numeric code, 1–9 and 11 (`docs/standards/tc-frame.md` §4; M2 plan D14). Code
    /// 10 was `InvalidMapId`: a MAP ID the channels do not hold is discarded by the MAP
    /// Demultiplexing Function, not by the Frame Validation Checks, and the code is never
    /// reused.
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
            Reason::ReservedControlCommand => 11,
        }
    }

    /// The reason with numeric code `code`, or `None` (M2 plan D14).
    #[must_use]
    pub fn from_code(code: u8) -> Option<Reason> {
        Reason::ALL.into_iter().find(|r| r.code() == code)
    }
}

/// Applies the Frame Validation Checks to `frame`, the octets of exactly one frame (232.0
/// §4.4.9.3; TC-021…TC-023, API-016), in `mode`.
///
/// The checks run in the order of [`Reason`]: (a) version; (b) Spacecraft ID; (c) the frame
/// type flags, the Reserved Spare, the VCID, and the Frame Length against the channel's limits;
/// (d) the Frame Length against `frame.len()` (232.0 §4.4.9.3.3 d); (e) the FECF; then the room
/// left by the MAP's Security Header and Trailer, and the Control Command of a Type-C frame. A
/// MAP ID the channels do not hold is not checked: the frame is read with no SDLS fields.
/// Type-A and Type-B frames are checked alike (TC-023). A frame that passes is returned as a
/// [`Checked`] frame.
///
/// In lenient mode the Reserved Spare and the Control Command checks accept their deviations and
/// report them as [`TcAnomaly`] flags (TC-003, TC-005, API-032); the anomaly set is empty
/// after a strict check.
///
/// On a Virtual Channel whose MAPs use SDLS, a frame of an unknown MAP passes with no Security
/// Header or Trailer (`SecurityLayout::NONE`), so a caller that takes the checked frame
/// directly sees those octets as data. Route the frame through the MAP reassembler, which
/// discards the FDU of an unknown MAP (§4.4.3.3; TC-022, OI-98), or check
/// `segment_header()` against the configured MAPs.
///
/// # Errors
/// The first check that fails, as a [`Rejection`].
pub fn check_frame<'a, L: VcLookup + ?Sized>(
    frame: &'a [u8],
    channels: &L,
    mode: DecodeMode,
) -> Result<Lenient<Checked<TcFrame<'a>>, TcAnomaly>, Rejection<Reason>> {
    check_bytes(frame, channels, mode)
}

impl aldis_core::checked::sealed::Sealed for TcFrame<'_> {}

/// The Frame Validation Checks of [`check_frame`], through the pipeline of `aldis_core::checked`
/// (TC-021…TC-023, API-016). What it matches besides the frame is the Virtual Channel and MAP
/// configuration.
impl<'a, L: VcLookup + ?Sized> CheckBytes<'a, L> for TcFrame<'a> {
    type Reason = Reason;
    type Anomaly = TcAnomaly;
    type Matched = (VcConfig, Option<MapConfig>);

    fn parse_exact(
        bytes: &'a [u8],
        channels: &L,
        mode: DecodeMode,
    ) -> Result<(TcFrame<'a>, AnomalySet<TcAnomaly>, Self::Matched), Rejection<Reason>> {
        parse(bytes, channels, Channels::Required, mode, true)
            .map(|p| (p.frame, p.anomalies, (p.vc, p.map)))
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Reason, Rejection, check_frame};
    use crate::anomaly::TcAnomaly;
    use crate::config::tests::standard;
    use crate::field;
    use aldis_core::{DecodeMode, Error, ErrorKind};

    #[test]
    fn reason_codes_are_stable() {
        // req: TC-021, TC-022
        let codes = [1, 2, 3, 4, 5, 6, 7, 8, 9, 11];
        for (r, code) in Reason::ALL.into_iter().zip(codes) {
            assert_eq!(r.code(), code);
            assert_eq!(Reason::from_code(code), Some(r));
        }
        // Code 10 was `InvalidMapId`, retired: the MAP Demultiplexing Function discards the FDU.
        assert_eq!(Reason::from_code(0), None);
        assert_eq!(Reason::from_code(10), None);
        assert_eq!(Reason::from_code(12), None);
    }

    #[test]
    fn rejections_carry_reason_and_error() {
        // req: TC-021, TC-023, GEN-041
        let t = standard(true);
        let r = check_frame(&[0x01, 0x55, 0x04], &t, DecodeMode::Strict).unwrap_err();
        assert_eq!(r.reason(), Reason::LengthMismatch);
        assert_eq!(
            (r.error().kind(), r.error().field()),
            (ErrorKind::LengthMismatch, Some(field::FRAME_LEN))
        );
        assert_eq!(Error::from(r), r.error());
        let x = Rejection::new(Reason::InvalidVcid, Error::new(ErrorKind::UnknownChannel));
        assert_eq!(x.reason().code(), 5);
    }

    #[test]
    fn checked_frames_are_returned_by_both_modes() {
        // req: API-016, API-030, API-032, TC-021
        let t = standard(false);
        // An Unlock command with a Reserved Spare of 01: strict rejects, lenient flags it.
        let bytes = [0x35, 0x55, 0x04, 0x05, 0x00, 0x00];
        let r = check_frame(&bytes, &t, DecodeMode::Strict).unwrap_err();
        assert_eq!(r.reason(), Reason::ReservedSpare);
        let v = check_frame(&bytes, &t, DecodeMode::Lenient).unwrap();
        assert!(v.anomalies().contains(TcAnomaly::ReservedSpareSet));
        assert_eq!(v.len(), 6);
        let strict = check_frame(
            &[0x31, 0x55, 0x04, 0x05, 0x00, 0x00],
            &t,
            DecodeMode::Strict,
        )
        .unwrap();
        assert!(strict.anomalies().is_empty());
        let proof = strict.into_view();
        assert_eq!(proof.as_frame().len(), 6);
        assert_eq!(proof.into_frame().len(), 6);
    }
}
