//! CCSDS Calendar Segmented Code, CCS (301.0 §3.4, Annex A; TIME-030, TIME-034).
//!
//! Every segment is binary-coded decimal, two digits per octet. CCS is UTC-based; second 60 is
//! accepted for a leap second, and leap-second handling is the caller's responsibility.

use aldis_core::{AnomalySet, DecodeMode, Error, ErrorKind, FieldId, Result};

use crate::anomaly::TimeAnomaly;
use crate::pfield::{PField, PFieldTail, TimeCodeId};
use crate::util::{days_in_month, days_in_year, wide8};

/// 10ⁿ for n = 0, 2, 4, …, 12: one past the largest sub-second value for 0–6 octets.
const SUBSECOND_LIMIT: [u64; 7] = [
    1,
    100,
    10_000,
    1_000_000,
    100_000_000,
    10_000_000_000,
    1_000_000_000_000,
];

/// Calendar variation (301.0 §3.4.2 bit 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CcsVariation {
    /// Month of year and day of month (§3.4.1.1).
    MonthDay,
    /// Day of year (§3.4.1.2).
    DayOfYear,
}

/// CCS layout: calendar variation and number of sub-second octets (TIME-030).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CcsFormat {
    variation: CcsVariation,
    subsecond: u8,
    tail: PFieldTail,
}

impl CcsFormat {
    /// Most sub-second octets: 6, down to 10⁻¹² s (301.0 §3.4.2, TIME-030).
    pub const MAX_SUBSECOND_OCTETS: u8 = 6;

    /// A format with `subsecond_octets` (0–6) octets of 10⁻², 10⁻⁴, … s (301.0 §3.4.2, TIME-030).
    ///
    /// # Errors
    /// `OutOfRange` (`subsecond_octets`) above 6.
    pub const fn new(variation: CcsVariation, subsecond_octets: u8) -> Result<CcsFormat> {
        if subsecond_octets > CcsFormat::MAX_SUBSECOND_OCTETS {
            return Err(Error::out_of_range(
                "subsecond_octets",
                wide8(CcsFormat::MAX_SUBSECOND_OCTETS),
                wide8(subsecond_octets),
            ));
        }
        Ok(CcsFormat {
            variation,
            subsecond: subsecond_octets,
            tail: PFieldTail::EMPTY,
        })
    }

    /// The same format with undefined P-field octets after octet 1 (TIME-005).
    #[must_use]
    pub const fn with_tail(self, tail: PFieldTail) -> CcsFormat {
        CcsFormat { tail, ..self }
    }

    /// The calendar variation (301.0 §3.4.2).
    #[must_use]
    pub const fn variation(&self) -> CcsVariation {
        self.variation
    }

    /// Number of sub-second octets (301.0 §3.4.2, TIME-030).
    #[must_use]
    pub const fn subsecond_octets(&self) -> u8 {
        self.subsecond
    }

    /// Undefined P-field octets (TIME-005).
    #[must_use]
    pub const fn tail(&self) -> PFieldTail {
        self.tail
    }

    /// T-field length in octets: 7 plus the sub-second octets (301.0 §3.4.1, TIME-003).
    #[must_use]
    pub fn t_len(&self) -> usize {
        usize::from(self.subsecond).wrapping_add(7)
    }

    /// The P-field for this format (301.0 §3.4.2).
    #[must_use]
    pub fn pfield(&self) -> PField {
        let variation = match self.variation {
            CcsVariation::MonthDay => 0,
            CcsVariation::DayOfYear => 0b1000,
        };
        PField::build(
            (TimeCodeId::Ccs.bits() << 4) | variation | self.subsecond,
            &[],
            self.tail,
        )
    }

    /// The format a CCS P-field describes.
    ///
    /// # Errors
    /// `ReservedValue` (`subsecond_octets`) for resolution code `111` ("not used"), in both
    /// modes, because it sets the T-field length (TIME-030).
    pub(crate) fn from_pfield(pfield: &PField) -> Result<CcsFormat> {
        let first = pfield.first();
        let subsecond = first & 0b111;
        if subsecond == 0b111 {
            return Err(
                Error::new(ErrorKind::ReservedValue).with_field(FieldId::new("subsecond_octets"))
            );
        }
        Ok(CcsFormat {
            variation: if first & 0b1000 == 0 {
                CcsVariation::MonthDay
            } else {
                CcsVariation::DayOfYear
            },
            subsecond,
            tail: PFieldTail::from_chain(pfield.rest()),
        })
    }
}

/// The date part of a CCS value; its kind must match the format's variation (301.0 §3.4.1.1–2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CalendarDate {
    /// Month 1–12 and day of month 1–31 (301.0 §3.4.1.1, Annex A).
    MonthDay {
        /// Month of year.
        month: u8,
        /// Day of month.
        day: u8,
    },
    /// Day of year, 1–366 (301.0 §3.4.1.2, Annex A).
    DayOfYear(u16),
}

/// A CCS value with plain decimal fields (301.0 §3.4.1, TIME-030).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CcsValue {
    /// Year A.D., 1–9999 (301.0 Annex A, TIME-030).
    pub year: u16,
    /// Month and day, or day of year (301.0 §3.4.1, TIME-030).
    pub date: CalendarDate,
    /// Hour, 0–23 (301.0 Annex A, TIME-030).
    pub hour: u8,
    /// Minute, 0–59 (301.0 Annex A, TIME-030).
    pub minute: u8,
    /// Second, 0–60 (60 for a leap second) (301.0 Annex A, TIME-030).
    pub second: u8,
    /// Sub-second digits as an integer: with `n` sub-second octets, the value of the `2n` (301.0
    /// §3.4.2, TIME-030)
    /// decimal digits after the point (e.g. `1234` with 2 octets is 0.1234 s).
    pub subsecond: u64,
}

/// A CCS time code: format and a valid value (301.0 §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ccs {
    format: CcsFormat,
    value: CcsValue,
}

/// `OutOfRange` for `field` with the limit and the value found.
fn range(field: &'static str, limit: u64, found: u64) -> Error {
    Error::out_of_range(field, limit, found)
}

/// `Ok` if `value` is in `1..=max`; otherwise `OutOfRange` naming the violated bound, 1 or
/// `max` (GEN-041).
fn one_based(field: &'static str, value: u16, max: u16) -> Result<()> {
    match value {
        0 => Err(range(field, 1, 0)),
        v if v > max => Err(range(field, max.into(), v.into())),
        _ => Ok(()),
    }
}

/// Checks `value` against `format` and the calendar (TIME-030).
fn validate(format: &CcsFormat, value: &CcsValue) -> Result<()> {
    one_based("year", value.year, 9999)?;
    match (format.variation, value.date) {
        (CcsVariation::MonthDay, CalendarDate::MonthDay { month, day }) => {
            one_based("month", month.into(), 12)?;
            one_based("day", day.into(), days_in_month(value.year, month).into())?;
        }
        (CcsVariation::DayOfYear, CalendarDate::DayOfYear(doy)) => {
            one_based("day_of_year", doy, days_in_year(value.year))?;
        }
        _ => return Err(Error::new(ErrorKind::InvalidField).with_field(FieldId::new("date"))),
    }
    if value.hour > 23 {
        return Err(range("hour", 23, value.hour.into()));
    }
    if value.minute > 59 {
        return Err(range("minute", 59, value.minute.into()));
    }
    if value.second > 60 {
        return Err(range("second", 60, value.second.into()));
    }
    let limit = SUBSECOND_LIMIT
        .get(usize::from(format.subsecond))
        .copied()
        .unwrap_or(1);
    if value.subsecond >= limit {
        return Err(range("subsecond", limit.saturating_sub(1), value.subsecond));
    }
    Ok(())
}

/// Two BCD digits for `n` (0–99).
const fn to_bcd(n: u8) -> u8 {
    ((n / 10 % 10) << 4) | (n % 10)
}

/// The decimal value of one BCD octet.
///
/// # Errors
/// `InvalidField` (`field`) if either nibble is above 9 (TIME-030).
fn from_bcd(octet: u8, field: &'static str) -> Result<u8> {
    let (hi, lo) = (octet >> 4, octet & 0xF);
    if hi > 9 || lo > 9 {
        return Err(Error::new(ErrorKind::InvalidField).with_field(FieldId::new(field)));
    }
    Ok(hi.wrapping_mul(10).wrapping_add(lo))
}

impl Ccs {
    /// A CCS code.
    ///
    /// # Errors
    /// `OutOfRange` naming the field for a calendar or clock value outside its range, including
    /// a day that does not exist in the month or year, and a sub-second value with more digits
    /// than the format has; `InvalidField` (`date`) if the date kind does not match the
    /// variation (TIME-030).
    pub fn new(format: CcsFormat, value: CcsValue) -> Result<Ccs> {
        validate(&format, &value)?;
        Ok(Ccs { format, value })
    }

    /// The format (301.0 §3.4).
    #[must_use]
    pub const fn format(&self) -> CcsFormat {
        self.format
    }

    /// The value (301.0 §3.4).
    #[must_use]
    pub const fn value(&self) -> CcsValue {
        self.value
    }

    /// The T-field octets; the first [`CcsFormat::t_len`] are used. The high nibble of the
    /// day-of-year segment is written as zero (TIME-034).
    pub(crate) fn t_octets(&self) -> [u8; 13] {
        let v = &self.value;
        let century = u8::try_from(v.year / 100).unwrap_or(0);
        let year = u8::try_from(v.year % 100).unwrap_or(0);
        let (a, b) = match v.date {
            CalendarDate::MonthDay { month, day } => (to_bcd(month), to_bcd(day)),
            CalendarDate::DayOfYear(doy) => (
                u8::try_from(doy / 100).unwrap_or(0),
                to_bcd(u8::try_from(doy % 100).unwrap_or(0)),
            ),
        };
        let mut sub = [0_u8; 6];
        let mut rest = v.subsecond;
        for octet in sub
            .iter_mut()
            .take(usize::from(self.format.subsecond))
            .rev()
        {
            *octet = to_bcd(u8::try_from(rest % 100).unwrap_or(0));
            rest /= 100;
        }
        let [s0, s1, s2, s3, s4, s5] = sub;
        [
            to_bcd(century),
            to_bcd(year),
            a,
            b,
            to_bcd(v.hour),
            to_bcd(v.minute),
            to_bcd(v.second),
            s0,
            s1,
            s2,
            s3,
            s4,
            s5,
        ]
    }

    /// Decodes a T-field of exactly [`CcsFormat::t_len`] octets (TIME-030, TIME-034).
    ///
    /// # Errors
    /// `InvalidField` for a nibble above 9; `ReservedValue` (`day_of_year`) in strict mode for a
    /// non-zero day-of-year high nibble, which lenient mode reports as
    /// [`TimeAnomaly::NonZeroDoyHighNibble`]; and the range errors of [`Ccs::new`].
    pub(crate) fn from_t(
        format: CcsFormat,
        t: &[u8],
        mode: DecodeMode,
        anomalies: &mut AnomalySet<TimeAnomaly>,
    ) -> Result<Ccs> {
        let mut octets = t.iter().copied();
        let mut next = || octets.next().unwrap_or(0);
        let century = from_bcd(next(), "year")?;
        let year = from_bcd(next(), "year")?;
        let (a, b) = (next(), next());
        let date = match format.variation {
            CcsVariation::MonthDay => CalendarDate::MonthDay {
                month: from_bcd(a, "month")?,
                day: from_bcd(b, "day")?,
            },
            CcsVariation::DayOfYear => {
                if a >> 4 != 0 {
                    match mode {
                        DecodeMode::Strict => {
                            return Err(Error::new(ErrorKind::ReservedValue)
                                .with_field(FieldId::new("day_of_year")));
                        }
                        DecodeMode::Lenient => anomalies.insert(TimeAnomaly::NonZeroDoyHighNibble),
                    }
                }
                let hundreds = from_bcd(a & 0x0F, "day_of_year")?;
                let rest = from_bcd(b, "day_of_year")?;
                CalendarDate::DayOfYear(
                    u16::from(hundreds)
                        .wrapping_mul(100)
                        .wrapping_add(u16::from(rest)),
                )
            }
        };
        let hour = from_bcd(next(), "hour")?;
        let minute = from_bcd(next(), "minute")?;
        let second = from_bcd(next(), "second")?;
        let mut subsecond: u64 = 0;
        for _ in 0..format.subsecond {
            let digits = from_bcd(next(), "subsecond")?;
            subsecond = subsecond.wrapping_mul(100).wrapping_add(u64::from(digits));
        }
        Ccs::new(
            format,
            CcsValue {
                year: u16::from(century)
                    .wrapping_mul(100)
                    .wrapping_add(u16::from(year)),
                date,
                hour,
                minute,
                second,
                subsecond,
            },
        )
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "test models on bounded values (RS-018)"
)]
mod tests {
    use super::{CalendarDate, Ccs, CcsFormat, CcsValue, CcsVariation, from_bcd, to_bcd};
    use crate::anomaly::TimeAnomaly;
    use crate::pfield::{PField, PFieldTail};
    use aldis_core::{AnomalySet, DecodeMode, ErrorKind, FieldId};
    use proptest::prelude::*;

    fn md(month: u8, day: u8) -> CalendarDate {
        CalendarDate::MonthDay { month, day }
    }

    fn value(year: u16, date: CalendarDate) -> CcsValue {
        CcsValue {
            year,
            date,
            hour: 12,
            minute: 30,
            second: 45,
            subsecond: 0,
        }
    }

    #[test]
    fn formats() {
        // req: TIME-030, TIME-001
        assert_eq!(
            CcsFormat::new(CcsVariation::MonthDay, 7)
                .unwrap_err()
                .kind(),
            ErrorKind::OutOfRange
        );
        let tail = PFieldTail::new(&[0x00]).unwrap();
        for variation in [CcsVariation::MonthDay, CcsVariation::DayOfYear] {
            for n in 0..=6 {
                let f = CcsFormat::new(variation, n).unwrap();
                assert_eq!(
                    (f.variation(), f.subsecond_octets(), f.t_len()),
                    (variation, n, 7 + usize::from(n))
                );
                for f in [f, f.with_tail(tail)] {
                    assert_eq!(CcsFormat::from_pfield(&f.pfield()), Ok(f));
                }
            }
        }
        assert_eq!(
            CcsFormat::new(CcsVariation::DayOfYear, 3).unwrap().tail(),
            PFieldTail::EMPTY
        );
        let with = CcsFormat::new(CcsVariation::DayOfYear, 3)
            .unwrap()
            .with_tail(tail);
        assert_eq!(with.tail(), tail);
        let e = CcsFormat::from_pfield(&PField::parse(&[0x57]).unwrap()).unwrap_err();
        assert_eq!(
            (e.kind(), e.field().map(FieldId::name)),
            (ErrorKind::ReservedValue, Some("subsecond_octets"))
        );
    }

    #[test]
    fn calendar_checks() {
        // req: TIME-030
        let mdf = CcsFormat::new(CcsVariation::MonthDay, 1).unwrap();
        let doyf = CcsFormat::new(CcsVariation::DayOfYear, 0).unwrap();
        let ok = Ccs::new(
            mdf,
            CcsValue {
                subsecond: 99,
                ..value(2024, md(2, 29))
            },
        )
        .unwrap();
        assert_eq!((ok.format(), ok.value().year), (mdf, 2024));
        let cases = [
            (mdf, value(0, md(1, 1)), "year"),
            (mdf, value(10_000, md(1, 1)), "year"),
            (mdf, value(2023, md(0, 1)), "month"),
            (mdf, value(2023, md(13, 1)), "month"),
            (mdf, value(2023, md(2, 29)), "day"),
            (mdf, value(2023, md(1, 0)), "day"),
            (
                doyf,
                value(2023, CalendarDate::DayOfYear(366)),
                "day_of_year",
            ),
            (doyf, value(2023, CalendarDate::DayOfYear(0)), "day_of_year"),
            (
                mdf,
                CcsValue {
                    hour: 24,
                    ..value(2023, md(1, 1))
                },
                "hour",
            ),
            (
                mdf,
                CcsValue {
                    minute: 60,
                    ..value(2023, md(1, 1))
                },
                "minute",
            ),
            (
                mdf,
                CcsValue {
                    second: 61,
                    ..value(2023, md(1, 1))
                },
                "second",
            ),
            (
                mdf,
                CcsValue {
                    subsecond: 100,
                    ..value(2023, md(1, 1))
                },
                "subsecond",
            ),
        ];
        for (f, v, field) in cases {
            let e = Ccs::new(f, v).unwrap_err();
            assert_eq!(
                (e.kind(), e.field().map(FieldId::name)),
                (ErrorKind::OutOfRange, Some(field)),
                "{v:?}"
            );
        }
        for (f, v) in [
            (mdf, value(2023, CalendarDate::DayOfYear(1))),
            (doyf, value(2023, md(1, 1))),
        ] {
            let e = Ccs::new(f, v).unwrap_err();
            assert_eq!(
                (e.kind(), e.field().map(FieldId::name)),
                (ErrorKind::InvalidField, Some("date"))
            );
        }
        assert!(
            Ccs::new(
                doyf,
                CcsValue {
                    second: 60,
                    ..value(2024, CalendarDate::DayOfYear(366))
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn errors_report_the_violated_bound() {
        // req: TIME-030, GEN-041
        use aldis_core::ErrorDetail;
        let mdf = CcsFormat::new(CcsVariation::MonthDay, 0).unwrap();
        let doyf = CcsFormat::new(CcsVariation::DayOfYear, 0).unwrap();
        for (f, v, expected, found) in [
            (mdf, value(0, md(1, 1)), 1, 0),
            (mdf, value(10_000, md(1, 1)), 9999, 10_000),
            (mdf, value(2023, md(0, 1)), 1, 0),
            (mdf, value(2023, md(13, 1)), 12, 13),
            (mdf, value(2023, md(4, 0)), 1, 0),
            (mdf, value(2023, md(4, 31)), 30, 31),
            (doyf, value(2023, CalendarDate::DayOfYear(0)), 1, 0),
            (doyf, value(2023, CalendarDate::DayOfYear(366)), 365, 366),
        ] {
            let e = Ccs::new(f, v).unwrap_err();
            assert_eq!(
                e.detail(),
                ErrorDetail::Expected { expected, found },
                "{v:?}"
            );
        }
    }

    #[test]
    fn bcd_helpers() {
        // req: TIME-030
        assert_eq!(
            (to_bcd(0), to_bcd(9), to_bcd(42), to_bcd(99)),
            (0x00, 0x09, 0x42, 0x99)
        );
        assert_eq!(from_bcd(0x59, "x"), Ok(59));
        for bad in [0x0A, 0xA0, 0xFF] {
            assert_eq!(
                from_bcd(bad, "x").unwrap_err().kind(),
                ErrorKind::InvalidField
            );
        }
    }

    #[test]
    fn doy_high_nibble() {
        // req: TIME-034, API-032
        let f = CcsFormat::new(CcsVariation::DayOfYear, 0).unwrap();
        let t = [0x20, 0x16, 0x11, 0x00, 0x00, 0x00, 0x00];
        let mut flags = AnomalySet::empty();
        let e = Ccs::from_t(f, &t, DecodeMode::Strict, &mut flags).unwrap_err();
        assert_eq!(
            (e.kind(), flags.is_empty()),
            (ErrorKind::ReservedValue, true)
        );
        let c = Ccs::from_t(f, &t, DecodeMode::Lenient, &mut flags).unwrap();
        assert_eq!(c.value().date, CalendarDate::DayOfYear(100));
        assert!(flags.contains(TimeAnomaly::NonZeroDoyHighNibble));
        let bad = [0x20, 0x16, 0x0A, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(
            Ccs::from_t(f, &bad, DecodeMode::Lenient, &mut flags)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidField
        );
    }

    #[test]
    fn every_bcd_octet_is_checked() {
        // req: TIME-030
        for (variation, t) in [
            (
                CcsVariation::MonthDay,
                [0x20, 0x16, 0x12, 0x31, 0x23, 0x59, 0x59, 0x99],
            ),
            (
                CcsVariation::DayOfYear,
                [0x20, 0x16, 0x03, 0x65, 0x23, 0x59, 0x59, 0x99],
            ),
        ] {
            let f = CcsFormat::new(variation, 1).unwrap();
            let mut flags = AnomalySet::empty();
            assert!(Ccs::from_t(f, &t, DecodeMode::Strict, &mut flags).is_ok());
            for i in 0..t.len() {
                for bad in [0x0A, 0xA0] {
                    let mut t = t;
                    t[i] = (t[i] & if bad == 0x0A { 0xF0 } else { 0x0F }) | bad;
                    let r = Ccs::from_t(f, &t, DecodeMode::Lenient, &mut flags);
                    if variation == CcsVariation::DayOfYear && i == 2 && bad == 0xA0 {
                        // The DOY high nibble is the unused nibble: flagged, not BCD (TIME-034).
                        assert!(r.is_ok() && flags.contains(TimeAnomaly::NonZeroDoyHighNibble));
                    } else {
                        let e = r.unwrap_err();
                        assert_eq!(
                            e.kind(),
                            ErrorKind::InvalidField,
                            "{variation:?} octet {i} {bad:#04X}"
                        );
                    }
                }
            }
        }
    }

    fn valid() -> impl Strategy<Value = (CcsFormat, CcsValue)> {
        (
            any::<bool>(),
            0_u8..=6,
            1_u16..=9999,
            1_u8..=12,
            1_u16..=366,
            0_u8..24,
            0_u8..60,
            0_u8..=60,
            any::<u64>(),
        )
            .prop_map(|(doy, n, year, month, day, hour, minute, second, sub)| {
                let variation = if doy {
                    CcsVariation::DayOfYear
                } else {
                    CcsVariation::MonthDay
                };
                let format = CcsFormat::new(variation, n).unwrap();
                let date = if doy {
                    CalendarDate::DayOfYear(day.min(crate::util::days_in_year(year)))
                } else {
                    let last = crate::util::days_in_month(year, month);
                    CalendarDate::MonthDay {
                        month,
                        day: u8::try_from(day).unwrap_or(1).clamp(1, last),
                    }
                };
                let limit = 10_u64.pow(2 * u32::from(n));
                (
                    format,
                    CcsValue {
                        year,
                        date,
                        hour,
                        minute,
                        second,
                        subsecond: sub % limit,
                    },
                )
            })
    }

    proptest! {
        #[test]
        fn t_field_round_trip((format, value) in valid()) {
            // req: TIME-030, API-020, VER-023
            let c = Ccs::new(format, value).unwrap();
            let t = c.t_octets();
            let mut flags = AnomalySet::empty();
            prop_assert_eq!(Ccs::from_t(format, &t[..format.t_len()], DecodeMode::Strict, &mut flags), Ok(c));
            prop_assert!(flags.is_empty());
            prop_assert_eq!(t[2] >> 4 == 0 || format.variation() == CcsVariation::MonthDay, true);
        }
    }
}
