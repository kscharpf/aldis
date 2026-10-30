//! Strategies for `aldis-time`: P-field tails, CUC, CDS, CCS, and agency-defined formats and
//! codes, calendar fields, the ASCII codes, and time-code octets next to the decoder's
//! boundaries (301.0; VER-054).
//!
//! The formats drawn by [`cuc_format`], [`cds_format`], [`ccs_format`], and [`agency_format`]
//! are conformant: they carry no undefined P-field extension octets, so a strict decoder accepts
//! their codes. The `*_with_tail` variants add such octets, which only a lenient decoder keeps
//! (TIME-005).

use aldis_time::{
    Agency, AgencyFormat, AsciiFormat, AsciiTime, CalendarDate, Ccs, CcsFormat, CcsValue,
    CcsVariation, Cds, CdsDayLen, CdsEpoch, CdsFormat, CdsSubMs, CdsValue, Cuc, CucEpoch,
    CucFormat, CucValue, Fraction, Hour, MAX_FRACTION_DIGITS, Micros, Minute, Month, MsOfDay,
    PFieldTail, Picos, Second, SubMillis, TimeCode, TimeCodeId, TimeFormat, Year,
};
use proptest::prelude::*;

use super::common::{up_to_u8, up_to_u16, up_to_u32, up_to_u64};
use super::{octets, proper_prefix, with_trailing};

/// The P-field extension flag, bit 0 (the most significant bit) of every P-field octet (301.0
/// §3.1.1).
const EXTENSION: u8 = 0x80;

/// A chain of 0 to `max_octets` undefined P-field extension octets: the flag set on every octet
/// but the last (301.0 §3.1.1, TIME-005).
///
/// # Panics
/// If `max_octets` exceeds 7, the most a P-field of 8 octets leaves after octet 1.
pub fn pfield_tail(max_octets: usize) -> impl Strategy<Value = PFieldTail> {
    assert!(max_octets <= 7, "a P-field tail has at most 7 octets");
    octets(0..=max_octets).prop_filter_map("a well-formed chain", |mut tail| {
        let n = tail.len();
        for (i, octet) in tail.iter_mut().enumerate() {
            if i + 1 == n {
                *octet &= !EXTENSION;
            } else {
                *octet |= EXTENSION;
            }
        }
        PFieldTail::new(&tail).ok()
    })
}

/// A non-empty [`pfield_tail`] of at most `max_octets` octets (TIME-005).
fn non_empty_tail(max_octets: usize) -> impl Strategy<Value = PFieldTail> {
    pfield_tail(max_octets).prop_filter("non-empty", |t| !t.is_empty())
}

/// `max + 1..=u32::MAX`, with `max + 1` drawn most often (GEN-041).
///
/// # Panics
/// If `max` is `u32::MAX`.
fn above_u32(max: u32) -> BoxedStrategy<u32> {
    assert!(max < u32::MAX, "no value above max");
    prop_oneof![1 => Just(max + 1), 1 => Just(u32::MAX), 2 => max + 1..=u32::MAX].boxed()
}

/// `max + 1..=u64::MAX`, with `max + 1` drawn most often (GEN-041).
///
/// # Panics
/// If `max` is `u64::MAX`.
fn above_u64(max: u64) -> BoxedStrategy<u64> {
    assert!(max < u64::MAX, "no value above max");
    prop_oneof![1 => Just(max + 1), 1 => Just(u64::MAX), 2 => max + 1..=u64::MAX].boxed()
}

// ----- CUC (301.0 §3.2) -----

/// Level 1 (1958 TAI) or Level 2 (agency) epoch (301.0 §3.2.2, TIME-010).
pub fn cuc_epoch() -> impl Strategy<Value = CucEpoch> {
    prop_oneof![Just(CucEpoch::Tai1958), Just(CucEpoch::Agency)]
}

/// A conformant CUC format: 1–7 coarse and 0–10 fine octets, P-field octet 2 forced or not, and
/// mission bits 0–3 (301.0 §3.2.2; TIME-010, TIME-013).
pub fn cuc_format() -> impl Strategy<Value = CucFormat> {
    (
        cuc_epoch(),
        1_u8..=CucFormat::MAX_COARSE,
        0_u8..=CucFormat::MAX_FINE,
        0_u8..3,
        0_u8..=3,
    )
        .prop_filter_map(
            "counts in range",
            |(epoch, coarse, fine, octet2, mission)| {
                let f = CucFormat::new(epoch, coarse, fine).ok()?;
                match octet2 {
                    0 => Some(f),
                    1 => Some(f.with_octet2()),
                    _ => f.with_mission_bits(mission).ok(),
                }
            },
        )
}

/// A CUC format with 1–6 undefined P-field octets after octet 2 (TIME-005).
pub fn cuc_format_with_tail() -> impl Strategy<Value = CucFormat> {
    (cuc_format(), non_empty_tail(6)).prop_filter_map("tail fits after octet 2", |(f, tail)| {
        f.with_tail(tail).ok()
    })
}

/// A coarse time that fits `format`: `0..2^(8 × coarse octets)`, edges included (TIME-012).
pub fn cuc_coarse(format: CucFormat) -> impl Strategy<Value = u64> {
    let bits = 8 * u32::from(format.coarse_octets());
    up_to_u64(u64::MAX >> (64 - bits))
}

/// A value representable in `format`: a [`cuc_coarse`] time and a left-aligned fraction with no
/// bits below the format's resolution (301.0 §3.2.1; TIME-011, M1 D2).
pub fn cuc_value(format: CucFormat) -> impl Strategy<Value = CucValue> {
    let keep = 8 * u32::from(format.fine_octets());
    let mask = if keep == 0 {
        0
    } else {
        u128::MAX << (128 - keep)
    };
    (cuc_coarse(format), any::<u128>()).prop_map(move |(c, f)| CucValue::new(c, f & mask))
}

/// A CUC code in a conformant [`cuc_format`] (301.0 §3.2; TIME-010…TIME-013).
pub fn cuc() -> impl Strategy<Value = Cuc> {
    cuc_format().prop_flat_map(cuc_in)
}

/// A CUC code in `format` (301.0 §3.2; TIME-011, TIME-012).
pub fn cuc_in(format: CucFormat) -> impl Strategy<Value = Cuc> {
    cuc_value(format).prop_filter_map("value fits", move |v| Cuc::new(format, v).ok())
}

/// Coarse times too wide for `format`, which `Cuc::new` and `Cuc::from_seconds_nanos` reject
/// (TIME-012).
///
/// `2^(8 × coarse octets)` is drawn most often. A format has at most 7 coarse octets, so there
/// is always a value above.
pub fn cuc_coarse_out_of_range(format: CucFormat) -> BoxedStrategy<u64> {
    let bits = 8 * u32::from(format.coarse_octets());
    above_u64(u64::MAX >> (64 - bits))
}

/// Left-aligned fractions with a bit below `format`'s resolution, which `Cuc::new` rejects
/// (301.0 §3.2.1, TIME-011).
///
/// The bit just below is drawn most often. A format has at most 10 fine octets (80 bits), so 48
/// bits always lie below.
pub fn cuc_fine_off_resolution(format: CucFormat) -> BoxedStrategy<u128> {
    let below = 128 - 8 * u32::from(format.fine_octets());
    let bit = prop_oneof![1 => Just(below - 1), 1 => Just(0), 2 => 0..below];
    (any::<u128>(), bit)
        .prop_map(|(f, bit)| f | (1_u128 << bit))
        .boxed()
}

/// Nanoseconds of a second, `0..10⁹` (TIME-011).
pub fn nanos() -> impl Strategy<Value = u32> {
    prop_oneof![1 => Just(0), 1 => Just(999_999_999), 4 => 0_u32..1_000_000_000]
}

// ----- CDS (301.0 §3.3) -----

/// The 1958 or an agency-defined epoch (301.0 §3.3.2, TIME-020).
pub fn cds_epoch() -> impl Strategy<Value = CdsEpoch> {
    prop_oneof![Just(CdsEpoch::Ccsds1958), Just(CdsEpoch::Agency)]
}

/// A 16- or 24-bit day segment (301.0 §3.3.2, TIME-020).
pub fn cds_day_len() -> impl Strategy<Value = CdsDayLen> {
    prop_oneof![Just(CdsDayLen::Bits16), Just(CdsDayLen::Bits24)]
}

/// No, microsecond, or picosecond submillisecond segment (301.0 §3.3.2, TIME-020).
pub fn cds_sub_ms() -> impl Strategy<Value = CdsSubMs> {
    prop_oneof![
        Just(CdsSubMs::None),
        Just(CdsSubMs::Micros),
        Just(CdsSubMs::Picos)
    ]
}

/// A conformant CDS format: every epoch, day length, and resolution (301.0 §3.3.2, TIME-020).
pub fn cds_format() -> impl Strategy<Value = CdsFormat> {
    (cds_epoch(), cds_day_len(), cds_sub_ms()).prop_map(|(e, d, s)| CdsFormat::new(e, d, s))
}

/// A CDS format with 1–7 undefined P-field octets (TIME-005).
pub fn cds_format_with_tail() -> impl Strategy<Value = CdsFormat> {
    (cds_format(), non_empty_tail(7)).prop_map(|(f, tail)| f.with_tail(tail))
}

/// Milliseconds of day, 0–86 400 999, the leap second included (301.0 Annex A, TIME-021).
pub fn ms_of_day() -> impl Strategy<Value = MsOfDay> {
    up_to_u32(MsOfDay::MAX).prop_filter_map("in range", MsOfDay::new)
}

/// A submillisecond value of resolution `subms` (301.0 Annex A, TIME-021).
pub fn sub_millis(subms: CdsSubMs) -> BoxedStrategy<SubMillis> {
    match subms {
        CdsSubMs::None => Just(SubMillis::None).boxed(),
        CdsSubMs::Micros => up_to_u16(Micros::MAX)
            .prop_filter_map("in range", |v| Micros::new(v).map(SubMillis::Micros))
            .boxed(),
        CdsSubMs::Picos => up_to_u32(Picos::MAX)
            .prop_filter_map("in range", |v| Picos::new(v).map(SubMillis::Picos))
            .boxed(),
    }
}

/// A value that fits `format`: a day count within its day segment, any milliseconds of day,
/// and a submillisecond value of its resolution (301.0 §3.3.1; TIME-020, TIME-021).
pub fn cds_value(format: CdsFormat) -> impl Strategy<Value = CdsValue> {
    let max_day: u32 = match format.day_len() {
        CdsDayLen::Bits16 => 0xFFFF,
        CdsDayLen::Bits24 => 0xFF_FFFF,
    };
    (up_to_u32(max_day), ms_of_day(), sub_millis(format.subms())).prop_map(
        |(day, ms_of_day, subms)| CdsValue {
            day,
            ms_of_day,
            subms,
        },
    )
}

/// Milliseconds of day above 86 400 999, which `MsOfDay::new` rejects (301.0 Annex A,
/// TIME-021).
pub fn ms_of_day_out_of_range() -> BoxedStrategy<u32> {
    above_u32(MsOfDay::MAX)
}

/// Picoseconds of millisecond above 999 999 999, which `Picos::new` rejects (TIME-021).
pub fn picos_out_of_range() -> BoxedStrategy<u32> {
    above_u32(Picos::MAX)
}

/// Day counts beyond a 16- or 24-bit day segment, `2^16` or `2^24` drawn most often, which
/// `Cds::new` rejects (301.0 §3.3.1, Annex A; TIME-020).
pub fn cds_day_out_of_range(day: CdsDayLen) -> BoxedStrategy<u32> {
    above_u32(match day {
        CdsDayLen::Bits16 => 0xFFFF,
        CdsDayLen::Bits24 => 0xFF_FFFF,
    })
}

/// A CDS code in a conformant [`cds_format`] (301.0 §3.3; TIME-020, TIME-021).
pub fn cds() -> impl Strategy<Value = Cds> {
    cds_format().prop_flat_map(cds_in)
}

/// A CDS code in `format` (301.0 §3.3; TIME-020, TIME-021).
pub fn cds_in(format: CdsFormat) -> impl Strategy<Value = Cds> {
    cds_value(format).prop_filter_map("value fits", move |v| Cds::new(format, v).ok())
}

// ----- Calendar fields and CCS (301.0 §3.4, Annex A) -----

/// A year 1–9999 (301.0 Annex A, TIME-030).
pub fn year() -> impl Strategy<Value = Year> {
    up_to_u16(Year::MAX - Year::MIN).prop_filter_map("in range", |v| Year::new(Year::MIN + v))
}

/// An hour 0–23 (301.0 Annex A, TIME-030).
pub fn hour() -> impl Strategy<Value = Hour> {
    up_to_u8(Hour::MAX).prop_filter_map("in range", Hour::new)
}

/// A minute 0–59 (301.0 Annex A, TIME-030).
pub fn minute() -> impl Strategy<Value = Minute> {
    up_to_u8(Minute::MAX).prop_filter_map("in range", Minute::new)
}

/// A second 0–60, the leap second included (301.0 Annex A; TIME-022, TIME-030).
pub fn second() -> impl Strategy<Value = Second> {
    up_to_u8(Second::MAX).prop_filter_map("in range", Second::new)
}

/// Month/day (code A) or day of year (code B) (301.0 §3.4.2, §3.5.1; TIME-030, TIME-031).
pub fn ccs_variation() -> impl Strategy<Value = CcsVariation> {
    prop_oneof![Just(CcsVariation::MonthDay), Just(CcsVariation::DayOfYear)]
}

/// A date that exists, expressed in `variation`: a month and day of month, or a day of year;
/// the last days of months and leap days included (301.0 Annex A, TIME-030).
pub fn calendar_date(variation: CcsVariation) -> BoxedStrategy<CalendarDate> {
    match variation {
        CcsVariation::MonthDay => (year(), 1_u8..=12, 1_u8..=31)
            .prop_filter_map("a date", |(y, m, d)| {
                let month = Month::new(m)?;
                // Days above 28 that do not exist in the month fall back to its last day.
                (28.min(d)..=d)
                    .rev()
                    .find_map(|day| CalendarDate::from_month_day(y, month, day).ok())
            })
            .boxed(),
        CcsVariation::DayOfYear => (year(), 1_u16..=366)
            .prop_filter_map("a date", |(y, d)| {
                // Day 366 of a common year falls back to day 365.
                CalendarDate::from_day_of_year(y, d)
                    .or_else(|_| CalendarDate::from_day_of_year(y, 365))
                    .ok()
            })
            .boxed(),
    }
}

/// A conformant CCS format: either variation, 0–6 sub-second octets (301.0 §3.4.2, TIME-030).
pub fn ccs_format() -> impl Strategy<Value = CcsFormat> {
    (ccs_variation(), 0_u8..=CcsFormat::MAX_SUBSECOND_OCTETS)
        .prop_filter_map("octets in range", |(v, n)| CcsFormat::new(v, n).ok())
}

/// A CCS format with 1–7 undefined P-field octets (TIME-005).
pub fn ccs_format_with_tail() -> impl Strategy<Value = CcsFormat> {
    (ccs_format(), non_empty_tail(7)).prop_map(|(f, tail)| f.with_tail(tail))
}

/// A value that fits `format`: a date in its variation, any time of day with the leap second,
/// and a sub-second value of at most two digits per sub-second octet (301.0 §3.4.1, TIME-030).
pub fn ccs_value(format: CcsFormat) -> impl Strategy<Value = CcsValue> {
    let limit = 10_u64.pow(2 * u32::from(format.subsecond_octets()));
    (
        calendar_date(format.variation()),
        hour(),
        minute(),
        second(),
        up_to_u64(limit - 1),
    )
        .prop_map(|(date, hour, minute, second, subsecond)| CcsValue {
            date,
            hour,
            minute,
            second,
            subsecond,
        })
}

/// A CCS code in a conformant [`ccs_format`] (301.0 §3.4, TIME-030).
pub fn ccs() -> impl Strategy<Value = Ccs> {
    ccs_format().prop_flat_map(ccs_in)
}

/// A CCS code in `format` (301.0 §3.4, TIME-030).
pub fn ccs_in(format: CcsFormat) -> impl Strategy<Value = Ccs> {
    ccs_value(format).prop_filter_map("value fits", move |v| Ccs::new(format, v).ok())
}

/// Sub-second values with more digits than `format`'s sub-second octets hold, `10^(2n)` drawn
/// most often, which `Ccs::new` rejects (301.0 §3.4.2, TIME-030).
pub fn ccs_subsecond_out_of_range(format: CcsFormat) -> BoxedStrategy<u64> {
    above_u64(10_u64.pow(2 * u32::from(format.subsecond_octets())) - 1)
}

// ----- Agency-defined codes (301.0 §3.6) -----

/// A conformant agency-defined format with a T-field of 1–16 octets (301.0 §3.6.2, TIME-006).
pub fn agency_format() -> impl Strategy<Value = AgencyFormat> {
    (1_u8..=16).prop_filter_map("length in range", |n| AgencyFormat::new(n).ok())
}

/// An agency-defined format with 1–7 undefined P-field octets (TIME-005, TIME-006).
pub fn agency_format_with_tail() -> impl Strategy<Value = AgencyFormat> {
    (agency_format(), non_empty_tail(7)).prop_map(|(f, tail)| f.with_tail(tail))
}

/// An agency-defined code in `format`, with an arbitrary T-field (301.0 §3.6, TIME-006).
pub fn agency_in(format: AgencyFormat) -> impl Strategy<Value = Agency> {
    octets(format.t_len()).prop_filter_map("T-field length", move |t| Agency::new(format, &t).ok())
}

/// An agency-defined code in a conformant [`agency_format`] (301.0 §3.6, TIME-006).
pub fn agency() -> impl Strategy<Value = Agency> {
    agency_format().prop_flat_map(agency_in)
}

// ----- Any binary code (301.0 §3.1) -----

/// A conformant format of any of the four binary codes (301.0 §3.1.1; TIME-001, TIME-003).
pub fn time_format() -> impl Strategy<Value = TimeFormat> {
    prop_oneof![
        cuc_format().prop_map(TimeFormat::Cuc),
        cds_format().prop_map(TimeFormat::Cds),
        ccs_format().prop_map(TimeFormat::Ccs),
        agency_format().prop_map(TimeFormat::Agency),
    ]
}

/// A code in `format` (301.0 §3.1; TIME-001, TIME-003).
pub fn time_code_in(format: TimeFormat) -> BoxedStrategy<TimeCode> {
    match format {
        TimeFormat::Cuc(f) => cuc_in(f).prop_map(TimeCode::Cuc).boxed(),
        TimeFormat::Cds(f) => cds_in(f).prop_map(TimeCode::Cds).boxed(),
        TimeFormat::Ccs(f) => ccs_in(f).prop_map(TimeCode::Ccs).boxed(),
        TimeFormat::Agency(f) => agency_in(f).prop_map(TimeCode::Agency).boxed(),
    }
}

/// A conformant code of any of the four binary codes, which a strict decoder accepts (301.0
/// §3.1–3.4, §3.6; TIME-001, TIME-003, TIME-006).
pub fn time_code() -> impl Strategy<Value = TimeCode> {
    time_format().prop_flat_map(time_code_in)
}

/// A code whose P-field carries undefined extension octets: rejected in strict mode, kept by a
/// lenient decoder (TIME-005).
pub fn time_code_with_tail() -> impl Strategy<Value = TimeCode> {
    prop_oneof![
        cuc_format_with_tail().prop_map(TimeFormat::Cuc),
        cds_format_with_tail().prop_map(TimeFormat::Cds),
        ccs_format_with_tail().prop_map(TimeFormat::Ccs),
        agency_format_with_tail().prop_map(TimeFormat::Agency),
    ]
    .prop_flat_map(time_code_in)
}

/// `code` encoded with its P-field (301.0 §3.1, API-020).
///
/// # Panics
/// If the code does not encode, which no code these strategies produce does.
#[must_use]
pub fn encode_with_pfield(code: &TimeCode) -> Vec<u8> {
    let mut out = [0_u8; 32];
    let n = code
        .encode(true, &mut out)
        .expect("a 32-octet buffer fits any code");
    out[..n].to_vec()
}

// ----- Time-code octets next to the decoder's boundaries -----

/// A reserved time code ID: `000`, `011`, or `111` (301.0 §3.1.1, TIME-002).
pub fn reserved_time_code_id() -> impl Strategy<Value = TimeCodeId> {
    prop_oneof![
        Just(TimeCodeId::Reserved000),
        Just(TimeCodeId::Reserved011),
        Just(TimeCodeId::Reserved111),
    ]
}

/// Octets of a time code with its P-field, next to the boundaries of `decode_with_pfield`
/// (301.0 §3.1; GEN-020, API-030, VER-023):
///
/// - a conformant code, possibly followed by other octets, which a strict decoder accepts;
/// - a code with undefined P-field extension octets (TIME-005, lenient only);
/// - a proper prefix of a code: one octet too short, or cut inside the P-field;
/// - a reserved time code ID in bits 1–3 of P-field octet 1 (TIME-002);
/// - CDS submillisecond resolution code `11` in bits 6–7 (301.0 §3.3.2, TIME-020);
/// - CCS sub-second resolution code `111` in bits 5–7 (301.0 §3.4.2, TIME-030);
/// - a P-field whose extension chain runs past 8 octets (TIME-005).
pub fn time_code_near_boundary() -> BoxedStrategy<Vec<u8>> {
    let conformant = time_code().prop_map(|c| encode_with_pfield(&c)).boxed();
    let with_tail = time_code_with_tail()
        .prop_map(|c| encode_with_pfield(&c))
        .boxed();
    prop_oneof![
        conformant
            .clone()
            .prop_flat_map(|v| with_trailing(v, 4))
            .boxed(),
        with_tail.clone(),
        prop_oneof![conformant.clone(), with_tail]
            .prop_flat_map(proper_prefix)
            .boxed(),
        (conformant.clone(), reserved_time_code_id())
            .prop_map(|(mut v, id)| {
                v[0] = (v[0] & 0x8F) | (id.bits() << 4);
                v
            })
            .boxed(),
        cds()
            .prop_map(|c| {
                let mut v = encode_with_pfield(&TimeCode::Cds(c));
                v[0] |= 0b11;
                v
            })
            .boxed(),
        ccs()
            .prop_map(|c| {
                let mut v = encode_with_pfield(&TimeCode::Ccs(c));
                v[0] |= 0b111;
                v
            })
            .boxed(),
        (conformant, 8_usize..10)
            .prop_map(|(v, n)| {
                let mut chain = vec![v[0] | EXTENSION];
                chain.extend(std::iter::repeat_n(EXTENSION, n - 1));
                chain.extend_from_slice(&v[1..]);
                chain
            })
            .boxed(),
    ]
    .boxed()
}

/// Input for a time-code decoder property: arbitrary octets or octets
/// [near a boundary](time_code_near_boundary) (GEN-020, API-030, VER-023).
pub fn time_code_decoder_input() -> BoxedStrategy<Vec<u8>> {
    prop_oneof![1 => octets(0..32), 3 => time_code_near_boundary()].boxed()
}

// ----- ASCII codes (301.0 §3.5) -----

/// A fraction of 0 to `max_digits` decimal digits (301.0 §3.5.1, TIME-031).
///
/// # Panics
/// If `max_digits` exceeds 12.
pub fn fraction(max_digits: u8) -> impl Strategy<Value = Fraction> {
    assert!(max_digits <= MAX_FRACTION_DIGITS, "at most 12 digits");
    (0..=max_digits).prop_flat_map(|digits| {
        up_to_u64(10_u64.pow(u32::from(digits)) - 1)
            .prop_filter_map("value below 10^digits", move |v| Fraction::new(v, digits))
    })
}

/// A digit count of 0–12 and a value with more digits than that, which `Fraction::new` rejects
/// (301.0 §3.5.1, TIME-031).
///
/// The value is at least 10^digits, `10^digits` drawn most often.
pub fn fraction_out_of_range() -> impl Strategy<Value = (u64, u8)> {
    (0..=MAX_FRACTION_DIGITS)
        .prop_flat_map(|digits| (above_u64(10_u64.pow(u32::from(digits)) - 1), Just(digits)))
}

/// An ASCII format: code A or B, 0–12 fractional digits, with or without `Z` (301.0 §3.5.1;
/// TIME-031, TIME-033).
pub fn ascii_format() -> impl Strategy<Value = AsciiFormat> {
    (ccs_variation(), 0..=MAX_FRACTION_DIGITS, any::<bool>())
        .prop_filter_map("digits in range", |(v, d, z)| {
            AsciiFormat::new(v, d, z).ok()
        })
}

/// A complete ASCII time code A or B with up to 12 fractional digits (301.0 §3.5.1, TIME-031).
pub fn ascii_time() -> impl Strategy<Value = AsciiTime> {
    (
        ccs_variation().prop_flat_map(calendar_date),
        hour(),
        minute(),
        second(),
        fraction(MAX_FRACTION_DIGITS),
        any::<bool>(),
    )
        .prop_map(|(date, hour, minute, second, fraction, utc)| AsciiTime {
            date,
            hour,
            minute,
            second,
            fraction,
            utc,
        })
}

/// `time` as ASCII text (301.0 §3.5.1, TIME-031).
///
/// # Panics
/// If the code does not encode, which no [`ascii_time`] does.
#[must_use]
pub fn ascii_text(time: &AsciiTime) -> Vec<u8> {
    let mut out = [0_u8; aldis_time::MAX_ASCII_LEN];
    let n = time.encode(&mut out).expect("MAX_ASCII_LEN fits any code");
    out[..n].to_vec()
}

/// ASCII text next to the parsers' boundaries (301.0 §3.5; TIME-031, TIME-032): a valid code,
/// a proper prefix of one, one octet replaced, or one octet inserted.
pub fn ascii_near_boundary() -> BoxedStrategy<Vec<u8>> {
    let valid = ascii_time().prop_map(|t| ascii_text(&t)).boxed();
    prop_oneof![
        valid.clone().boxed(),
        valid.clone().prop_flat_map(proper_prefix).boxed(),
        (valid.clone(), any::<prop::sample::Index>(), any::<u8>())
            .prop_map(|(mut v, i, b)| {
                let i = i.index(v.len());
                v[i] = b;
                v
            })
            .boxed(),
        (valid, any::<prop::sample::Index>(), any::<u8>())
            .prop_map(|(mut v, i, b)| {
                let i = i.index(v.len() + 1);
                v.insert(i, b);
                v
            })
            .boxed(),
    ]
    .boxed()
}

/// Input for an ASCII parser property: arbitrary octets or text
/// [near a boundary](ascii_near_boundary) (GEN-020, TIME-031, TIME-032).
pub fn ascii_parser_input() -> BoxedStrategy<Vec<u8>> {
    prop_oneof![1 => octets(0..40), 3 => ascii_near_boundary()].boxed()
}

#[cfg(test)]
mod tests {
    use super::{
        ascii_parser_input, ascii_text, ascii_time, ccs_format, ccs_in, ccs_subsecond_out_of_range,
        cds_day_len, cds_day_out_of_range, cuc_coarse_out_of_range, cuc_fine_off_resolution,
        cuc_format, cuc_value, encode_with_pfield, fraction_out_of_range, ms_of_day_out_of_range,
        picos_out_of_range, time_code, time_code_decoder_input, time_code_near_boundary,
        time_code_with_tail,
    };
    use crate::strategies::config;
    use aldis_core::ErrorKind;
    use aldis_time::{
        AsciiOptions, AsciiTime, Ccs, CcsValue, CdsDayLen, Cuc, CucValue, Fraction, MsOfDay, Picos,
        TimeCode, decode_with_pfield, decode_with_pfield_lenient,
    };
    use proptest::prelude::*;

    /// Whether `d` is `code` and spans `octets`.
    fn is(d: &aldis_time::Decoded, octets: &[u8], code: &TimeCode) -> bool {
        d.code() == *code && d.len() == octets.len()
    }

    const REGRESSIONS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/proptest-regressions/strategies.txt"
    );

    proptest! {
        #![proptest_config(config(256, REGRESSIONS))]

        #[test]
        fn conformant_codes_decode_strictly(code in time_code()) {
            // req: VER-054, TIME-001, TIME-003
            let octets = encode_with_pfield(&code);
            prop_assert!(is(&decode_with_pfield(&octets).unwrap(), &octets, &code));
        }

        #[test]
        fn codes_with_tails_are_lenient_only(code in time_code_with_tail()) {
            // req: VER-054, TIME-005
            let octets = encode_with_pfield(&code);
            prop_assert_eq!(
                decode_with_pfield(&octets).map_err(|e| e.kind()).err(),
                Some(ErrorKind::ReservedValue)
            );
            prop_assert!(is(&decode_with_pfield_lenient(&octets).unwrap(), &octets, &code));
        }

        #[test]
        fn boundary_inputs_never_panic(a in time_code_near_boundary(), b in time_code_decoder_input()) {
            // req: VER-054, GEN-020
            for bytes in [a, b] {
                let _ = decode_with_pfield(&bytes);
                let _ = decode_with_pfield_lenient(&bytes);
            }
        }

        #[test]
        fn out_of_range_values_are_out_of_range(
            ms in ms_of_day_out_of_range(),
            ps in picos_out_of_range(),
            (day_len, day) in cds_day_len().prop_flat_map(|d| (Just(d), cds_day_out_of_range(d))),
            (value, digits) in fraction_out_of_range(),
        ) {
            // req: VER-054, TIME-020, TIME-021, TIME-031
            prop_assert!(MsOfDay::new(ms).is_none() && Picos::new(ps).is_none());
            let max_day = if day_len == CdsDayLen::Bits16 { 0xFFFF } else { 0xFF_FFFF };
            prop_assert!(day > max_day);
            prop_assert!(Fraction::new(value, digits).is_none());
        }

        #[test]
        fn misfits_do_not_fit(
            (f, coarse, fine, ok) in cuc_format().prop_flat_map(|f| (
                Just(f), cuc_coarse_out_of_range(f), cuc_fine_off_resolution(f), cuc_value(f),
            )),
            (c, sub) in ccs_format().prop_flat_map(|f| (ccs_in(f), ccs_subsecond_out_of_range(f))),
        ) {
            // req: VER-054, TIME-011, TIME-012, TIME-030
            prop_assert!(Cuc::new(f, CucValue::new(coarse, 0)).is_err());
            prop_assert!(Cuc::new(f, CucValue::new(ok.coarse(), fine)).is_err());
            prop_assert!(Cuc::new(f, ok).is_ok());
            let v = CcsValue { subsecond: sub, ..c.value() };
            prop_assert!(Ccs::new(c.format(), v).is_err());
        }

        #[test]
        fn ascii_times_parse_back(t in ascii_time(), noise in ascii_parser_input()) {
            // req: VER-054, TIME-031
            let text = ascii_text(&t);
            prop_assert_eq!(AsciiTime::parse(&text, &AsciiOptions::DEFAULT), Ok(t));
            let _ = AsciiTime::parse(&noise, &AsciiOptions::DEFAULT);
        }
    }
}
