//! Unified Frame Intake Stream: Delimiting, Fill Removal, and Channel Validation
//! (232.0 §4.4.9.2, §4.4.9.3; TC-020, TC-021).
//!
//! Replaces fragmented delimiting and validation passes with a single in-place iterator over
//! an ingested data unit (such as a CLTU's decoded codeblocks; 232.0 §4.4.9.2.3).
//!
//! The stream is `aldis_core::stream::FrameStream` over [`TcIntake`], the TC rules of the
//! [`Delimiter`]. A valid frame is yielded as [`IntakeItem::Accepted`], bundled with its verified [`VcConfig`]
//! and optional [`MapConfig`]. Corrupted frames, fill data, and truncated headers are yielded as
//! [`IntakeItem::Discarded`], classified by a unified [`DiscardReason`].

use aldis_core::lookup::{ChannelLookup, MapLookup, PhysicalLookup};
use aldis_core::stream::{Delimiter, Peek};
use aldis_core::{AnomalySet, Checked, DecodeMode, Error};

use crate::anomaly::TcAnomaly;
use crate::checked::Reason;
use crate::config::{MapConfig, PhysicalChannelConfig, VcConfig};
use crate::header::{MIN_FRAME_LEN, RawHeader, TcMapId, TcSpacecraftId, TcVcId};
use crate::view::{Channels, TcFrame, check_head};

/// Why wire octets or a frame were discarded (232.0 §4.4.9.2, §4.4.9.3; TC-020, TC-021).
///
/// Combines the delimiting causes (fill, truncation) and the 11 CCSDS Frame Validation Check
/// reasons into one classification.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DiscardReason {
    /// Discarded as fill data, no longer than the longest fill the coding sublayer appends
    /// (232.0 §4.4.9.2.6).
    Fill,
    /// Discarded: it does not start a valid header and is longer than the longest fill the
    /// coding sublayer appends, so a Frame Length error may have shortened a frame before it
    /// (232.0 §4.4.9.2.6 note 2; OI-47).
    LeftoverFill,
    /// Discarded as a truncated frame whose declared length exceeds remaining octets (232.0 §4.4.9.2.5).
    TruncatedFrame {
        /// The frame length declared in the header.
        declared_len: usize,
    },
    /// 1, check (a): Transfer Frame Version Number is not `00` (§4.1.2.2.2).
    UnexpectedVersion,
    /// 2, check (b): Spacecraft ID is not a valid Spacecraft ID of the Physical Channel (Table 5-1).
    UnexpectedSpacecraft,
    /// 3, check (c): Bypass Flag `0` with Control Command Flag `1` (Table 4-1; TC-002).
    ReservedFlags,
    /// 4, check (c), strict mode only: Reserved Spare is not `00` (§4.1.2.4.2; TC-003).
    ReservedSpare,
    /// 5, check (c): VCID is not valid on the Master Channel (§4.4.7.3; TC-022).
    InvalidVcid,
    /// 6, check (c): Frame Length exceeds the Maximum Transfer Frame Length (Tables 5-1, 5-3).
    FrameTooLong,
    /// 7, check (c): Frame Length leaves no data octet (§4.1.1; CMN-041).
    FrameTooShort,
    /// 8, check (d): Frame Length disagrees with octets present (§4.4.9.3.3 d).
    LengthMismatch,
    /// 9, check (e): FECF does not match recomputed CRC (§4.1.4.3; TC-006).
    FecfMismatch,
    /// 11, strict mode only: Type-C data field is not a recognized command (§4.1.3.3.4; TC-005).
    ReservedControlCommand,
}

impl DiscardReason {
    /// Stable numeric code for C ABI and telemetry classification.
    ///
    /// Codes 1–9 and 11 match the CCSDS Frame Validation Check reasons (10 is retired); 20 is Fill, 21 is `TruncatedFrame`, and 22 is `LeftoverFill`.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            DiscardReason::UnexpectedVersion => 1,
            DiscardReason::UnexpectedSpacecraft => 2,
            DiscardReason::ReservedFlags => 3,
            DiscardReason::ReservedSpare => 4,
            DiscardReason::InvalidVcid => 5,
            DiscardReason::FrameTooLong => 6,
            DiscardReason::FrameTooShort => 7,
            DiscardReason::LengthMismatch => 8,
            DiscardReason::FecfMismatch => 9,
            DiscardReason::ReservedControlCommand => 11,
            DiscardReason::Fill => 20,
            DiscardReason::TruncatedFrame { .. } => 21,
            DiscardReason::LeftoverFill => 22,
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
            DiscardReason::FrameTooLong => Some(Reason::FrameTooLong),
            DiscardReason::FrameTooShort => Some(Reason::FrameTooShort),
            DiscardReason::LengthMismatch => Some(Reason::LengthMismatch),
            DiscardReason::FecfMismatch => Some(Reason::FecfMismatch),
            DiscardReason::ReservedControlCommand => Some(Reason::ReservedControlCommand),
            DiscardReason::TruncatedFrame { .. }
            | DiscardReason::Fill
            | DiscardReason::LeftoverFill => None,
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
            Reason::FrameTooLong => DiscardReason::FrameTooLong,
            Reason::FrameTooShort => DiscardReason::FrameTooShort,
            Reason::LengthMismatch => DiscardReason::LengthMismatch,
            Reason::FecfMismatch => DiscardReason::FecfMismatch,
            Reason::ReservedControlCommand => DiscardReason::ReservedControlCommand,
        }
    }
}

/// The TC rules of the frame stream (232.0 §4.4.9.2, §4.4.9.3; TC-020, TC-021).
///
/// A header that parses with a Frame Length of at least [`MIN_FRAME_LEN`] declares a frame; a
/// frame that declares more octets than remain is discarded as truncated after the header
/// checks (a), (b), and (c) when its version is 0, and is fill otherwise (OI-41).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcIntake;

impl Delimiter for TcIntake {
    type Frame<'a> = TcFrame<'a>;
    type Anomaly = TcAnomaly;
    type Rejected = Reason;
    type Reason = DiscardReason;
    type Scid = TcSpacecraftId;
    type VcId = TcVcId;
    type MapId = TcMapId;
    type Vc = VcConfig;
    type Map = MapConfig;
    type Physical = PhysicalChannelConfig;

    const FILL: DiscardReason = DiscardReason::Fill;
    const LEFTOVER_FILL: DiscardReason = DiscardReason::LeftoverFill;

    fn peek<'a, L>(rest: &'a [u8], _channels: &L) -> Peek<'a, DiscardReason>
    where
        L: ChannelLookup<TcSpacecraftId, TcVcId, Vc = VcConfig>
            + MapLookup<TcSpacecraftId, TcVcId, TcMapId, Map = MapConfig>
            + PhysicalLookup<Physical = PhysicalChannelConfig>
            + ?Sized,
    {
        match RawHeader::parse(rest) {
            Some(h) if h.frame_len >= MIN_FRAME_LEN => Peek::frame(h.frame_len),
            // Unparseable header, frame_len < MIN_FRAME_LEN, or fill pattern (such as 0x55 with
            // TFVN 01; §4.4.9.2.6 note 2) is discarded as fill data taking the rest of the unit
            // (232.0 §4.4.9.2.6 b; OI-41).
            _ => Peek::Fill,
        }
    }

    fn incomplete<L>(
        rest: &[u8],
        declared_len: usize,
        channels: &L,
        mode: DecodeMode,
    ) -> Option<(DiscardReason, Error)>
    where
        L: ChannelLookup<TcSpacecraftId, TcVcId, Vc = VcConfig>
            + MapLookup<TcSpacecraftId, TcVcId, TcMapId, Map = MapConfig>
            + PhysicalLookup<Physical = PhysicalChannelConfig>
            + ?Sized,
    {
        // If fewer octets remain than declared and TFVN == 0, validate the header first:
        // Check (a) version, (b) spacecraft, and (c) flags, spares, VCID, FrameTooLong,
        // FrameTooShort take precedence before the length consistency check (d)
        // (232.0 §4.4.9.3; docs/standards/tc-frame.md §2.1).
        if !matches!(RawHeader::parse(rest), Some(h) if h.version == 0) {
            return None;
        }
        let mut anomalies = AnomalySet::empty();
        if let Err(rejection) = check_head(
            rest,
            channels,
            Channels::Required,
            mode,
            false,
            &mut anomalies,
        ) {
            return Some((DiscardReason::from(rejection.reason()), rejection.error()));
        }
        // A valid header declaring more octets than remain in the data unit is discarded as a
        // truncated frame (232.0 §4.4.9.2.5; OI-41).
        Some((
            DiscardReason::TruncatedFrame { declared_len },
            Error::incomplete(declared_len),
        ))
    }
}

/// An item yielded by [`FrameStream`] or [`LenientFrameStream`] (232.0 §4.4.9.2, §4.4.9.3;
/// TC-020, TC-021).
///
/// `F` is the accepted frame: a [`Checked`] frame from the strict stream, and the same in a
/// [`Lenient`](aldis_core::Lenient) from the lenient one (API-030).
pub type IntakeItem<'a, F = Checked<TcFrame<'a>>> = aldis_core::stream::IntakeItem<'a, TcIntake, F>;

/// In-place zero-allocation iterator over a received data unit (232.0 §4.4.9.2, §4.4.9.3;
/// TC-020, TC-021), in strict mode.
///
/// Fuses delimiting, fill removal, and channel validation in a single pass.
/// [`LenientFrameStream`] is its lenient twin (API-030).
pub type FrameStream<'a, 'c, L> = aldis_core::stream::FrameStream<'a, 'c, TcIntake, L>;

/// The lenient twin of [`FrameStream`]: the frames it accepts are [`Lenient`](aldis_core::Lenient) and carry the
/// anomalies of the deviations tolerated (232.0 §4.4.9.2, §4.4.9.3; TC-020, TC-021, API-032).
pub type LenientFrameStream<'a, 'c, L> =
    aldis_core::stream::LenientFrameStream<'a, 'c, TcIntake, L>;

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{DiscardReason, FrameStream, IntakeItem, LenientFrameStream};
    use crate::anomaly::TcAnomaly;
    use crate::config::tests::standard;

    const UNLOCK: [u8; 6] = [0x31, 0x55, 0x04, 0x05, 0x00, 0x00];

    #[test]
    fn stream_accepts_valid_frames_and_discards_fill() {
        // req: TC-020, TC-021
        let table = standard(false);
        let mut unit = UNLOCK.to_vec();
        unit.extend_from_slice(&UNLOCK);
        unit.extend_from_slice(&[0x55, 0x55, 0x55]);

        let mut stream = FrameStream::new(&unit, &table, 6);
        let first = stream.next().unwrap();
        assert!(matches!(first, IntakeItem::Accepted { ref frame, .. } if frame.len() == 6));
        let second = stream.next().unwrap();
        assert!(matches!(second, IntakeItem::Accepted { ref frame, .. } if frame.len() == 6));
        let third = stream.next().unwrap();
        assert!(
            matches!(third, IntakeItem::Discarded { reason: DiscardReason::Fill, octets, .. } if octets.len() == 3)
        );
        assert_eq!(stream.next(), None);
    }

    #[test]
    fn stream_discards_truncated_frame() {
        // req: TC-020
        let table = standard(false);
        let mut unit = UNLOCK.to_vec();
        unit.extend_from_slice(&[0x01, 0x55, 0x04, 0x09, 0x00, 0xAA]); // claims 10 bytes, has 6

        let mut stream = FrameStream::new(&unit, &table, 6);
        let first = stream.next().unwrap();
        assert!(matches!(first, IntakeItem::Accepted { .. }));

        let second = stream.next().unwrap();
        match second {
            IntakeItem::Discarded {
                reason,
                octets,
                error,
            } => {
                assert_eq!(reason, DiscardReason::TruncatedFrame { declared_len: 10 });
                assert_eq!(octets.len(), 6);
                assert_eq!(error.kind(), aldis_core::ErrorKind::Incomplete);
            }
            IntakeItem::Accepted { .. } => panic!("expected truncated frame discard"),
        }
        assert_eq!(stream.next(), None);
    }

    #[test]
    fn stream_continues_past_validation_rejection() {
        // req: TC-021
        let table = standard(true); // requires FECF, min frame length 8
        // Unlock frame claiming 8 octets with wrong FECF 0x0000 (fails check e, FecfMismatch)
        // followed by 3 bytes of fill
        let unit: [u8; 11] = [
            0x31, 0x55, 0x04, 0x07, 0x00, 0x00, 0x00, 0x00, 0x55, 0x55, 0x55,
        ];

        let mut stream = FrameStream::new(&unit, &table, 6);
        let first = stream.next().unwrap();
        match first {
            IntakeItem::Discarded { reason, octets, .. } => {
                assert_eq!(reason, DiscardReason::FecfMismatch);
                assert_eq!(octets.len(), 8);
            }
            IntakeItem::Accepted { .. } => panic!("expected validation rejection"),
        }
        let second = stream.next().unwrap();
        assert!(matches!(
            second,
            IntakeItem::Discarded {
                reason: DiscardReason::Fill,
                ..
            }
        ));
        assert_eq!(stream.next(), None);
    }

    #[test]
    fn stream_discards_unexpected_version_and_continues() {
        // req: TC-021
        let table = standard(false);
        // Header with version 01 (0x71) declaring 6 octets, followed by a valid Unlock frame
        let mut unit = [0_u8; 12];
        unit[..6].copy_from_slice(&[0x71, 0x55, 0x04, 0x05, 0x00, 0x00]);
        unit[6..].copy_from_slice(&UNLOCK);

        let mut stream = FrameStream::new(&unit, &table, 6);
        let first = stream.next().unwrap();
        match first {
            IntakeItem::Discarded { reason, octets, .. } => {
                assert_eq!(reason, DiscardReason::UnexpectedVersion);
                assert_eq!(octets.len(), 6);
            }
            IntakeItem::Accepted { .. } => panic!("expected version rejection"),
        }
        let second = stream.next().unwrap();
        assert!(matches!(second, IntakeItem::Accepted { ref frame, .. } if frame.len() == 6));
        assert_eq!(stream.next(), None);
    }

    #[test]
    fn stream_discards_frame_too_short_and_continues() {
        // req: TC-021
        let table = standard(true); // FECF required: minimum frame length is 8
        // Header with version 00 declaring 6 octets (leaving no room for FECF; FrameTooShort),
        // followed by a valid Unlock frame with FECF
        let mut unlock_fecf = [0x31, 0x55, 0x04, 0x07, 0x00, 0x00, 0x00, 0x00];
        let _ = aldis_core::crc::write_fecf(&mut unlock_fecf);

        let mut unit = [0_u8; 14];
        unit[..6].copy_from_slice(&[0x31, 0x55, 0x04, 0x05, 0x00, 0x00]);
        unit[6..].copy_from_slice(&unlock_fecf);

        let mut stream = FrameStream::new(&unit, &table, 6);
        let first = stream.next().unwrap();
        match first {
            IntakeItem::Discarded { reason, octets, .. } => {
                assert_eq!(reason, DiscardReason::FrameTooShort);
                assert_eq!(octets.len(), 6);
            }
            IntakeItem::Accepted { .. } => panic!("expected frame too short rejection"),
        }
        let second = stream.next().unwrap();
        assert!(matches!(second, IntakeItem::Accepted { ref frame, .. } if frame.len() == 8));
        assert_eq!(stream.next(), None);
    }

    #[test]
    fn item_accessors_and_remaining() {
        // req: TC-020, TC-021
        let table = standard(false);
        let mut stream = FrameStream::new(&UNLOCK, &table, 6);
        assert_eq!(stream.remaining(), &UNLOCK);
        let item = stream.next().unwrap();
        assert_eq!(item.octets(), &UNLOCK);
        assert_eq!(item.len(), 6);
        assert!(!item.is_empty());
        assert_eq!(stream.remaining(), []);
        assert_eq!(stream.next(), None);

        // Also test discarded item accessors
        let fill = [0x55_u8; 3];
        let mut stream_fill = FrameStream::new(&fill, &table, 6);
        let discarded = stream_fill.next().unwrap();
        assert_eq!(discarded.octets(), &fill);
        assert_eq!(discarded.len(), 3);
        assert!(!discarded.is_empty());
    }

    #[test]
    fn fill_above_the_bound_is_discarded_as_leftover_fill() {
        // req: TC-020
        // OI-47: the rest that starts no valid header is Fill up to `max_fill` octets and
        // LeftoverFill above, in the strict and the lenient stream.
        let table = standard(false);
        let mut unit = UNLOCK.to_vec();
        unit.extend_from_slice(&[0x55; 7]);
        for max_fill in [6, 7] {
            let mut strict = FrameStream::new(&unit, &table, max_fill);
            let mut lenient = LenientFrameStream::new(&unit, &table, max_fill);
            assert!(matches!(strict.next(), Some(IntakeItem::Accepted { .. })));
            assert!(matches!(lenient.next(), Some(IntakeItem::Accepted { .. })));
            let want = if max_fill == 6 {
                DiscardReason::LeftoverFill
            } else {
                DiscardReason::Fill
            };
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
            let item = FrameStream::new(&unit, &table, max_fill).nth(1).unwrap();
            assert!(
                matches!(item, IntakeItem::Discarded { error, .. } if error.field() == Some(field))
            );
        }
    }

    #[test]
    fn discard_reason_codes_and_validation_conversions() {
        // req: TC-021
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
            (DiscardReason::FrameTooLong, 6, Some(Reason::FrameTooLong)),
            (DiscardReason::FrameTooShort, 7, Some(Reason::FrameTooShort)),
            (
                DiscardReason::LengthMismatch,
                8,
                Some(Reason::LengthMismatch),
            ),
            (DiscardReason::FecfMismatch, 9, Some(Reason::FecfMismatch)),
            (
                DiscardReason::ReservedControlCommand,
                11,
                Some(Reason::ReservedControlCommand),
            ),
            (DiscardReason::Fill, 20, None),
            (DiscardReason::TruncatedFrame { declared_len: 10 }, 21, None),
            (DiscardReason::LeftoverFill, 22, None),
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
    fn stream_discards_truncated_header_check_failure() {
        // req: TC-020, TC-021
        let table = standard(false);
        // Header claiming 10 bytes (h.frame_len == 10), but SCID is 0x000 (UnexpectedSpacecraft)
        // rest has only 6 bytes, so fewer octets remain than declared, and check_head fails on check (b).
        let unit: [u8; 6] = [0x00, 0x00, 0x04, 0x09, 0x00, 0x00];
        let mut stream = FrameStream::new(&unit, &table, 6);
        let item = stream.next().unwrap();
        match item {
            IntakeItem::Discarded { reason, octets, .. } => {
                assert_eq!(reason, DiscardReason::UnexpectedSpacecraft);
                assert_eq!(octets.len(), 6);
            }
            IntakeItem::Accepted { .. } => panic!("expected spacecraft discard"),
        }
        assert_eq!(stream.next(), None);

        // Header with version 1 claiming 10 bytes when only 6 bytes are present.
        // Falls through the if h.version == 0 check to Fill.
        let v1_unit: [u8; 6] = [0x40, 0x55, 0x04, 0x09, 0x00, 0x00];
        let mut stream_v1 = FrameStream::new(&v1_unit, &table, 6);
        let item_v1 = stream_v1.next().unwrap();
        assert_eq!(item_v1.octets().len(), 6);
        assert!(matches!(
            item_v1,
            IntakeItem::Discarded {
                reason: DiscardReason::Fill,
                ..
            }
        ));
        assert_eq!(stream_v1.next(), None);

        // Header claiming 5 bytes (< MIN_FRAME_LEN 6).
        let short_hdr: [u8; 6] = [0x00, 0x55, 0x04, 0x04, 0x00, 0x00];
        let mut stream_short = FrameStream::new(&short_hdr, &table, 6);
        let item_short = stream_short.next().unwrap();
        assert!(matches!(
            item_short,
            IntakeItem::Discarded {
                reason: DiscardReason::Fill,
                ..
            }
        ));
        assert_eq!(stream_short.next(), None);
    }

    #[test]
    fn stream_accepts_segmented_frame_with_map() {
        // req: TC-020
        let table = standard(false);
        // VC 2 (segmented): SCID 0x155, VCID 2, len 7 (8 octets), seq unsegmented (0xC0 = MAP 0)
        let frame: [u8; 8] = [0x01, 0x55, 0x08, 0x07, 0x00, 0xC0, 0x11, 0x22];
        let mut stream = FrameStream::new(&frame, &table, 6);
        let item = stream.next().unwrap();
        match item {
            IntakeItem::Accepted { frame, map, .. } => {
                assert_eq!(frame.len(), 8);
                assert!(map.is_some());
            }
            IntakeItem::Discarded { .. } => panic!("expected accepted frame"),
        }
        assert_eq!(stream.next(), None);
    }

    #[test]
    fn lenient_stream_reports_anomalies() {
        // req: TC-020, API-030, API-032
        let table = standard(false);
        let spare = [0x35, 0x55, 0x04, 0x05, 0x00, 0x00];
        let mut unit = UNLOCK.to_vec();
        unit.extend_from_slice(&spare);
        let mut strict = FrameStream::new(&unit, &table, 6);
        assert!(matches!(strict.next(), Some(IntakeItem::Accepted { .. })));
        assert!(matches!(
            strict.next(),
            Some(IntakeItem::Discarded {
                reason: DiscardReason::ReservedSpare,
                ..
            })
        ));
        let mut lenient = LenientFrameStream::new(&unit, &table, 6);
        let first = lenient.next().unwrap();
        assert_eq!((first.len(), first.is_empty()), (6, false));
        assert!(
            matches!(first, IntakeItem::Accepted { ref frame, .. } if frame.anomalies().is_empty())
        );
        let second = lenient.next().unwrap();
        assert!(matches!(second, IntakeItem::Accepted { ref frame, .. }
            if frame.anomalies().contains(TcAnomaly::ReservedSpareSet)));
        assert_eq!((lenient.next(), lenient.remaining().len()), (None, 0));
        // A discarded item is the same in both modes, and it reports its octets.
        let cut = &UNLOCK[..4];
        let discarded = LenientFrameStream::new(cut, &table, 6).next().unwrap();
        assert_eq!((discarded.octets(), discarded.len()), (cut, 4));
    }
}
