//! Unified Frame Intake Stream: Delimiting, Fill Removal, and Channel Validation
//! (232.0 §4.4.9.2, §4.4.9.3; TC-020, TC-021).
//!
//! Replaces fragmented delimiting and validation passes with a single in-place iterator over
//! an ingested data unit (such as a CLTU's decoded codeblocks; 232.0 §4.4.9.2.3).
//!
//! A valid frame is yielded as [`IntakeItem::Accepted`], bundled with its verified [`VcConfig`]
//! and optional [`MapConfig`]. Corrupted frames, fill data, and truncated headers are yielded as
//! [`IntakeItem::Discarded`], classified by a unified [`DiscardReason`].

use aldis_core::{AnomalySet, DecodeMode, Error, ErrorKind, Lenient};

use crate::anomaly::TcAnomaly;
use crate::config::{MapConfig, VcConfig, VcLookup};
use crate::header::{MIN_FRAME_LEN, RawHeader};
use crate::validate::{Reason, ValidatedTcFrame};
use crate::view::{check_head, parse};

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
    /// 10: MAP ID of the Segment Header is not valid on the VC (§4.4.3.3; TC-022).
    InvalidMapId,
    /// 11, strict mode only: Type-C data field is not a recognized command (§4.1.3.3.4; TC-005).
    ReservedControlCommand,
}

impl DiscardReason {
    /// Stable numeric code for C ABI and telemetry classification.
    ///
    /// Codes 1–11 match the CCSDS Frame Validation Check reasons; 20 is Fill, 21 is `TruncatedFrame`, and 22 is `LeftoverFill`.
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
            DiscardReason::InvalidMapId => 10,
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
            DiscardReason::InvalidMapId => Some(Reason::InvalidMapId),
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
            Reason::InvalidMapId => DiscardReason::InvalidMapId,
            Reason::ReservedControlCommand => DiscardReason::ReservedControlCommand,
        }
    }
}

/// An item yielded by [`FrameStream`] or [`LenientFrameStream`] (232.0 §4.4.9.2, §4.4.9.3;
/// TC-020, TC-021).
///
/// `F` is the accepted frame: a [`ValidatedTcFrame`] from the strict stream, and the same in a
/// [`Lenient`] from the lenient one (API-030).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntakeItem<'a, F = ValidatedTcFrame<'a>> {
    /// A validated frame, bundled with its matched Virtual Channel and MAP configurations.
    Accepted {
        /// The validated frame view.
        frame: F,
        /// The verified Virtual Channel configuration.
        vc: VcConfig,
        /// The verified MAP configuration, if a Segment Header was present.
        map: Option<MapConfig>,
    },
    /// Discarded wire octets with the reason and underlying error.
    Discarded {
        /// The reason for discard.
        reason: DiscardReason,
        /// The discarded octets.
        octets: &'a [u8],
        /// The error detail of the rejection.
        error: Error,
    },
}

impl<'a, F> IntakeItem<'a, F> {
    /// The item with its accepted frame converted by `convert`; a discarded item is unchanged.
    pub fn map_frame<G>(self, convert: impl FnOnce(F) -> G) -> IntakeItem<'a, G> {
        match self {
            IntakeItem::Accepted { frame, vc, map } => IntakeItem::Accepted {
                frame: convert(frame),
                vc,
                map,
            },
            IntakeItem::Discarded {
                reason,
                octets,
                error,
            } => IntakeItem::Discarded {
                reason,
                octets,
                error,
            },
        }
    }
}

impl<'a> IntakeItem<'a, ValidatedTcFrame<'a>> {
    /// The octets of the item (the accepted frame or discarded octets).
    #[must_use]
    pub fn octets(&self) -> &'a [u8] {
        match self {
            IntakeItem::Accepted { frame, .. } => frame.octets(),
            IntakeItem::Discarded { octets, .. } => octets,
        }
    }

    /// Length in octets consumed from the input stream.
    #[must_use]
    pub fn len(&self) -> usize {
        self.octets().len()
    }

    /// Always `false`: every intake item consumes at least one octet.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }
}

impl<'a> IntakeItem<'a, Lenient<ValidatedTcFrame<'a>, TcAnomaly>> {
    /// The octets of the item (the accepted frame or discarded octets).
    #[must_use]
    pub fn octets(&self) -> &'a [u8] {
        match self {
            IntakeItem::Accepted { frame, .. } => frame.octets(),
            IntakeItem::Discarded { octets, .. } => octets,
        }
    }

    /// Length in octets consumed from the input stream.
    #[must_use]
    pub fn len(&self) -> usize {
        self.octets().len()
    }

    /// Always `false`: every intake item consumes at least one octet.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }
}

/// In-place zero-allocation iterator over a received data unit (232.0 §4.4.9.2, §4.4.9.3; TC-020, TC-021).
///
/// Fuses delimiting, fill removal, and channel validation in a single pass, in strict mode.
/// [`LenientFrameStream`] is its lenient twin (API-030).
#[derive(Debug, Clone)]
pub struct FrameStream<'a, 'c, L: VcLookup + ?Sized> {
    rest: &'a [u8],
    channels: &'c L,
    max_fill: usize,
}

impl<'a, 'c, L: VcLookup + ?Sized> FrameStream<'a, 'c, L> {
    /// Creates a new intake stream over one clean data unit (such as a CLTU's decoded
    /// codeblocks), where `max_fill` is the longest fill the coding sublayer appends (six
    /// octets for the BCH codeblock, 232.0 §4.4.9.2.6 note 2; OI-47).
    #[must_use]
    pub const fn new(data_unit: &'a [u8], channels: &'c L, max_fill: usize) -> Self {
        FrameStream {
            rest: data_unit,
            channels,
            max_fill,
        }
    }

    /// The remaining unconsumed octets of the data unit.
    #[must_use]
    pub const fn remaining(&self) -> &'a [u8] {
        self.rest
    }
}

impl<'a, L: VcLookup + ?Sized> Iterator for FrameStream<'a, '_, L> {
    type Item = IntakeItem<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        step(
            &mut self.rest,
            self.channels,
            self.max_fill,
            DecodeMode::Strict,
        )
        .map(|item| item.map_frame(|(frame, _)| frame))
    }
}

impl<L: VcLookup + ?Sized> core::iter::FusedIterator for FrameStream<'_, '_, L> {}

/// The lenient twin of [`FrameStream`]: the frames it accepts are [`Lenient`] and carry the
/// anomalies of the deviations tolerated (232.0 §4.4.9.2, §4.4.9.3; TC-020, TC-021, API-032).
#[derive(Debug, Clone)]
pub struct LenientFrameStream<'a, 'c, L: VcLookup + ?Sized> {
    rest: &'a [u8],
    channels: &'c L,
    max_fill: usize,
}

impl<'a, 'c, L: VcLookup + ?Sized> LenientFrameStream<'a, 'c, L> {
    /// Creates a new lenient intake stream over one clean data unit, where `max_fill` is the
    /// longest fill the coding sublayer appends (OI-47).
    #[must_use]
    pub const fn new(data_unit: &'a [u8], channels: &'c L, max_fill: usize) -> Self {
        LenientFrameStream {
            rest: data_unit,
            channels,
            max_fill,
        }
    }

    /// The remaining unconsumed octets of the data unit.
    #[must_use]
    pub const fn remaining(&self) -> &'a [u8] {
        self.rest
    }
}

impl<'a, L: VcLookup + ?Sized> Iterator for LenientFrameStream<'a, '_, L> {
    type Item = IntakeItem<'a, Lenient<ValidatedTcFrame<'a>, TcAnomaly>>;

    fn next(&mut self) -> Option<Self::Item> {
        step(
            &mut self.rest,
            self.channels,
            self.max_fill,
            DecodeMode::Lenient,
        )
        .map(|item| item.map_frame(|(frame, anomalies)| Lenient::new(frame, anomalies)))
    }
}

impl<L: VcLookup + ?Sized> core::iter::FusedIterator for LenientFrameStream<'_, '_, L> {}

/// One step of both streams: the next item of `rest`, which it advances.
fn step<'a, L: VcLookup + ?Sized>(
    rest: &mut &'a [u8],
    channels: &L,
    max_fill: usize,
    mode: DecodeMode,
) -> Option<IntakeItem<'a, (ValidatedTcFrame<'a>, AnomalySet<TcAnomaly>)>> {
    if rest.is_empty() {
        return None;
    }

    if let Some(h) = RawHeader::parse(rest)
        && h.frame_len >= MIN_FRAME_LEN
    {
        // If the declared length is available in `rest`, reconstruct and validate the frame.
        if let Some((frame_bytes, remaining)) = rest.split_at_checked(h.frame_len) {
            *rest = remaining;
            return match parse(frame_bytes, channels, mode, true) {
                Ok(p) => Some(IntakeItem::Accepted {
                    frame: (ValidatedTcFrame::new(p.frame), p.anomalies),
                    vc: p.vc,
                    map: p.map,
                }),
                Err(rejection) => Some(IntakeItem::Discarded {
                    reason: DiscardReason::from(rejection.reason()),
                    octets: frame_bytes,
                    error: rejection.error(),
                }),
            };
        }

        // If fewer octets remain than declared and TFVN == 0, validate the header first:
        // Check (a) version, (b) spacecraft, and (c) flags, spares, VCID, FrameTooLong, FrameTooShort
        // take precedence before the length consistency check (d) (232.0 §4.4.9.3; docs/standards/tc-frame.md §2.1).
        if h.version == 0 {
            let mut anomalies = AnomalySet::empty();
            if let Err(rejection) = check_head(rest, channels, mode, false, &mut anomalies) {
                let octets = *rest;
                *rest = &[];
                return Some(IntakeItem::Discarded {
                    reason: DiscardReason::from(rejection.reason()),
                    octets,
                    error: rejection.error(),
                });
            }

            // A valid header declaring more octets than remain in the data unit is discarded as a
            // truncated frame (232.0 §4.4.9.2.5; OI-41).
            let octets = *rest;
            *rest = &[];
            return Some(IntakeItem::Discarded {
                reason: DiscardReason::TruncatedFrame {
                    declared_len: h.frame_len,
                },
                octets,
                error: Error::incomplete(h.frame_len),
            });
        }
    }

    // Unparseable header, frame_len < MIN_FRAME_LEN, or fill pattern (such as 0x55 with TFVN 01; §4.4.9.2.6 note 2)
    // is discarded as fill data taking the rest of the unit (232.0 §4.4.9.2.6 b; OI-41).
    let octets = *rest;
    *rest = &[];
    if octets.len() > max_fill {
        // More than the coding sublayer appends: a Frame Length error may have shortened a frame
        // (232.0 §4.4.9.2.6 note 2; OI-47).
        return Some(IntakeItem::Discarded {
            reason: DiscardReason::LeftoverFill,
            octets,
            error: Error::new(ErrorKind::Malformed).with_field(crate::field::FRAME_LEN),
        });
    }
    Some(IntakeItem::Discarded {
        reason: DiscardReason::Fill,
        octets,
        error: Error::new(ErrorKind::Malformed).with_field(crate::field::FRAME_DATA_FIELD),
    })
}

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
        assert_eq!(stream.remaining(), &[]);
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
    }

    #[test]
    fn discard_reason_codes_and_validation_conversions() {
        // req: TC-021
        use crate::validate::Reason;

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
            (DiscardReason::InvalidMapId, 10, Some(Reason::InvalidMapId)),
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
