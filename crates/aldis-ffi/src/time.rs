//! C ABI of the time code codec `aldis-time` (CCSDS 301.0-B-4; M2 plan D4; CABI-010…012,
//! CABI-020…022, CABI-033).
//!
//! A time code format is one flat struct, [`aldis_time_format_t`], whose `code` selects the
//! fields that apply; a value is one flat struct, [`aldis_time_value_t`], read the same way. CUC
//! fine time crosses as `uint8_t[10]`, left-aligned as on the wire, with its octet count,
//! because the C ABI has no 128-bit type (CABI-010; M1 D2). ASCII codes are octet buffers with
//! an output length and no terminating NUL. Calls: [`aldis_time_decode`],
//! [`aldis_time_decode_implicit`], [`aldis_time_encode`], [`aldis_time_pfield_parse`],
//! [`aldis_time_format_pfield`], [`aldis_time_format_t_len`], [`aldis_time_cuc_to_ns`],
//! [`aldis_time_cuc_from_ns`], [`aldis_time_ascii_parse`], [`aldis_time_ascii_format`],
//! [`aldis_time_ascii_encoded_len`], [`aldis_time_ascii_subset_parse`], and
//! [`aldis_time_convert`].

use aldis::core::field;
use aldis::core::{Error, Result};
use aldis::time::{
    Agency, AgencyFormat, AsciiFormat, AsciiOptions, AsciiSubset, AsciiTime, CalendarDate, Ccs,
    CcsFormat, CcsValue, CcsVariation, Cds, CdsDayLen, CdsEpoch, CdsFormat, CdsSubMs, CdsValue,
    Cuc, CucEpoch, CucFormat, CucValue, Decoded, Fraction, Hour, Micros, Minute, Month, MsOfDay,
    PField, PFieldTail, Picos, Second, SubMillis, TimeCode, TimeFormat, UtcInstant, Year,
    decode_implicit, decode_with_pfield,
};

use crate::abi::{self, Out, invalid_argument};

/// [`aldis_time_format_t::code`]: CCSDS Unsegmented Time Code (301.0 §3.2).
pub const ALDIS_TIME_CUC: u32 = 0;
/// [`aldis_time_format_t::code`]: CCSDS Day Segmented Time Code (301.0 §3.3).
pub const ALDIS_TIME_CDS: u32 = 1;
/// [`aldis_time_format_t::code`]: CCSDS Calendar Segmented Time Code (301.0 §3.4).
pub const ALDIS_TIME_CCS: u32 = 2;
/// [`aldis_time_format_t::code`]: agency-defined code (301.0 §3.6).
pub const ALDIS_TIME_AGENCY: u32 = 3;
/// [`aldis_time_format_t::code`] of a decoded P-field whose time code ID is reserved, decoded
/// in lenient mode (TIME-002); never accepted as an input format.
pub const ALDIS_TIME_UNKNOWN: u32 = 4;
/// [`aldis_time_format_t::code`] for [`aldis_time_convert`] only: an ASCII code A or B
/// (301.0 §3.5).
pub const ALDIS_TIME_ASCII: u32 = 5;

/// [`aldis_time_format_t::day_len`]: a 16-bit CDS day segment (301.0 §3.3.2).
pub const ALDIS_TIME_CDS_DAY_16: u8 = 0;
/// [`aldis_time_format_t::day_len`]: a 24-bit CDS day segment (301.0 §3.3.2).
pub const ALDIS_TIME_CDS_DAY_24: u8 = 1;
/// [`aldis_time_format_t::subms`]: no CDS submillisecond segment (301.0 §3.3.2).
pub const ALDIS_TIME_CDS_SUBMS_NONE: u8 = 0;
/// [`aldis_time_format_t::subms`]: microseconds of millisecond, 16 bits (301.0 §3.3.2).
pub const ALDIS_TIME_CDS_SUBMS_MICROS: u8 = 1;
/// [`aldis_time_format_t::subms`]: picoseconds of millisecond, 32 bits (301.0 §3.3.2).
pub const ALDIS_TIME_CDS_SUBMS_PICOS: u8 = 2;
/// CCS and ASCII variation: month and day of month, CCS bit 4 = 0, ASCII code A
/// (301.0 §3.4.2, §3.5.1.1).
pub const ALDIS_TIME_MONTH_DAY: u8 = 0;
/// CCS and ASCII variation: day of year, CCS bit 4 = 1, ASCII code B (301.0 §3.4.2,
/// §3.5.1.2).
pub const ALDIS_TIME_DAY_OF_YEAR: u8 = 1;

/// Largest P-field, in octets (301.0 §3.1.1).
pub const ALDIS_TIME_MAX_PFIELD_LEN: u32 = 8;
/// Largest number of CUC fine octets (301.0 §3.2.2).
pub const ALDIS_TIME_CUC_MAX_FINE: u32 = 10;
/// Largest agency-defined T-field, in octets (301.0 §3.6.2).
pub const ALDIS_TIME_MAX_AGENCY_LEN: u32 = 16;
/// Largest ASCII time code, in octets: code A with 12 fraction digits and `Z` (301.0 §3.5.1).
pub const ALDIS_TIME_MAX_ASCII_LEN: u32 = 33;

/// Anomaly bit: reserved time code ID, decoded as `ALDIS_TIME_UNKNOWN` (bit 0 of
/// `TimeAnomaly`; TIME-002, API-032).
pub const ALDIS_TIME_ANOMALY_RESERVED_TIME_CODE_ID: u32 = 1 << 0;
/// Anomaly bit: CCS day of year with a non-zero high nibble (bit 2; 301.0 §3.4.1.2, API-032).
pub const ALDIS_TIME_ANOMALY_NON_ZERO_DOY_HIGH_NIBBLE: u32 = 1 << 2;
/// Anomaly bit: a P-field extension the time code does not define (bit 3; TIME-005, API-032).
pub const ALDIS_TIME_ANOMALY_UNDEFINED_PFIELD_EXTENSION: u32 = 1 << 3;

/// [`aldis_time_ascii_subset_t::present`]: the variation (code A or B) is known.
pub const ALDIS_TIME_SUBSET_VARIATION: u32 = 1 << 0;
/// [`aldis_time_ascii_subset_t::present`]: `year` is present.
pub const ALDIS_TIME_SUBSET_YEAR: u32 = 1 << 1;
/// [`aldis_time_ascii_subset_t::present`]: `month` is present.
pub const ALDIS_TIME_SUBSET_MONTH: u32 = 1 << 2;
/// [`aldis_time_ascii_subset_t::present`]: `day` is present.
pub const ALDIS_TIME_SUBSET_DAY: u32 = 1 << 3;
/// [`aldis_time_ascii_subset_t::present`]: `day_of_year` is present.
pub const ALDIS_TIME_SUBSET_DAY_OF_YEAR: u32 = 1 << 4;
/// [`aldis_time_ascii_subset_t::present`]: `hour` is present.
pub const ALDIS_TIME_SUBSET_HOUR: u32 = 1 << 5;
/// [`aldis_time_ascii_subset_t::present`]: `minute` is present.
pub const ALDIS_TIME_SUBSET_MINUTE: u32 = 1 << 6;
/// [`aldis_time_ascii_subset_t::present`]: `second` is present.
pub const ALDIS_TIME_SUBSET_SECOND: u32 = 1 << 7;
/// [`aldis_time_ascii_subset_t::present`]: `fraction` and `fraction_digits` are present.
pub const ALDIS_TIME_SUBSET_FRACTION: u32 = 1 << 8;

/// A time code format: what a P-field carries, or an implicit format (301.0 §3; TIME-001…006,
/// CABI-033).
///
/// `code` selects the fields that apply; the others are ignored on input and 0 on output.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_time_format_t {
    /// `sizeof(aldis_time_format_t)`, set by the caller (CABI-033).
    pub struct_size: u32,
    /// `ALDIS_TIME_CUC`, `_CDS`, `_CCS`, or `_AGENCY` (`_UNKNOWN` on output only).
    pub code: u32,
    /// CUC and CDS: 1 for an agency-defined epoch (CUC Level 2), 0 for 1958 January 1 (CUC
    /// Level 1).
    pub agency_epoch: u8,
    /// CUC: octets of the basic time unit, 1 to 7.
    pub coarse_octets: u8,
    /// CUC: octets of the fractional time unit, 0 to 10.
    pub fine_octets: u8,
    /// CUC: P-field octet 2 bits 6–7, reserved for mission definition, 0 to 3.
    pub mission_bits: u8,
    /// CUC: 1 to write P-field octet 2 even when no field needs it.
    pub octet2: u8,
    /// CDS: `ALDIS_TIME_CDS_DAY_16` or `_24`.
    pub day_len: u8,
    /// CDS: `ALDIS_TIME_CDS_SUBMS_NONE`, `_MICROS`, or `_PICOS`.
    pub subms: u8,
    /// CCS: `ALDIS_TIME_MONTH_DAY` or `ALDIS_TIME_DAY_OF_YEAR`.
    pub variation: u8,
    /// CCS: number of subsecond octets, 0 to 6.
    pub subsecond_octets: u8,
    /// Agency-defined: T-field length, 1 to 16.
    pub t_len: u8,
    /// Number of P-field extension octets in `tail`, 0 to 7 (6 for CUC, after octet 2).
    pub tail_len: u8,
    /// P-field extension octets the code does not define, kept on round trip (TIME-005).
    pub tail: [u8; 7],
}

/// A time code value; `code` of its format selects the fields that apply (301.0 §3;
/// TIME-010…033). Fields that do not apply are ignored on input and 0 on output.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_time_value_t {
    /// CUC: count of basic time units.
    pub coarse: u64,
    /// CCS: subsecond value, two decimal digits per subsecond octet.
    pub subsecond: u64,
    /// CDS: day segment.
    pub day: u32,
    /// CDS: milliseconds of day, at most 86 400 999.
    pub ms_of_day: u32,
    /// CDS: microseconds (at most 999) or picoseconds (at most 999 999 999) of millisecond.
    pub subms: u32,
    /// CCS: year, 1 to 9999.
    pub year: u16,
    /// CCS, day-of-year variation: day of year.
    pub day_of_year: u16,
    /// CUC: fine time, left-aligned as on the wire; octets from `fine_len` on are 0.
    pub fine: [u8; 10],
    /// CUC: number of fine octets, 0 to 10.
    pub fine_len: u8,
    /// CCS: the date's variation, `ALDIS_TIME_MONTH_DAY` or `ALDIS_TIME_DAY_OF_YEAR`.
    pub variation: u8,
    /// CCS, month-day variation: month, 1 to 12.
    pub month: u8,
    /// CCS, month-day variation: day of month.
    pub day_of_month: u8,
    /// CCS: hour, 0 to 23.
    pub hour: u8,
    /// CCS: minute, 0 to 59.
    pub minute: u8,
    /// CCS: second, 0 to 60.
    pub second: u8,
    /// Agency-defined: number of T-field octets in `tfield`.
    pub tfield_len: u8,
    /// Agency-defined: the T-field.
    pub tfield: [u8; 16],
}

/// A decoded time code: its format, value, P-field, and length (301.0 §3; TIME-001…006,
/// API-010, API-032).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_time_decoded_t {
    /// The format, from the P-field or as given; `code` is `ALDIS_TIME_UNKNOWN` for a reserved
    /// time code ID decoded in lenient mode.
    pub format: aldis_time_format_t,
    /// The value; all 0 for `ALDIS_TIME_UNKNOWN`.
    pub value: aldis_time_value_t,
    /// The P-field octets as received; `pfield_len` is 0 for an implicit P-field.
    pub pfield: [u8; 8],
    /// Number of P-field octets, 0 to 8.
    pub pfield_len: u8,
    /// Lenient-mode anomaly flags, `ALDIS_TIME_ANOMALY_*`; 0 in strict mode.
    pub anomalies: u32,
    /// Octets consumed: P-field and T-field.
    pub len: usize,
}

/// A parsed P-field (301.0 §3.1.1; TIME-001, TIME-002, TIME-005).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_time_pfield_t {
    /// The octets, first to last.
    pub octets: [u8; 8],
    /// Number of octets, 1 to 8.
    pub len: u8,
    /// Time code identification, octet 1 bits 1–3.
    pub time_code_id: u8,
    /// Whether the time code ID is one CCSDS reserves.
    pub reserved: bool,
}

/// A calendar date (301.0 §3.4.1; TIME-033).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_time_date_t {
    /// Year, 1 to 9999.
    pub year: u16,
    /// Month, 1 to 12.
    pub month: u8,
    /// Day of month.
    pub day: u8,
}

/// A complete ASCII time code A or B (301.0 §3.5.1.1–2; TIME-031).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_time_ascii_t {
    /// Decimal fraction of the second, as an integer of `fraction_digits` digits.
    pub fraction: u64,
    /// Year, 1 to 9999.
    pub year: u16,
    /// Code B: day of year.
    pub day_of_year: u16,
    /// `ALDIS_TIME_MONTH_DAY` (code A) or `ALDIS_TIME_DAY_OF_YEAR` (code B).
    pub variation: u8,
    /// Code A: month.
    pub month: u8,
    /// Code A: day of month.
    pub day: u8,
    /// Hour, 0 to 23.
    pub hour: u8,
    /// Minute, 0 to 59.
    pub minute: u8,
    /// Second, 0 to 60.
    pub second: u8,
    /// Number of fraction digits, 0 to 12; 0 writes no period.
    pub fraction_digits: u8,
    /// 1 if the code ends with `Z`.
    pub utc: u8,
}

/// An ASCII code or subset of 301.0 §3.5.1.3: every field present is valid (TIME-032).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_time_ascii_subset_t {
    /// Which fields are present: `ALDIS_TIME_SUBSET_*` bits.
    pub present: u32,
    /// Decimal fraction of the second.
    pub fraction: u64,
    /// Year.
    pub year: u16,
    /// Day of year.
    pub day_of_year: u16,
    /// `ALDIS_TIME_MONTH_DAY` (code A) or `ALDIS_TIME_DAY_OF_YEAR` (code B).
    pub variation: u8,
    /// Month.
    pub month: u8,
    /// Day of month.
    pub day: u8,
    /// Hour.
    pub hour: u8,
    /// Minute.
    pub minute: u8,
    /// Second.
    pub second: u8,
    /// Number of fraction digits.
    pub fraction_digits: u8,
    /// Whether the code ends with `Z`.
    pub utc: bool,
}

/// Options of the ASCII parsers (301.0 §3.5.1; TIME-031, CABI-033).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_time_ascii_options_t {
    /// `sizeof(aldis_time_ascii_options_t)`, set by the caller (CABI-033).
    pub struct_size: u32,
    /// Largest number of fraction digits accepted, 0 to 12.
    pub max_fraction_digits: u8,
}

/// The layout of an ASCII code to produce (301.0 §3.5.1; TIME-031, TIME-033).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_time_ascii_format_t {
    /// `ALDIS_TIME_MONTH_DAY` (code A) or `ALDIS_TIME_DAY_OF_YEAR` (code B).
    pub variation: u8,
    /// Fraction digits, 0 to 12.
    pub fraction_digits: u8,
    /// 1 to end the code with `Z`.
    pub utc: u8,
}

/// A time to convert, or a converted time: a CDS or CCS code (`format` and `value`), or an
/// ASCII code (`format.code` = `ALDIS_TIME_ASCII`, and `ascii`) (TIME-033).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_time_instant_t {
    /// The code: `ALDIS_TIME_CDS`, `_CCS`, or `_ASCII`, and for CDS and CCS its format.
    pub format: aldis_time_format_t,
    /// CDS and CCS: the value.
    pub value: aldis_time_value_t,
    /// ASCII: the code.
    pub ascii: aldis_time_ascii_t,
}

/// What [`aldis_time_convert`] produces (TIME-033, CABI-033).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_time_convert_config_t {
    /// `sizeof(aldis_time_convert_config_t)`, set by the caller (CABI-033).
    pub struct_size: u32,
    /// The target: a CDS or CCS format, or `code` = `ALDIS_TIME_ASCII`.
    pub to: aldis_time_format_t,
    /// For an ASCII target: its layout.
    pub ascii: aldis_time_ascii_format_t,
    /// 1 if `agency_epoch` gives the epoch of agency-epoch CDS codes, on either side.
    pub has_agency_epoch: u8,
    /// The agency-defined epoch of a CDS code (301.0 §3.3.2).
    pub agency_epoch: aldis_time_date_t,
}

const fn variation(value: u8) -> Result<CcsVariation> {
    match value {
        ALDIS_TIME_MONTH_DAY => Ok(CcsVariation::MonthDay),
        ALDIS_TIME_DAY_OF_YEAR => Ok(CcsVariation::DayOfYear),
        _ => Err(invalid_argument()),
    }
}

const fn variation_code(v: CcsVariation) -> u8 {
    match v {
        CcsVariation::MonthDay => ALDIS_TIME_MONTH_DAY,
        CcsVariation::DayOfYear => ALDIS_TIME_DAY_OF_YEAR,
    }
}

fn tail(f: &aldis_time_format_t) -> Result<PFieldTail> {
    PFieldTail::new(
        f.tail
            .get(..usize::from(f.tail_len))
            .ok_or_else(invalid_argument)?,
    )
}

/// The time format of a C struct, each characteristic parsed into its type (RS-020).
pub(crate) fn time_format(f: &aldis_time_format_t) -> Result<TimeFormat> {
    if usize::try_from(f.struct_size).map_or(true, |n| n < size_of::<aldis_time_format_t>()) {
        return Err(invalid_argument());
    }
    let epoch = abi::flag(f.agency_epoch)?;
    Ok(match f.code {
        ALDIS_TIME_CUC => {
            let level = if epoch {
                CucEpoch::Agency
            } else {
                CucEpoch::Tai1958
            };
            let mut c = CucFormat::new(level, f.coarse_octets, f.fine_octets)?;
            if f.mission_bits != 0 {
                c = c.with_mission_bits(f.mission_bits)?;
            }
            if abi::flag(f.octet2)? {
                c = c.with_octet2();
            }
            let t = tail(f)?;
            if !t.is_empty() {
                c = c.with_tail(t)?;
            }
            TimeFormat::Cuc(c)
        }
        ALDIS_TIME_CDS => {
            let day = match f.day_len {
                ALDIS_TIME_CDS_DAY_16 => CdsDayLen::Bits16,
                ALDIS_TIME_CDS_DAY_24 => CdsDayLen::Bits24,
                _ => return Err(invalid_argument()),
            };
            let subms = match f.subms {
                ALDIS_TIME_CDS_SUBMS_NONE => CdsSubMs::None,
                ALDIS_TIME_CDS_SUBMS_MICROS => CdsSubMs::Micros,
                ALDIS_TIME_CDS_SUBMS_PICOS => CdsSubMs::Picos,
                _ => return Err(invalid_argument()),
            };
            let epoch = if epoch {
                CdsEpoch::Agency
            } else {
                CdsEpoch::Ccsds1958
            };
            TimeFormat::Cds(CdsFormat::new(epoch, day, subms).with_tail(tail(f)?))
        }
        ALDIS_TIME_CCS => TimeFormat::Ccs(
            CcsFormat::new(variation(f.variation)?, f.subsecond_octets)?.with_tail(tail(f)?),
        ),
        ALDIS_TIME_AGENCY => TimeFormat::Agency(AgencyFormat::new(f.t_len)?.with_tail(tail(f)?)),
        _ => return Err(invalid_argument()),
    })
}

fn tail_out(t: PFieldTail, out: &mut aldis_time_format_t) {
    let bytes = t.as_bytes();
    for (d, s) in out.tail.iter_mut().zip(bytes) {
        *d = *s;
    }
    out.tail_len = u8::try_from(bytes.len()).unwrap_or(0);
}

const fn base_format(code: u32) -> aldis_time_format_t {
    aldis_time_format_t {
        struct_size: FORMAT_SIZE,
        code,
        agency_epoch: 0,
        coarse_octets: 0,
        fine_octets: 0,
        mission_bits: 0,
        octet2: 0,
        day_len: 0,
        subms: 0,
        variation: 0,
        subsecond_octets: 0,
        t_len: 0,
        tail_len: 0,
        tail: [0; 7],
    }
}

/// `sizeof(aldis_time_format_t)`, which every written format carries.
const FORMAT_SIZE: u32 = {
    let n = size_of::<aldis_time_format_t>();
    assert!(n <= 0xFFFF, "a format struct fits its struct_size");
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "checked above to fit"
    )]
    let size = n as u32;
    size
};

/// The C struct of a time format.
pub(crate) fn format_out(f: &TimeFormat) -> aldis_time_format_t {
    match f {
        TimeFormat::Cuc(c) => {
            let mut out = aldis_time_format_t {
                agency_epoch: u8::from(c.epoch() == CucEpoch::Agency),
                coarse_octets: c.coarse_octets(),
                fine_octets: c.fine_octets(),
                mission_bits: c.mission_bits(),
                octet2: u8::from(c.has_octet2()),
                ..base_format(ALDIS_TIME_CUC)
            };
            tail_out(c.tail(), &mut out);
            out
        }
        TimeFormat::Cds(c) => {
            let mut out = aldis_time_format_t {
                agency_epoch: u8::from(c.epoch() == CdsEpoch::Agency),
                day_len: match c.day_len() {
                    CdsDayLen::Bits16 => ALDIS_TIME_CDS_DAY_16,
                    CdsDayLen::Bits24 => ALDIS_TIME_CDS_DAY_24,
                },
                subms: match c.subms() {
                    CdsSubMs::None => ALDIS_TIME_CDS_SUBMS_NONE,
                    CdsSubMs::Micros => ALDIS_TIME_CDS_SUBMS_MICROS,
                    CdsSubMs::Picos => ALDIS_TIME_CDS_SUBMS_PICOS,
                },
                ..base_format(ALDIS_TIME_CDS)
            };
            tail_out(c.tail(), &mut out);
            out
        }
        TimeFormat::Ccs(c) => {
            let mut out = aldis_time_format_t {
                variation: variation_code(c.variation()),
                subsecond_octets: c.subsecond_octets(),
                ..base_format(ALDIS_TIME_CCS)
            };
            tail_out(c.tail(), &mut out);
            out
        }
        TimeFormat::Agency(c) => {
            let mut out = aldis_time_format_t {
                t_len: u8::try_from(c.t_len()).unwrap_or(0),
                ..base_format(ALDIS_TIME_AGENCY)
            };
            tail_out(c.tail(), &mut out);
            out
        }
    }
}

/// CUC fine time from its C octets: left-aligned, with no octet set from `fine_len` on.
fn fine_in(v: &aldis_time_value_t) -> Result<u128> {
    let n = usize::from(v.fine_len);
    let (used, rest) = v.fine.split_at_checked(n).ok_or_else(invalid_argument)?;
    if rest.iter().any(|&b| b != 0) {
        return Err(invalid_argument());
    }
    let mut octets = [0_u8; 16];
    for (d, s) in octets.iter_mut().zip(used) {
        *d = *s;
    }
    Ok(u128::from_be_bytes(octets))
}

fn date_in(variation: u8, year: u16, month: u8, day: u8, day_of_year: u16) -> Result<CalendarDate> {
    let year = Year::try_from(year)?;
    match self::variation(variation)? {
        CcsVariation::MonthDay => CalendarDate::from_month_day(year, Month::try_from(month)?, day),
        CcsVariation::DayOfYear => CalendarDate::from_day_of_year(year, day_of_year),
    }
}

/// The time code of a C format and value, each field parsed into its type (RS-020).
pub(crate) fn code_in(format: TimeFormat, v: &aldis_time_value_t) -> Result<TimeCode> {
    Ok(match format {
        TimeFormat::Cuc(f) => TimeCode::Cuc(Cuc::new(f, CucValue::new(v.coarse, fine_in(v)?))?),
        TimeFormat::Cds(f) => {
            let subms = match f.subms() {
                CdsSubMs::None => SubMillis::None,
                CdsSubMs::Micros => SubMillis::Micros(Micros::try_from(
                    u16::try_from(v.subms).unwrap_or(u16::MAX),
                )?),
                CdsSubMs::Picos => SubMillis::Picos(Picos::try_from(v.subms)?),
            };
            TimeCode::Cds(Cds::new(
                f,
                CdsValue {
                    day: v.day,
                    ms_of_day: MsOfDay::try_from(v.ms_of_day)?,
                    subms,
                },
            )?)
        }
        TimeFormat::Ccs(f) => {
            let date = date_in(v.variation, v.year, v.month, v.day_of_month, v.day_of_year)?;
            TimeCode::Ccs(Ccs::new(
                f,
                CcsValue {
                    date,
                    hour: Hour::try_from(v.hour)?,
                    minute: Minute::try_from(v.minute)?,
                    second: Second::try_from(v.second)?,
                    subsecond: v.subsecond,
                },
            )?)
        }
        TimeFormat::Agency(f) => {
            let t = v
                .tfield
                .get(..usize::from(v.tfield_len))
                .ok_or_else(invalid_argument)?;
            TimeCode::Agency(Agency::new(f, t)?)
        }
    })
}

fn date_out(date: CalendarDate, out: &mut aldis_time_value_t) {
    out.year = date.year().get();
    out.variation = variation_code(date.variation());
    if let Some((month, day)) = date.month_day() {
        out.month = month.get();
        out.day_of_month = day;
    }
    out.day_of_year = date.day_of_year().unwrap_or(0);
}

/// The C struct of a time value.
pub(crate) fn value_out(code: &TimeCode) -> aldis_time_value_t {
    let mut out = aldis_time_value_t::default();
    match code {
        TimeCode::Cuc(c) => {
            out.coarse = c.value().coarse();
            for (d, s) in out.fine.iter_mut().zip(c.value().fine().to_be_bytes()) {
                *d = s;
            }
            out.fine_len = c.format().fine_octets();
        }
        TimeCode::Cds(c) => {
            let v = c.value();
            out.day = v.day;
            out.ms_of_day = v.ms_of_day.get();
            out.subms = v.subms.get();
        }
        TimeCode::Ccs(c) => {
            let v = c.value();
            date_out(v.date, &mut out);
            out.hour = v.hour.get();
            out.minute = v.minute.get();
            out.second = v.second.get();
            out.subsecond = v.subsecond;
        }
        TimeCode::Agency(c) => {
            let t = c.tfield();
            for (d, s) in out.tfield.iter_mut().zip(t) {
                *d = *s;
            }
            out.tfield_len = u8::try_from(t.len()).unwrap_or(0);
        }
        // A reserved time code ID: no value (TIME-002).
        TimeCode::Unknown(_) => {}
    }
    out
}

fn pfield_octets(p: &PField) -> ([u8; 8], u8) {
    let mut octets = [0_u8; 8];
    for (d, s) in octets
        .iter_mut()
        .zip(core::iter::once(&p.first()).chain(p.rest()))
    {
        *d = *s;
    }
    (octets, u8::try_from(p.len()).unwrap_or(0))
}

/// The C struct of a decoded time code.
pub(crate) fn decoded(d: &Decoded) -> aldis_time_decoded_t {
    let code = d.code();
    let (pfield, pfield_len) = d.pfield().map_or(([0; 8], 0), |p| pfield_octets(&p));
    aldis_time_decoded_t {
        format: code
            .format()
            .map_or(base_format(ALDIS_TIME_UNKNOWN), |f| format_out(&f)),
        value: value_out(&code),
        pfield,
        pfield_len,
        anomalies: d.anomalies().bits(),
        len: d.len(),
    }
}

fn fraction_in(value: u64, digits: u8) -> Result<Fraction> {
    if digits > aldis::time::MAX_FRACTION_DIGITS {
        return Err(Error::out_of_range(
            field::FRACTION_DIGITS,
            u64::from(aldis::time::MAX_FRACTION_DIGITS),
            u64::from(digits),
        ));
    }
    Fraction::new(value, digits).ok_or_else(|| {
        let limit = 10_u64
            .checked_pow(u32::from(digits))
            .map_or(u64::MAX, |p| p.saturating_sub(1));
        Error::out_of_range(field::FRACTION, limit, value)
    })
}

/// The ASCII code of a C struct, each field parsed into its type (RS-020).
fn ascii_in(a: &aldis_time_ascii_t) -> Result<AsciiTime> {
    Ok(AsciiTime {
        date: date_in(a.variation, a.year, a.month, a.day, a.day_of_year)?,
        hour: Hour::try_from(a.hour)?,
        minute: Minute::try_from(a.minute)?,
        second: Second::try_from(a.second)?,
        fraction: fraction_in(a.fraction, a.fraction_digits)?,
        utc: abi::flag(a.utc)?,
    })
}

fn ascii_out(t: &AsciiTime) -> aldis_time_ascii_t {
    let (month, day) = t.date.month_day().map_or((0, 0), |(m, d)| (m.get(), d));
    aldis_time_ascii_t {
        fraction: t.fraction.value(),
        year: t.date.year().get(),
        day_of_year: t.date.day_of_year().unwrap_or(0),
        variation: variation_code(t.date.variation()),
        month,
        day,
        hour: t.hour.get(),
        minute: t.minute.get(),
        second: t.second.get(),
        fraction_digits: t.fraction.digits(),
        utc: u8::from(t.utc),
    }
}

fn subset_out(s: &AsciiSubset) -> aldis_time_ascii_subset_t {
    let mut out = aldis_time_ascii_subset_t {
        utc: s.utc(),
        ..aldis_time_ascii_subset_t::default()
    };
    let mut set = |bit: u32, present: bool| {
        if present {
            out.present |= bit;
        }
    };
    set(ALDIS_TIME_SUBSET_VARIATION, s.variation().is_some());
    set(ALDIS_TIME_SUBSET_YEAR, s.year().is_some());
    set(ALDIS_TIME_SUBSET_MONTH, s.month().is_some());
    set(ALDIS_TIME_SUBSET_DAY, s.day().is_some());
    set(ALDIS_TIME_SUBSET_DAY_OF_YEAR, s.day_of_year().is_some());
    set(ALDIS_TIME_SUBSET_HOUR, s.hour().is_some());
    set(ALDIS_TIME_SUBSET_MINUTE, s.minute().is_some());
    set(ALDIS_TIME_SUBSET_SECOND, s.second().is_some());
    set(ALDIS_TIME_SUBSET_FRACTION, s.fraction().is_some());
    out.variation = s.variation().map_or(0, variation_code);
    out.year = s.year().map_or(0, Year::get);
    out.month = s.month().map_or(0, Month::get);
    out.day = s.day().unwrap_or(0);
    out.day_of_year = s.day_of_year().unwrap_or(0);
    out.hour = s.hour().map_or(0, Hour::get);
    out.minute = s.minute().map_or(0, Minute::get);
    out.second = s.second().map_or(0, Second::get);
    out.fraction = s.fraction().map_or(0, |f| f.value());
    out.fraction_digits = s.fraction().map_or(0, |f| f.digits());
    out
}

const fn options(o: aldis_time_ascii_options_t) -> Result<AsciiOptions> {
    AsciiOptions::new(o.max_fraction_digits)
}

fn convert(
    from: &aldis_time_instant_t,
    cfg: &aldis_time_convert_config_t,
) -> Result<aldis_time_instant_t> {
    let epoch = if abi::flag(cfg.has_agency_epoch)? {
        let e = cfg.agency_epoch;
        Some(date_in(ALDIS_TIME_MONTH_DAY, e.year, e.month, e.day, 0)?)
    } else {
        None
    };
    let instant = if from.format.code == ALDIS_TIME_ASCII {
        UtcInstant::from_ascii(&ascii_in(&from.ascii)?)
    } else {
        match code_in(time_format(&from.format)?, &from.value)? {
            TimeCode::Cds(c) => UtcInstant::from_cds(&c, epoch)?,
            TimeCode::Ccs(c) => UtcInstant::from_ccs(&c),
            _ => return Err(invalid_argument()),
        }
    };
    if cfg.to.code == ALDIS_TIME_ASCII {
        let a = cfg.ascii;
        let format = AsciiFormat::new(
            variation(a.variation)?,
            a.fraction_digits,
            abi::flag(a.utc)?,
        )?;
        return Ok(aldis_time_instant_t {
            format: base_format(ALDIS_TIME_ASCII),
            ascii: ascii_out(&instant.to_ascii(format)?),
            ..aldis_time_instant_t::default()
        });
    }
    let code = match time_format(&cfg.to)? {
        TimeFormat::Cds(f) => TimeCode::Cds(instant.to_cds(f, epoch)?),
        TimeFormat::Ccs(f) => TimeCode::Ccs(instant.to_ccs(f)?),
        _ => return Err(invalid_argument()),
    };
    Ok(aldis_time_instant_t {
        format: code
            .format()
            .map_or(base_format(ALDIS_TIME_UNKNOWN), |f| format_out(&f)),
        value: value_out(&code),
        ..aldis_time_instant_t::default()
    })
}

fn write_octets(octets: &[u8], out: &mut [u8], out_len: Out<'_, usize>) -> Result<()> {
    let dst = out
        .get_mut(..octets.len())
        .ok_or(Error::buffer_too_small(octets.len()))?;
    dst.copy_from_slice(octets);
    out_len.write(octets.len());
    Ok(())
}

/// Decodes a time code that starts with its P-field (301.0 §3.1; TIME-001…006, TIME-010…030,
/// API-030).
///
/// # Errors
/// `ALDIS_ERR_INCOMPLETE` (with `needed`), `ALDIS_ERR_MALFORMED` (`pfield`) for an extension
/// chain longer than 8 octets, `ALDIS_ERR_RESERVED_VALUE` in strict mode (`time_code_id`,
/// `pfield`, `subms`, `subsecond_octets`, `day_of_year`), and the value errors of each code
/// (`ms_of_day`, `subms`, BCD digits); CABI-022 violations.
///
/// # Safety
/// `bytes` is valid for `len` octets (or `NULL` with 0); `out` is valid for one write; `err` is
/// `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_time_decode(
    bytes: *const u8,
    len: usize,
    mode: u32,
    out: *mut aldis_time_decoded_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let bytes = abi::bytes(bytes, len)?;
            let mode = abi::mode(mode)?;
            let out = abi::out(out)?;
            out.write(decoded(&decode_with_pfield(bytes, mode)?));
            Ok(())
        })
    }
}

/// Decodes the T-field of a time code whose format is implicit (301.0 §3.1; TIME-003).
///
/// # Errors
/// As [`aldis_time_decode`], and the errors of the format; CABI-022 violations.
///
/// # Safety
/// `bytes` is valid for `len` octets (or `NULL` with 0); `format` is valid for a read; `out` is
/// valid for one write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_time_decode_implicit(
    bytes: *const u8,
    len: usize,
    format: *const aldis_time_format_t,
    mode: u32,
    out: *mut aldis_time_decoded_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let bytes = abi::bytes(bytes, len)?;
            let format = time_format(&abi::config(format)?)?;
            let mode = abi::mode(mode)?;
            let out = abi::out(out)?;
            out.write(decoded(&decode_implicit(bytes, &format, mode)?));
            Ok(())
        })
    }
}

/// Encodes a time code, with its P-field when `include_pfield` (301.0 §3; TIME-001…033,
/// API-020…022).
///
/// # Errors
/// The errors of the format and value: `ALDIS_ERR_OUT_OF_RANGE` naming the field (`coarse`,
/// `fine`, `cds_day`, `ms_of_day`, `subms`, `year`, `month`, `day`, `day_of_year`, `hour`,
/// `minute`, `second`, `subsecond`), `ALDIS_ERR_INVALID_FIELD` (`subms`, `date`),
/// `ALDIS_ERR_LENGTH_MISMATCH` (`tfield`); `ALDIS_ERR_BUFFER_TOO_SMALL`; CABI-022 violations.
///
/// # Safety
/// `format` and `value` are valid for reads; `out` is valid for `cap` octets; `out_len` is valid
/// for one write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_time_encode(
    format: *const aldis_time_format_t,
    value: *const aldis_time_value_t,
    include_pfield: bool,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let format = time_format(&abi::config(format)?)?;
            let value = abi::read(value)?;
            let out = abi::bytes_mut(out, cap, &[])?;
            let out_len = abi::out(out_len)?;
            let code = code_in(format, &value)?;
            out_len.write(code.encode(include_pfield, out)?);
            Ok(())
        })
    }
}

/// Parses the P-field that `bytes` begins with (301.0 §3.1.1; TIME-001, TIME-005).
///
/// # Errors
/// `ALDIS_ERR_INCOMPLETE` (with `needed`), `ALDIS_ERR_MALFORMED` (`pfield`); CABI-022
/// violations.
///
/// # Safety
/// `bytes` is valid for `len` octets (or `NULL` with 0); `out` is valid for one write; `err` is
/// `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_time_pfield_parse(
    bytes: *const u8,
    len: usize,
    out: *mut aldis_time_pfield_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let bytes = abi::bytes(bytes, len)?;
            let out = abi::out(out)?;
            let p = PField::parse(bytes)?;
            let (octets, len) = pfield_octets(&p);
            out.write(aldis_time_pfield_t {
                octets,
                len,
                time_code_id: p.id().bits(),
                reserved: p.id().is_reserved(),
            });
            Ok(())
        })
    }
}

/// The P-field octets of a format (301.0 §3.1.1; TIME-001, TIME-005).
///
/// # Errors
/// The errors of the format, `ALDIS_ERR_BUFFER_TOO_SMALL`; CABI-022 violations.
///
/// # Safety
/// `format` is valid for a read; `out` is valid for `cap` octets; `out_len` is valid for one
/// write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_time_format_pfield(
    format: *const aldis_time_format_t,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let format = time_format(&abi::config(format)?)?;
            let out = abi::bytes_mut(out, cap, &[])?;
            let out_len = abi::out(out_len)?;
            let (octets, n) = pfield_octets(&format.pfield());
            write_octets(
                octets.get(..usize::from(n)).unwrap_or_default(),
                out,
                out_len,
            )
        })
    }
}

/// The T-field length of a format in octets (301.0 §3; TIME-003).
///
/// # Errors
/// The errors of the format; CABI-022 violations.
///
/// # Safety
/// `format` is valid for a read; `t_len` is valid for one write; `err` is `NULL` or valid for
/// one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_time_format_t_len(
    format: *const aldis_time_format_t,
    t_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let format = time_format(&abi::config(format)?)?;
            abi::out(t_len)?.write(format.t_len());
            Ok(())
        })
    }
}

/// CUC time as seconds and nanoseconds, the fraction truncated (301.0 §3.2; TIME-011). Only
/// `coarse`, `fine`, and `fine_len` of `value` are read.
///
/// # Errors
/// CABI-022 violations, including a `fine_len` above 10 or fine octets set from `fine_len` on.
///
/// # Safety
/// `value` is valid for a read; `seconds` and `nanoseconds` are valid for one write each; `err`
/// is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_time_cuc_to_ns(
    value: *const aldis_time_value_t,
    seconds: *mut u64,
    nanoseconds: *mut u32,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let value = abi::read(value)?;
            let seconds = abi::out(seconds)?;
            let nanoseconds = abi::out(nanoseconds)?;
            let (s, ns) = CucValue::new(value.coarse, fine_in(&value)?).to_seconds_nanos();
            seconds.write(s);
            nanoseconds.write(ns);
            Ok(())
        })
    }
}

/// CUC time of a CUC format for seconds and nanoseconds, the fraction rounded up to the
/// format's resolution (301.0 §3.2; TIME-011, TIME-012).
///
/// # Errors
/// `ALDIS_ERR_OUT_OF_RANGE` (`nanoseconds`) at 10⁹ or more, or (`coarse`) when the seconds do
/// not fit; the errors of the format; CABI-022 violations, including a format that is not CUC.
///
/// # Safety
/// `format` is valid for a read; `out` is valid for one write; `err` is `NULL` or valid for one
/// write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_time_cuc_from_ns(
    format: *const aldis_time_format_t,
    seconds: u64,
    nanoseconds: u32,
    out: *mut aldis_time_value_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let TimeFormat::Cuc(format) = time_format(&abi::config(format)?)? else {
                return Err(invalid_argument());
            };
            let out = abi::out(out)?;
            let cuc = Cuc::from_seconds_nanos(format, seconds, nanoseconds)?;
            out.write(value_out(&TimeCode::Cuc(cuc)));
            Ok(())
        })
    }
}

/// Parses a complete ASCII code A or B; `text` must be exactly the code, without a NUL
/// (301.0 §3.5.1.1–2; TIME-031).
///
/// # Errors
/// `ALDIS_ERR_MALFORMED` for a separator, length, or trailing-octet problem,
/// `ALDIS_ERR_INVALID_FIELD` naming a subfield that holds a non-digit, `ALDIS_ERR_OUT_OF_RANGE`
/// naming a value outside its range, or `fraction` for more digits than `options` allow;
/// CABI-022 violations.
///
/// # Safety
/// `text` is valid for `len` octets (or `NULL` with 0); `options` is valid for a read; `out` is
/// valid for one write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_time_ascii_parse(
    text: *const u8,
    len: usize,
    options: *const aldis_time_ascii_options_t,
    out: *mut aldis_time_ascii_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let text = abi::bytes(text, len)?;
            let options = self::options(abi::config(options)?)?;
            let out = abi::out(out)?;
            out.write(ascii_out(&AsciiTime::parse(text, &options)?));
            Ok(())
        })
    }
}

/// Writes a complete ASCII code A or B, without a NUL (301.0 §3.5.1; TIME-031, API-022).
///
/// # Errors
/// The errors of the fields (`year`, `month`, `day`, `day_of_year`, `hour`, `minute`,
/// `second`, `fraction`, `fraction_digits`), `ALDIS_ERR_BUFFER_TOO_SMALL` (with `needed`);
/// CABI-022 violations.
///
/// # Safety
/// `time` is valid for a read; `out` is valid for `cap` octets; `out_len` is valid for one
/// write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_time_ascii_format(
    time: *const aldis_time_ascii_t,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let time = ascii_in(&abi::read(time)?)?;
            let out = abi::bytes_mut(out, cap, &[])?;
            let out_len = abi::out(out_len)?;
            out_len.write(time.encode(out)?);
            Ok(())
        })
    }
}

/// The length in octets that [`aldis_time_ascii_format`] writes for `time`, at most
/// `ALDIS_TIME_MAX_ASCII_LEN` (301.0 §3.5.1; TIME-031).
///
/// # Errors
/// The errors of the fields; CABI-022 violations.
///
/// # Safety
/// `time` is valid for a read; `out_len` is valid for one write; `err` is `NULL` or valid for
/// one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_time_ascii_encoded_len(
    time: *const aldis_time_ascii_t,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let time = ascii_in(&abi::read(time)?)?;
            abi::out(out_len)?.write(time.encoded_len());
            Ok(())
        })
    }
}

/// Parses a complete ASCII code or any subset of 301.0 §3.5.1.3; `text` must be exactly the
/// code (TIME-032).
///
/// # Errors
/// `ALDIS_ERR_MALFORMED` for a shape §5.1 of time-codes.md does not allow,
/// `ALDIS_ERR_INVALID_FIELD`, `ALDIS_ERR_OUT_OF_RANGE` as [`aldis_time_ascii_parse`];
/// CABI-022 violations.
///
/// # Safety
/// `text` is valid for `len` octets (or `NULL` with 0); `options` is valid for a read; `out` is
/// valid for one write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_time_ascii_subset_parse(
    text: *const u8,
    len: usize,
    options: *const aldis_time_ascii_options_t,
    out: *mut aldis_time_ascii_subset_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let text = abi::bytes(text, len)?;
            let options = self::options(abi::config(options)?)?;
            let out = abi::out(out)?;
            out.write(subset_out(&AsciiSubset::parse(text, &options)?));
            Ok(())
        })
    }
}

/// Converts a CDS, CCS, or ASCII time to another of the three through UTC, exactly where the
/// target's resolution allows and truncated otherwise (301.0 §3.3–3.5; TIME-033).
///
/// # Errors
/// `ALDIS_ERR_INVALID_CONFIG` (`agency_epoch`) for an agency-epoch CDS code without
/// `has_agency_epoch`; `ALDIS_ERR_OUT_OF_RANGE` (`cds_day`) before the CDS epoch, (`second`) for
/// a leap second that is not 23:59:60, (`year`) outside 1 to 9999; the errors of the formats and
/// values; CABI-022 violations.
///
/// # Safety
/// `from` and `cfg` are valid for reads; `out` is valid for one write; `err` is `NULL` or valid
/// for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_time_convert(
    from: *const aldis_time_instant_t,
    cfg: *const aldis_time_convert_config_t,
    out: *mut aldis_time_instant_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let from = abi::read(from)?;
            let cfg = abi::config(cfg)?;
            let out = abi::out(out)?;
            out.write(convert(&from, &cfg)?);
            Ok(())
        })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use aldis::core::Anomaly as _;
    use aldis::core::ErrorKind;
    use aldis::time::TimeAnomaly;

    /// A CABI-022 rejection.
    const ARG: Error = Error::new(ErrorKind::InvalidField);

    #[test]
    fn anomaly_constants_are_the_flag_bits() {
        // req: GEN-043, API-033
        assert_eq!(
            ALDIS_TIME_ANOMALY_RESERVED_TIME_CODE_ID,
            1 << TimeAnomaly::ReservedTimeCodeId.bit()
        );
        assert_eq!(
            ALDIS_TIME_ANOMALY_NON_ZERO_DOY_HIGH_NIBBLE,
            1 << TimeAnomaly::NonZeroDoyHighNibble.bit()
        );
        assert_eq!(
            ALDIS_TIME_ANOMALY_UNDEFINED_PFIELD_EXTENSION,
            1 << TimeAnomaly::UndefinedPFieldExtension.bit()
        );
    }

    #[test]
    fn limits_are_the_crate_values() {
        // req: CABI-010
        assert_eq!(
            usize::try_from(ALDIS_TIME_MAX_PFIELD_LEN).ok(),
            Some(aldis::time::pfield::MAX_PFIELD_LEN)
        );
        assert_eq!(
            usize::try_from(ALDIS_TIME_MAX_ASCII_LEN).ok(),
            Some(aldis::time::MAX_ASCII_LEN)
        );
        assert_eq!(u32::from(CucFormat::MAX_FINE), ALDIS_TIME_CUC_MAX_FINE);
        assert_eq!(
            u32::from(aldis::time::code::MAX_AGENCY_LEN),
            ALDIS_TIME_MAX_AGENCY_LEN
        );
        assert_eq!(
            usize::try_from(FORMAT_SIZE).ok(),
            Some(size_of::<aldis_time_format_t>())
        );
    }

    #[test]
    fn configurations_start_with_struct_size() {
        // req: CABI-033
        assert_eq!(core::mem::offset_of!(aldis_time_format_t, struct_size), 0);
        assert_eq!(
            core::mem::offset_of!(aldis_time_ascii_options_t, struct_size),
            0
        );
        assert_eq!(
            core::mem::offset_of!(aldis_time_convert_config_t, struct_size),
            0
        );
        let small = aldis_time_format_t {
            struct_size: 4,
            ..base_format(ALDIS_TIME_CDS)
        };
        assert_eq!(time_format(&small), Err(ARG));
    }

    #[test]
    fn fine_octets_beyond_the_count_are_rejected() {
        // req: CABI-022, CABI-010
        let mut v = aldis_time_value_t {
            fine_len: 1,
            ..aldis_time_value_t::default()
        };
        v.fine[0] = 0x80;
        assert_eq!(fine_in(&v), Ok(1_u128 << 127));
        v.fine[1] = 1;
        assert_eq!(fine_in(&v), Err(ARG));
        v.fine_len = 11;
        assert_eq!(fine_in(&v), Err(ARG));
    }

    #[test]
    fn fractions_are_checked() {
        // req: TIME-031, CABI-022
        assert_eq!(fraction_in(5, 1).map(|f| f.value()), Ok(5));
        assert_eq!(
            fraction_in(10, 1),
            Err(Error::out_of_range(field::FRACTION, 9, 10))
        );
        assert_eq!(
            fraction_in(0, 13),
            Err(Error::out_of_range(field::FRACTION_DIGITS, 12, 13))
        );
    }
}
