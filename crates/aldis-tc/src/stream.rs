//! Unified Frame Intake Stream: Delimiting, Fill Removal, and Channel Validation
//! (232.0 §4.4.9.2, §4.4.9.3; TC-020, TC-021).
//!
//! Replaces fragmented delimiting and validation passes with a single in-place iterator over
//! an ingested data unit (such as a CLTU's decoded codeblocks; 232.0 §4.4.9.2.3).
//!
//! A valid frame is yielded as [`IntakeItem::Accepted`], bundled with its verified [`VcConfig`]
//! and optional [`MapConfig`]. Corrupted frames, fill data, and truncated headers are yielded as
//! [`IntakeItem::Discarded`], classified by a unified [`DiscardReason`].

use aldis_core::{AnomalySet, DecodeMode, Error, ErrorKind};

use crate::config::{MapConfig, VcConfig, VcLookup};
use crate::header::{MIN_FRAME_LEN, RawHeader};
use crate::validate::Reason;
use crate::view::{TcFrame, check_head, parse};

/// Why wire octets or a frame were discarded (232.0 §4.4.9.2, §4.4.9.3; TC-020, TC-021).
///
/// Combines the delimiting causes (fill, truncation) and the 11 CCSDS Frame Validation Check
/// reasons into one classification.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DiscardReason {
    /// Discarded as fill data (232.0 §4.4.9.2.6).
    Fill,
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
    /// Codes 1–11 match the CCSDS Frame Validation Check reasons; 20 is Fill and 21 is `TruncatedFrame`.
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
            DiscardReason::TruncatedFrame { .. } | DiscardReason::Fill => None,
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

/// An item yielded by [`FrameStream`] (232.0 §4.4.9.2, §4.4.9.3; TC-020, TC-021).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntakeItem<'a> {
    /// A validated frame, bundled with its matched Virtual Channel and MAP configurations.
    Accepted {
        /// The decoded frame view.
        frame: TcFrame<'a>,
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

impl<'a> IntakeItem<'a> {
    /// The octets of the item (the accepted frame or discarded octets).
    #[must_use]
    pub const fn octets(&self) -> &'a [u8] {
        match self {
            IntakeItem::Accepted { frame, .. } => frame.octets(),
            IntakeItem::Discarded { octets, .. } => octets,
        }
    }

    /// Length in octets consumed from the input stream.
    #[must_use]
    pub const fn len(&self) -> usize {
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
/// Fuses delimiting, fill removal, and channel validation in a single pass.
#[derive(Debug, Clone)]
pub struct FrameStream<'a, 'c, L: VcLookup + ?Sized> {
    rest: &'a [u8],
    channels: &'c L,
    mode: DecodeMode,
}

impl<'a, 'c, L: VcLookup + ?Sized> FrameStream<'a, 'c, L> {
    /// Creates a new intake stream over one clean data unit (such as a CLTU's decoded codeblocks).
    #[must_use]
    pub const fn new(data_unit: &'a [u8], channels: &'c L, mode: DecodeMode) -> Self {
        FrameStream {
            rest: data_unit,
            channels,
            mode,
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
        if self.rest.is_empty() {
            return None;
        }

        if let Some(h) = RawHeader::parse(self.rest)
            && h.frame_len >= MIN_FRAME_LEN
        {
            // If the declared length is available in self.rest, reconstruct and validate the frame.
            if let Some((frame_bytes, remaining)) = self.rest.split_at_checked(h.frame_len) {
                self.rest = remaining;
                return match parse(frame_bytes, self.channels, self.mode, true) {
                    Ok((frame, vc, map)) => Some(IntakeItem::Accepted { frame, vc, map }),
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
                if let Err(rejection) =
                    check_head(self.rest, self.channels, self.mode, false, &mut anomalies)
                {
                    let octets = self.rest;
                    self.rest = &[];
                    return Some(IntakeItem::Discarded {
                        reason: DiscardReason::from(rejection.reason()),
                        octets,
                        error: rejection.error(),
                    });
                }

                // A valid header declaring more octets than remain in the data unit is discarded as a
                // truncated frame (232.0 §4.4.9.2.5; OI-41).
                let octets = self.rest;
                self.rest = &[];
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
        let octets = self.rest;
        self.rest = &[];
        Some(IntakeItem::Discarded {
            reason: DiscardReason::Fill,
            octets,
            error: Error::new(ErrorKind::Malformed).with_field(crate::field::FRAME_DATA_FIELD),
        })
    }
}

impl<L: VcLookup + ?Sized> core::iter::FusedIterator for FrameStream<'_, '_, L> {}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{DiscardReason, FrameStream, IntakeItem};
    use crate::config::tests::standard;
    use aldis_core::DecodeMode;

    const UNLOCK: [u8; 6] = [0x31, 0x55, 0x04, 0x05, 0x00, 0x00];

    #[test]
    fn stream_accepts_valid_frames_and_discards_fill() {
        // req: TC-020, TC-021
        let table = standard(false);
        let mut unit = UNLOCK.to_vec();
        unit.extend_from_slice(&UNLOCK);
        unit.extend_from_slice(&[0x55, 0x55, 0x55]);

        let mut stream = FrameStream::new(&unit, &table, DecodeMode::Strict);
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

        let mut stream = FrameStream::new(&unit, &table, DecodeMode::Strict);
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

        let mut stream = FrameStream::new(&unit, &table, DecodeMode::Strict);
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

        let mut stream = FrameStream::new(&unit, &table, DecodeMode::Strict);
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

        let mut stream = FrameStream::new(&unit, &table, DecodeMode::Strict);
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
}
