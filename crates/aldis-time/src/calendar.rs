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

    /// The year (TIME-030).
    #[must_use]
    pub const fn year(&self) -> Year {
        self.year
    }

    /// Month and day of month, for a date built with
    /// [`from_month_day`](Self::from_month_day) (301.0 §3.4.1.1).
    #[must_use]
    pub const fn month_day(&self) -> Option<(Month, u8)> {
        match self.day {
            Day::MonthDay(month, day) => Some((month, day)),
            Day::Ordinal(_) => None,
        }
    }

    /// Day of year, for a date built with [`from_day_of_year`](Self::from_day_of_year) (301.0
    /// §3.4.1.2).
    #[must_use]
    pub const fn day_of_year(&self) -> Option<u16> {
        match self.day {
            Day::MonthDay(..) => None,
            Day::Ordinal(day) => Some(day),
        }
    }

    /// The CCS calendar variation this date is expressed in (301.0 §3.4.2 bit 4).
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
