//! CCSDS Calendar Segmented Code, CCS (301.0 §3.4, Annex A; TIME-030, TIME-034).
//!
//! Every segment is binary-coded decimal, two digits per octet. CCS is UTC-based; second 60 is
//! accepted for a leap second, and leap-second handling is the caller's responsibility.

use aldis_core::{AnomalySet, DecodeMode, Error, ErrorKind, FieldId, Result};

use crate::anomaly::TimeAnomaly;
use crate::calendar::{CalendarDate, Hour, Minute, Month, Second, Year};
use crate::pfield::{PField, PFieldTail, TimeCodeId};
use crate::util::wide8;

/// One past the largest sub-second value for `octets` sub-second octets: 10^(2 × octets), for
/// 0–6 octets (301.0 §3.4.2).
const fn subsecond_limit(octets: u8) -> u64 {
    match octets {
        0 => 1,
        1 => 100,
        2 => 10_000,
        3 => 1_000_000,
        4 => 100_000_000,
        5 => 10_000_000_000,
        _ => 1_000_000_000_000,
    }
}

/// Calendar variation (301.0 §3.4.2 bit 4, TIME-030).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CcsVariation {
    /// Month of year and day of month (§3.4.1.1).
    MonthDay,
    /// Day of year (§3.4.1.2).
    DayOfYear,
}

/// CCS layout: calendar variation and number of sub-second octets (301.0 §3.4.2, TIME-030).
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

    /// The same format with undefined P-field octets after octet 1 (301.0 §3.4.2, TIME-005).
    #[must_use]
    pub const fn with_tail(self, tail: PFieldTail) -> CcsFormat {
        CcsFormat { tail, ..self }
    }

    /// The calendar variation (301.0 §3.4.2, TIME-030).
    #[must_use]
    pub const fn variation(&self) -> CcsVariation {
        self.variation
    }

    /// Number of sub-second octets (301.0 §3.4.2, TIME-030).
    #[must_use]
    pub const fn subsecond_octets(&self) -> u8 {
        self.subsecond
    }

    /// Undefined P-field octets (301.0 §3.4.2, TIME-005).
    #[must_use]
    pub const fn tail(&self) -> PFieldTail {
        self.tail
    }

    /// T-field length in octets: 7 plus the sub-second octets (301.0 §3.4.1, TIME-003).
    #[must_use]
    pub fn t_len(&self) -> usize {
        usize::from(self.subsecond).wrapping_add(7)
    }

    /// The P-field for this format (301.0 §3.4.2, TIME-030).
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

/// A CCS value (301.0 §3.4.1, TIME-030).
///
/// Every field holds only values 301.0 allows; whether the value fits a particular
/// [`CcsFormat`] is checked by [`Ccs::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CcsValue {
    /// The date, as month and day or as day of year (301.0 §3.4.1, TIME-030).
    pub date: CalendarDate,
    /// Hour (301.0 Annex A, TIME-030).
    pub hour: Hour,
    /// Minute (301.0 Annex A, TIME-030).
    pub minute: Minute,
    /// Second, 60 for a leap second (301.0 Annex A, TIME-030).
    pub second: Second,
    /// The sub-second digits as one integer (301.0 §3.4.2, TIME-030): with `n` sub-second
    /// octets, the `2n` decimal digits after the point, e.g. `1234` with 2 octets is 0.1234 s.
    pub subsecond: u64,
}

/// A CCS time code: a format and a value that fits it (301.0 §3.4, TIME-030, TIME-034).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ccs {
    format: CcsFormat,
    value: CcsValue,
}

/// Two BCD digits for `n` (0–99).
const fn to_bcd(n: u8) -> u8 {
    ((n / 10 % 10) << 4) | (n % 10)
}

/// The decimal value of one BCD octet.
///
/// # Errors
/// `InvalidField` (`field`) if either nibble is above 9 (TIME-030).
const fn from_bcd(octet: u8, field: &'static str) -> Result<u8> {
    let (hi, lo) = (octet >> 4, octet & 0xF);
    if hi > 9 || lo > 9 {
        return Err(Error::new(ErrorKind::InvalidField).with_field(FieldId::new(field)));
    }
    Ok(hi.wrapping_mul(10).wrapping_add(lo))
}

impl Ccs {
    /// A CCS code from a format and a value that fits it. The fields of `value` are valid by
    /// construction; only the fit to `format` is checked here (301.0 §3.4.1, TIME-030).
    ///
    /// # Errors
    /// `InvalidField` (`date`) if the date is not expressed in the format's calendar variation;
    /// `OutOfRange` (`subsecond`) if the sub-second value has more digits than the format's
    /// sub-second octets hold.
    pub const fn new(format: CcsFormat, value: CcsValue) -> Result<Ccs> {
        if !matches!(
            (value.date.variation(), format.variation),
            (CcsVariation::MonthDay, CcsVariation::MonthDay)
                | (CcsVariation::DayOfYear, CcsVariation::DayOfYear)
        ) {
            return Err(Error::new(ErrorKind::InvalidField).with_field(FieldId::new("date")));
        }
        let limit = subsecond_limit(format.subsecond);
        if value.subsecond >= limit {
            return Err(Error::out_of_range(
                "subsecond",
                limit.saturating_sub(1),
                value.subsecond,
            ));
        }
        Ok(Ccs { format, value })
    }

    /// A code whose value the caller already knows fits the format: the date is in the
    /// format's variation and the sub-second value has at most its digits (conversions).
    pub(crate) const fn from_parts_unchecked(format: CcsFormat, value: CcsValue) -> Ccs {
        Ccs { format, value }
    }

    /// The format (301.0 §3.4, TIME-030, TIME-034).
    #[must_use]
    pub const fn format(&self) -> CcsFormat {
        self.format
    }

    /// The value (301.0 §3.4, TIME-030, TIME-034).
    #[must_use]
    pub const fn value(&self) -> CcsValue {
        self.value
    }

    /// The T-field octets; the first [`CcsFormat::t_len`] are used. The high nibble of the
    /// day-of-year segment is written as zero (TIME-034).
    pub(crate) fn t_octets(&self) -> [u8; 13] {
        let v = &self.value;
        let year = v.date.year().get();
        let century = u8::try_from(year / 100).unwrap_or(0);
        let year = u8::try_from(year % 100).unwrap_or(0);
        let (a, b) = match (v.date.month_day(), v.date.day_of_year()) {
            (Some((month, day)), _) => (to_bcd(month.get()), to_bcd(day)),
            (None, doy) => {
                let doy = doy.unwrap_or(0);
                (
                    u8::try_from(doy / 100).unwrap_or(0),
                    to_bcd(u8::try_from(doy % 100).unwrap_or(0)),
                )
            }
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
            to_bcd(v.hour.get()),
            to_bcd(v.minute.get()),
            to_bcd(v.second.get()),
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
    /// [`TimeAnomaly::NonZeroDoyHighNibble`]; and `OutOfRange` from the field types for a value
    /// outside its range or a day that does not exist.
    pub(crate) fn from_t(
        format: CcsFormat,
        t: &[u8],
        mode: DecodeMode,
        anomalies: &mut AnomalySet<TimeAnomaly>,
    ) -> Result<Ccs> {
        let mut octets = t.iter().copied();
        let mut next = || octets.next().unwrap_or(0);
        // Each field is parsed into its type as it is read (TIME-030).
        let century = from_bcd(next(), "year")?;
        let year = Year::try_from(
            u16::from(century)
                .wrapping_mul(100)
                .wrapping_add(u16::from(from_bcd(next(), "year")?)),
        )?;
        let (a, b) = (next(), next());
        let date = match format.variation {
            CcsVariation::MonthDay => {
                let month = Month::try_from(from_bcd(a, "month")?)?;
                CalendarDate::from_month_day(year, month, from_bcd(b, "day")?)?
            }
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
                CalendarDate::from_day_of_year(
                    year,
                    u16::from(hundreds)
                        .wrapping_mul(100)
                        .wrapping_add(u16::from(rest)),
                )?
            }
        };
        let hour = Hour::try_from(from_bcd(next(), "hour")?)?;
        let minute = Minute::try_from(from_bcd(next(), "minute")?)?;
        let second = Second::try_from(from_bcd(next(), "second")?)?;
        let mut subsecond: u64 = 0;
        for _ in 0..format.subsecond {
            let digits = from_bcd(next(), "subsecond")?;
            subsecond = subsecond.wrapping_mul(100).wrapping_add(u64::from(digits));
        }
        // The value has at most 2n digits for n octets, so it always fits the format.
        Ok(Ccs {
            format,
            value: CcsValue {
                date,
                hour,
                minute,
                second,
                subsecond,
            },
        })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "test models on bounded values (RS-018)"
)]
mod tests {
    use super::{Ccs, CcsFormat, CcsValue, CcsVariation, from_bcd, subsecond_limit, to_bcd};
    use crate::anomaly::TimeAnomaly;
    use crate::calendar::{CalendarDate, Hour, Minute, Month, Second, Year};
    use crate::pfield::{PField, PFieldTail};
    use aldis_core::{AnomalySet, DecodeMode, ErrorDetail, ErrorKind, FieldId};
    use proptest::prelude::*;

    fn md(year: u16, month: u8, day: u8) -> CalendarDate {
        CalendarDate::from_month_day(Year::new(year).unwrap(), Month::new(month).unwrap(), day)
            .unwrap()
    }

    fn doy(year: u16, day: u16) -> CalendarDate {
        CalendarDate::from_day_of_year(Year::new(year).unwrap(), day).unwrap()
    }

    fn value(date: CalendarDate, subsecond: u64) -> CcsValue {
        CcsValue {
            date,
            hour: Hour::new(12).unwrap(),
            minute: Minute::new(30).unwrap(),
            second: Second::new(45).unwrap(),
            subsecond,
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
        let f = CcsFormat::new(CcsVariation::DayOfYear, 3).unwrap();
        assert_eq!(
            (f.tail(), f.with_tail(tail).tail()),
            (PFieldTail::EMPTY, tail)
        );
        let e = CcsFormat::from_pfield(&PField::parse(&[0x57]).unwrap()).unwrap_err();
        assert_eq!(
            (e.kind(), e.field().map(FieldId::name)),
            (ErrorKind::ReservedValue, Some("subsecond_octets"))
        );
    }

    #[test]
    fn value_must_fit_the_format() {
        // req: TIME-030, GEN-041
        let mdf = CcsFormat::new(CcsVariation::MonthDay, 1).unwrap();
        let doyf = CcsFormat::new(CcsVariation::DayOfYear, 0).unwrap();
        let ok = Ccs::new(mdf, value(md(2024, 2, 29), 99)).unwrap();
        assert_eq!((ok.format(), ok.value().subsecond), (mdf, 99));
        for (f, v) in [
            (mdf, value(doy(2023, 1), 0)),
            (doyf, value(md(2023, 1, 1), 0)),
        ] {
            let e = Ccs::new(f, v).unwrap_err();
            assert_eq!(
                (e.kind(), e.field().map(FieldId::name)),
                (ErrorKind::InvalidField, Some("date"))
            );
        }
        let e = Ccs::new(mdf, value(md(2023, 1, 1), 100)).unwrap_err();
        assert_eq!(e.field().map(FieldId::name), Some("subsecond"));
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 99,
                found: 100
            }
        );
        let e = Ccs::new(doyf, value(doy(2023, 1), 1)).unwrap_err();
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 0,
                found: 1
            }
        );
        assert!(Ccs::new(doyf, value(doy(2024, 366), 0)).is_ok());
    }

    #[test]
    fn decoding_parses_every_field() {
        // req: TIME-030, GEN-041
        let f = CcsFormat::new(CcsVariation::MonthDay, 0).unwrap();
        let ok = [0x20, 0x24, 0x02, 0x29, 0x23, 0x59, 0x60];
        let mut flags = AnomalySet::empty();
        let c = Ccs::from_t(f, &ok, DecodeMode::Strict, &mut flags).unwrap();
        assert_eq!(c.value().date, md(2024, 2, 29));
        assert_eq!(
            (
                c.value().hour.get(),
                c.value().minute.get(),
                c.value().second.get()
            ),
            (23, 59, 60)
        );
        for (t, field, expected, found) in [
            ([0x00, 0x00, 0x01, 0x01, 0, 0, 0], "year", 1, 0),
            ([0x20, 0x23, 0x13, 0x01, 0, 0, 0], "month", 12, 13),
            ([0x20, 0x23, 0x00, 0x01, 0, 0, 0], "month", 1, 0),
            ([0x20, 0x23, 0x02, 0x29, 0, 0, 0], "day", 28, 29),
            ([0x20, 0x23, 0x01, 0x01, 0x24, 0, 0], "hour", 23, 24),
            ([0x20, 0x23, 0x01, 0x01, 0, 0x60, 0], "minute", 59, 60),
            ([0x20, 0x23, 0x01, 0x01, 0, 0, 0x61], "second", 60, 61),
        ] {
            let e = Ccs::from_t(f, &t, DecodeMode::Lenient, &mut flags).unwrap_err();
            assert_eq!(
                (e.kind(), e.field().map(FieldId::name)),
                (ErrorKind::OutOfRange, Some(field))
            );
            assert_eq!(
                e.detail(),
                ErrorDetail::Expected { expected, found },
                "{t:02X?}"
            );
        }
        let doyf = CcsFormat::new(CcsVariation::DayOfYear, 0).unwrap();
        let e = Ccs::from_t(
            doyf,
            &[0x20, 0x23, 0x03, 0x66, 0, 0, 0],
            DecodeMode::Strict,
            &mut flags,
        )
        .unwrap_err();
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 365,
                found: 366
            }
        );
    }

    #[test]
    fn subsecond_limits() {
        // req: TIME-030
        for octets in 0..=6_u8 {
            assert_eq!(subsecond_limit(octets), 10_u64.pow(2 * u32::from(octets)));
        }
    }

    #[test]
    fn ccs_codes_are_const_constructible() {
        // req: TIME-030, RS-012
        const DATE: Result<CalendarDate, aldis_core::Error> = match (Year::new(2024), Month::new(2))
        {
            (Some(year), Some(month)) => CalendarDate::from_month_day(year, month, 29),
            _ => panic!("valid fields"),
        };
        const CODE: Result<Ccs, aldis_core::Error> = match (
            CcsFormat::new(CcsVariation::MonthDay, 0),
            DATE,
            Hour::new(0),
            Minute::new(0),
            Second::new(0),
        ) {
            (Ok(format), Ok(date), Some(hour), Some(minute), Some(second)) => Ccs::new(
                format,
                CcsValue {
                    date,
                    hour,
                    minute,
                    second,
                    subsecond: 0,
                },
            ),
            _ => panic!("valid parts"),
        };
        const ORDINAL: Result<CalendarDate, aldis_core::Error> = match Year::new(2024) {
            Some(year) => CalendarDate::from_day_of_year(year, 366),
            None => panic!("valid year"),
        };
        assert_eq!(CODE.map(|c| c.value().date), Ok(md(2024, 2, 29)));
        assert_eq!(ORDINAL, Ok(doy(2024, 366)));
        assert_ne!(md(2024, 1, 1), doy(2024, 1), "equality keeps the variation");
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
        assert_eq!(c.value().date, doy(2016, 100));
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
            .prop_map(
                |(ordinal, n, year, month, day, hour, minute, second, sub)| {
                    let variation = if ordinal {
                        CcsVariation::DayOfYear
                    } else {
                        CcsVariation::MonthDay
                    };
                    let y = Year::new(year).unwrap();
                    let date = if ordinal {
                        CalendarDate::from_day_of_year(
                            y,
                            day.min(crate::calendar::days_in_year(year)),
                        )
                    } else {
                        let last = crate::calendar::days_in_month(year, month);
                        let day = u8::try_from(day).unwrap_or(1).clamp(1, last);
                        CalendarDate::from_month_day(y, Month::new(month).unwrap(), day)
                    }
                    .unwrap();
                    let limit = 10_u64.pow(2 * u32::from(n));
                    let value = CcsValue {
                        date,
                        hour: Hour::new(hour).unwrap(),
                        minute: Minute::new(minute).unwrap(),
                        second: Second::new(second).unwrap(),
                        subsecond: sub % limit,
                    };
                    (CcsFormat::new(variation, n).unwrap(), value)
                },
            )
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
