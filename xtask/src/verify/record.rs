//! The evidence record of a verification run (VER-057) and the merge-gate rule (VER-058).
//!
//! A record is canonical JSON: object keys sorted, no insignificant whitespace, UTF-8. Its
//! SHA-256 is taken over exactly those bytes, so anyone can recompute it from the git note.

use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sha2::{Digest as _, Sha256};

use super::checks::Host;

/// The `format` value of every record this version of the tool writes: a check that did not run
/// names the record it takes its result from (`source`), and may be `reused` from an earlier
/// commit of the branch (VER-056, VER-057). A tool that knows only formats 1 and 2 refuses it
/// rather than accept its reuses.
pub const FORMAT: &str = "aldis-verify-record/3";

/// The format of the records written with the input rule before reuse: with the `scope` of the
/// rule, a skip measured against the base and the record covering it. Still read.
pub const FORMAT_2: &str = "aldis-verify-record/2";

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
    /// Not run: none of the check's inputs changed against the base, and its result is taken
    /// from a record in which it passed with the same inputs (VER-056).
    Skipped,
    /// Not run: its inputs changed against the base, but are those of an earlier commit in
    /// whose record it passed, and its result is taken from that record (VER-056).
    Reused,
}

impl Outcome {
    /// The name recorded in the evidence record.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::NotRun => "not-run",
            Self::Skipped => "skipped",
            Self::Reused => "reused",
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
pub fn utc(secs: u64) -> String {
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

/// Whether `record`'s own scope leaves out the check `name` of entry `entry` with `result`
/// (`skipped` or `reused`): in format 3, its decision in `scope` and the source the entry names,
/// and for a skip the base; in format 2, a skip decided in `scope` for a base covered by a record;
/// in format 1, a skipped C++ check when C++ was not affected.
fn not_run_recorded(record: &Value, entry: &Value, name: &str, result: &str) -> bool {
    let scope = record.get("scope");
    let field = |k: &str| scope.and_then(|s| s.get(k));
    let decided = || {
        field("checks")
            .and_then(|c| c.get(name))
            .and_then(|d| d.get("run"))
            == Some(&Value::Bool(false))
    };
    match record.get("format").and_then(Value::as_str) {
        Some(FORMAT) => {
            let source = entry.get("source");
            let named = |k: &str| source.and_then(|s| s.get(k)).is_some_and(Value::is_string);
            decided()
                && named("commit")
                && named("record")
                && (result == "reused" || field("base").is_some_and(Value::is_string))
        }
        Some(FORMAT_2) => {
            result == "skipped"
                && field("base").is_some_and(Value::is_string)
                && field("base_record").is_some_and(Value::is_string)
                && decided()
        }
        Some(FORMAT_1) => {
            let cpp = record.get("cpp");
            result == "skipped"
                && name.starts_with("cpp-")
                && cpp.and_then(|c| c.get("affected")) == Some(&Value::Bool(false))
                && cpp
                    .and_then(|c| c.get("base"))
                    .is_some_and(Value::is_string)
        }
        _ => false,
    }
}

/// Why `record` does not meet the merge gate (VER-058); empty when it does. Checks the record
/// only: that it belongs to the commit under review, and that its skips and reuses hold in git
/// (`scope::recheck`), is the caller's part.
pub fn gate_problems(record: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    let format = record.get("format").and_then(Value::as_str);
    if ![FORMAT, FORMAT_2, FORMAT_1].map(Some).contains(&format) {
        problems.push(format!(
            "not a `{FORMAT}`, `{FORMAT_2}`, or `{FORMAT_1}` record"
        ));
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
            "skipped" | "reused" if not_run_recorded(record, c, name, result) => {}
            "skipped" | "reused" => problems.push(format!(
                "{name}: {result}, but the record's scope runs it or names no source"
            )),
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
        json!({"format": FORMAT_2, "checks": checks, "scope": {
            "base": "b", "base_record": "c", "checks": {"cpp-tidy": {"run": false}, "fmt": {"run": true}},
        }})
    }

    // req: VER-057, VER-058
    #[test]
    fn gate_of_format_3() {
        let source = json!({"commit": "c", "record": "d"});
        let rec = |checks: &Value| {
            json!({"format": FORMAT, "checks": checks, "scope": {"base": "b", "checks": {
                "fmt": {"run": true}, "clippy": {"run": false}, "cpp-tidy": {"run": false},
            }}})
        };
        let ok = rec(&json!([
            {"name": "fmt", "host": "container", "gating": true, "result": "pass"},
            {"name": "clippy", "host": "container", "gating": true, "result": "reused", "source": source},
            {"name": "cpp-tidy", "host": "container", "gating": true, "result": "skipped", "source": source},
        ]));
        assert_eq!(gate_problems(&ok), Vec::<String>::new());
        // A check that did not run names its source, and the scope decided not to run it.
        let mut bad = ok.clone();
        bad["checks"][1]["source"] = Value::Null;
        bad["checks"][2]["source"]["record"] = json!(7);
        bad["checks"][0]["result"] = json!("reused");
        bad["checks"][0]["source"] = source.clone();
        assert_eq!(gate_problems(&bad).len(), 3);
        let mut ran = ok.clone();
        ran["scope"]["checks"]["clippy"]["run"] = json!(true);
        assert_eq!(
            gate_problems(&ran),
            ["clippy: reused, but the record's scope runs it or names no source"]
        );
        // A skip is measured against a base.
        let mut no_base = ok;
        no_base["scope"]["base"] = Value::Null;
        assert_eq!(
            gate_problems(&no_base),
            ["cpp-tidy: skipped, but the record's scope runs it or names no source"]
        );
        // Format 2 knows no reuse.
        let two = record(&json!([
            {"name": "cpp-tidy", "host": "container", "gating": true, "result": "reused", "source": source},
        ]));
        assert_eq!(gate_problems(&two).len(), 1);
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
            ["fmt: skipped, but the record's scope runs it or names no source"]
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
            [
                "not a `aldis-verify-record/3`, `aldis-verify-record/2`, or \
                 `aldis-verify-record/1` record"
            ]
        );
    }
}
