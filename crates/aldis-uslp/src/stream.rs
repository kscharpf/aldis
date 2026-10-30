//! Unified USLP Frame Intake Stream: Delimiting, Fill Removal, and Channel Validation
//! (732.1 §4.3.11.2, §4.3.11.3; USLP-014, USLP-015).
//!
//! Replaces fragmented delimiting and validation passes with a single in-place iterator over
//! an ingested data unit (such as a CLTU's decoded codeblocks).
//!
//! A valid frame is yielded as [`IntakeItem::Accepted`], bundled with its verified [`VcConfig`]
//! and optional [`MapConfig`]. Discarded frames, fill data, and truncated headers are yielded as
//! [`IntakeItem::Discarded`], classified by a unified [`DiscardReason`].

use aldis_core::{AnomalySet, DecodeMode, Error, ErrorKind, Lenient};

use crate::anomaly::UslpAnomaly;
use crate::config::{MapConfig, VcConfig, VcLookup};
use crate::delimit::{Declared, declared_len};
use crate::field;
use crate::header::{RawRest, RawStart};
use crate::validate::{Reason, ValidatedUslpFrame};
use crate::view::parse;

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

/// An item yielded by [`FrameStream`] or [`LenientFrameStream`] (732.1 §4.3.11.2, §4.3.11.3;
/// USLP-014, USLP-015).
///
/// `F` is the accepted frame: a [`ValidatedUslpFrame`] from the strict stream, and the same in a
/// [`Lenient`] from the lenient one (API-016, API-030). A discarded item has no anomalies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntakeItem<'a, F = ValidatedUslpFrame<'a>> {
    /// A validated USLP Transfer Frame, bundled with its matched Virtual Channel and MAP configurations.
    Accepted {
        /// The validated frame view.
        frame: F,
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

impl<'a> IntakeItem<'a, ValidatedUslpFrame<'a>> {
    /// The octets of the item.
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

impl<'a> IntakeItem<'a, Lenient<ValidatedUslpFrame<'a>, UslpAnomaly>> {
    /// The octets of the item.
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

/// The position in a data unit and the channels, shared by the strict and the lenient stream.
#[derive(Debug, Clone)]
struct Intake<'a, 'c, L: VcLookup + ?Sized> {
    rest: &'a [u8],
    channels: &'c L,
    max_fill: usize,
}

/// In-place zero-allocation iterator over a received USLP data unit (732.1 §4.3.11.2, §4.3.11.3; USLP-014, USLP-015).
///
/// Fuses delimiting, fill removal, and channel validation in a single pass, in strict mode.
/// [`LenientFrameStream`] is the lenient one (API-030).
#[derive(Debug, Clone)]
pub struct FrameStream<'a, 'c, L: VcLookup + ?Sized> {
    intake: Intake<'a, 'c, L>,
}

/// The lenient twin of [`FrameStream`]: each item comes with the anomalies of the frame it
/// accepted, none for a discarded item (732.1 §4.3.11.2, §4.3.11.3; USLP-014, API-032).
#[derive(Debug, Clone)]
pub struct LenientFrameStream<'a, 'c, L: VcLookup + ?Sized> {
    intake: Intake<'a, 'c, L>,
}

impl<'a, 'c, L: VcLookup + ?Sized> FrameStream<'a, 'c, L> {
    /// Creates a new strict intake stream over one clean data unit, where `max_fill` is the
    /// longest fill the coding sublayer appends (six octets for the BCH codeblock, seven or 31
    /// for the LDPC codeblocks, 732.1 §4.3.11.2.4 note 2; OI-47).
    #[must_use]
    pub const fn new(data_unit: &'a [u8], channels: &'c L, max_fill: usize) -> Self {
        FrameStream {
            intake: Intake {
                rest: data_unit,
                channels,
                max_fill,
            },
        }
    }

    /// The remaining unconsumed octets of the data unit.
    #[must_use]
    pub const fn remaining(&self) -> &'a [u8] {
        self.intake.rest
    }
}

impl<'a, 'c, L: VcLookup + ?Sized> LenientFrameStream<'a, 'c, L> {
    /// Creates a new lenient intake stream over one clean data unit, where `max_fill` is the
    /// longest fill the coding sublayer appends (OI-47).
    #[must_use]
    pub const fn new(data_unit: &'a [u8], channels: &'c L, max_fill: usize) -> Self {
        LenientFrameStream {
            intake: Intake {
                rest: data_unit,
                channels,
                max_fill,
            },
        }
    }

    /// The remaining unconsumed octets of the data unit.
    #[must_use]
    pub const fn remaining(&self) -> &'a [u8] {
        self.intake.rest
    }
}

impl<'a, L: VcLookup + ?Sized> Iterator for FrameStream<'a, '_, L> {
    type Item = IntakeItem<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        // A strict decode has no anomalies; the empty set is dropped.
        self.intake
            .next_in(DecodeMode::Strict)
            .map(|item| item.map_frame(|(frame, _)| frame))
    }
}

impl<'a, L: VcLookup + ?Sized> Iterator for LenientFrameStream<'a, '_, L> {
    type Item = IntakeItem<'a, Lenient<ValidatedUslpFrame<'a>, UslpAnomaly>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.intake
            .next_in(DecodeMode::Lenient)
            .map(|item| item.map_frame(|(frame, found)| Lenient::new(frame, found)))
    }
}

impl<'a, L: VcLookup + ?Sized> Intake<'a, '_, L> {
    /// The next item; an accepted frame comes with its anomalies.
    fn next_in(
        &mut self,
        mode: DecodeMode,
    ) -> Option<IntakeItem<'a, (ValidatedUslpFrame<'a>, AnomalySet<UslpAnomaly>)>> {
        if self.rest.is_empty() {
            return None;
        }

        // If an unexpected-version frame is present with its declared length available in rest,
        // report DiscardReason::UnexpectedVersion and advance past it.
        let unexpected_version_frame = (|| {
            let start = RawStart::parse(self.rest)?;
            let error = start.check_version().err()?;
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
            let (frame_bytes, remaining) = self.rest.split_at_checked(len)?;
            Some((frame_bytes, remaining, error))
        })();
        if let Some((frame_bytes, remaining, error)) = unexpected_version_frame {
            self.rest = remaining;
            return Some(IntakeItem::Discarded {
                reason: DiscardReason::UnexpectedVersion,
                octets: frame_bytes,
                error,
            });
        }

        let len = match declared_len(self.rest, self.channels) {
            Some(Declared::Len(len)) => len,
            Some(Declared::TruncatedOnly) => self.rest.len(),
            Some(Declared::NoTruncatedLength) => {
                let octets = self.rest;
                self.rest = &[];
                return Some(IntakeItem::Discarded {
                    reason: DiscardReason::NoTruncatedLength,
                    octets,
                    error: Error::new(ErrorKind::UnknownChannel).with_field(field::VC_ID),
                });
            }
            None if self.rest.len() > self.max_fill => {
                // More than the coding sublayer appends: a Frame Length error may have
                // shortened a frame (732.1 §4.3.11.2.4 note 2; OI-47).
                let octets = self.rest;
                self.rest = &[];
                return Some(IntakeItem::Discarded {
                    reason: DiscardReason::LeftoverFill,
                    octets,
                    error: Error::new(ErrorKind::Malformed).with_field(field::FRAME_LEN),
                });
            }
            None => {
                let octets = self.rest;
                self.rest = &[];
                return Some(IntakeItem::Discarded {
                    reason: DiscardReason::Fill,
                    octets,
                    error: Error::new(ErrorKind::Malformed).with_field(field::FRAME_DATA_FIELD),
                });
            }
        };

        if let Some((frame_bytes, remaining)) = self.rest.split_at_checked(len) {
            self.rest = remaining;
            match parse(frame_bytes, self.channels, mode, true) {
                Ok(p) => Some(IntakeItem::Accepted {
                    frame: (ValidatedUslpFrame::new(p.frame), p.anomalies),
                    vc: p.vc,
                    map: p.map,
                }),
                Err(rejection) => Some(IntakeItem::Discarded {
                    reason: DiscardReason::from(rejection.reason()),
                    octets: frame_bytes,
                    error: rejection.error(),
                }),
            }
        } else {
            let octets = self.rest;
            self.rest = &[];
            match parse(octets, self.channels, mode, true) {
                Err(rejection) if rejection.reason() != Reason::LengthMismatch => {
                    Some(IntakeItem::Discarded {
                        reason: DiscardReason::from(rejection.reason()),
                        octets,
                        error: rejection.error(),
                    })
                }
                _ => Some(IntakeItem::Discarded {
                    reason: DiscardReason::IncompleteFrame { declared_len: len },
                    octets,
                    error: Error::incomplete(len),
                }),
            }
        }
    }
}

impl<L: VcLookup + ?Sized> core::iter::FusedIterator for FrameStream<'_, '_, L> {}

impl<L: VcLookup + ?Sized> core::iter::FusedIterator for LenientFrameStream<'_, '_, L> {}
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
        assert_eq!(stream.remaining(), &[]);
        assert_eq!(stream.next(), None);

        // The lenient stream reports what is left too (API-030).
        let mut lenient = LenientFrameStream::new(&TRUNCATED, &table, 31);
        assert_eq!(lenient.remaining(), &TRUNCATED);
        let item = lenient.next().unwrap();
        assert_eq!(
            (item.octets(), item.len(), item.is_empty()),
            (&TRUNCATED[..], 12, false)
        );
        assert_eq!(lenient.remaining(), &[]);
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
    }

    #[test]
    fn discard_reason_codes_and_validation_conversions() {
        // req: USLP-015
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
