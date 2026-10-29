//! Runs every live case of `test-vectors/{time,spp,epp,tm,tc,aos,uslp}/`, and of the files of `test-vectors/core/`
//! whose functions have a C ABI (`crc16`, `ocf`, `oid`; M2 plan D14), through the `extern "C"`
//! functions of `aldis-ffi`, with the testkit's runner and exact comparison (M2 plan D4; TV-012, VER-021,
//! CABI-012, CABI-020, CABI-021). The handlers build the C structs from the vector's raw values,
//! call the C ABI, and write the outputs from the C structs, so every value the vectors check
//! crosses the ABI, including the field codes of errors (M2 plan D3).
//!
//! Every error must come back consistently: the returned status is the negated kind of the
//! detail (CABI-020, CABI-021).
//!
//! AOS (M2 WP-4 module 8): every live case of `test-vectors/aos/` runs through `aldis_aos_*`,
//! one master channel per SCID, picked by the SCID of the frame's header (c-abi.md §11).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test-only harness: malformed vectors must fail loudly (RS-018)"
)]
#![expect(unsafe_code, reason = "the harness calls the C ABI it tests")]

use core::num::NonZeroU16;

use aldis::aos::AosAnomaly;
use aldis::core::{Anomaly, ErrorKind, FieldId};
use aldis::epp::EppAnomaly;
use aldis::spp::SppAnomaly;
use aldis::time::TimeAnomaly;
use aldis::tm::TmAnomaly;
use aldis_ffi::aos::*;
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
// ---- USLP (M2 WP-5 module 9)
use aldis::uslp::{Reason, UslpAnomaly};
use aldis_ffi::uslp::*;
// ---- end USLP
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

// ---------------------------------------------------------------- AOS (WP-4 module 8)

/// An AOS physical channel and its virtual channels as C structs, one master channel per SCID
/// in the order the SCIDs first appear (c-abi.md §11: a call takes one master channel).
struct AosChannels {
    pc: aldis_aos_config_t,
    mcs: Vec<(aldis_aos_mc_config_t, Vec<aldis_aos_vc_config_t>)>,
}

impl AosChannels {
    /// The master channel of `scid`: a caller with several master channels picks the one of a
    /// frame's SCID. If the configuration has none of that SCID, the first master channel
    /// stands in, which rejects the frame as `UnknownChannel` (`spacecraft_id`), since a master
    /// channel without a virtual channel is not a configuration (AOS-020, OI-55).
    fn mc(&self, scid: u16) -> (aldis_aos_mc_config_t, Vec<aldis_aos_vc_config_t>) {
        self.mcs
            .iter()
            .find(|(m, _)| m.scid == scid)
            .or_else(|| self.mcs.first())
            .cloned()
            .unwrap_or_else(|| (aos_mc(scid, self.pc), Vec::new()))
    }
}

impl AosChannels {
    /// The master channel of `scid` for a call that builds a frame of that SCID: a caller with
    /// no master channel of it has nothing to pass, and reports an unknown Spacecraft ID.
    fn mc_for_frame(&self, scid: u16) -> R<(aldis_aos_mc_config_t, Vec<aldis_aos_vc_config_t>)> {
        self.mcs
            .iter()
            .find(|(m, _)| m.scid == scid)
            .cloned()
            .ok_or_else(|| Fail {
                kind: ErrorKind::UnknownChannel.name().to_owned(),
                field: NonZeroU16::new(aldis_ffi::field::ALDIS_FIELD_SPACECRAFT_ID),
            })
    }
}

fn aos_mc(scid: u16, pc: aldis_aos_config_t) -> aldis_aos_mc_config_t {
    aldis_aos_mc_config_t {
        struct_size: size_of_u32::<aldis_aos_mc_config_t>(),
        scid,
        pc,
    }
}

fn aos_flag(obj: &Value, name: &str) -> u8 {
    u8::from(obj.get(name).and_then(Value::as_bool).unwrap_or(false))
}

fn aos_size(obj: &Value, name: &str) -> u64 {
    obj.get(name).map_or(0, |v| v.as_u64().unwrap())
}

fn aos_channels(case: &CodecCase) -> AosChannels {
    let cfg = Value::Object(case.config.clone().unwrap());
    let pc = aldis_aos_config_t {
        struct_size: size_of_u32::<aldis_aos_config_t>(),
        fhec: aos_flag(&cfg, "fhec"),
        fecf: aos_flag(&cfg, "fecf"),
        edition: match cfg.get("edition").and_then(Value::as_str) {
            None | Some("issue5") => ALDIS_AOS_EDITION_ISSUE5,
            Some("issue4") => ALDIS_AOS_EDITION_ISSUE4,
            Some(other) => panic!("{}: edition {other}", case.id),
        },
        frame_len: usize_of(&cfg, "frame_len"),
        insert_zone_len: usize::try_from(aos_size(&cfg, "insert_zone_len")).unwrap(),
    };
    let mut mcs: Vec<(aldis_aos_mc_config_t, Vec<aldis_aos_vc_config_t>)> = Vec::new();
    for v in cfg
        .get("vcs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let scid = u16_of(v, "scid");
        let vc = aldis_aos_vc_config_t {
            struct_size: size_of_u32::<aldis_aos_vc_config_t>(),
            vcid: u8_of(v, "vcid"),
            content: match str_of(v, "content") {
                "mpdu" => ALDIS_AOS_CONTENT_MPDU,
                "bpdu" => ALDIS_AOS_CONTENT_BPDU,
                "vca_sdu" => ALDIS_AOS_CONTENT_VCA_SDU,
                "idle_data" => ALDIS_AOS_CONTENT_IDLE_DATA,
                other => panic!("{}: content {other}", case.id),
            },
            ocf: aos_flag(v, "ocf"),
            security_header_len: u8::try_from(aos_size(v, "security_header_len")).unwrap(),
            security_trailer_len: u16::try_from(aos_size(v, "security_trailer_len")).unwrap(),
        };
        match mcs.iter_mut().find(|(m, _)| m.scid == scid) {
            Some((_, vcs)) => vcs.push(vc),
            None => mcs.push((aos_mc(scid, pc), vec![vc])),
        }
    }
    AosChannels { pc, mcs }
}

/// The octets of `len` at `offset` in `bytes`, which the view guarantees lie inside it.
fn aos_slice(bytes: &[u8], offset: usize, len: usize) -> &[u8] {
    &bytes[offset..offset + len]
}

/// Every field of the Primary Header (TV-002), from the C struct.
fn aos_header_json(h: &aldis_aos_header_t) -> Map<String, Value> {
    let len = if h.has_fhec { 8 } else { 6 };
    assert!(h.has_fhec || h.octets[6..] == [0, 0]);
    json!({
        "scid": h.scid,
        "vcid": h.vcid,
        "vc_frame_count": h.vc_frame_count,
        "replay_flag": h.replay_flag,
        "cycle_use_flag": h.cycle_use_flag,
        "cycle_field": h.cycle_field,
        "vc_frame_count_cycle": h.cycle_use_flag.then_some(h.cycle_field),
        "issue4_spare": h.issue4_spare,
        "fhec": h.has_fhec.then_some(h.fhec),
        "corrected_symbols": h.corrected_symbols,
        "header_hex": hex::encode(&h.octets[..len]),
        "is_oid": h.vcid == ALDIS_AOS_OID_VCID,
    })
    .as_object()
    .cloned()
    .unwrap()
}

/// The vector names of a decoded data unit: its kind, and the pointer's name and value.
fn aos_pdu_names(p: &aldis_aos_pdu_t) -> (&'static str, Option<&'static str>, Option<u16>) {
    let kind = match p.content {
        ALDIS_AOS_CONTENT_MPDU => "mpdu",
        ALDIS_AOS_CONTENT_BPDU => "bpdu",
        ALDIS_AOS_CONTENT_VCA_SDU => "vca_sdu",
        ALDIS_AOS_CONTENT_IDLE_DATA => "idle_data",
        other => panic!("content {other}"),
    };
    let (pointer, value) = match p.pointer {
        ALDIS_AOS_POINTER_NONE => (None, None),
        ALDIS_AOS_POINTER_STARTS_AT => (Some("starts_at"), Some(p.pointer_value)),
        ALDIS_AOS_POINTER_NO_PACKET_START => (Some("no_packet_start"), None),
        ALDIS_AOS_POINTER_ONLY_IDLE_DATA => (Some("only_idle_data"), None),
        ALDIS_AOS_POINTER_VALID_BITS => (Some("valid_bits"), Some(p.pointer_value)),
        ALDIS_AOS_POINTER_ALL_VALID => (Some("all_valid"), None),
        ALDIS_AOS_POINTER_ONLY_IDLE => (Some("only_idle"), None),
        other => panic!("pointer {other}"),
    };
    (kind, pointer, value)
}

/// Every field the view exposes (TV-002), read from the C view and the caller's input.
fn aos_json(v: &aldis_aos_frame_t, bytes: &[u8], pc: &aldis_aos_config_t, mode: u32) -> R<Value> {
    let (l, p) = (&v.layout, &v.pdu);
    let ocf = (l.ocf_len != 0).then(|| aos_slice(bytes, l.ocf_offset, l.ocf_len));
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
    let mut header = aldis_aos_header_t::default();
    // SAFETY: `bytes`, `pc`, and `header` are live.
    call(|e| unsafe {
        aldis_aos_header_decode(bytes.as_ptr(), bytes.len(), pc, &raw mut header, e)
    })?;
    assert_eq!(header, v.header);
    let data = aos_slice(bytes, l.data_offset, l.data_len);
    // The data unit alone decodes to the view's (AOS-017).
    let mut alone = *p;
    if p.content == ALDIS_AOS_CONTENT_MPDU {
        // SAFETY: `data` and `alone` are live.
        call(|e| unsafe {
            aldis_aos_mpdu_decode(
                data.as_ptr(),
                data.len(),
                pc.edition,
                mode,
                &raw mut alone,
                e,
            )
        })?;
    } else if p.content == ALDIS_AOS_CONTENT_BPDU {
        // SAFETY: as above.
        call(|e| unsafe {
            aldis_aos_bpdu_decode(data.as_ptr(), data.len(), mode, &raw mut alone, e)
        })?;
    }
    assert_eq!(alone, *p);
    assert_eq!(v.anomalies & p.anomalies, p.anomalies);
    let (kind, pointer, value) = aos_pdu_names(p);
    let unit = p.content == ALDIS_AOS_CONTENT_MPDU || p.content == ALDIS_AOS_CONTENT_BPDU;
    let mut out = aos_header_json(&v.header);
    let rest = json!({
        "insert_zone_hex": hex::encode(aos_slice(bytes, l.insert_zone_offset, l.insert_zone_len)),
        "security_header_hex": hex::encode(aos_slice(
            bytes,
            l.security_header_offset,
            l.security_header_len
        )),
        "data_field_hex": hex::encode(data),
        "security_trailer_hex": hex::encode(aos_slice(
            bytes,
            l.security_trailer_offset,
            l.security_trailer_len
        )),
        "ocf_hex": ocf.map(hex::encode),
        "ocf_report": report,
        "fecf": v.has_fecf.then_some(v.fecf),
        "data_unit": kind,
        "pdu_pointer_raw": unit.then_some(p.pointer_raw),
        "pointer": pointer,
        "pointer_value": value,
        "pdu_spare": unit.then_some(p.spare),
        "zone_hex": hex::encode(aos_slice(bytes, l.zone_offset, l.zone_len)),
        "consumed": l.frame_len,
        "anomalies": anomalies::<AosAnomaly>(v.anomalies),
    });
    out.extend(rest.as_object().cloned().unwrap());
    Ok(Value::Object(out))
}

/// The sender's fields of an `encode` case, or of one OID frame (VCID 63, no data unit header).
fn aos_fields(p: &Value) -> aldis_aos_fields_t {
    let cycle = p.get("cycle").filter(|v| !v.is_null());
    let mut f = aldis_aos_fields_t {
        vcid: p
            .get("vcid")
            .map_or(ALDIS_AOS_OID_VCID, |_| u8_of(p, "vcid")),
        vc_frame_count: u32_of(p, "vc_frame_count"),
        replay_flag: aos_flag(p, "replay_flag"),
        cycle_use_flag: u8::from(cycle.is_some()),
        cycle: cycle.map_or(0, |c| u8::try_from(c.as_u64().unwrap()).unwrap()),
        ..aldis_aos_fields_t::default()
    };
    let Some(pdu) = p.get("pdu") else {
        return f;
    };
    let value = || u16::try_from(get(pdu, "value").as_u64().unwrap()).unwrap();
    let kind = (
        str_of(pdu, "kind"),
        pdu.get("pointer").and_then(Value::as_str),
    );
    (f.pointer, f.pointer_value) = match kind {
        ("mpdu", Some("starts_at")) => (ALDIS_AOS_POINTER_STARTS_AT, value()),
        ("mpdu", Some("no_packet_start")) => (ALDIS_AOS_POINTER_NO_PACKET_START, 0),
        ("mpdu", Some("only_idle_data")) => (ALDIS_AOS_POINTER_ONLY_IDLE_DATA, 0),
        ("bpdu", Some("valid_bits")) => (ALDIS_AOS_POINTER_VALID_BITS, value()),
        ("bpdu", Some("all_valid")) => (ALDIS_AOS_POINTER_ALL_VALID, 0),
        ("bpdu", Some("only_idle")) => (ALDIS_AOS_POINTER_ONLY_IDLE, 0),
        ("none", _) => (ALDIS_AOS_POINTER_NONE, 0),
        other => panic!("pdu {other:?}"),
    };
    f
}

/// The octets of an optional hex member, empty for none.
fn aos_octets(p: &Value, name: &str) -> Vec<u8> {
    p.get(name)
        .filter(|v| !v.is_null())
        .map(|v| hex::decode(v.as_str().unwrap()).unwrap())
        .unwrap_or_default()
}

/// Encodes one frame with `aldis_aos_encode` (API-020).
fn aos_encode(case: &CodecCase, ch: &AosChannels) -> R<Value> {
    let par = params(case);
    let (mc, vcs) = ch.mc_for_frame(u16_of(&par, "scid"))?;
    let fields = aos_fields(&par);
    let (iz, sh, zone, st, ocf) = (
        aos_octets(&par, "insert_zone_hex"),
        aos_octets(&par, "security_header_hex"),
        aos_octets(&par, "zone_hex"),
        aos_octets(&par, "security_trailer_hex"),
        aos_octets(&par, "ocf_hex"),
    );
    let cap = par
        .get("out_len")
        .map_or(ch.pc.frame_len, |_| usize_of(&par, "out_len"));
    let mut out = vec![0xEE_u8; cap];
    let mut written = 0_usize;
    let content = aldis_aos_content_t {
        insert_zone: iz.as_ptr(),
        insert_zone_len: iz.len(),
        security_header: sh.as_ptr(),
        security_header_len: sh.len(),
        zone: zone.as_ptr(),
        zone_len: zone.len(),
        security_trailer: st.as_ptr(),
        security_trailer_len: st.len(),
        ocf: ocf.as_ptr(),
        ocf_len: ocf.len(),
    };
    // SAFETY: every pointer is to a live local or vector, with its length.
    let result = call(|e| unsafe {
        aldis_aos_encode(
            &raw const mc,
            vcs.as_ptr(),
            vcs.len(),
            &raw const fields,
            &raw const content,
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
    Ok(json!({ "output_hex": hex::encode(&out[..written]) }))
}

/// Consecutive OID frames of one SCID from one generator of `form` (AOS-016, CMN-021).
fn aos_oid_frames(case: &CodecCase, ch: &AosChannels, form: u32) -> R<Vec<String>> {
    let p = params(case);
    let (mc, vcs) = ch.mc_for_frame(u16_of(&p, "scid"))?;
    let oid = Oid::new(form);
    let mut frames = Vec::new();
    for f in get(&p, "frames").as_array().unwrap() {
        let fields = aos_fields(f);
        let iz = f.get("insert_zone_hex").map_or_else(
            || vec![0; ch.pc.insert_zone_len],
            |_| hex_of(f, "insert_zone_hex"),
        );
        let content = aldis_aos_content_t {
            insert_zone: iz.as_ptr(),
            insert_zone_len: iz.len(),
            ..aldis_aos_content_t::default()
        };
        let mut out = vec![0xEE_u8; ch.pc.frame_len];
        let mut n = 0_usize;
        // SAFETY: every pointer is to a live local or vector; `oid.h` is a live handle.
        call(|e| unsafe {
            aldis_aos_encode_oid(
                &raw const mc,
                vcs.as_ptr(),
                vcs.len(),
                &raw const fields,
                &raw const content,
                oid.h,
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

/// The FHEC cases of `aos/fhec_*.json` (AOS-013, AOS-014).
fn aos_fhec(case: &CodecCase) -> R<Value> {
    let bytes = input(case);
    match case.operation.as_str() {
        "fhec_encode" => {
            let mut fhec = 0_u16;
            // SAFETY: every pointer is to a live local or vector.
            call(|e| unsafe {
                aldis_aos_fhec_encode(bytes.as_ptr(), bytes.len(), &raw mut fhec, e)
            })?;
            let mut header = bytes;
            header.extend(fhec.to_be_bytes());
            Ok(json!({
                "fhec_hex": hex::encode(&fhec.to_be_bytes()),
                "output_hex": hex::encode(&header),
            }))
        }
        "fhec_decode" => {
            let mut c = aldis_aos_corrected_header_t::default();
            // SAFETY: as above.
            call(|e| unsafe { aldis_aos_fhec_decode(bytes.as_ptr(), bytes.len(), &raw mut c, e) })?;
            Ok(json!({
                "header_hex": hex::encode(&c.octets),
                "corrected_symbols": c.corrected_symbols,
            }))
        }
        op => panic!("{}: unsupported operation {op}", case.id),
    }
}

fn aos_run(case: &CodecCase) -> R<Value> {
    if case.operation.starts_with("fhec_") {
        return aos_fhec(case);
    }
    let ch = aos_channels(case);
    // The configuration is checked first, as the Rust runner builds it (API-040): the
    // physical channel, then each master channel.
    // SAFETY: every pointer is to a live local or vector.
    call(|e| unsafe { aldis_aos_config_check(&raw const ch.pc, e) })?;
    for (mc, vcs) in &ch.mcs {
        // SAFETY: as above.
        call(|e| unsafe { aldis_aos_mc_config_check(mc, vcs.as_ptr(), vcs.len(), e) })?;
    }
    if ch.mcs.is_empty() {
        // A configuration without a virtual channel names no master channel; its SCID is the
        // caller's to give, and the check rejects it whatever it is (AOS-020, OI-55).
        let mc = aos_mc(1, ch.pc);
        // SAFETY: as above.
        call(|e| unsafe { aldis_aos_mc_config_check(&raw const mc, core::ptr::null(), 0, e) })?;
    }
    let bytes = input(case);
    match case.operation.as_str() {
        "config" => Ok(json!({})),
        "peek_length" => {
            let mut n = 0_usize;
            // SAFETY: as above.
            call(|e| unsafe {
                aldis_aos_peek_length(bytes.as_ptr(), bytes.len(), &raw const ch.pc, &raw mut n, e)
            })?;
            Ok(json!({ "len": n }))
        }
        "decode_header" => {
            let mut h = aldis_aos_header_t::default();
            // SAFETY: as above.
            call(|e| unsafe {
                aldis_aos_header_decode(
                    bytes.as_ptr(),
                    bytes.len(),
                    &raw const ch.pc,
                    &raw mut h,
                    e,
                )
            })?;
            Ok(Value::Object(aos_header_json(&h)))
        }
        "decode" => {
            // The SCID from the header picks the master channel; a frame whose header does not
            // decode fails before the lookup, on any master channel (c-abi.md §11).
            let mut h = aldis_aos_header_t::default();
            // SAFETY: as above.
            let scid = call(|e| unsafe {
                aldis_aos_header_decode(
                    bytes.as_ptr(),
                    bytes.len(),
                    &raw const ch.pc,
                    &raw mut h,
                    e,
                )
            })
            .map_or(0, |()| h.scid);
            let (mc, vcs) = ch.mc(scid);
            let mut view = aldis_aos_frame_t::default();
            // SAFETY: as above.
            call(|e| unsafe {
                aldis_aos_decode(
                    bytes.as_ptr(),
                    bytes.len(),
                    &raw const mc,
                    vcs.as_ptr(),
                    vcs.len(),
                    mode(case),
                    &raw mut view,
                    e,
                )
            })?;
            aos_json(&view, &bytes, &ch.pc, mode(case))
        }
        "encode" => aos_encode(case, &ch),
        "encode_oid" => {
            let frames = aos_oid_frames(case, &ch, ALDIS_OID_FIBONACCI)?;
            // The Galois form writes the same frames (CMN-022).
            let galois = aos_oid_frames(case, &ch, ALDIS_OID_GALOIS)?;
            assert_eq!(frames, galois, "{}: the Galois form differs", case.id);
            Ok(json!({ "frames_hex": frames }))
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

/// Encodes one frame with `aldis_tm_encode` (API-020).
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
    let content = aldis_tm_content_t {
        fsh_data: fsh.as_ptr(),
        fsh_data_len: fsh.len(),
        security_header: sh.as_ptr(),
        security_header_len: sh.len(),
        data: data.as_ptr(),
        data_len: data.len(),
        security_trailer: st.as_ptr(),
        security_trailer_len: st.len(),
        ocf: ocf.as_ptr(),
        ocf_len: ocf.len(),
    };
    // SAFETY: every pointer is to a live local or vector, with its length.
    let result = call(|e| unsafe {
        aldis_tm_encode(
            mc,
            vcs.as_ptr(),
            vcs.len(),
            &raw const fields,
            &raw const content,
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
        let fsh = f
            .get("fsh_data_hex")
            .map_or_else(Vec::new, |_| hex_of(f, "fsh_data_hex"));
        let ocf = tm_ocf(f);
        let content = aldis_tm_content_t {
            fsh_data: fsh.as_ptr(),
            fsh_data_len: fsh.len(),
            ocf: ocf.as_ptr(),
            ocf_len: ocf.len(),
            ..aldis_tm_content_t::default()
        };
        let mut out = vec![0xEE_u8; ch.pc.frame_len];
        let mut n = 0_usize;
        // SAFETY: every pointer is to a live local or vector; `oid.h` is a live handle.
        call(|e| unsafe {
            aldis_tm_encode_oid(
                mc,
                vcs.as_ptr(),
                vcs.len(),
                vcid,
                mcc,
                vcc,
                &raw const content,
                oid.h,
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

// ---------------------------------------------------------------- USLP (M2 WP-5 module 9)

/// A USLP channel table as C structs: the physical channel, the virtual channels, the MAPs.
struct UslpChannels {
    pc: aldis_uslp_config_t,
    vcs: Vec<aldis_uslp_vc_config_t>,
    maps: Vec<aldis_uslp_map_config_t>,
}

fn uslp_frame_type(v: &Value) -> u8 {
    match v.as_str().unwrap() {
        "fixed" => ALDIS_USLP_FRAME_FIXED,
        "variable" => ALDIS_USLP_FRAME_VARIABLE,
        other => panic!("frame type {other}"),
    }
}

fn uslp_channels(case: &CodecCase) -> UslpChannels {
    let cfg = Value::Object(case.config.clone().unwrap());
    let pc = aldis_uslp_config_t {
        struct_size: size_of_u32::<aldis_uslp_config_t>(),
        frame_type: uslp_frame_type(get(&cfg, "frame_type")),
        fecf: u8::from(get(&cfg, "fecf").as_bool().unwrap()),
        frame_len: usize_of(&cfg, "frame_len"),
        insert_zone_len: usize_of(&cfg, "insert_zone_len"),
    };
    let vcs = get(&cfg, "vcs")
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            let security = get(v, "security");
            let per_map = security.as_str() == Some("per_map");
            aldis_uslp_vc_config_t {
                struct_size: size_of_u32::<aldis_uslp_vc_config_t>(),
                scid: u16_of(v, "scid"),
                vcid: u8_of(v, "vcid"),
                frame_type: uslp_frame_type(get(v, "frame_type")),
                vcf_count_len_sequence: u8_of(v, "vcf_count_len_sequence"),
                vcf_count_len_expedited: u8_of(v, "vcf_count_len_expedited"),
                ocf: match str_of(v, "ocf") {
                    "absent" => ALDIS_USLP_OCF_ABSENT,
                    "allowed" => ALDIS_USLP_OCF_ALLOWED,
                    "required" => ALDIS_USLP_OCF_REQUIRED,
                    other => panic!("{}: ocf {other}", case.id),
                },
                // `null` is no truncated frame length: 0 in the C struct.
                truncated_frame_len: get(v, "truncated_frame_len")
                    .as_u64()
                    .map_or(0, |n| u8::try_from(n).unwrap()),
                security: if per_map {
                    ALDIS_USLP_SECURITY_PER_MAP
                } else {
                    ALDIS_USLP_SECURITY_VC
                },
                security_header_len: if per_map {
                    0
                } else {
                    u8_of(security, "header_len")
                },
                security_trailer_len: if per_map {
                    0
                } else {
                    u32_of(security, "trailer_len")
                },
            }
        })
        .collect();
    let maps = get(&cfg, "maps")
        .as_array()
        .unwrap()
        .iter()
        .map(|m| aldis_uslp_map_config_t {
            struct_size: size_of_u32::<aldis_uslp_map_config_t>(),
            scid: u16_of(m, "scid"),
            vcid: u8_of(m, "vcid"),
            map_id: u8_of(m, "map_id"),
            security_header_len: u8_of(m, "security_header_len"),
            security_trailer_len: u32_of(m, "security_trailer_len"),
        })
        .collect();
    UslpChannels { pc, vcs, maps }
}

fn uslp_channel(p: &Value) -> aldis_uslp_channel_t {
    aldis_uslp_channel_t {
        scid: u16_of(p, "spacecraft_id"),
        source_or_destination: match str_of(p, "source_or_destination") {
            "source" => ALDIS_USLP_SOURCE,
            "destination" => ALDIS_USLP_DESTINATION,
            other => panic!("source_or_destination {other}"),
        },
        vcid: u8_of(p, "vc_id"),
        map_id: u8_of(p, "map_id"),
    }
}

fn uslp_fields(p: &Value) -> aldis_uslp_fields_t {
    aldis_uslp_fields_t {
        channel: uslp_channel(p),
        kind: match str_of(p, "kind") {
            "sequence_controlled" => ALDIS_USLP_KIND_SEQUENCE_CONTROLLED,
            "expedited" => ALDIS_USLP_KIND_EXPEDITED,
            "protocol_control" => ALDIS_USLP_KIND_PROTOCOL_CONTROL,
            other => panic!("kind {other}"),
        },
        vcf_count: u64_of(p, "vcf_count"),
    }
}

/// The TFDF Header of a parameter object: the pointer exactly for rules `000`–`010`.
fn uslp_tfdf(p: &Value) -> aldis_uslp_tfdf_t {
    let rule = u8_of(p, "tfdz_construction_rule");
    let pointer = p.get("pointer").filter(|v| !v.is_null());
    assert_eq!(
        rule <= ALDIS_USLP_RULE_SDU_CONTINUATION,
        pointer.is_some(),
        "pointer exactly for 000-010"
    );
    aldis_uslp_tfdf_t {
        rule,
        upid: u8_of(p, "upid"),
        pointer: pointer.map_or(0, |v| u16::try_from(v.as_u64().unwrap()).unwrap()),
    }
}

/// The OCF of a parameter object, empty for none.
fn uslp_ocf(p: &Value) -> Vec<u8> {
    p.get("ocf_hex")
        .and_then(Value::as_str)
        .map(|h| hex::decode(h).unwrap())
        .unwrap_or_default()
}

/// The octets of `len` at `offset` in `bytes`, which the view guarantees lie inside it.
fn uslp_slice(bytes: &[u8], offset: usize, len: usize) -> &[u8] {
    &bytes[offset..offset + len]
}

/// Every field the view exposes (TV-002), read from the C view and the caller's input, with
/// the keys of the Rust runner.
fn uslp_json(v: &aldis_uslp_frame_t, bytes: &[u8]) -> Value {
    let (c, t, l) = (&v.channel, &v.tfdf, &v.layout);
    let part = |offset, len| hex::encode(uslp_slice(bytes, offset, len));
    let mut out = json!({
        "truncated": v.truncated,
        "spacecraft_id": c.scid,
        "source_or_destination": match c.source_or_destination {
            ALDIS_USLP_SOURCE => "source",
            ALDIS_USLP_DESTINATION => "destination",
            other => panic!("source_or_destination {other}"),
        },
        "vc_id": c.vcid,
        "map_id": c.map_id,
        "frame_len": l.frame_len,
        "security_header_hex": part(l.security_header_offset, l.security_header_len),
        "tfdz_construction_rule": t.rule,
        "upid": t.upid,
        "upid_assigned": v.upid_assigned,
        "tfdz_hex": part(l.tfdz_offset, l.tfdz_len),
        "security_trailer_hex": part(l.security_trailer_offset, l.security_trailer_len),
        "consumed": l.frame_len,
        "anomalies": anomalies::<UslpAnomaly>(v.anomalies),
    });
    // The fields tile the frame in wire order (CABI-012).
    assert_eq!(l.tfdf_header_offset + l.tfdf_header_len, l.tfdz_offset);
    assert_eq!(l.fecf_offset + l.fecf_len, l.frame_len);
    if v.truncated {
        assert_eq!(
            (v.kind, v.ocf_flag, v.vcf_count, v.has_fecf),
            (ALDIS_USLP_KIND_EXPEDITED, false, 0, false)
        );
        return out;
    }
    let has_pointer = t.rule <= ALDIS_USLP_RULE_SDU_CONTINUATION;
    let pointer_kind = match (t.rule, t.pointer == ALDIS_USLP_POINTER_NONE) {
        (ALDIS_USLP_RULE_PACKETS_SPANNING_FRAMES, false) => Some("packet_starts_at"),
        (ALDIS_USLP_RULE_PACKETS_SPANNING_FRAMES, true) => Some("no_packet_start_or_end"),
        (ALDIS_USLP_RULE_SDU_START | ALDIS_USLP_RULE_SDU_CONTINUATION, false) => {
            Some("last_valid_octet")
        }
        (ALDIS_USLP_RULE_SDU_START | ALDIS_USLP_RULE_SDU_CONTINUATION, true) => {
            Some("sdu_continues")
        }
        _ => None,
    };
    let extra = json!({
        "kind": match v.kind {
            ALDIS_USLP_KIND_SEQUENCE_CONTROLLED => "sequence_controlled",
            ALDIS_USLP_KIND_EXPEDITED => "expedited",
            ALDIS_USLP_KIND_PROTOCOL_CONTROL => "protocol_control",
            other => panic!("kind {other}"),
        },
        "ocf_flag": v.ocf_flag,
        "vcf_count_len": v.vcf_count_len,
        "vcf_count": v.vcf_count,
        "insert_zone_hex": part(l.insert_zone_offset, l.insert_zone_len),
        "pointer": has_pointer.then_some(t.pointer),
        "pointer_kind": pointer_kind,
        "ocf_hex": (l.ocf_len != 0).then(|| part(l.ocf_offset, l.ocf_len)),
        "fecf_hex": v.has_fecf.then(|| format!("{:04X}", v.fecf)),
    });
    assert_eq!(v.ocf_flag, l.ocf_len != 0);
    if v.has_fecf {
        assert_eq!(
            uslp_slice(bytes, l.fecf_offset, l.fecf_len),
            v.fecf.to_be_bytes()
        );
    }
    if let (Value::Object(m), Value::Object(e)) = (&mut out, extra) {
        m.extend(e);
    }
    out
}

/// Encodes one frame with `aldis_uslp_encode` (API-020).
fn uslp_encode(case: &CodecCase, ch: &UslpChannels) -> R<Value> {
    let p = params(case);
    let fields = uslp_fields(&p);
    let (iz, tfdz, ocf) = (
        hex_of(&p, "insert_zone_hex"),
        hex_of(&p, "tfdz_hex"),
        uslp_ocf(&p),
    );
    let content = aldis_uslp_content_t {
        tfdf: uslp_tfdf(&p),
        insert_zone: iz.as_ptr(),
        insert_zone_len: iz.len(),
        tfdz: tfdz.as_ptr(),
        tfdz_len: tfdz.len(),
        ocf: ocf.as_ptr(),
        ocf_len: ocf.len(),
    };
    let mut out = vec![0xEE_u8; 70_000];
    let mut written = 0_usize;
    // SAFETY: every pointer is to a live local or vector, with its length.
    let result = call(|e| unsafe {
        aldis_uslp_encode(
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            ch.vcs.len(),
            ch.maps.as_ptr(),
            ch.maps.len(),
            &raw const fields,
            &raw const content,
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
    Ok(json!({ "output_hex": hex::encode(&out[..written]) }))
}

fn uslp_encode_truncated(case: &CodecCase, ch: &UslpChannels) -> R<Value> {
    let p = params(case);
    let channel = uslp_channel(&p);
    let tfdz = hex_of(&p, "tfdz_hex");
    let content = aldis_uslp_content_t {
        tfdf: uslp_tfdf(&p),
        tfdz: tfdz.as_ptr(),
        tfdz_len: tfdz.len(),
        ..aldis_uslp_content_t::default()
    };
    let mut out = [0_u8; 64];
    let mut n = 0_usize;
    // SAFETY: every pointer is to a live local or vector, with its length.
    call(|e| unsafe {
        aldis_uslp_encode_truncated(
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            ch.vcs.len(),
            ch.maps.as_ptr(),
            ch.maps.len(),
            &raw const channel,
            &raw const content,
            out.as_mut_ptr(),
            out.len(),
            &raw mut n,
            e,
        )
    })?;
    Ok(json!({ "output_hex": hex::encode(&out[..n]) }))
}

/// One OID frame from a generator of `form`, advanced by the case's `oid_skip_octets`
/// (USLP-009, CMN-021).
fn uslp_oid_frame(case: &CodecCase, ch: &UslpChannels, form: u32) -> R<String> {
    let p = params(case);
    let oid = Oid::new(form);
    let _ = oid.fill(usize_of(&p, "oid_skip_octets"));
    let fields = uslp_fields(&p);
    let (iz, ocf) = (hex_of(&p, "insert_zone_hex"), uslp_ocf(&p));
    let content = aldis_uslp_content_t {
        insert_zone: iz.as_ptr(),
        insert_zone_len: iz.len(),
        ocf: ocf.as_ptr(),
        ocf_len: ocf.len(),
        ..aldis_uslp_content_t::default()
    };
    let mut out = vec![0_u8; 70_000];
    let mut n = 0_usize;
    // SAFETY: every pointer is to a live local or vector; `oid.h` is a live handle.
    call(|e| unsafe {
        aldis_uslp_encode_oid(
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            ch.vcs.len(),
            ch.maps.as_ptr(),
            ch.maps.len(),
            &raw const fields,
            &raw const content,
            oid.h,
            out.as_mut_ptr(),
            out.len(),
            &raw mut n,
            e,
        )
    })?;
    Ok(hex::encode(&out[..n]))
}

fn uslp_validate(case: &CodecCase, ch: &UslpChannels) -> Value {
    let bytes = input(case);
    let mut item = aldis_uslp_stream_item_t::default();
    let mut offset = 0_usize;
    let mut det = aldis_error_detail_t {
        struct_size: size_of_u32::<aldis_error_detail_t>(),
        ..aldis_error_detail_t::default()
    };
    // SAFETY: every pointer is to a live local or vector, with its length.
    let status = unsafe {
        aldis_uslp_stream_next(
            bytes.as_ptr(),
            bytes.len(),
            &raw mut offset,
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            ch.vcs.len(),
            ch.maps.as_ptr(),
            ch.maps.len(),
            mode(case),
            &raw mut item,
            &raw mut det,
        )
    };
    if status != ALDIS_OK {
        let kind = ErrorKind::from_code(u16::try_from(det.kind).unwrap()).unwrap();
        let field = NonZeroU16::new(det.field).map(|code| {
            FieldId::from_code(code.get())
                .unwrap_or_else(|| panic!("field code {code} is not catalogued"))
                .code()
        });
        return json!({
            "accepted": false,
            "error_kind": kind.name(),
            "error_field": field.map(NonZeroU16::get),
        });
    }
    match item.kind {
        ALDIS_USLP_STREAM_ACCEPTED => {
            if item.len == bytes.len() {
                json!({ "accepted": true, "frame_len": item.frame.layout.frame_len })
            } else {
                let r = Reason::LengthMismatch;
                json!({
                    "accepted": false,
                    "reason": format!("{r:?}"),
                    "reason_code": u32::from(r.code()),
                    "error_kind": "LengthMismatch",
                    "error_field": 68,
                })
            }
        }
        ALDIS_USLP_STREAM_DISCARDED => {
            let (r, kind, field) = match item.discard_reason {
                ALDIS_USLP_DISCARD_INCOMPLETE_FRAME | ALDIS_USLP_DISCARD_FILL => {
                    (Reason::LengthMismatch, "LengthMismatch", Some(68_u16))
                }
                ALDIS_USLP_DISCARD_NO_TRUNCATED_LENGTH => (
                    Reason::TruncatedFrameNotAllowed,
                    "InvalidField",
                    Some(91_u16),
                ),
                reason_code => {
                    let r = Reason::from_code(u8::try_from(reason_code).unwrap())
                        .unwrap_or_else(|| panic!("{}: discard reason {}", case.id, reason_code));
                    let kind =
                        ErrorKind::from_code(u16::try_from(-item.error_code).unwrap()).unwrap();
                    let field = NonZeroU16::new(item.error_field)
                        .map(|code| {
                            FieldId::from_code(code.get())
                                .unwrap_or_else(|| panic!("field code {code} is not catalogued"))
                                .code()
                        })
                        .map(NonZeroU16::get);
                    (r, kind.name(), field)
                }
            };
            json!({
                "accepted": false,
                "reason": format!("{r:?}"),
                "reason_code": u32::from(r.code()),
                "error_kind": kind,
                "error_field": field,
            })
        }
        other => panic!("{}: unexpected item kind {other}", case.id),
    }
}

fn uslp_delimit(case: &CodecCase, ch: &UslpChannels) -> R<Value> {
    let unit = input(case);
    let mut items = Vec::new();
    let mut at = 0_usize;
    loop {
        let mut item = aldis_uslp_stream_item_t::default();
        let mut offset = at;
        // SAFETY: every pointer is to a live local or vector, with its length.
        call(|e| unsafe {
            aldis_uslp_stream_next(
                unit.as_ptr(),
                unit.len(),
                &raw mut offset,
                &raw const ch.pc,
                ch.vcs.as_ptr(),
                ch.vcs.len(),
                ch.maps.as_ptr(),
                ch.maps.len(),
                mode(case),
                &raw mut item,
                e,
            )
        })?;
        assert_eq!(item.offset, at);
        let kind = match item.kind {
            ALDIS_USLP_STREAM_END => break,
            ALDIS_USLP_STREAM_ACCEPTED => "frame",
            ALDIS_USLP_STREAM_DISCARDED => match item.discard_reason {
                ALDIS_USLP_DISCARD_FILL => "fill",
                ALDIS_USLP_DISCARD_INCOMPLETE_FRAME => "incomplete",
                ALDIS_USLP_DISCARD_NO_TRUNCATED_LENGTH => "no_truncated_length",
                ALDIS_USLP_DISCARD_NON_TRUNCATED_FRAME_NOT_ALLOWED => "non_truncated_not_allowed",
                _ => "frame",
            },
            other => panic!("delimited kind {other}"),
        };
        assert!(item.len > 0);
        if item.kind == ALDIS_USLP_STREAM_ACCEPTED {
            assert_eq!(
                item.declared_len, item.len,
                "a frame declares its own length"
            );
        }
        items.push(json!({
            "kind": kind,
            "offset": at,
            "octets_hex": hex::encode(&unit[at..at + item.len]),
            "declared_len": (kind == "incomplete")
                .then_some(item.declared_len),
        }));
        at = offset;
    }
    assert_eq!(at, unit.len(), "{}: the items cover the unit", case.id);
    Ok(json!({ "items": items }))
}

fn uslp_run(case: &CodecCase) -> R<Value> {
    let ch = uslp_channels(case);
    // The configuration is checked first, as the Rust runner builds it (API-040).
    // SAFETY: every pointer is to a live local or vector.
    call(|e| unsafe { aldis_uslp_config_check(&raw const ch.pc, e) })?;
    // SAFETY: as above.
    call(|e| unsafe {
        aldis_uslp_vc_table_check(
            &raw const ch.pc,
            ch.vcs.as_ptr(),
            ch.vcs.len(),
            ch.maps.as_ptr(),
            ch.maps.len(),
            e,
        )
    })?;
    let bytes = input(case);
    match case.operation.as_str() {
        "config" => Ok(json!({ "vcs": ch.vcs.len(), "maps": ch.maps.len() })),
        "peek_length" => {
            let mut n = 0_usize;
            // SAFETY: every pointer is to a live local or vector.
            call(|e| unsafe {
                aldis_uslp_peek_length(
                    bytes.as_ptr(),
                    bytes.len(),
                    &raw const ch.pc,
                    ch.vcs.as_ptr(),
                    ch.vcs.len(),
                    &raw mut n,
                    e,
                )
            })?;
            Ok(json!({ "frame_len": n }))
        }
        "decode" => {
            let mut view = aldis_uslp_frame_t::default();
            // SAFETY: as above.
            call(|e| unsafe {
                aldis_uslp_decode(
                    bytes.as_ptr(),
                    bytes.len(),
                    &raw const ch.pc,
                    ch.vcs.as_ptr(),
                    ch.vcs.len(),
                    ch.maps.as_ptr(),
                    ch.maps.len(),
                    mode(case),
                    &raw mut view,
                    e,
                )
            })?;
            Ok(uslp_json(&view, &bytes))
        }
        "encode" => uslp_encode(case, &ch),
        "encode_truncated" => uslp_encode_truncated(case, &ch),
        "encode_oid" => {
            let frame = uslp_oid_frame(case, &ch, ALDIS_OID_FIBONACCI)?;
            // The Galois form writes the same frame (CMN-022).
            let galois = uslp_oid_frame(case, &ch, ALDIS_OID_GALOIS)?;
            assert_eq!(frame, galois, "{}: the Galois form differs", case.id);
            Ok(json!({ "output_hex": frame }))
        }
        "validate" => Ok(uslp_validate(case, &ch)),
        "delimit" => uslp_delimit(case, &ch),
        op => panic!("{}: unsupported operation {op}", case.id),
    }
}

// ---------------------------------------------------------------- end USLP

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
vector_test!(aos_fhec_encode, "aos", "fhec_encode", aos_run);
vector_test!(
    aos_fhec_correct_1_symbol,
    "aos",
    "fhec_correct_1_symbol",
    aos_run
);
vector_test!(
    aos_fhec_correct_2_symbols,
    "aos",
    "fhec_correct_2_symbols",
    aos_run
);
vector_test!(aos_fhec_miscorrection, "aos", "fhec_miscorrection", aos_run);
vector_test!(aos_fhec_uncorrectable, "aos", "fhec_uncorrectable", aos_run);
vector_test!(aos_minimal, "aos", "minimal", aos_run);
vector_test!(aos_fhec_frame, "aos", "fhec_frame", aos_run);
vector_test!(aos_insert_zone, "aos", "insert_zone", aos_run);
vector_test!(aos_security_layout, "aos", "security_layout", aos_run);
vector_test!(aos_mpdu_pointers, "aos", "mpdu_pointers", aos_run);
vector_test!(aos_bpdu_pointers, "aos", "bpdu_pointers", aos_run);
vector_test!(aos_oid, "aos", "oid", aos_run);
vector_test!(aos_issue4_compat, "aos", "issue4_compat", aos_run);
vector_test!(aos_scid_10bit, "aos", "scid_10bit", aos_run);
vector_test!(aos_config, "aos", "config", aos_run);
vector_test!(aos_invalid, "aos", "invalid", aos_run);
vector_test!(tm_minimal, "tm", "minimal", tm_run);
vector_test!(tm_fsh_ocf_fecf, "tm", "fsh_ocf_fecf", tm_run);
vector_test!(tm_fhp_values, "tm", "fhp_values", tm_run);
vector_test!(tm_oid_sequence, "tm", "oid_sequence", tm_run);
vector_test!(tm_security_layout, "tm", "security_layout", tm_run);
vector_test!(tm_invalid, "tm", "invalid", tm_run);

// ---------------------------------------------------------------- TC (M2 WP-3 module 8)

/// Every file of `test-vectors/tc/` through the C ABI of `aldis-tc` and the CLCW (M2 plan
/// WP-3 module 8, D14).
mod tc {
    use core::num::NonZeroU16;

    use aldis::tc::{Reason, TcAnomaly};
    use aldis_ffi::tc::*;
    use aldis_testkit::{CodecCase, hex, run_codec_file};
    use serde_json::{Value, json};

    use super::{
        ALDIS_MODE_LENIENT, ALDIS_OK, ErrorKind, Fail, FieldId, R, aldis_error_detail_t, anomalies,
        call, get, handle, input, mode, params, size_of_u32, str_of, u8_of, u16_of, usize_of,
    };

    /// A master channel: its struct, its virtual channels, and its MAPs.
    type Mc = (
        aldis_tc_mc_config_t,
        Vec<aldis_tc_vc_config_t>,
        Vec<aldis_tc_map_config_t>,
    );

    /// A TC physical channel and its master channels, one per Spacecraft ID in order of first
    /// appearance, as C structs (c-abi.md §11: one master channel per call).
    struct Channels {
        pc: aldis_tc_config_t,
        mcs: Vec<Mc>,
    }

    impl Channels {
        /// The one master channel a frame operation runs on.
        fn mc(&self, case: &CodecCase) -> &Mc {
            match self.mcs.as_slice() {
                [mc] => mc,
                _ => panic!("{}: not one master channel", case.id),
            }
        }
    }

    fn opt_u8(v: &Value, name: &str) -> u8 {
        v.get(name).map_or(0, |_| u8_of(v, name))
    }

    fn opt_u16(v: &Value, name: &str) -> u16 {
        v.get(name).map_or(0, |_| u16_of(v, name))
    }

    fn flag(v: &Value, name: &str) -> u8 {
        u8::from(get(v, name).as_bool().unwrap())
    }

    fn channels(case: &CodecCase) -> Option<Channels> {
        let cfg = Value::Object(case.config.clone()?);
        let pc = aldis_tc_config_t {
            struct_size: size_of_u32::<aldis_tc_config_t>(),
            fecf: flag(&cfg, "fecf"),
            max_frame_len: usize_of(&cfg, "max_frame_len"),
        };
        let vcs = get(&cfg, "vcs").as_array().unwrap();
        let maps = get(&cfg, "maps").as_array().unwrap();
        let mut scids: Vec<u16> = Vec::new();
        for v in vcs.iter().chain(maps) {
            let scid = u16_of(v, "scid");
            if !scids.contains(&scid) {
                scids.push(scid);
            }
        }
        let mcs = scids
            .into_iter()
            .map(|scid| {
                let mc = aldis_tc_mc_config_t {
                    struct_size: size_of_u32::<aldis_tc_mc_config_t>(),
                    scid,
                    pc,
                };
                let on = |v: &&Value| u16_of(v, "scid") == scid;
                let vcs = vcs
                    .iter()
                    .filter(on)
                    .map(|v| aldis_tc_vc_config_t {
                        struct_size: size_of_u32::<aldis_tc_vc_config_t>(),
                        vcid: u8_of(v, "vcid"),
                        segment_header: flag(v, "segment_header"),
                        security_header_len: opt_u8(v, "security_header_len"),
                        security_trailer_len: opt_u16(v, "security_trailer_len"),
                        max_frame_len: usize_of(v, "max_frame_len"),
                    })
                    .collect();
                let maps = maps
                    .iter()
                    .filter(on)
                    .map(|m| aldis_tc_map_config_t {
                        struct_size: size_of_u32::<aldis_tc_map_config_t>(),
                        vcid: u8_of(m, "vcid"),
                        map_id: u8_of(m, "map_id"),
                        security_header_len: opt_u8(m, "security_header_len"),
                        security_trailer_len: opt_u16(m, "security_trailer_len"),
                    })
                    .collect();
                (mc, vcs, maps)
            })
            .collect();
        Some(Channels { pc, mcs })
    }

    fn frame_type_name(t: u8) -> &'static str {
        match t {
            ALDIS_TC_FRAME_AD => "AD",
            ALDIS_TC_FRAME_BD => "BD",
            ALDIS_TC_FRAME_BC => "BC",
            other => panic!("frame type {other}"),
        }
    }

    /// The octets of `len` at `offset` in `bytes`, which the view guarantees lie inside it.
    fn slice(bytes: &[u8], offset: usize, len: usize) -> &[u8] {
        &bytes[offset..offset + len]
    }

    /// Every field the view exposes (TV-002), read from the C view and the caller's input.
    fn json_of(v: &aldis_tc_frame_t, bytes: &[u8]) -> R<Value> {
        let (h, l) = (&v.header, &v.layout);
        // The header alone decodes to the view's header (API-014).
        let mut header = aldis_tc_header_t::default();
        // SAFETY: `bytes` and `header` are live.
        call(|e| unsafe {
            aldis_tc_header_decode(bytes.as_ptr(), bytes.len(), &raw mut header, e)
        })?;
        assert_eq!(header, *h);
        assert_eq!(l.frame_len, h.frame_len);
        let data = slice(bytes, l.data_offset, l.data_len);
        let (command, vr) = match v.control.command {
            ALDIS_TC_CONTROL_NONE => (Value::Null, Value::Null),
            ALDIS_TC_CONTROL_UNLOCK => (json!("unlock"), Value::Null),
            ALDIS_TC_CONTROL_SET_VR => (json!("set_vr"), json!(v.control.set_vr_value)),
            other => panic!("control command {other}"),
        };
        if v.control.command != ALDIS_TC_CONTROL_NONE {
            // The data field decodes to the same command on its own (TC-005).
            let mut alone = aldis_tc_control_t::default();
            // SAFETY: `data` and `alone` are live.
            call(|e| unsafe {
                aldis_tc_control_decode(data.as_ptr(), data.len(), &raw mut alone, e)
            })?;
            assert_eq!(alone, v.control);
        }
        Ok(json!({
            "frame_type": frame_type_name(h.frame_type),
            "bypass_flag": u8::from(h.frame_type != ALDIS_TC_FRAME_AD),
            "control_command_flag": u8::from(h.frame_type == ALDIS_TC_FRAME_BC),
            "spacecraft_id": h.scid,
            "vc_id": h.vcid,
            "frame_len": h.frame_len,
            "sequence_number": h.sequence_number,
            "segment_header": v.has_segment_header.then(|| json!({
                "sequence_flags": v.sequence_flags,
                "map_id": v.map_id,
            })),
            "security_header_hex": hex::encode(slice(
                bytes,
                l.security_header_offset,
                l.security_header_len
            )),
            "data_hex": hex::encode(data),
            "security_trailer_hex": hex::encode(slice(
                bytes,
                l.security_trailer_offset,
                l.security_trailer_len
            )),
            "fecf_hex": v.has_fecf.then(|| format!("{:04X}", v.fecf)),
            "control_command": command,
            "set_vr_value": vr,
            "consumed": l.frame_len,
            "anomalies": anomalies::<TcAnomaly>(v.anomalies),
        }))
    }

    /// The sender's fields of an `encode` case.
    fn fields_of(p: &Value) -> aldis_tc_fields_t {
        let sh = p.get("segment_header").unwrap_or(&Value::Null);
        aldis_tc_fields_t {
            frame_type: match str_of(p, "frame_type") {
                "AD" => ALDIS_TC_FRAME_AD,
                "BD" => ALDIS_TC_FRAME_BD,
                "BC" => ALDIS_TC_FRAME_BC,
                other => panic!("frame type {other}"),
            },
            scid: u16_of(p, "spacecraft_id"),
            vcid: u8_of(p, "vc_id"),
            sequence_number: u8_of(p, "sequence_number"),
            segment_header: u8::from(!sh.is_null()),
            sequence_flags: if sh.is_null() {
                0
            } else {
                u8_of(sh, "sequence_flags")
            },
            map_id: if sh.is_null() { 0 } else { u8_of(sh, "map_id") },
        }
    }

    /// Encodes one frame with `aldis_tc_encode`; checks that the two-phase writer agrees on a
    /// Type-D frame (API-020, API-023) and `aldis_tc_control_encode` on a Type-C one (TC-005).
    fn encode(case: &CodecCase, ch: &Channels) -> R<Value> {
        let (mc, vcs, maps) = ch.mc(case);
        let p = params(case);
        let fields = fields_of(&p);
        let control = match p.get("control_command").and_then(Value::as_str) {
            None => aldis_tc_control_t::default(),
            Some("unlock") => aldis_tc_control_t {
                command: ALDIS_TC_CONTROL_UNLOCK,
                set_vr_value: 0,
            },
            Some("set_vr") => aldis_tc_control_t {
                command: ALDIS_TC_CONTROL_SET_VR,
                set_vr_value: u8_of(&p, "set_vr_value"),
            },
            Some(other) => panic!("control command {other}"),
        };
        let user = if control.command == ALDIS_TC_CONTROL_NONE {
            hex::decode(str_of(&p, "user_data_hex")).unwrap()
        } else {
            Vec::new()
        };
        let content = aldis_tc_content_t {
            user_data: user.as_ptr(),
            user_data_len: user.len(),
            control,
        };
        let mut out = vec![0xEE_u8; 1100];
        let mut written = 0_usize;
        // SAFETY: every pointer is to a live local or vector, with its length.
        let result = call(|e| unsafe {
            aldis_tc_encode(
                mc,
                vcs.as_ptr(),
                vcs.len(),
                maps.as_ptr(),
                maps.len(),
                &raw const fields,
                &raw const content,
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
        let frame = &out[..written];
        if control.command != ALDIS_TC_CONTROL_NONE {
            let mut octets = [0_u8; 3];
            let mut n = 0_usize;
            // SAFETY: every pointer is to a live local.
            call(|e| unsafe {
                aldis_tc_control_encode(
                    &raw const control,
                    octets.as_mut_ptr(),
                    octets.len(),
                    &raw mut n,
                    e,
                )
            })?;
            assert_eq!(&frame[5..5 + n], &octets[..n], "{}", case.id);
        }
        Ok(json!({ "output_hex": hex::encode(frame) }))
    }

    /// The Frame Validation Checks: an accepted frame, or a rejection with its reason and the
    /// error of the failed check (TC-021, TC-022).
    fn validate(case: &CodecCase, ch: &Channels, bytes: &[u8]) -> R<Value> {
        let (mc, vcs, maps) = ch.mc(case);
        let mut item = aldis_tc_stream_item_t::default();
        let mut offset = 0_usize;
        let mut det = aldis_error_detail_t {
            struct_size: size_of_u32::<aldis_error_detail_t>(),
            ..aldis_error_detail_t::default()
        };
        // SAFETY: every pointer is to a live local or vector, with its length.
        let status = unsafe {
            aldis_tc_stream_next(
                bytes.as_ptr(),
                bytes.len(),
                &raw mut offset,
                mc,
                vcs.as_ptr(),
                vcs.len(),
                maps.as_ptr(),
                maps.len(),
                mode(case),
                &raw mut item,
                &raw mut det,
            )
        };
        if status != ALDIS_OK {
            let kind = ErrorKind::from_code(u16::try_from(det.kind).unwrap()).unwrap();
            let field = NonZeroU16::new(det.field).map(|code| {
                FieldId::from_code(code.get())
                    .unwrap_or_else(|| panic!("field code {code} is not catalogued"))
                    .code()
            });
            return Err(Fail {
                kind: kind.name().to_owned(),
                field,
            });
        }
        match item.kind {
            ALDIS_TC_STREAM_ACCEPTED => {
                if item.len == bytes.len() {
                    Ok(json!({ "accepted": true, "frame_len": item.frame.layout.frame_len }))
                } else {
                    let r = Reason::LengthMismatch;
                    Ok(json!({
                        "accepted": false,
                        "reason": format!("{r:?}"),
                        "reason_code": u32::from(r.code()),
                        "error_kind": "LengthMismatch",
                        "error_field": 68,
                    }))
                }
            }
            ALDIS_TC_STREAM_DISCARDED => {
                let (r, kind, field) = match item.discard_reason {
                    ALDIS_TC_DISCARD_TRUNCATED_FRAME | ALDIS_TC_DISCARD_FILL => {
                        (Reason::LengthMismatch, "LengthMismatch", Some(68_u16))
                    }
                    reason_code => {
                        let r = Reason::from_code(u8::try_from(reason_code).unwrap())
                            .unwrap_or_else(|| {
                                panic!("{}: discard reason {}", case.id, reason_code)
                            });
                        let kind =
                            ErrorKind::from_code(u16::try_from(-item.error_code).unwrap()).unwrap();
                        let field = NonZeroU16::new(item.error_field)
                            .map(|code| {
                                FieldId::from_code(code.get())
                                    .unwrap_or_else(|| {
                                        panic!("field code {code} is not catalogued")
                                    })
                                    .code()
                            })
                            .map(NonZeroU16::get);
                        (r, kind.name(), field)
                    }
                };
                Ok(json!({
                    "accepted": false,
                    "reason": format!("{r:?}"),
                    "reason_code": u32::from(r.code()),
                    "error_kind": kind,
                    "error_field": field,
                }))
            }
            other => panic!("{}: unexpected item kind {other}", case.id),
        }
    }

    /// Every item of a data unit, one call each via the intake stream (TC-020).
    fn delimit(unit: &[u8]) -> R<Value> {
        let pc = aldis_tc_config_t {
            struct_size: size_of_u32::<aldis_tc_config_t>(),
            fecf: 0,
            max_frame_len: 1024,
        };
        let mc = aldis_tc_mc_config_t {
            struct_size: size_of_u32::<aldis_tc_mc_config_t>(),
            scid: 0x155,
            pc,
        };
        let vc = aldis_tc_vc_config_t {
            struct_size: size_of_u32::<aldis_tc_vc_config_t>(),
            vcid: 1,
            segment_header: 0,
            security_header_len: 0,
            security_trailer_len: 0,
            max_frame_len: 1024,
        };
        let mut items = Vec::new();
        let mut offset = 0;
        loop {
            let mut item = aldis_tc_stream_item_t::default();
            let mut next_offset = offset;
            // SAFETY: unit and item are live; mc and vc pointers are valid.
            call(|e| unsafe {
                aldis_tc_stream_next(
                    unit.as_ptr(),
                    unit.len(),
                    &raw mut next_offset,
                    &raw const mc,
                    &raw const vc,
                    1,
                    core::ptr::null(),
                    0,
                    ALDIS_MODE_LENIENT,
                    &raw mut item,
                    e,
                )
            })?;
            assert_eq!(item.offset, offset);
            let (kind, declared) = match item.kind {
                ALDIS_TC_STREAM_END => break,
                ALDIS_TC_STREAM_ACCEPTED => ("frame", Value::Null),
                ALDIS_TC_STREAM_DISCARDED => match item.discard_reason {
                    ALDIS_TC_DISCARD_TRUNCATED_FRAME => ("truncated", json!(item.declared_len)),
                    ALDIS_TC_DISCARD_FILL => ("fill", Value::Null),
                    _ => ("frame", Value::Null),
                },
                other => panic!("item kind {other}"),
            };
            assert!(item.len > 0);
            items.push(json!({
                "kind": kind,
                "offset": offset,
                "octets_hex": hex::encode(slice(unit, offset, item.len)),
                "declared_len": declared,
            }));
            offset = next_offset;
        }
        assert_eq!(offset, unit.len());
        Ok(json!({ "items": items }))
    }

    fn clcw_json(c: &aldis_clcw_t, mask: u32) -> Value {
        let set = |f: u8| {
            assert!(f <= 1);
            f == 1
        };
        json!({
            "status": c.status,
            "cop_in_effect": c.cop_in_effect,
            "vc_id": c.vcid,
            "no_rf_available": set(c.no_rf_available),
            "no_bit_lock": set(c.no_bit_lock),
            "lockout": set(c.lockout),
            "wait": set(c.wait),
            "retransmit": set(c.retransmit),
            "farm_b_counter": c.farm_b_counter,
            "report_value": c.report_value,
            "anomalies": anomalies::<TcAnomaly>(mask),
        })
    }

    fn clcw_encode(case: &CodecCase) -> R<Value> {
        let p = params(case);
        let c = aldis_clcw_t {
            status: u8_of(&p, "status"),
            cop_in_effect: u8_of(&p, "cop_in_effect"),
            vcid: u8_of(&p, "vc_id"),
            no_rf_available: flag(&p, "no_rf_available"),
            no_bit_lock: flag(&p, "no_bit_lock"),
            lockout: flag(&p, "lockout"),
            wait: flag(&p, "wait"),
            retransmit: flag(&p, "retransmit"),
            farm_b_counter: u8_of(&p, "farm_b_counter"),
            report_value: u8_of(&p, "report_value"),
        };
        let mut word = 0_u32;
        // SAFETY: `c` and `word` are live.
        call(|e| unsafe { aldis_clcw_encode(&raw const c, &raw mut word, e) })?;
        Ok(json!({ "output_hex": hex::encode(&word.to_be_bytes()) }))
    }

    fn run(case: &CodecCase) -> R<Value> {
        let ch = channels(case);
        if let Some(ch) = &ch {
            // The configuration is checked first, as the Rust runner builds it (API-040).
            // SAFETY: every pointer is to a live local or vector.
            call(|e| unsafe { aldis_tc_config_check(&raw const ch.pc, e) })?;
            for (mc, vcs, maps) in &ch.mcs {
                // SAFETY: as above.
                call(|e| unsafe {
                    aldis_tc_mc_config_check(
                        mc,
                        vcs.as_ptr(),
                        vcs.len(),
                        maps.as_ptr(),
                        maps.len(),
                        e,
                    )
                })?;
            }
        }
        let need = || {
            ch.as_ref()
                .unwrap_or_else(|| panic!("{}: no configuration", case.id))
        };
        let bytes = input(case);
        match case.operation.as_str() {
            "config" => {
                let mcs = &need().mcs;
                Ok(json!({
                    "vcs": mcs.iter().map(|m| m.1.len()).sum::<usize>(),
                    "maps": mcs.iter().map(|m| m.2.len()).sum::<usize>(),
                }))
            }
            "decode" => {
                let (mc, vcs, maps) = need().mc(case);
                let mut view = aldis_tc_frame_t::default();
                // SAFETY: every pointer is to a live local or vector.
                call(|e| unsafe {
                    aldis_tc_decode(
                        bytes.as_ptr(),
                        bytes.len(),
                        mc,
                        vcs.as_ptr(),
                        vcs.len(),
                        maps.as_ptr(),
                        maps.len(),
                        mode(case),
                        &raw mut view,
                        e,
                    )
                })?;
                json_of(&view, &bytes)
            }
            "peek_length" => {
                let mut n = 0_usize;
                // SAFETY: as above.
                call(|e| unsafe {
                    aldis_tc_peek_length(bytes.as_ptr(), bytes.len(), &raw mut n, e)
                })?;
                Ok(json!({ "frame_len": n }))
            }
            "encode" => encode(case, need()),
            "validate" => validate(case, need(), &bytes),
            "delimit" => delimit(&bytes),
            "clcw_decode" => {
                let word = u32::from_be_bytes(bytes.as_slice().try_into().unwrap());
                let (mut c, mut mask) = (aldis_clcw_t::default(), u32::MAX);
                // SAFETY: as above.
                call(|e| unsafe {
                    aldis_clcw_decode(word, mode(case), &raw mut c, &raw mut mask, e)
                })?;
                Ok(clcw_json(&c, mask))
            }
            "clcw_encode" => clcw_encode(case),
            op => panic!("{}: unsupported operation {op}", case.id),
        }
    }

    vector_test!(tc_ad, "tc", "ad", run);
    vector_test!(tc_bd, "tc", "bd", run);
    vector_test!(tc_bc_unlock, "tc", "bc_unlock", run);
    vector_test!(tc_bc_setvr, "tc", "bc_setvr", run);
    vector_test!(tc_segment_header, "tc", "segment_header", run);
    vector_test!(tc_max_length, "tc", "max_length", run);
    vector_test!(
        tc_delimit_multiple_frames_with_fill,
        "tc",
        "delimit_multiple_frames_with_fill",
        run
    );
    vector_test!(tc_validation_failures, "tc", "validation_failures", run);
    vector_test!(tc_decode_errors, "tc", "decode_errors", run);
    vector_test!(tc_config, "tc", "config", run);
    vector_test!(tc_clcw, "tc", "clcw", run);
}

// ---- USLP (M2 WP-5 module 9)
vector_test!(uslp_config, "uslp", "config", uslp_run);
vector_test!(
    uslp_fixed_packets_rule000,
    "uslp",
    "fixed_packets_rule000",
    uslp_run
);
vector_test!(
    uslp_fixed_mapa_rules001_010,
    "uslp",
    "fixed_mapa_rules001_010",
    uslp_run
);
vector_test!(
    uslp_variable_rules011_111,
    "uslp",
    "variable_rules011_111",
    uslp_run
);
vector_test!(
    uslp_vcf_count_lengths_0_to_7,
    "uslp",
    "vcf_count_lengths_0_to_7",
    uslp_run
);
vector_test!(uslp_truncated, "uslp", "truncated", uslp_run);
vector_test!(
    uslp_truncated_sdls_layout,
    "uslp",
    "truncated_sdls_layout",
    uslp_run
);
vector_test!(uslp_oid, "uslp", "oid", uslp_run);
vector_test!(uslp_insert_zone, "uslp", "insert_zone", uslp_run);
vector_test!(
    uslp_protocol_control_cop1,
    "uslp",
    "protocol_control_cop1",
    uslp_run
);
vector_test!(
    uslp_delimit_multiple_frames_with_fill,
    "uslp",
    "delimit_multiple_frames_with_fill",
    uslp_run
);
vector_test!(
    uslp_validation_failures,
    "uslp",
    "validation_failures",
    uslp_run
);
vector_test!(uslp_invalid, "uslp", "invalid", uslp_run);
// ---- end USLP
