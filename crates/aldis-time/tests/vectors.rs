//! Runs the shared conformance vectors in `test-vectors/time/` against `aldis-time` (VER-021,
//! TV-012). Every case's outputs must match exactly; unknown operations fail.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test-only harness crate: malformed vectors must fail loudly (RS-018)"
)]

use aldis_core::{DecodeMode, Error};
use aldis_testkit::{CodecCase, Mode, Outcome, hex, run_codec_file};
use aldis_time::{
    Agency, AgencyFormat, CalendarDate, Ccs, CcsFormat, CcsValue, CcsVariation, Cds, CdsDayLen,
    CdsEpoch, CdsFormat, CdsSubMs, CdsValue, Cuc, CucEpoch, CucFormat, CucValue, Decoded, Hour,
    Micros, Minute, Month, MsOfDay, PFieldTail, Picos, Second, SubMillis, TimeCode, TimeFormat,
    Year, decode_implicit, decode_with_pfield,
};
use serde_json::{Map, Value, json};

fn param<'a>(case: &'a CodecCase, name: &str) -> &'a Value {
    case.params
        .as_ref()
        .and_then(|p| p.get(name))
        .unwrap_or_else(|| panic!("{}: missing param {name}", case.id))
}

fn get<'a>(obj: &'a Value, name: &str) -> &'a Value {
    obj.get(name)
        .unwrap_or_else(|| panic!("missing {name} in {obj}"))
}

fn u64_of(obj: &Value, name: &str) -> u64 {
    get(obj, name).as_u64().unwrap()
}

fn u8_of(obj: &Value, name: &str) -> u8 {
    u8::try_from(u64_of(obj, name)).unwrap()
}

fn str_of<'a>(obj: &'a Value, name: &str) -> &'a str {
    get(obj, name).as_str().unwrap()
}

fn input(case: &CodecCase) -> Vec<u8> {
    hex::decode(case.input_hex.as_deref().unwrap_or("")).unwrap()
}

fn mode(case: &CodecCase) -> DecodeMode {
    match case.mode.unwrap_or_default() {
        Mode::Strict => DecodeMode::Strict,
        Mode::Lenient => DecodeMode::Lenient,
    }
}

fn tail(obj: &Value) -> PFieldTail {
    let hex = obj
        .get("pfield_tail_hex")
        .and_then(Value::as_str)
        .unwrap_or("");
    PFieldTail::new(&hex::decode(hex).unwrap()).unwrap()
}

/// Left-aligns raw fine octets.
fn fine_left(raw: &[u8]) -> u128 {
    let mut buf = [0_u8; 16];
    buf[..raw.len()].copy_from_slice(raw);
    u128::from_be_bytes(buf)
}

fn fine_raw(fine: u128, octets: u8) -> String {
    hex::encode(&fine.to_be_bytes()[..usize::from(octets)])
}

fn format_of(obj: &Value) -> Result<TimeFormat, Error> {
    Ok(match str_of(obj, "code") {
        "cuc" => {
            let epoch = match str_of(obj, "epoch") {
                "level1" => CucEpoch::Tai1958,
                _ => CucEpoch::Agency,
            };
            let mut f = CucFormat::new(
                epoch,
                u8_of(obj, "coarse_octets"),
                u8_of(obj, "fine_octets"),
            )?;
            let mission = u8_of(obj, "mission_bits");
            if mission != 0 {
                f = f.with_mission_bits(mission)?;
            }
            if get(obj, "octet2").as_bool().unwrap() {
                f = f.with_octet2();
            }
            let t = tail(obj);
            if !t.is_empty() {
                f = f.with_tail(t)?;
            }
            TimeFormat::Cuc(f)
        }
        "cds" => TimeFormat::Cds(
            CdsFormat::new(
                if str_of(obj, "epoch") == "ccsds" {
                    CdsEpoch::Ccsds1958
                } else {
                    CdsEpoch::Agency
                },
                if u64_of(obj, "day_bits") == 16 {
                    CdsDayLen::Bits16
                } else {
                    CdsDayLen::Bits24
                },
                match str_of(obj, "subms") {
                    "none" => CdsSubMs::None,
                    "us" => CdsSubMs::Micros,
                    _ => CdsSubMs::Picos,
                },
            )
            .with_tail(tail(obj)),
        ),
        "ccs" => TimeFormat::Ccs(
            CcsFormat::new(
                if str_of(obj, "variation") == "month_day" {
                    CcsVariation::MonthDay
                } else {
                    CcsVariation::DayOfYear
                },
                u8_of(obj, "subsecond_octets"),
            )?
            .with_tail(tail(obj)),
        ),
        "agency" => {
            TimeFormat::Agency(AgencyFormat::new(u8_of(obj, "t_len"))?.with_tail(tail(obj)))
        }
        other => panic!("unknown code {other}"),
    })
}

fn code_of(format: TimeFormat, v: &Value) -> Result<TimeCode, Error> {
    Ok(match format {
        TimeFormat::Cuc(f) => {
            let raw = hex::decode(str_of(v, "fine_hex")).unwrap();
            TimeCode::Cuc(Cuc::new(
                f,
                CucValue::new(u64_of(v, "coarse"), fine_left(&raw)),
            )?)
        }
        TimeFormat::Cds(f) => {
            // Each field is parsed into its type; the vector's raw value may be invalid.
            let raw = u32::try_from(u64_of(v, "subms")).unwrap();
            let subms = match f.subms() {
                CdsSubMs::None => SubMillis::None,
                CdsSubMs::Micros => {
                    SubMillis::Micros(Micros::try_from(u16::try_from(raw).unwrap_or(u16::MAX))?)
                }
                CdsSubMs::Picos => SubMillis::Picos(Picos::try_from(raw)?),
            };
            let ms = u32::try_from(u64_of(v, "ms_of_day")).unwrap();
            TimeCode::Cds(Cds::new(
                f,
                CdsValue {
                    day: u32::try_from(u64_of(v, "day")).unwrap(),
                    ms_of_day: MsOfDay::try_from(ms)?,
                    subms,
                },
            )?)
        }
        TimeFormat::Ccs(f) => {
            let year = Year::try_from(u16::try_from(u64_of(v, "year")).unwrap())?;
            let date = match get(v, "day_of_year").as_u64() {
                Some(doy) => CalendarDate::from_day_of_year(year, u16::try_from(doy).unwrap())?,
                None => CalendarDate::from_month_day(
                    year,
                    Month::try_from(u8_of(v, "month"))?,
                    u8_of(v, "day"),
                )?,
            };
            TimeCode::Ccs(Ccs::new(
                f,
                CcsValue {
                    date,
                    hour: Hour::try_from(u8_of(v, "hour"))?,
                    minute: Minute::try_from(u8_of(v, "minute"))?,
                    second: Second::try_from(u8_of(v, "second"))?,
                    subsecond: u64_of(v, "subsecond"),
                },
            )?)
        }
        TimeFormat::Agency(f) => TimeCode::Agency(Agency::new(
            f,
            &hex::decode(str_of(v, "tfield_hex")).unwrap(),
        )?),
    })
}

fn tail_hex(t: PFieldTail) -> String {
    hex::encode(t.as_bytes())
}

fn describe(code: &TimeCode) -> (&'static str, Value, Value) {
    match code {
        TimeCode::Cuc(c) => {
            let f = c.format();
            (
                "cuc",
                json!({
                    "epoch": match f.epoch() { CucEpoch::Tai1958 => "level1", CucEpoch::Agency => "agency" },
                    "coarse_octets": f.coarse_octets(),
                    "fine_octets": f.fine_octets(),
                    "mission_bits": f.mission_bits(),
                    "octet2": f.has_octet2(),
                    "pfield_tail_hex": tail_hex(f.tail()),
                }),
                json!({ "coarse": c.value().coarse(), "fine_hex": fine_raw(c.value().fine(), f.fine_octets()) }),
            )
        }
        TimeCode::Cds(c) => {
            let f = c.format();
            (
                "cds",
                json!({
                    "epoch": match f.epoch() { CdsEpoch::Ccsds1958 => "ccsds", CdsEpoch::Agency => "agency" },
                    "day_bits": match f.day_len() { CdsDayLen::Bits16 => 16, CdsDayLen::Bits24 => 24 },
                    "subms": match f.subms() { CdsSubMs::None => "none", CdsSubMs::Micros => "us", CdsSubMs::Picos => "ps" },
                    "pfield_tail_hex": tail_hex(f.tail()),
                }),
                json!({ "day": c.value().day, "ms_of_day": c.value().ms_of_day.get(), "subms": c.value().subms.get() }),
            )
        }
        TimeCode::Ccs(c) => {
            let f = c.format();
            let v = c.value();
            let (month, day) = v
                .date
                .month_day()
                .map_or((Value::Null, Value::Null), |(m, d)| {
                    (json!(m.get()), json!(d))
                });
            let doy = v.date.day_of_year().map_or(Value::Null, |d| json!(d));
            (
                "ccs",
                json!({
                    "variation": match f.variation() { CcsVariation::MonthDay => "month_day", CcsVariation::DayOfYear => "day_of_year" },
                    "subsecond_octets": f.subsecond_octets(),
                    "pfield_tail_hex": tail_hex(f.tail()),
                }),
                json!({
                    "year": v.date.year().get(), "month": month, "day": day, "day_of_year": doy,
                    "hour": v.hour.get(), "minute": v.minute.get(), "second": v.second.get(), "subsecond": v.subsecond,
                }),
            )
        }
        TimeCode::Agency(c) => (
            "agency",
            json!({ "t_len": c.format().t_len(), "pfield_tail_hex": tail_hex(c.format().tail()) }),
            json!({ "tfield_hex": hex::encode(c.tfield()) }),
        ),
        TimeCode::Unknown(_) => ("unknown", Value::Null, Value::Null),
    }
}

fn decoded(d: &Decoded) -> Value {
    let (code, format, value) = describe(&d.code());
    let pfield = d.pfield().map_or_else(String::new, |p| {
        let mut octets = vec![p.first()];
        octets.extend_from_slice(p.rest());
        hex::encode(&octets)
    });
    json!({
        "code": code,
        "pfield_hex": pfield,
        "format": format,
        "value": value,
        "consumed": d.len(),
        "anomalies": d.anomalies().iter().map(|a| format!("{a:?}")).collect::<Vec<_>>(),
    })
}

fn run(case: &CodecCase) -> Result<Value, Error> {
    match case.operation.as_str() {
        "decode" => {
            let bytes = input(case);
            decode_with_pfield(&bytes, mode(case)).map(|d| decoded(&d))
        }
        "decode_implicit" => {
            let format = format_of(param(case, "format"))?;
            decode_implicit(&input(case), &format, mode(case)).map(|d| decoded(&d))
        }
        "encode" => {
            let format = format_of(param(case, "format"))?;
            let code = code_of(format, param(case, "value"))?;
            let include = param(case, "include_pfield").as_bool().unwrap();
            let mut out = [0_u8; 32];
            let n = code.encode(include, &mut out)?;
            Ok(json!({ "output_hex": hex::encode(&out[..n]) }))
        }
        "cuc_to_ns" => {
            let raw = hex::decode(param(case, "fine_hex").as_str().unwrap()).unwrap();
            let v = CucValue::new(param(case, "coarse").as_u64().unwrap(), fine_left(&raw));
            let (s, ns) = v.to_seconds_nanos();
            Ok(json!({ "seconds": s, "nanoseconds": ns }))
        }
        "cuc_from_ns" => {
            let params = case.params.clone().map(Value::Object).unwrap();
            let f = CucFormat::new(
                CucEpoch::Tai1958,
                u8_of(&params, "coarse_octets"),
                u8_of(&params, "fine_octets"),
            )?;
            let v = Cuc::from_seconds_nanos(
                f,
                u64_of(&params, "seconds"),
                u32::try_from(u64_of(&params, "nanoseconds")).unwrap(),
            )?
            .value();
            Ok(json!({ "coarse": v.coarse(), "fine_hex": fine_raw(v.fine(), f.fine_octets()) }))
        }
        op => panic!("{}: unsupported operation {op}", case.id),
    }
}

fn handle(case: &CodecCase) -> Outcome {
    match run(case) {
        Ok(v) => Outcome::Ok(v.as_object().cloned().unwrap_or_else(Map::new)),
        Err(e) => Outcome::Err(e.kind().name().to_owned()),
    }
}

macro_rules! vector_test {
    ($name:ident, $file:literal) => {
        #[test]
        fn $name() {
            // req: VER-021, TV-001, TV-002, TV-003
            run_codec_file(concat!("time/", $file, ".json"), handle).assert_passed();
        }
    };
}

vector_test!(pfield, "pfield");
vector_test!(cuc, "cuc");
vector_test!(cds, "cds");
vector_test!(ccs, "ccs");
