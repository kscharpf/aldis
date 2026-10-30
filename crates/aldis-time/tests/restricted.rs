//! A test for every restricted type of `aldis-time` (RS-020, VER-023; M2 plan WP-7): every type
//! with a checked constructor accepts exactly the range that CCSDS 301.0-B-4 gives it, rejects
//! every other value with the documented error (GEN-041), and every accepted value survives its
//! wire form (a binary time code or an ASCII code) unchanged.
//!
//! The ranges are written from the clause each test cites; where the input domain is small (a
//! `u8`, a `u16`, a flag) the test walks all of it, otherwise it checks the boundaries and draws
//! the rest from the shared strategies of `aldis-testkit` (VER-054). Each test names its type
//! with a `// restricted:` marker, which `cargo xtask restricted` checks. They are an
//! integration test so that the strategies and the tests see the same copy of the crate (M2 plan
//! D6).
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test helpers outside #[test] fail loudly; test models on bounded values (RS-018)"
)]

use std::fmt::Debug;
use std::ops::RangeInclusive;

use aldis_core::{Error, ErrorDetail, ErrorKind, FieldId};
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::time::{
    ascii_time, calendar_date, ccs, ccs_format, ccs_subsecond_out_of_range, ccs_value,
    ccs_variation, cds, cds_day_out_of_range, cds_format, cds_value, cuc, cuc_coarse_out_of_range,
    cuc_epoch, cuc_fine_off_resolution, cuc_format, cuc_value, fraction, fraction_out_of_range,
    hour, minute, ms_of_day, ms_of_day_out_of_range, nanos, picos_out_of_range, second, year,
};
use aldis_testkit::strategies::{self};
use aldis_time::{
    Agency, AgencyFormat, AsciiFormat, AsciiOptions, AsciiTime, CalendarDate, Ccs, CcsFormat,
    CcsValue, CcsVariation, Cds, CdsDayLen, CdsEpoch, CdsFormat, CdsSubMs, CdsValue, Cuc, CucEpoch,
    CucFormat, CucValue, Fraction, Hour, MAX_ASCII_LEN, Micros, Minute, Month, MsOfDay, PField,
    PFieldTail, Picos, ReservedPField, Second, SubMillis, TimeCode, TimeFormat, UtcInstant, Year,
    decode_implicit, decode_with_pfield, decode_with_pfield_lenient, field,
};

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/restricted.txt"
);

/// Both calendar variations (301.0 §3.4.2 bit 4).
const VARIATIONS: [CcsVariation; 2] = [CcsVariation::MonthDay, CcsVariation::DayOfYear];

/// An error as its kind, field, and detail (GEN-041).
const fn parts(e: &Error) -> (ErrorKind, Option<FieldId>, ErrorDetail) {
    (e.kind(), e.field(), e.detail())
}

/// `OutOfRange` naming `field`, with the violated bound and the value found (GEN-041).
const fn out_of_range(
    field: FieldId,
    expected: u64,
    found: u64,
) -> (ErrorKind, Option<FieldId>, ErrorDetail) {
    (
        ErrorKind::OutOfRange,
        Some(field),
        ErrorDetail::Expected { expected, found },
    )
}

/// An error of `kind` naming `field`, without detail.
const fn plain(kind: ErrorKind, field: FieldId) -> (ErrorKind, Option<FieldId>, ErrorDetail) {
    (kind, Some(field), ErrorDetail::None)
}

/// The bound of `spec` that `v` violates: its start if `v` is below it, else its end (GEN-041).
fn violated<R: Copy + PartialOrd>(spec: &RangeInclusive<R>, v: R) -> R {
    if v < *spec.start() {
        *spec.start()
    } else {
        *spec.end()
    }
}

/// A range-checked field type over `domain`: `new` accepts exactly the values of `spec` and
/// `get` gives them back; `TryFrom` agrees, and rejects every other value with `OutOfRange`
/// naming `field`, the violated bound, and the value (RS-020, GEN-041).
fn check_bounded<R, T>(
    domain: impl IntoIterator<Item = R>,
    spec: &RangeInclusive<R>,
    field: FieldId,
    new: impl Fn(R) -> Option<T>,
    get: impl Fn(T) -> R,
) where
    R: Copy + PartialOrd + Into<u64> + Debug,
    T: Copy + Debug + TryFrom<R, Error = Error>,
{
    for v in domain {
        let accepted = spec.contains(&v);
        assert_eq!(new(v).map(&get), accepted.then_some(v), "new({v:?})");
        match T::try_from(v) {
            Ok(t) => assert!(accepted && get(t) == v, "try_from({v:?})"),
            Err(e) => {
                assert!(!accepted, "try_from({v:?}) rejected a valid value");
                assert_eq!(
                    parts(&e),
                    out_of_range(field, violated(spec, v).into(), v.into()),
                    "try_from({v:?})"
                );
            }
        }
    }
}

/// `code` encoded with its P-field and decoded strictly, and encoded without it and decoded
/// with its format: both decodes consume the whole encoding (301.0 §3.1; TIME-001, TIME-003).
fn binary_round_trip(code: TimeCode) -> (TimeCode, TimeCode) {
    let mut out = [0_u8; 32];
    let n = code.encode(true, &mut out).unwrap();
    let explicit = decode_with_pfield(&out[..n]).unwrap();
    assert_eq!(explicit.len(), n);
    let format = code.format().unwrap();
    let n = code.encode(false, &mut out).unwrap();
    let implicit = decode_implicit(&out[..n], &format).unwrap();
    assert_eq!(implicit.len(), n);
    (explicit.code(), implicit.code())
}

/// Whether `code` decodes back to itself, with and without its P-field.
fn binary_identity(code: TimeCode) -> bool {
    binary_round_trip(code) == (code, code)
}

/// `t` written as ASCII text and parsed back (301.0 §3.5.1, TIME-031).
fn ascii_round_trip(t: AsciiTime) -> AsciiTime {
    let mut out = [0_u8; MAX_ASCII_LEN];
    let n = t.encode(&mut out).unwrap();
    AsciiTime::parse(&out[..n], &AsciiOptions::DEFAULT).unwrap()
}

/// 2000-01-01 as month and day, or as day of year 1.
fn new_year(variation: CcsVariation) -> CalendarDate {
    let y = Year::new(2000).unwrap();
    match variation {
        CcsVariation::MonthDay => CalendarDate::from_month_day(y, Month::new(1).unwrap(), 1),
        CcsVariation::DayOfYear => CalendarDate::from_day_of_year(y, 1),
    }
    .unwrap()
}

/// The CCS value 2000-01-01 00:00:00 (month/day variation) with `f` applied.
fn ccs_value_with(f: impl FnOnce(&mut CcsValue)) -> CcsValue {
    let mut v = CcsValue {
        date: new_year(CcsVariation::MonthDay),
        hour: Hour::new(0).unwrap(),
        minute: Minute::new(0).unwrap(),
        second: Second::new(0).unwrap(),
        subsecond: 0,
    };
    f(&mut v);
    v
}

/// A CCS code of `v` in the format of its date's variation without sub-second octets.
fn ccs_code(v: CcsValue) -> TimeCode {
    TimeCode::Ccs(Ccs::new(CcsFormat::new(v.date.variation(), 0).unwrap(), v).unwrap())
}

/// The ASCII code of a CCS value without fraction, ending with `Z`.
const fn ascii_of(v: CcsValue) -> AsciiTime {
    AsciiTime {
        date: v.date,
        hour: v.hour,
        minute: v.minute,
        second: v.second,
        fraction: Fraction::NONE,
        utc: true,
    }
}

/// Two BCD digits of `n` (0–99) (301.0 §3.4.1).
const fn bcd(n: u8) -> u8 {
    ((n / 10) << 4) | (n % 10)
}

/// The implicit CCS T-field of 2000-01-01 00:00:00 (month/day variation, no sub-second octets)
/// with BCD segment `index` set to `n`, decoded strictly (301.0 §3.4.1.1, TIME-030).
fn ccs_with_segment(index: usize, n: u8) -> Result<TimeCode, Error> {
    let mut t = [bcd(20), bcd(0), bcd(1), bcd(1), bcd(0), bcd(0), bcd(0)];
    t[index] = bcd(n);
    let format = TimeFormat::Ccs(CcsFormat::new(CcsVariation::MonthDay, 0).unwrap());
    decode_implicit(&t, &format).map(|d| d.code())
}

/// A calendar field type on the wire: every two-digit BCD value of CCS segment `index` decodes
/// if and only if `spec` contains it, and is otherwise rejected with the same error as the
/// type's `TryFrom` (301.0 §3.4.1, Annex A; TIME-030).
fn check_ccs_segment(index: usize, spec: &RangeInclusive<u8>, field: FieldId) {
    for n in 0..=99_u8 {
        match ccs_with_segment(index, n) {
            Ok(_) => assert!(spec.contains(&n), "segment {index} = {n}"),
            Err(e) => {
                assert!(!spec.contains(&n), "segment {index} = {n}: {e}");
                assert_eq!(
                    parts(&e),
                    out_of_range(field, violated(spec, n).into(), n.into())
                );
            }
        }
    }
}

// ----- Calendar fields (301.0 §3.4, §3.5, Annex A) -----

/// Year A.D. 1–9999 (301.0 Annex A; the ASCII YYYY subfield, §3.5.1.1–2): every `u16`, every
/// accepted year through a CCS code and an ASCII code, and year 0000 rejected on the wire.
#[test]
fn year_is_exactly_1_to_9999() {
    // restricted: Year
    // req: TIME-030, TIME-031, GEN-041, API-001, VER-023
    let spec = 1..=9999_u16;
    assert_eq!((Year::MIN, Year::MAX), (*spec.start(), *spec.end()));
    check_bounded(0..=u16::MAX, &spec, field::YEAR, Year::new, Year::get);
    for y in spec {
        let year = Year::new(y).unwrap();
        let v = ccs_value_with(|v| {
            v.date = CalendarDate::from_month_day(year, Month::new(1).unwrap(), 1).unwrap();
        });
        assert!(binary_identity(ccs_code(v)), "year {y}");
        assert_eq!(ascii_round_trip(ascii_of(v)).date.year(), year);
    }
    // Century and year segments 00 00: the one BCD year outside the range.
    let mut t = [0, 0, bcd(1), bcd(1), 0, 0, 0];
    let format = TimeFormat::Ccs(CcsFormat::new(CcsVariation::MonthDay, 0).unwrap());
    let e = decode_implicit(&t, &format).unwrap_err();
    assert_eq!(parts(&e), out_of_range(field::YEAR, 1, 0));
    t[1] = bcd(1);
    assert!(decode_implicit(&t, &format).is_ok());
}

/// Month of year 1–12 (301.0 Annex A; ASCII MM, §3.5.1.1): every `u8`, every month through a
/// CCS and an ASCII code, and every BCD month segment.
#[test]
fn month_is_exactly_1_to_12() {
    // restricted: Month
    // req: TIME-030, TIME-031, GEN-041, API-001, VER-023
    let spec = 1..=12_u8;
    assert_eq!((Month::MIN, Month::MAX), (*spec.start(), *spec.end()));
    check_bounded(0..=u8::MAX, &spec, field::MONTH, Month::new, Month::get);
    for m in spec.clone() {
        let month = Month::new(m).unwrap();
        let v = ccs_value_with(|v| {
            v.date = CalendarDate::from_month_day(Year::new(2000).unwrap(), month, 1).unwrap();
        });
        assert!(binary_identity(ccs_code(v)));
        assert_eq!(
            ascii_round_trip(ascii_of(v)).date.month_day(),
            Some((month, 1))
        );
    }
    check_ccs_segment(2, &spec, field::MONTH);
}

/// Hour of day 0–23 (301.0 Annex A; ASCII hh, §3.5.1.1): every `u8`, every hour through a CCS
/// and an ASCII code, and every BCD hour segment.
#[test]
fn hour_is_exactly_0_to_23() {
    // restricted: Hour
    // req: TIME-030, TIME-031, GEN-041, API-001, VER-023
    let spec = 0..=23_u8;
    assert_eq!((Hour::MIN, Hour::MAX), (*spec.start(), *spec.end()));
    check_bounded(0..=u8::MAX, &spec, field::HOUR, Hour::new, Hour::get);
    for h in spec.clone() {
        let v = ccs_value_with(|v| v.hour = Hour::new(h).unwrap());
        assert!(binary_identity(ccs_code(v)));
        assert_eq!(ascii_round_trip(ascii_of(v)).hour, v.hour);
    }
    check_ccs_segment(4, &spec, field::HOUR);
}

/// Minute of hour 0–59 (301.0 Annex A; ASCII mm, §3.5.1.1): every `u8`, every minute through a
/// CCS and an ASCII code, and every BCD minute segment.
#[test]
fn minute_is_exactly_0_to_59() {
    // restricted: Minute
    // req: TIME-030, TIME-031, GEN-041, API-001, VER-023
    let spec = 0..=59_u8;
    assert_eq!((Minute::MIN, Minute::MAX), (*spec.start(), *spec.end()));
    check_bounded(0..=u8::MAX, &spec, field::MINUTE, Minute::new, Minute::get);
    for m in spec.clone() {
        let v = ccs_value_with(|v| v.minute = Minute::new(m).unwrap());
        assert!(binary_identity(ccs_code(v)));
        assert_eq!(ascii_round_trip(ascii_of(v)).minute, v.minute);
    }
    check_ccs_segment(5, &spec, field::MINUTE);
}

/// Second of minute 0–60: 0–59, and 60 when a positive leap second is introduced (301.0 Annex
/// A; ASCII ss, §3.5.1.1). Every `u8`, every second through a CCS and an ASCII code, and every
/// BCD second segment; 60 is accepted at any minute, leap seconds being the caller's (TIME-022).
#[test]
fn second_is_exactly_0_to_60() {
    // restricted: Second
    // req: TIME-030, TIME-031, TIME-022, GEN-041, API-001, VER-023
    let spec = 0..=60_u8;
    assert_eq!((Second::MIN, Second::MAX), (*spec.start(), *spec.end()));
    check_bounded(0..=u8::MAX, &spec, field::SECOND, Second::new, Second::get);
    for s in spec.clone() {
        let v = ccs_value_with(|v| v.second = Second::new(s).unwrap());
        assert!(binary_identity(ccs_code(v)));
        assert_eq!(ascii_round_trip(ascii_of(v)).second, v.second);
    }
    check_ccs_segment(6, &spec, field::SECOND);
}

/// Whether `year` is a leap year: divisible by 4, except the years divisible by 100 and not by
/// 400 (301.0 Annex A, footnote).
const fn spec_leap(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

/// The last day of `month` in `year` (301.0 Annex A, Day-of-month).
const fn spec_last_day(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if spec_leap(year) => 29,
        _ => 28,
    }
}

/// A date exists exactly as 301.0 Annex A says: day of month 1–31, 1–30, or 1–28 (1–29 in leap
/// years) by month, day of year 1–365 (366 in leap years). Every year, month, and day of month
/// 0–32 and 255, and every day of year 0–367 and `u16::MAX`; each rejection is `OutOfRange`
/// naming the violated bound.
#[test]
fn calendar_dates_are_exactly_the_annex_a_dates() {
    // restricted: CalendarDate
    // req: TIME-030, GEN-041, VER-023
    for y in Year::MIN..=Year::MAX {
        let year = Year::new(y).unwrap();
        for m in 1..=12_u8 {
            let month = Month::new(m).unwrap();
            let days = 1..=spec_last_day(y, m);
            for d in (0..=32).chain([u8::MAX]) {
                match CalendarDate::from_month_day(year, month, d) {
                    Ok(date) => {
                        assert!(days.contains(&d), "{y}-{m}-{d}");
                        assert_eq!(date.month_day(), Some((month, d)));
                        assert_eq!((date.year(), date.day_of_year()), (year, None));
                        assert_eq!(date.variation(), CcsVariation::MonthDay);
                    }
                    Err(e) => {
                        assert!(!days.contains(&d), "{y}-{m}-{d}");
                        let bound = violated(&days, d);
                        assert_eq!(parts(&e), out_of_range(field::DAY, bound.into(), d.into()));
                    }
                }
            }
        }
        let days = 1..=if spec_leap(y) { 366_u16 } else { 365 };
        for d in (0..=367).chain([u16::MAX]) {
            match CalendarDate::from_day_of_year(year, d) {
                Ok(date) => {
                    assert!(days.contains(&d), "{y}-{d}");
                    assert_eq!((date.day_of_year(), date.month_day()), (Some(d), None));
                    assert_eq!(date.variation(), CcsVariation::DayOfYear);
                }
                Err(e) => {
                    assert!(!days.contains(&d), "{y}-{d}");
                    let bound = violated(&days, d);
                    assert_eq!(
                        parts(&e),
                        out_of_range(field::DAY_OF_YEAR, bound.into(), d.into())
                    );
                }
            }
        }
    }
}

/// Every date of a common, a leap, a century, and a 400-year year, and the first and last
/// year, in both variations, survives a CCS code and an ASCII code (301.0 §3.4.1, §3.5.1).
#[test]
fn calendar_dates_survive_their_wire_forms() {
    // restricted: CalendarDate
    // req: TIME-030, TIME-031, VER-023
    for y in [1, 1900, 2000, 2023, 2024, 9999] {
        let year = Year::new(y).unwrap();
        let mut dates = Vec::new();
        for m in 1..=12 {
            for d in 1..=spec_last_day(y, m) {
                let month = Month::new(m).unwrap();
                dates.push(CalendarDate::from_month_day(year, month, d).unwrap());
            }
        }
        let last = if spec_leap(y) { 366 } else { 365 };
        dates.extend((1..=last).map(|d| CalendarDate::from_day_of_year(year, d).unwrap()));
        for date in dates {
            let v = ccs_value_with(|v| v.date = date);
            assert!(binary_identity(ccs_code(v)), "{date:?}");
            assert_eq!(ascii_round_trip(ascii_of(v)).date, date);
        }
    }
}

// ----- CDS segments (301.0 §3.3, Annex A) -----

/// The implicit CDS T-field of day 0 in `format` with the milliseconds-of-day segment `ms` and
/// the submillisecond segment `sub` (as many low octets as the resolution has), decoded
/// strictly (301.0 §3.3.1).
fn cds_wire(format: CdsFormat, ms: u32, sub: u32) -> Result<CdsValue, Error> {
    let mut t = vec![0_u8; 2];
    t.extend(ms.to_be_bytes());
    let sub = sub.to_be_bytes();
    t.extend(match format.subms() {
        CdsSubMs::None => &sub[4..],
        CdsSubMs::Micros => &sub[2..],
        CdsSubMs::Picos => &sub[..],
    });
    match decode_implicit(&t, &TimeFormat::Cds(format))?.code() {
        TimeCode::Cds(c) => Ok(c.value()),
        _ => Err(Error::new(ErrorKind::Malformed)),
    }
}

/// A CDS format of day 16 bits and resolution `subms`.
const fn cds16(subms: CdsSubMs) -> CdsFormat {
    CdsFormat::new(CdsEpoch::Ccsds1958, CdsDayLen::Bits16, subms)
}

/// `MsOfDay` and the wire segment at `ms`: accepted and decoded as itself if and only if `ms` is
/// in `spec`, else `OutOfRange` naming the bound.
fn check_ms_of_day(ms: u32, spec: &RangeInclusive<u32>) {
    check_bounded([ms], spec, field::MS_OF_DAY, MsOfDay::new, MsOfDay::get);
    match cds_wire(cds16(CdsSubMs::None), ms, 0) {
        Ok(v) => assert_eq!(Some(v.ms_of_day), MsOfDay::new(ms)),
        Err(e) => assert_eq!(parts(&e), parts(&MsOfDay::try_from(ms).unwrap_err())),
    }
}

/// `Picos` and the wire segment at `ps`, as [`check_ms_of_day`].
fn check_picos(ps: u32, spec: &RangeInclusive<u32>) {
    check_bounded([ps], spec, field::SUBMS, Picos::new, Picos::get);
    match cds_wire(cds16(CdsSubMs::Picos), 0, ps) {
        Ok(v) => assert_eq!(Some(v.subms), Picos::new(ps).map(SubMillis::Picos)),
        Err(e) => assert_eq!(parts(&e), parts(&Picos::try_from(ps).unwrap_err())),
    }
}

/// Milliseconds of day 0 to 86 400 999: 0 to 86 399 999, and to 86 400 999 when a positive leap
/// second is introduced (301.0 Annex A; TIME-021). The boundaries and `u32::MAX`, here and on
/// the wire; the property tests below draw the rest.
#[test]
fn ms_of_day_is_exactly_0_to_86_400_999() {
    // restricted: MsOfDay
    // req: TIME-021, TIME-022, GEN-041, API-001, VER-023
    let spec = 0..=86_400_999_u32;
    assert_eq!((MsOfDay::MIN, MsOfDay::MAX), (*spec.start(), *spec.end()));
    for ms in [
        0,
        1,
        86_399_999,
        86_400_000,
        86_400_999,
        86_401_000,
        u32::MAX,
    ] {
        check_ms_of_day(ms, &spec);
    }
}

/// Microseconds of millisecond 0–999 (301.0 Annex A; TIME-021): every `u16`, as a value and as
/// the 16-bit submillisecond segment of a CDS code (§3.3.2 bits 6–7 `01`).
#[test]
fn micros_are_exactly_0_to_999() {
    // restricted: Micros
    // req: TIME-021, GEN-041, API-001, VER-023
    let spec = 0..=999_u16;
    assert_eq!((Micros::MIN, Micros::MAX), (*spec.start(), *spec.end()));
    check_bounded(0..=u16::MAX, &spec, field::SUBMS, Micros::new, Micros::get);
    for us in 0..=u16::MAX {
        match cds_wire(cds16(CdsSubMs::Micros), 0, us.into()) {
            Ok(v) => assert_eq!(Some(v.subms), Micros::new(us).map(SubMillis::Micros)),
            Err(e) => assert_eq!(parts(&e), parts(&Micros::try_from(us).unwrap_err())),
        }
    }
}

/// Picoseconds of millisecond 0 to 999 999 999: the 32-bit picosecond segment (301.0 §3.3.2
/// bits 6–7 `10`) counts the 10⁹ picoseconds of a millisecond (TIME-021). The boundaries and
/// `u32::MAX`, here and on the wire; the property tests below draw the rest.
#[test]
fn picos_are_exactly_0_to_999_999_999() {
    // restricted: Picos
    // req: TIME-021, GEN-041, API-001, VER-023
    let spec = 0..=999_999_999_u32;
    assert_eq!((Picos::MIN, Picos::MAX), (*spec.start(), *spec.end()));
    for ps in [0, 1, 999_999_998, 999_999_999, 1_000_000_000, u32::MAX] {
        check_picos(ps, &spec);
    }
}

/// The largest day of a `day`-bit day segment: 2¹⁶ − 1 or 2²⁴ − 1 (301.0 §3.3.1, Annex A Day).
const fn spec_max_day(day: CdsDayLen) -> u32 {
    match day {
        CdsDayLen::Bits16 => (1 << 16) - 1,
        CdsDayLen::Bits24 => (1 << 24) - 1,
    }
}

/// A CDS value fits a format exactly when its day fits the 16- or 24-bit day segment (301.0
/// §3.3.1, Annex A Day 0 to 2¹⁶ − 1 or 2²⁴ − 1) and its submillisecond segment has the
/// format's resolution (§3.3.2 bits 6–7). Every day length and resolution pair, at the day
/// boundaries; the property tests below draw the rest.
#[test]
fn cds_values_fit_exactly_their_format() {
    // restricted: Cds
    // req: TIME-020, TIME-021, GEN-041, VER-023
    let subs = [
        SubMillis::None,
        SubMillis::Micros(Micros::new(999).unwrap()),
        SubMillis::Picos(Picos::new(999_999_999).unwrap()),
    ];
    let resolutions = [CdsSubMs::None, CdsSubMs::Micros, CdsSubMs::Picos];
    for epoch in [CdsEpoch::Ccsds1958, CdsEpoch::Agency] {
        for day_len in [CdsDayLen::Bits16, CdsDayLen::Bits24] {
            let max = spec_max_day(day_len);
            for resolution in resolutions {
                let format = CdsFormat::new(epoch, day_len, resolution);
                for subms in subs {
                    for day in [0, max, max + 1, u32::MAX] {
                        let ms_of_day = MsOfDay::new(MsOfDay::MAX).unwrap();
                        let value = CdsValue {
                            day,
                            ms_of_day,
                            subms,
                        };
                        let got = Cds::new(format, value);
                        if day > max {
                            let e = got.unwrap_err();
                            assert_eq!(
                                parts(&e),
                                out_of_range(field::CDS_DAY, max.into(), day.into())
                            );
                        } else if subms.resolution() == resolution {
                            let c = got.unwrap();
                            assert_eq!((c.format(), c.value()), (format, value));
                            assert!(binary_identity(TimeCode::Cds(c)));
                        } else {
                            let e = got.unwrap_err();
                            assert_eq!(parts(&e), plain(ErrorKind::InvalidField, field::SUBMS));
                        }
                    }
                }
            }
        }
    }
}

// ----- CUC (301.0 §3.2) -----

/// The CUC octet counts of 301.0 §3.2.2: octet 1 bits 4–5 give 1–4 coarse octets and octet 2
/// bits 1–2 add 0–3, so 1–7; octet 1 bits 6–7 give 0–3 fine octets and octet 2 bits 3–5 add
/// 0–7, so 0–10 (TIME-010).
const CUC_COARSE: RangeInclusive<u8> = 1..=7;
/// See [`CUC_COARSE`].
const CUC_FINE: RangeInclusive<u8> = 0..=10;

/// `CucFormat::new` accepts exactly 1–7 coarse and 0–10 fine octets (301.0 §3.2.2, TIME-010):
/// every epoch and every pair of `u8` counts; the coarse count is checked first. Every accepted
/// format describes itself in its P-field, using octet 2 exactly when the counts need it, and a
/// code in it decodes back to the same format.
#[test]
fn cuc_formats_are_exactly_1_to_7_coarse_and_0_to_10_fine_octets() {
    // restricted: CucFormat
    // req: TIME-010, TIME-001, GEN-041, VER-023
    assert_eq!(
        (CucFormat::MAX_COARSE, CucFormat::MAX_FINE),
        (*CUC_COARSE.end(), *CUC_FINE.end())
    );
    for epoch in [CucEpoch::Tai1958, CucEpoch::Agency] {
        for c in 0..=u8::MAX {
            for f in 0..=u8::MAX {
                match CucFormat::new(epoch, c, f) {
                    Ok(format) => {
                        assert!(CUC_COARSE.contains(&c) && CUC_FINE.contains(&f));
                        let counts = (format.coarse_octets(), format.fine_octets());
                        assert_eq!((format.epoch(), counts), (epoch, (c, f)));
                        assert_eq!(format.t_len(), usize::from(c + f));
                        assert_eq!(format.has_octet2(), c > 4 || f > 3);
                        let code = TimeCode::Cuc(Cuc::new(format, CucValue::default()).unwrap());
                        assert_eq!(code.pfield().len(), 1 + usize::from(format.has_octet2()));
                        assert!(binary_identity(code));
                    }
                    Err(e) if !CUC_COARSE.contains(&c) => {
                        let bound = violated(&CUC_COARSE, c);
                        assert_eq!(
                            parts(&e),
                            out_of_range(field::COARSE_OCTETS, bound.into(), c.into())
                        );
                    }
                    Err(e) => {
                        assert!(!CUC_FINE.contains(&f), "({c}, {f})");
                        let bound = (*CUC_FINE.end()).into();
                        assert_eq!(parts(&e), out_of_range(field::FINE_OCTETS, bound, f.into()));
                    }
                }
            }
        }
    }
}

/// A CUC value fits a format exactly when its coarse time fits the coarse octets, which it never
/// wraps (TIME-012), and its left-aligned fraction has no bit below the fine octets (301.0
/// §3.2.1; TIME-011). Every count, at the coarse boundaries and with the fraction bits just
/// inside and just outside the resolution; the property tests below draw the rest.
#[test]
fn cuc_values_fit_exactly_their_format() {
    // restricted: Cuc
    // req: TIME-011, TIME-012, GEN-041, VER-023
    for c in CUC_COARSE {
        for f in CUC_FINE {
            let format = CucFormat::new(CucEpoch::Tai1958, c, f).unwrap();
            let max = u64::MAX >> (64 - 8 * u32::from(c));
            let kept = 8 * u32::from(f);
            let mask = if kept == 0 {
                0
            } else {
                u128::MAX << (128 - kept)
            };
            for coarse in [0, max] {
                for fine in [0, mask, 1_u128 << (128 - kept.max(1))] {
                    let fine = fine & mask;
                    let code = Cuc::new(format, CucValue::new(coarse, fine)).unwrap();
                    assert_eq!(code.value(), CucValue::new(coarse, fine));
                    assert!(binary_identity(TimeCode::Cuc(code)));
                }
            }
            for coarse in [max + 1, u64::MAX] {
                let e = Cuc::new(format, CucValue::new(coarse, 0)).unwrap_err();
                assert_eq!(parts(&e), out_of_range(field::COARSE, max, coarse));
                let e = Cuc::from_seconds_nanos(format, coarse, 0).unwrap_err();
                assert_eq!(parts(&e), out_of_range(field::COARSE, max, coarse));
            }
            // The bit just below the resolution, and the lowest bit.
            for bit in [127 - kept, 0] {
                let e = Cuc::new(format, CucValue::new(0, mask | (1 << bit))).unwrap_err();
                assert_eq!(parts(&e), plain(ErrorKind::OutOfRange, field::FINE));
            }
            // From nanoseconds: 0 to 10⁹ − 1 of a second (TIME-011).
            assert!(Cuc::from_seconds_nanos(format, max, 999_999_999).is_ok());
            for nanos in [1_000_000_000, u32::MAX] {
                let e = Cuc::from_seconds_nanos(format, 0, nanos).unwrap_err();
                let found = u64::from(nanos);
                assert_eq!(
                    parts(&e),
                    out_of_range(field::NANOSECONDS, 999_999_999, found)
                );
            }
        }
    }
}

// ----- CCS (301.0 §3.4) -----

/// `CcsFormat::new` accepts exactly 0–6 sub-second octets: P-field bits 5–7 give resolutions
/// `000` (1 s) to `110` (10⁻¹² s), one octet of two digits each, and `111` is not used (301.0
/// §3.4.2; TIME-030). Every variation and every `u8`; every accepted format describes itself in
/// its P-field, and code `111` on the wire is `ReservedValue` in both modes.
#[test]
fn ccs_formats_are_exactly_0_to_6_subsecond_octets() {
    // restricted: CcsFormat
    // req: TIME-030, TIME-001, GEN-041, VER-023
    let spec = 0..=6_u8;
    assert_eq!(CcsFormat::MAX_SUBSECOND_OCTETS, *spec.end());
    for variation in VARIATIONS {
        for n in 0..=u8::MAX {
            match CcsFormat::new(variation, n) {
                Ok(format) => {
                    assert!(spec.contains(&n));
                    assert_eq!(
                        (format.variation(), format.subsecond_octets()),
                        (variation, n)
                    );
                    assert_eq!(format.t_len(), 7 + usize::from(n));
                    assert_eq!(format.pfield().first() & 0b111, n);
                    let v = ccs_value_with(|v| v.date = new_year(variation));
                    assert!(binary_identity(TimeCode::Ccs(Ccs::new(format, v).unwrap())));
                }
                Err(e) => {
                    assert!(!spec.contains(&n));
                    let bound = (*spec.end()).into();
                    assert_eq!(
                        parts(&e),
                        out_of_range(field::SUBSECOND_OCTETS, bound, n.into())
                    );
                }
            }
        }
        let format = CcsFormat::new(variation, 0).unwrap();
        let mut octets = [0_u8; 16];
        let code = TimeCode::Ccs(
            Ccs::new(format, ccs_value_with(|v| v.date = new_year(variation))).unwrap(),
        );
        let len = code.encode(true, &mut octets).unwrap();
        octets[0] |= 0b111;
        for e in [
            decode_with_pfield(&octets[..len]).unwrap_err(),
            decode_with_pfield_lenient(&octets[..len]).unwrap_err(),
        ] {
            assert_eq!(
                parts(&e),
                plain(ErrorKind::ReservedValue, field::SUBSECOND_OCTETS)
            );
        }
    }
}

/// A CCS value fits a format exactly when its date is in the format's calendar variation (301.0
/// §3.4.2 bit 4) and its sub-second value has at most the 2n digits of n sub-second octets
/// (§3.4.1, §3.4.2): every variation pair and octet count, at the sub-second boundary; the
/// property tests below draw the rest.
#[test]
fn ccs_values_fit_exactly_their_format() {
    // restricted: Ccs
    // req: TIME-030, GEN-041, VER-023
    for format_variation in VARIATIONS {
        for n in 0..=CcsFormat::MAX_SUBSECOND_OCTETS {
            let format = CcsFormat::new(format_variation, n).unwrap();
            let limit = 10_u64.pow(2 * u32::from(n));
            for date_variation in VARIATIONS {
                for subsecond in [0, limit - 1, limit, u64::MAX] {
                    let v = ccs_value_with(|v| {
                        v.date = new_year(date_variation);
                        v.subsecond = subsecond;
                    });
                    match Ccs::new(format, v) {
                        Ok(c) => {
                            assert!(date_variation == format_variation && subsecond < limit);
                            assert_eq!((c.format(), c.value()), (format, v));
                            assert!(binary_identity(TimeCode::Ccs(c)));
                        }
                        Err(e) if date_variation != format_variation => {
                            assert_eq!(parts(&e), plain(ErrorKind::InvalidField, field::DATE));
                        }
                        Err(e) => {
                            assert!(subsecond >= limit);
                            let expected = out_of_range(field::SUBSECOND, limit - 1, subsecond);
                            assert_eq!(parts(&e), expected);
                        }
                    }
                }
            }
        }
    }
}

// ----- Agency-defined codes and reserved P-fields (301.0 §3.1.1, §3.6) -----

/// `AgencyFormat::new` accepts exactly T-fields of 1–16 octets: P-field bits 4–7 hold the
/// length minus one, 0–15 (301.0 §3.6.2; TIME-006). Every `u8`; every accepted length round
/// trips through a code, and every value of bits 4–7 on the wire decodes to its length.
#[test]
fn agency_formats_are_exactly_1_to_16_octets() {
    // restricted: AgencyFormat
    // req: TIME-006, GEN-041, VER-023
    let spec = 1..=16_u8;
    for n in 0..=u8::MAX {
        match AgencyFormat::new(n) {
            Ok(format) => {
                assert!(spec.contains(&n));
                assert_eq!(format.t_len(), usize::from(n));
                assert_eq!(format.pfield().first() & 0x0F, n - 1);
                let t: Vec<u8> = (0..n).map(|i| i.wrapping_mul(37)).collect();
                let code = TimeCode::Agency(Agency::new(format, &t).unwrap());
                assert!(binary_identity(code));
            }
            Err(e) => {
                assert!(!spec.contains(&n));
                let bound = violated(&spec, n).into();
                assert_eq!(parts(&e), out_of_range(field::T_LEN, bound, n.into()));
            }
        }
    }
    for bits in 0..=0x0F_u8 {
        let mut octets = vec![0b0110_0000 | bits];
        octets.extend(std::iter::repeat_n(0xA5, usize::from(bits) + 1));
        let d = decode_with_pfield(&octets).unwrap();
        let TimeCode::Agency(a) = d.code() else {
            panic!("not an agency-defined code");
        };
        assert_eq!(
            (a.format().t_len(), d.len()),
            (usize::from(bits) + 1, octets.len())
        );
    }
}

/// `Agency::new` accepts exactly a T-field of the format's length, 1 to 16 octets (301.0
/// §3.6.1–2; TIME-006), and rejects every other length with `LengthMismatch` (`tfield`) and
/// both lengths. Every format and every slice length 0–17; an accepted code travels with and
/// without its P-field and keeps its octets.
#[test]
fn agency_codes_hold_exactly_their_formats_t_field() {
    // restricted: Agency
    // req: TIME-006, GEN-041, VER-023
    for t_len in 1..=16_u8 {
        let format = AgencyFormat::new(t_len).unwrap();
        for n in 0..=17_u8 {
            let t: Vec<u8> = (0..n)
                .map(|i| i.wrapping_mul(71).wrapping_add(t_len))
                .collect();
            match Agency::new(format, &t) {
                Ok(code) => {
                    assert_eq!(n, t_len);
                    assert_eq!(code.tfield(), t.as_slice());
                    assert_eq!(code.format(), format);
                    assert!(binary_identity(TimeCode::Agency(code)));
                }
                Err(e) => {
                    assert_ne!(n, t_len);
                    let detail = ErrorDetail::Expected {
                        expected: t_len.into(),
                        found: n.into(),
                    };
                    assert_eq!(
                        parts(&e),
                        (ErrorKind::LengthMismatch, Some(field::TFIELD), detail)
                    );
                }
            }
        }
    }
}

/// `PFieldTail::new` accepts exactly the octets that may follow octet 1 of a P-field: at most 8
/// octets in all (TIME-005), so 0–7, each with its extension flag (bit 0, 301.0 §3.1.1) set but
/// the last. Every slice of 0–2 octets, and every flag pattern of 3–8 octets; other slices are
/// `InvalidField` (`pfield_tail`). An accepted tail travels on an agency-defined P-field, which
/// a lenient decode keeps raw and a strict one rejects (TIME-005).
#[test]
fn pfield_tails_are_exactly_the_extension_chains() {
    // restricted: PFieldTail
    // req: TIME-005, TIME-006, VER-023
    const EXTENSION: u8 = 0b1000_0000;
    const MAX_PFIELD: usize = 8;
    let chain = |o: &[u8]| {
        o.len() < MAX_PFIELD
            && o.iter()
                .enumerate()
                .all(|(i, &b)| (b & EXTENSION != 0) == (i + 1 < o.len()))
    };
    let mut cases: Vec<Vec<u8>> = vec![Vec::new()];
    for a in 0..=u8::MAX {
        cases.push(vec![a]);
        cases.extend((0..=u8::MAX).map(|b| vec![a, b]));
    }
    for len in 3..=MAX_PFIELD {
        for flags in 0..1_u32 << len {
            let octet = |i: usize| {
                let flag = if flags >> i & 1 == 1 { EXTENSION } else { 0 };
                flag | u8::try_from(i * 13).unwrap()
            };
            cases.push((0..len).map(octet).collect());
        }
    }
    let format = AgencyFormat::new(2).unwrap();
    for octets in &cases {
        match PFieldTail::new(octets) {
            Ok(tail) => {
                assert!(chain(octets), "{octets:x?}");
                assert_eq!(
                    (tail.as_bytes(), tail.len()),
                    (octets.as_slice(), octets.len())
                );
                let code = TimeCode::Agency(Agency::new(format.with_tail(tail), &[1, 2]).unwrap());
                let mut out = [0_u8; 32];
                let n = code.encode(true, &mut out).unwrap();
                assert_eq!(n, 1 + octets.len() + 2);
                let d = decode_with_pfield_lenient(&out[..n]).unwrap();
                let TimeCode::Agency(a) = d.code() else {
                    panic!("not an agency-defined code");
                };
                assert_eq!((a.format().tail(), d.len()), (tail, n));
                let strict = decode_with_pfield(&out[..n]);
                assert_eq!(strict.is_ok(), tail.is_empty(), "{octets:x?}");
            }
            Err(e) => {
                assert!(!chain(octets), "{octets:x?}");
                assert_eq!(
                    parts(&e),
                    (
                        ErrorKind::InvalidField,
                        Some(field::PFIELD_TAIL),
                        ErrorDetail::None
                    )
                );
            }
        }
    }
}

/// `ReservedPField::new` accepts exactly the P-fields whose time code identification (bits
/// 1–3 of octet 1) is `000`, `011`, or `111`, the identifications reserved for future
/// application (301.0 §3.1.1; TIME-002). Every octet 1, alone and extended by one octet; on the
/// wire a lenient decode returns exactly these as an unknown code, which encodes back to its
/// P-field, and a strict decode rejects them.
#[test]
fn reserved_pfields_are_exactly_the_reserved_identifications() {
    // restricted: ReservedPField
    // req: TIME-002, TIME-001, VER-023
    let reserved = [0b000, 0b011, 0b111];
    for first in 0..=u8::MAX {
        let octets: &[u8] = if first & 0x80 == 0 {
            &[first]
        } else {
            &[first, 0x00]
        };
        let pfield = PField::parse(octets).unwrap();
        let is_reserved = reserved.contains(&((first >> 4) & 0b111));
        let r = ReservedPField::new(pfield);
        assert_eq!(r.map(|r| r.pfield()), is_reserved.then_some(pfield));
        if let Some(r) = r {
            let lenient = decode_with_pfield_lenient(octets).unwrap();
            assert_eq!(
                (lenient.code(), lenient.len()),
                (TimeCode::Unknown(r), octets.len())
            );
            let mut out = [0_u8; 8];
            let n = TimeCode::Unknown(r).encode(true, &mut out).unwrap();
            assert_eq!(&out[..n], octets);
            let e = decode_with_pfield(octets).unwrap_err();
            assert_eq!(
                parts(&e),
                plain(ErrorKind::ReservedValue, field::TIME_CODE_ID)
            );
        }
    }
}

// ----- ASCII (301.0 §3.5) -----

/// The most fractional digits Aldis supports: as many as required, 301.0 §3.5.1.1 says, and 0
/// to 12 (a configurable maximum) by TIME-031.
const MAX_DIGITS: u8 = 12;

/// `Fraction::new` accepts exactly 0–12 digits (TIME-031) and a value of at most that many
/// decimal digits, below 10^digits (301.0 §3.5.1.1, each `d` 0–9): every `u8` digit count, and
/// for each count the values 0, 10^digits − 1, 10^digits, and `u64::MAX`; each accepted
/// fraction survives an ASCII code with its leading zeros.
#[test]
fn fractions_are_exactly_up_to_12_decimal_digits() {
    // restricted: Fraction
    // req: TIME-031, VER-023
    assert_eq!(aldis_time::MAX_FRACTION_DIGITS, MAX_DIGITS);
    for digits in 0..=u8::MAX {
        let limit = (digits <= MAX_DIGITS).then(|| 10_u64.pow(u32::from(digits)));
        for value in [0, 1, 9, 10, u64::MAX]
            .into_iter()
            .chain(limit.map(|l| [l - 1, l]).into_iter().flatten())
        {
            let accepted = limit.is_some_and(|l| value < l);
            let got = Fraction::new(value, digits);
            assert_eq!(
                got.map(|f| (f.value(), f.digits())),
                accepted.then_some((value, digits))
            );
            if let Some(f) = got {
                let t = ascii_of(ccs_value_with(|_| ()));
                assert_eq!(ascii_round_trip(AsciiTime { fraction: f, ..t }).fraction, f);
            }
        }
    }
}

/// An ASCII code A of 2000-01-01T00:00:00 with `n` fractional digits `1` (none: no period).
fn ascii_with_digits(n: usize) -> Vec<u8> {
    let mut text = b"2000-01-01T00:00:00".to_vec();
    if n > 0 {
        text.push(b'.');
        text.extend(std::iter::repeat_n(b'1', n));
    }
    text
}

/// `AsciiOptions::new` accepts exactly a maximum of 0–12 fractional digits, the configurable
/// maximum of TIME-031 (301.0 §3.5.1.1): every `u8`. On the wire each accepted maximum lets
/// through exactly the codes with at most that many digits and rejects the others with
/// `OutOfRange` (`fraction`) naming it.
#[test]
fn ascii_options_are_exactly_up_to_12_digits() {
    // restricted: AsciiOptions
    // req: TIME-031, GEN-041, VER-023
    assert_eq!(AsciiOptions::DEFAULT.max_fraction_digits(), MAX_DIGITS);
    for max in 0..=u8::MAX {
        match AsciiOptions::new(max) {
            Ok(options) => {
                assert!(max <= MAX_DIGITS);
                assert_eq!(options.max_fraction_digits(), max);
                for n in 0..=usize::from(MAX_DIGITS) + 1 {
                    let got = AsciiTime::parse(&ascii_with_digits(n), &options);
                    let found = u64::try_from(n).unwrap();
                    if found <= u64::from(max) {
                        assert_eq!(usize::from(got.unwrap().fraction.digits()), n);
                    } else {
                        let e = got.unwrap_err();
                        assert_eq!(parts(&e), out_of_range(field::FRACTION, max.into(), found));
                    }
                }
            }
            Err(e) => {
                assert!(max > MAX_DIGITS);
                let bound = MAX_DIGITS.into();
                let expected = out_of_range(field::MAX_FRACTION_DIGITS, bound, max.into());
                assert_eq!(parts(&e), expected);
            }
        }
    }
}

/// `AsciiFormat::new` accepts exactly 0–12 fractional digits (TIME-031), for code A or B
/// (301.0 §3.5.1.1–2) and with or without `Z`: every variation, `u8`, and flag. Every accepted
/// format writes a code with its variation, digit count, and `Z`, which parses back unchanged.
#[test]
fn ascii_formats_are_exactly_up_to_12_digits() {
    // restricted: AsciiFormat
    // req: TIME-031, TIME-033, GEN-041, VER-023
    let instant = UtcInstant::from_ascii(&AsciiTime {
        fraction: Fraction::new(123_456_789_012, 12).unwrap(),
        ..ascii_of(ccs_value_with(|_| ()))
    });
    for variation in VARIATIONS {
        for digits in 0..=u8::MAX {
            for utc in [false, true] {
                match AsciiFormat::new(variation, digits, utc) {
                    Ok(format) => {
                        assert!(digits <= MAX_DIGITS);
                        let got = (format.variation(), format.fraction_digits(), format.utc());
                        assert_eq!(got, (variation, digits, utc));
                        let t = instant.to_ascii(format).unwrap();
                        assert_eq!(t.date.variation(), variation);
                        assert_eq!((t.fraction.digits(), t.utc), (digits, utc));
                        assert_eq!(ascii_round_trip(t), t);
                    }
                    Err(e) => {
                        assert!(digits > MAX_DIGITS);
                        let bound = MAX_DIGITS.into();
                        let expected = out_of_range(field::FRACTION_DIGITS, bound, digits.into());
                        assert_eq!(parts(&e), expected);
                    }
                }
            }
        }
    }
}

// ----- The same properties over the shared strategies (VER-054) -----

proptest! {
    #![proptest_config(strategies::config(256, REGRESSIONS))]

    /// Any year, hour, minute, and second from the strategies survives a CCS code and an ASCII
    /// code in place of a strategy-drawn code's own (301.0 §3.4, §3.5.1, Annex A).
    #[test]
    fn calendar_fields_survive_any_code(
        c in ccs(), y in year(), h in hour(), m in minute(), s in second(),
    ) {
        // restricted: Year, Hour, Minute, Second
        // req: TIME-030, TIME-031, VER-023
        let date = c.value().date;
        let date = match (date.month_day(), date.day_of_year()) {
            // Day 1 exists in every month and year.
            (Some((month, _)), _) => CalendarDate::from_month_day(y, month, 1).unwrap(),
            _ => CalendarDate::from_day_of_year(y, 1).unwrap(),
        };
        let v = CcsValue { date, hour: h, minute: m, second: s, ..c.value() };
        let code = TimeCode::Ccs(Ccs::new(c.format(), v).unwrap());
        prop_assert!(binary_identity(code));
        let t = AsciiTime { utc: false, ..ascii_of(v) };
        prop_assert_eq!(ascii_round_trip(t), t);
    }

    /// Any date of either variation survives a CCS and an ASCII code (301.0 §3.4.1, §3.5.1).
    #[test]
    fn any_date_survives_its_wire_forms(
        date in ccs_variation().prop_flat_map(calendar_date),
    ) {
        // restricted: CalendarDate
        // req: TIME-030, TIME-031, VER-023
        let v = ccs_value_with(|v| v.date = date);
        prop_assert!(binary_identity(ccs_code(v)));
        prop_assert_eq!(ascii_round_trip(ascii_of(v)).date, date);
    }

    /// Any milliseconds of day, valid or above 86 400 999, behaves on the wire as `MsOfDay` does
    /// (301.0 Annex A; TIME-021).
    #[test]
    fn any_ms_of_day_is_checked_alike(ok in ms_of_day(), bad in ms_of_day_out_of_range()) {
        // restricted: MsOfDay
        // req: TIME-021, GEN-041, VER-023
        let spec = 0..=86_400_999_u32;
        check_ms_of_day(ok.get(), &spec);
        check_ms_of_day(bad, &spec);
    }

    /// Any picoseconds of millisecond, valid or above 999 999 999, behave on the wire as `Picos`
    /// does (301.0 §3.3.2; TIME-021).
    #[test]
    fn any_picos_are_checked_alike(ok in 0..=999_999_999_u32, bad in picos_out_of_range()) {
        // restricted: Picos
        // req: TIME-021, GEN-041, VER-023
        let spec = 0..=999_999_999_u32;
        check_picos(ok, &spec);
        check_picos(bad, &spec);
    }

    /// Any CDS code survives its encoding; a day beyond the day segment is `OutOfRange`
    /// (`cds_day`) naming the segment's largest day (301.0 §3.3.1, Annex A; TIME-020).
    #[test]
    fn any_cds_value_fits_exactly(
        code in cds(),
        (format, value, day) in cds_format().prop_flat_map(|f| (
            Just(f), cds_value(f), cds_day_out_of_range(f.day_len()),
        )),
    ) {
        // restricted: Cds
        // req: TIME-020, TIME-021, GEN-041, VER-023
        prop_assert!(binary_identity(TimeCode::Cds(code)));
        prop_assert_eq!(Cds::new(format, value).map(|c| c.value()), Ok(value));
        let e = Cds::new(format, CdsValue { day, ..value }).unwrap_err();
        let max = spec_max_day(format.day_len());
        prop_assert_eq!(parts(&e), out_of_range(field::CDS_DAY, max.into(), day.into()));
    }

    /// Any CUC code survives its encoding; a coarse time too wide for the format, or a fraction
    /// below its resolution, is rejected; any seconds and nanoseconds that fit give a code that
    /// survives too (301.0 §3.2.1; TIME-011, TIME-012).
    #[test]
    fn any_cuc_value_fits_exactly(
        code in cuc(),
        (format, value, coarse, fine) in cuc_format().prop_flat_map(|f| (
            Just(f), cuc_value(f), cuc_coarse_out_of_range(f), cuc_fine_off_resolution(f),
        )),
        ns in nanos(),
    ) {
        // restricted: Cuc
        // req: TIME-011, TIME-012, GEN-041, VER-023
        prop_assert!(binary_identity(TimeCode::Cuc(code)));
        prop_assert_eq!(Cuc::new(format, value).map(|c| c.value()), Ok(value));
        let max = u64::MAX >> (64 - 8 * u32::from(format.coarse_octets()));
        let e = Cuc::new(format, CucValue::new(coarse, value.fine())).unwrap_err();
        prop_assert_eq!(parts(&e), out_of_range(field::COARSE, max, coarse));
        let e = Cuc::new(format, CucValue::new(value.coarse(), fine)).unwrap_err();
        prop_assert_eq!(parts(&e), plain(ErrorKind::OutOfRange, field::FINE));
        let from_ns = Cuc::from_seconds_nanos(format, value.coarse(), ns).unwrap();
        prop_assert!(binary_identity(TimeCode::Cuc(from_ns)));
    }

    /// Any CUC format of any epoch survives a code's P-field (301.0 §3.2.2; TIME-010, TIME-013).
    #[test]
    fn any_cuc_format_survives_its_pfield(format in cuc_format(), epoch in cuc_epoch()) {
        // restricted: CucFormat
        // req: TIME-010, TIME-013, VER-023
        let format = CucFormat::new(epoch, format.coarse_octets(), format.fine_octets())
            .and_then(|f| f.with_mission_bits(format.mission_bits()))
            .unwrap();
        prop_assert!(binary_identity(TimeCode::Cuc(Cuc::new(format, CucValue::default()).unwrap())));
    }

    /// Any CCS code survives its encoding; a sub-second value with more digits than the format
    /// holds is `OutOfRange` (`subsecond`) (301.0 §3.4.2; TIME-030).
    #[test]
    fn any_ccs_value_fits_exactly(
        code in ccs(),
        (format, value, sub) in ccs_format().prop_flat_map(|f| (
            Just(f), ccs_value(f), ccs_subsecond_out_of_range(f),
        )),
    ) {
        // restricted: Ccs, CcsFormat
        // req: TIME-030, GEN-041, VER-023
        prop_assert!(binary_identity(TimeCode::Ccs(code)));
        prop_assert_eq!(Ccs::new(format, value).map(|c| c.value()), Ok(value));
        let limit = 10_u64.pow(2 * u32::from(format.subsecond_octets()));
        let e = Ccs::new(format, CcsValue { subsecond: sub, ..value }).unwrap_err();
        prop_assert_eq!(parts(&e), out_of_range(field::SUBSECOND, limit - 1, sub));
    }

    /// Any fraction of up to 12 digits survives an ASCII code; a value with more digits than its
    /// count is rejected (301.0 §3.5.1; TIME-031).
    #[test]
    fn any_fraction_fits_exactly(
        f in fraction(MAX_DIGITS), (value, digits) in fraction_out_of_range(), t in ascii_time(),
    ) {
        // restricted: Fraction
        // req: TIME-031, VER-023
        let t = AsciiTime { fraction: f, ..t };
        prop_assert_eq!(ascii_round_trip(t), t);
        prop_assert_eq!(Fraction::new(value, digits), None);
    }

    /// Any ASCII code written in any accepted format parses back (301.0 §3.5.1; TIME-031,
    /// TIME-033).
    #[test]
    fn any_ascii_format_writes_a_code_that_parses_back(
        t in ascii_time(), variation in ccs_variation(), digits in 0..=MAX_DIGITS, utc in any::<bool>(),
    ) {
        // restricted: AsciiFormat, AsciiOptions
        // req: TIME-031, TIME-033, VER-023
        let format = AsciiFormat::new(variation, digits, utc).unwrap();
        let out = UtcInstant::from_ascii(&t).to_ascii(format).unwrap();
        prop_assert_eq!(ascii_round_trip(out), out);
        let options = AsciiOptions::new(digits).unwrap();
        let mut text = [0_u8; MAX_ASCII_LEN];
        let n = out.encode(&mut text).unwrap();
        prop_assert_eq!(AsciiTime::parse(&text[..n], &options), Ok(out));
    }
}
