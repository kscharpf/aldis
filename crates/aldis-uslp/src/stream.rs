//! Unified USLP Frame Intake Stream: Delimiting, Fill Removal, and Channel Validation
//! (732.1 §4.3.11.2, §4.3.11.3; USLP-014, USLP-015).
//!
//! Replaces fragmented delimiting and validation passes with a single in-place iterator over
//! an ingested data unit (such as a CLTU's decoded codeblocks).
//!
//! The stream is `aldis_core::stream::FrameStream` over [`UslpIntake`], the USLP rules of the
//! [`Delimiter`]. A valid frame is yielded as [`IntakeItem::Accepted`], bundled with its verified [`VcConfig`]
//! and optional [`MapConfig`]. Discarded frames, fill data, and truncated headers are yielded as
//! [`IntakeItem::Discarded`], classified by a unified [`DiscardReason`].

use aldis_core::lookup::{ChannelLookup, MapLookup, PhysicalLookup};
use core::num::NonZeroUsize;

use aldis_core::stream::{Cut, Delimiter, Peek};
use aldis_core::{Checked, DecodeMode, Error, ErrorKind};

use crate::anomaly::UslpAnomaly;
use crate::checked::Reason;
use crate::config::{MapConfig, PhysicalChannelConfig, VcConfig};
use crate::delimit::{Declared, declared_len};
use crate::field;
use crate::header::{RawRest, RawStart, UslpMapId, UslpSpacecraftId, UslpVcId};
use crate::view::{Channels, UslpFrame, parse};

/// Why wire octets or a USLP frame were discarded (732.1 §4.3.11.2, §4.3.11.3; USLP-014, USLP-015).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DiscardReason {
    /// Discarded as fill data, no longer than the longest fill the coding sublayer appends
    /// (732.1 §4.3.11.2.4 a–b).
    Fill,
    /// Discarded: it does not start a valid header and is longer than the longest fill the
    /// coding sublayer appends, so a Frame Length error may have shortened a frame before it
    /// (732.1 §4.3.11.2.4 note 2; OI-47).
    LeftoverFill,
    /// Discarded as an incomplete frame whose declared length exceeds remaining octets (§4.3.11.2.3).
    IncompleteFrame {
        /// The frame length declared in the header or by the VC's truncated length.
        declared_len: usize,
    },
    /// A truncated header whose SCID and VCID name no configured VC or no truncated length.
    NoTruncatedLength,
    /// 1, test (a): TFVN is not `1100` (§4.1.2.2.2; USLP-001).
    UnexpectedVersion,
    /// 2, test (b): SCID names no Master Channel of the Physical Channel.
    UnexpectedSpacecraft,
    /// 3, test (c): Bypass/Sequence Control Flag `0` with Protocol Control Command Flag `1` (Table 4-1; USLP-001).
    ReservedFlags,
    /// 4, test (c), strict mode only: Reserve Spares are not `00` (§4.1.2.9.2).
    ReservedSpare,
    /// 5, test (c): VCID is not valid on the Master Channel.
    InvalidVcid,
    /// 6, test (c): MAP ID is not valid on the VC.
    InvalidMapId,
    /// 7, test (c): a truncated frame on a VC without a truncated frame length (Annex D1.2.1; USLP-002).
    TruncatedFrameNotAllowed,
    /// 8, test (c): VCF Count Length is not the VC's for the frame's quality of service (§4.1.2.11.4; USLP-004).
    VcfCountLengthMismatch,
    /// 9, test (c): OCF Flag disagrees with the VC's OCF use (§4.1.5.3–4).
    OcfFlagMismatch,
    /// 10, test (c): on a fixed-length Physical Channel, Frame Length is not the configured length (§4.1.2.7; USLP-003).
    FixedLengthMismatch,
    /// 11, test (c): on a variable-length Physical Channel, Frame Length exceeds the maximum (Table 5-1 note 2; USLP-003).
    FrameTooLong,
    /// 12, test (c): Frame Length leaves no TFDZ octet after the fields present (CMN-041; OI-44 item 2).
    FrameTooShort,
    /// 13, test (d): octets present disagree with Frame Length or truncated frame length (§4.3.11.3.4 d).
    LengthMismatch,
    /// 14, test (e): FECF does not match recomputed CRC (§4.1.6; Annex B).
    FecfMismatch,
    /// 15: TFDZ Construction Rule is of the other TFDZ type than the VC's (USLP-016).
    ConstructionRuleMismatch,
    /// 16: First Header or Last Valid Octet Pointer lies beyond the TFDZ (USLP-006).
    PointerBeyondZone,
    /// 17: OID frame on VCID 63 deviates from required header or length (Annex D1.2.4; USLP-007).
    OidHeaderMismatch,
    /// 18: a non-truncated frame on a VC that has a truncated frame length (Annex D1.2.1; USLP-002; OI-48).
    NonTruncatedFrameNotAllowed,
}

impl DiscardReason {
    /// Stable numeric code for C ABI and telemetry classification.
    ///
    /// Codes 1–18 match the validation check reasons; 20 is Fill, 21 `IncompleteFrame`,
    /// 22 `NoTruncatedLength`, 23 `LeftoverFill`.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            DiscardReason::UnexpectedVersion => 1,
            DiscardReason::UnexpectedSpacecraft => 2,
            DiscardReason::ReservedFlags => 3,
            DiscardReason::ReservedSpare => 4,
            DiscardReason::InvalidVcid => 5,
            DiscardReason::InvalidMapId => 6,
            DiscardReason::TruncatedFrameNotAllowed => 7,
            DiscardReason::VcfCountLengthMismatch => 8,
            DiscardReason::OcfFlagMismatch => 9,
            DiscardReason::FixedLengthMismatch => 10,
            DiscardReason::FrameTooLong => 11,
            DiscardReason::FrameTooShort => 12,
            DiscardReason::LengthMismatch => 13,
            DiscardReason::FecfMismatch => 14,
            DiscardReason::ConstructionRuleMismatch => 15,
            DiscardReason::PointerBeyondZone => 16,
            DiscardReason::OidHeaderMismatch => 17,
            DiscardReason::NonTruncatedFrameNotAllowed => 18,
            DiscardReason::Fill => 20,
            DiscardReason::IncompleteFrame { .. } => 21,
            DiscardReason::NoTruncatedLength => 22,
            DiscardReason::LeftoverFill => 23,
        }
    }

    /// The validation reason if this discard was caused by a validation check failure.
    #[must_use]
    pub const fn validation_reason(self) -> Option<Reason> {
        match self {
            DiscardReason::UnexpectedVersion => Some(Reason::UnexpectedVersion),
            DiscardReason::UnexpectedSpacecraft => Some(Reason::UnexpectedSpacecraft),
            DiscardReason::ReservedFlags => Some(Reason::ReservedFlags),
            DiscardReason::ReservedSpare => Some(Reason::ReservedSpare),
            DiscardReason::InvalidVcid => Some(Reason::InvalidVcid),
            DiscardReason::InvalidMapId => Some(Reason::InvalidMapId),
            DiscardReason::TruncatedFrameNotAllowed => Some(Reason::TruncatedFrameNotAllowed),
            DiscardReason::VcfCountLengthMismatch => Some(Reason::VcfCountLengthMismatch),
            DiscardReason::OcfFlagMismatch => Some(Reason::OcfFlagMismatch),
            DiscardReason::FixedLengthMismatch => Some(Reason::FixedLengthMismatch),
            DiscardReason::FrameTooLong => Some(Reason::FrameTooLong),
            DiscardReason::FrameTooShort => Some(Reason::FrameTooShort),
            DiscardReason::LengthMismatch => Some(Reason::LengthMismatch),
            DiscardReason::FecfMismatch => Some(Reason::FecfMismatch),
            DiscardReason::ConstructionRuleMismatch => Some(Reason::ConstructionRuleMismatch),
            DiscardReason::PointerBeyondZone => Some(Reason::PointerBeyondZone),
            DiscardReason::OidHeaderMismatch => Some(Reason::OidHeaderMismatch),
            DiscardReason::NonTruncatedFrameNotAllowed => Some(Reason::NonTruncatedFrameNotAllowed),
            DiscardReason::Fill
            | DiscardReason::LeftoverFill
            | DiscardReason::IncompleteFrame { .. }
            | DiscardReason::NoTruncatedLength => None,
        }
    }
}

impl From<Reason> for DiscardReason {
    fn from(r: Reason) -> Self {
        match r {
            Reason::UnexpectedVersion => DiscardReason::UnexpectedVersion,
            Reason::UnexpectedSpacecraft => DiscardReason::UnexpectedSpacecraft,
            Reason::ReservedFlags => DiscardReason::ReservedFlags,
            Reason::ReservedSpare => DiscardReason::ReservedSpare,
            Reason::InvalidVcid => DiscardReason::InvalidVcid,
            Reason::InvalidMapId => DiscardReason::InvalidMapId,
            Reason::TruncatedFrameNotAllowed => DiscardReason::TruncatedFrameNotAllowed,
            Reason::VcfCountLengthMismatch => DiscardReason::VcfCountLengthMismatch,
            Reason::OcfFlagMismatch => DiscardReason::OcfFlagMismatch,
            Reason::FixedLengthMismatch => DiscardReason::FixedLengthMismatch,
            Reason::FrameTooLong => DiscardReason::FrameTooLong,
            Reason::FrameTooShort => DiscardReason::FrameTooShort,
            Reason::LengthMismatch => DiscardReason::LengthMismatch,
            Reason::FecfMismatch => DiscardReason::FecfMismatch,
            Reason::ConstructionRuleMismatch => DiscardReason::ConstructionRuleMismatch,
            Reason::PointerBeyondZone => DiscardReason::PointerBeyondZone,
            Reason::OidHeaderMismatch => DiscardReason::OidHeaderMismatch,
            Reason::NonTruncatedFrameNotAllowed => DiscardReason::NonTruncatedFrameNotAllowed,
        }
    }
}

/// The USLP rules of the frame stream (732.1 §4.3.11.2, §4.3.11.3; USLP-014, USLP-015).
///
/// A frame of an unexpected version whose length (declared, or its VC's truncated frame length)
/// fits in the unit is discarded and the stream goes on after it; a valid header declares a
/// frame, and a frame that declares more octets than remain is checked as far as it goes and
/// discarded as incomplete unless an earlier check fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UslpIntake;

impl Delimiter for UslpIntake {
    type Frame<'a> = UslpFrame<'a>;
    type Anomaly = UslpAnomaly;
    type Rejected = Reason;
    type Reason = DiscardReason;
    type Scid = UslpSpacecraftId;
    type VcId = UslpVcId;
    type MapId = UslpMapId;
    type Vc = VcConfig;
    type Map = MapConfig;
    type Physical = PhysicalChannelConfig;

    const FILL: DiscardReason = DiscardReason::Fill;
    const LEFTOVER_FILL: DiscardReason = DiscardReason::LeftoverFill;

    fn peek<'a, L>(rest: &'a [u8], channels: &L) -> Peek<'a, DiscardReason>
    where
        L: ChannelLookup<UslpSpacecraftId, UslpVcId, Vc = VcConfig>
            + MapLookup<UslpSpacecraftId, UslpVcId, UslpMapId, Map = MapConfig>
            + PhysicalLookup<Physical = PhysicalChannelConfig>
            + ?Sized,
    {
        // If an unexpected-version frame is present with its declared length available in rest,
        // report DiscardReason::UnexpectedVersion and advance past it.
        let unexpected_version_frame = (|| {
            let start = RawStart::parse(rest)?;
            let error = start.check_version().err()?;
            let len = if start.truncated {
                channels
                    .vc(start.channel.spacecraft_id, start.channel.vc_id)
                    .and_then(|vc| vc.truncated_frame_len())?
                    .get()
            } else {
                let raw = RawRest::parse(rest)?;
                if raw.frame_len < 7 {
                    return None;
                }
                raw.frame_len
            };
            let cut = Cut::new(rest, NonZeroUsize::new(len)?)?;
            Some((cut, error))
        })();
        if let Some((cut, error)) = unexpected_version_frame {
            return Peek::Discard {
                cut,
                reason: DiscardReason::UnexpectedVersion,
                error,
            };
        }

        match declared_len(rest, channels) {
            Some(Declared::Len(len)) => Peek::frame(len),
            Some(Declared::TruncatedOnly) => Peek::frame(rest.len()),
            Some(Declared::NoTruncatedLength) => Peek::DiscardRest {
                reason: DiscardReason::NoTruncatedLength,
                error: Error::new(ErrorKind::UnknownChannel).with_field(field::VC_ID),
            },
            None => Peek::Fill,
        }
    }

    fn incomplete<L>(
        rest: &[u8],
        declared_len: usize,
        channels: &L,
        mode: DecodeMode,
    ) -> Option<(DiscardReason, Error)>
    where
        L: ChannelLookup<UslpSpacecraftId, UslpVcId, Vc = VcConfig>
            + MapLookup<UslpSpacecraftId, UslpVcId, UslpMapId, Map = MapConfig>
            + PhysicalLookup<Physical = PhysicalChannelConfig>
            + ?Sized,
    {
        match parse(rest, channels, Channels::Required, mode, true) {
            Err(rejection) if rejection.reason() != Reason::LengthMismatch => {
                Some((DiscardReason::from(rejection.reason()), rejection.error()))
            }
            _ => Some((
                DiscardReason::IncompleteFrame { declared_len },
                Error::incomplete(declared_len),
            )),
        }
    }
}

/// An item yielded by [`FrameStream`] or [`LenientFrameStream`] (732.1 §4.3.11.2, §4.3.11.3;
/// USLP-014, USLP-015).
///
/// `F` is the accepted frame: a [`Checked`] frame from the strict stream, and the same in a
/// [`Lenient`](aldis_core::Lenient) from the lenient one (API-016, API-030). A discarded item
/// has no anomalies.
pub type IntakeItem<'a, F = Checked<UslpFrame<'a>>> =
    aldis_core::stream::IntakeItem<'a, UslpIntake, F>;

/// In-place zero-allocation iterator over a received USLP data unit (732.1 §4.3.11.2,
/// §4.3.11.3; USLP-014, USLP-015), in strict mode.
///
/// Fuses delimiting, fill removal, and channel validation in a single pass.
/// [`LenientFrameStream`] is the lenient one (API-030).
pub type FrameStream<'a, 'c, L> = aldis_core::stream::FrameStream<'a, 'c, UslpIntake, L>;

/// The lenient twin of [`FrameStream`]: each item comes with the anomalies of the frame it
/// accepted, none for a discarded item (732.1 §4.3.11.2, §4.3.11.3; USLP-014, API-032).
pub type LenientFrameStream<'a, 'c, L> =
    aldis_core::stream::LenientFrameStream<'a, 'c, UslpIntake, L>;

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{DiscardReason, FrameStream, IntakeItem, LenientFrameStream};
    use crate::config::tests::variable;

    const TRUNCATED: [u8; 12] = [0xC0, 0x15, 0x58, 0x85, 0xE5, 1, 2, 3, 4, 5, 6, 7];

    #[test]
    fn stream_accepts_valid_frames_and_discards_fill() {
        // req: USLP-014, USLP-015
        let table = variable();
        let mut unit: [u8; 27] = [0; 27];
        unit[..12].copy_from_slice(&TRUNCATED);
        unit[12..24].copy_from_slice(&TRUNCATED);
        unit[24..].copy_from_slice(&[0x55, 0x55, 0x55]);

        let mut stream = FrameStream::new(&unit, &table, 31);
        let first = stream.next().unwrap();
        assert!(matches!(first, IntakeItem::Accepted { ref frame, .. } if frame.len() == 12));
        let second = stream.next().unwrap();
        assert!(matches!(second, IntakeItem::Accepted { ref frame, .. } if frame.len() == 12));
        let third = stream.next().unwrap();
        assert!(
            matches!(third, IntakeItem::Discarded { reason: DiscardReason::Fill, octets, .. } if octets.len() == 3)
        );
        assert_eq!(stream.next(), None);
    }

    #[test]
    fn stream_discards_unexpected_version_and_continues() {
        // req: USLP-014, USLP-015
        let table = variable();
        // A truncated frame with version 0 instead of 12 (0x00 instead of 0xC0) followed by a valid frame
        let mut bad = TRUNCATED;
        bad[0] = 0x00;
        let mut unit = [0_u8; 24];
        unit[..12].copy_from_slice(&bad);
        unit[12..].copy_from_slice(&TRUNCATED);

        let mut stream = FrameStream::new(&unit, &table, 31);
        let first = stream.next().unwrap();
        match first {
            IntakeItem::Discarded { reason, octets, .. } => {
                assert_eq!(reason, DiscardReason::UnexpectedVersion);
                assert_eq!(octets.len(), 12);
            }
            IntakeItem::Accepted { .. } => panic!("expected version rejection"),
        }
        let second = stream.next().unwrap();
        assert!(matches!(second, IntakeItem::Accepted { ref frame, .. } if frame.len() == 12));
        assert_eq!(stream.next(), None);
    }

    #[test]
    fn stream_discards_non_truncated_frame_not_allowed() {
        // req: USLP-014, USLP-015
        let table = variable();
        // VC 4 requires truncated frames; a non-truncated frame is rejected with NonTruncatedFrameNotAllowed
        // 7-octet primary header: version 12 (0xC0), SCID 0x155, VCID 4, non-truncated flag (0)
        let bad = [0xC0, 0x15, 0x58, 0x84, 0x00, 0x10, 0x00];
        let mut stream = FrameStream::new(&bad, &table, 31);
        let first = stream.next().unwrap();
        match first {
            IntakeItem::Discarded { reason, octets, .. } => {
                assert_eq!(reason, DiscardReason::NonTruncatedFrameNotAllowed);
                assert_eq!(octets.len(), 7);
            }
            IntakeItem::Accepted { .. } => panic!("expected non-truncated rejection"),
        }
        assert_eq!(stream.next(), None);
    }

    #[test]
    fn item_accessors_and_remaining() {
        // req: USLP-014, USLP-015
        let table = variable();
        let mut stream = FrameStream::new(&TRUNCATED, &table, 31);
        assert_eq!(stream.remaining(), &TRUNCATED);
        let item = stream.next().unwrap();
        assert_eq!(item.octets(), &TRUNCATED);
        assert_eq!(item.len(), 12);
        assert!(!item.is_empty());
        assert_eq!(stream.remaining(), []);
        assert_eq!(stream.next(), None);

        // The lenient stream reports what is left too (API-030).
        let mut lenient = LenientFrameStream::new(&TRUNCATED, &table, 31);
        assert_eq!(lenient.remaining(), &TRUNCATED);
        let item = lenient.next().unwrap();
        assert_eq!(
            (item.octets(), item.len(), item.is_empty()),
            (&TRUNCATED[..], 12, false)
        );
        assert_eq!(lenient.remaining(), []);
        assert!(lenient.next().is_none());

        // Discarded item accessors
        let fill = [0x55_u8; 3];
        let lenient_fill = LenientFrameStream::new(&fill, &table, 31).next().unwrap();
        assert_eq!(lenient_fill.octets(), &fill);
        let mut stream_fill = FrameStream::new(&fill, &table, 31);
        let discarded = stream_fill.next().unwrap();
        assert_eq!(discarded.octets(), &fill);
        assert_eq!(discarded.len(), 3);
        assert!(!discarded.is_empty());
    }

    #[test]
    fn fill_above_the_bound_is_discarded_as_leftover_fill() {
        // req: USLP-015
        // OI-47: the rest that starts no valid header is Fill up to `max_fill` octets and
        // LeftoverFill above, in the strict and the lenient stream.
        let table = variable();
        let fill = [0x55_u8; 7];
        for max_fill in [6, 7] {
            let want = if max_fill == 6 {
                DiscardReason::LeftoverFill
            } else {
                DiscardReason::Fill
            };
            let mut strict = FrameStream::new(&fill, &table, max_fill);
            let mut lenient = LenientFrameStream::new(&fill, &table, max_fill);
            assert!(
                matches!(strict.next(), Some(IntakeItem::Discarded { reason, octets, .. })
                    if reason == want && octets.len() == 7)
            );
            assert!(
                matches!(lenient.next(), Some(IntakeItem::Discarded { reason, .. }) if reason == want)
            );
            assert_eq!((strict.next(), lenient.next()), (None, None));
        }
        // The two items name different fields: Fill the Frame Data Field, LeftoverFill the
        // Frame Length (OI-47).
        for (max_fill, field) in [
            (6, crate::field::FRAME_LEN),
            (7, crate::field::FRAME_DATA_FIELD),
        ] {
            let item = FrameStream::new(&fill, &table, max_fill).next().unwrap();
            assert!(
                matches!(item, IntakeItem::Discarded { error, .. } if error.field() == Some(field))
            );
        }
    }

    #[test]
    fn discard_reason_codes_and_validation_conversions() {
        // req: USLP-015
        use crate::checked::Reason;

        let pairs = [
            (
                DiscardReason::UnexpectedVersion,
                1,
                Some(Reason::UnexpectedVersion),
            ),
            (
                DiscardReason::UnexpectedSpacecraft,
                2,
                Some(Reason::UnexpectedSpacecraft),
            ),
            (DiscardReason::ReservedFlags, 3, Some(Reason::ReservedFlags)),
            (DiscardReason::ReservedSpare, 4, Some(Reason::ReservedSpare)),
            (DiscardReason::InvalidVcid, 5, Some(Reason::InvalidVcid)),
            (DiscardReason::InvalidMapId, 6, Some(Reason::InvalidMapId)),
            (
                DiscardReason::TruncatedFrameNotAllowed,
                7,
                Some(Reason::TruncatedFrameNotAllowed),
            ),
            (
                DiscardReason::VcfCountLengthMismatch,
                8,
                Some(Reason::VcfCountLengthMismatch),
            ),
            (
                DiscardReason::OcfFlagMismatch,
                9,
                Some(Reason::OcfFlagMismatch),
            ),
            (
                DiscardReason::FixedLengthMismatch,
                10,
                Some(Reason::FixedLengthMismatch),
            ),
            (DiscardReason::FrameTooLong, 11, Some(Reason::FrameTooLong)),
            (
                DiscardReason::FrameTooShort,
                12,
                Some(Reason::FrameTooShort),
            ),
            (
                DiscardReason::LengthMismatch,
                13,
                Some(Reason::LengthMismatch),
            ),
            (DiscardReason::FecfMismatch, 14, Some(Reason::FecfMismatch)),
            (
                DiscardReason::ConstructionRuleMismatch,
                15,
                Some(Reason::ConstructionRuleMismatch),
            ),
            (
                DiscardReason::PointerBeyondZone,
                16,
                Some(Reason::PointerBeyondZone),
            ),
            (
                DiscardReason::OidHeaderMismatch,
                17,
                Some(Reason::OidHeaderMismatch),
            ),
            (
                DiscardReason::NonTruncatedFrameNotAllowed,
                18,
                Some(Reason::NonTruncatedFrameNotAllowed),
            ),
            (DiscardReason::Fill, 20, None),
            (DiscardReason::LeftoverFill, 23, None),
            (
                DiscardReason::IncompleteFrame { declared_len: 20 },
                21,
                None,
            ),
            (DiscardReason::NoTruncatedLength, 22, None),
        ];

        for (reason, code, val) in pairs {
            assert_eq!(reason.code(), code);
            assert_eq!(reason.validation_reason(), val);
            if let Some(r) = val {
                assert_eq!(DiscardReason::from(r), reason);
            }
        }
    }

    #[test]
    fn stream_discards_no_truncated_length() {
        // req: USLP-014
        let table = variable();
        // Truncated frame (flag = 1) targeting VC 8, which has no truncated length configured
        let mut frame = TRUNCATED;
        frame[..4].copy_from_slice(&[0xC0, 0x15, 0x59, 0x01]);
        let mut stream = FrameStream::new(&frame, &table, 31);
        let item = stream.next().unwrap();
        assert!(matches!(
            item,
            IntakeItem::Discarded {
                reason: DiscardReason::NoTruncatedLength,
                ..
            }
        ));
        assert_eq!(stream.next(), None);
    }

    #[test]
    fn stream_handles_incomplete_frames_and_early_errors() {
        // req: USLP-014, USLP-015
        let table = variable();
        // VC 8 non-truncated frame declaring 20 bytes (0x00, 0x13), but only 10 bytes present
        let mut unit = [0_u8; 10];
        unit[..7].copy_from_slice(&[0xC0, 0x15, 0x59, 0x00, 0x00, 0x13, 0x00]);
        let mut stream = FrameStream::new(&unit, &table, 31);
        let item = stream.next().unwrap();
        match item {
            IntakeItem::Discarded { reason, octets, .. } => {
                assert_eq!(reason, DiscardReason::IncompleteFrame { declared_len: 20 });
                assert_eq!(octets.len(), 10);
            }
            IntakeItem::Accepted { .. } => panic!("expected incomplete discard"),
        }
        assert_eq!(stream.next(), None);

        // Same incomplete frame, but with unknown SCID 0x000 (check b fails before check d)
        let mut bad_scid = unit;
        bad_scid[..4].copy_from_slice(&[0xC0, 0x00, 0x09, 0x00]);
        let mut stream_scid = FrameStream::new(&bad_scid, &table, 31);
        let item_scid = stream_scid.next().unwrap();
        assert!(matches!(
            item_scid,
            IntakeItem::Discarded {
                reason: DiscardReason::UnexpectedSpacecraft,
                ..
            }
        ));
        assert_eq!(stream_scid.next(), None);
    }

    #[test]
    fn stream_handles_unexpected_version_variants() {
        // req: USLP-014, USLP-015
        let table = variable();
        // 1. Non-truncated with version 0, length 12 >= 7, available in rest
        let mut v0_avail = [0_u8; 12];
        v0_avail[..7].copy_from_slice(&[0x00, 0x15, 0x59, 0x00, 0x00, 0x0B, 0x00]);
        let mut stream1 = FrameStream::new(&v0_avail, &table, 31);
        let item1 = stream1.next().unwrap();
        assert!(matches!(
            item1,
            IntakeItem::Discarded {
                reason: DiscardReason::UnexpectedVersion,
                ..
            }
        ));
        assert_eq!(stream1.next(), None);

        // 1b. The shortest frame, 7 octets (the Frame Length field is 6), is discarded whole.
        let v0_least = [0x00, 0x15, 0x59, 0x00, 0x00, 0x06, 0x00];
        let mut stream1b = FrameStream::new(&v0_least, &table, 31);
        assert!(matches!(
            stream1b.next(),
            Some(IntakeItem::Discarded {
                reason: DiscardReason::UnexpectedVersion,
                octets,
                ..
            }) if octets.len() == 7
        ));
        assert_eq!(stream1b.next(), None);

        // 2. Non-truncated with version 0, raw.frame_len < 7 (e.g. 5) -> falls through to Fill
        let mut v0_short = [0_u8; 8];
        v0_short[..7].copy_from_slice(&[0x00, 0x15, 0x59, 0x00, 0x00, 0x04, 0x00]);
        let mut stream2 = FrameStream::new(&v0_short, &table, 31);
        let item2 = stream2.next().unwrap();
        assert!(matches!(
            item2,
            IntakeItem::Discarded {
                reason: DiscardReason::Fill,
                ..
            }
        ));
        assert_eq!(stream2.next(), None);

        // 3. Non-truncated with version 0, len > rest.len() -> split_at_checked is None -> falls through to Fill
        let mut v0_trunc = [0_u8; 8];
        v0_trunc[..7].copy_from_slice(&[0x00, 0x15, 0x59, 0x00, 0x00, 0x14, 0x00]);
        let mut stream3 = FrameStream::new(&v0_trunc, &table, 31);
        let item3 = stream3.next().unwrap();
        assert!(matches!(
            item3,
            IntakeItem::Discarded {
                reason: DiscardReason::Fill,
                ..
            }
        ));
        assert_eq!(stream3.next(), None);

        // 4. Truncated with version 0, targeting VC 8 (no truncated length) -> ? returns None -> falls through to Fill
        let mut v0_no_trunc = TRUNCATED;
        v0_no_trunc[..4].copy_from_slice(&[0x00, 0x15, 0x59, 0x01]);
        let mut stream4 = FrameStream::new(&v0_no_trunc, &table, 31);
        let item4 = stream4.next().unwrap();
        assert!(matches!(
            item4,
            IntakeItem::Discarded {
                reason: DiscardReason::Fill,
                ..
            }
        ));
        assert_eq!(stream4.next(), None);

        // 5. Non-truncated with version 0, rest.len() < 7 -> RawRest::parse is None -> ? returns None -> falls through to Fill
        let v0_fewer_than_7 = [0x00, 0x15, 0x59, 0x00, 0x00];
        let mut stream5 = FrameStream::new(&v0_fewer_than_7, &table, 31);
        let item5 = stream5.next().unwrap();
        assert!(matches!(
            item5,
            IntakeItem::Discarded {
                reason: DiscardReason::Fill,
                ..
            }
        ));
        assert_eq!(stream5.next(), None);
    }

    #[test]
    fn stream_discards_validation_failure_on_available_len() {
        // req: USLP-015
        let table = variable();
        // Non-truncated frame on VC 8 (12 bytes) with bad FECF (e.g. zeroed)
        let mut bad_fecf = [0_u8; 12];
        bad_fecf[..7].copy_from_slice(&[0xC0, 0x15, 0x59, 0x00, 0x00, 0x0B, 0x00]);
        let mut stream = FrameStream::new(&bad_fecf, &table, 31);
        let item = stream.next().unwrap();
        assert!(matches!(
            item,
            IntakeItem::Discarded {
                reason: DiscardReason::FecfMismatch,
                ..
            }
        ));
        assert_eq!(stream.next(), None);
    }
}
