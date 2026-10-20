//! Range-checked calendar and clock fields shared by CCS and the ASCII codes (301.0 §3.4,
//! §3.5, Annex A; TIME-030, TIME-031).
//!
//! Each type can hold only values that 301.0 Annex A allows, so a value is checked once, when
//! it is constructed or decoded, and never again (parse, don't validate). A [`CalendarDate`]
//! exists only for days that occur in its year: the month/day or day-of-year check that needs
//! the year is made when the date is built.

use aldis_core::Result;

use crate::ccs::CcsVariation;
use crate::util::{bounded, out_of_bounds, wide8, wide16};

/// Gregorian leap year: divisible by 4, except centuries not divisible by 400 (301.0 Annex A).
pub(crate) const fn is_leap(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

/// Days in `month` (1–12) of `year`; 0 for an invalid month.
pub(crate) const fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days in `year`.
pub(crate) const fn days_in_year(year: u16) -> u16 {
    if is_leap(year) { 366 } else { 365 }
}

/// Days from 0000-03-01 to `year`-`month`-`day` in the proleptic Gregorian calendar, by
/// Hinnant's closed-form `days_from_civil`: no loops or tables (M1 D3, GEN-022). Valid for
/// years 1–9999 and months 1–12; every intermediate stays far below `u32::MAX`.
#[expect(
    clippy::as_conversions,
    reason = "lossless widening u16/u8 -> u32 in const context"
)]
pub(crate) const fn day_number(year: u16, month: u8, day: u8) -> u32 {
    // March-based year: January and February count with the previous year.
    let y = if month <= 2 {
        (year as u32).wrapping_sub(1)
    } else {
        year as u32
    };
    let m = month as u32;
    let era = y / 400;
    let yoe = y % 400;
    let mp = if m > 2 {
        m.wrapping_sub(3)
    } else {
        m.wrapping_add(9)
    };
    let doy =
        ((153_u32.wrapping_mul(mp).wrapping_add(2)) / 5).wrapping_add((day as u32).wrapping_sub(1));
    let doe = yoe
        .wrapping_mul(365)
        .wrapping_add(yoe / 4)
        .wrapping_sub(yoe / 100)
        .wrapping_add(doy);
    era.wrapping_mul(146_097).wrapping_add(doe)
}

/// The civil date of day number `day` (days from 0000-03-01), by Hinnant's closed-form
/// `civil_from_days` (M1 D3, GEN-022). The year may exceed 9999 for large CDS day counts; the
/// caller checks it.
pub(crate) const fn civil(day: u32) -> (u32, u8, u8) {
    let era = day / 146_097;
    let doe = day % 146_097;
    let yoe = doe
        .wrapping_sub(doe / 1460)
        .wrapping_add(doe / 36_524)
        .wrapping_sub(doe / 146_096)
        / 365;
    let doy = doe.wrapping_sub(
        365_u32
            .wrapping_mul(yoe)
            .wrapping_add(yoe / 4)
            .wrapping_sub(yoe / 100),
    );
    let mp = (5_u32.wrapping_mul(doy).wrapping_add(2)) / 153;
    let d = doy
        .wrapping_sub((153_u32.wrapping_mul(mp).wrapping_add(2)) / 5)
        .wrapping_add(1);
    let m = if mp < 10 {
        mp.wrapping_add(3)
    } else {
        mp.wrapping_sub(9)
    };
    let y = yoe.wrapping_add(era.wrapping_mul(400));
    let y = if m <= 2 { y.wrapping_add(1) } else { y };
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "month is 1-12 and day 1-31 by construction of the algorithm"
    )]
    (y, m as u8, d as u8)
}

bounded!(
    /// Year A.D., 1–9999 (301.0 Annex A, §3.5.1; TIME-030).
    Year, u16, "year", 1, 9999
);
bounded!(
    /// Month of year, 1–12 (301.0 Annex A; TIME-030).
    Month, u8, "month", 1, 12
);
bounded!(
    /// Hour of day, 0–23 (301.0 Annex A; TIME-030).
    Hour, u8, "hour", 0, 23
);
bounded!(
    /// Minute of hour, 0–59 (301.0 Annex A; TIME-030).
    Minute, u8, "minute", 0, 59
);
bounded!(
    /// Second of minute, 0–60: 60 is a positive leap second, accepted at any minute because
    /// leap-second handling is the caller's (301.0 Annex A, TIME-022, TIME-030).
    Second, u8, "second", 0, 60
);

/// How a [`CalendarDate`] names its day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Day {
    /// Month and day of month.
    MonthDay(Month, u8),
    /// Day of year.
    Ordinal(u16),
}

/// A date that exists: a year with a valid month and day of month, or a valid day of year
/// (301.0 §3.4.1, Annex A; TIME-030).
///
/// Equality keeps the calendar variation: the same day built with
/// [`from_month_day`](Self::from_month_day) and with
/// [`from_day_of_year`](Self::from_day_of_year) compares unequal, because a CCS code carries the
/// variation on the wire (301.0 §3.4.2 bit 4). It is not a chronological comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CalendarDate {
    year: Year,
    day: Day,
}

impl CalendarDate {
    /// The date `day` of `month` in `year`.
    ///
    /// # Errors
    /// `OutOfRange` (`day`) unless `day` is 1 to the number of days in that month, with leap
    /// years for February (301.0 Annex A, TIME-030).
    pub const fn from_month_day(year: Year, month: Month, day: u8) -> Result<CalendarDate> {
        let last = days_in_month(year.0, month.0);
        if day == 0 || day > last {
            return Err(out_of_bounds("day", wide8(day), 1, wide8(last)));
        }
        Ok(CalendarDate {
            year,
            day: Day::MonthDay(month, day),
        })
    }

    /// The date with day of year `day_of_year` in `year`.
    ///
    /// # Errors
    /// `OutOfRange` (`day_of_year`) unless it is 1 to 365, or 366 in a leap year (301.0 Annex
    /// A, TIME-030).
    pub const fn from_day_of_year(year: Year, day_of_year: u16) -> Result<CalendarDate> {
        let last = days_in_year(year.0);
        if day_of_year == 0 || day_of_year > last {
            return Err(out_of_bounds(
                "day_of_year",
                wide16(day_of_year),
                1,
                wide16(last),
            ));
        }
        Ok(CalendarDate {
            year,
            day: Day::Ordinal(day_of_year),
        })
    }

    /// A month/day date the caller already knows exists (conversions, from [`civil`]).
    pub(crate) const fn from_month_day_unchecked(
        year: Year,
        month: Month,
        day: u8,
    ) -> CalendarDate {
        CalendarDate {
            year,
            day: Day::MonthDay(month, day),
        }
    }

    /// A day-of-year date the caller already knows exists (conversions).
    pub(crate) const fn from_day_of_year_unchecked(year: Year, day_of_year: u16) -> CalendarDate {
        CalendarDate {
            year,
            day: Day::Ordinal(day_of_year),
        }
    }

    /// The day number of this date (days from 0000-03-01, see [`day_number`]).
    #[expect(
        clippy::as_conversions,
        reason = "lossless widening u16 -> u32 in const context"
    )]
    pub(crate) const fn day_number(self) -> u32 {
        match self.day {
            Day::MonthDay(month, day) => day_number(self.year.0, month.0, day),
            Day::Ordinal(doy) => day_number(self.year.0, 1, 1)
                .wrapping_add(doy as u32)
                .wrapping_sub(1),
        }
    }

    /// The year (301.0 Annex A, TIME-030).
    #[must_use]
    pub const fn year(&self) -> Year {
        self.year
    }

    /// Month and day of month, for a date built with
    /// [`from_month_day`](Self::from_month_day) (301.0 §3.4.1.1, TIME-030).
    #[must_use]
    pub const fn month_day(&self) -> Option<(Month, u8)> {
        match self.day {
            Day::MonthDay(month, day) => Some((month, day)),
            Day::Ordinal(_) => None,
        }
    }

    /// Day of year, for a date built with [`from_day_of_year`](Self::from_day_of_year) (301.0
    /// §3.4.1.2, TIME-030).
    #[must_use]
    pub const fn day_of_year(&self) -> Option<u16> {
        match self.day {
            Day::MonthDay(..) => None,
            Day::Ordinal(day) => Some(day),
        }
    }

    /// The CCS calendar variation this date is expressed in (301.0 §3.4.2 bit 4, TIME-030).
    #[must_use]
    pub const fn variation(&self) -> CcsVariation {
        match self.day {
            Day::MonthDay(..) => CcsVariation::MonthDay,
            Day::Ordinal(_) => CcsVariation::DayOfYear,
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{
        CalendarDate, Hour, Minute, Month, Second, Year, days_in_month, days_in_year, is_leap,
    };
    use crate::ccs::CcsVariation;
    use aldis_core::{ErrorDetail, ErrorKind, FieldId};

    #[test]
    fn gregorian_rules() {
        // req: TIME-030
        assert!(is_leap(2024) && is_leap(2000) && !is_leap(1900) && !is_leap(2023));
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2023, 4), 30);
        assert_eq!(days_in_month(2023, 12), 31);
        assert_eq!(days_in_month(2023, 0), 0);
        assert_eq!(days_in_month(2023, 13), 0);
        assert_eq!((days_in_year(2024), days_in_year(2023)), (366, 365));
    }

    #[test]
    fn fields_hold_only_valid_values() {
        // req: TIME-030, GEN-041
        assert_eq!(Year::new(1).map(Year::get), Some(1));
        assert_eq!(Year::new(9999).map(u16::from), Some(9999));
        assert_eq!((Year::new(0), Year::new(10_000)), (None, None));
        assert_eq!((Year::MIN, Year::MAX), (1, 9999));
        assert_eq!(Month::new(12).map(Month::get), Some(12));
        assert_eq!((Month::new(0), Month::new(13)), (None, None));
        assert_eq!(Hour::new(23).map(u8::from), Some(23));
        assert_eq!(Hour::new(24), None);
        assert_eq!(Minute::new(59).map(Minute::get), Some(59));
        assert_eq!(Minute::new(60), None);
        assert_eq!(Second::new(60).map(Second::get), Some(60));
        assert_eq!(Second::new(61), None);
        assert_eq!(Second::new(0).map(Second::get), Some(0));
        for (e, field, expected, found) in [
            (Year::try_from(0).unwrap_err(), "year", 1, 0),
            (Year::try_from(10_000).unwrap_err(), "year", 9999, 10_000),
            (Month::try_from(0).unwrap_err(), "month", 1, 0),
            (Month::try_from(13).unwrap_err(), "month", 12, 13),
            (Hour::try_from(24).unwrap_err(), "hour", 23, 24),
            (Minute::try_from(60).unwrap_err(), "minute", 59, 60),
            (Second::try_from(61).unwrap_err(), "second", 60, 61),
        ] {
            assert_eq!(e.kind(), ErrorKind::OutOfRange);
            assert_eq!(e.field().map(FieldId::name), Some(field));
            assert_eq!(e.detail(), ErrorDetail::Expected { expected, found });
        }
        assert_eq!(Hour::try_from(0).map(Hour::get), Ok(0));
    }

    #[test]
    fn day_numbers_are_a_bijection_over_years_1_to_9999() {
        // req: TIME-033, GEN-022
        use super::{civil, day_number};
        // An independent day-by-day walk checks the closed-form functions (M1 D3).
        let mut expected = day_number(1, 1, 1);
        for year in 1..=9999_u16 {
            for month in 1..=12_u8 {
                for day in 1..=days_in_month(year, month) {
                    assert_eq!(
                        day_number(year, month, day),
                        expected,
                        "{year}-{month}-{day}"
                    );
                    assert_eq!(civil(expected), (u32::from(year), month, day));
                    expected += 1;
                }
            }
        }
        assert_eq!(day_number(1970, 1, 1) - day_number(1958, 1, 1), 4383);
        let (y, m, d) = civil(day_number(9999, 12, 31) + 1);
        assert_eq!(
            (y, m, d),
            (10_000, 1, 1),
            "the year can pass 9999; callers check it"
        );
        let date = CalendarDate::from_day_of_year(Year::new(2024).unwrap(), 60).unwrap();
        assert_eq!(date.day_number(), day_number(2024, 2, 29));
        assert_eq!(
            CalendarDate::from_day_of_year_unchecked(Year::new(2024).unwrap(), 60),
            date
        );
        let md = CalendarDate::from_month_day_unchecked(
            Year::new(2024).unwrap(),
            Month::new(2).unwrap(),
            29,
        );
        assert_eq!(md.day_number(), date.day_number());
        assert_eq!((Year::default().get(), Hour::default().get()), (1, 0));
    }

    #[test]
    fn dates_exist() {
        // req: TIME-030, GEN-041
        let y = |n| Year::new(n).unwrap();
        let m = |n| Month::new(n).unwrap();
        let d = CalendarDate::from_month_day(y(2024), m(2), 29).unwrap();
        assert_eq!(
            (d.year(), d.month_day(), d.day_of_year()),
            (y(2024), Some((m(2), 29)), None)
        );
        assert_eq!(d.variation(), CcsVariation::MonthDay);
        let o = CalendarDate::from_day_of_year(y(2024), 366).unwrap();
        assert_eq!(
            (o.month_day(), o.day_of_year(), o.variation()),
            (None, Some(366), CcsVariation::DayOfYear)
        );
        for (e, field, expected, found) in [
            (
                CalendarDate::from_month_day(y(2023), m(2), 29).unwrap_err(),
                "day",
                28,
                29,
            ),
            (
                CalendarDate::from_month_day(y(2023), m(4), 0).unwrap_err(),
                "day",
                1,
                0,
            ),
            (
                CalendarDate::from_month_day(y(1900), m(2), 29).unwrap_err(),
                "day",
                28,
                29,
            ),
            (
                CalendarDate::from_day_of_year(y(2023), 366).unwrap_err(),
                "day_of_year",
                365,
                366,
            ),
            (
                CalendarDate::from_day_of_year(y(2023), 0).unwrap_err(),
                "day_of_year",
                1,
                0,
            ),
        ] {
            assert_eq!(e.kind(), ErrorKind::OutOfRange);
            assert_eq!(e.field().map(FieldId::name), Some(field));
            assert_eq!(e.detail(), ErrorDetail::Expected { expected, found });
        }
        assert!(CalendarDate::from_month_day(y(2000), m(2), 29).is_ok());
        assert!(CalendarDate::from_month_day(y(2023), m(12), 31).is_ok());
    }
}
