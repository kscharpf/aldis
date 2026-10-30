//! Calls every public `aldis-time` API (VER-010, M1 D7) and runs every `test-vectors/time/`
//! case (`vector`). Inputs pass through `black_box`.

use core::hint::black_box as bb;

use aldis_core::{AnomalySet, DecodeMode, ErrorKind, Lenient};
use aldis_time::{
    Agency, AgencyFormat, AsciiFormat, AsciiOptions, AsciiSubset, AsciiTime, CalendarDate, Ccs,
    CcsFormat, CcsValue, CcsVariation, Cds, CdsDayLen, CdsEpoch, CdsFormat, CdsSubMs, CdsValue,
    Cuc, CucEpoch, CucFormat, CucValue, Decoded, Fraction, Hour, MAX_ASCII_LEN,
    MAX_FRACTION_DIGITS, Micros, Minute, Month, MsOfDay, PField, PFieldTail, Picos, ReservedPField,
    Second, SubMillis, TimeAnomaly, TimeCode, TimeCodeId, TimeFormat, UtcInstant, Year,
    decode_implicit, decode_implicit_lenient, decode_with_pfield, decode_with_pfield_lenient,
};

use crate::exercise::Tally;
use crate::vectors::{Case, Fail, Map, Out, Params as _, Run, Scratch, Val, copy, get, need};

fn round_trip(t: &mut Tally, code: TimeCode) {
    let mut out = [0_u8; 32];
    match code.encode(bb(true), &mut out) {
        Ok(n) => {
            let d = decode_with_pfield_lenient(bb(out.get(..n).unwrap_or_default()))
                .map(Lenient::into_view);
            t.check(d.is_ok_and(|d| d.code() == code && d.len() == n && !d.is_empty()));
            t.check(d.is_ok_and(|d| d.pfield() == Some(code.pfield())));
        }
        Err(_) => t.check(false),
    }
    match (code.encode(bb(false), &mut out), code.format()) {
        (Ok(m), Some(format)) => {
            let d = decode_implicit(bb(out.get(..m).unwrap_or_default()), &format);
            t.check(
                m == format.t_len() && d.is_ok_and(|d| d.code() == code && d.pfield().is_none()),
            );
            t.check(format.pfield() == code.pfield());
        }
        _ => t.check(false),
    }
    t.check(
        code.encode(true, bb(&mut out[..1]))
            .is_err_and(|e| e.kind() == ErrorKind::BufferTooSmall),
    );
}

fn pfield(t: &mut Tally) {
    let p = PField::parse(bb(&[0x9F, 0x80, 0x01, 0xAA]));
    t.check(p.is_ok_and(|p| {
        p.first() == 0x9F && p.rest() == [0x80, 0x01] && p.len() == 3 && !p.is_empty()
    }));
    t.check(p.is_ok_and(|p| p.id() == TimeCodeId::CucLevel1 && p.id().bits() == 1));
    t.check(PField::parse(bb(&[0x80; 9])).is_err_and(|e| e.kind() == ErrorKind::Malformed));
    t.check(
        TimeCodeId::from_octet(bb(0x70)) == TimeCodeId::Reserved111
            && TimeCodeId::Reserved011.is_reserved(),
    );
    let tail = PFieldTail::new(bb(&[0x81, 0x02]));
    t.check(tail.is_ok_and(|x| x.as_bytes() == [0x81, 0x02] && x.len() == 2 && !x.is_empty()));
    t.check(PFieldTail::new(bb(&[0x80])).is_err() && PFieldTail::EMPTY.is_empty());
    let unknown = decode_with_pfield_lenient(bb(&[0x30, 0x00]));
    t.check(unknown.is_ok_and(|d| {
        matches!(d.code(), TimeCode::Unknown(r) if r.pfield().id().is_reserved())
            && d.len() == 1
            && d.anomalies().contains(TimeAnomaly::ReservedTimeCodeId)
            && d.code().format().is_none()
    }));
    t.check(decode_with_pfield(bb(&[0x30])).is_err());
}

fn cuc_codes(t: &mut Tally) {
    let cuc =
        CucFormat::new(CucEpoch::Tai1958, bb(5), bb(4)).and_then(|f| f.with_mission_bits(bb(2)));
    if let Ok(f) = cuc {
        t.check(f.epoch() == CucEpoch::Tai1958 && f.coarse_octets() == 5 && f.fine_octets() == 4);
        t.check(f.mission_bits() == 2 && f.has_octet2() && f.tail().is_empty() && f.t_len() == 9);
        t.check(f.with_octet2() == f && f.with_tail(PFieldTail::EMPTY).is_ok());
        let from_ns = Cuc::from_seconds_nanos(f, bb(1_000), bb(500_000_000));
        t.check(from_ns.is_ok_and(|c| {
            c.value().to_seconds_nanos() == (1_000, 500_000_000) && c.value().fine() == 1 << 127
        }));
        t.check(PField::parse(bb(&[0x1E])).is_ok_and(|p| ReservedPField::new(p).is_none()));
        if let Ok(c) = from_ns.and_then(|c| Cuc::new(f, c.value())) {
            t.check(c.format() == f && c.value().coarse() == 1_000);
            round_trip(t, TimeCode::Cuc(c));
        } else {
            t.check(false);
        }
        t.check(Cuc::new(f, CucValue::new(bb(u64::MAX), 0)).is_err());
    } else {
        t.check(false);
    }
    t.check(CucFormat::new(CucEpoch::Agency, bb(8), 0).is_err());
}

fn cds_codes(t: &mut Tally) {
    let cds = CdsFormat::new(CdsEpoch::Agency, CdsDayLen::Bits24, CdsSubMs::Picos);
    t.check(
        cds.epoch() == CdsEpoch::Agency && cds.day_len() == CdsDayLen::Bits24 && cds.t_len() == 11,
    );
    t.check(
        cds.subms() == CdsSubMs::Picos && cds.subms().max() == 999_999_999 && cds.tail().is_empty(),
    );
    t.check(Picos::try_from(bb(1_000_000_000)).is_err() && Picos::MAX == 999_999_999);
    t.check(Micros::new(bb(999)).map(u16::from) == Some(999) && Micros::new(bb(1000)).is_none());
    t.check(MsOfDay::new(bb(86_401_000)).is_none() && MsOfDay::MIN == 0);
    match (MsOfDay::try_from(bb(86_400_999)), Picos::new(bb(5))) {
        (Ok(ms), Some(ps)) => {
            let subms = SubMillis::Picos(ps);
            t.check(
                ms.get() == MsOfDay::MAX
                    && subms.resolution() == CdsSubMs::Picos
                    && subms.get() == 5,
            );
            t.check(SubMillis::None.get() == 0 && SubMillis::None.resolution() == CdsSubMs::None);
            let value = CdsValue {
                day: bb(1),
                ms_of_day: ms,
                subms,
            };
            match Cds::new(cds.with_tail(PFieldTail::EMPTY), value) {
                Ok(c) => {
                    t.check(c.format() == cds && c.value() == value);
                    round_trip(t, TimeCode::Cds(c));
                }
                Err(_) => t.check(false),
            }
            t.check(
                Cds::new(
                    cds,
                    CdsValue {
                        day: bb(0x100_0000),
                        ..value
                    },
                )
                .is_err(),
            );
            t.check(
                Cds::new(
                    cds,
                    CdsValue {
                        subms: SubMillis::None,
                        ..value
                    },
                )
                .is_err(),
            );
        }
        _ => t.check(false),
    }
}

fn ccs_codes(t: &mut Tally) {
    t.check(
        Year::new(bb(0)).is_none() && Year::try_from(bb(10_000_u16)).is_err() && Year::MAX == 9999,
    );
    t.check(
        Month::new(bb(13)).is_none()
            && Hour::new(bb(24)).is_none()
            && Minute::new(bb(60)).is_none(),
    );
    t.check(Second::new(bb(61)).is_none() && Second::MAX == 60);
    let parts = (
        CcsFormat::new(CcsVariation::DayOfYear, bb(2)),
        Year::try_from(bb(2024_u16)),
        Hour::try_from(bb(23_u8)),
        Minute::try_from(bb(59_u8)),
        Second::try_from(bb(60_u8)),
    );
    match parts {
        (Ok(f), Ok(year), Ok(hour), Ok(minute), Ok(second)) => {
            t.check(f.variation() == CcsVariation::DayOfYear && f.subsecond_octets() == 2);
            t.check(f.t_len() == 9 && f.tail().is_empty() && f.with_tail(PFieldTail::EMPTY) == f);
            t.check(
                year.get() == 2024 && hour.get() == 23 && minute.get() == 59 && second.get() == 60,
            );
            t.check(CalendarDate::from_day_of_year(year, bb(367)).is_err());
            let leap = Month::new(bb(2)).map(|feb| CalendarDate::from_month_day(year, feb, bb(29)));
            t.check(leap.is_some_and(|d| {
                d.is_ok_and(|d| {
                    d.month_day()
                        .is_some_and(|(m, day)| m.get() == 2 && day == 29)
                })
            }));
            match CalendarDate::from_day_of_year(year, bb(366)) {
                Ok(date) => {
                    t.check(date.year() == year && date.day_of_year() == Some(366));
                    t.check(
                        date.month_day().is_none() && date.variation() == CcsVariation::DayOfYear,
                    );
                    let v = CcsValue {
                        date,
                        hour,
                        minute,
                        second,
                        subsecond: 1234,
                    };
                    match Ccs::new(f, v) {
                        Ok(c) => {
                            t.check(c.format() == f && c.value() == v);
                            round_trip(t, TimeCode::Ccs(c));
                        }
                        Err(_) => t.check(false),
                    }
                    t.check(
                        Ccs::new(
                            f,
                            CcsValue {
                                subsecond: bb(10_000),
                                ..v
                            },
                        )
                        .is_err(),
                    );
                }
                Err(_) => t.check(false),
            }
        }
        _ => t.check(false),
    }
}

fn agency_codes(t: &mut Tally) {
    match AgencyFormat::new(bb(3)) {
        Ok(f) => {
            t.check(f.t_len() == 3 && f.tail().is_empty() && f.with_tail(PFieldTail::EMPTY) == f);
            match Agency::new(f, bb(&[1, 2, 3])) {
                Ok(a) => {
                    t.check(a.format() == f && a.tfield() == [1, 2, 3]);
                    round_trip(t, TimeCode::Agency(a));
                }
                Err(_) => t.check(false),
            }
            t.check(Agency::new(f, bb(&[1])).is_err());
        }
        Err(_) => t.check(false),
    }
    t.check(AgencyFormat::new(bb(17)).is_err());
    let fmt = TimeFormat::Cds(CdsFormat::new(
        CdsEpoch::Ccsds1958,
        CdsDayLen::Bits16,
        CdsSubMs::None,
    ));
    t.check(decode_implicit(bb(&[0; 3]), &fmt).is_err());
}

fn ascii_codes(t: &mut Tally) {
    let opts = AsciiOptions::default();
    t.check(opts == AsciiOptions::DEFAULT && opts.max_fraction_digits() == MAX_FRACTION_DIGITS);
    t.check(AsciiOptions::new(bb(13)).is_err());
    let f = Fraction::new(bb(123), 3);
    t.check(f.is_some_and(|f| f.digits() == 3 && f.value() == 123 && f.picos() == 123_000_000_000));
    t.check(Fraction::new(bb(1000), 3).is_none() && Fraction::NONE == Fraction::default());
    let text = bb(b"1988-01-18T17:20:43.123456Z".as_slice());
    match AsciiTime::parse(text, &opts) {
        Ok(a) => {
            t.check(a.hour.get() == 17 && a.minute.get() == 20 && a.second.get() == 43 && a.utc);
            t.check(
                a.date
                    .month_day()
                    .is_some_and(|(m, d)| m.get() == 1 && d == 18),
            );
            let mut out = [0_u8; MAX_ASCII_LEN];
            t.check(
                a.encode(&mut out)
                    .is_ok_and(|n| n == a.encoded_len() && out.get(..n) == Some(text)),
            );
            t.check(
                a.encode(bb(&mut out[..4]))
                    .is_err_and(|e| e.kind() == ErrorKind::BufferTooSmall),
            );
            conversions(t, a);
        }
        Err(_) => t.check(false),
    }
    t.check(
        AsciiTime::parse(bb(b"1988-018T17:20:60"), &opts)
            .is_ok_and(|a| a.date.day_of_year() == Some(18)),
    );
    t.check(
        AsciiTime::parse(bb(b"1988-13-18T17:20:43"), &opts)
            .is_err_and(|e| e.kind() == ErrorKind::OutOfRange),
    );
    t.check(
        AsciiTime::parse(bb(b"1988-01-18"), &opts).is_err_and(|e| e.kind() == ErrorKind::Malformed),
    );
    match AsciiSubset::parse(bb(b"-01-18T17:20Z"), &opts) {
        Ok(s) => {
            t.check(s.variation() == Some(CcsVariation::MonthDay) && s.year().is_none() && s.utc());
            t.check(
                s.month().map(Month::get) == Some(1)
                    && s.day() == Some(18)
                    && s.day_of_year().is_none(),
            );
            t.check(s.hour().map(Hour::get) == Some(17) && s.minute().map(Minute::get) == Some(20));
            t.check(s.second().is_none() && s.fraction().is_none());
        }
        Err(_) => t.check(false),
    }
    t.check(AsciiSubset::parse(bb(b"1988-018"), &opts).is_ok_and(|s| s.day_of_year() == Some(18)));
    t.check(AsciiSubset::parse(bb(b"::43.5"), &opts).is_ok_and(|s| {
        s.second().map(Second::get) == Some(43) && s.fraction() == Fraction::new(5, 1)
    }));
    t.check(
        AsciiSubset::parse(bb(b"1988Z"), &opts).is_err_and(|e| e.kind() == ErrorKind::Malformed),
    );
}

fn conversions(t: &mut Tally, a: AsciiTime) {
    let instant = UtcInstant::from_ascii(&a);
    t.check(instant.picos() == 123_456_000_000 && instant.hour() == a.hour);
    t.check(instant.minute() == a.minute && instant.second() == a.second);
    t.check(
        instant
            .date(CcsVariation::DayOfYear)
            .is_ok_and(|d| d.day_of_year() == Some(18)),
    );
    let cds = CdsFormat::new(CdsEpoch::Ccsds1958, CdsDayLen::Bits16, CdsSubMs::Micros);
    match instant.to_cds(cds, None) {
        Ok(c) => {
            t.check(c.value().day == 10_974 && c.value().ms_of_day.get() == 62_443_123);
            t.check(UtcInstant::from_cds(&c, None) == Ok(instant));
        }
        Err(_) => t.check(false),
    }
    let agency = CdsFormat::new(CdsEpoch::Agency, CdsDayLen::Bits16, CdsSubMs::None);
    t.check(
        instant
            .to_cds(agency, None)
            .is_err_and(|e| e.kind() == ErrorKind::InvalidConfig),
    );
    t.check(
        instant
            .to_cds(agency, Some(a.date))
            .is_ok_and(|c| c.value().day == 0),
    );
    match CcsFormat::new(CcsVariation::MonthDay, bb(3)) {
        Ok(f) => {
            t.check(instant.to_ccs(f).is_ok_and(|c| {
                c.value().subsecond == 123_456 && UtcInstant::from_ccs(&c) == instant
            }));
        }
        Err(_) => t.check(false),
    }
    match AsciiFormat::new(CcsVariation::DayOfYear, bb(2), false) {
        Ok(f) => {
            t.check(
                f.variation() == CcsVariation::DayOfYear && f.fraction_digits() == 2 && !f.utc(),
            );
            t.check(
                instant
                    .to_ascii(f)
                    .is_ok_and(|b| b.fraction == Fraction::new(12, 2).unwrap_or_default()),
            );
        }
        Err(_) => t.check(false),
    }
    t.check(AsciiFormat::new(CcsVariation::MonthDay, bb(13), true).is_err());
}

/// Bounded fields (conversion to the integer, default, `get`), and the P-field of each format
/// built directly as well as through `TimeFormat`.
fn fields_and_formats(t: &mut Tally) {
    t.check(Year::new(bb(2024)).map(u16::from) == Some(2024) && Year::default().get() == Year::MIN);
    t.check(Month::new(bb(12)).map(u8::from) == Some(12) && Month::default().get() == Month::MIN);
    t.check(Hour::new(bb(23)).map(u8::from) == Some(23) && Hour::default().get() == Hour::MIN);
    t.check(
        Minute::new(bb(59)).map(u8::from) == Some(59) && Minute::default().get() == Minute::MIN,
    );
    t.check(
        Second::new(bb(60)).map(u8::from) == Some(60) && Second::default().get() == Second::MIN,
    );
    t.check(
        MsOfDay::new(bb(5)).map(u32::from) == Some(5) && MsOfDay::default().get() == MsOfDay::MIN,
    );
    t.check(Picos::new(bb(7)).is_some_and(|p| p.get() == 7 && u32::from(p) == 7));
    t.check(Micros::default().get() == Micros::MIN && Picos::default().get() == Picos::MIN);
    let formats = (
        CucFormat::new(CucEpoch::Tai1958, bb(4), bb(2)),
        CcsFormat::new(CcsVariation::MonthDay, bb(1)),
        AgencyFormat::new(bb(2)),
    );
    if let (Ok(cuc), Ok(ccs), Ok(agency)) = formats {
        let cds = CdsFormat::new(CdsEpoch::Ccsds1958, CdsDayLen::Bits16, CdsSubMs::Micros);
        t.check(cuc.pfield() == TimeFormat::Cuc(cuc).pfield());
        t.check(cds.pfield() == TimeFormat::Cds(cds).pfield());
        t.check(ccs.pfield() == TimeFormat::Ccs(ccs).pfield());
        t.check(agency.pfield() == TimeFormat::Agency(agency).pfield());
    } else {
        t.check(false);
    }
    // A reserved time code ID: the P-field converts back from its reserved wrapper.
    let reserved = PField::parse(bb(&[0x30]))
        .ok()
        .and_then(ReservedPField::new);
    t.check(
        reserved.is_some_and(|r| PField::from(r) == r.pfield() && r.pfield().id().is_reserved()),
    );
}

/// Runs the `aldis-time` group.
pub fn run(t: &mut Tally) {
    pfield(t);
    cuc_codes(t);
    cds_codes(t);
    ccs_codes(t);
    agency_codes(t);
    ascii_codes(t);
    fields_and_formats(t);
}

/// A `usize` as reported by the host suites.
fn wide(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

const fn mode(case: &Case) -> DecodeMode {
    if case.lenient {
        DecodeMode::Lenient
    } else {
        DecodeMode::Strict
    }
}

/// The P-field extension of a format object (`pfield_tail_hex`, empty when absent).
fn tail(obj: Map) -> Result<PFieldTail, Fail> {
    let octets = get(obj, "pfield_tail_hex").map_or(Some(&[][..]), Val::bytes);
    PFieldTail::new(bb(need(octets)?)).map_err(|_| Fail::Vector)
}

/// Left-aligns raw fine octets.
fn fine_left(raw: &[u8]) -> Result<u128, Fail> {
    let mut buf = [0_u8; 16];
    if !copy(need(buf.get_mut(..raw.len()))?, raw) {
        return Err(Fail::Vector);
    }
    Ok(u128::from_be_bytes(buf))
}

/// The leading `octets` octets of a left-aligned fine time.
fn fine_raw(o: &mut Out, key: &str, fine: u128, octets: u8) {
    let bytes = fine.to_be_bytes();
    match bytes.get(..usize::from(octets)) {
        Some(raw) => o.bytes(key, raw),
        None => o.null(key),
    }
}

/// A format object; `code` names the time code.
fn format_of(code: &str, obj: Map) -> Result<TimeFormat, Fail> {
    Ok(match code {
        "cuc" => {
            let epoch = if obj.text("epoch")? == "level1" {
                CucEpoch::Tai1958
            } else {
                CucEpoch::Agency
            };
            let mut f = CucFormat::new(
                epoch,
                bb(obj.num("coarse_octets")?),
                bb(obj.num("fine_octets")?),
            )?;
            let mission: u8 = obj.num("mission_bits")?;
            if mission != 0 {
                f = f.with_mission_bits(bb(mission))?;
            }
            if obj.flag("octet2")? {
                f = f.with_octet2();
            }
            let t = tail(obj)?;
            if !t.is_empty() {
                f = f.with_tail(t)?;
            }
            TimeFormat::Cuc(f)
        }
        "cds" => TimeFormat::Cds(
            CdsFormat::new(
                if obj.text("epoch")? == "ccsds" {
                    CdsEpoch::Ccsds1958
                } else {
                    CdsEpoch::Agency
                },
                if obj.num::<u64>("day_bits")? == 16 {
                    CdsDayLen::Bits16
                } else {
                    CdsDayLen::Bits24
                },
                match obj.text("subms")? {
                    "none" => CdsSubMs::None,
                    "us" => CdsSubMs::Micros,
                    _ => CdsSubMs::Picos,
                },
            )
            .with_tail(tail(obj)?),
        ),
        "ccs" => TimeFormat::Ccs(
            CcsFormat::new(
                if obj.text("variation")? == "month_day" {
                    CcsVariation::MonthDay
                } else {
                    CcsVariation::DayOfYear
                },
                bb(obj.num("subsecond_octets")?),
            )?
            .with_tail(tail(obj)?),
        ),
        "agency" => {
            TimeFormat::Agency(AgencyFormat::new(bb(obj.num("t_len")?))?.with_tail(tail(obj)?))
        }
        _ => return Err(Fail::Vector),
    })
}

/// A date from `year` and either `day_of_year` or `month` and `day`.
fn date_of(v: Map, year: Year, by_day_of_year: bool) -> Result<CalendarDate, Fail> {
    Ok(if by_day_of_year {
        CalendarDate::from_day_of_year(year, bb(v.num("day_of_year")?))?
    } else {
        CalendarDate::from_month_day(
            year,
            Month::try_from(bb(v.num::<u8>("month")?))?,
            bb(v.num("day")?),
        )?
    })
}

fn code_of(format: TimeFormat, v: Map) -> Result<TimeCode, Fail> {
    Ok(match format {
        TimeFormat::Cuc(f) => {
            let fine = fine_left(v.bytes("fine_hex")?)?;
            TimeCode::Cuc(Cuc::new(f, CucValue::new(bb(v.num("coarse")?), bb(fine)))?)
        }
        TimeFormat::Cds(f) => {
            // Each field is parsed into its type; the vector's raw value may be invalid.
            let raw: u32 = v.num("subms")?;
            let subms = match f.subms() {
                CdsSubMs::None => SubMillis::None,
                CdsSubMs::Micros => SubMillis::Micros(Micros::try_from(bb(
                    u16::try_from(raw).unwrap_or(u16::MAX)
                ))?),
                CdsSubMs::Picos => SubMillis::Picos(Picos::try_from(bb(raw))?),
            };
            TimeCode::Cds(Cds::new(
                f,
                CdsValue {
                    day: bb(v.num("day")?),
                    ms_of_day: MsOfDay::try_from(bb(v.num::<u32>("ms_of_day")?))?,
                    subms,
                },
            )?)
        }
        TimeFormat::Ccs(f) => {
            let year = Year::try_from(bb(v.num::<u16>("year")?))?;
            let by_day_of_year = !v.val("day_of_year")?.is_null();
            TimeCode::Ccs(Ccs::new(
                f,
                CcsValue {
                    date: date_of(v, year, by_day_of_year)?,
                    hour: Hour::try_from(bb(v.num::<u8>("hour")?))?,
                    minute: Minute::try_from(bb(v.num::<u8>("minute")?))?,
                    second: Second::try_from(bb(v.num::<u8>("second")?))?,
                    subsecond: bb(v.num("subsecond")?),
                },
            )?)
        }
        TimeFormat::Agency(f) => TimeCode::Agency(Agency::new(f, bb(v.bytes("tfield_hex")?))?),
    })
}

const fn code_name(code: &TimeCode) -> &'static str {
    match code {
        TimeCode::Cuc(_) => "cuc",
        TimeCode::Cds(_) => "cds",
        TimeCode::Ccs(_) => "ccs",
        TimeCode::Agency(_) => "agency",
        TimeCode::Unknown(_) => "unknown",
    }
}

/// The `format` object of a decoded code (`null` for an unknown code).
fn format_fields(o: &mut Out, key: &str, code: &TimeCode) {
    let fill = |x: &mut Out| match code {
        TimeCode::Cuc(c) => {
            let f = c.format();
            x.text(
                "epoch",
                match f.epoch() {
                    CucEpoch::Tai1958 => "level1",
                    CucEpoch::Agency => "agency",
                },
            );
            x.num("coarse_octets", f.coarse_octets());
            x.num("fine_octets", f.fine_octets());
            x.num("mission_bits", f.mission_bits());
            x.flag("octet2", f.has_octet2());
            x.bytes("pfield_tail_hex", f.tail().as_bytes());
        }
        TimeCode::Cds(c) => {
            let f = c.format();
            x.text(
                "epoch",
                match f.epoch() {
                    CdsEpoch::Ccsds1958 => "ccsds",
                    CdsEpoch::Agency => "agency",
                },
            );
            x.num(
                "day_bits",
                match f.day_len() {
                    CdsDayLen::Bits16 => 16_u8,
                    CdsDayLen::Bits24 => 24,
                },
            );
            x.text(
                "subms",
                match f.subms() {
                    CdsSubMs::None => "none",
                    CdsSubMs::Micros => "us",
                    CdsSubMs::Picos => "ps",
                },
            );
            x.bytes("pfield_tail_hex", f.tail().as_bytes());
        }
        TimeCode::Ccs(c) => {
            let f = c.format();
            x.text(
                "variation",
                match f.variation() {
                    CcsVariation::MonthDay => "month_day",
                    CcsVariation::DayOfYear => "day_of_year",
                },
            );
            x.num("subsecond_octets", f.subsecond_octets());
            x.bytes("pfield_tail_hex", f.tail().as_bytes());
        }
        TimeCode::Agency(c) => {
            x.num("t_len", wide(c.format().t_len()));
            x.bytes("pfield_tail_hex", c.format().tail().as_bytes());
        }
        TimeCode::Unknown(_) => {}
    };
    if matches!(code, TimeCode::Unknown(_)) {
        o.null(key);
    } else {
        o.obj(key, fill);
    }
}

/// The `value` object of a code (`null` for an unknown code).
fn value_fields(o: &mut Out, key: &str, code: &TimeCode) {
    let fill = |x: &mut Out| match code {
        TimeCode::Cuc(c) => {
            x.num("coarse", c.value().coarse());
            fine_raw(x, "fine_hex", c.value().fine(), c.format().fine_octets());
        }
        TimeCode::Cds(c) => {
            x.num("day", c.value().day);
            x.num("ms_of_day", c.value().ms_of_day.get());
            x.num("subms", c.value().subms.get());
        }
        TimeCode::Ccs(c) => {
            let v = c.value();
            x.num("year", v.date.year().get());
            x.opt_num("month", v.date.month_day().map(|(m, _)| m.get()));
            x.opt_num("day", v.date.month_day().map(|(_, d)| d));
            x.opt_num("day_of_year", v.date.day_of_year());
            x.num("hour", v.hour.get());
            x.num("minute", v.minute.get());
            x.num("second", v.second.get());
            x.num("subsecond", v.subsecond);
        }
        TimeCode::Agency(c) => x.bytes("tfield_hex", c.tfield()),
        TimeCode::Unknown(_) => {}
    };
    if matches!(code, TimeCode::Unknown(_)) {
        o.null(key);
    } else {
        o.obj(key, fill);
    }
}

fn decoded(o: &mut Out, d: &Decoded, anomalies: AnomalySet<TimeAnomaly>) {
    let code = d.code();
    o.text("code", code_name(&code));
    let mut pfield = [0_u8; 8];
    let len = d.pfield().map_or(0, |p| {
        let mut n = 0_usize;
        for (slot, octet) in pfield
            .iter_mut()
            .zip(core::iter::once(p.first()).chain(p.rest().iter().copied()))
        {
            *slot = octet;
            n = n.saturating_add(1);
        }
        n
    });
    o.bytes("pfield_hex", pfield.get(..len).unwrap_or_default());
    format_fields(o, "format", &code);
    value_fields(o, "value", &code);
    o.num("consumed", wide(d.len()));
    o.names("anomalies", anomalies.iter());
}

fn options(p: Map) -> Result<AsciiOptions, Fail> {
    match get(p, "max_fraction_digits") {
        None => Ok(AsciiOptions::DEFAULT),
        Some(n) => {
            let digits = need(n.num().and_then(|n| u8::try_from(n).ok()))?;
            AsciiOptions::new(bb(digits)).map_err(|_| Fail::Vector)
        }
    }
}

fn ascii_fields(o: &mut Out, t: &AsciiTime) {
    let md = t.date.month_day();
    o.text("variation", if md.is_some() { "A" } else { "B" });
    o.num("year", t.date.year().get());
    o.opt_num("month", md.map(|(m, _)| m.get()));
    o.opt_num("day", md.map(|(_, d)| d));
    o.opt_num("day_of_year", t.date.day_of_year());
    o.num("hour", t.hour.get());
    o.num("minute", t.minute.get());
    o.num("second", t.second.get());
    o.num("fraction_digits", t.fraction.digits());
    o.num("fraction", t.fraction.value());
    o.flag("z", t.utc);
}

fn ascii_from_fields(v: Map) -> Result<AsciiTime, Fail> {
    let year = Year::try_from(bb(v.num::<u16>("year")?))?;
    Ok(AsciiTime {
        date: date_of(v, year, v.text("variation")? != "A")?,
        hour: Hour::try_from(bb(v.num::<u8>("hour")?))?,
        minute: Minute::try_from(bb(v.num::<u8>("minute")?))?,
        second: Second::try_from(bb(v.num::<u8>("second")?))?,
        fraction: need(Fraction::new(
            bb(v.num("fraction")?),
            bb(v.num("fraction_digits")?),
        ))?,
        utc: v.flag("z")?,
    })
}

fn subset_fields(o: &mut Out, s: &AsciiSubset) {
    o.opt_text(
        "variation",
        s.variation().map(|v| match v {
            CcsVariation::MonthDay => "A",
            CcsVariation::DayOfYear => "B",
        }),
    );
    o.opt_num("year", s.year().map(Year::get));
    o.opt_num("month", s.month().map(Month::get));
    o.opt_num("day", s.day());
    o.opt_num("day_of_year", s.day_of_year());
    o.opt_num("hour", s.hour().map(Hour::get));
    o.opt_num("minute", s.minute().map(Minute::get));
    o.opt_num("second", s.second().map(Second::get));
    o.opt_num("fraction_digits", s.fraction().map(|f| f.digits()));
    o.opt_num("fraction", s.fraction().map(|f| f.value()));
    o.flag("z", s.utc());
}

/// Formats `t` and reports it as `text`.
fn ascii_text(o: &mut Out, t: &AsciiTime) -> Run {
    let mut out = [0_u8; MAX_ASCII_LEN];
    let n = t.encode(bb(&mut out))?;
    o.bytes("text", need(out.get(..n))?);
    Ok(())
}

fn convert(case: &Case, o: &mut Out) -> Run {
    let p = case.params;
    let epoch = match get(p, "epoch") {
        None => None,
        Some(e) => {
            let e = need(e.obj())?;
            let year = need(Year::new(e.num("year")?))?;
            let month = need(Month::new(e.num("month")?))?;
            Some(
                CalendarDate::from_month_day(year, month, e.num("day")?)
                    .map_err(|_| Fail::Vector)?,
            )
        }
    };
    let from = p.obj("from")?;
    let instant = match from.text("code")? {
        "ascii" => UtcInstant::from_ascii(&AsciiTime::parse(
            bb(from.text("text")?.as_bytes()),
            &AsciiOptions::DEFAULT,
        )?),
        code => match code_of(format_of(code, from.obj("format")?)?, from.obj("value")?)? {
            TimeCode::Cds(c) => UtcInstant::from_cds(&c, bb(epoch))?,
            TimeCode::Ccs(c) => UtcInstant::from_ccs(&c),
            _ => return Err(Fail::Vector),
        },
    };
    let to = p.obj("to")?;
    match to.text("code")? {
        "ascii" => {
            let variation = if to.text("variation")? == "A" {
                CcsVariation::MonthDay
            } else {
                CcsVariation::DayOfYear
            };
            let format =
                AsciiFormat::new(variation, bb(to.num("fraction_digits")?), to.flag("z")?)?;
            let t = instant.to_ascii(bb(format))?;
            ascii_text(o, &t).map_err(|_| Fail::Vector)
        }
        code => {
            let code = match format_of(code, to)? {
                TimeFormat::Cds(f) => TimeCode::Cds(instant.to_cds(bb(f), bb(epoch))?),
                TimeFormat::Ccs(f) => TimeCode::Ccs(instant.to_ccs(bb(f))?),
                _ => return Err(Fail::Vector),
            };
            value_fields(o, "value", &code);
            Ok(())
        }
    }
}

/// One `test-vectors/time/` case, as `crates/aldis-time/tests/vectors.rs` runs it.
pub fn vector(case: &Case, o: &mut Out, _: &mut Scratch) -> Run {
    let p = case.params;
    match case.op {
        "decode" => {
            let (code, found) = mode(case).dispatch(
                || decode_with_pfield(bb(case.input)),
                || decode_with_pfield_lenient(bb(case.input)),
            )?;
            decoded(o, &code, found);
        }
        "decode_implicit" => {
            let f = p.obj("format")?;
            let format = format_of(f.text("code")?, f)?;
            let (code, found) = mode(case).dispatch(
                || decode_implicit(bb(case.input), &format),
                || decode_implicit_lenient(bb(case.input), &format),
            )?;
            decoded(o, &code, found);
        }
        "encode" => {
            let f = p.obj("format")?;
            let code = code_of(format_of(f.text("code")?, f)?, p.obj("value")?)?;
            let mut out = [0_u8; 32];
            let n = code.encode(bb(p.flag("include_pfield")?), bb(&mut out))?;
            o.bytes("output_hex", need(out.get(..n))?);
        }
        "cuc_to_ns" => {
            let fine = fine_left(p.bytes("fine_hex")?)?;
            let (s, ns) = CucValue::new(bb(p.num("coarse")?), bb(fine)).to_seconds_nanos();
            o.num("seconds", s);
            o.num("nanoseconds", ns);
        }
        "cuc_from_ns" => {
            let f = CucFormat::new(
                CucEpoch::Tai1958,
                bb(p.num("coarse_octets")?),
                bb(p.num("fine_octets")?),
            )?;
            let c = Cuc::from_seconds_nanos(f, bb(p.num("seconds")?), bb(p.num("nanoseconds")?))?;
            if c.to_seconds_nanos() != c.value().to_seconds_nanos() {
                return Err(Fail::Vector);
            }
            o.num("coarse", c.value().coarse());
            fine_raw(o, "fine_hex", c.value().fine(), f.fine_octets());
        }
        "ascii_parse" => {
            let text = p.text("text")?.as_bytes();
            let t = AsciiTime::parse(bb(text), &options(p)?)?;
            // The formatted code is the input again (TIME-031 round trip).
            let mut out = [0_u8; MAX_ASCII_LEN];
            let n = t.encode(bb(&mut out)).map_err(|_| Fail::Vector)?;
            if out.get(..n) != Some(text) || t.encoded_len() != n {
                return Err(Fail::Vector);
            }
            ascii_fields(o, &t);
        }
        "ascii_format" => ascii_text(o, &ascii_from_fields(p.obj("value")?)?)?,
        "ascii_subset" => {
            let s = AsciiSubset::parse(bb(p.text("text")?.as_bytes()), &options(p)?)?;
            subset_fields(o, &s);
        }
        "convert" => convert(case, o)?,
        _ => return Err(Fail::Vector),
    }
    Ok(())
}
