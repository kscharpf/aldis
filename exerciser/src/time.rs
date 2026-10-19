//! Calls every public `aldis-time` API (VER-010, M1 D7). Inputs pass through `black_box`.

use core::hint::black_box as bb;

use aldis_core::{DecodeMode, ErrorKind};
use aldis_time::{
    Agency, AgencyFormat, CalendarDate, Ccs, CcsFormat, CcsValue, CcsVariation, Cds, CdsDayLen,
    CdsEpoch, CdsFormat, CdsSubMs, CdsValue, Cuc, CucEpoch, CucFormat, CucValue, PField,
    PFieldTail, TimeAnomaly, TimeCode, TimeCodeId, TimeFormat, decode_implicit, decode_with_pfield,
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

fn codes(t: &mut Tally) {
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

    let cds = CdsFormat::new(CdsEpoch::Agency, CdsDayLen::Bits24, CdsSubMs::Picos);
    t.check(
        cds.epoch() == CdsEpoch::Agency && cds.day_len() == CdsDayLen::Bits24 && cds.t_len() == 11,
    );
    t.check(
        cds.subms() == CdsSubMs::Picos && cds.subms().max() == 999_999_999 && cds.tail().is_empty(),
    );
    let value = CdsValue {
        day: bb(1),
        ms_of_day: bb(86_400_999),
        subms: bb(5),
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
                ms_of_day: bb(86_401_000),
                ..value
            },
        )
        .is_err(),
    );

    match CcsFormat::new(CcsVariation::DayOfYear, bb(2)) {
        Ok(f) => {
            t.check(f.variation() == CcsVariation::DayOfYear && f.subsecond_octets() == 2);
            t.check(f.t_len() == 9 && f.tail().is_empty() && f.with_tail(PFieldTail::EMPTY) == f);
            let v = CcsValue {
                year: bb(2024),
                date: CalendarDate::DayOfYear(bb(366)),
                hour: 23,
                minute: 59,
                second: bb(60),
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
                        year: bb(2023),
                        ..v
                    },
                )
                .is_err(),
            );
        }
        Err(_) => t.check(false),
    }

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
    let fmt = TimeFormat::Cds(cds);
    t.check(decode_implicit(bb(&[0; 3]), &fmt, DecodeMode::Strict).is_err());
}

/// Runs the `aldis-time` group.
pub fn run(t: &mut Tally) {
    pfield(t);
    codes(t);
}
