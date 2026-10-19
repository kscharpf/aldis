//! Calls every public `aldis-time` API (VER-010, M1 D7). Inputs pass through `black_box`.

use core::hint::black_box as bb;

use aldis_core::{DecodeMode, ErrorKind};
use aldis_time::{
    Agency, AgencyFormat, CalendarDate, Ccs, CcsFormat, CcsValue, CcsVariation, Cds, CdsDayLen,
    CdsEpoch, CdsFormat, CdsSubMs, CdsValue, Cuc, CucEpoch, CucFormat, CucValue, Hour, Micros,
    Minute, Month, MsOfDay, PField, PFieldTail, Picos, Second, SubMillis, TimeAnomaly, TimeCode,
    TimeCodeId, TimeFormat, Year, decode_implicit, decode_with_pfield,
};

use crate::exercise::Tally;

fn round_trip(t: &mut Tally, code: TimeCode) {
    let mut out = [0_u8; 32];
    match code.encode(bb(true), &mut out) {
        Ok(n) => {
            let d = decode_with_pfield(bb(out.get(..n).unwrap_or_default()), DecodeMode::Lenient);
            t.check(d.is_ok_and(|d| d.code() == code && d.len() == n && !d.is_empty()));
            t.check(d.is_ok_and(|d| d.pfield() == Some(code.pfield())));
        }
        Err(_) => t.check(false),
    }
    match (code.encode(bb(false), &mut out), code.format()) {
        (Ok(m), Some(format)) => {
            let d = decode_implicit(
                bb(out.get(..m).unwrap_or_default()),
                &format,
                DecodeMode::Strict,
            );
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
    t.check(TimeCodeId::from_octet(bb(0x70)) == TimeCodeId::Reserved(7));
    let tail = PFieldTail::new(bb(&[0x81, 0x02]));
    t.check(tail.is_ok_and(|x| x.as_bytes() == [0x81, 0x02] && x.len() == 2 && !x.is_empty()));
    t.check(PFieldTail::new(bb(&[0x80])).is_err() && PFieldTail::EMPTY.is_empty());
    let unknown = decode_with_pfield(bb(&[0x30, 0x00]), DecodeMode::Lenient);
    t.check(unknown.is_ok_and(|d| {
        matches!(d.code(), TimeCode::Unknown(_))
            && d.len() == 1
            && d.anomalies().contains(TimeAnomaly::ReservedTimeCodeId)
            && d.code().format().is_none()
    }));
    t.check(decode_with_pfield(bb(&[0x30]), DecodeMode::Strict).is_err());
}

fn cuc_codes(t: &mut Tally) {
    let cuc =
        CucFormat::new(CucEpoch::Tai1958, bb(5), bb(4)).and_then(|f| f.with_mission_bits(bb(2)));
    if let Ok(f) = cuc {
        t.check(f.epoch() == CucEpoch::Tai1958 && f.coarse_octets() == 5 && f.fine_octets() == 4);
        t.check(f.mission_bits() == 2 && f.has_octet2() && f.tail().is_empty() && f.t_len() == 9);
        t.check(f.with_octet2() == f && f.with_tail(PFieldTail::EMPTY).is_ok());
        let v = CucValue::from_seconds_nanos(bb(1_000), bb(500_000_000), &f);
        t.check(
            v.is_ok_and(|v| v.to_seconds_nanos() == (1_000, 500_000_000) && v.coarse() == 1_000),
        );
        t.check(v.is_ok_and(|v| v.fine() == 1 << 127));
        if let Ok(c) = v.and_then(|v| Cuc::new(f, v)) {
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
    t.check(decode_implicit(bb(&[0; 3]), &fmt, DecodeMode::Strict).is_err());
}

/// Runs the `aldis-time` group.
pub fn run(t: &mut Tally) {
    pfield(t);
    cuc_codes(t);
    cds_codes(t);
    ccs_codes(t);
    agency_codes(t);
}
