//! Unified USLP Frame Intake Stream: Delimiting, Fill Removal, and Channel Validation
//! (732.1 §4.3.11.2, §4.3.11.3; USLP-014, USLP-015).
//!
//! Replaces fragmented delimiting and validation passes with a single in-place iterator over
//! an ingested data unit (such as a CLTU's decoded codeblocks).
//!
//! A valid frame is yielded as [`IntakeItem::Accepted`], bundled with its verified [`VcConfig`]
//! and optional [`MapConfig`]. Discarded frames, fill data, and truncated headers are yielded as
//! [`IntakeItem::Discarded`], classified by a unified [`DiscardReason`].

use aldis_core::{DecodeMode, Error, ErrorKind};

use crate::config::{MapConfig, VcConfig, VcLookup};
use crate::delimit::{Declared, declared_len};
use crate::field;
use crate::header::{RawRest, RawStart, VERSION};
use crate::validate::Reason;
use crate::view::{UslpFrame, parse};

/// Why wire octets or a USLP frame were discarded (732.1 §4.3.11.2, §4.3.11.3; USLP-014, USLP-015).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DiscardReason {
    /// Discarded as fill data (732.1 §4.3.11.2.4 a–b).
    Fill,
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
    /// 22 `NoTruncatedLength`.
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

/// An item yielded by [`FrameStream`] (732.1 §4.3.11.2, §4.3.11.3; USLP-014, USLP-015).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntakeItem<'a> {
    /// A validated USLP Transfer Frame, bundled with its matched Virtual Channel and MAP configurations.
    Accepted {
        /// The decoded frame view.
        frame: UslpFrame<'a>,
        /// The verified Virtual Channel configuration.
        vc: VcConfig,
        /// The verified MAP configuration, if present.
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
    /// The octets of the item.
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

/// In-place zero-allocation iterator over a received USLP data unit (732.1 §4.3.11.2, §4.3.11.3; USLP-014, USLP-015).
///
/// Fuses delimiting, fill removal, and channel validation in a single pass.
#[derive(Debug, Clone)]
pub struct FrameStream<'a, 'c, L: VcLookup + ?Sized> {
    rest: &'a [u8],
    channels: &'c L,
    mode: DecodeMode,
}

impl<'a, 'c, L: VcLookup + ?Sized> FrameStream<'a, 'c, L> {
    /// Creates a new intake stream over one clean data unit.
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

        // If an unexpected-version frame is present with its declared length available in rest,
        // pass it to parse to report DiscardReason::UnexpectedVersion and advance past it.
        let unexpected_version_frame = (|| {
            let start = RawStart::parse(self.rest)?;
            if start.version == VERSION {
                return None;
            }
            let len = if start.truncated {
                self.channels
                    .vc(start.channel.spacecraft_id, start.channel.vc_id)
                    .and_then(|vc| vc.truncated_frame_len())?
                    .get()
            } else {
                let raw = RawRest::parse(self.rest)?;
                if raw.frame_len < 7 {
                    return None;
                }
                raw.frame_len
            };
            self.rest.split_at_checked(len)
        })();
        if let Some((frame_bytes, remaining)) = unexpected_version_frame {
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

        match declared_len(self.rest, self.channels) {
            Some(Declared::Len(len)) => {
                if let Some((frame_bytes, remaining)) = self.rest.split_at_checked(len) {
                    self.rest = remaining;
                    match parse(frame_bytes, self.channels, self.mode, true) {
                        Ok((frame, vc, map)) => Some(IntakeItem::Accepted { frame, vc, map }),
                        Err(rejection) => Some(IntakeItem::Discarded {
                            reason: DiscardReason::from(rejection.reason()),
                            octets: frame_bytes,
                            error: rejection.error(),
                        }),
                    }
                } else {
                    let octets = self.rest;
                    self.rest = &[];
                    Some(IntakeItem::Discarded {
                        reason: DiscardReason::IncompleteFrame { declared_len: len },
                        octets,
                        error: Error::incomplete(len),
                    })
                }
            }
            Some(Declared::NoTruncatedLength) => {
                let octets = self.rest;
                self.rest = &[];
                Some(IntakeItem::Discarded {
                    reason: DiscardReason::NoTruncatedLength,
                    octets,
                    error: Error::new(ErrorKind::UnknownChannel).with_field(field::VC_ID),
                })
            }
            Some(Declared::TruncatedOnly) => {
                let octets = self.rest;
                self.rest = &[];
                Some(IntakeItem::Discarded {
                    reason: DiscardReason::NonTruncatedFrameNotAllowed,
                    octets,
                    error: Error::new(ErrorKind::InvalidField)
                        .with_field(field::END_OF_FRAME_PRIMARY_HEADER_FLAG),
                })
            }
            None => {
                let octets = self.rest;
                self.rest = &[];
                Some(IntakeItem::Discarded {
                    reason: DiscardReason::Fill,
                    octets,
                    error: Error::new(ErrorKind::Malformed).with_field(field::FRAME_DATA_FIELD),
                })
            }
        }
    }
}

impl<L: VcLookup + ?Sized> core::iter::FusedIterator for FrameStream<'_, '_, L> {}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{DiscardReason, FrameStream, IntakeItem};
    use crate::config::tests::variable;
    use aldis_core::DecodeMode;

    const TRUNCATED: [u8; 12] = [0xC0, 0x15, 0x58, 0x85, 0xE5, 1, 2, 3, 4, 5, 6, 7];

    #[test]
    fn stream_accepts_valid_frames_and_discards_fill() {
        // req: USLP-014, USLP-015
        let table = variable();
        let mut unit: [u8; 27] = [0; 27];
        unit[..12].copy_from_slice(&TRUNCATED);
        unit[12..24].copy_from_slice(&TRUNCATED);
        unit[24..].copy_from_slice(&[0x55, 0x55, 0x55]);

        let mut stream = FrameStream::new(&unit, &table, DecodeMode::Strict);
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

        let mut stream = FrameStream::new(&unit, &table, DecodeMode::Strict);
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
        let mut stream = FrameStream::new(&bad, &table, DecodeMode::Strict);
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
}
