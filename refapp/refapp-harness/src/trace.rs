//! Golden traces: writer, canonical-form check, and comparator (docs/reference-application.md §7).

use std::fmt;

use serde_json::{Map, Value, json};

/// Which node a record belongs to (§7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeId {
    /// The flight node.
    Flight,
    /// The ground node.
    Ground,
}

impl NodeId {
    /// The trace spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            NodeId::Flight => "flight",
            NodeId::Ground => "ground",
        }
    }
}

/// Link direction (§7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dir {
    /// Flight to ground. Sorts first in the tie order of §6.2.
    Down,
    /// Ground to flight.
    Up,
}

impl Dir {
    /// The trace spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Dir::Down => "down",
            Dir::Up => "up",
        }
    }
}

/// Fault kinds (§6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    /// The unit is not delivered.
    Drop,
    /// The unit is delivered twice.
    Duplicate,
    /// The unit arrives later.
    Delay,
    /// One bit is inverted.
    Flip,
    /// The unit is cut short.
    Truncate,
}

impl FaultKind {
    /// The trace spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            FaultKind::Drop => "drop",
            FaultKind::Duplicate => "duplicate",
            FaultKind::Delay => "delay",
            FaultKind::Flip => "flip",
            FaultKind::Truncate => "truncate",
        }
    }

    /// Number of arguments the fault takes (§6.3).
    #[must_use]
    pub const fn arity(self) -> usize {
        match self {
            FaultKind::Drop | FaultKind::Duplicate => 0,
            FaultKind::Delay | FaultKind::Truncate => 1,
            FaultKind::Flip => 2,
        }
    }

    const ALL: [FaultKind; 5] = [
        FaultKind::Drop,
        FaultKind::Duplicate,
        FaultKind::Delay,
        FaultKind::Flip,
        FaultKind::Truncate,
    ];
}

/// Application event vocabulary (§7.4, APP-083): code and name.
pub const APP_EVENTS: &[(u16, &str)] = &[
    (1, "seq_gap"),
    (2, "seq_repeat"),
    (3, "seq_reset"),
    (4, "dump_reassembled"),
    (5, "dump_discarded"),
    (6, "packet_rejected"),
    (20, "cmd_received"),
    (21, "table_received"),
];

/// Name of an application event code, if the code is in the vocabulary.
#[must_use]
pub fn app_event_name(code: u16) -> Option<&'static str> {
    APP_EVENTS
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, name)| *name)
}

/// The header line (§7.2, APP-081).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Scenario identifier.
    pub scenario: String,
    /// Scenario version.
    pub scenario_version: u16,
    /// Case ID.
    pub case: String,
    /// Trace version.
    pub trace_version: u32,
    /// Requirement IDs the case verifies, sorted.
    pub requirements: Vec<String>,
}

impl Header {
    /// The canonical line, LF included.
    #[must_use]
    pub fn to_line(&self) -> String {
        line(&json!({
            "kind": "header",
            "scenario": self.scenario,
            "scenario_version": self.scenario_version,
            "case": self.case,
            "trace_version": self.trace_version,
            "requirements": self.requirements,
        }))
    }
}

/// One record after the header (§7.3, APP-082).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    /// An application generated a packet.
    PacketOut {
        /// Instant, µs.
        t_us: u64,
        /// Node.
        node: NodeId,
        /// APID.
        apid: u16,
        /// Packet octets.
        octets: Vec<u8>,
    },
    /// A fault is applied to a unit.
    Fault {
        /// Instant, µs.
        t_us: u64,
        /// Direction.
        dir: Dir,
        /// Unit index in that direction.
        index: u64,
        /// Fault kind.
        fault: FaultKind,
        /// Fault arguments.
        args: Vec<u64>,
    },
    /// A node sent a link unit.
    UnitSent {
        /// Instant, µs.
        t_us: u64,
        /// Direction.
        dir: Dir,
        /// Unit index in that direction.
        index: u64,
        /// Unit octets as sent.
        octets: Vec<u8>,
    },
    /// A unit arrived.
    UnitDelivered {
        /// Instant, µs.
        t_us: u64,
        /// Direction.
        dir: Dir,
        /// Unit index in that direction.
        index: u64,
        /// 0, or 1 for the second copy of a duplicate.
        copy: u8,
    },
    /// A packet was delivered to an application.
    PacketIn {
        /// Instant, µs.
        t_us: u64,
        /// Node.
        node: NodeId,
        /// APID.
        apid: u16,
        /// Packet octets.
        octets: Vec<u8>,
    },
    /// A node raised an application event.
    AppEvent {
        /// Instant, µs.
        t_us: u64,
        /// Node.
        node: NodeId,
        /// Code of §7.4.
        code: u16,
        /// Arguments.
        args: Vec<u64>,
    },
}

/// Octets as uppercase hexadecimal (§7.1).
#[must_use]
pub fn hex(octets: &[u8]) -> String {
    use fmt::Write as _;
    let mut s = String::with_capacity(octets.len() * 2);
    for b in octets {
        let _ = write!(s, "{b:02X}");
    }
    s
}

fn line(v: &Value) -> String {
    let mut s = v.to_string();
    s.push('\n');
    s
}

impl Record {
    /// The record's instant.
    #[must_use]
    pub const fn t_us(&self) -> u64 {
        match self {
            Record::PacketOut { t_us, .. }
            | Record::Fault { t_us, .. }
            | Record::UnitSent { t_us, .. }
            | Record::UnitDelivered { t_us, .. }
            | Record::PacketIn { t_us, .. }
            | Record::AppEvent { t_us, .. } => *t_us,
        }
    }

    /// The canonical line, LF included.
    ///
    /// # Errors
    /// If an application event code is not in the vocabulary (APP-083).
    pub fn to_line(&self) -> Result<String, String> {
        let v = match self {
            Record::PacketOut {
                t_us,
                node,
                apid,
                octets,
            } => json!({ "t_us": t_us, "kind": "packet_out", "node": node.as_str(),
                         "apid": apid, "hex": hex(octets) }),
            Record::Fault {
                t_us,
                dir,
                index,
                fault,
                args,
            } => json!({ "t_us": t_us, "kind": "fault", "dir": dir.as_str(), "index": index,
                         "fault": fault.as_str(), "args": args }),
            Record::UnitSent {
                t_us,
                dir,
                index,
                octets,
            } => json!({ "t_us": t_us, "kind": "unit_sent", "dir": dir.as_str(), "index": index,
                         "hex": hex(octets) }),
            Record::UnitDelivered {
                t_us,
                dir,
                index,
                copy,
            } => json!({ "t_us": t_us, "kind": "unit_delivered", "dir": dir.as_str(),
                         "index": index, "copy": copy }),
            Record::PacketIn {
                t_us,
                node,
                apid,
                octets,
            } => json!({ "t_us": t_us, "kind": "packet_in", "node": node.as_str(),
                         "apid": apid, "hex": hex(octets) }),
            Record::AppEvent {
                t_us,
                node,
                code,
                args,
            } => {
                let name = app_event_name(*code)
                    .ok_or_else(|| format!("application event code {code} is not in §7.4"))?;
                json!({ "t_us": t_us, "kind": "app_event", "node": node.as_str(), "code": code,
                        "name": name, "args": args })
            }
        };
        Ok(line(&v))
    }
}

/// Keys of each line kind, in canonical order (§7.2, §7.3).
fn expected_keys(kind: &str) -> Option<&'static [&'static str]> {
    Some(match kind {
        "header" => &[
            "kind",
            "scenario",
            "scenario_version",
            "case",
            "trace_version",
            "requirements",
        ],
        "packet_out" | "packet_in" => &["t_us", "kind", "node", "apid", "hex"],
        "fault" => &["t_us", "kind", "dir", "index", "fault", "args"],
        "unit_sent" => &["t_us", "kind", "dir", "index", "hex"],
        "unit_delivered" => &["t_us", "kind", "dir", "index", "copy"],
        "app_event" => &["t_us", "kind", "node", "code", "name", "args"],
        _ => return None,
    })
}

fn uint(m: &Map<String, Value>, key: &str, max: u64) -> Result<u64, String> {
    m.get(key)
        .and_then(Value::as_u64)
        .filter(|v| *v <= max)
        .ok_or_else(|| format!("`{key}` must be an unsigned integer ≤ {max}"))
}

fn one_of<'a>(m: &'a Map<String, Value>, key: &str, allowed: &[&str]) -> Result<&'a str, String> {
    m.get(key)
        .and_then(Value::as_str)
        .filter(|s| allowed.contains(s))
        .ok_or_else(|| format!("`{key}` must be one of {allowed:?}"))
}

fn uint_list(m: &Map<String, Value>, key: &str) -> Result<usize, String> {
    let list = m
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("`{key}` must be an array"))?;
    if list.iter().all(|v| v.as_u64().is_some()) {
        Ok(list.len())
    } else {
        Err(format!("`{key}` must hold unsigned integers"))
    }
}

fn check_hex(m: &Map<String, Value>) -> Result<(), String> {
    let s = m.get("hex").and_then(Value::as_str).unwrap_or_default();
    let ok = !s.is_empty()
        && s.len().is_multiple_of(2)
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b));
    if ok {
        Ok(())
    } else {
        Err("`hex` must be a non-empty string of uppercase hex digit pairs".to_owned())
    }
}

/// Checks one parsed line against §7.2–7.4 (keys, order, and value types).
fn check_value(v: &Value, first: bool) -> Result<Option<u64>, String> {
    let m = v.as_object().ok_or("not a JSON object")?;
    let kind = m
        .get("kind")
        .and_then(Value::as_str)
        .ok_or("no string `kind`")?;
    if first != (kind == "header") {
        return Err(if first {
            "line 1 must be the header".to_owned()
        } else {
            "only line 1 may be a header".to_owned()
        });
    }
    let keys = expected_keys(kind).ok_or_else(|| format!("unknown kind `{kind}`"))?;
    if !m.keys().map(String::as_str).eq(keys.iter().copied()) {
        return Err(format!(
            "keys of `{kind}` must be exactly {keys:?}, in that order"
        ));
    }
    if kind == "header" {
        one_of(m, "scenario", &["aldis-refapp"])?;
        uint(m, "scenario_version", u64::from(u16::MAX))?;
        uint(m, "trace_version", u64::from(u32::MAX))?;
        if !m.get("case").is_some_and(Value::is_string) {
            return Err("`case` must be a string".to_owned());
        }
        let reqs = m
            .get("requirements")
            .and_then(Value::as_array)
            .ok_or("`requirements` must be an array")?;
        if !reqs.iter().all(Value::is_string) {
            return Err("`requirements` must hold strings".to_owned());
        }
        return Ok(None);
    }
    let t = uint(m, "t_us", u64::MAX)?;
    match kind {
        "packet_out" | "packet_in" => {
            one_of(m, "node", &["flight", "ground"])?;
            uint(m, "apid", 2047)?;
            check_hex(m)?;
        }
        "unit_sent" => {
            one_of(m, "dir", &["down", "up"])?;
            uint(m, "index", u64::MAX)?;
            check_hex(m)?;
        }
        "unit_delivered" => {
            one_of(m, "dir", &["down", "up"])?;
            uint(m, "index", u64::MAX)?;
            uint(m, "copy", 1)?;
        }
        "fault" => {
            one_of(m, "dir", &["down", "up"])?;
            uint(m, "index", u64::MAX)?;
            let names: Vec<&str> = FaultKind::ALL.iter().map(|f| f.as_str()).collect();
            let name = one_of(m, "fault", &names)?;
            let arity = FaultKind::ALL
                .iter()
                .find(|f| f.as_str() == name)
                .map_or(0, |f| f.arity());
            if uint_list(m, "args")? != arity {
                return Err(format!("fault `{name}` takes {arity} argument(s)"));
            }
        }
        _ => {
            // app_event
            one_of(m, "node", &["flight", "ground"])?;
            let code = uint(m, "code", u64::from(u16::MAX))?;
            let expected = u16::try_from(code).ok().and_then(app_event_name);
            let name = m.get("name").and_then(Value::as_str);
            if expected.is_none() || name != expected {
                return Err(format!(
                    "app_event code {code} and name {name:?} do not match §7.4"
                ));
            }
            uint_list(m, "args")?;
        }
    }
    Ok(Some(t))
}

/// Checks that `text` is a trace in the canonical form of §7.1–7.4 (APP-080).
///
/// # Errors
/// The first problem found, with its 1-based line number.
pub fn check_canonical(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Err("empty trace: line 1 must be the header".to_owned());
    }
    if !text.ends_with('\n') {
        return Err("the last line must end with LF".to_owned());
    }
    let mut last_t = 0;
    for (i, l) in text.split_terminator('\n').enumerate() {
        let n = i + 1;
        let at = |e: String| format!("line {n}: {e}");
        let v: Value = serde_json::from_str(l).map_err(|e| at(e.to_string()))?;
        if let Some(t) = check_value(&v, n == 1).map_err(at)? {
            if t < last_t {
                return Err(at(format!("t_us {t} is earlier than {last_t}")));
            }
            last_t = t;
        }
        let canonical = v.to_string();
        if canonical != l {
            return Err(at(
                "not canonical (whitespace, number form, or string escapes)".to_owned(),
            ));
        }
    }
    Ok(())
}

/// The first difference between an expected and an actual trace (APP-085).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    /// 1-based line number of the first differing line.
    pub line: usize,
    /// The expected line, or `None` if the expected trace ended.
    pub expected: Option<String>,
    /// The actual line, or `None` if the actual trace ended.
    pub actual: Option<String>,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let show = |l: &Option<String>| l.clone().unwrap_or_else(|| "<end of trace>".to_owned());
        write!(
            f,
            "first difference at line {}\n  expected: {}\n  actual:   {}",
            self.line,
            show(&self.expected),
            show(&self.actual)
        )
    }
}

/// Compares two traces byte for byte (APP-085). Decoding the differing record with the strict
/// codecs is added with the first codecs the traces carry (A1).
///
/// # Errors
/// The first differing line.
pub fn compare(expected: &str, actual: &str) -> Result<(), Mismatch> {
    if expected == actual {
        return Ok(());
    }
    let mut e = expected.split_inclusive('\n');
    let mut a = actual.split_inclusive('\n');
    let mut line = 1;
    loop {
        match (e.next(), a.next()) {
            (Some(x), Some(y)) if x == y => line += 1,
            (x, y) => {
                return Err(Mismatch {
                    line,
                    expected: x.map(|s| s.trim_end_matches('\n').to_owned()),
                    actual: y.map(|s| s.trim_end_matches('\n').to_owned()),
                });
            }
        }
    }
}
