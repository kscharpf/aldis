//! The evidence record of a verification run (VER-057) and the merge-gate rule (VER-058).
//!
//! A record is canonical JSON: object keys sorted, no insignificant whitespace, UTF-8. Its
//! SHA-256 is taken over exactly those bytes, so anyone can recompute it from the git note.

use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sha2::{Digest as _, Sha256};

use super::checks::Host;

/// The `format` value of every record this version of the tool writes: with the `scope` of the
/// input rule (VER-056). A tool that knows only format 1 refuses it rather than accept its skips.
pub const FORMAT: &str = "aldis-verify-record/2";

/// The format of the records written before the input rule, with only the C++ scope `cpp`.
/// Still read: a skip in one is accepted for a C++ check whose scope says C++ was not affected.
pub const FORMAT_1: &str = "aldis-verify-record/1";

/// Result of one check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Ran and succeeded.
    Pass,
    /// Ran and failed.
    Fail,
    /// Not run: host-bound to a host this is not, or not selected with `--only`.
    NotRun,
    /// Not run: none of the check's inputs changed against the base (VER-056).
    Skipped,
}

impl Outcome {
    /// The name recorded in the evidence record.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::NotRun => "not-run",
            Self::Skipped => "skipped",
        }
    }
}

/// Lowercase hex of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(2 * bytes.len()), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// Current UTC time as RFC 3339 with whole seconds, e.g. `2026-10-19T16:02:34Z`.
pub fn utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    utc(secs)
}

/// RFC 3339 form of a Unix time (civil-from-days, H. Hinnant's algorithm).
fn utc(secs: u64) -> String {
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// Canonical JSON text of `value`: keys sorted, no whitespace.
pub fn canonical(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                if let Some(v) = map.get(key) {
                    write_canonical(v, out);
                }
            }
            out.push('}');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// Whether `record`'s own scope skips the check `name`: in format 2, its decision in `scope`,
/// for a base covered by a record; in format 1, a C++ check when C++ was not affected.
fn skip_recorded(record: &Value, name: &str) -> bool {
    match record.get("format").and_then(Value::as_str) {
        Some(FORMAT) => {
            let scope = record.get("scope");
            let field = |k: &str| scope.and_then(|s| s.get(k));
            field("base").is_some_and(Value::is_string)
                && field("base_record").is_some_and(Value::is_string)
                && field("checks")
                    .and_then(|c| c.get(name))
                    .and_then(|d| d.get("run"))
                    == Some(&Value::Bool(false))
        }
        Some(FORMAT_1) => {
            let cpp = record.get("cpp");
            name.starts_with("cpp-")
                && cpp.and_then(|c| c.get("affected")) == Some(&Value::Bool(false))
                && cpp
                    .and_then(|c| c.get("base"))
                    .is_some_and(Value::is_string)
        }
        _ => false,
    }
}

/// Why `record` does not meet the merge gate (VER-058); empty when it does. Checks the record
/// only: that it belongs to the commit under review, and that its skips hold in git
/// (`scope::recheck`), is the caller's part.
pub fn gate_problems(record: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    let format = record.get("format").and_then(Value::as_str);
    if format != Some(FORMAT) && format != Some(FORMAT_1) {
        problems.push(format!("not a `{FORMAT}` or `{FORMAT_1}` record"));
    }
    let Some(checks) = record.get("checks").and_then(Value::as_array) else {
        problems.push("no checks".to_owned());
        return problems;
    };
    if checks.is_empty() {
        problems.push("no checks".to_owned());
    }
    for c in checks {
        let field = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or("?");
        let gating = c.get("gating").and_then(Value::as_bool).unwrap_or(true);
        let (name, result) = (field("name"), field("result"));
        if !gating {
            continue;
        }
        match result {
            "pass" => {}
            "skipped" if skip_recorded(record, name) => {}
            "skipped" => problems.push(format!("{name}: skipped, but the record's scope runs it")),
            // Only a portable check must run; a host-bound one may be left to the nightly runs.
            "not-run" if field("host") != Host::Container.name() => {}
            "not-run" => problems.push(format!("{name}: portable check not run")),
            _ => problems.push(format!("{name}: {result}")),
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // req: VER-057
    #[test]
    fn canonical_sorts_keys_without_whitespace() {
        let v = json!({"b": [1, {"z": null, "a": "x\"y"}], "a": true});
        assert_eq!(canonical(&v), r#"{"a":true,"b":[1,{"a":"x\"y","z":null}]}"#);
    }

    // req: VER-057
    #[test]
    fn sha256_of_abc() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    // req: VER-057
    #[test]
    fn utc_dates() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc(1_792_425_754), "2026-10-19T16:02:34Z");
    }

    fn record(checks: &Value) -> Value {
        json!({"format": FORMAT, "checks": checks, "scope": {
            "base": "b", "base_record": "c", "checks": {"cpp-tidy": {"run": false}, "fmt": {"run": true}},
        }})
    }

    // req: VER-058
    #[test]
    fn gate() {
        let ok = record(&json!([
            {"name": "fmt", "host": "container", "gating": true, "result": "pass"},
            {"name": "cpp-tidy", "host": "container", "gating": true, "result": "skipped"},
            {"name": "test-macos", "host": "macos", "gating": true, "result": "not-run"},
            {"name": "cpp-leon3", "host": "container-x86_64", "gating": false, "result": "fail"},
        ]));
        assert_eq!(gate_problems(&ok), Vec::<String>::new());

        // A skip needs the record's scope to skip that check, for a covered base.
        let mut forged = record(&json!([
            {"name": "fmt", "host": "container", "gating": true, "result": "skipped"},
            {"name": "cpp-tidy", "host": "container", "gating": true, "result": "skipped"},
        ]));
        assert_eq!(
            gate_problems(&forged),
            ["fmt: skipped, but the record's scope runs it"]
        );
        forged["scope"]["base_record"] = Value::Null;
        assert_eq!(gate_problems(&forged).len(), 2);

        // Format 1: a C++ check skipped by the C++ scope.
        let old = |affected: bool, name: &str| {
            json!({"format": FORMAT_1, "cpp": {"affected": affected, "base": "b"}, "checks": [
                {"name": name, "host": "container", "gating": true, "result": "skipped"},
            ]})
        };
        assert_eq!(gate_problems(&old(false, "cpp-tidy")), Vec::<String>::new());
        assert_eq!(gate_problems(&old(true, "cpp-tidy")).len(), 1);
        assert_eq!(gate_problems(&old(false, "clippy")).len(), 1);
        // A record of --only never passes: its portable checks are not run.
        let only = record(&json!([
            {"name": "fmt", "host": "container", "gating": true, "result": "pass"},
            {"name": "clippy", "host": "container", "gating": true, "result": "not-run"},
        ]));
        assert_eq!(gate_problems(&only), ["clippy: portable check not run"]);

        let bad = record(&json!([
            {"name": "fmt", "host": "container", "gating": true, "result": "fail"},
            {"name": "spec", "host": "container", "gating": true, "result": "not-run"},
            {"name": "test-macos", "host": "macos", "gating": true, "result": "fail"},
        ]));
        assert_eq!(
            gate_problems(&bad),
            [
                "fmt: fail",
                "spec: portable check not run",
                "test-macos: fail"
            ]
        );
        assert_eq!(gate_problems(&record(&json!([]))), ["no checks"]);
        assert_eq!(
            gate_problems(
                &json!({"checks": [{"name": "fmt", "host": "container", "gating": true, "result": "pass"}]})
            ),
            ["not a `aldis-verify-record/2` or `aldis-verify-record/1` record"]
        );
    }
}
