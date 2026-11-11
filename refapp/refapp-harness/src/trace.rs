//! Golden traces: writer, canonical-form check, and comparator (docs/reference-application.md §7).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use aldis::spp::SpacePacket;
use serde_json::{Map, Value, json};

use crate::frame::{FramedPackets, decode_frame};
use crate::micros::Micros;
use crate::scenario::Scenario;

/// Which node a record belongs to (§7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
    (100, "frame_gap"),
    (101, "frame_discontinuity"),
    (102, "frame_rejected"),
    (103, "frame_discarded"),
    (104, "clcw_changed"),
    (105, "partial_discarded"),
    (106, "extraction_error"),
    (110, "sequence_violation"),
    (111, "unknown_map"),
    (112, "sdu_too_long"),
    (113, "downlink_refused"),
    (114, "uplink_refused"),
    (200, "fop1_alert"),
    (201, "fop1_transfer"),
    (202, "fop1_directive"),
    (203, "fop1_state"),
    (210, "farm1_state"),
    (211, "farm1_unlock"),
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
        t_us: Micros,
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
        t_us: Micros,
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
        t_us: Micros,
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
        t_us: Micros,
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
        t_us: Micros,
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
        t_us: Micros,
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
    pub const fn t_us(&self) -> Micros {
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

/// What [`decode_strict`] decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Decoded {
    /// `unit_sent`, `packet_out`, and `packet_in` records decoded.
    pub records: usize,
    /// Of those, the `unit_sent` records decoded as frames.
    pub frames: usize,
    /// The packets found inside the frames and decoded.
    pub framed_packets: usize,
}

/// Decodes the octets of every `unit_sent`, `packet_out`, and `packet_in` record of `text`, a
/// canonical trace, strictly (APP-086, API-030).
///
/// `packet_out` and `packet_in` octets must be a strict Space Packet that fills them exactly. A
/// `unit_sent` is a Space Packet in a case of [`Level::Packets`](crate::scenario::Level::Packets)
/// and a TM frame (`down`) or TC frame (`up`) in a case of
/// [`Level::Frames`](crate::scenario::Level::Frames) or `cop1`
/// ([`crate::frame::decode_frame`]); the level is that of the case the header names, in
/// `scenario`. In a frame-level case the frames of each direction also go, in trace order as a
/// clean link, through the engines of the nodes ([`FramedPackets`]: the `PacketExtractor` of each
/// downlink VC, the `MapReassembler` of the uplink), and every complete packet they hand out is
/// decoded strictly too. Returns what was
/// decoded.
///
/// # Errors
/// If `text` is not canonical (APP-080), its case is not in `scenario`, or the first record
/// whose octets do not decode, with its 1-based line number.
pub fn decode_strict(scenario: &Scenario, text: &str) -> Result<Decoded, String> {
    check_canonical(text)?;
    let mut lines = text.split_terminator('\n');
    let header: Value = serde_json::from_str(lines.next().unwrap_or_default())
        .map_err(|e| format!("line 1: {e}"))?;
    let case_id = header["case"]
        .as_str()
        .ok_or_else(|| "line 1: header without a case".to_owned())?;
    let level = scenario.case(case_id)?.level;
    let mut decoded = Decoded::default();
    let mut framed = FramedPackets::new(scenario)?;
    // The uplink units as sent, by index, the indices a fault dropped or corrupted, and whether
    // any was.
    let mut sent_up: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
    let mut corrupted_up: BTreeSet<u64> = BTreeSet::new();
    let mut up_loss = false;
    let direction = |v: &Value| match v["dir"].as_str() {
        Some("down") => Some(Dir::Down),
        Some("up") => Some(Dir::Up),
        _ => None,
    };
    for (i, l) in lines.enumerate() {
        let at = |e: String| format!("line {}: {e}", i + 2);
        let v: Value = serde_json::from_str(l).map_err(|e| at(e.to_string()))?;
        let kind = v["kind"].as_str().unwrap_or_default();
        if kind == "fault" && level.has_frames() {
            if direction(&v) == Some(Dir::Up)
                && matches!(v["fault"].as_str(), Some("drop" | "flip" | "truncate"))
            {
                up_loss = true;
                if let Some(index) = v["index"].as_u64() {
                    corrupted_up.insert(index);
                }
            }
            continue;
        }
        if kind == "unit_delivered" && level.has_frames() {
            // The reassembler sees the frames as received (APP-146): a unit that FARM-1 discards,
            // as a retransmitted or reordered Type-AD frame, gives no packet. A corrupted unit
            // fails the FECF (APP-139) and reaches no engine but FARM-1's invalid-frame input.
            let index = v["index"]
                .as_u64()
                .ok_or_else(|| at("unit_delivered record without an index".to_owned()))?;
            if direction(&v) == Some(Dir::Up) && !corrupted_up.contains(&index) {
                let octets = sent_up
                    .get(&index)
                    .ok_or_else(|| at(format!("unit {index} delivered but never sent")))?;
                let now = v["t_us"]
                    .as_u64()
                    .ok_or_else(|| at("record without t_us".to_owned()))?;
                decoded.framed_packets += framed.receive_up(now, octets, up_loss).map_err(at)?;
            }
            continue;
        }
        if !matches!(kind, "unit_sent" | "packet_out" | "packet_in") {
            continue;
        }
        let hex = v["hex"]
            .as_str()
            .ok_or_else(|| at(format!("{kind} record without hex")))?;
        let octets = unhex(hex).map_err(at)?;
        if kind == "unit_sent" && level.has_frames() {
            let dir = direction(&v)
                .ok_or_else(|| at("unit_sent record without a direction".to_owned()))?;
            let frame = decode_frame(scenario, level, dir, &octets).map_err(at)?;
            decoded.frames += 1;
            match dir {
                Dir::Down => decoded.framed_packets += framed.take_down(&frame).map_err(at)?,
                Dir::Up => {
                    let index = v["index"]
                        .as_u64()
                        .ok_or_else(|| at("unit_sent record without an index".to_owned()))?;
                    sent_up.insert(index, octets);
                }
            }
        } else {
            let packet = SpacePacket::decode(&octets)
                .map_err(|e| at(format!("{kind} is not a strict Space Packet: {e:?}")))?;
            if packet.len() != octets.len() {
                return Err(at(format!(
                    "{kind} has {} octet(s) after its packet of {}",
                    octets.len() - packet.len(),
                    packet.len()
                )));
            }
        }
        decoded.records += 1;
    }
    Ok(decoded)
}

fn unhex(s: &str) -> Result<Vec<u8>, String> {
    let (pairs, rest) = s.as_bytes().as_chunks::<2>();
    if !rest.is_empty() {
        return Err("odd number of hex digits".to_owned());
    }
    pairs
        .iter()
        .map(|pair| {
            let text = std::str::from_utf8(pair).map_err(|e| e.to_string())?;
            u8::from_str_radix(text, 16).map_err(|e| e.to_string())
        })
        .collect()
}

/// Whether a projected packet was generated or delivered (§7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PacketRecord {
    /// A `packet_out` record.
    Out,
    /// A `packet_in` record.
    In,
}

/// The packet projection of a trace (APP-003).
///
/// For each record kind, node, and APID, the packet octets in trace order, as the uppercase
/// hexadecimal of the trace. Order across APIDs and the instants are not part of it.
pub type Projection = BTreeMap<(PacketRecord, NodeId, u16), Vec<String>>;

/// The packet projection of `text`, a canonical trace (APP-003, §7.3).
///
/// # Errors
/// If `text` is not a canonical trace (APP-080).
pub fn packet_projection(text: &str) -> Result<Projection, String> {
    check_canonical(text)?;
    let mut projection = Projection::new();
    for l in text.split_terminator('\n') {
        let v: Value = serde_json::from_str(l).map_err(|e| e.to_string())?;
        let record = match v["kind"].as_str() {
            Some("packet_out") => PacketRecord::Out,
            Some("packet_in") => PacketRecord::In,
            _ => continue,
        };
        let node = match v["node"].as_str() {
            Some("flight") => NodeId::Flight,
            Some("ground") => NodeId::Ground,
            other => return Err(format!("unknown node {other:?}")),
        };
        let apid = v["apid"]
            .as_u64()
            .and_then(|a| u16::try_from(a).ok())
            .ok_or("packet record without a valid apid")?;
        let octets = v["hex"].as_str().ok_or("packet record without hex")?;
        projection
            .entry((record, node, apid))
            .or_default()
            .push(octets.to_owned());
    }
    Ok(projection)
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
