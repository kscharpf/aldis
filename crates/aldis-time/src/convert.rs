//! Conversions between CDS, CCS, and ASCII representations of one UTC instant (301.0 §3.3–3.5,
//! Annex A; TIME-033; `docs/standards/time-codes.md` §5.2, M1 D3, OI-18, OI-29).
//!
//! Every conversion goes through [`UtcInstant`], which keeps picoseconds, the finest resolution
//! of the three codes, so only the target can lose precision; it truncates toward zero. There is
//! no time-scale change: leap seconds pass through as second 60 (TIME-022).

use aldis_core::{Error, ErrorKind, Result};

use crate::ascii::{AsciiFormat, AsciiTime, Fraction};
use crate::calendar::{CalendarDate, Hour, Minute, Month, Second, Year, civil, day_number};
use crate::ccs::{Ccs, CcsFormat, CcsValue, CcsVariation};
use crate::cds::{Cds, CdsEpoch, CdsFormat, CdsSubMs, CdsValue, Micros, MsOfDay, Picos, SubMillis};
use crate::field;

/// Picoseconds per millisecond.
const PICOS_PER_MS: u64 = 1_000_000_000;

/// Day number of 1958 January 1, the CDS Level 1 epoch (301.0 §3.3.1).
const CDS_EPOCH: u32 = day_number(1958, 1, 1);

/// Milliseconds of day at which a positive leap second starts (301.0 Annex A).
const LEAP_MS: u32 = 86_400_000;

/// 10^(12 − 2n): picoseconds per unit of a CCS sub-second value with `n` octets.
const fn ccs_scale(octets: u8) -> u64 {
    match octets {
        0 => 1_000_000_000_000,
        1 => 10_000_000_000,
        2 => 100_000_000,
        3 => 1_000_000,
        4 => 10_000,
        5 => 100,
        _ => 1,
    }
}

/// The day number of a CDS code's epoch.
///
/// # Errors
/// `InvalidConfig` (`agency_epoch`) for an agency-defined epoch without the caller's epoch date
/// (M1 D3).
const fn epoch_day(epoch: CdsEpoch, agency_epoch: Option<CalendarDate>) -> Result<u32> {
    match (epoch, agency_epoch) {
        (CdsEpoch::Ccsds1958, _) => Ok(CDS_EPOCH),
        (CdsEpoch::Agency, Some(date)) => Ok(date.day_number()),
        (CdsEpoch::Agency, None) => {
            Err(Error::new(ErrorKind::InvalidConfig).with_field(field::AGENCY_EPOCH))
        }
    }
}

/// One UTC instant: a day, a time of day that may be a leap second, and picoseconds (301.0
/// §3.3–3.5, TIME-033).
///
/// Built only from a CDS, CCS, or ASCII code, so every field is valid (RS-020). Instants order
/// chronologically; a leap second 23:59:60 falls between 23:59:59 and the next day's 00:00:00.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcInstant {
    day: u32,
    hour: Hour,
    minute: Minute,
    second: Second,
    picos: u64,
}

impl UtcInstant {
    /// The instant of a CDS code. Milliseconds of day from 86 400 000 are the leap second
    /// 23:59:60 (301.0 Annex A, TIME-033).
    ///
    /// # Errors
    /// `InvalidConfig` (`agency_epoch`) for an agency-defined epoch without `agency_epoch`
    /// (M1 D3).
    pub fn from_cds(cds: &Cds, agency_epoch: Option<CalendarDate>) -> Result<UtcInstant> {
        let base = epoch_day(cds.format().epoch(), agency_epoch)?;
        let value = cds.value();
        let ms = value.ms_of_day.get();
        let (hour, minute, second, ms_rest) = if ms >= LEAP_MS {
            (23, 59, 60, ms.wrapping_sub(LEAP_MS))
        } else {
            let secs = ms / 1000;
            (secs / 3600, secs / 60 % 60, secs % 60, ms % 1000)
        };
        let sub = match value.subms {
            SubMillis::None => 0,
            SubMillis::Micros(us) => u64::from(us.get()).wrapping_mul(1_000_000),
            SubMillis::Picos(ps) => u64::from(ps.get()),
        };
        let narrow = |n: u32| u8::try_from(n).unwrap_or(u8::MAX);
        // Valid by construction: hour < 24, minute < 60, second ≤ 60.
        Ok(UtcInstant {
            day: base.wrapping_add(value.day),
            hour: Hour::new(narrow(hour)).unwrap_or_default(),
            minute: Minute::new(narrow(minute)).unwrap_or_default(),
            second: Second::new(narrow(second)).unwrap_or_default(),
            picos: u64::from(ms_rest)
                .wrapping_mul(PICOS_PER_MS)
                .wrapping_add(sub),
        })
    }

    /// The instant of a CCS code (301.0 §3.4, TIME-033).
    #[must_use]
    pub const fn from_ccs(ccs: &Ccs) -> UtcInstant {
        let value = ccs.value();
        UtcInstant {
            day: value.date.day_number(),
            hour: value.hour,
            minute: value.minute,
            second: value.second,
            picos: value
                .subsecond
                .wrapping_mul(ccs_scale(ccs.format().subsecond_octets())),
        }
    }

    /// The instant of an ASCII code (301.0 §3.5, TIME-033).
    #[must_use]
    pub const fn from_ascii(ascii: &AsciiTime) -> UtcInstant {
        UtcInstant {
            day: ascii.date.day_number(),
            hour: ascii.hour,
            minute: ascii.minute,
            second: ascii.second,
            picos: ascii.fraction.picos(),
        }
    }

    /// The instant as a CDS code in `format`, sub-milliseconds truncated toward zero (301.0 §3.3,
    /// TIME-033).
    ///
    /// # Errors
    /// - `InvalidConfig` (`agency_epoch`) for an agency-defined epoch without `agency_epoch`.
    /// - `OutOfRange` (`cds_day`) beyond the format's day segment, with the segment's largest day
    ///   and the day found (GEN-041).
    /// - `OutOfRange` (`cds_day`) before the epoch, with no detail: the day relative to the epoch
    ///   is negative, so no unsigned expected/found pair would describe it.
    /// - `OutOfRange` (`second`) for second 60 at a minute other than 23:59, which CDS cannot
    ///   represent (OI-29).
    pub fn to_cds(&self, format: CdsFormat, agency_epoch: Option<CalendarDate>) -> Result<Cds> {
        let base = epoch_day(format.epoch(), agency_epoch)?;
        let day = self
            .day
            .checked_sub(base)
            .ok_or(Error::new(ErrorKind::OutOfRange).with_field(field::CDS_DAY))?;
        let (h, m, s) = (self.hour.get(), self.minute.get(), self.second.get());
        if s == 60 && (h, m) != (23, 59) {
            return Err(Error::out_of_range(field::SECOND, 59, 60));
        }
        let secs = u32::from(h)
            .wrapping_mul(3600)
            .wrapping_add(u32::from(m).wrapping_mul(60))
            .wrapping_add(u32::from(s));
        let whole_ms = u32::try_from(self.picos / PICOS_PER_MS).unwrap_or(0);
        let ms = secs.wrapping_mul(1000).wrapping_add(whole_ms);
        let rest = self.picos % PICOS_PER_MS;
        // Valid by construction: at most 23:59:60.999, and sub-millisecond values below 1 ms.
        let subms = match format.subms() {
            CdsSubMs::None => SubMillis::None,
            CdsSubMs::Micros => SubMillis::Micros(
                Micros::new(u16::try_from(rest / 1_000_000).unwrap_or(0)).unwrap_or_default(),
            ),
            CdsSubMs::Picos => {
                SubMillis::Picos(Picos::new(u32::try_from(rest).unwrap_or(0)).unwrap_or_default())
            }
        };
        Cds::new(
            format,
            CdsValue {
                day,
                ms_of_day: MsOfDay::new(ms).unwrap_or_default(),
                subms,
            },
        )
    }

    /// The instant as a CCS code in `format`, the sub-second value truncated toward zero
    /// (301.0 §3.4, TIME-033).
    ///
    /// # Errors
    /// `OutOfRange` (`year`) after year 9999.
    pub fn to_ccs(&self, format: CcsFormat) -> Result<Ccs> {
        let value = CcsValue {
            date: self.date(format.variation())?,
            hour: self.hour,
            minute: self.minute,
            second: self.second,
            subsecond: self
                .picos
                .checked_div(ccs_scale(format.subsecond_octets()))
                .unwrap_or(0),
        };
        // Fits: the date is in the format's variation and the sub-second value has at most the
        // format's digits.
        Ok(Ccs::from_parts_unchecked(format, value))
    }

    /// The instant as an ASCII code in `format`, the fraction truncated toward zero (301.0 §3.5,
    /// TIME-033).
    ///
    /// # Errors
    /// `OutOfRange` (`year`) after year 9999.
    pub fn to_ascii(&self, format: AsciiFormat) -> Result<AsciiTime> {
        Ok(AsciiTime {
            date: self.date(format.variation())?,
            hour: self.hour,
            minute: self.minute,
            second: self.second,
            fraction: Fraction::from_picos(self.picos, format),
            utc: format.utc(),
        })
    }

    /// The date of the instant in `variation` (301.0 §3.4.1, §3.5.1, TIME-033).
    ///
    /// # Errors
    /// `OutOfRange` (`year`) after year 9999.
    pub fn date(&self, variation: CcsVariation) -> Result<CalendarDate> {
        let (y, m, d) = civil(self.day);
        let year = Year::try_from(u16::try_from(y).unwrap_or(u16::MAX))?;
        // Valid by construction of the day-number algorithm (M1 D3).
        Ok(match variation {
            CcsVariation::MonthDay => {
                CalendarDate::from_month_day_unchecked(year, Month::new(m).unwrap_or_default(), d)
            }
            CcsVariation::DayOfYear => {
                let doy = self
                    .day
                    .wrapping_sub(day_number(year.get(), 1, 1))
                    .wrapping_add(1);
                CalendarDate::from_day_of_year_unchecked(year, u16::try_from(doy).unwrap_or(1))
            }
        })
    }

    /// Hour (301.0 Annex A, TIME-033).
    #[must_use]
    pub const fn hour(&self) -> Hour {
        self.hour
    }

    /// Minute (301.0 Annex A, TIME-033).
    #[must_use]
    pub const fn minute(&self) -> Minute {
        self.minute
    }

    /// Second, 60 for a leap second (301.0 Annex A, TIME-033).
    #[must_use]
    pub const fn second(&self) -> Second {
        self.second
    }

    /// Picoseconds of the second, below 10¹² (301.0 Annex A, TIME-033).
    #[must_use]
    pub const fn picos(&self) -> u64 {
        self.picos
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{CDS_EPOCH, UtcInstant, ccs_scale};
    use crate::ascii::{AsciiFormat, AsciiOptions, AsciiTime, Fraction};
    use crate::calendar::{CalendarDate, Month, Year};
    use crate::ccs::{CcsFormat, CcsVariation};
    use crate::cds::{
        Cds, CdsDayLen, CdsEpoch, CdsFormat, CdsSubMs, CdsValue, Micros, MsOfDay, Picos, SubMillis,
    };
    use crate::field;
    use aldis_core::{ErrorDetail, ErrorKind};

    fn cds_fmt(day: CdsDayLen, subms: CdsSubMs, epoch: CdsEpoch) -> CdsFormat {
        CdsFormat::new(epoch, day, subms)
    }

    fn cds(day: u32, ms: u32, subms: SubMillis, format: CdsFormat) -> Cds {
        Cds::new(
            format,
            CdsValue {
                day,
                ms_of_day: MsOfDay::new(ms).unwrap(),
                subms,
            },
        )
        .unwrap()
    }

    fn ascii(text: &str) -> AsciiTime {
        AsciiTime::parse(text.as_bytes(), &AsciiOptions::DEFAULT).unwrap()
    }

    fn epoch(y: u16, m: u8, d: u8) -> CalendarDate {
        CalendarDate::from_month_day(Year::new(y).unwrap(), Month::new(m).unwrap(), d).unwrap()
    }

    #[test]
    fn cds_to_instant() {
        // req: TIME-033, TIME-022
        let fmt = cds_fmt(CdsDayLen::Bits16, CdsSubMs::Micros, CdsEpoch::Ccsds1958);
        let leap = cds(
            0,
            86_400_500,
            SubMillis::Micros(Micros::new(7).unwrap()),
            fmt,
        );
        let inst = UtcInstant::from_cds(&leap, None).unwrap();
        assert_eq!(
            (
                inst.hour().get(),
                inst.minute().get(),
                inst.second().get(),
                inst.picos()
            ),
            (23, 59, 60, 500_007_000_000)
        );
        let pico = cds_fmt(CdsDayLen::Bits16, CdsSubMs::Picos, CdsEpoch::Ccsds1958);
        let inst = UtcInstant::from_cds(
            &cds(1, 3_723_004, SubMillis::Picos(Picos::new(5).unwrap()), pico),
            None,
        )
        .unwrap();
        assert_eq!(
            (
                inst.hour().get(),
                inst.minute().get(),
                inst.second().get(),
                inst.picos()
            ),
            (1, 2, 3, 4_000_000_005)
        );
        let none = cds_fmt(CdsDayLen::Bits16, CdsSubMs::None, CdsEpoch::Ccsds1958);
        assert_eq!(
            UtcInstant::from_cds(&cds(0, 0, SubMillis::None, none), None)
                .unwrap()
                .date(CcsVariation::MonthDay),
            Ok(epoch(1958, 1, 1))
        );
        let agency = cds_fmt(CdsDayLen::Bits16, CdsSubMs::None, CdsEpoch::Agency);
        let agency_cds = cds(366, 0, SubMillis::None, agency);
        let err = UtcInstant::from_cds(&agency_cds, None).unwrap_err();
        assert_eq!(
            (err.kind(), err.field()),
            (ErrorKind::InvalidConfig, Some(field::AGENCY_EPOCH))
        );
        let inst = UtcInstant::from_cds(&agency_cds, Some(epoch(2000, 1, 1))).unwrap();
        assert_eq!(inst.date(CcsVariation::MonthDay), Ok(epoch(2001, 1, 1)));
    }

    #[test]
    fn instants_order_chronologically_and_errors_carry_detail() {
        // req: TIME-033, GEN-041
        let order = [
            "1999-12-31T23:59:59.999999999999",
            "1999-12-31T23:59:60",
            "1999-365T23:59:60.5",
            "2000-01-01T00:00:00",
            "2000-001T00:00:00.000000000001",
            "2000-01-01T00:00:01",
        ]
        .map(|text| UtcInstant::from_ascii(&ascii(text)));
        for pair in order.windows(2) {
            assert!(pair[0] < pair[1], "{pair:?}");
        }
        let fmt = cds_fmt(CdsDayLen::Bits16, CdsSubMs::None, CdsEpoch::Ccsds1958);
        let early = UtcInstant::from_ascii(&ascii("1957-12-31T23:59:59")).to_cds(fmt, None);
        assert_eq!(early.unwrap_err().detail(), ErrorDetail::None);
        let late = UtcInstant::from_ascii(&ascii("2137-06-07T00:00:00")).to_cds(fmt, None);
        assert_eq!(
            late.unwrap_err().detail(),
            ErrorDetail::Expected {
                expected: 65_535,
                found: 65_536
            }
        );
    }

    #[test]
    fn instant_to_cds() {
        // req: TIME-033, OI-29
        let fmt = cds_fmt(CdsDayLen::Bits16, CdsSubMs::Picos, CdsEpoch::Ccsds1958);
        let inst = UtcInstant::from_ascii(&ascii("1988-01-18T17:20:43.123456789012"));
        let code = inst.to_cds(fmt, None).unwrap();
        assert_eq!(
            (
                code.value().day,
                code.value().ms_of_day.get(),
                code.value().subms.get()
            ),
            (10_974, 62_443_123, 456_789_012)
        );
        let us = inst
            .to_cds(
                cds_fmt(CdsDayLen::Bits16, CdsSubMs::Micros, CdsEpoch::Ccsds1958),
                None,
            )
            .unwrap();
        assert_eq!(us.value().subms.get(), 456);
        let err = UtcInstant::from_ascii(&ascii("1957-12-31T23:59:59"))
            .to_cds(fmt, None)
            .unwrap_err();
        assert_eq!(
            (err.kind(), err.field()),
            (ErrorKind::OutOfRange, Some(field::CDS_DAY))
        );
        let err = UtcInstant::from_ascii(&ascii("2137-06-07T00:00:00"))
            .to_cds(fmt, None)
            .unwrap_err();
        assert_eq!(err.field(), Some(field::CDS_DAY));
        let late = UtcInstant::from_ascii(&ascii("2137-06-07T00:00:00"));
        assert_eq!(
            late.to_cds(
                cds_fmt(CdsDayLen::Bits24, CdsSubMs::None, CdsEpoch::Ccsds1958),
                None
            )
            .unwrap()
            .value()
            .day,
            65_536
        );
        let err = UtcInstant::from_ascii(&ascii("2016-06-30T12:30:60"))
            .to_cds(fmt, None)
            .unwrap_err();
        assert_eq!(
            (err.kind(), err.field()),
            (ErrorKind::OutOfRange, Some(field::SECOND))
        );
        for text in ["2016-06-30T23:30:60", "2016-06-30T12:59:60"] {
            assert!(
                UtcInstant::from_ascii(&ascii(text))
                    .to_cds(fmt, None)
                    .is_err(),
                "{text}"
            );
        }
        let ok = UtcInstant::from_ascii(&ascii("2016-12-31T23:59:60.999"))
            .to_cds(fmt, None)
            .unwrap();
        assert_eq!(ok.value().ms_of_day.get(), 86_400_999);
        let agency = cds_fmt(CdsDayLen::Bits16, CdsSubMs::None, CdsEpoch::Agency);
        let inst = UtcInstant::from_ascii(&ascii("2000-01-02T00:00:01"));
        assert_eq!(
            inst.to_cds(agency, None).unwrap_err().kind(),
            ErrorKind::InvalidConfig
        );
        assert_eq!(
            inst.to_cds(agency, Some(epoch(2000, 1, 1)))
                .unwrap()
                .value()
                .day,
            1
        );
        assert_eq!(CDS_EPOCH, crate::calendar::day_number(1958, 1, 1));
    }

    #[test]
    fn instant_to_ccs_and_ascii() {
        // req: TIME-033
        let inst = UtcInstant::from_ascii(&ascii("2024-12-31T01:02:03.987654321098"));
        let doy = inst
            .to_ccs(CcsFormat::new(CcsVariation::DayOfYear, 2).unwrap())
            .unwrap();
        assert_eq!(
            (doy.value().date.day_of_year(), doy.value().subsecond),
            (Some(366), 9876)
        );
        let md = inst
            .to_ccs(CcsFormat::new(CcsVariation::MonthDay, 0).unwrap())
            .unwrap();
        assert_eq!(
            (md.value().date, md.value().subsecond),
            (epoch(2024, 12, 31), 0)
        );
        assert_eq!(UtcInstant::from_ccs(&doy).picos(), 987_600_000_000);
        let doy_ascii = inst
            .to_ascii(AsciiFormat::new(CcsVariation::DayOfYear, 3, true).unwrap())
            .unwrap();
        assert_eq!(
            (
                doy_ascii.date.day_of_year(),
                doy_ascii.fraction,
                doy_ascii.utc
            ),
            (Some(366), Fraction::new(987, 3).unwrap(), true)
        );
        let big = cds_fmt(CdsDayLen::Bits24, CdsSubMs::None, CdsEpoch::Ccsds1958);
        let far = UtcInstant::from_cds(&cds(16_777_215, 0, SubMillis::None, big), None).unwrap();
        assert_eq!(
            far.to_ccs(CcsFormat::new(CcsVariation::MonthDay, 0).unwrap())
                .unwrap_err()
                .kind(),
            ErrorKind::OutOfRange
        );
        let err = far
            .to_ascii(AsciiFormat::new(CcsVariation::MonthDay, 0, false).unwrap())
            .unwrap_err();
        assert_eq!(err.field(), Some(field::YEAR));
        assert_eq!(
            far.date(CcsVariation::DayOfYear).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
        assert_eq!(
            [0, 1, 2, 3, 4, 5, 6].map(ccs_scale),
            [
                1_000_000_000_000,
                10_000_000_000,
                100_000_000,
                1_000_000,
                10_000,
                100,
                1
            ]
        );
    }
}
