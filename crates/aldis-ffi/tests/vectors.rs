//! Runs every live case of `test-vectors/{time,spp,epp,tm}/`, and of the files of `test-vectors/core/`
//! whose functions have a C ABI (`crc16`, `ocf`, `oid`; M2 plan D14), through the `extern "C"`
//! functions of `aldis-ffi`, with the testkit's runner and exact comparison (M2 plan D4; TV-012, VER-021,
//! CABI-012, CABI-020, CABI-021). The handlers build the C structs from the vector's raw values,
//! call the C ABI, and write the outputs from the C structs, so every value the vectors check
//! crosses the ABI, including the field codes of errors (M2 plan D3).
//!
//! Every error must come back consistently: the returned status is the negated kind of the
//! detail (CABI-020, CABI-021).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test-only harness: malformed vectors must fail loudly (RS-018)"
)]
#![expect(unsafe_code, reason = "the harness calls the C ABI it tests")]

use core::num::NonZeroU16;

use aldis::core::{Anomaly, ErrorKind, FieldId};
use aldis::epp::EppAnomaly;
use aldis::spp::SppAnomaly;
use aldis::time::TimeAnomaly;
use aldis::tm::TmAnomaly;
use aldis_ffi::crc::aldis_crc16;
use aldis_ffi::epp::*;
use aldis_ffi::ocf::*;
use aldis_ffi::oid::*;
use aldis_ffi::spp::*;
use aldis_ffi::spp_state::*;
use aldis_ffi::time::*;
use aldis_ffi::tm::*;
use aldis_ffi::{ALDIS_MODE_LENIENT, ALDIS_MODE_STRICT, ALDIS_OK, aldis_error_detail_t};
use aldis_testkit::{CodecCase, Mode, Outcome, hex, run_codec_file};
use serde_json::{Map, Value, json};

/// A failed call: what `Outcome::Err` carries.
struct Fail {
    kind: String,
    field: Option<NonZeroU16>,
}

type R<T> = Result<T, Fail>;

/// Calls `f` with a detail and turns a failure into its kind and field code.
fn call(f: impl FnOnce(*mut aldis_error_detail_t) -> i32) -> R<()> {
    let mut d = aldis_error_detail_t {
        struct_size: size_of_u32::<aldis_error_detail_t>(),
        ..aldis_error_detail_t::default()
    };
    let status = f(&raw mut d);
    if status == ALDIS_OK {
        return Ok(());
    }
    // The library never writes the caller's struct_size (CABI-021).
    assert_eq!(d.struct_size, size_of_u32::<aldis_error_detail_t>());
    assert!(status < 0, "status {status}");
    assert_eq!(
        -status,
        i32::try_from(d.kind).unwrap(),
        "status and detail disagree"
    );
    let kind = ErrorKind::from_code(u16::try_from(d.kind).unwrap()).unwrap();
    // A field code crosses the ABI only if the catalogue has it (API-082).
    let field = NonZeroU16::new(d.field).map(|code| {
        FieldId::from_code(code.get())
            .unwrap_or_else(|| panic!("field code {code} is not catalogued"))
            .code()
    });
    Err(Fail {
        kind: kind.name().to_owned(),
        field,
    })
}

fn handle(run: impl FnOnce() -> R<Value>) -> Outcome {
    match run() {
        Ok(v) => Outcome::Ok(v.as_object().cloned().unwrap_or_else(Map::new)),
        Err(f) => Outcome::Err {
            kind: f.kind,
            field: f.field,
        },
    }
}

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

fn u16_of(obj: &Value, name: &str) -> u16 {
    u16::try_from(u64_of(obj, name)).unwrap()
}

fn u32_of(obj: &Value, name: &str) -> u32 {
    u32::try_from(u64_of(obj, name)).unwrap()
}

fn usize_of(obj: &Value, name: &str) -> usize {
    usize::try_from(u64_of(obj, name)).unwrap()
}

fn str_of<'a>(obj: &'a Value, name: &str) -> &'a str {
    get(obj, name).as_str().unwrap()
}

fn params(case: &CodecCase) -> Value {
    Value::Object(case.params.clone().unwrap_or_default())
}

fn hex_of(obj: &Value, name: &str) -> Vec<u8> {
    hex::decode(str_of(obj, name)).unwrap()
}

fn input(case: &CodecCase) -> Vec<u8> {
    hex::decode(case.input_hex.as_deref().unwrap_or("")).unwrap()
}

fn mode(case: &CodecCase) -> u32 {
    match case.mode.unwrap_or_default() {
        Mode::Strict => ALDIS_MODE_STRICT,
        Mode::Lenient => ALDIS_MODE_LENIENT,
    }
}

/// The names of the anomaly bits of `mask`, in ascending bit order, as the Rust runners print
/// them.
fn anomalies<A: Anomaly>(mask: u32) -> Vec<String> {
    (0..32_u8)
        .filter(|b| mask & (1 << b) != 0)
        .map(|b| format!("{:?}", A::from_bit(b).unwrap()))
        .collect()
}

fn size_of_u32<T>() -> u32 {
    u32::try_from(size_of::<T>()).unwrap()
}

// ---------------------------------------------------------------- time

fn format_size() -> u32 {
    size_of_u32::<aldis_time_format_t>()
}

fn with_tail(mut f: aldis_time_format_t, obj: &Value) -> aldis_time_format_t {
    let tail = hex::decode(
        obj.get("pfield_tail_hex")
            .and_then(Value::as_str)
            .unwrap_or(""),
    )
    .unwrap();
    f.tail_len = u8::try_from(tail.len()).unwrap();
    f.tail[..tail.len()].copy_from_slice(&tail);
    f
}

/// The C format of a vector's format object (time vectors' names).
fn format_of(obj: &Value) -> aldis_time_format_t {
    let base = aldis_time_format_t {
        struct_size: format_size(),
        ..aldis_time_format_t::default()
    };
    let f = match str_of(obj, "code") {
        "cuc" => aldis_time_format_t {
            code: ALDIS_TIME_CUC,
            agency_epoch: u8::from(str_of(obj, "epoch") != "level1"),
            coarse_octets: u8_of(obj, "coarse_octets"),
            fine_octets: u8_of(obj, "fine_octets"),
            mission_bits: u8_of(obj, "mission_bits"),
            octet2: u8::from(get(obj, "octet2").as_bool().unwrap()),
            ..base
        },
        "cds" => aldis_time_format_t {
            code: ALDIS_TIME_CDS,
            agency_epoch: u8::from(str_of(obj, "epoch") != "ccsds"),
            day_len: if u64_of(obj, "day_bits") == 16 {
                ALDIS_TIME_CDS_DAY_16
            } else {
                ALDIS_TIME_CDS_DAY_24
            },
            subms: match str_of(obj, "subms") {
                "none" => ALDIS_TIME_CDS_SUBMS_NONE,
                "us" => ALDIS_TIME_CDS_SUBMS_MICROS,
                _ => ALDIS_TIME_CDS_SUBMS_PICOS,
            },
            ..base
        },
        "ccs" => aldis_time_format_t {
            code: ALDIS_TIME_CCS,
            variation: if str_of(obj, "variation") == "month_day" {
                ALDIS_TIME_MONTH_DAY
            } else {
                ALDIS_TIME_DAY_OF_YEAR
            },
            subsecond_octets: u8_of(obj, "subsecond_octets"),
            ..base
        },
        "agency" => aldis_time_format_t {
            code: ALDIS_TIME_AGENCY,
            t_len: u8_of(obj, "t_len"),
            ..base
        },
        other => panic!("unknown code {other}"),
    };
    with_tail(f, obj)
}

/// The C value of a vector's value object for a format.
fn value_of(code: u32, v: &Value) -> aldis_time_value_t {
    let mut out = aldis_time_value_t::default();
    match code {
        ALDIS_TIME_CUC => {
            out.coarse = u64_of(v, "coarse");
            let fine = hex_of(v, "fine_hex");
            out.fine[..fine.len()].copy_from_slice(&fine);
            out.fine_len = u8::try_from(fine.len()).unwrap();
        }
        ALDIS_TIME_CDS => {
            out.day = u32_of(v, "day");
            out.ms_of_day = u32_of(v, "ms_of_day");
            out.subms = u32_of(v, "subms");
        }
        ALDIS_TIME_CCS => {
            out.year = u16_of(v, "year");
            if let Some(doy) = get(v, "day_of_year").as_u64() {
                out.variation = ALDIS_TIME_DAY_OF_YEAR;
                out.day_of_year = u16::try_from(doy).unwrap();
            } else {
                out.variation = ALDIS_TIME_MONTH_DAY;
                out.month = u8_of(v, "month");
                out.day_of_month = u8_of(v, "day");
            }
            out.hour = u8_of(v, "hour");
            out.minute = u8_of(v, "minute");
            out.second = u8_of(v, "second");
            out.subsecond = u64_of(v, "subsecond");
        }
        _ => {
            let t = hex_of(v, "tfield_hex");
            out.tfield[..t.len()].copy_from_slice(&t);
            out.tfield_len = u8::try_from(t.len()).unwrap();
        }
    }
    out
}

fn tail_hex(f: &aldis_time_format_t) -> String {
    hex::encode(&f.tail[..usize::from(f.tail_len)])
}

/// The JSON value object of a C value, as the Rust runner describes it.
fn describe_value(f: &aldis_time_format_t, v: &aldis_time_value_t) -> Value {
    match f.code {
        ALDIS_TIME_CUC => {
            assert_eq!(v.fine_len, f.fine_octets);
            json!({ "coarse": v.coarse, "fine_hex": hex::encode(&v.fine[..usize::from(f.fine_octets)]) })
        }
        ALDIS_TIME_CDS => json!({ "day": v.day, "ms_of_day": v.ms_of_day, "subms": v.subms }),
        ALDIS_TIME_CCS => {
            let md = v.variation == ALDIS_TIME_MONTH_DAY;
            json!({
                "year": v.year,
                "month": if md { json!(v.month) } else { Value::Null },
                "day": if md { json!(v.day_of_month) } else { Value::Null },
                "day_of_year": if md { Value::Null } else { json!(v.day_of_year) },
                "hour": v.hour, "minute": v.minute, "second": v.second, "subsecond": v.subsecond,
            })
        }
        ALDIS_TIME_AGENCY => {
            json!({ "tfield_hex": hex::encode(&v.tfield[..usize::from(v.tfield_len)]) })
        }
        _ => Value::Null,
    }
}

/// The code name and JSON format object of a C format.
fn describe_format(f: &aldis_time_format_t) -> (&'static str, Value) {
    match f.code {
        ALDIS_TIME_CUC => (
            "cuc",
            json!({
                "epoch": if f.agency_epoch == 0 { "level1" } else { "agency" },
                "coarse_octets": f.coarse_octets,
                "fine_octets": f.fine_octets,
                "mission_bits": f.mission_bits,
                "octet2": f.octet2 == 1,
                "pfield_tail_hex": tail_hex(f),
            }),
        ),
        ALDIS_TIME_CDS => (
            "cds",
            json!({
                "epoch": if f.agency_epoch == 0 { "ccsds" } else { "agency" },
                "day_bits": if f.day_len == ALDIS_TIME_CDS_DAY_16 { 16 } else { 24 },
                "subms": match f.subms {
                    ALDIS_TIME_CDS_SUBMS_NONE => "none",
                    ALDIS_TIME_CDS_SUBMS_MICROS => "us",
                    _ => "ps",
                },
                "pfield_tail_hex": tail_hex(f),
            }),
        ),
        ALDIS_TIME_CCS => (
            "ccs",
            json!({
                "variation": if f.variation == ALDIS_TIME_MONTH_DAY { "month_day" } else { "day_of_year" },
                "subsecond_octets": f.subsecond_octets,
                "pfield_tail_hex": tail_hex(f),
            }),
        ),
        ALDIS_TIME_AGENCY => (
            "agency",
            json!({ "t_len": f.t_len, "pfield_tail_hex": tail_hex(f) }),
        ),
        _ => ("unknown", Value::Null),
    }
}

fn describe_decoded(d: &aldis_time_decoded_t) -> Value {
    let (code, format) = describe_format(&d.format);
    json!({
        "code": code,
        "pfield_hex": hex::encode(&d.pfield[..usize::from(d.pfield_len)]),
        "format": format,
        "value": describe_value(&d.format, &d.value),
        "consumed": d.len,
        "anomalies": anomalies::<TimeAnomaly>(d.anomalies),
    })
}

fn time_decode(bytes: &[u8], mode: u32) -> R<aldis_time_decoded_t> {
    let mut d = aldis_time_decoded_t::default();
    // SAFETY: every pointer is to a live local or slice.
    call(|e| unsafe { aldis_time_decode(bytes.as_ptr(), bytes.len(), mode, &raw mut d, e) })?;
    Ok(d)
}

fn time_encode(f: &aldis_time_format_t, v: &aldis_time_value_t, pfield: bool) -> R<Vec<u8>> {
    let mut out = [0_u8; 32];
    let mut n = 0;
    // SAFETY: every pointer is to a live local.
    call(|e| unsafe {
        aldis_time_encode(f, v, pfield, out.as_mut_ptr(), out.len(), &raw mut n, e)
    })?;
    Ok(out[..n].to_vec())
}

fn ascii_options(case: &CodecCase) -> aldis_time_ascii_options_t {
    aldis_time_ascii_options_t {
        struct_size: size_of_u32::<aldis_time_ascii_options_t>(),
        max_fraction_digits: case
            .params
            .as_ref()
            .and_then(|p| p.get("max_fraction_digits"))
            .map_or(12, |n| u8::try_from(n.as_u64().unwrap()).unwrap()),
    }
}

fn ascii_parse(text: &[u8], o: &aldis_time_ascii_options_t) -> R<aldis_time_ascii_t> {
    let mut t = aldis_time_ascii_t::default();
    // SAFETY: every pointer is to a live local or slice.
    call(|e| unsafe { aldis_time_ascii_parse(text.as_ptr(), text.len(), o, &raw mut t, e) })?;
    Ok(t)
}

fn ascii_format(t: &aldis_time_ascii_t) -> R<String> {
    let mut out = [0_u8; 33];
    let mut n = 0;
    // SAFETY: every pointer is to a live local.
    call(|e| unsafe { aldis_time_ascii_format(t, out.as_mut_ptr(), out.len(), &raw mut n, e) })?;
    let mut len = 0;
    // SAFETY: as above.
    call(|e| unsafe { aldis_time_ascii_encoded_len(t, &raw mut len, e) })?;
    assert_eq!(len, n);
    Ok(String::from_utf8(out[..n].to_vec()).unwrap())
}

fn ascii_json(t: &aldis_time_ascii_t) -> Value {
    let a = t.variation == ALDIS_TIME_MONTH_DAY;
    json!({
        "variation": if a { "A" } else { "B" },
        "year": t.year,
        "month": if a { json!(t.month) } else { Value::Null },
        "day": if a { json!(t.day) } else { Value::Null },
        "day_of_year": if a { Value::Null } else { json!(t.day_of_year) },
        "hour": t.hour, "minute": t.minute, "second": t.second,
        "fraction_digits": t.fraction_digits, "fraction": t.fraction, "z": t.utc == 1,
    })
}

fn ascii_from_json(v: &Value) -> aldis_time_ascii_t {
    let a = str_of(v, "variation") == "A";
    aldis_time_ascii_t {
        fraction: u64_of(v, "fraction"),
        year: u16_of(v, "year"),
        day_of_year: if a { 0 } else { u16_of(v, "day_of_year") },
        variation: if a {
            ALDIS_TIME_MONTH_DAY
        } else {
            ALDIS_TIME_DAY_OF_YEAR
        },
        month: if a { u8_of(v, "month") } else { 0 },
        day: if a { u8_of(v, "day") } else { 0 },
        hour: u8_of(v, "hour"),
        minute: u8_of(v, "minute"),
        second: u8_of(v, "second"),
        fraction_digits: u8_of(v, "fraction_digits"),
        utc: u8::from(get(v, "z").as_bool().unwrap()),
    }
}

fn subset_json(s: &aldis_time_ascii_subset_t) -> Value {
    let has = |bit: u32| s.present & bit != 0;
    let opt = |bit: u32, v: Value| if has(bit) { v } else { Value::Null };
    json!({
        "variation": opt(ALDIS_TIME_SUBSET_VARIATION, json!(if s.variation == ALDIS_TIME_MONTH_DAY { "A" } else { "B" })),
        "year": opt(ALDIS_TIME_SUBSET_YEAR, json!(s.year)),
        "month": opt(ALDIS_TIME_SUBSET_MONTH, json!(s.month)),
        "day": opt(ALDIS_TIME_SUBSET_DAY, json!(s.day)),
        "day_of_year": opt(ALDIS_TIME_SUBSET_DAY_OF_YEAR, json!(s.day_of_year)),
        "hour": opt(ALDIS_TIME_SUBSET_HOUR, json!(s.hour)),
        "minute": opt(ALDIS_TIME_SUBSET_MINUTE, json!(s.minute)),
        "second": opt(ALDIS_TIME_SUBSET_SECOND, json!(s.second)),
        "fraction_digits": opt(ALDIS_TIME_SUBSET_FRACTION, json!(s.fraction_digits)),
        "fraction": opt(ALDIS_TIME_SUBSET_FRACTION, json!(s.fraction)),
        "z": s.utc,
    })
}

/// A format object of a convert case, with its `code` added.
fn with_code(format: &Value, code: &str) -> Value {
    let mut f = format.clone();
    f.as_object_mut()
        .unwrap()
        .insert("code".to_owned(), json!(code));
    f
}

fn convert(case: &CodecCase) -> R<Value> {
    let mut cfg = aldis_time_convert_config_t {
        struct_size: size_of_u32::<aldis_time_convert_config_t>(),
        ..aldis_time_convert_config_t::default()
    };
    if let Some(e) = case.params.as_ref().and_then(|p| p.get("epoch")) {
        cfg.has_agency_epoch = 1;
        cfg.agency_epoch = aldis_time_date_t {
            year: u16_of(e, "year"),
            month: u8_of(e, "month"),
            day: u8_of(e, "day"),
        };
    }
    let from = param(case, "from");
    let mut instant = aldis_time_instant_t::default();
    match str_of(from, "code") {
        "ascii" => {
            let options = aldis_time_ascii_options_t {
                struct_size: size_of_u32::<aldis_time_ascii_options_t>(),
                max_fraction_digits: 12,
            };
            instant.format = aldis_time_format_t {
                struct_size: format_size(),
                code: ALDIS_TIME_ASCII,
                ..aldis_time_format_t::default()
            };
            instant.ascii = ascii_parse(str_of(from, "text").as_bytes(), &options)?;
        }
        code => {
            instant.format = format_of(&with_code(get(from, "format"), code));
            instant.value = value_of(instant.format.code, get(from, "value"));
        }
    }
    let to = param(case, "to");
    if str_of(to, "code") == "ascii" {
        cfg.to = aldis_time_format_t {
            struct_size: format_size(),
            code: ALDIS_TIME_ASCII,
            ..aldis_time_format_t::default()
        };
        cfg.ascii = aldis_time_ascii_format_t {
            variation: if str_of(to, "variation") == "A" {
                ALDIS_TIME_MONTH_DAY
            } else {
                ALDIS_TIME_DAY_OF_YEAR
            },
            fraction_digits: u8_of(to, "fraction_digits"),
            utc: u8::from(get(to, "z").as_bool().unwrap()),
        };
    } else {
        cfg.to = format_of(to);
    }
    let mut out = aldis_time_instant_t::default();
    // SAFETY: every pointer is to a live local.
    call(|e| unsafe { aldis_time_convert(&raw const instant, &raw const cfg, &raw mut out, e) })?;
    Ok(if out.format.code == ALDIS_TIME_ASCII {
        json!({ "text": ascii_format(&out.ascii)? })
    } else {
        json!({ "value": describe_value(&out.format, &out.value) })
    })
}

fn time_run(case: &CodecCase) -> R<Value> {
    match case.operation.as_str() {
        "decode" => time_decode(&input(case), mode(case)).map(|d| describe_decoded(&d)),
        "decode_implicit" => {
            let f = format_of(param(case, "format"));
            let bytes = input(case);
            let mut d = aldis_time_decoded_t::default();
            // SAFETY: every pointer is to a live local or slice.
            call(|e| unsafe {
                aldis_time_decode_implicit(
                    bytes.as_ptr(),
                    bytes.len(),
                    &raw const f,
                    mode(case),
                    &raw mut d,
                    e,
                )
            })?;
            Ok(describe_decoded(&d))
        }
        "encode" => {
            let f = format_of(param(case, "format"));
            let v = value_of(f.code, param(case, "value"));
            let include = param(case, "include_pfield").as_bool().unwrap();
            let out = time_encode(&f, &v, include)?;
            if include {
                // The P-field the format writes is the one the encoder wrote.
                let mut p = [0_u8; 8];
                let mut n = 0;
                // SAFETY: every pointer is to a live local.
                call(|e| unsafe {
                    aldis_time_format_pfield(&raw const f, p.as_mut_ptr(), p.len(), &raw mut n, e)
                })
                .unwrap_or_else(|_| panic!("{}: pfield", case.id));
                assert_eq!(&out[..n], &p[..n], "{}", case.id);
            }
            Ok(json!({ "output_hex": hex::encode(&out) }))
        }
        "cuc_to_ns" => {
            let p = params(case);
            let v = value_of(ALDIS_TIME_CUC, &p);
            let (mut s, mut ns) = (0_u64, 0_u32);
            // SAFETY: every pointer is to a live local.
            call(|e| unsafe { aldis_time_cuc_to_ns(&raw const v, &raw mut s, &raw mut ns, e) })?;
            Ok(json!({ "seconds": s, "nanoseconds": ns }))
        }
        "cuc_from_ns" => {
            let p = params(case);
            let f = aldis_time_format_t {
                struct_size: format_size(),
                code: ALDIS_TIME_CUC,
                coarse_octets: u8_of(&p, "coarse_octets"),
                fine_octets: u8_of(&p, "fine_octets"),
                ..aldis_time_format_t::default()
            };
            let mut v = aldis_time_value_t::default();
            // SAFETY: every pointer is to a live local.
            call(|e| unsafe {
                aldis_time_cuc_from_ns(
                    &raw const f,
                    u64_of(&p, "seconds"),
                    u32_of(&p, "nanoseconds"),
                    &raw mut v,
                    e,
                )
            })?;
            Ok(
                json!({ "coarse": v.coarse, "fine_hex": hex::encode(&v.fine[..usize::from(v.fine_len)]) }),
            )
        }
        "ascii_parse" => {
            let text = param(case, "text").as_str().unwrap().as_bytes();
            let t = ascii_parse(text, &ascii_options(case))?;
            // The formatted code is the input again (TIME-031 round trip).
            assert_eq!(
                ascii_format(&t).ok().as_deref(),
                core::str::from_utf8(text).ok(),
                "{}",
                case.id
            );
            Ok(ascii_json(&t))
        }
        "ascii_format" => {
            let t = ascii_from_json(param(case, "value"));
            Ok(json!({ "text": ascii_format(&t)? }))
        }
        "ascii_subset" => {
            let text = param(case, "text").as_str().unwrap().as_bytes();
            let o = ascii_options(case);
            let mut s = aldis_time_ascii_subset_t::default();
            // SAFETY: every pointer is to a live local or slice.
            call(|e| unsafe {
                aldis_time_ascii_subset_parse(
                    text.as_ptr(),
                    text.len(),
                    &raw const o,
                    &raw mut s,
                    e,
                )
            })?;
            Ok(subset_json(&s))
        }
        "convert" => convert(case),
        op => panic!("{}: unsupported operation {op}", case.id),
    }
}

// ---------------------------------------------------------------- SPP

fn spp_config(case: &CodecCase) -> aldis_spp_config_t {
    aldis_spp_config_t {
        struct_size: size_of_u32::<aldis_spp_config_t>(),
        max_packet_len: case.config.as_ref().map_or(ALDIS_SPP_MAX_PACKET_LEN, |c| {
            u32::try_from(c["max_packet_len"].as_u64().unwrap()).unwrap()
        }),
    }
}

fn spp_fields(p: &Value) -> aldis_spp_fields_t {
    aldis_spp_fields_t {
        packet_type: u8_of(p, "packet_type"),
        sequence_flags: u8_of(p, "sequence_flags"),
        apid: u16_of(p, "apid"),
        sequence_count: u16_of(p, "sequence_count"),
    }
}

fn spp_decode(bytes: &[u8], mode: u32) -> R<aldis_spp_packet_t> {
    let mut p = aldis_spp_packet_t::default();
    // SAFETY: every pointer is to a live local or slice.
    call(|e| unsafe { aldis_spp_decode(bytes.as_ptr(), bytes.len(), mode, &raw mut p, e) })?;
    Ok(p)
}

fn spp_json(p: &aldis_spp_packet_t, bytes: &[u8]) -> Value {
    let tc = p.fields.packet_type == ALDIS_SPP_TELECOMMAND;
    json!({
        "packet_type": p.fields.packet_type,
        "secondary_header_flag": u8::from(p.secondary_header_flag),
        "apid": p.fields.apid,
        "idle": p.idle,
        "sequence_flags": p.fields.sequence_flags,
        "sequence_count": p.fields.sequence_count,
        "packet_name": if tc { json!(p.fields.sequence_count) } else { Value::Null },
        "data_len": p.data_len,
        "packet_len": p.packet_len,
        "data_hex": hex::encode(&bytes[p.data_offset..p.data_offset + p.data_len]),
        "consumed": p.packet_len,
        "anomalies": anomalies::<SppAnomaly>(p.anomalies),
    })
}

fn spp_encode(case: &CodecCase) -> R<Value> {
    let cfg = spp_config(case);
    let p = params(case);
    let fields = spp_fields(&p);
    let sec = hex_of(&p, "secondary_header_hex");
    let user = hex_of(&p, "user_data_hex");
    let mut out = vec![0_u8; 6 + sec.len() + user.len()];
    let mut n = 0;
    // SAFETY: every pointer is to a live local or vector.
    call(|e| unsafe {
        aldis_spp_encode(
            &raw const cfg,
            &raw const fields,
            sec.as_ptr(),
            sec.len(),
            user.as_ptr(),
            user.len(),
            out.as_mut_ptr(),
            out.len(),
            &raw mut n,
            e,
        )
    })?;
    // The two-phase writer, as two stateless calls, writes the same octets (API-020, D4).
    let mut two = vec![0_u8; n];
    let mut layout = aldis_spp_writer_layout_t::default();
    // SAFETY: every pointer is to a live local or vector.
    call(|e| unsafe {
        aldis_spp_writer_begin(
            &raw const cfg,
            &raw const fields,
            sec.len(),
            user.len(),
            two.as_mut_ptr(),
            two.len(),
            &raw mut layout,
            e,
        )
    })
    .unwrap_or_else(|_| panic!("{}: writer_begin", case.id));
    two[layout.secondary_header_offset..][..layout.secondary_header_len].copy_from_slice(&sec);
    two[layout.user_data_offset..][..layout.user_data_cap].copy_from_slice(&user);
    let mut m = 0;
    // SAFETY: as above.
    call(|e| unsafe {
        aldis_spp_writer_finish(
            &raw const cfg,
            &raw const fields,
            sec.len(),
            user.len(),
            user.len(),
            two.as_mut_ptr(),
            two.len(),
            &raw mut m,
            e,
        )
    })
    .unwrap_or_else(|_| panic!("{}: writer_finish", case.id));
    assert_eq!(
        (m, &two[..]),
        (n, &out[..n]),
        "{}: two-phase writer differs",
        case.id
    );
    Ok(json!({ "output_hex": hex::encode(&out[..n]) }))
}

fn spp_layout(kind: u32) -> aldis_spp_secondary_layout_t {
    aldis_spp_secondary_layout_t {
        struct_size: size_of_u32::<aldis_spp_secondary_layout_t>(),
        kind,
        time_format: aldis_time_format_t {
            struct_size: format_size(),
            ..aldis_time_format_t::default()
        },
        ..aldis_spp_secondary_layout_t::default()
    }
}

fn spp_split(
    bytes: &[u8],
    mode: u32,
    layout: &aldis_spp_secondary_layout_t,
) -> R<aldis_spp_split_t> {
    let mut s = aldis_spp_split_t::default();
    // SAFETY: every pointer is to a live local or slice.
    call(|e| unsafe { aldis_spp_split(bytes.as_ptr(), bytes.len(), mode, layout, &raw mut s, e) })?;
    Ok(s)
}

fn split_time(case: &CodecCase) -> R<Value> {
    let l = param(case, "layout");
    let f = get(l, "format");
    let mut layout = spp_layout(ALDIS_SPP_SECONDARY_TIME);
    layout.time_pfield = u8::from(get(l, "pfield").as_bool().unwrap());
    layout.ancillary_len = usize_of(l, "ancillary_len");
    match str_of(f, "code") {
        "cuc" => {
            layout.time_format.code = ALDIS_TIME_CUC;
            layout.time_format.agency_epoch = u8::from(get(f, "level2").as_bool().unwrap());
            layout.time_format.coarse_octets = u8_of(f, "coarse");
            layout.time_format.fine_octets = u8_of(f, "fine");
        }
        "cds" => {
            layout.time_format.code = ALDIS_TIME_CDS;
            layout.time_format.agency_epoch = u8::from(get(f, "agency_epoch").as_bool().unwrap());
            layout.time_format.day_len = if get(f, "day24").as_bool().unwrap() {
                ALDIS_TIME_CDS_DAY_24
            } else {
                ALDIS_TIME_CDS_DAY_16
            };
            layout.time_format.subms = u8_of(f, "subms");
        }
        "ccs" => {
            layout.time_format.code = ALDIS_TIME_CCS;
            layout.time_format.subsecond_octets = u8_of(f, "subsecond_octets");
        }
        code => panic!("time code {code}"),
    }
    let bytes = input(case);
    let s = spp_split(&bytes, mode(case), &layout)?;
    assert!(s.has_time, "{}", case.id);
    let sec = &bytes[s.secondary_header_offset..][..s.secondary_header_len];
    let time_code = &sec[..s.time.len];
    // The decoded code re-encodes to the received octets through the C ABI (API-020).
    let reencoded = time_encode(&s.time.format, &s.time.value, s.time.pfield_len > 0)
        .unwrap_or_else(|_| panic!("{}: re-encode", case.id));
    assert_eq!(
        reencoded, time_code,
        "{}: time code re-encodes differently",
        case.id
    );
    Ok(json!({
        "secondary_header_hex": hex::encode(sec),
        "user_data_hex": hex::encode(&bytes[s.user_data_offset..][..s.user_data_len]),
        "time_code_hex": hex::encode(time_code),
        "ancillary_hex": hex::encode(&bytes[s.ancillary_offset..][..s.ancillary_len]),
    }))
}

fn decode_stream(case: &CodecCase) -> Value {
    let bytes = input(case);
    let (mut at, mut packets, mut stop) = (0, Vec::new(), Value::Null);
    while at < bytes.len() {
        match spp_decode(&bytes[at..], mode(case)) {
            Ok(p) => {
                packets.push(json!({
                    "apid": p.fields.apid,
                    "sequence_count": p.fields.sequence_count,
                    "consumed": p.packet_len,
                    "anomalies": anomalies::<SppAnomaly>(p.anomalies),
                }));
                at += p.packet_len;
            }
            Err(e) => {
                stop = json!(e.kind);
                break;
            }
        }
    }
    json!({ "packets": packets, "stop_error": stop, "offset": at })
}

/// Storage for one helper: `uint64_t` words, as a C caller declares it (CABI-031).
fn storage(size: usize) -> Vec<u64> {
    vec![0_u64; size.div_ceil(8)]
}

fn track(case: &CodecCase) -> Value {
    let mut words = storage(aldis_spp_tracker_storage_size());
    let cfg = aldis_spp_tracker_config_t {
        struct_size: size_of_u32::<aldis_spp_tracker_config_t>(),
        capacity: u32::try_from(usize_of(&params(case), "capacity")).unwrap(),
    };
    let mut h: *mut aldis_spp_tracker_t = core::ptr::null_mut();
    // SAFETY: `words` is live, aligned storage of the published size.
    call(|e| unsafe {
        aldis_spp_tracker_init(
            words.as_mut_ptr().cast(),
            words.len() * 8,
            &raw const cfg,
            &raw mut h,
            e,
        )
    })
    .unwrap_or_else(|_| panic!("{}: tracker_init", case.id));
    let mut results = Vec::new();
    for ev in param(case, "events").as_array().unwrap() {
        let apid = u16_of(ev, "apid");
        results.push(match str_of(ev, "op") {
            "track" => {
                let mut t = aldis_spp_tracked_t::default();
                // SAFETY: `h` is the live handle; `t` a live local.
                match call(|e| unsafe {
                    aldis_spp_tracker_track(h, apid, u16_of(ev, "count"), &raw mut t, e)
                }) {
                    Err(f) => json!({ "error": f.kind }),
                    Ok(()) => match t.continuity {
                        ALDIS_SPP_TRACK_FIRST => json!({ "continuity": "First" }),
                        ALDIS_SPP_TRACK_IN_SEQUENCE => json!({ "continuity": "InSequence" }),
                        ALDIS_SPP_TRACK_REPEAT => json!({ "continuity": "Repeat" }),
                        ALDIS_SPP_TRACK_BACKWARDS => json!({ "continuity": "Backwards" }),
                        _ => json!({ "continuity": "Gap", "missing": t.missing }),
                    },
                }
            }
            "forget" => {
                let mut forgot = false;
                // SAFETY: as above.
                call(|e| unsafe { aldis_spp_tracker_forget(h, apid, &raw mut forgot, e) })
                    .unwrap_or_else(|_| panic!("{}: forget", case.id));
                json!({ "forgot": forgot })
            }
            op => panic!("unknown tracker op {op}"),
        });
    }
    // SAFETY: `h` is the live handle.
    unsafe { aldis_spp_tracker_deinit(h) };
    json!({ "results": results })
}

fn segment(case: &CodecCase) -> R<Value> {
    let cfg = spp_config(case);
    let p = params(case);
    let sdu = hex_of(&p, "sdu_hex");
    let sec = hex_of(&p, "secondary_header_hex");
    let seg_cfg = aldis_spp_segmenter_config_t {
        struct_size: size_of_u32::<aldis_spp_segmenter_config_t>(),
        packet_type: u8_of(&p, "packet_type"),
        apid: u16_of(&p, "apid"),
        max_user_len: usize_of(&p, "max_user_len"),
    };
    let mut words = storage(ALDIS_SPP_SEGMENTER_STORAGE_SIZE);
    let mut h: *mut aldis_spp_segmenter_t = core::ptr::null_mut();
    // SAFETY: `words` is live, aligned storage of the published size; the rest are locals.
    call(|e| unsafe {
        aldis_spp_segmenter_init(
            words.as_mut_ptr().cast(),
            words.len() * 8,
            &raw const cfg,
            &raw const seg_cfg,
            sdu.as_ptr(),
            sdu.len(),
            &raw mut h,
            e,
        )
    })?;
    let mut count = u16_of(&p, "sequence_count");
    let mut packets = Vec::new();
    let mut out = vec![0_u8; 70_000];
    loop {
        let (mut n, mut next) = (0, 0_u16);
        // SAFETY: `h` is the live handle; the buffers are live and disjoint.
        call(|e| unsafe {
            aldis_spp_segmenter_next(
                h,
                sdu.as_ptr(),
                sdu.len(),
                count,
                sec.as_ptr(),
                sec.len(),
                out.as_mut_ptr(),
                out.len(),
                &raw mut n,
                &raw mut next,
                e,
            )
        })?;
        if n == 0 {
            break;
        }
        packets.push(json!(hex::encode(&out[..n])));
        count = next;
    }
    let mut remaining = 1;
    // SAFETY: as above.
    call(|e| unsafe { aldis_spp_segmenter_remaining(h, &raw mut remaining, e) })
        .unwrap_or_else(|_| panic!("{}: remaining", case.id));
    assert_eq!(remaining, 0, "{}", case.id);
    // SAFETY: as above.
    unsafe { aldis_spp_segmenter_deinit(h) };
    Ok(json!({ "packets_hex": packets, "next_count": count }))
}

fn discard_name(d: u32) -> &'static str {
    match d {
        ALDIS_SPP_DISCARD_GAP => "Gap",
        ALDIS_SPP_DISCARD_RESTARTED => "Restarted",
        ALDIS_SPP_DISCARD_INTERRUPTED => "Interrupted",
        ALDIS_SPP_DISCARD_ORPHAN => "Orphan",
        ALDIS_SPP_DISCARD_OVERFLOW => "Overflow",
        other => panic!("discard {other}"),
    }
}

fn reassemble(case: &CodecCase) -> Value {
    let p = params(case);
    let mut buf = vec![0_u8; usize_of(&p, "buffer_len")];
    let mut words = storage(ALDIS_SPP_REASSEMBLER_STORAGE_SIZE);
    let mut h: *mut aldis_spp_reassembler_t = core::ptr::null_mut();
    // SAFETY: `words` is live, aligned storage of the published size.
    call(|e| unsafe {
        aldis_spp_reassembler_init(
            words.as_mut_ptr().cast(),
            words.len() * 8,
            u16_of(&p, "apid"),
            &raw mut h,
            e,
        )
    })
    .unwrap_or_else(|_| panic!("{}: reassembler_init", case.id));
    let layout = spp_layout(ALDIS_SPP_SECONDARY_ABSENT);
    let mut results = Vec::new();
    for packet in get(&p, "packets_hex").as_array().unwrap() {
        let bytes = hex::decode(packet.as_str().unwrap()).unwrap();
        let mut r = aldis_spp_reassembly_t::default();
        // SAFETY: `h` is the live handle; the buffers are live and disjoint.
        let pushed = call(|e| unsafe {
            aldis_spp_reassembler_push(
                h,
                bytes.as_ptr(),
                bytes.len(),
                ALDIS_MODE_STRICT,
                &raw const layout,
                buf.as_mut_ptr(),
                buf.len(),
                &raw mut r,
                e,
            )
        });
        results.push(if let Err(f) = pushed {
            json!({ "error": f.kind })
        } else {
            let discarded = match r.discarded {
                ALDIS_SPP_DISCARD_NONE => Value::Null,
                d => json!(discard_name(d)),
            };
            match r.status {
                ALDIS_SPP_REASSEMBLY_IN_PROGRESS => {
                    json!({ "discarded": discarded, "status": "InProgress" })
                }
                ALDIS_SPP_REASSEMBLY_COMPLETE => json!({
                    "discarded": discarded,
                    "status": "Complete",
                    "sdu_hex": hex::encode(&buf[..r.sdu_len]),
                }),
                _ => json!({
                    "discarded": discarded,
                    "status": "Dropped",
                    "reason": discard_name(r.reason),
                }),
            }
        });
    }
    // SAFETY: `h` is the live handle.
    unsafe { aldis_spp_reassembler_deinit(h) };
    json!({ "results": results })
}

fn spp_run(case: &CodecCase) -> R<Value> {
    match case.operation.as_str() {
        "decode" => {
            let bytes = input(case);
            spp_decode(&bytes, mode(case)).map(|p| spp_json(&p, &bytes))
        }
        "peek_length" => {
            let bytes = input(case);
            let mut n = 0;
            // SAFETY: every pointer is to a live local or slice.
            call(|e| unsafe { aldis_spp_peek_length(bytes.as_ptr(), bytes.len(), &raw mut n, e) })?;
            Ok(json!({ "len": n }))
        }
        "encode" => spp_encode(case),
        "idle" => {
            let cfg = spp_config(case);
            let p = params(case);
            let pattern = hex_of(&p, "pattern_hex");
            let total = usize_of(&p, "total_len");
            let mut out = vec![0_u8; total];
            let mut n = 0;
            // SAFETY: every pointer is to a live local or vector.
            call(|e| unsafe {
                aldis_spp_idle(
                    &raw const cfg,
                    total,
                    u16_of(&p, "sequence_count"),
                    pattern.as_ptr(),
                    pattern.len(),
                    out.as_mut_ptr(),
                    out.len(),
                    &raw mut n,
                    e,
                )
            })?;
            Ok(json!({ "output_hex": hex::encode(&out[..n]) }))
        }
        "octet_string" => {
            let cfg = spp_config(case);
            let p = params(case);
            let octets = hex_of(&p, "octets_hex");
            let mut out = vec![0_u8; octets.len() + 6];
            let (mut n, mut next) = (0, 0_u16);
            // SAFETY: every pointer is to a live local or vector.
            call(|e| unsafe {
                aldis_spp_octet_string_encode(
                    &raw const cfg,
                    u8_of(&p, "packet_type"),
                    u16_of(&p, "apid"),
                    u16_of(&p, "sequence_count"),
                    get(&p, "secondary_header_indicator").as_bool().unwrap(),
                    octets.as_ptr(),
                    octets.len(),
                    out.as_mut_ptr(),
                    out.len(),
                    &raw mut n,
                    &raw mut next,
                    e,
                )
            })?;
            Ok(json!({ "output_hex": hex::encode(&out[..n]), "next_count": next }))
        }
        "decode_stream" => Ok(decode_stream(case)),
        "split" => {
            let bytes = input(case);
            let layout = match param(case, "secondary_header_len") {
                Value::Null => spp_layout(ALDIS_SPP_SECONDARY_ABSENT),
                v => aldis_spp_secondary_layout_t {
                    opaque_len: usize::try_from(v.as_u64().unwrap()).unwrap(),
                    ..spp_layout(ALDIS_SPP_SECONDARY_OPAQUE)
                },
            };
            let s = spp_split(&bytes, mode(case), &layout)?;
            assert!(!s.has_time);
            Ok(json!({
                "secondary_header_hex": hex::encode(&bytes[s.secondary_header_offset..][..s.secondary_header_len]),
                "user_data_hex": hex::encode(&bytes[s.user_data_offset..][..s.user_data_len]),
            }))
        }
        "split_time" => split_time(case),
        "track" => Ok(track(case)),
        "segment" => segment(case),
        "reassemble" => Ok(reassemble(case)),
        op => panic!("{}: unsupported operation {op}", case.id),
    }
}

// ---------------------------------------------------------------- EPP

fn epp_any_config() -> aldis_epp_config_t {
    aldis_epp_config_t {
        struct_size: size_of_u32::<aldis_epp_config_t>(),
        min_data_len: 0,
        max_data_len: ALDIS_EPP_MAX_DATA_LEN,
        valid_epis: 0xFF,
        valid_extended_epis: 0xFFFF,
    }
}

fn epp_config(case: &CodecCase) -> aldis_epp_config_t {
    let c = Value::Object(case.config.clone().unwrap());
    let mask = |name: &str| -> u16 {
        get(&c, name)
            .as_array()
            .unwrap()
            .iter()
            .map(|v| 1_u16 << v.as_u64().unwrap())
            .fold(0, |a, b| a | b)
    };
    aldis_epp_config_t {
        struct_size: size_of_u32::<aldis_epp_config_t>(),
        min_data_len: u32_of(&c, "min_data_len"),
        max_data_len: u32_of(&c, "max_data_len"),
        valid_epis: u8::try_from(mask("valid_epis")).unwrap(),
        valid_extended_epis: mask("valid_extended_epis"),
    }
}

fn epp_fields(p: &Value) -> aldis_epp_fields_t {
    aldis_epp_fields_t {
        epi: u8_of(p, "epi"),
        epi_extension: u8_of(p, "epi_extension"),
        user_defined: u8_of(p, "user_defined"),
    }
}

fn header_size(p: &Value) -> u32 {
    match get(p, "header_size") {
        Value::String(s) if s == "auto" => ALDIS_EPP_HEADER_AUTO,
        v => u32::try_from(v.as_u64().unwrap()).unwrap(),
    }
}

fn epp_decode(bytes: &[u8], mode: u32) -> R<aldis_epp_packet_t> {
    let mut p = aldis_epp_packet_t::default();
    // SAFETY: every pointer is to a live local or slice.
    call(|e| unsafe { aldis_epp_decode(bytes.as_ptr(), bytes.len(), mode, &raw mut p, e) })?;
    Ok(p)
}

fn epp_header(
    cfg: &aldis_epp_config_t,
    fields: &aldis_epp_fields_t,
    size: u32,
    data_len: usize,
) -> R<(String, u32)> {
    let mut out = [0_u8; 8];
    let (mut n, mut total) = (0, 0_u32);
    // SAFETY: every pointer is to a live local.
    call(|e| unsafe {
        aldis_epp_header_encode(
            cfg,
            fields,
            size,
            data_len,
            out.as_mut_ptr(),
            out.len(),
            &raw mut n,
            &raw mut total,
            e,
        )
    })?;
    Ok((hex::encode(&out[..n]), total))
}

fn epp_encode(case: &CodecCase) -> R<Value> {
    let p = params(case);
    let data = hex_of(&p, "data_hex");
    let cfg = epp_any_config();
    let fields = epp_fields(&p);
    let size = header_size(&p);
    let mut out = vec![0_u8; data.len() + 8];
    let mut n = 0;
    // SAFETY: every pointer is to a live local or vector.
    call(|e| unsafe {
        aldis_epp_encode(
            &raw const cfg,
            &raw const fields,
            size,
            data.as_ptr(),
            data.len(),
            out.as_mut_ptr(),
            out.len(),
            &raw mut n,
            e,
        )
    })?;
    // The two-phase writer, as two stateless calls, writes the same octets (API-020, D4).
    let mut two = vec![0_u8; data.len() + 8];
    let mut layout = aldis_epp_writer_layout_t::default();
    // SAFETY: as above.
    call(|e| unsafe {
        aldis_epp_writer_begin(
            &raw const cfg,
            &raw const fields,
            size,
            data.len(),
            two.as_mut_ptr(),
            two.len(),
            &raw mut layout,
            e,
        )
    })
    .unwrap_or_else(|_| panic!("{}: writer_begin", case.id));
    two[layout.data_offset..][..layout.data_cap].copy_from_slice(&data);
    let mut m = 0;
    // SAFETY: as above.
    call(|e| unsafe {
        aldis_epp_writer_finish(
            &raw const cfg,
            &raw const fields,
            size,
            data.len(),
            data.len(),
            two.as_mut_ptr(),
            two.len(),
            &raw mut m,
            e,
        )
    })
    .unwrap_or_else(|_| panic!("{}: writer_finish", case.id));
    assert_eq!(
        (m, &two[..m]),
        (n, &out[..n]),
        "{}: two-phase writer differs",
        case.id
    );
    Ok(json!({ "output_hex": hex::encode(&out[..n]) }))
}

#[expect(clippy::too_many_lines, reason = "one arm per vector operation")]
fn epp_run(case: &CodecCase) -> R<Value> {
    match case.operation.as_str() {
        "decode" => {
            let bytes = input(case);
            let p = epp_decode(&bytes, mode(case))?;
            Ok(json!({
                "epi": p.epi,
                "user_defined": if p.has_extension { json!(p.user_defined) } else { Value::Null },
                "epi_extension": if p.has_extension { json!(p.epi_extension) } else { Value::Null },
                "ccsds_defined": if p.has_ccsds_defined { json!(p.ccsds_defined) } else { Value::Null },
                "header_len": p.header_len,
                "packet_len": p.packet_len,
                "data_hex": hex::encode(&bytes[p.data_offset..][..p.data_len]),
                "consumed": p.packet_len,
                "anomalies": anomalies::<EppAnomaly>(p.anomalies),
            }))
        }
        "reencode_header" => {
            // The owned header of the view, written again with the header encoder: the
            // extension only with EPI 110, the CCSDS Defined Field as 0 (EPP-006, EPP-007).
            let p = epp_decode(&input(case), mode(case))?;
            let fields = aldis_epp_fields_t {
                epi: p.epi,
                epi_extension: if p.epi == ALDIS_EPP_EPI_EXTENDED {
                    p.epi_extension
                } else {
                    0
                },
                user_defined: p.user_defined,
            };
            let (header_hex, _) = epp_header(
                &epp_any_config(),
                &fields,
                u32::from(p.header_len),
                p.data_len,
            )
            .unwrap_or_else(|_| panic!("{}: re-encode", case.id));
            Ok(json!({ "header_hex": header_hex }))
        }
        "peek_length" => {
            let bytes = input(case);
            let mut n = 0;
            // SAFETY: every pointer is to a live local or slice.
            call(|e| unsafe { aldis_epp_peek_length(bytes.as_ptr(), bytes.len(), &raw mut n, e) })?;
            Ok(json!({ "len": n }))
        }
        "encode" => epp_encode(case),
        "encode_header" => {
            let p = params(case);
            let (header_hex, total) = epp_header(
                &epp_any_config(),
                &epp_fields(&p),
                header_size(&p),
                usize_of(&p, "data_len"),
            )?;
            Ok(json!({ "header_hex": header_hex, "total_len": total }))
        }
        "idle" => {
            let p = params(case);
            let total = usize_of(&p, "total_len");
            let mut out = vec![0_u8; total];
            let mut n = 0;
            // SAFETY: every pointer is to a live local or vector.
            call(|e| unsafe {
                aldis_epp_idle(
                    total,
                    u8_of(&p, "fill"),
                    out.as_mut_ptr(),
                    out.len(),
                    &raw mut n,
                    e,
                )
            })?;
            Ok(json!({ "output_hex": hex::encode(&out[..n]) }))
        }
        "idle_header" => {
            let p = params(case);
            let mut out = [0_u8; 8];
            let (mut n, mut total) = (0, 0_u32);
            // SAFETY: every pointer is to a live local.
            call(|e| unsafe {
                aldis_epp_idle_header(
                    usize_of(&p, "total_len"),
                    out.as_mut_ptr(),
                    out.len(),
                    &raw mut n,
                    &raw mut total,
                    e,
                )
            })?;
            Ok(json!({ "header_hex": hex::encode(&out[..n]), "total_len": total }))
        }
        "validate" => {
            let cfg = epp_config(case);
            let bytes = input(case);
            // SAFETY: every pointer is to a live local or slice.
            call(|e| unsafe {
                aldis_epp_validate(
                    bytes.as_ptr(),
                    bytes.len(),
                    ALDIS_MODE_STRICT,
                    &raw const cfg,
                    e,
                )
            })?;
            Ok(json!({}))
        }
        "config" => {
            let cfg = epp_config(case);
            // SAFETY: `cfg` is a live local.
            call(|e| unsafe { aldis_epp_config_check(&raw const cfg, e) })?;
            Ok(json!({}))
        }
        op => panic!("{}: unsupported operation {op}", case.id),
    }
}

// ---------------------------------------------------------------- aldis-core: CRC, OCF, OID

/// An OID generator of one form in storage of its own (CABI-031): the storage, and the handle
/// into it.
struct Oid {
    _words: Vec<u64>,
    h: *mut aldis_oid_generator_t,
}

impl Oid {
    fn new(form: u32) -> Oid {
        let mut words = storage(aldis_oid_generator_storage_size());
        let mut h = core::ptr::null_mut();
        // SAFETY: `words` is live, aligned storage of the published size.
        call(|e| unsafe {
            aldis_oid_generator_init(
                words.as_mut_ptr().cast(),
                words.len() * 8,
                form,
                &raw mut h,
                e,
            )
        })
        .unwrap_or_else(|f| panic!("aldis_oid_generator_init: {}", f.kind));
        Oid { _words: words, h }
    }

    /// The next `len` octets, from `aldis_oid_generator_fill`.
    fn fill(&self, len: usize) -> Vec<u8> {
        let mut out = vec![0_u8; len];
        // SAFETY: `h` is the handle of the live storage; `out` is a live vector.
        call(|e| unsafe { aldis_oid_generator_fill(self.h, out.as_mut_ptr(), len, e) })
            .unwrap_or_else(|f| panic!("aldis_oid_generator_fill: {}", f.kind));
        out
    }
}

impl Drop for Oid {
    fn drop(&mut self) {
        // SAFETY: `h` is the handle of the storage this struct still owns.
        unsafe { aldis_oid_generator_deinit(self.h) }
    }
}

/// Fills of `lens` octets from one generator of each form, which must agree (CMN-022).
fn oid_fills(lens: &[usize]) -> Vec<String> {
    let (fib, gal) = (Oid::new(ALDIS_OID_FIBONACCI), Oid::new(ALDIS_OID_GALOIS));
    lens.iter()
        .map(|&len| {
            let out = fib.fill(len);
            assert_eq!(out, gal.fill(len), "the Galois form differs (CMN-022)");
            hex::encode(&out)
        })
        .collect()
}

fn ocf_report_name(report: u32) -> &'static str {
    match report {
        ALDIS_OCF_CLCW => "clcw",
        ALDIS_OCF_PROJECT_SPECIFIC => "project_specific",
        ALDIS_OCF_SDLS_FSR => "sdls_fsr",
        other => panic!("unknown report {other}"),
    }
}

fn core_run(case: &CodecCase) -> R<Value> {
    match case.operation.as_str() {
        "crc16" => {
            let data = input(case);
            let mut crc = 0_u16;
            // SAFETY: every pointer is to a live local or vector.
            call(|e| unsafe { aldis_crc16(data.as_ptr(), data.len(), &raw mut crc, e) })?;
            Ok(json!({ "value_hex": format!("{crc:04X}") }))
        }
        "oid_fill" => {
            let outputs = oid_fills(&[usize_of(&params(case), "octets")]);
            Ok(json!({ "output_hex": outputs[0] }))
        }
        "oid_fill_sequence" => {
            let lens: Vec<usize> = param(case, "fills")
                .as_array()
                .unwrap()
                .iter()
                .map(|n| usize::try_from(n.as_u64().unwrap()).unwrap())
                .collect();
            Ok(json!({ "outputs_hex": oid_fills(&lens) }))
        }
        "ocf_classify" => {
            let octets = input(case);
            let mut report = u32::MAX;
            // SAFETY: every pointer is to a live local or vector.
            call(|e| unsafe {
                aldis_ocf_classify(octets.as_ptr(), octets.len(), &raw mut report, e)
            })?;
            // The OCF crosses the ABI as the caller's octets: classifying never changes them.
            Ok(json!({ "report": ocf_report_name(report), "octets_hex": hex::encode(&octets) }))
        }
        "ocf_report" => {
            let octets = input(case);
            let report = match str_of(&params(case), "report") {
                "clcw" => ALDIS_OCF_CLCW,
                "project_specific" => ALDIS_OCF_PROJECT_SPECIFIC,
                "sdls_fsr" => ALDIS_OCF_SDLS_FSR,
                other => panic!("{}: unknown report {other}", case.id),
            };
            // SAFETY: every pointer is to a live vector.
            call(|e| unsafe { aldis_ocf_check(octets.as_ptr(), octets.len(), report, e) })?;
            Ok(json!({ "octets_hex": hex::encode(&octets) }))
        }
        op => panic!("{}: unsupported operation {op}", case.id),
    }
}

// ---------------------------------------------------------------- TM

/// A TM physical channel and, when the configuration has an SCID, its master channel with its
/// virtual channels, as C structs.
struct TmChannels {
    pc: aldis_tm_config_t,
    mc: Option<(aldis_tm_mc_config_t, Vec<aldis_tm_vc_config_t>)>,
}

impl TmChannels {
    fn mc(&self, case: &CodecCase) -> (&aldis_tm_mc_config_t, &[aldis_tm_vc_config_t]) {
        let (mc, vcs) = self
            .mc
            .as_ref()
            .unwrap_or_else(|| panic!("{}: no master channel", case.id));
        (mc, vcs)
    }
}

/// A Secondary Header length of the vectors, `null` for none: 0 in the C structs.
fn tm_fsh(v: Option<&Value>) -> u8 {
    v.filter(|v| !v.is_null())
        .map_or(0, |n| u8::try_from(n.as_u64().unwrap()).unwrap())
}

fn tm_channels(case: &CodecCase) -> TmChannels {
    let cfg = Value::Object(case.config.clone().unwrap());
    let pc = aldis_tm_config_t {
        struct_size: size_of_u32::<aldis_tm_config_t>(),
        fecf: u8::from(get(&cfg, "fecf").as_bool().unwrap()),
        frame_len: usize_of(&cfg, "frame_len"),
    };
    let Some(scid) = cfg.get("scid") else {
        return TmChannels { pc, mc: None };
    };
    let mc = aldis_tm_mc_config_t {
        struct_size: size_of_u32::<aldis_tm_mc_config_t>(),
        scid: u16::try_from(scid.as_u64().unwrap()).unwrap(),
        fsh_len: tm_fsh(cfg.get("mc_fsh")),
        ocf: u8::from(cfg.get("mc_ocf").and_then(Value::as_bool).unwrap_or(false)),
        pc,
    };
    let vcs = cfg
        .get("vcs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|v| aldis_tm_vc_config_t {
            struct_size: size_of_u32::<aldis_tm_vc_config_t>(),
            vcid: u8_of(v, "vcid"),
            content: match str_of(v, "content") {
                "packets" => ALDIS_TM_CONTENT_PACKETS,
                "vca_sdu" => ALDIS_TM_CONTENT_VCA_SDU,
                other => panic!("{}: content {other}", case.id),
            },
            fsh_len: tm_fsh(v.get("fsh")),
            ocf: u8::from(v.get("ocf").and_then(Value::as_bool).unwrap_or(false)),
            security_header_len: v.get("security_header_len").map_or(0, |_| {
                u8::try_from(u64_of(v, "security_header_len")).unwrap()
            }),
            security_trailer_len: v
                .get("security_trailer_len")
                .map_or(0, |_| u16_of(v, "security_trailer_len")),
        })
        .collect();
    TmChannels {
        pc,
        mc: Some((mc, vcs)),
    }
}

/// The sender's fields of an `encode` case.
fn tm_fields(p: &Value) -> aldis_tm_fields_t {
    let s = get(p, "status");
    let mut f = aldis_tm_fields_t {
        vcid: u8_of(p, "vcid"),
        mc_frame_count: u8_of(p, "mc_frame_count"),
        vc_frame_count: u8_of(p, "vc_frame_count"),
        ..aldis_tm_fields_t::default()
    };
    if get(s, "sync").as_bool().unwrap() {
        f.sync_flag = 1;
        f.packet_order_flag = u8::from(get(s, "packet_order_flag").as_bool().unwrap());
        f.segment_length_id = u8_of(s, "segment_length_id");
        f.first_header_pointer = u16_of(s, "first_header_pointer");
    } else {
        f.pointer = match str_of(s, "pointer") {
            "starts_at" => {
                f.packet_offset = u16_of(s, "packet_offset");
                ALDIS_TM_POINTER_STARTS_AT
            }
            "no_packet_start" => ALDIS_TM_POINTER_NO_PACKET_START,
            "only_idle_data" => ALDIS_TM_POINTER_ONLY_IDLE_DATA,
            other => panic!("pointer {other}"),
        };
    }
    f
}

/// The octets of `len` at `offset` in `bytes`, which the view guarantees lie inside it.
fn tm_slice(bytes: &[u8], offset: usize, len: usize) -> &[u8] {
    &bytes[offset..offset + len]
}

/// Every field the view exposes (TV-002), read from the C view and the caller's input.
fn tm_json(v: &aldis_tm_frame_t, bytes: &[u8]) -> R<Value> {
    let (h, l) = (&v.header, &v.layout);
    let pointer = match v.pointer {
        ALDIS_TM_POINTER_STARTS_AT => Some("starts_at"),
        ALDIS_TM_POINTER_NO_PACKET_START => Some("no_packet_start"),
        ALDIS_TM_POINTER_ONLY_IDLE_DATA => Some("only_idle_data"),
        ALDIS_TM_POINTER_NONE => None,
        other => panic!("pointer {other}"),
    };
    assert_eq!(v.pointer == ALDIS_TM_POINTER_NONE, h.sync_flag);
    let ocf = (l.ocf_len != 0).then(|| tm_slice(bytes, l.ocf_offset, l.ocf_len));
    let report = match ocf {
        Some(o) => {
            let mut report = u32::MAX;
            // SAFETY: `o` and `report` are live.
            call(|e| unsafe { aldis_ocf_classify(o.as_ptr(), o.len(), &raw mut report, e) })?;
            Some(ocf_report_name(report))
        }
        None => None,
    };
    // The header alone decodes to the view's header (API-014).
    let mut header = aldis_tm_header_t::default();
    // SAFETY: `bytes` and `header` are live.
    call(|e| unsafe { aldis_tm_header_decode(bytes.as_ptr(), bytes.len(), &raw mut header, e) })?;
    assert_eq!(header, *h);
    Ok(json!({
        "scid": h.scid,
        "vcid": h.vcid,
        "ocf_flag": h.ocf_flag,
        "mc_frame_count": h.mc_frame_count,
        "vc_frame_count": h.vc_frame_count,
        "fsh_flag": h.fsh_flag,
        "sync_flag": h.sync_flag,
        "packet_order_flag": h.packet_order_flag,
        "segment_length_id": h.segment_length_id,
        "first_header_pointer": h.first_header_pointer,
        "pointer": pointer,
        "packet_offset": (v.pointer == ALDIS_TM_POINTER_STARTS_AT).then_some(v.packet_offset),
        "fsh_len": (v.fsh_len != 0).then_some(v.fsh_len),
        "fsh_data_hex": (v.fsh_len != 0)
            .then(|| hex::encode(tm_slice(bytes, l.fsh_data_offset, l.fsh_data_len))),
        "security_header_hex": hex::encode(tm_slice(
            bytes,
            l.security_header_offset,
            l.security_header_len
        )),
        "data_hex": hex::encode(tm_slice(bytes, l.data_offset, l.data_len)),
        "security_trailer_hex": hex::encode(tm_slice(
            bytes,
            l.security_trailer_offset,
            l.security_trailer_len
        )),
        "ocf_hex": ocf.map(hex::encode),
        "ocf_report": report,
        "fecf": v.has_fecf.then_some(v.fecf),
        "consumed": l.frame_len,
        "anomalies": anomalies::<TmAnomaly>(v.anomalies),
    }))
}

/// The OCF of a parameter object, empty for none.
fn tm_ocf(p: &Value) -> Vec<u8> {
    p.get("ocf_hex")
        .filter(|v| !v.is_null())
        .map(|v| hex::decode(v.as_str().unwrap()).unwrap())
        .unwrap_or_default()
}

/// Copies `src` to `offset` in `frame`, where the writer's layout gives it `len` octets.
fn tm_put(frame: &mut [u8], offset: usize, len: usize, src: &[u8]) {
    assert_eq!(
        src.len(),
        len,
        "a field of another length than the layout's"
    );
    frame[offset..offset + len].copy_from_slice(src);
}

/// Encodes one frame with `aldis_tm_encode` and checks that the two-phase writer agrees
/// (API-020, API-023).
fn tm_encode(case: &CodecCase, ch: &TmChannels) -> R<Value> {
    let (mc, vcs) = ch.mc(case);
    let par = params(case);
    let fields = tm_fields(&par);
    let (fsh, sh, data, st, ocf) = (
        hex_of(&par, "fsh_data_hex"),
        hex_of(&par, "security_header_hex"),
        hex_of(&par, "data_hex"),
        hex_of(&par, "security_trailer_hex"),
        tm_ocf(&par),
    );
    let cap = par
        .get("out_len")
        .map_or(ch.pc.frame_len, |_| usize_of(&par, "out_len"));
    let mut out = vec![0xEE_u8; cap];
    let mut written = 0_usize;
    // SAFETY: every pointer is to a live local or vector, with its length.
    let result = call(|e| unsafe {
        aldis_tm_encode(
            mc,
            vcs.as_ptr(),
            vcs.len(),
            &raw const fields,
            fsh.as_ptr(),
            fsh.len(),
            sh.as_ptr(),
            sh.len(),
            data.as_ptr(),
            data.len(),
            st.as_ptr(),
            st.len(),
            ocf.as_ptr(),
            ocf.len(),
            out.as_mut_ptr(),
            out.len(),
            &raw mut written,
            e,
        )
    });
    if result.is_err() {
        assert!(out.iter().all(|&b| b == 0xEE), "{}: out changed", case.id);
    }
    result?;
    // The two-phase writer: begin, fill each field at its offset, finish (API-023).
    let mut two = vec![0xEE_u8; written];
    let mut layout = aldis_tm_layout_t::default();
    // SAFETY: every pointer is to a live local or vector, with its length.
    call(|e| unsafe {
        aldis_tm_writer_begin(
            mc,
            vcs.as_ptr(),
            vcs.len(),
            &raw const fields,
            two.as_mut_ptr(),
            two.len(),
            &raw mut layout,
            e,
        )
    })?;
    assert_eq!(layout.frame_len, written);
    tm_put(&mut two, layout.fsh_data_offset, layout.fsh_data_len, &fsh);
    tm_put(
        &mut two,
        layout.security_header_offset,
        layout.security_header_len,
        &sh,
    );
    tm_put(&mut two, layout.data_offset, layout.data_len, &data);
    tm_put(
        &mut two,
        layout.security_trailer_offset,
        layout.security_trailer_len,
        &st,
    );
    tm_put(&mut two, layout.ocf_offset, layout.ocf_len, &ocf);
    let mut finished = 0_usize;
    // SAFETY: as above.
    call(|e| unsafe {
        aldis_tm_writer_finish(
            mc,
            vcs.as_ptr(),
            vcs.len(),
            &raw const fields,
            two.as_mut_ptr(),
            two.len(),
            &raw mut finished,
            e,
        )
    })?;
    assert_eq!(finished, written, "{}", case.id);
    assert_eq!(two, out[..written], "{}: two-phase writer differs", case.id);
    Ok(json!({ "output_hex": hex::encode(&out[..written]) }))
}

/// Consecutive OID frames from one generator of `form` (TM-010, CMN-021).
fn tm_oid_frames(case: &CodecCase, ch: &TmChannels, form: u32) -> R<Vec<String>> {
    let (mc, vcs) = ch.mc(case);
    let p = params(case);
    let vcid = u8_of(&p, "vcid");
    let oid = Oid::new(form);
    let mut frames = Vec::new();
    for f in get(&p, "frames").as_array().unwrap() {
        let (mcc, vcc) = (u8_of(f, "mc_frame_count"), u8_of(f, "vc_frame_count"));
        let mut out = vec![0xEE_u8; ch.pc.frame_len];
        let mut l = aldis_tm_layout_t::default();
        // SAFETY: every pointer is to a live local or vector; `oid.h` is a live handle.
        call(|e| unsafe {
            aldis_tm_oid_writer_begin(
                mc,
                vcs.as_ptr(),
                vcs.len(),
                vcid,
                mcc,
                vcc,
                oid.h,
                out.as_mut_ptr(),
                out.len(),
                &raw mut l,
                e,
            )
        })?;
        if f.get("fsh_data_hex").is_some() {
            tm_put(
                &mut out,
                l.fsh_data_offset,
                l.fsh_data_len,
                &hex_of(f, "fsh_data_hex"),
            );
        }
        let ocf = tm_ocf(f);
        if !ocf.is_empty() {
            tm_put(&mut out, l.ocf_offset, l.ocf_len, &ocf);
        }
        let mut n = 0_usize;
        // SAFETY: as above.
        call(|e| unsafe {
            aldis_tm_oid_writer_finish(
                mc,
                vcs.as_ptr(),
                vcs.len(),
                vcid,
                mcc,
                vcc,
                out.as_mut_ptr(),
                out.len(),
                &raw mut n,
                e,
            )
        })?;
        frames.push(hex::encode(&out[..n]));
    }
    Ok(frames)
}

fn tm_run(case: &CodecCase) -> R<Value> {
    let ch = tm_channels(case);
    // The configuration is checked first, as the Rust runner builds it (API-040).
    // SAFETY: every pointer is to a live local or vector.
    call(|e| unsafe { aldis_tm_config_check(&raw const ch.pc, e) })?;
    if let Some((mc, vcs)) = &ch.mc {
        // SAFETY: as above.
        call(|e| unsafe { aldis_tm_mc_config_check(mc, vcs.as_ptr(), vcs.len(), e) })?;
    }
    let bytes = input(case);
    let mut view = aldis_tm_frame_t::default();
    match case.operation.as_str() {
        "config" => Ok(json!({})),
        "peek_length" => {
            let mut n = 0_usize;
            // SAFETY: every pointer is to a live local or vector.
            call(|e| unsafe {
                aldis_tm_peek_length(bytes.as_ptr(), bytes.len(), &raw const ch.pc, &raw mut n, e)
            })?;
            Ok(json!({ "len": n }))
        }
        "decode" => {
            // SAFETY: as above.
            call(|e| unsafe {
                aldis_tm_decode(
                    bytes.as_ptr(),
                    bytes.len(),
                    &raw const ch.pc,
                    mode(case),
                    &raw mut view,
                    e,
                )
            })?;
            tm_json(&view, &bytes)
        }
        "decode_unchecked" => {
            // SAFETY: as above.
            call(|e| unsafe {
                aldis_tm_decode_unchecked(
                    bytes.as_ptr(),
                    bytes.len(),
                    &raw const ch.pc,
                    mode(case),
                    &raw mut view,
                    e,
                )
            })?;
            tm_json(&view, &bytes)
        }
        "decode_with" => {
            let (mc, vcs) = ch.mc(case);
            // SAFETY: as above.
            call(|e| unsafe {
                aldis_tm_decode_with(
                    bytes.as_ptr(),
                    bytes.len(),
                    mc,
                    vcs.as_ptr(),
                    vcs.len(),
                    mode(case),
                    &raw mut view,
                    e,
                )
            })?;
            tm_json(&view, &bytes)
        }
        "validate" => {
            let (mc, vcs) = ch.mc(case);
            // SAFETY: as above.
            call(|e| unsafe {
                aldis_tm_validate(
                    bytes.as_ptr(),
                    bytes.len(),
                    mc,
                    vcs.as_ptr(),
                    vcs.len(),
                    mode(case),
                    e,
                )
            })?;
            Ok(json!({}))
        }
        "encode" => tm_encode(case, &ch),
        "encode_oid" => {
            let frames = tm_oid_frames(case, &ch, ALDIS_OID_FIBONACCI)?;
            // The Galois form writes the same frames (CMN-022).
            let galois = tm_oid_frames(case, &ch, ALDIS_OID_GALOIS)?;
            assert_eq!(frames, galois, "{}: the Galois form differs", case.id);
            Ok(json!({ "frames_hex": frames }))
        }
        op => panic!("{}: unsupported operation {op}", case.id),
    }
}

// ---------------------------------------------------------------- tests

macro_rules! vector_test {
    ($name:ident, $dir:literal, $file:literal, $run:ident) => {
        #[test]
        fn $name() {
            // req: VER-021, TV-001, TV-002, TV-003, CABI-010, CABI-011, CABI-012, CABI-020, CABI-021
            run_codec_file(concat!($dir, "/", $file, ".json"), |c| handle(|| $run(c)))
                .assert_passed();
        }
    };
}

vector_test!(core_crc16, "core", "crc16", core_run);
vector_test!(core_ocf, "core", "ocf", core_run);
vector_test!(core_oid, "core", "oid", core_run);
vector_test!(time_pfield, "time", "pfield", time_run);
vector_test!(time_cuc, "time", "cuc", time_run);
vector_test!(time_cds, "time", "cds", time_run);
vector_test!(time_ccs, "time", "ccs", time_run);
vector_test!(time_ascii, "time", "ascii", time_run);
vector_test!(time_convert, "time", "convert", time_run);
vector_test!(spp_decode_valid, "spp", "decode_valid", spp_run);
vector_test!(spp_decode_invalid, "spp", "decode_invalid", spp_run);
vector_test!(spp_field_codes, "spp", "field_codes", spp_run);
vector_test!(spp_encode_packets, "spp", "encode", spp_run);
vector_test!(spp_idle_packets, "spp", "idle", spp_run);
vector_test!(spp_octet_string, "spp", "octet_string", spp_run);
vector_test!(spp_stream, "spp", "stream", spp_run);
vector_test!(spp_split_opaque, "spp", "split", spp_run);
vector_test!(
    spp_secondary_header_time,
    "spp",
    "secondary_header_time",
    spp_run
);
vector_test!(spp_sequence_tracker, "spp", "sequence_tracker", spp_run);
vector_test!(spp_segment, "spp", "segment", spp_run);
vector_test!(spp_reassemble, "spp", "reassemble", spp_run);
vector_test!(epp_header_1, "epp", "header_1", epp_run);
vector_test!(epp_header_2, "epp", "header_2", epp_run);
vector_test!(epp_header_4, "epp", "header_4", epp_run);
vector_test!(epp_header_8, "epp", "header_8", epp_run);
vector_test!(epp_idle_packets, "epp", "idle", epp_run);
vector_test!(
    epp_auto_size_boundaries,
    "epp",
    "auto_size_boundaries",
    epp_run
);
vector_test!(epp_invalid, "epp", "invalid", epp_run);
vector_test!(epp_config_and_validate, "epp", "config", epp_run);
vector_test!(tm_minimal, "tm", "minimal", tm_run);
vector_test!(tm_fsh_ocf_fecf, "tm", "fsh_ocf_fecf", tm_run);
vector_test!(tm_fhp_values, "tm", "fhp_values", tm_run);
vector_test!(tm_oid_sequence, "tm", "oid_sequence", tm_run);
vector_test!(tm_security_layout, "tm", "security_layout", tm_run);
vector_test!(tm_invalid, "tm", "invalid", tm_run);
