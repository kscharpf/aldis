//! Cross-checks the Aldis test vectors against independent implementations (VER-040, M1 D5).
//!
//! Each module turns one vector case into a [`Verdict`]. The `spacepackets` checks ([`spp`],
//! [`time`]) use only the independent implementation and the case's own parameters. The NASA
//! cFE checks ([`cfe`], [`cfe_properties`]) also compare cFE with `aldis-spp` on the same
//! packets. The checks per implementation, and every case they skip, are listed in
//! `docs/verification/cross-checks.md`.

pub mod cfe;
#[expect(
    unsafe_code,
    reason = "the bindings to the cFE MSG module, the only unsafe code of the cross-checks; each \
              block states why it is sound"
)]
pub mod cfe_msg;
pub mod cfe_properties;
pub mod spp;
pub mod time;

use aldis_testkit::{CodecCase, VectorFile, hex};
use serde_json::{Map, Value};

/// The verdict of an independent implementation on one vector case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Every value the implementation produces equals the expected value.
    Agree,
    /// The case is outside the implementation's scope (cross-checks.md §2), with the reason.
    Skip(String),
    /// The implementation differs from the vector exactly as a discrepancy recorded in
    /// cross-checks.md §3 describes, with its identifier. Each check names the precise
    /// condition under which it returns this; any other difference is a [`Verdict::Disagree`].
    Known(String),
    /// The implementation disagrees with the vector, with the differences.
    Disagree(String),
}

/// A standard module's check of one case.
pub type Check = fn(&CodecCase) -> Verdict;

/// The verdicts on one vector file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReport {
    /// The file, relative to `test-vectors/`.
    pub file: &'static str,
    /// Cases on which the implementation agrees.
    pub agreed: usize,
    /// Superseded cases, which are not checked (TV-021).
    pub superseded: usize,
    /// Skipped cases: identifier and reason.
    pub skipped: Vec<(String, String)>,
    /// Known discrepancies (cross-checks.md §3): identifier and description.
    pub known: Vec<(String, String)>,
    /// Disagreements: identifier and differences.
    pub disagreements: Vec<(String, String)>,
}

/// Checks every non-superseded case of `test-vectors/<file>` with `check`.
///
/// # Panics
/// If the file cannot be read or is not a codec vector file.
#[must_use]
pub fn check_file(file: &'static str, check: Check) -> FileReport {
    let path = aldis_testkit::vectors_dir().join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
    let cases = match VectorFile::from_json(&text) {
        Ok(VectorFile::Codec(codec)) => codec.cases,
        Ok(VectorFile::EngineScenario(_)) => panic!("{file}: not a codec file"),
        Err(e) => panic!("{file}: {e}"),
    };
    let mut report = FileReport {
        file,
        agreed: 0,
        superseded: 0,
        skipped: Vec::new(),
        known: Vec::new(),
        disagreements: Vec::new(),
    };
    for case in &cases {
        if case.superseded.is_some() {
            report.superseded += 1;
            continue;
        }
        match check(case) {
            Verdict::Agree => report.agreed += 1,
            Verdict::Skip(reason) => report.skipped.push((case.id.clone(), reason)),
            Verdict::Known(entry) => report.known.push((case.id.clone(), entry)),
            Verdict::Disagree(diff) => report.disagreements.push((case.id.clone(), diff)),
        }
    }
    report
}

/// Compares the values an implementation produced with the case's expected values.
///
/// Only the produced keys are compared: the expectation may hold more (cross-checks.md §2
/// lists them). A key `a.b` names the member `b` of the expected object `a`. A produced key
/// the expectation lacks is a disagreement, and so is producing nothing, which would
/// otherwise agree with any case.
#[must_use]
pub fn compare_produced(case: &CodecCase, produced: &Map<String, Value>) -> Verdict {
    if produced.is_empty() {
        return Verdict::Disagree("no values produced".to_owned());
    }
    let diffs: Vec<String> = produced
        .iter()
        .filter_map(|(key, got)| match expected_at(case, key) {
            None => Some(format!("{key}: not in the vector, produced {got}")),
            Some(want) if want != got => Some(format!("{key}: expected {want}, produced {got}")),
            Some(_) => None,
        })
        .collect();
    if diffs.is_empty() {
        Verdict::Agree
    } else {
        Verdict::Disagree(diffs.join("; "))
    }
}

/// The expected value at `key`, where `a.b` names the member `b` of the expected object `a`.
fn expected_at<'a>(case: &'a CodecCase, key: &str) -> Option<&'a Value> {
    let mut parts = key.split('.');
    let first = case.expect.values.get(parts.next()?)?;
    parts.try_fold(first, |value, part| value.get(part))
}

/// The case's `input_hex` as octets; empty when absent.
///
/// # Panics
/// If `input_hex` is not hex.
#[must_use]
pub fn input(case: &CodecCase) -> Vec<u8> {
    case.input_hex.as_deref().map_or_else(Vec::new, |text| {
        hex::decode(text).unwrap_or_else(|e| panic!("{}: input_hex: {e}", case.id))
    })
}

/// The parameter `key` of `case`.
///
/// # Panics
/// If the case has no such parameter.
#[must_use]
pub fn param<'a>(case: &'a CodecCase, key: &str) -> &'a Value {
    case.params
        .as_ref()
        .and_then(|params| params.get(key))
        .unwrap_or_else(|| panic!("{}: no parameter {key}", case.id))
}

/// The unsigned integer parameter `key` of `case`.
///
/// # Panics
/// If the parameter is missing or not an unsigned integer.
#[must_use]
pub fn param_u64(case: &CodecCase, key: &str) -> u64 {
    param(case, key)
        .as_u64()
        .unwrap_or_else(|| panic!("{}: parameter {key} is not an unsigned integer", case.id))
}

/// The hex parameter `key` of `case` as octets.
///
/// # Panics
/// If the parameter is missing or not hex.
#[must_use]
pub fn param_hex(case: &CodecCase, key: &str) -> Vec<u8> {
    let text = param(case, key)
        .as_str()
        .unwrap_or_else(|| panic!("{}: parameter {key} is not a string", case.id));
    hex::decode(text).unwrap_or_else(|e| panic!("{}: parameter {key}: {e}", case.id))
}

/// The expected value `key` of a successful case.
///
/// # Panics
/// If the expectation has no such value.
#[must_use]
pub fn expected<'a>(case: &'a CodecCase, key: &str) -> &'a Value {
    case.expect
        .values
        .get(key)
        .unwrap_or_else(|| panic!("{}: no expected {key}", case.id))
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value, json};

    use super::{CodecCase, Verdict, compare_produced};

    fn case(expect: &Value) -> CodecCase {
        serde_json::from_value(json!({
            "id": "test-001",
            "requirements": [],
            "operation": "decode",
            "expect": expect,
        }))
        .unwrap()
    }

    fn map(value: &Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn compare_produced_reports_every_difference() {
        // req: VER-040
        let case = case(&json!({"ok": true, "apid": 5, "sequence_count": 7, "idle": false}));
        assert_eq!(
            compare_produced(&case, &map(&json!({"apid": 5, "sequence_count": 7}))),
            Verdict::Agree
        );
        assert_eq!(
            compare_produced(&case, &map(&json!({"apid": 6, "sequence_count": 7}))),
            Verdict::Disagree("apid: expected 5, produced 6".to_owned())
        );
        assert_eq!(
            compare_produced(&case, &map(&json!({"apid": 5, "data_len": 1}))),
            Verdict::Disagree("data_len: not in the vector, produced 1".to_owned())
        );
        assert_eq!(
            compare_produced(&case, &Map::new()),
            Verdict::Disagree("no values produced".to_owned())
        );
    }

    #[test]
    fn compare_produced_reaches_into_objects() {
        // req: VER-040
        let case = case(&json!({"ok": true, "value": {"day": 3, "ms_of_day": 4}, "consumed": 7}));
        assert_eq!(
            compare_produced(&case, &map(&json!({"value.day": 3, "consumed": 7}))),
            Verdict::Agree
        );
        assert_eq!(
            compare_produced(&case, &map(&json!({"value.day": 2}))),
            Verdict::Disagree("value.day: expected 3, produced 2".to_owned())
        );
        assert_eq!(
            compare_produced(&case, &map(&json!({"value.subms": 0}))),
            Verdict::Disagree("value.subms: not in the vector, produced 0".to_owned())
        );
        assert_eq!(
            compare_produced(&case, &map(&json!({"consumed.x": 7}))),
            Verdict::Disagree("consumed.x: not in the vector, produced 7".to_owned())
        );
    }
}
