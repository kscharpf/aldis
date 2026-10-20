//! ASCII time codes A and B, and their subsets (301.0 §3.5; TIME-031, TIME-032).
//!
//! The grammar is written down in `docs/standards/time-codes.md` §5.1 (OI-29). Parsing reads
//! octets, not `str`, and formatting writes octets into a caller buffer without `core::fmt`,
//! which would bring formatting machinery with panic paths into flight builds.
//!
//! ASCII codes are UTC-based; second 60 is accepted at any minute, and leap-second handling is
//! the caller's responsibility (TIME-022, TIME-031).

use aldis_core::{Error, ErrorKind, FieldId, Result};

use crate::calendar::{CalendarDate, Hour, Minute, Month, Second, Year, days_in_month, is_leap};
use crate::ccs::CcsVariation;
use crate::util::{out_of_bounds, wide8};

/// Most fractional digits: 12, down to 10⁻¹² s (301.0 §3.5.1, TIME-031).
pub const MAX_FRACTION_DIGITS: u8 = 12;

/// Longest complete ASCII code in octets: `YYYY-MM-DDThh:mm:ss.`, 12 digits, and `Z` (301.0
/// §3.5.1.1; TIME-031; grammar: time-codes.md §5.1).
pub const MAX_ASCII_LEN: usize = 33;

/// `Malformed` for a structural problem: length, separators, or stray octets (§5.1).
const fn malformed() -> Error {
    Error::new(ErrorKind::Malformed)
}

/// `InvalidField` naming the subfield that holds a non-digit (§5.1).
const fn not_digit(field: &'static str) -> Error {
    Error::new(ErrorKind::InvalidField).with_field(FieldId::new(field))
}

/// 10^`n` for `n` = 0–12.
const fn pow10(n: u8) -> u64 {
    match n {
        0 => 1,
        1 => 10,
        2 => 100,
        3 => 1_000,
        4 => 10_000,
        5 => 100_000,
        6 => 1_000_000,
        7 => 10_000_000,
        8 => 100_000_000,
        9 => 1_000_000_000,
        10 => 10_000_000_000,
        11 => 100_000_000_000,
        _ => 1_000_000_000_000,
    }
}

/// The decimal fraction of a second: up to 12 digits and a value that fits them (301.0 §3.5.1;
/// TIME-031, RS-020). `1234` with 4 digits is 0.1234 s; leading zeros count as digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Fraction {
    digits: u8,
    value: u64,
}

impl Fraction {
    /// No fractional digits: the code has no period (301.0 §3.5.1, TIME-031).
    pub const NONE: Fraction = Fraction {
        digits: 0,
        value: 0,
    };

    /// A fraction of `digits` digits (0–12) whose value `value` is below 10^`digits`, or `None`
    /// (301.0 §3.5.1, TIME-031).
    #[must_use]
    pub const fn new(value: u64, digits: u8) -> Option<Fraction> {
        if digits <= MAX_FRACTION_DIGITS && value < pow10(digits) {
            Some(Fraction { digits, value })
        } else {
            None
        }
    }

    /// Number of digits (301.0 §3.5.1, TIME-031).
    #[must_use]
    pub const fn digits(&self) -> u8 {
        self.digits
    }

    /// The digits as an integer (301.0 §3.5.1, TIME-031).
    #[must_use]
    pub const fn value(&self) -> u64 {
        self.value
    }

    /// The fraction in picoseconds, exactly (301.0 §3.5.1, TIME-033).
    #[must_use]
    pub const fn picos(&self) -> u64 {
        self.value
            .wrapping_mul(pow10(MAX_FRACTION_DIGITS.wrapping_sub(self.digits)))
    }

    /// The fraction with `format`'s digits for `picos` (below 10¹²), truncated toward zero
    /// (TIME-033). Taking the format, not a digit count, keeps the count at most 12 (RS-020).
    pub(crate) fn from_picos(picos: u64, format: AsciiFormat) -> Fraction {
        let digits = format.fraction_digits;
        Fraction {
            digits,
            value: picos
                .checked_div(pow10(MAX_FRACTION_DIGITS.wrapping_sub(digits)))
                .unwrap_or(0),
        }
    }
}

/// Parsing options (301.0 §3.5.1, TIME-031).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AsciiOptions {
    max_fraction_digits: u8,
}

impl AsciiOptions {
    /// Up to 12 fractional digits (301.0 §3.5.1, TIME-031).
    pub const DEFAULT: AsciiOptions = AsciiOptions {
        max_fraction_digits: MAX_FRACTION_DIGITS,
    };

    /// Options accepting at most `max_fraction_digits` fractional digits: the configurable
    /// maximum of TIME-031 (301.0 §3.5.1).
    ///
    /// # Errors
    /// `OutOfRange` (`max_fraction_digits`) above 12.
    pub const fn new(max_fraction_digits: u8) -> Result<AsciiOptions> {
        if max_fraction_digits > MAX_FRACTION_DIGITS {
            return Err(Error::out_of_range(
                "max_fraction_digits",
                wide8(MAX_FRACTION_DIGITS),
                wide8(max_fraction_digits),
            ));
        }
        Ok(AsciiOptions {
            max_fraction_digits,
        })
    }

    /// The maximum number of fractional digits (301.0 §3.5.1, TIME-031).
    #[must_use]
    pub const fn max_fraction_digits(&self) -> u8 {
        self.max_fraction_digits
    }
}

impl Default for AsciiOptions {
    fn default() -> AsciiOptions {
        AsciiOptions::DEFAULT
    }
}

/// The layout of an ASCII code to produce: code A or B, fractional digits, and `Z`
/// (301.0 §3.5.1; TIME-031, TIME-033).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AsciiFormat {
    variation: CcsVariation,
    fraction_digits: u8,
    utc: bool,
}

impl AsciiFormat {
    /// Code A (`MonthDay`) or B (`DayOfYear`) with `fraction_digits` digits (0–12), ending
    /// with `Z` if `utc` (301.0 §3.5.1.1–2, TIME-031).
    ///
    /// # Errors
    /// `OutOfRange` (`fraction_digits`) above 12.
    pub const fn new(
        variation: CcsVariation,
        fraction_digits: u8,
        utc: bool,
    ) -> Result<AsciiFormat> {
        if fraction_digits > MAX_FRACTION_DIGITS {
            return Err(Error::out_of_range(
                "fraction_digits",
                wide8(MAX_FRACTION_DIGITS),
                wide8(fraction_digits),
            ));
        }
        Ok(AsciiFormat {
            variation,
            fraction_digits,
            utc,
        })
    }

    /// Code A or B (301.0 §3.5.1.1–2, TIME-031).
    #[must_use]
    pub const fn variation(&self) -> CcsVariation {
        self.variation
    }

    /// Fractional digits (301.0 §3.5.1, TIME-031).
    #[must_use]
    pub const fn fraction_digits(&self) -> u8 {
        self.fraction_digits
    }

    /// Whether the code ends with `Z` (301.0 §3.5.1, TIME-031).
    #[must_use]
    pub const fn utc(&self) -> bool {
        self.utc
    }
}

/// The value of an all-digit subfield of at most 4 digits.
///
/// # Errors
/// `InvalidField` (`field`) for a non-digit octet.
fn number(digits: &[u8], field: &'static str) -> Result<u16> {
    digits.iter().try_fold(0_u16, |acc, &b| {
        if b.is_ascii_digit() {
            Ok(acc
                .wrapping_mul(10)
                .wrapping_add(u16::from(b.wrapping_sub(b'0'))))
        } else {
            Err(not_digit(field))
        }
    })
}

/// Narrows a subfield value of at most 2 digits.
fn small(n: u16) -> u8 {
    u8::try_from(n).unwrap_or(u8::MAX)
}

/// The fraction digits after a `.`: at least one, all digits, at most `max` (§5.1).
///
/// # Errors
/// `Malformed` if empty or containing `Z`; `InvalidField` (`fraction`) for another non-digit;
/// `OutOfRange` (`fraction`) for more than `max` digits.
fn fraction_digits(digits: &[u8], max: u8) -> Result<Fraction> {
    if digits.is_empty() || digits.contains(&b'Z') {
        return Err(malformed());
    }
    if !digits.iter().all(u8::is_ascii_digit) {
        return Err(not_digit("fraction"));
    }
    let found = u64::try_from(digits.len()).unwrap_or(u64::MAX);
    if found > u64::from(max) {
        return Err(Error::out_of_range("fraction", wide8(max), found));
    }
    let value = digits.iter().fold(0_u64, |acc, &b| {
        acc.wrapping_mul(10)
            .wrapping_add(u64::from(b.wrapping_sub(b'0')))
    });
    Ok(Fraction {
        digits: u8::try_from(digits.len()).unwrap_or(max),
        value,
    })
}

/// What follows `ss` in a complete code: nothing, or `.` and digits, then an optional `Z`.
fn parse_tail(tail: &[u8], max: u8) -> Result<(Fraction, bool)> {
    let (body, utc) = match tail {
        [body @ .., b'Z'] => (body, true),
        _ => (tail, false),
    };
    match body {
        [] => Ok((Fraction::NONE, utc)),
        [b'.', digits @ ..] => Ok((fraction_digits(digits, max)?, utc)),
        _ => Err(malformed()),
    }
}

/// Two decimal digits.
const fn dec2(n: u8) -> [u8; 2] {
    [b'0'.wrapping_add(n / 10 % 10), b'0'.wrapping_add(n % 10)]
}

/// Three decimal digits.
fn dec3(n: u16) -> [u8; 3] {
    let [h, t, o] = [n / 100 % 10, n / 10 % 10, n % 10].map(|d| small(d).wrapping_add(b'0'));
    [h, t, o]
}

/// Four decimal digits.
fn dec4(n: u16) -> [u8; 4] {
    [n / 1000 % 10, n / 100 % 10, n / 10 % 10, n % 10].map(|d| small(d).wrapping_add(b'0'))
}

/// Twelve decimal digits, zero-padded.
fn dec12(mut n: u64) -> [u8; 12] {
    let mut out = [b'0'; 12];
    for slot in out.iter_mut().rev() {
        *slot = u8::try_from(n % 10).unwrap_or(0).wrapping_add(b'0');
        n /= 10;
    }
    out
}

/// A complete ASCII time code A or B (301.0 §3.5.1.1–2, TIME-031). Every field is valid by
/// construction (RS-020); code A or B follows the date's variation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AsciiTime {
    /// The date: month and day for code A, day of year for code B (301.0 §3.5.1, TIME-031).
    pub date: CalendarDate,
    /// Hour (301.0 §3.5.1.1, TIME-031).
    pub hour: Hour,
    /// Minute (301.0 §3.5.1.1, TIME-031).
    pub minute: Minute,
    /// Second, 60 for a leap second (301.0 §3.5.1.1, TIME-031).
    pub second: Second,
    /// Decimal fraction of the second; [`Fraction::NONE`] writes no period (301.0 §3.5.1,
    /// TIME-031).
    pub fraction: Fraction,
    /// Whether the code ends with the optional `Z`, preserved on round trip (301.0 §3.5.1,
    /// TIME-031).
    pub utc: bool,
}

/// The date subfields of a complete code, before their values are checked.
enum DateDigits {
    MonthDay([u8; 4], [u8; 2], [u8; 2]),
    Ordinal([u8; 4], [u8; 3]),
}

impl AsciiTime {
    /// Parses a complete code A or B; the input must be exactly the code (301.0 §3.5.1.1–2,
    /// TIME-031; grammar: time-codes.md §5.1).
    ///
    /// # Errors
    /// `Malformed` for a separator, length, or trailing-octet problem; `InvalidField` naming a
    /// subfield that holds a non-digit; `OutOfRange` naming a value outside its range, or
    /// `fraction` for more digits than `options` allow.
    pub fn parse(text: &[u8], options: &AsciiOptions) -> Result<AsciiTime> {
        let code_a = text.get(4) == Some(&b'-') && text.get(7) == Some(&b'-');
        let (date, rest) = if code_a {
            let [y0, y1, y2, y3, b'-', m0, m1, b'-', d0, d1, b'T', rest @ ..] = text else {
                return Err(malformed());
            };
            (
                DateDigits::MonthDay([*y0, *y1, *y2, *y3], [*m0, *m1], [*d0, *d1]),
                rest,
            )
        } else {
            let [y0, y1, y2, y3, b'-', d0, d1, d2, b'T', rest @ ..] = text else {
                return Err(malformed());
            };
            (
                DateDigits::Ordinal([*y0, *y1, *y2, *y3], [*d0, *d1, *d2]),
                rest,
            )
        };
        let [h0, h1, b':', i0, i1, b':', s0, s1, tail @ ..] = rest else {
            return Err(malformed());
        };
        let (fraction, utc) = parse_tail(tail, options.max_fraction_digits)?;
        // Digits in subfield order, then ranges in the same order (§5.1).
        let date = match date {
            DateDigits::MonthDay(y, m, d) => {
                let (y, m, d) = (
                    number(&y, "year")?,
                    number(&m, "month")?,
                    number(&d, "day")?,
                );
                (y, Some((m, d)), None)
            }
            DateDigits::Ordinal(y, d) => {
                (number(&y, "year")?, None, Some(number(&d, "day_of_year")?))
            }
        };
        let hour = number(&[*h0, *h1], "hour")?;
        let minute = number(&[*i0, *i1], "minute")?;
        let second = number(&[*s0, *s1], "second")?;
        let year = Year::try_from(date.0)?;
        let date = match (date.1, date.2) {
            (Some((month, day)), _) => {
                CalendarDate::from_month_day(year, Month::try_from(small(month))?, small(day))?
            }
            (None, doy) => CalendarDate::from_day_of_year(year, doy.unwrap_or(0))?,
        };
        Ok(AsciiTime {
            date,
            hour: Hour::try_from(small(hour))?,
            minute: Minute::try_from(small(minute))?,
            second: Second::try_from(small(second))?,
            fraction,
            utc,
        })
    }

    /// Length of the encoded code in octets, at most [`MAX_ASCII_LEN`] (301.0 §3.5.1, TIME-031).
    #[must_use]
    pub fn encoded_len(&self) -> usize {
        let date: usize = if self.date.month_day().is_some() {
            10
        } else {
            8
        };
        let fraction = if self.fraction.digits > 0 {
            usize::from(self.fraction.digits).wrapping_add(1)
        } else {
            0
        };
        // `T` and `hh:mm:ss` are 9 octets.
        date.wrapping_add(9)
            .wrapping_add(fraction)
            .wrapping_add(usize::from(self.utc))
    }

    /// Writes the code into the start of `out`, without `core::fmt`; returns its length
    /// (301.0 §3.5.1; TIME-031, API-022).
    ///
    /// # Errors
    /// `BufferTooSmall` (`needed` = [`encoded_len`](Self::encoded_len)) if `out` is too short;
    /// `out` is then unchanged.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize> {
        let n = self.encoded_len();
        let dst = out.get_mut(..n).ok_or(Error::buffer_too_small(n))?;
        let [y0, y1, y2, y3] = dec4(self.date.year().get());
        let date = match (self.date.month_day(), self.date.day_of_year()) {
            (Some((month, day)), _) => {
                let ([m0, m1], [d0, d1]) = (dec2(month.get()), dec2(day));
                [y0, y1, y2, y3, b'-', m0, m1, b'-', d0, d1]
            }
            (None, doy) => {
                let [d0, d1, d2] = dec3(doy.unwrap_or(0));
                [y0, y1, y2, y3, b'-', d0, d1, d2, 0, 0]
            }
        };
        let ([h0, h1], [i0, i1], [s0, s1]) = (
            dec2(self.hour.get()),
            dec2(self.minute.get()),
            dec2(self.second.get()),
        );
        let clock = [b'T', h0, h1, b':', i0, i1, b':', s0, s1];
        let digits = dec12(self.fraction.value);
        let dot: &[u8] = if self.fraction.digits > 0 { b"." } else { b"" };
        let zulu: &[u8] = if self.utc { b"Z" } else { b"" };
        let skip = usize::from(MAX_FRACTION_DIGITS.wrapping_sub(self.fraction.digits));
        let octets = date
            .iter()
            .take(if self.date.month_day().is_some() {
                10
            } else {
                8
            })
            .chain(&clock)
            .chain(dot)
            .chain(digits.iter().skip(skip))
            .chain(zulu);
        for (d, s) in dst.iter_mut().zip(octets) {
            *d = *s;
        }
        Ok(n)
    }
}

/// One subfield of a subset shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Year,
    Month,
    Day,
    DayOfYear,
    Hour,
    Minute,
    Second,
}

impl Part {
    /// The field name used in errors.
    const fn name(self) -> &'static str {
        match self {
            Part::Year => "year",
            Part::Month => "month",
            Part::Day => "day",
            Part::DayOfYear => "day_of_year",
            Part::Hour => "hour",
            Part::Minute => "minute",
            Part::Second => "second",
        }
    }
}

/// A subset shape: `-` and `:` are literal, every other octet is a digit position (§5.1).
struct Shape {
    pattern: &'static [u8],
    variation: Option<CcsVariation>,
    /// Subfields as (part, offset, length).
    parts: &'static [(Part, usize, usize)],
    /// Calendar: ends with its last subfield. Time: ends at `ss`, so a fraction may follow.
    ends_full: bool,
    /// Time: starts with `hh`.
    starts_with_hour: bool,
}

const MD: Option<CcsVariation> = Some(CcsVariation::MonthDay);
const DOY: Option<CcsVariation> = Some(CcsVariation::DayOfYear);

/// Calendar shapes of 301.0 §3.5.1.3 (§5.1).
const CALENDAR: [Shape; 8] = [
    Shape {
        pattern: b"YYYY-MM-DD",
        variation: MD,
        parts: &[(Part::Year, 0, 4), (Part::Month, 5, 2), (Part::Day, 8, 2)],
        ends_full: true,
        starts_with_hour: false,
    },
    Shape {
        pattern: b"YYYY-MM",
        variation: MD,
        parts: &[(Part::Year, 0, 4), (Part::Month, 5, 2)],
        ends_full: false,
        starts_with_hour: false,
    },
    Shape {
        pattern: b"YYYY",
        variation: None,
        parts: &[(Part::Year, 0, 4)],
        ends_full: false,
        starts_with_hour: false,
    },
    Shape {
        pattern: b"-MM-DD",
        variation: MD,
        parts: &[(Part::Month, 1, 2), (Part::Day, 4, 2)],
        ends_full: true,
        starts_with_hour: false,
    },
    Shape {
        pattern: b"--DD",
        variation: MD,
        parts: &[(Part::Day, 2, 2)],
        ends_full: true,
        starts_with_hour: false,
    },
    Shape {
        pattern: b"-MM",
        variation: MD,
        parts: &[(Part::Month, 1, 2)],
        ends_full: false,
        starts_with_hour: false,
    },
    Shape {
        pattern: b"YYYY-DDD",
        variation: DOY,
        parts: &[(Part::Year, 0, 4), (Part::DayOfYear, 5, 3)],
        ends_full: true,
        starts_with_hour: false,
    },
    Shape {
        pattern: b"-DDD",
        variation: DOY,
        parts: &[(Part::DayOfYear, 1, 3)],
        ends_full: true,
        starts_with_hour: false,
    },
];

/// Time shapes of 301.0 §3.5.1.3, before any fraction (§5.1).
const TIME: [Shape; 7] = [
    Shape {
        pattern: b"hh:mm:ss",
        variation: None,
        parts: &[
            (Part::Hour, 0, 2),
            (Part::Minute, 3, 2),
            (Part::Second, 6, 2),
        ],
        ends_full: true,
        starts_with_hour: true,
    },
    Shape {
        pattern: b"hh:mm",
        variation: None,
        parts: &[(Part::Hour, 0, 2), (Part::Minute, 3, 2)],
        ends_full: false,
        starts_with_hour: true,
    },
    Shape {
        pattern: b"hh",
        variation: None,
        parts: &[(Part::Hour, 0, 2)],
        ends_full: false,
        starts_with_hour: true,
    },
    Shape {
        pattern: b":mm:ss",
        variation: None,
        parts: &[(Part::Minute, 1, 2), (Part::Second, 4, 2)],
        ends_full: true,
        starts_with_hour: false,
    },
    Shape {
        pattern: b":mm",
        variation: None,
        parts: &[(Part::Minute, 1, 2)],
        ends_full: false,
        starts_with_hour: false,
    },
    Shape {
        pattern: b"::ss",
        variation: None,
        parts: &[(Part::Second, 2, 2)],
        ends_full: true,
        starts_with_hour: false,
    },
    Shape {
        pattern: b"::",
        variation: None,
        parts: &[],
        ends_full: true,
        starts_with_hour: false,
    },
];

/// The shape that `text` has, if any: same length, separators where the shape has them, and no
/// separator anywhere else.
fn shape_of<'s>(text: &[u8], shapes: &'s [Shape]) -> Option<&'s Shape> {
    shapes.iter().find(|shape| {
        shape.pattern.len() == text.len()
            && shape.pattern.iter().zip(text).all(|(&p, &c)| {
                if p == b'-' || p == b':' {
                    c == p
                } else {
                    c != b'-' && c != b':'
                }
            })
    })
}

/// The raw subfield values of a subset, before range checks.
#[derive(Debug, Clone, Copy, Default)]
struct Raw {
    year: Option<u16>,
    month: Option<u16>,
    day: Option<u16>,
    day_of_year: Option<u16>,
    hour: Option<u16>,
    minute: Option<u16>,
    second: Option<u16>,
}

impl Raw {
    /// Reads the digits of every subfield of `shape` from `text`.
    fn read(&mut self, text: &[u8], shape: &Shape) -> Result<()> {
        for &(part, at, len) in shape.parts {
            let digits = text.get(at..at.wrapping_add(len)).unwrap_or_default();
            let value = Some(number(digits, part.name())?);
            match part {
                Part::Year => self.year = value,
                Part::Month => self.month = value,
                Part::Day => self.day = value,
                Part::DayOfYear => self.day_of_year = value,
                Part::Hour => self.hour = value,
                Part::Minute => self.minute = value,
                Part::Second => self.second = value,
            }
        }
        Ok(())
    }
}

/// A subset of an ASCII time code: any subfield may be absent (301.0 §3.5.1.3, TIME-032).
///
/// Every present field is valid, using the widest case where context is missing: a day of
/// month without a month is 1–31, with a month but no year it may be February 29, and a day of
/// year without a year is 1–366 (§5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AsciiSubset {
    variation: Option<CcsVariation>,
    year: Option<Year>,
    month: Option<Month>,
    day: Option<u8>,
    day_of_year: Option<u16>,
    hour: Option<Hour>,
    minute: Option<Minute>,
    second: Option<Second>,
    fraction: Option<Fraction>,
    utc: bool,
}

impl AsciiSubset {
    /// Parses a complete code or any subset of 301.0 §3.5.1.3; the explicit subset parser of
    /// TIME-032. The input must be exactly the code.
    ///
    /// # Errors
    /// `Malformed` for a shape §5.1 does not allow, including `Z` without a time part (OI-29);
    /// `InvalidField` naming a subfield that holds a non-digit; `OutOfRange` naming a value
    /// outside its range, or `fraction` for more digits than `options` allow.
    pub fn parse(text: &[u8], options: &AsciiOptions) -> Result<AsciiSubset> {
        let (text, utc) = match text {
            [body @ .., b'Z'] => (body, true),
            _ => (text, false),
        };
        if text.is_empty() || text.iter().filter(|&&b| b == b'T').nth(1).is_some() {
            return Err(malformed());
        }
        let mut halves = text.splitn(2, |&b| b == b'T');
        let first = halves.next().unwrap_or_default();
        let (calendar, time) = match halves.next() {
            Some(time) if first.is_empty() || time.is_empty() => return Err(malformed()),
            Some(time) => (first, time),
            // Calendar shapes use `-` or four digits, time shapes `:` or two: no overlap.
            None if shape_of(first, &CALENDAR).is_some() => (first, &[][..]),
            None => (&[][..], first),
        };
        let mut raw = Raw::default();
        let mut variation = None;
        let mut fraction = None;
        if !calendar.is_empty() {
            let shape = shape_of(calendar, &CALENDAR).ok_or(malformed())?;
            if !time.is_empty() && !shape.ends_full {
                return Err(malformed());
            }
            variation = shape.variation;
            raw.read(calendar, shape)?;
        }
        if time.is_empty() {
            if utc {
                return Err(malformed());
            }
        } else {
            let mut pieces = time.splitn(2, |&b| b == b'.');
            let clock = pieces.next().unwrap_or_default();
            let digits = pieces.next();
            let shape = shape_of(clock, &TIME).ok_or(malformed())?;
            let bare_separators = shape.parts.is_empty() && digits.is_none();
            let misplaced_fraction = digits.is_some() && !shape.ends_full;
            let truncated_after_t = !calendar.is_empty() && !shape.starts_with_hour;
            if bare_separators || misplaced_fraction || truncated_after_t {
                return Err(malformed());
            }
            raw.read(clock, shape)?;
            if let Some(digits) = digits {
                fraction = Some(fraction_digits(digits, options.max_fraction_digits)?);
            }
        }
        AsciiSubset::checked(raw, variation, fraction, utc)
    }

    /// Range checks, using the widest case where context is missing (§5.1).
    fn checked(
        raw: Raw,
        variation: Option<CcsVariation>,
        fraction: Option<Fraction>,
        utc: bool,
    ) -> Result<AsciiSubset> {
        let year = raw.year.map(Year::try_from).transpose()?;
        let month = raw.month.map(|m| Month::try_from(small(m))).transpose()?;
        let day = match raw.day {
            Some(day) => {
                let last = match (year, month) {
                    (_, None) => 31,
                    // A leap year allows February 29.
                    (None, Some(m)) => days_in_month(2000, m.get()),
                    (Some(y), Some(m)) => days_in_month(y.get(), m.get()),
                };
                if day == 0 || day > u16::from(last) {
                    return Err(out_of_bounds("day", day.into(), 1, last.into()));
                }
                Some(small(day))
            }
            None => None,
        };
        let day_of_year = match raw.day_of_year {
            Some(doy) => {
                let last = if year.is_none_or(|y| is_leap(y.get())) {
                    366
                } else {
                    365
                };
                if doy == 0 || doy > last {
                    return Err(out_of_bounds("day_of_year", doy.into(), 1, last.into()));
                }
                Some(doy)
            }
            None => None,
        };
        Ok(AsciiSubset {
            variation,
            year,
            month,
            day,
            day_of_year,
            hour: raw.hour.map(|h| Hour::try_from(small(h))).transpose()?,
            minute: raw.minute.map(|m| Minute::try_from(small(m))).transpose()?,
            second: raw.second.map(|s| Second::try_from(small(s))).transpose()?,
            fraction,
            utc,
        })
    }

    /// Code A (month/day) or B (day of year), if the subset shows which (301.0 §3.5.1.3, TIME-032).
    #[must_use]
    pub const fn variation(&self) -> Option<CcsVariation> {
        self.variation
    }

    /// Year, if present (301.0 §3.5.1.3, TIME-032).
    #[must_use]
    pub const fn year(&self) -> Option<Year> {
        self.year
    }

    /// Month, if present (301.0 §3.5.1.3, TIME-032).
    #[must_use]
    pub const fn month(&self) -> Option<Month> {
        self.month
    }

    /// Day of month, if present (301.0 §3.5.1.3, TIME-032).
    #[must_use]
    pub const fn day(&self) -> Option<u8> {
        self.day
    }

    /// Day of year, if present (301.0 §3.5.1.3, TIME-032).
    #[must_use]
    pub const fn day_of_year(&self) -> Option<u16> {
        self.day_of_year
    }

    /// Hour, if present (301.0 §3.5.1.3, TIME-032).
    #[must_use]
    pub const fn hour(&self) -> Option<Hour> {
        self.hour
    }

    /// Minute, if present (301.0 §3.5.1.3, TIME-032).
    #[must_use]
    pub const fn minute(&self) -> Option<Minute> {
        self.minute
    }

    /// Second, if present (301.0 §3.5.1.3, TIME-032).
    #[must_use]
    pub const fn second(&self) -> Option<Second> {
        self.second
    }

    /// Fraction of the second, if present (301.0 §3.5.1.3, TIME-032).
    #[must_use]
    pub const fn fraction(&self) -> Option<Fraction> {
        self.fraction
    }

    /// Whether the code ends with `Z` (301.0 §3.5.1.3; TIME-032, OI-29).
    #[must_use]
    pub const fn utc(&self) -> bool {
        self.utc
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{
        AsciiFormat, AsciiOptions, AsciiSubset, AsciiTime, Fraction, MAX_ASCII_LEN, dec2, dec3,
        dec4, dec12, number, pow10, small,
    };
    use crate::calendar::{CalendarDate, Hour, Minute, Month, Second, Year};
    use crate::ccs::CcsVariation;
    use aldis_core::{ErrorDetail, ErrorKind, FieldId};
    use proptest::prelude::*;

    fn kind_field(r: Result<AsciiTime, aldis_core::Error>) -> (ErrorKind, Option<&'static str>) {
        let e = r.unwrap_err();
        (e.kind(), e.field().map(FieldId::name))
    }

    fn parse(text: &str) -> Result<AsciiTime, aldis_core::Error> {
        AsciiTime::parse(text.as_bytes(), &AsciiOptions::DEFAULT)
    }

    fn subset(text: &str) -> Result<AsciiSubset, aldis_core::Error> {
        AsciiSubset::parse(text.as_bytes(), &AsciiOptions::DEFAULT)
    }

    #[test]
    fn fractions_hold_only_valid_values() {
        // req: TIME-031, RS-020
        let f = Fraction::new(1234, 4).unwrap();
        assert_eq!(
            (f.digits(), f.value(), f.picos()),
            (4, 1234, 123_400_000_000)
        );
        assert_eq!(Fraction::new(10_000, 4), None);
        assert_eq!(Fraction::new(0, 13), None);
        assert_eq!(
            Fraction::new(999_999_999_999, 12).map(|f| f.picos()),
            Some(999_999_999_999)
        );
        assert_eq!(
            (Fraction::NONE, Fraction::default().picos()),
            (Fraction::new(0, 0).unwrap(), 0)
        );
        let format = |digits| AsciiFormat::new(CcsVariation::MonthDay, digits, false).unwrap();
        assert_eq!(
            Fraction::from_picos(123_456_789_012, format(3)),
            Fraction::new(123, 3).unwrap()
        );
        assert_eq!(Fraction::from_picos(5, format(0)), Fraction::NONE);
        assert_eq!(
            Fraction::from_picos(999_999_999_999, format(12)),
            Fraction::new(999_999_999_999, 12).unwrap()
        );
        for n in 0..=12_u8 {
            assert_eq!(pow10(n), 10_u64.pow(u32::from(n)));
        }
    }

    #[test]
    fn options_and_formats() {
        // req: TIME-031
        assert_eq!(
            AsciiOptions::new(13).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
        assert_eq!(AsciiOptions::new(3).unwrap().max_fraction_digits(), 3);
        assert_eq!(AsciiOptions::default(), AsciiOptions::DEFAULT);
        assert_eq!(AsciiOptions::DEFAULT.max_fraction_digits(), 12);
        let e = AsciiFormat::new(CcsVariation::MonthDay, 13, false).unwrap_err();
        assert_eq!(
            (e.kind(), e.field().map(FieldId::name)),
            (ErrorKind::OutOfRange, Some("fraction_digits"))
        );
        let f = AsciiFormat::new(CcsVariation::DayOfYear, 6, true).unwrap();
        assert_eq!(
            (f.variation(), f.fraction_digits(), f.utc()),
            (CcsVariation::DayOfYear, 6, true)
        );
    }

    #[test]
    fn helpers() {
        // req: TIME-031
        assert_eq!(number(b"0123", "x"), Ok(123));
        assert_eq!(
            number(b"1a", "x").unwrap_err().kind(),
            ErrorKind::InvalidField
        );
        assert_eq!((small(255), small(300)), (255, 255));
        assert_eq!(
            (dec2(7), dec3(42), dec4(9), dec12(1)),
            (*b"07", *b"042", *b"0009", *b"000000000001")
        );
    }

    #[test]
    fn complete_codes_parse() {
        // req: TIME-031
        let a = parse("1988-01-18T17:20:43.123456Z").unwrap();
        assert_eq!(a.date.month_day().map(|(m, d)| (m.get(), d)), Some((1, 18)));
        assert_eq!((a.hour.get(), a.minute.get(), a.second.get()), (17, 20, 43));
        assert_eq!(
            (a.fraction, a.utc),
            (Fraction::new(123_456, 6).unwrap(), true)
        );
        let b = parse("1988-018T17:20:43").unwrap();
        assert_eq!(
            (b.date.day_of_year(), b.fraction, b.utc),
            (Some(18), Fraction::NONE, false)
        );
        assert_eq!(parse("2016-12-31T23:59:60Z").unwrap().second.get(), 60);
        let three = AsciiOptions::new(3).unwrap();
        assert!(AsciiTime::parse(b"2024-01-01T00:00:00.123", &three).is_ok());
        let e = AsciiTime::parse(b"2024-01-01T00:00:00.1234", &three).unwrap_err();
        assert_eq!(
            (e.field().map(FieldId::name), e.detail()),
            (
                Some("fraction"),
                ErrorDetail::Expected {
                    expected: 3,
                    found: 4
                }
            )
        );
    }

    #[test]
    fn complete_code_errors() {
        // req: TIME-031, GEN-041
        for text in [
            "",
            "2024",
            "2024-01-01T00:00",
            "2024-01-01 00:00:00",
            "2024-01-01T00-00:00",
            "2024-01-01T00:00:00 ",
            "2024-01-01T00:00:00.",
            "2024-01-01T00:00:00.1Z2",
            "2024-01-01T00:00:00Z0",
            "2024-01-01t00:00:00",
            "2024-001 00:00:00",
            "2024-001T00:00",
            "2024-01-01T00:00:00z",
            "2024-01-01T00:00:00x",
        ] {
            assert_eq!(
                parse(text).unwrap_err().kind(),
                ErrorKind::Malformed,
                "{text:?}"
            );
        }
        for (text, field) in [
            ("2O24-01-01T00:00:00", "year"),
            ("2O24-001T00:00:00", "year"),
            ("2024-0x-01T00:00:00", "month"),
            ("2024-01-0xT00:00:00", "day"),
            ("2024-0x1T00:00:00", "day_of_year"),
            ("2024-01-01T0x:00:00", "hour"),
            ("2024-01-01T00:0x:00", "minute"),
            ("2024-01-01T00:00:0x", "second"),
            ("2024-01-01T00:00:00.1x", "fraction"),
        ] {
            assert_eq!(
                kind_field(parse(text)),
                (ErrorKind::InvalidField, Some(field)),
                "{text}"
            );
        }
        for (text, field) in [
            ("0000-01-01T00:00:00", "year"),
            ("2024-13-01T00:00:00", "month"),
            ("2023-02-29T00:00:00", "day"),
            ("2023-366T00:00:00", "day_of_year"),
            ("2024-01-01T24:00:00", "hour"),
            ("2024-01-01T00:60:00", "minute"),
            ("2024-01-01T00:00:61", "second"),
            ("2024-01-01T00:00:00.1234567890123", "fraction"),
        ] {
            assert_eq!(
                kind_field(parse(text)),
                (ErrorKind::OutOfRange, Some(field)),
                "{text}"
            );
        }
    }

    #[test]
    fn encoding() {
        // req: TIME-031, API-022
        for text in [
            "1988-01-18T17:20:43.123456Z",
            "1988-018T17:20:43",
            "2000-01-01T00:00:00.000001",
            "2016-12-31T23:59:60.999999999999Z",
            "0001-001T00:00:00.5",
        ] {
            let t = parse(text).unwrap();
            let mut out = [0_u8; MAX_ASCII_LEN];
            let n = t.encode(&mut out).unwrap();
            assert_eq!((&out[..n], t.encoded_len()), (text.as_bytes(), text.len()));
        }
        let longest = parse("2016-12-31T23:59:60.999999999999Z").unwrap();
        assert_eq!(longest.encoded_len(), MAX_ASCII_LEN);
        let mut short = [0xEE_u8; 10];
        let e = longest.encode(&mut short).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::BufferTooSmall, ErrorDetail::Needed(33))
        );
        assert_eq!(short, [0xEE; 10]);
        let built = AsciiTime {
            date: CalendarDate::from_month_day(
                Year::new(2024).unwrap(),
                Month::new(2).unwrap(),
                29,
            )
            .unwrap(),
            hour: Hour::new(1).unwrap(),
            minute: Minute::new(2).unwrap(),
            second: Second::new(3).unwrap(),
            fraction: Fraction::new(5, 2).unwrap(),
            utc: false,
        };
        let mut out = [0_u8; MAX_ASCII_LEN];
        let n = built.encode(&mut out).unwrap();
        assert_eq!(&out[..n], b"2024-02-29T01:02:03.05");
    }

    #[test]
    fn subsets_parse_every_shape() {
        // req: TIME-032
        let s = subset("1988-01-18").unwrap();
        assert_eq!(
            (
                s.variation(),
                s.year().map(Year::get),
                s.month().map(Month::get),
                s.day()
            ),
            (Some(CcsVariation::MonthDay), Some(1988), Some(1), Some(18))
        );
        assert_eq!(
            (
                s.hour(),
                s.minute(),
                s.second(),
                s.fraction(),
                s.utc(),
                s.day_of_year()
            ),
            (None, None, None, None, false, None)
        );
        assert_eq!(subset("1988").unwrap().variation(), None);
        assert_eq!(subset("1988-01").unwrap().day(), None);
        assert_eq!(subset("-01-18").unwrap().year(), None);
        assert_eq!(subset("--18").unwrap().day(), Some(18));
        assert_eq!(subset("-01").unwrap().month().map(Month::get), Some(1));
        let b = subset("1988-018").unwrap();
        assert_eq!(
            (b.variation(), b.day_of_year()),
            (Some(CcsVariation::DayOfYear), Some(18))
        );
        assert_eq!(subset("-018").unwrap().day_of_year(), Some(18));
        let t = subset("17:20:43.12").unwrap();
        assert_eq!(
            (
                t.hour().map(Hour::get),
                t.minute().map(Minute::get),
                t.second().map(Second::get)
            ),
            (Some(17), Some(20), Some(43))
        );
        assert_eq!(t.fraction(), Fraction::new(12, 2));
        assert_eq!(subset("17:20").unwrap().second(), None);
        assert_eq!(subset("17").unwrap().minute(), None);
        assert_eq!(subset(":20:43").unwrap().hour(), None);
        assert_eq!(subset("::43.5").unwrap().fraction(), Fraction::new(5, 1));
        assert_eq!(subset("::.25").unwrap().second(), None);
        assert_eq!(
            subset("1988-01-18T17").unwrap().hour().map(Hour::get),
            Some(17)
        );
        let z = subset("-01-18T17:20Z").unwrap();
        assert!(z.utc() && z.year().is_none());
        assert!(subset("17:20Z").unwrap().utc());
        assert_eq!(
            subset("1988-01-18T17:20:43.123456Z").unwrap().fraction(),
            Fraction::new(123_456, 6)
        );
    }

    #[test]
    fn subset_errors() {
        // req: TIME-032, GEN-041
        for text in [
            "",
            "Z",
            "1988-01-18Z",
            "1988-01T17",
            "1988-01-18T:20",
            "::",
            "17:.5",
            "T17",
            "1988-01-18T",
            "1988-01-18T17T20",
            "1988-1-18",
            "17:2",
            "17:20:43.",
            "17:20:43.1Z2",
            "1988-01-18T17:20.5",
        ] {
            assert_eq!(
                subset(text).unwrap_err().kind(),
                ErrorKind::Malformed,
                "{text:?}"
            );
        }
        for (text, field) in [
            ("19x8", "year"),
            ("-0x", "month"),
            ("--1x", "day"),
            ("-0x8", "day_of_year"),
            ("1x", "hour"),
            (":2x", "minute"),
            ("::4x", "second"),
            ("::.1x", "fraction"),
        ] {
            let e = subset(text).unwrap_err();
            assert_eq!(
                (e.kind(), e.field().map(FieldId::name)),
                (ErrorKind::InvalidField, Some(field)),
                "{text}"
            );
        }
        for (text, field, expected, found) in [
            ("0000", "year", 1, 0),
            ("-13", "month", 12, 13),
            ("--32", "day", 31, 32),
            ("--00", "day", 1, 0),
            ("-02-30", "day", 29, 30),
            ("2023-02-29", "day", 28, 29),
            ("-367", "day_of_year", 366, 367),
            ("2023-366", "day_of_year", 365, 366),
            ("-000", "day_of_year", 1, 0),
            ("24", "hour", 23, 24),
            (":60", "minute", 59, 60),
            ("::61", "second", 60, 61),
        ] {
            let e = subset(text).unwrap_err();
            assert_eq!(
                (e.kind(), e.field().map(FieldId::name)),
                (ErrorKind::OutOfRange, Some(field)),
                "{text}"
            );
            assert_eq!(
                e.detail(),
                ErrorDetail::Expected { expected, found },
                "{text}"
            );
        }
        assert!(subset("-02-29").is_ok() && subset("--31").is_ok() && subset("-366").is_ok());
        assert!(subset("2024-366").is_ok() && subset("2024-02-29").is_ok());
        let e = AsciiSubset::parse(b"::00.1234", &AsciiOptions::new(3).unwrap()).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::OutOfRange);
    }

    proptest! {
        #[test]
        fn arbitrary_text_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..40)) {
            // req: GEN-020, TIME-031, TIME-032
            if let Ok(t) = AsciiTime::parse(&bytes, &AsciiOptions::DEFAULT) {
                let mut out = [0_u8; MAX_ASCII_LEN];
                let n = t.encode(&mut out).unwrap();
                prop_assert_eq!(&out[..n], bytes.as_slice());
                prop_assert!(AsciiSubset::parse(&bytes, &AsciiOptions::DEFAULT).is_ok());
            }
            let _ = AsciiSubset::parse(&bytes, &AsciiOptions::DEFAULT);
        }
    }
}
