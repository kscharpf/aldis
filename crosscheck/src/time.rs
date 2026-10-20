//! Time code vectors (301.0-B-4) against `spacepackets` 0.18.0: `time/pfield`, `time/cuc`,
//! `time/cds`, `time/ascii`, and the Time Code Field of `spp/secondary_header_time`.
//!
//! CUC goes through `CucTime`, CDS through `get_dyn_time_provider_from_bytes` and `CdsTime`,
//! and ASCII formatting through `spacepackets::time::ascii`, whose strftime patterns `chrono`
//! renders. What each operation compares is listed in cross-checks.md §2. The differences
//! recorded in §3 are reported as [`Verdict::Known`] only under the exact condition each
//! entry describes; any other difference is a disagreement.

use aldis_testkit::{CodecCase, Mode, hex};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use serde_json::{Map, Value, json};
use spacepackets::time::cds::{
    CdsBase, CdsError, CdsTime, CdsTimestamp, DaysLen16Bits, DaysLen24Bits, LengthOfDaySegment,
    SubmillisPrecision, get_dyn_time_provider_from_bytes,
};
use spacepackets::time::cuc::{
    CucError, CucTime, FractionalPart, FractionalResolution, WidthCounterPair,
    fractional_part_from_subsec_ns,
};
use spacepackets::time::{
    CcsdsTimeCode, CcsdsTimeProvider, TimeReader, TimeWriter, TimestampError, ascii,
    ccsds_time_code_from_p_field,
};
use spacepackets::{ByteConversionError, CcsdsPacket, CcsdsPacketReader};

use crate::{Verdict, compare_produced, expected, input, param, param_hex, param_u64};

/// Nanoseconds per second.
const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// P-field bit 0, the extension flag (301.0 §3.1.1; CCSDS bit 0 is the most significant).
const EXTENSION_FLAG: u8 = 0x80;

/// The longest time code `spacepackets` writes: a 24-bit-day CDS code with picoseconds.
const MAX_CODE_LEN: usize = 12;

/// The skip for a case with an agency-defined epoch.
const AGENCY_EPOCH: &str = "agency-defined epoch (Level 2): spacepackets reads only the \
                            1958 January 1 epoch";

/// The skip for a CDS range that `spacepackets` does not check.
const CDS_RANGE: &str = "spacepackets does not check the CDS ranges (ms of day, sub-ms; TIME-021)";

/// Checks one case of `time/cuc.json`.
#[must_use]
pub fn check_cuc(case: &CodecCase) -> Verdict {
    match case.operation.as_str() {
        "decode" => cuc_decode(case),
        "decode_implicit" => cuc_decode_implicit(case),
        "encode" => cuc_encode(case),
        "cuc_to_ns" => cuc_to_ns(case),
        "cuc_from_ns" => cuc_from_ns(case),
        other => Verdict::Skip(format!("operation {other}: no counterpart in spacepackets")),
    }
}

/// Checks one case of `time/pfield.json`: `spacepackets` selects the decoder from the time code
/// ID (`ccsds_time_code_from_p_field`), and encodes CUC and CDS as for their own files.
#[must_use]
pub fn check_pfield(case: &CodecCase) -> Verdict {
    match case.operation.as_str() {
        "decode" => pfield_decode(case),
        "encode" => match member_str(case, param(case, "format"), "code") {
            "cuc" => cuc_encode(case),
            "cds" => cds_encode(case),
            other => Verdict::Skip(format!("{other} code: spacepackets has no {other} encoder")),
        },
        other => Verdict::Skip(format!("operation {other}: no counterpart in spacepackets")),
    }
}

fn pfield_decode(case: &CodecCase) -> Verdict {
    let Some(&pfield) = input(case).first() else {
        return Verdict::Skip(
            "empty input: spacepackets selects its decoder by the P-field".to_owned(),
        );
    };
    let lenient = case.mode == Some(Mode::Lenient);
    match ccsds_time_code_from_p_field(pfield) {
        Ok(CcsdsTimeCode::CucCcsdsEpoch | CcsdsTimeCode::CucAgencyEpoch) => cuc_decode(case),
        Ok(CcsdsTimeCode::Cds) => cds_decode(case),
        Ok(CcsdsTimeCode::Ccs) => Verdict::Skip("CCS: spacepackets has no CCS codec".to_owned()),
        Ok(CcsdsTimeCode::AgencyDefined) => Verdict::Skip(
            "agency-defined time code (ID 110): spacepackets has no reader for it".to_owned(),
        ),
        // 301.0 §3.1.1 reserves IDs 000, 011, and 111; spacepackets knows no other.
        Err(_) if case.expect.error.as_deref() == Some("ReservedValue") => Verdict::Agree,
        Err(_) if lenient && case.expect.ok => Verdict::Skip(
            "a reserved time code ID in lenient mode: spacepackets has one, strict, mode"
                .to_owned(),
        ),
        Err(id) => Verdict::Disagree(format!("spacepackets rejects time code ID {id:03b}")),
    }
}

/// Checks one case of `time/cds.json`.
#[must_use]
pub fn check_cds(case: &CodecCase) -> Verdict {
    match case.operation.as_str() {
        "decode" => cds_decode(case),
        "decode_implicit" => cds_decode_implicit(case),
        "encode" => cds_encode(case),
        other => Verdict::Skip(format!("operation {other}: no counterpart in spacepackets")),
    }
}

/// Checks one case of `time/ascii.json`.
#[must_use]
pub fn check_ascii(case: &CodecCase) -> Verdict {
    match case.operation.as_str() {
        "ascii_format" => ascii_format(case),
        "ascii_parse" | "ascii_subset" => {
            Verdict::Skip("spacepackets has no ASCII time code parser".to_owned())
        }
        other => Verdict::Skip(format!("operation {other}: no counterpart in spacepackets")),
    }
}

/// Checks the Time Code Field of one case of `spp/secondary_header_time.json`.
#[must_use]
pub fn check_secondary_header(case: &CodecCase) -> Verdict {
    if case.operation != "split_time" {
        return Verdict::Skip(format!(
            "operation {}: no counterpart in spacepackets",
            case.operation
        ));
    }
    split_time(case)
}

/// The member `key` of the JSON object `value`.
fn member<'a>(case: &CodecCase, value: &'a Value, key: &str) -> &'a Value {
    value
        .get(key)
        .unwrap_or_else(|| panic!("{}: no member {key}", case.id))
}

fn member_str<'a>(case: &CodecCase, value: &'a Value, key: &str) -> &'a str {
    member(case, value, key)
        .as_str()
        .unwrap_or_else(|| panic!("{}: {key} is not a string", case.id))
}

fn member_u64(case: &CodecCase, value: &Value, key: &str) -> u64 {
    member(case, value, key)
        .as_u64()
        .unwrap_or_else(|| panic!("{}: {key} is not an unsigned integer", case.id))
}

fn member_bool(case: &CodecCase, value: &Value, key: &str) -> bool {
    member(case, value, key)
        .as_bool()
        .unwrap_or_else(|| panic!("{}: {key} is not a boolean", case.id))
}

fn member_hex(case: &CodecCase, value: &Value, key: &str) -> Vec<u8> {
    hex::decode(member_str(case, value, key)).unwrap_or_else(|e| panic!("{}: {key}: {e}", case.id))
}

/// Whether `spacepackets`' rejection is its counterpart of the expected error kind.
fn counterpart(expected: &str, err: &TimestampError) -> bool {
    match err {
        TimestampError::ByteConversion(ByteConversionError::FromSliceTooSmall { .. }) => {
            matches!(expected, "Incomplete" | "LengthMismatch")
        }
        TimestampError::Cuc(
            CucError::InvalidCounter { .. } | CucError::InvalidFractions { .. },
        )
        | TimestampError::Cds(CdsError::InvalidCcsdsDays(_)) => expected == "OutOfRange",
        TimestampError::InvalidTimeCode { .. }
        | TimestampError::Cds(CdsError::InvalidCtorForDaysOfLenInPreamble(_)) => {
            expected == "InvalidField"
        }
        _ => false,
    }
}

/// The verdict when `spacepackets` rejects the case's input.
fn rejected(case: &CodecCase, err: &TimestampError) -> Verdict {
    let agency = match err {
        TimestampError::InvalidTimeCode { found, .. } => {
            *found == CcsdsTimeCode::CucAgencyEpoch as u8
        }
        TimestampError::CustomEpochNotSupported => true,
        _ => false,
    };
    match (case.expect.ok, case.expect.error.as_deref()) {
        (true, _) if agency => Verdict::Skip(AGENCY_EPOCH.to_owned()),
        (true, _) => Verdict::Disagree(format!("spacepackets rejects: {err}")),
        (false, Some(want)) if counterpart(want, err) => Verdict::Agree,
        (false, want) => Verdict::Disagree(format!(
            "expected {}, spacepackets reports {err}",
            want.unwrap_or("an error")
        )),
    }
}

/// The verdict when `spacepackets` accepts a case that expects an error and no known
/// discrepancy applies.
fn undetected(case: &CodecCase) -> Verdict {
    match case.expect.error.as_deref() {
        Some("OutOfRange") => Verdict::Skip(CDS_RANGE.to_owned()),
        want => Verdict::Disagree(format!(
            "expected {}, spacepackets accepts the input",
            want.unwrap_or("an error")
        )),
    }
}

// ---------------------------------------------------------------------------------------------
// CUC (301.0 §3.2)

/// A CUC format within `spacepackets`' scope: Level 1, 1–4 coarse and 0–3 fine octets, and a
/// one-octet P-field (301.0 §3.2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CucScope {
    coarse: u8,
    fine: FractionalResolution,
}

impl CucScope {
    /// The format, or why it is outside the scope.
    fn new(level2: bool, coarse: u64, fine: u64, octet2: bool) -> Result<Self, String> {
        if level2 {
            return Err(AGENCY_EPOCH.to_owned());
        }
        let coarse_octets = u8::try_from(coarse).ok().filter(|c| (1..=4).contains(c));
        let fine_octets = u8::try_from(fine)
            .ok()
            .and_then(|f| FractionalResolution::try_from(f).ok());
        match (coarse_octets, fine_octets) {
            (Some(coarse), Some(fine)) if !octet2 => Ok(Self { coarse, fine }),
            (Some(_), Some(_)) => {
                Err("P-field octet 2: spacepackets writes a one-octet P-field".to_owned())
            }
            _ => Err(format!(
                "{coarse} coarse and {fine} fine octets: spacepackets supports 1–4 and 0–3"
            )),
        }
    }

    /// The format of a `time/cuc.json` case.
    fn from_vector(case: &CodecCase, format: &Value) -> Result<Self, String> {
        if member_str(case, format, "epoch") != "level1" {
            return Err(AGENCY_EPOCH.to_owned());
        }
        if member_u64(case, format, "mission_bits") != 0 {
            return Err(
                "mission-defined bits in P-field octet 2: spacepackets writes a \
                        one-octet P-field"
                    .to_owned(),
            );
        }
        if format
            .get("pfield_tail_hex")
            .and_then(Value::as_str)
            .is_some_and(|tail| !tail.is_empty())
        {
            return Err(
                "P-field extension octets: spacepackets writes a one-octet P-field".to_owned(),
            );
        }
        Self::new(
            false,
            member_u64(case, format, "coarse_octets"),
            member_u64(case, format, "fine_octets"),
            member_bool(case, format, "octet2"),
        )
    }

    /// The number of fine octets.
    const fn fine_octets(self) -> usize {
        self.fine as usize
    }

    /// The value with `coarse` and the fine octets `fine`. The outer error says why the value
    /// is outside the scope; the inner one is `spacepackets`' rejection.
    fn time(self, coarse: u64, fine: &[u8]) -> Result<Result<CucTime, CucError>, String> {
        let Ok(coarse) = u32::try_from(coarse) else {
            return Err(format!(
                "coarse time {coarse}: spacepackets has a 32-bit counter"
            ));
        };
        if fine.len() != self.fine_octets() {
            return Err(format!(
                "{} fine octets: spacepackets supports 0–3",
                fine.len()
            ));
        }
        let counter = fine
            .iter()
            .fold(0_u32, |acc, octet| (acc << 8) | u32::from(*octet));
        let fractions = FractionalPart::new_checked(self.fine, counter)
            .ok_or_else(|| format!("fine time {counter:#x} rejected by FractionalPart"))?;
        Ok(CucTime::new_generic(
            WidthCounterPair(self.coarse, coarse),
            fractions,
        ))
    }

    /// The P-field `spacepackets` writes for this format.
    fn pfield(self) -> u8 {
        let time = CucTime::new_generic(
            WidthCounterPair(self.coarse, 0),
            FractionalPart::new_checked(self.fine, 0)
                .unwrap_or_else(|| panic!("fine time 0 rejected for {self:?}")),
        )
        .unwrap_or_else(|e| panic!("the zero value of {self:?} rejected: {e}"));
        written(&time)[0]
    }
}

/// The octets `spacepackets` writes for `time`.
fn written(time: &impl TimeWriter) -> Vec<u8> {
    let mut buf = [0_u8; MAX_CODE_LEN];
    let len = time
        .write_to_bytes(&mut buf)
        .unwrap_or_else(|e| panic!("spacepackets cannot write a time code: {e}"));
    buf[..len].to_vec()
}

/// The fine time octets of `time`.
fn fine_octets(time: &CucTime) -> Vec<u8> {
    let fractions = time.fractions();
    let len = fractions.resolution() as usize;
    fractions.counter().to_be_bytes()[4 - len..].to_vec()
}

/// The P-field octets `spacepackets` reports.
fn pfield_octets(time: &(impl CcsdsTimeProvider + ?Sized)) -> Vec<u8> {
    let (len, octets) = time.p_field();
    octets[..len].to_vec()
}

/// The values `spacepackets` reads from a CUC code. With an implicit P-field, `prefix` is the
/// length of the P-field the harness put in front, and the format values are not produced.
fn cuc_values(time: &CucTime, explicit: bool, prefix: usize) -> Map<String, Value> {
    let mut values = Map::new();
    if explicit {
        values.insert("code".into(), json!("cuc"));
        values.insert(
            "pfield_hex".into(),
            json!(hex::encode(&pfield_octets(&time.to_leap_sec_helper(0)))),
        );
        // `from_bytes` accepts only time code ID 001 (Level 1).
        values.insert("format.epoch".into(), json!("level1"));
        values.insert("format.coarse_octets".into(), json!(time.counter_width()));
        values.insert(
            "format.fine_octets".into(),
            json!(time.fractions().resolution() as u8),
        );
    }
    values.insert("value.coarse".into(), json!(time.counter()));
    values.insert(
        "value.fine_hex".into(),
        json!(hex::encode(&fine_octets(time))),
    );
    values.insert(
        "consumed".into(),
        json!(time.len_written().saturating_sub(prefix)),
    );
    values
}

/// Known discrepancy D3 (cross-checks.md §3): the input's P-field sets the extension flag, and
/// `spacepackets` decodes it as a one-octet P-field, taking octet 2 as T-field.
fn ignored_extension(pfield: u8, provider: &(impl CcsdsTimeProvider + ?Sized)) -> Option<Verdict> {
    let reported = pfield_octets(provider);
    (pfield & EXTENSION_FLAG != 0 && reported.len() == 1).then(|| {
        Verdict::Known(format!(
            "D3: P-field {pfield:02X} sets the extension flag; spacepackets reads a one-octet \
             P-field and takes octet 2 as T-field"
        ))
    })
}

fn cuc_decode(case: &CodecCase) -> Verdict {
    let octets = input(case);
    match CucTime::from_bytes(&octets) {
        Ok(time) => {
            if let Some(known) = octets
                .first()
                .and_then(|p| ignored_extension(*p, &time.to_leap_sec_helper(0)))
            {
                return known;
            }
            if case.expect.ok {
                compare_produced(case, &cuc_values(&time, true, 0))
            } else {
                undetected(case)
            }
        }
        Err(e) => rejected(case, &e),
    }
}

fn cuc_decode_implicit(case: &CodecCase) -> Verdict {
    let scope = match CucScope::from_vector(case, param(case, "format")) {
        Ok(scope) => scope,
        Err(reason) => return Verdict::Skip(reason),
    };
    // spacepackets has no reader for an implicit P-field: put its own P-field for the format
    // in front (301.0 §3.1.1 allows either form for the same T-field).
    let mut octets = vec![scope.pfield()];
    octets.extend(input(case));
    match CucTime::from_bytes(&octets) {
        Ok(time) if case.expect.ok => compare_produced(case, &cuc_values(&time, false, 1)),
        Ok(_) => undetected(case),
        Err(e) => rejected(case, &e),
    }
}

fn cuc_encode(case: &CodecCase) -> Verdict {
    let format = param(case, "format");
    let scope = match CucScope::from_vector(case, format) {
        Ok(scope) => scope,
        Err(reason) => return Verdict::Skip(reason),
    };
    let value = param(case, "value");
    let built = match scope.time(
        member_u64(case, value, "coarse"),
        &member_hex(case, value, "fine_hex"),
    ) {
        Ok(built) => built,
        Err(reason) => return Verdict::Skip(reason),
    };
    match built {
        Ok(time) if case.expect.ok => {
            let octets = written(&time);
            // Without a P-field, the T-field follows spacepackets' one-octet P-field.
            let skip = usize::from(!param_bool(case, "include_pfield"));
            let mut values = Map::new();
            values.insert("output_hex".into(), json!(hex::encode(&octets[skip..])));
            compare_produced(case, &values)
        }
        Ok(_) => undetected(case),
        Err(e) => rejected(case, &TimestampError::from(e)),
    }
}

fn param_bool(case: &CodecCase, key: &str) -> bool {
    param(case, key)
        .as_bool()
        .unwrap_or_else(|| panic!("{}: parameter {key} is not a boolean", case.id))
}

/// `numerator / denominator`, rounded down and up.
const fn floor_ceil(numerator: u64, denominator: u64) -> (u64, u64) {
    (numerator / denominator, numerator.div_ceil(denominator))
}

fn cuc_to_ns(case: &CodecCase) -> Verdict {
    let coarse = param_u64(case, "coarse");
    let fine = param_hex(case, "fine_hex");
    // The coarse width does not affect the conversion; 4 octets hold every coarse value here.
    let scope = match CucScope::new(false, 4, fine.len() as u64, false) {
        Ok(scope) => scope,
        Err(reason) => return Verdict::Skip(reason),
    };
    let time = match scope.time(coarse, &fine) {
        Ok(Ok(time)) => time,
        Ok(Err(e)) => return Verdict::Disagree(format!("spacepackets rejects the value: {e}")),
        Err(reason) => return Verdict::Skip(reason),
    };
    let nanoseconds = u64::from(time.to_leap_sec_helper(0).subsec_nanos());
    let mut values = Map::new();
    values.insert("seconds".into(), json!(time.counter()));
    values.insert("nanoseconds".into(), json!(nanoseconds));
    match compare_produced(case, &values) {
        Verdict::Disagree(diff) => {
            divisor_discrepancy(case, coarse, &fine, nanoseconds).unwrap_or(Verdict::Disagree(diff))
        }
        other => other,
    }
}

/// Known discrepancy D1 (cross-checks.md §3): `spacepackets` produced the fine time divided by
/// 2^(8n) − 1, and the vector holds it divided by 2^(8n), each rounded down; the seconds agree.
fn divisor_discrepancy(
    case: &CodecCase,
    coarse: u64,
    fine: &[u8],
    produced: u64,
) -> Option<Verdict> {
    let counter = fine
        .iter()
        .fold(0_u64, |acc, octet| (acc << 8) | u64::from(*octet));
    let modulus = 1_u64.checked_shl(u32::try_from(8 * fine.len()).ok()?)?;
    let scaled = counter.checked_mul(NANOS_PER_SECOND)?;
    let binary = scaled / modulus;
    let spacepackets = scaled / modulus.checked_sub(1).filter(|d| *d > 0)?;
    let want = expected(case, "nanoseconds").as_u64()?;
    let seconds = expected(case, "seconds").as_u64()?;
    (seconds == coarse && want == binary && produced == spacepackets && want != produced).then(
        || {
            Verdict::Known(format!(
                "D1: fine time {} divided by 2^{} − 1 is {produced} ns in spacepackets; divided \
                 by 2^{}, {want} ns in the vector (301.0 §3.2.1)",
                hex::encode(fine),
                8 * fine.len(),
                8 * fine.len()
            ))
        },
    )
}

fn cuc_from_ns(case: &CodecCase) -> Verdict {
    let seconds = param_u64(case, "seconds");
    let nanoseconds = param_u64(case, "nanoseconds");
    let scope = match CucScope::new(
        false,
        param_u64(case, "coarse_octets"),
        param_u64(case, "fine_octets"),
        false,
    ) {
        Ok(scope) => scope,
        Err(reason) => return Verdict::Skip(reason),
    };
    if nanoseconds > NANOS_PER_SECOND {
        return Verdict::Skip("spacepackets panics on a sub-second part above 1 s".to_owned());
    }
    let Ok(coarse) = u32::try_from(seconds) else {
        return Verdict::Skip(format!("{seconds} s: spacepackets has a 32-bit counter"));
    };
    let fractions = fractional_part_from_subsec_ns(scope.fine, nanoseconds);
    match CucTime::new_generic(WidthCounterPair(scope.coarse, coarse), fractions) {
        Ok(time) => {
            let mut values = Map::new();
            values.insert("coarse".into(), json!(time.counter()));
            values.insert("fine_hex".into(), json!(hex::encode(&fine_octets(&time))));
            let verdict = if case.expect.ok {
                compare_produced(case, &values)
            } else {
                Verdict::Disagree(format!(
                    "expected {}, spacepackets produces {}",
                    case.expect.error.as_deref().unwrap_or("an error"),
                    Value::from(values)
                ))
            };
            match verdict {
                Verdict::Disagree(diff) => {
                    rounding_discrepancy(case, scope, seconds, nanoseconds, &time)
                        .unwrap_or(Verdict::Disagree(diff))
                }
                other => other,
            }
        }
        Err(e) => rejected(case, &TimestampError::from(e)),
    }
}

/// Known discrepancy D4 (cross-checks.md §3): `spacepackets` rounds the binary fraction of the
/// sub-second part down, and the vector rounds it up (TIME-011), carrying into the coarse time
/// or, when the coarse time is full, failing with `OutOfRange`.
fn rounding_discrepancy(
    case: &CodecCase,
    scope: CucScope,
    seconds: u64,
    nanoseconds: u64,
    time: &CucTime,
) -> Option<Verdict> {
    let bits = u32::try_from(8 * scope.fine_octets()).ok()?;
    let modulus = 1_u64.checked_shl(bits)?;
    let (down, up) = floor_ceil(nanoseconds.checked_mul(modulus)?, NANOS_PER_SECOND);
    let fine_hex = |fine: u64| {
        let octets = fine.to_be_bytes();
        hex::encode(&octets[8 - scope.fine_octets()..])
    };
    // spacepackets: the truncated fraction, the coarse time unchanged.
    let produced_down =
        u64::from(time.counter()) == seconds && hex::encode(&fine_octets(time)) == fine_hex(down);
    // The vector: the rounded-up fraction, with its carry.
    let (coarse_up, fine_up) = if up == modulus {
        (seconds.checked_add(1)?, 0)
    } else {
        (seconds, up)
    };
    let coarse_limit = 1_u64.checked_shl(8 * u32::from(scope.coarse))?;
    let vector_up = if coarse_up < coarse_limit {
        case.expect.ok
            && expected(case, "coarse").as_u64() == Some(coarse_up)
            && expected(case, "fine_hex").as_str() == Some(fine_hex(fine_up).as_str())
    } else {
        case.expect.error.as_deref() == Some("OutOfRange")
    };
    (down != up && produced_down && vector_up).then(|| {
        Verdict::Known(format!(
            "D4: {seconds} s + {nanoseconds} ns is fine time {} rounded down in spacepackets, \
             {} rounded up in the vector (TIME-011)",
            fine_hex(down),
            if coarse_up < coarse_limit {
                format!("{coarse_up} s + {}", fine_hex(fine_up))
            } else {
                "an OutOfRange coarse carry".to_owned()
            }
        ))
    })
}

// ---------------------------------------------------------------------------------------------
// CDS (301.0 §3.3)

/// A CDS format within `spacepackets`' scope: the 1958 epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CdsScope {
    day24: bool,
    subms: SubmillisPrecision,
}

impl CdsScope {
    /// The format of a `time/cds.json` case.
    fn from_vector(case: &CodecCase, format: &Value) -> Result<Self, String> {
        if member_str(case, format, "epoch") != "ccsds" {
            return Err(AGENCY_EPOCH.to_owned());
        }
        if format
            .get("pfield_tail_hex")
            .and_then(Value::as_str)
            .is_some_and(|tail| !tail.is_empty())
        {
            return Err(
                "P-field extension octets: spacepackets writes a one-octet P-field".to_owned(),
            );
        }
        let day24 = match member_u64(case, format, "day_bits") {
            16 => false,
            24 => true,
            other => panic!("{}: day_bits {other}", case.id),
        };
        let subms = match member_str(case, format, "subms") {
            "none" => SubmillisPrecision::Absent,
            "us" => SubmillisPrecision::Microseconds,
            "ps" => SubmillisPrecision::Picoseconds,
            other => panic!("{}: subms {other}", case.id),
        };
        Ok(Self { day24, subms })
    }

    /// The format of a `spp/secondary_header_time.json` layout.
    fn from_layout(case: &CodecCase, format: &Value) -> Result<Self, String> {
        if member_bool(case, format, "agency_epoch") {
            return Err(AGENCY_EPOCH.to_owned());
        }
        let subms = match member_u64(case, format, "subms") {
            0 => SubmillisPrecision::Absent,
            1 => SubmillisPrecision::Microseconds,
            2 => SubmillisPrecision::Picoseconds,
            other => panic!("{}: subms {other}", case.id),
        };
        Ok(Self {
            day24: member_bool(case, format, "day24"),
            subms,
        })
    }

    /// The octets `spacepackets` writes for the value, or its rejection. The outer error says
    /// why the value is outside the scope.
    fn encode(
        self,
        day: u64,
        ms_of_day: u64,
        subms: u64,
    ) -> Result<Result<Vec<u8>, String>, String> {
        let (Ok(day), Ok(ms_of_day), Ok(subms)) = (
            u32::try_from(day),
            u32::try_from(ms_of_day),
            u32::try_from(subms),
        ) else {
            return Err("a field beyond 32 bits: spacepackets has 32-bit fields".to_owned());
        };
        let wide = match CdsTime::new_with_u24_days(day, ms_of_day) {
            Ok(time) => time,
            Err(e) => return Ok(Err(e.to_string())),
        };
        let unsupported = || format!("set_submillis rejects {subms} at {:?}", self.subms);
        Ok(if self.day24 {
            let mut time = wide;
            if time.set_submillis(self.subms, subms) {
                Ok(written(&time))
            } else {
                Err(unsupported())
            }
        } else {
            match CdsTime::<DaysLen16Bits>::try_from(wide) {
                Ok(mut time) => {
                    if time.set_submillis(self.subms, subms) {
                        Ok(written(&time))
                    } else {
                        Err(unsupported())
                    }
                }
                Err(e) => Err(e.to_string()),
            }
        })
    }

    /// The P-field `spacepackets` writes for this format.
    fn pfield(self) -> u8 {
        match self.encode(0, 0, 0) {
            Ok(Ok(octets)) => octets[0],
            other => panic!("the zero value of {self:?}: {other:?}"),
        }
    }
}

/// The values `spacepackets` reads from a CDS code; see [`cuc_values`] for `explicit` and
/// `prefix`.
fn cds_values<T>(time: &T, explicit: bool, prefix: usize) -> Map<String, Value>
where
    T: CcsdsTimeProvider + CdsTimestamp + ?Sized,
{
    let mut values = Map::new();
    if explicit {
        values.insert("code".into(), json!("cds"));
        values.insert(
            "pfield_hex".into(),
            json!(hex::encode(&pfield_octets(time))),
        );
        // The readers reject an agency-defined epoch (`CustomEpochNotSupported`).
        values.insert("format.epoch".into(), json!("ccsds"));
        values.insert(
            "format.day_bits".into(),
            json!(match time.len_of_day_seg() {
                LengthOfDaySegment::Short16Bits => 16,
                LengthOfDaySegment::Long24Bits => 24,
            }),
        );
        values.insert(
            "format.subms".into(),
            json!(match time.submillis_precision() {
                SubmillisPrecision::Absent => "none",
                SubmillisPrecision::Microseconds => "us",
                SubmillisPrecision::Picoseconds => "ps",
                SubmillisPrecision::Reserved => "reserved",
            }),
        );
    }
    values.insert("value.day".into(), json!(time.ccsds_days_as_u32()));
    values.insert("value.ms_of_day".into(), json!(time.ms_of_day()));
    values.insert("value.subms".into(), json!(time.submillis()));
    values.insert(
        "consumed".into(),
        json!(time.len_as_bytes().saturating_sub(prefix)),
    );
    values
}

/// Known discrepancy D2 (cross-checks.md §3): the vector rejects the reserved sub-millisecond
/// code `11` with `ReservedValue`, and `spacepackets` decodes it as a code without that segment.
fn reserved_subms(case: &CodecCase, time: &(impl CdsBase + ?Sized)) -> Option<Verdict> {
    (time.submillis_precision() == SubmillisPrecision::Reserved).then(|| {
        if case.expect.error.as_deref() == Some("ReservedValue") {
            Verdict::Known(
                "D2: sub-millisecond code 11 is reserved (301.0 §3.3.2); spacepackets decodes it \
                 as absent"
                    .to_owned(),
            )
        } else {
            Verdict::Disagree(
                "spacepackets decodes the reserved sub-millisecond code 11".to_owned(),
            )
        }
    })
}

/// The verdict on a CDS code `spacepackets` decoded. `pfield` is the input's P-field octet when
/// it is explicit; otherwise `prefix` is the length of the P-field the harness put in front.
fn cds_decoded<T>(case: &CodecCase, time: &T, pfield: Option<u8>, prefix: usize) -> Verdict
where
    T: CcsdsTimeProvider + CdsTimestamp + ?Sized,
{
    if let Some(known) = pfield
        .and_then(|p| ignored_extension(p, time))
        .or_else(|| reserved_subms(case, time))
    {
        return known;
    }
    if case.expect.ok {
        compare_produced(case, &cds_values(time, pfield.is_some(), prefix))
    } else {
        undetected(case)
    }
}

fn cds_decode(case: &CodecCase) -> Verdict {
    let octets = input(case);
    let Some(&pfield) = octets.first() else {
        return Verdict::Skip(
            "empty input: spacepackets reads the first octet unchecked".to_owned(),
        );
    };
    match get_dyn_time_provider_from_bytes(&octets) {
        Ok(time) => cds_decoded(case, &*time, Some(pfield), 0),
        Err(e) => rejected(case, &e),
    }
}

fn cds_decode_implicit(case: &CodecCase) -> Verdict {
    let scope = match CdsScope::from_vector(case, param(case, "format")) {
        Ok(scope) => scope,
        Err(reason) => return Verdict::Skip(reason),
    };
    // As for CUC: spacepackets' own P-field for the format in front.
    let mut octets = vec![scope.pfield()];
    octets.extend(input(case));
    match get_dyn_time_provider_from_bytes(&octets) {
        Ok(time) => cds_decoded(case, &*time, None, 1),
        Err(e) => rejected(case, &e),
    }
}

fn cds_encode(case: &CodecCase) -> Verdict {
    let scope = match CdsScope::from_vector(case, param(case, "format")) {
        Ok(scope) => scope,
        Err(reason) => return Verdict::Skip(reason),
    };
    let value = param(case, "value");
    let built = match scope.encode(
        member_u64(case, value, "day"),
        member_u64(case, value, "ms_of_day"),
        member_u64(case, value, "subms"),
    ) {
        Ok(built) => built,
        Err(reason) => return Verdict::Skip(reason),
    };
    match (built, case.expect.ok) {
        (Ok(octets), true) => {
            let skip = usize::from(!param_bool(case, "include_pfield"));
            let mut values = Map::new();
            values.insert("output_hex".into(), json!(hex::encode(&octets[skip..])));
            compare_produced(case, &values)
        }
        (Ok(_), false) => undetected(case),
        (Err(e), true) => Verdict::Disagree(format!("spacepackets rejects the value: {e}")),
        (Err(_), false) if case.expect.error.as_deref() == Some("OutOfRange") => Verdict::Agree,
        (Err(e), false) => Verdict::Disagree(format!(
            "expected {}, spacepackets reports {e}",
            case.expect.error.as_deref().unwrap_or("an error")
        )),
    }
}

// ---------------------------------------------------------------------------------------------
// ASCII (301.0 §3.5)

/// The vector's `text` written at the three fraction digits of `spacepackets`' patterns
/// (`%.3f`): further digits dropped, missing ones zero. 301.0 §3.5.1.1–2 leave the number of
/// fraction digits to the user, so this is the same instant at that precision.
fn at_millis(text: &str) -> String {
    let (body, terminator) = text
        .strip_suffix('Z')
        .map_or((text, ""), |body| (body, "Z"));
    let (base, digits) = body.split_once('.').unwrap_or((body, ""));
    let fraction: String = digits
        .chars()
        .chain(std::iter::repeat('0'))
        .take(3)
        .collect();
    format!("{base}.{fraction}{terminator}")
}

/// The `chrono` date and time of an ASCII value. A second 60 is `chrono`'s leap second:
/// second 59 with a sub-second part of at least one second.
fn date_time(case: &CodecCase, value: &Value) -> Option<NaiveDateTime> {
    let year = i32::try_from(member_u64(case, value, "year")).ok()?;
    let date = match member_str(case, value, "variation") {
        "A" => NaiveDate::from_ymd_opt(
            year,
            u32::try_from(member_u64(case, value, "month")).ok()?,
            u32::try_from(member_u64(case, value, "day")).ok()?,
        )?,
        _ => NaiveDate::from_yo_opt(
            year,
            u32::try_from(member_u64(case, value, "day_of_year")).ok()?,
        )?,
    };
    let digits = u32::try_from(member_u64(case, value, "fraction_digits")).ok()?;
    let fraction = member_u64(case, value, "fraction");
    let nanoseconds = if digits <= 9 {
        fraction.checked_mul(10_u64.checked_pow(9 - digits)?)?
    } else {
        fraction / 10_u64.checked_pow(digits - 9)?
    };
    let second = u32::try_from(member_u64(case, value, "second")).ok()?;
    let (second, nanoseconds) = if second == 60 {
        (59, nanoseconds.checked_add(NANOS_PER_SECOND)?)
    } else {
        (second, nanoseconds)
    };
    let time = NaiveTime::from_hms_nano_opt(
        u32::try_from(member_u64(case, value, "hour")).ok()?,
        u32::try_from(member_u64(case, value, "minute")).ok()?,
        second,
        u32::try_from(nanoseconds).ok()?,
    )?;
    Some(NaiveDateTime::new(date, time))
}

fn ascii_format(case: &CodecCase) -> Verdict {
    let value = param(case, "value");
    let Some(naive) = date_time(case, value) else {
        return Verdict::Disagree("chrono rejects the date or time".to_owned());
    };
    let utc = naive.and_utc();
    let text = match (
        member_str(case, value, "variation"),
        member_bool(case, value, "z"),
    ) {
        ("A", false) => ascii::generate_time_code_a(&utc).to_string(),
        ("A", true) => ascii::generate_time_code_a_terminated(&utc).to_string(),
        (_, false) => ascii::generate_time_code_b(&utc).to_string(),
        (_, true) => ascii::generate_time_code_b_terminated(&utc).to_string(),
    };
    let vector = expected(case, "text")
        .as_str()
        .unwrap_or_else(|| panic!("{}: text is not a string", case.id));
    let want = at_millis(vector);
    if text == want {
        Verdict::Agree
    } else {
        Verdict::Disagree(format!(
            "text: expected {want} ({vector} at 3 digits), produced {text}"
        ))
    }
}

// ---------------------------------------------------------------------------------------------
// Time secondary header (133.0 §4.1.4.2, 301.0 §3.2–3.3)

/// The time code format of a secondary header layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimeScope {
    Cuc(CucScope),
    Cds(CdsScope),
}

/// The length `spacepackets` reads for the time code at the start of `octets`, or the verdict
/// when a known discrepancy applies.
fn time_code_len(
    case: &CodecCase,
    scope: TimeScope,
    explicit: bool,
    octets: &[u8],
) -> Result<Result<usize, Verdict>, TimestampError> {
    Ok(match scope {
        TimeScope::Cuc(_) => {
            let time = CucTime::from_bytes(octets)?;
            let known = octets
                .first()
                .filter(|_| explicit)
                .and_then(|p| ignored_extension(*p, &time.to_leap_sec_helper(0)));
            known.map_or(Ok(time.len_written()), Err)
        }
        TimeScope::Cds(cds) if cds.day24 => {
            let time = CdsTime::<DaysLen24Bits>::from_bytes(octets)?;
            reserved_subms(case, &time).map_or(Ok(time.len_as_bytes()), Err)
        }
        TimeScope::Cds(_) => {
            let time = CdsTime::<DaysLen16Bits>::from_bytes(octets)?;
            reserved_subms(case, &time).map_or(Ok(time.len_as_bytes()), Err)
        }
    })
}

fn split_time(case: &CodecCase) -> Verdict {
    let layout = param(case, "layout");
    let format = member(case, layout, "format");
    let explicit = member_bool(case, layout, "pfield");
    let scope = match member_str(case, format, "code") {
        "cuc" => CucScope::new(
            member_bool(case, format, "level2"),
            member_u64(case, format, "coarse"),
            member_u64(case, format, "fine"),
            false,
        )
        .map(TimeScope::Cuc),
        "cds" => CdsScope::from_layout(case, format).map(TimeScope::Cds),
        other => Err(format!("{other}: spacepackets has no {other} codec")),
    };
    let scope = match scope {
        Ok(scope) => scope,
        Err(reason) => return Verdict::Skip(reason),
    };
    let octets = input(case);
    let packet = match CcsdsPacketReader::new(&octets, None) {
        Ok(packet) => packet,
        Err(e) => return Verdict::Disagree(format!("spacepackets rejects the packet: {e}")),
    };
    if !packet.sec_header_flag() {
        return Verdict::Skip(
            "the layout's Secondary Header Flag rule (SPP-008), which spacepackets does not have"
                .to_owned(),
        );
    }
    let data = packet.packet_data();
    // An implicit P-field: spacepackets' own P-field for the format in front, as for CUC.
    let prefix = usize::from(!explicit);
    let mut code = Vec::with_capacity(prefix + data.len());
    if !explicit {
        code.push(match scope {
            TimeScope::Cuc(cuc) => cuc.pfield(),
            TimeScope::Cds(cds) => cds.pfield(),
        });
    }
    code.extend_from_slice(data);
    let len = match time_code_len(case, scope, explicit, &code) {
        Ok(Ok(len)) => len,
        Ok(Err(known)) => return known,
        Err(e) => return rejected(case, &e),
    };
    match case.expect.error.as_deref() {
        None => {
            let field = data.get(..len.saturating_sub(prefix)).unwrap_or(data);
            let mut values = Map::new();
            values.insert("time_code_hex".into(), json!(hex::encode(field)));
            compare_produced(case, &values)
        }
        Some("LengthMismatch") => Verdict::Skip(
            "the time code fits; the mismatch is in the ancillary field, which spacepackets does \
             not model"
                .to_owned(),
        ),
        Some(_) => undetected(case),
    }
}

#[cfg(test)]
mod tests {
    use aldis_testkit::CodecCase;
    use serde_json::{Value, json};

    use super::{at_millis, check_cds, check_cuc};
    use crate::Verdict;

    fn case(
        operation: &str,
        params: Option<Value>,
        input: Option<&str>,
        expect: &Value,
    ) -> CodecCase {
        let mut case = json!({
            "id": "test-001",
            "requirements": [],
            "operation": operation,
            "expect": expect,
        });
        if let Some(params) = params {
            case["params"] = params;
        }
        if let Some(input) = input {
            case["input_hex"] = json!(input);
        }
        serde_json::from_value(case).unwrap()
    }

    fn is_known(verdict: &Verdict, entry: &str) -> bool {
        matches!(verdict, Verdict::Known(text) if text.starts_with(entry))
    }

    fn is_disagree(verdict: &Verdict) -> bool {
        matches!(verdict, Verdict::Disagree(_))
    }

    #[test]
    fn divisor_discrepancy_needs_the_binary_fraction_in_the_vector() {
        // req: VER-040
        let params = json!({"coarse": 7, "fine_hex": "80"});
        let to_ns = |nanoseconds: u64| {
            check_cuc(&case(
                "cuc_to_ns",
                Some(params.clone()),
                None,
                &json!({"ok": true, "seconds": 7, "nanoseconds": nanoseconds}),
            ))
        };
        // 128/256 s in the vector, 128/255 s in spacepackets.
        assert!(is_known(&to_ns(500_000_000), "D1"));
        assert_eq!(to_ns(501_960_784), Verdict::Agree);
        assert!(is_disagree(&to_ns(500_000_001)));
    }

    #[test]
    fn rounding_discrepancy_needs_the_ceiling_in_the_vector() {
        // req: VER-040
        let params = json!({"seconds": 5, "nanoseconds": 999_999_999, "coarse_octets": 1,
                            "fine_octets": 1});
        let from_ns =
            |expect: &Value| check_cuc(&case("cuc_from_ns", Some(params.clone()), None, expect));
        // Rounded up, the fraction carries into the coarse time.
        assert!(is_known(
            &from_ns(&json!({"ok": true, "coarse": 6, "fine_hex": "00"})),
            "D4"
        ));
        assert_eq!(
            from_ns(&json!({"ok": true, "coarse": 5, "fine_hex": "FF"})),
            Verdict::Agree
        );
        assert!(is_disagree(&from_ns(
            &json!({"ok": true, "coarse": 6, "fine_hex": "01"})
        )));
        assert!(is_disagree(&from_ns(
            &json!({"ok": false, "error": "OutOfRange"})
        )));
    }

    #[test]
    fn ignored_extension_needs_the_flag() {
        // req: VER-040
        let decode = |input: &str, coarse: u64| {
            check_cuc(&case(
                "decode",
                None,
                Some(input),
                &json!({"ok": true, "value": {"coarse": coarse}}),
            ))
        };
        assert!(is_known(&decode("9C200102030405", 4_328_719_365), "D3"));
        assert!(is_known(&decode("9C200102030405", 0), "D3"));
        // Without the flag, a wrong value is a disagreement.
        assert!(is_disagree(&decode("1C01020304", 0)));
    }

    #[test]
    fn reserved_subms_needs_reserved_value_in_the_vector() {
        // req: VER-040
        let decode =
            |expect: &Value| check_cds(&case("decode", None, Some("43000000000000"), expect));
        assert!(is_known(
            &decode(&json!({"ok": false, "error": "ReservedValue"})),
            "D2"
        ));
        assert!(is_disagree(&decode(
            &json!({"ok": false, "error": "Incomplete"})
        )));
        assert!(is_disagree(&decode(&json!({"ok": true, "consumed": 7}))));
    }

    #[test]
    fn at_millis_truncates_and_pads_the_fraction() {
        // req: VER-040
        assert_eq!(
            at_millis("1988-01-18T17:20:43.123456Z"),
            "1988-01-18T17:20:43.123Z"
        );
        assert_eq!(at_millis("1988-018T17:20:43"), "1988-018T17:20:43.000");
        assert_eq!(
            at_millis("2000-01-01T00:00:00.5"),
            "2000-01-01T00:00:00.500"
        );
        assert_eq!(
            at_millis("2016-12-31T23:59:60.999999999999Z"),
            "2016-12-31T23:59:60.999Z"
        );
    }
}
