//! Packet Secondary Header layouts and the split of the Packet Data Field (133.0 §4.1.4;
//! SPP-007, SPP-008, OI-30).
//!
//! The layout is a managed parameter per APID (133.0 Table 5-1). `aldis-spp` keeps no APID
//! table: the caller looks the layout up and passes it to
//! [`SpacePacket::split`](crate::view::SpacePacket::split).

use core::num::NonZeroUsize;

use aldis_core::{DecodeMode, Error, ErrorDetail, ErrorKind, FieldId, Result};
#[cfg(feature = "time")]
use aldis_time::{Decoded, PField, TimeFormat, decode_implicit, decode_with_pfield};

use crate::header::{invalid, len_u64};

/// The secondary header of one APID (133.0 §4.1.4.2.1.4–5, SPP-007, SPP-008).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SecondaryHeaderLayout {
    /// The APID's packets have no secondary header.
    Absent,
    /// A secondary header of a fixed number of octets whose contents the caller interprets; it
    /// counts as an Ancillary Data Field only (133.0 §4.1.4.2.1.5 b).
    Opaque(NonZeroUsize),
    /// A Time Code Field, optionally followed by an Ancillary Data Field (133.0
    /// §4.1.4.2.1.5 a, c; SPP-008).
    #[cfg(feature = "time")]
    Time(TimeHeaderLayout),
}

/// `LengthMismatch` (`secondary_header`): the data field holds `found` octets but the secondary
/// header needs `expected`, or at least `expected` when it ends inside a time code's P-field
/// extension.
fn too_short(expected: usize, found: usize) -> Error {
    Error::new(ErrorKind::LengthMismatch)
        .with_field(FieldId::new("secondary_header"))
        .with_detail(ErrorDetail::Expected {
            expected: len_u64(expected),
            found: len_u64(found),
        })
}

/// A time secondary header: a CUC or CDS Time Code Field, with or without its P-field, then
/// `ancillary_len` octets of Ancillary Data Field (133.0 §4.1.4.2.2, SPP-008).
#[cfg(feature = "time")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimeHeaderLayout {
    format: TimeFormat,
    pfield: bool,
    ancillary_len: usize,
}

#[cfg(feature = "time")]
impl TimeHeaderLayout {
    /// A layout for the time code `format`, whose P-field is present when `pfield` is set
    /// (133.0 §4.1.4.2.2.4–5), followed by `ancillary_len` ancillary octets (SPP-008).
    ///
    /// # Errors
    /// `InvalidConfig` (`time_code`) for a code other than CUC or CDS: 133.0 §4.1.4.2.2.2
    /// allows only the "segmented binary or unsegmented binary" codes of 301.0 (OI-30).
    pub const fn new(
        format: TimeFormat,
        pfield: bool,
        ancillary_len: usize,
    ) -> Result<TimeHeaderLayout> {
        match format {
            TimeFormat::Cuc(_) | TimeFormat::Cds(_) => Ok(TimeHeaderLayout {
                format,
                pfield,
                ancillary_len,
            }),
            TimeFormat::Ccs(_) | TimeFormat::Agency(_) => {
                Err(Error::new(ErrorKind::InvalidConfig).with_field(FieldId::new("time_code")))
            }
        }
    }

    /// The configured time code format; with a P-field, its time code ID is the one packets
    /// must carry (133.0 §4.1.4.2.2.3).
    #[must_use]
    pub const fn format(&self) -> TimeFormat {
        self.format
    }

    /// Whether the Time Code Field starts with its P-field (133.0 §4.1.4.2.2.5).
    #[must_use]
    pub const fn has_pfield(&self) -> bool {
        self.pfield
    }

    /// Length of the Ancillary Data Field in octets (133.0 §4.1.4.2.3).
    #[must_use]
    pub const fn ancillary_len(&self) -> usize {
        self.ancillary_len
    }

    /// Decodes the Time Code Field at the start of `data` (SPP-008, OI-30).
    fn decode(&self, data: &[u8], mode: DecodeMode) -> Result<Decoded> {
        // `needed` counts the time code only; the Ancillary Data Field follows it.
        let short = |e: Error| match (e.kind(), e.detail()) {
            (ErrorKind::Incomplete, ErrorDetail::Needed(needed)) => {
                too_short(needed.saturating_add(self.ancillary_len), data.len())
            }
            _ => e,
        };
        if !self.pfield {
            return decode_implicit(data, &self.format, mode).map_err(short);
        }
        // The characteristics may change, the code may not (133.0 §4.1.4.2.2.3–4).
        let id = PField::parse(data).map_err(short)?.id();
        let expected = self.format.pfield().id();
        if id != expected {
            return Err(invalid("time_code_id").with_detail(ErrorDetail::Expected {
                expected: u64::from(expected.bits()),
                found: u64::from(id.bits()),
            }));
        }
        decode_with_pfield(data, mode).map_err(short)
    }
}

impl SecondaryHeaderLayout {
    /// Splits a Packet Data Field whose Secondary Header Flag is `flag`; a time code is decoded
    /// under `mode` (SPP-007, SPP-008).
    ///
    /// # Errors
    /// - `InvalidField` (`secondary_header_flag`) if the flag disagrees with the layout: set
    ///   with [`Absent`](Self::Absent), or clear with a secondary header. 133.0 §4.1.3.3.3.3
    ///   fixes the flag per APID, so this holds in both modes.
    /// - `LengthMismatch` (`secondary_header`) if `data` is shorter than the secondary header,
    ///   including a time code cut off inside its P-field or T-field.
    /// - With a time layout: `InvalidField` (`time_code_id`) for a P-field with another time
    ///   code ID, and the other errors of the time code decoder.
    #[cfg_attr(
        not(feature = "time"),
        expect(unused_variables, reason = "`mode` is used only to decode a time code")
    )]
    pub(crate) fn split(
        self,
        flag: bool,
        data: &[u8],
        mode: DecodeMode,
    ) -> Result<SplitPacket<'_>> {
        let present = !matches!(self, SecondaryHeaderLayout::Absent);
        if flag != present {
            return Err(invalid("secondary_header_flag"));
        }
        let (len, time_len) = match self {
            SecondaryHeaderLayout::Absent => (0, 0),
            SecondaryHeaderLayout::Opaque(len) => (len.get(), 0),
            #[cfg(feature = "time")]
            SecondaryHeaderLayout::Time(time) => {
                let decoded = time.decode(data, mode)?;
                let time_len = decoded.len();
                let (secondary_header, user_data) = data
                    .split_at_checked(time_len.saturating_add(time.ancillary_len))
                    .ok_or(too_short(
                        time_len.saturating_add(time.ancillary_len),
                        data.len(),
                    ))?;
                return Ok(SplitPacket {
                    secondary_header,
                    user_data,
                    time_len,
                    time: Some(decoded),
                });
            }
        };
        let (secondary_header, user_data) = data
            .split_at_checked(len)
            .ok_or(too_short(len, data.len()))?;
        Ok(SplitPacket {
            secondary_header,
            user_data,
            time_len,
            #[cfg(feature = "time")]
            time: None,
        })
    }
}

/// A Packet Data Field split by its layout, borrowing from the input (SPP-007, SPP-008,
/// GEN-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitPacket<'a> {
    secondary_header: &'a [u8],
    user_data: &'a [u8],
    time_len: usize,
    #[cfg(feature = "time")]
    time: Option<Decoded>,
}

impl<'a> SplitPacket<'a> {
    /// The Packet Secondary Header; empty with [`SecondaryHeaderLayout::Absent`] (133.0
    /// §4.1.4.2, SPP-007).
    #[must_use]
    pub const fn secondary_header(&self) -> &'a [u8] {
        self.secondary_header
    }

    /// The User Data Field; may be empty when a secondary header is present (133.0 §4.1.4.3,
    /// SPP-007).
    #[must_use]
    pub const fn user_data(&self) -> &'a [u8] {
        self.user_data
    }

    /// The Ancillary Data Field: the secondary header after the Time Code Field, or the whole
    /// secondary header with an opaque layout (133.0 §4.1.4.2.1.5, §4.1.4.2.3).
    #[must_use]
    pub fn ancillary_data(&self) -> &'a [u8] {
        self.secondary_header
            .get(self.time_len..)
            .unwrap_or_default()
    }

    /// The decoded Time Code Field, with a time layout (SPP-008); its length includes the
    /// P-field when present.
    #[cfg(feature = "time")]
    #[must_use]
    pub const fn time_code(&self) -> Option<Decoded> {
        self.time
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::SecondaryHeaderLayout;
    use aldis_core::{DecodeMode, ErrorDetail, ErrorKind, FieldId};
    use core::num::NonZeroUsize;

    const STRICT: DecodeMode = DecodeMode::Strict;

    #[test]
    fn split_rules() {
        // req: SPP-007, OI-30
        let two = SecondaryHeaderLayout::Opaque(NonZeroUsize::new(2).unwrap());
        let s = two.split(true, &[1, 2, 3], STRICT).unwrap();
        assert_eq!(
            (s.secondary_header(), s.user_data()),
            (&[1, 2][..], &[3][..])
        );
        assert_eq!(s.ancillary_data(), &[1, 2]);
        assert_eq!(
            two.split(true, &[1, 2], STRICT).unwrap().user_data(),
            <&[u8]>::default()
        );
        let s = SecondaryHeaderLayout::Absent
            .split(false, &[9], STRICT)
            .unwrap();
        assert_eq!(
            (s.secondary_header(), s.user_data()),
            (<&[u8]>::default(), &[9][..])
        );
        assert_eq!(s.ancillary_data(), <&[u8]>::default());
        for (layout, flag) in [(SecondaryHeaderLayout::Absent, true), (two, false)] {
            let e = layout.split(flag, &[1, 2, 3], STRICT).unwrap_err();
            assert_eq!(
                (e.kind(), e.field().map(FieldId::name)),
                (ErrorKind::InvalidField, Some("secondary_header_flag"))
            );
        }
        let e = two.split(true, &[1], STRICT).unwrap_err();
        assert_eq!(
            (e.kind(), e.field().map(FieldId::name)),
            (ErrorKind::LengthMismatch, Some("secondary_header"))
        );
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 2,
                found: 1
            }
        );
    }

    #[cfg(feature = "time")]
    mod time {
        use super::{STRICT, SecondaryHeaderLayout};
        use crate::secondary::TimeHeaderLayout;
        use aldis_core::{DecodeMode, ErrorDetail, ErrorKind, FieldId};
        use aldis_time::{
            CcsFormat, CcsVariation, CdsDayLen, CdsEpoch, CdsFormat, CdsSubMs, CucEpoch, CucFormat,
            TimeAnomaly, TimeCode, TimeFormat,
        };

        fn cuc(coarse: u8, fine: u8) -> TimeFormat {
            TimeFormat::Cuc(CucFormat::new(CucEpoch::Tai1958, coarse, fine).unwrap())
        }

        fn layout(format: TimeFormat, pfield: bool, anc: usize) -> SecondaryHeaderLayout {
            SecondaryHeaderLayout::Time(TimeHeaderLayout::new(format, pfield, anc).unwrap())
        }

        #[test]
        fn layouts_hold_only_cuc_or_cds() {
            // req: SPP-008, OI-30
            let l = TimeHeaderLayout::new(cuc(4, 2), true, 3).unwrap();
            assert_eq!(
                (l.format(), l.has_pfield(), l.ancillary_len()),
                (cuc(4, 2), true, 3)
            );
            let cds = CdsFormat::new(CdsEpoch::Ccsds1958, CdsDayLen::Bits16, CdsSubMs::None);
            assert!(
                !TimeHeaderLayout::new(TimeFormat::Cds(cds), false, 0)
                    .unwrap()
                    .has_pfield()
            );
            let ccs = TimeFormat::Ccs(CcsFormat::new(CcsVariation::MonthDay, 0).unwrap());
            let agency = TimeFormat::Agency(aldis_time::AgencyFormat::new(2).unwrap());
            for bad in [ccs, agency] {
                let e = TimeHeaderLayout::new(bad, false, 0).unwrap_err();
                assert_eq!(
                    (e.kind(), e.field().map(FieldId::name)),
                    (ErrorKind::InvalidConfig, Some("time_code"))
                );
            }
        }

        #[test]
        fn time_split() {
            // req: SPP-008, SPP-007, OI-30
            let data = [0x12, 0x34, 0x56, 0x78, 0x80, 0x00, 0xAB, 0x01];
            let s = layout(cuc(4, 2), false, 1)
                .split(true, &data, STRICT)
                .unwrap();
            assert_eq!(
                (
                    s.secondary_header().len(),
                    s.user_data(),
                    s.ancillary_data()
                ),
                (7, &[0x01][..], &[0xAB][..])
            );
            let t = s.time_code().unwrap();
            assert!(
                matches!(t.code(), TimeCode::Cuc(c) if c.value().coarse() == 0x1234_5678)
                    && t.pfield().is_none()
            );
            // With the P-field: CUC 4+2 is octet 0x1E.
            let with_p = [0x1E, 0x12, 0x34, 0x56, 0x78, 0x80, 0x00];
            let s = layout(cuc(4, 2), true, 0)
                .split(true, &with_p, STRICT)
                .unwrap();
            assert_eq!(
                (s.time_code().unwrap().len(), s.user_data()),
                (7, <&[u8]>::default())
            );
            // A CDS P-field (0x40) where CUC is configured.
            let e = layout(cuc(4, 2), true, 0)
                .split(true, &[0x40, 0, 0, 0, 0, 0, 0], STRICT)
                .unwrap_err();
            assert_eq!(
                (e.kind(), e.field().map(FieldId::name)),
                (ErrorKind::InvalidField, Some("time_code_id"))
            );
            assert_eq!(
                e.detail(),
                ErrorDetail::Expected {
                    expected: 1,
                    found: 4
                }
            );
            // Cut off in the T-field, in the P-field extension, and in the ancillary field;
            // `expected` includes the ancillary octets wherever the cut falls.
            for (lay, d, expected) in [
                (layout(cuc(4, 2), false, 0), &data[..3], 6),
                (layout(cuc(4, 2), false, 4), &data[..3], 10),
                (layout(cuc(5, 3), true, 0), &[0x9E][..], 2),
                (layout(cuc(5, 3), true, 2), &[0x9E][..], 4),
                (layout(cuc(4, 2), false, 4), &data[..7], 10),
            ] {
                let e = lay.split(true, d, STRICT).unwrap_err();
                assert_eq!(
                    (e.kind(), e.field().map(FieldId::name)),
                    (ErrorKind::LengthMismatch, Some("secondary_header"))
                );
                assert_eq!(
                    e.detail(),
                    ErrorDetail::Expected {
                        expected,
                        found: aldis_core_len(d)
                    }
                );
            }
            // A P-field longer than 8 octets is the time decoder's `Malformed`, unchanged.
            let e = layout(cuc(4, 2), true, 0)
                .split(
                    true,
                    &[0x9E, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80],
                    STRICT,
                )
                .unwrap_err();
            assert_eq!(e.kind(), ErrorKind::Malformed);
            assert_eq!(
                layout(cuc(4, 2), false, 0)
                    .split(false, &data, STRICT)
                    .unwrap_err()
                    .kind(),
                ErrorKind::InvalidField
            );
        }

        #[test]
        fn time_split_follows_the_decode_mode() {
            // req: SPP-008, API-031, TIME-005
            // CUC 4+2 P-field with an extension octet that adds nothing, then an undefined
            // third octet: strict rejects it, lenient flags it.
            let data = [0x9E, 0x80, 0x00, 0, 0, 0, 0, 0, 0];
            let lay = layout(cuc(4, 2), true, 0);
            assert_eq!(
                lay.split(true, &data, STRICT).unwrap_err().kind(),
                ErrorKind::ReservedValue
            );
            let s = lay.split(true, &data, DecodeMode::Lenient).unwrap();
            assert!(
                s.time_code()
                    .unwrap()
                    .anomalies()
                    .contains(TimeAnomaly::UndefinedPFieldExtension)
            );
        }

        fn aldis_core_len(d: &[u8]) -> u64 {
            u64::try_from(d.len()).unwrap()
        }
    }
}
