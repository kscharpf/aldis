//! Runs codec vector cases through a crate's handler and compares outcomes exactly.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::model::{CodecCase, CodecFile, VectorFile};

/// What a crate's handler produced for one case.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Success with the output values (compared exactly with `expect`).
    Ok(Map<String, Value>),
    /// Failure with the error kind name from the catalogue (e.g. `"CrcMismatch"`).
    Err(String),
    /// The handler does not implement this operation. Always a failure (TV-012).
    Unsupported,
}

/// One failed case.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    /// Case identifier.
    pub case_id: String,
    /// What differed.
    pub message: String,
}

/// Result of running one vector file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Report {
    /// The vector file.
    pub file: PathBuf,
    /// Cases that passed.
    pub passed: usize,
    /// Superseded cases, which are not run (TV-021).
    pub superseded: usize,
    /// Cases that failed.
    pub failures: Vec<Failure>,
}

impl Report {
    /// Panics with a readable summary if any case failed or no case ran.
    ///
    /// # Panics
    /// On any failure, or if the file contained no runnable case.
    pub fn assert_passed(&self) {
        let mut msg = String::new();
        for f in &self.failures {
            let _ = writeln!(msg, "  {}: {}", f.case_id, f.message);
        }
        assert!(
            self.failures.is_empty(),
            "{}: {} of {} cases failed\n{msg}",
            self.file.display(),
            self.failures.len(),
            self.failures.len() + self.passed
        );
        assert!(
            self.passed > 0,
            "{}: no runnable cases",
            self.file.display()
        );
    }
}

/// Compares one handler outcome with a case's expectation.
///
/// # Errors
/// Returns a description of the first difference.
pub fn compare(case: &CodecCase, outcome: &Outcome) -> Result<(), String> {
    let exp = &case.expect;
    match (exp.ok, outcome) {
        (_, Outcome::Unsupported) => Err(format!(
            "operation {:?} not supported by handler",
            case.operation
        )),
        (true, Outcome::Ok(actual)) => {
            if *actual == exp.values {
                return Ok(());
            }
            let mut diffs = Vec::new();
            for (k, v) in &exp.values {
                match actual.get(k) {
                    None => diffs.push(format!("missing {k} (expected {v})")),
                    Some(a) if a != v => diffs.push(format!("{k}: expected {v}, got {a}")),
                    Some(_) => {}
                }
            }
            for (k, a) in actual {
                if !exp.values.contains_key(k) {
                    diffs.push(format!("unexpected output {k} = {a}"));
                }
            }
            Err(diffs.join("; "))
        }
        (true, Outcome::Err(kind)) => Err(format!("expected success, got error {kind}")),
        (false, Outcome::Ok(actual)) => Err(format!(
            "expected error {}, got success {}",
            exp.error.as_deref().unwrap_or("?"),
            Value::Object(actual.clone())
        )),
        (false, Outcome::Err(kind)) => match exp.error.as_deref() {
            Some(e) if e == kind => Ok(()),
            Some(e) => Err(format!("expected error {e}, got {kind}")),
            None => Err("vector expects failure but names no error kind".to_owned()),
        },
    }
}

/// Runs every non-superseded case of `file` through `handler`.
pub fn run_codec(
    file: &CodecFile,
    path: &Path,
    mut handler: impl FnMut(&CodecCase) -> Outcome,
) -> Report {
    let mut report = Report {
        file: path.to_path_buf(),
        ..Report::default()
    };
    for case in &file.cases {
        if case.superseded.is_some() {
            report.superseded += 1;
            continue;
        }
        match compare(case, &handler(case)) {
            Ok(()) => report.passed += 1,
            Err(message) => report.failures.push(Failure {
                case_id: case.id.clone(),
                message,
            }),
        }
    }
    report
}

/// Loads `test-vectors/<relative>` and runs it as a codec file.
///
/// # Panics
/// If the file cannot be read or parsed, or is not a codec file.
pub fn run_codec_file(relative: &str, handler: impl FnMut(&CodecCase) -> Outcome) -> Report {
    let path = crate::vectors_dir().join(relative);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    match VectorFile::from_json(&text) {
        Ok(VectorFile::Codec(file)) => run_codec(&file, &path, handler),
        Ok(VectorFile::EngineScenario(_)) => panic!("{}: expected a codec file", path.display()),
        Err(e) => panic!("{}: {e}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::{Outcome, compare, run_codec};
    use crate::model::{CodecFile, VectorFile};
    use serde_json::{Map, Value, json};
    use std::path::Path;

    fn file(cases: &Value) -> CodecFile {
        let v = json!({ "kind": "codec", "standard": "test", "provenance": "hand-derived",
                        "provenance_note": "unit test", "cases": cases });
        match VectorFile::from_json(&v.to_string()).unwrap() {
            VectorFile::Codec(f) => f,
            VectorFile::EngineScenario(_) => unreachable!(),
        }
    }

    fn obj(v: &Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn exact_match_passes_and_any_difference_fails() {
        // req: TV-002, TV-012
        let f = file(
            &json!([{ "id": "c1", "requirements": ["TV-002"], "operation": "op",
                               "expect": { "ok": true, "a": 1, "b": "x" } }]),
        );
        let case = &f.cases[0];
        assert!(compare(case, &Outcome::Ok(obj(&json!({ "a": 1, "b": "x" })))).is_ok());
        assert!(compare(case, &Outcome::Ok(obj(&json!({ "a": 2, "b": "x" })))).is_err());
        assert!(compare(case, &Outcome::Ok(obj(&json!({ "a": 1 })))).is_err());
        let extra = compare(
            case,
            &Outcome::Ok(obj(&json!({ "a": 1, "b": "x", "c": 0 }))),
        );
        assert!(extra.unwrap_err().contains("unexpected output c"));
        assert!(compare(case, &Outcome::Unsupported).is_err());
        assert!(compare(case, &Outcome::Err("CrcMismatch".into())).is_err());
    }

    #[test]
    fn error_kinds_must_match() {
        // req: TV-001, TV-012
        let f = file(
            &json!([{ "id": "e1", "requirements": ["TV-001"], "operation": "op",
                               "expect": { "ok": false, "error": "CrcMismatch" } }]),
        );
        let case = &f.cases[0];
        assert!(compare(case, &Outcome::Err("CrcMismatch".into())).is_ok());
        assert!(compare(case, &Outcome::Err("LengthMismatch".into())).is_err());
        assert!(compare(case, &Outcome::Ok(Map::new())).is_err());
    }

    #[test]
    fn superseded_cases_are_skipped_and_counted() {
        // req: TV-021
        let f = file(&json!([
            { "id": "old", "requirements": ["TV-021"], "operation": "op", "expect": { "ok": true },
              "superseded": { "reason": "wrong expected value", "by": "new" } },
            { "id": "new", "requirements": ["TV-021"], "operation": "op", "expect": { "ok": true } }
        ]));
        let r = run_codec(&f, Path::new("t.json"), |_| Outcome::Ok(Map::new()));
        assert_eq!((r.passed, r.superseded, r.failures.len()), (1, 1, 0));
    }

    #[test]
    fn unknown_fields_are_rejected() {
        // req: TV-001
        let bad = json!({ "kind": "codec", "standard": "t", "provenance": "hand-derived",
                          "cases": [], "typo_field": 1 });
        assert!(VectorFile::from_json(&bad.to_string()).is_err());
        let bad_case = json!({ "kind": "codec", "standard": "t", "provenance": "hand-derived",
                               "cases": [{ "id": "x", "requirements": [], "operation": "op",
                                           "expect": { "ok": true }, "inputhex": "00" }] });
        assert!(VectorFile::from_json(&bad_case.to_string()).is_err());
        assert!(VectorFile::from_json(r#"{ "kind": "bogus" }"#).is_err());
    }
}
