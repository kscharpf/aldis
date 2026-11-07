//! The scenario file `refapp/scenario/scenario.json` (docs/reference-application.md, APP-107).
//!
//! These types are the single source of truth for the file format: [`Scenario::load`]
//! deserializes the file with them (unknown fields are errors), [`schema`] publishes the JSON
//! Schema generated from them, and [`rust_constants`] generates `refapp-scenario`'s constants,
//! so the validator, the schema, and the generated code cannot drift apart.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::ranged::{EvenRanged, Ranged};
use refapp_scenario::{Crc16, Duration, TableId, TableLength};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The scenario file's text, compiled into the harness.
pub const SCENARIO_JSON: &str = include_str!("../../scenario/scenario.json");

/// The whole scenario (docs/reference-application.md).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    /// Scenario identifier, always `aldis-refapp` (§7.2).
    pub scenario: String,
    /// Scenario version (§7.2, APP-002).
    pub scenario_version: u16,
    /// Spacecraft ID of every frame (§3.4, APP-026); fictional (OI-26).
    pub spacecraft_id: Ranged<u16, 0, 1023>,
    /// Time model (§5).
    pub time: Time,
    /// Link model (§6.1).
    pub link: Link,
    /// APIDs (§3.1).
    pub apid: Apids,
    /// TM downlink (§4.1).
    pub tm: Tm,
    /// TC uplink (§4.2).
    pub tc: Tc,
    /// COP-1 parameters (§4.2).
    pub cop1: Cop1,
    /// Cases (§6.4).
    pub cases: Vec<Case>,
}

/// Time model (§5, APP-060).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Time {
    /// CUC coarse time of the run's start, seconds since 1958-01-01 TAI.
    pub t0_coarse_s: u32,
    /// CUC coarse octets.
    pub cuc_coarse_octets: Ranged<u8, 1, 4>,
    /// CUC fractional octets.
    pub cuc_fine_octets: Ranged<u8, 0, 3>,
    /// Explicit P-field octet (301.0 §3.2.2).
    pub cuc_explicit_pfield: u8,
}

/// Link model (§6.1, APP-071, APP-072).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Link {
    /// One-way delay, both directions, µs.
    pub one_way_delay_us: u64,
    /// Downlink frame slot period, µs (A2 on).
    pub frame_slot_us: Ranged<u64, 1, 18_446_744_073_709_551_615>,
    /// Uplink slot period, µs (A2 on).
    pub uplink_slot_us: Ranged<u64, 1, 18_446_744_073_709_551_615>,
}

/// APIDs (§3.1, APP-020).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Apids {
    /// Housekeeping.
    pub housekeeping: Ranged<u16, 0, 2046>,
    /// Event reports.
    pub events: Ranged<u16, 0, 2046>,
    /// Payload science.
    pub payload: Ranged<u16, 0, 2046>,
    /// Memory dump.
    pub dump: Ranged<u16, 0, 2046>,
    /// Commands.
    pub commands: Ranged<u16, 0, 2046>,
    /// Table upload.
    pub table_upload: Ranged<u16, 0, 2046>,
}

/// TM downlink (§4.1, APP-030…APP-034).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tm {
    /// Transfer frame length, octets.
    pub frame_length: Ranged<u16, 0, 2048>,
    /// FECF present.
    pub fecf: bool,
    /// VC of housekeeping and events.
    pub vcid_realtime: Ranged<u8, 0, 7>,
    /// VC of payload and dump.
    pub vcid_payload: Ranged<u8, 0, 7>,
    /// VC of OID frames.
    pub vcid_idle: Ranged<u8, 0, 7>,
    /// Release deadline of the real-time VC, µs.
    pub release_deadline_realtime_us: u64,
    /// Release deadline of the payload VC, µs.
    pub release_deadline_payload_us: u64,
    /// Maximum packet length, octets.
    pub max_packet_length: u16,
    /// Idle Packet data octet.
    pub idle_pattern: u8,
}

/// TC uplink (§4.2, APP-040…APP-042).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tc {
    /// Maximum transfer frame length, octets.
    pub max_frame_length: Ranged<u16, 0, 1024>,
    /// FECF present.
    pub fecf: bool,
    /// The uplink VC.
    pub vcid: Ranged<u8, 0, 63>,
    /// MAP of real-time commands.
    pub map_commands: Ranged<u8, 0, 63>,
    /// MAP of the table upload.
    pub map_table_upload: Ranged<u8, 0, 63>,
    /// Maximum frame data unit length, octets.
    pub max_fdu_length: Ranged<u16, 0, 1019>,
    /// Maximum packet length, octets.
    pub max_packet_length: u16,
}

/// COP-1 parameters (§4.2, APP-044).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cop1 {
    /// FOP sliding window width K.
    pub k: Ranged<u8, 1, 255>,
    /// FARM sliding window width W.
    pub w: EvenRanged<u8, 2, 254>,
    /// `T1_Initial`, µs.
    pub t1_initial_us: u64,
    /// `Transmission_Limit`.
    pub transmission_limit: Ranged<u8, 1, 255>,
    /// `Timeout_Type` (0 or 1).
    pub timeout_type: Ranged<u8, 0, 1>,
    /// FARM-1 internal buffer depth (COP-032).
    pub farm_buffer_depth: Ranged<u8, 1, 255>,
}

/// Link level of a case (§1, §6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// A1: one Space Packet per link unit, sent when output.
    Packets,
    /// A2 on: one frame per slot.
    Frames,
}

/// One case (§6.4, APP-076).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// Case ID, also the golden-trace file name.
    #[schemars(regex(pattern = r"^[a-z0-9]+(-[a-z0-9]+)*$"))]
    pub id: String,
    /// Link level.
    pub level: Level,
    /// End instant, exclusive, µs.
    pub end_us: u64,
    /// Version of the case's golden trace (§7.5, APP-087).
    #[schemars(range(min = 1))]
    pub trace_version: u32,
    /// Requirement IDs the case verifies, sorted (§7.2).
    pub requirements: Vec<String>,
    /// The ops script: what the ground node releases, and when (§6.5, APP-126). Entry `i` is
    /// the ops-script entry index `i` of the node interface (§8.1).
    #[serde(default)]
    pub ops: Vec<OpsEntry>,
    /// The fault script: faults by (direction, unit index) (§6.3, §6.5, APP-075, APP-126).
    #[serde(default)]
    pub faults: Vec<FaultEntry>,
}

/// One entry of a case's ops script (§6.5, APP-126).
///
/// Entries are in release order. An entry names its kind and the parameters of the packet the
/// ground node builds from it (§3.3.5, §3.3.6), never its octets, so that a node in another
/// language reads the same file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OpsEntry {
    /// A command packet on the commands APID (§3.3.5).
    Command {
        /// Release instant, µs: the ground node outputs the packet then (§6.2).
        at_us: u64,
        /// Execution mode and time.
        execution: Execution,
        /// Function and arguments.
        call: Call,
        /// Octets appended after the arguments, so that the packet length does not match the
        /// function code (APP-024). Absent: none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extra_octets: Option<u8>,
    },
    /// A table-upload packet on the table-upload APID (§3.3.6). Table octet `i` is
    /// `(3 · i + 1) mod 256`.
    TableUpload {
        /// Release instant, µs.
        at_us: u64,
        /// Table ID.
        #[schemars(range(min = 1, max = 255))]
        table_id: u8,
        /// Table length, octets.
        #[schemars(range(min = 1, max = 503))]
        length: u16,
    },
    /// A FOP-1 directive: reserved from A1, issued from A3 on, ignored below (APP-019).
    Fop1Directive {
        /// Release instant, µs.
        at_us: u64,
        /// The directive; A3 adds the others it needs (APP-002).
        directive: Fop1Directive,
    },
}

impl OpsEntry {
    /// The entry as the ground node takes it (§6.5).
    ///
    /// # Errors
    /// `InvalidField` or `OutOfRange` for a table upload whose ID or length is outside its range,
    /// which [`Scenario::check`] rejects first.
    pub fn to_node(&self) -> Result<refapp_ground::OpsEntry, String> {
        use refapp_ground as node;
        Ok(match *self {
            OpsEntry::Command {
                execution,
                call,
                extra_octets,
                ..
            } => node::OpsEntry::Command {
                execution: match execution {
                    Execution::Immediate {} => node::Execution::Immediate,
                    Execution::TimeTagged { exec_us } => node::Execution::TimeTagged {
                        after_t0: Duration::from_micros(exec_us),
                    },
                },
                call: match call {
                    Call::Noop {} => node::Call::Noop,
                    Call::DumpMemory { address, length } => {
                        node::Call::DumpMemory { address, length }
                    }
                    Call::ActivateTable { table_id, crc } => node::Call::ActivateTable {
                        table_id,
                        crc: Crc16::from_wire(crc),
                    },
                    Call::SetPayloadMode { mode } => node::Call::SetPayloadMode { mode },
                    Call::Unknown { code } => node::Call::Unknown { code },
                },
                extra_octets: extra_octets.unwrap_or(0),
            },
            OpsEntry::TableUpload {
                table_id, length, ..
            } => node::OpsEntry::TableUpload {
                table_id: TableId::new(table_id)
                    .map_err(|e| format!("table_id: {:?}", e.kind()))?,
                length: TableLength::new(length)
                    .map_err(|e| format!("table length: {:?}", e.kind()))?,
            },
            OpsEntry::Fop1Directive { .. } => node::OpsEntry::Fop1Directive,
        })
    }

    /// The release instant, µs (§6.5).
    #[must_use]
    pub const fn at_us(&self) -> u64 {
        match self {
            OpsEntry::Command { at_us, .. }
            | OpsEntry::TableUpload { at_us, .. }
            | OpsEntry::Fop1Directive { at_us, .. } => *at_us,
        }
    }
}

/// Execution mode of a command (§2.3, §3.3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Execution {
    /// Executes when delivered; the execution-time field is zero.
    Immediate {},
    /// Executes at the on-board time T0 + `exec_us`, a multiple of the frame slot (APP-061).
    TimeTagged {
        /// Execution instant, µs after T0.
        exec_us: u64,
    },
}

/// Function and arguments of a command (§3.3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "function", rename_all = "snake_case", deny_unknown_fields)]
pub enum Call {
    /// `NOOP`, function code `0x01`.
    Noop {},
    /// `DUMP_MEMORY`, function code `0x02`.
    DumpMemory {
        /// First address of the dump.
        address: u32,
        /// Length, octets.
        #[schemars(range(min = 1, max = 1024))]
        length: u16,
    },
    /// `ACTIVATE_TABLE`, function code `0x03`.
    ActivateTable {
        /// Table ID.
        #[schemars(range(min = 1, max = 255))]
        table_id: u8,
        /// CRC-16 of the table that the command claims (CMN-001).
        crc: u16,
    },
    /// `SET_PAYLOAD_MODE`, function code `0x04`.
    SetPayloadMode {
        /// New mode: 0 off, 1 on.
        #[schemars(range(max = 1))]
        mode: u8,
    },
    /// A function code that no function has (reason 1 of `CMD_REJECTED`, §3.3.2).
    Unknown {
        /// The code; not one of `0x01`…`0x04`.
        code: u8,
    },
}

/// A FOP-1 directive (§2.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Fop1Directive {
    /// *Initiate AD Service (without CLCW Check)* (232.1 §4.1).
    InitiateAdWithoutClcw,
}

/// Direction of a link unit (§6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Flight node to ground node.
    Down,
    /// Ground node to flight node.
    Up,
}

/// One entry of a case's fault script (§6.3, APP-075): the fault applied to the unit sent in
/// direction `dir` with index `index`, counted from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FaultEntry {
    /// Direction in which the unit travels.
    pub dir: Direction,
    /// Index of the unit among those sent in that direction.
    pub index: u64,
    /// The fault.
    pub fault: Fault,
}

/// A fault and its arguments (§6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Fault {
    /// The unit is not delivered.
    Drop {},
    /// The unit is delivered twice at the same instant.
    Duplicate {},
    /// The unit arrives `delay_us` later than normal.
    Delay {
        /// Extra delay, µs.
        delay_us: u64,
    },
    /// One bit is inverted; frame levels only, where the FECF catches it.
    Flip {
        /// Octet number, from 0.
        octet: u32,
        /// Bit number in the octet, 0 the most significant (GEN-050).
        #[schemars(range(max = 7))]
        bit: u8,
    },
    /// Only the first `octets` octets are delivered.
    Truncate {
        /// Octets kept.
        octets: u32,
    },
}

impl Scenario {
    /// Parses and checks a scenario file.
    ///
    /// # Errors
    /// If the text does not match the model, or breaks a rule the schema cannot express.
    pub fn parse(text: &str) -> Result<Scenario, String> {
        let s: Scenario = serde_json::from_str(text).map_err(|e| format!("scenario.json: {e}"))?;
        s.check()?;
        Ok(s)
    }

    /// The scenario compiled into the harness.
    ///
    /// # Errors
    /// As [`Scenario::parse`].
    pub fn load() -> Result<Scenario, String> {
        Scenario::parse(SCENARIO_JSON)
    }

    /// The case with the given ID.
    ///
    /// # Errors
    /// If there is no such case.
    pub fn case(&self, id: &str) -> Result<&Case, String> {
        self.cases
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| format!("no case `{id}` in scenario.json"))
    }

    fn check(&self) -> Result<(), String> {
        if self.scenario != "aldis-refapp" {
            return Err(format!(
                "scenario.json: scenario is `{}`, expected `aldis-refapp`",
                self.scenario
            ));
        }
        let mut ids = BTreeSet::new();
        for c in &self.cases {
            if !ids.insert(c.id.as_str()) {
                return Err(format!("scenario.json: duplicate case `{}`", c.id));
            }
            let valid_id = !c.id.is_empty()
                && c.id.split('-').all(|part| {
                    !part.is_empty()
                        && part
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                });
            if !valid_id {
                return Err(format!("scenario.json: invalid case ID `{}`", c.id));
            }
            if c.trace_version == 0 {
                return Err(format!("scenario.json: case `{}`: trace_version 0", c.id));
            }
            let mut sorted = c.requirements.clone();
            sorted.sort();
            sorted.dedup();
            if sorted != c.requirements || !c.requirements.iter().all(|r| is_requirement_id(r)) {
                return Err(format!(
                    "scenario.json: case `{}`: requirements must be sorted, unique requirement IDs",
                    c.id
                ));
            }
            self.check_scripts(c)
                .map_err(|e| format!("scenario.json: case `{}`: {e}", c.id))?;
        }
        Ok(())
    }

    /// The rules of §6.5 that the schema cannot express, and the ranges of the entries
    /// (APP-061, APP-126, APP-128).
    fn check_scripts(&self, c: &Case) -> Result<(), String> {
        let slot = self.link.frame_slot_us.get();
        let delay = self.link.one_way_delay_us;
        let mut previous = 0;
        // Execution times of the time-tagged commands the flight node holds (§2.3).
        let mut queue: Vec<u64> = Vec::new();
        for (i, entry) in c.ops.iter().enumerate() {
            let at_us = match entry {
                OpsEntry::Command {
                    at_us,
                    execution,
                    call,
                    ..
                } => {
                    let arrival = at_us.saturating_add(delay);
                    let on_grid = arrival.is_multiple_of(slot);
                    match execution {
                        Execution::Immediate {} if !on_grid => {
                            return Err(format!(
                                "ops[{i}]: an immediate command must arrive on a multiple of \
                                 {slot} µs (APP-128)"
                            ));
                        }
                        Execution::TimeTagged { exec_us } => {
                            check_tagged(&mut queue, arrival, *exec_us, slot)
                                .map_err(|e| format!("ops[{i}]: {e}"))?;
                        }
                        Execution::Immediate {} => {}
                    }
                    check_call(*call).map_err(|e| format!("ops[{i}]: {e}"))?;
                    *at_us
                }
                OpsEntry::TableUpload {
                    at_us,
                    table_id,
                    length,
                } => {
                    if TableId::new(*table_id).is_err() {
                        return Err(format!("ops[{i}]: table_id is 1 to 255"));
                    }
                    if TableLength::new(*length).is_err() {
                        return Err(format!("ops[{i}]: table length is 1 to 503"));
                    }
                    *at_us
                }
                OpsEntry::Fop1Directive { at_us, .. } => *at_us,
            };
            if at_us < previous {
                return Err(format!("ops[{i}]: release instants must not decrease"));
            }
            if at_us >= c.end_us {
                return Err(format!("ops[{i}]: released at or after the end instant"));
            }
            previous = at_us;
        }
        for (i, f) in c.faults.iter().enumerate() {
            match f.fault {
                Fault::Flip { .. } if c.level == Level::Packets => {
                    return Err(format!(
                        "faults[{i}]: `flip` needs a FECF, so frame level only"
                    ));
                }
                Fault::Flip { bit: 8.., .. } => {
                    return Err(format!("faults[{i}]: bit is 0 to 7"));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// Time-tag queue capacity of the flight node (§2.3).
const QUEUE_CAPACITY: usize = 8;

/// A time-tagged command arriving at `arrival`: its execution time is on the grid (APP-061),
/// and one that the flight node rejects at delivery (§2.3: a time tag before the arrival, or a
/// full queue) arrives on the grid too (APP-128). `queue` holds the execution times of the
/// commands the flight node still has queued.
fn check_tagged(queue: &mut Vec<u64>, arrival: u64, exec_us: u64, slot: u64) -> Result<(), String> {
    if !exec_us.is_multiple_of(slot) {
        return Err(format!(
            "execution time must be a multiple of {slot} µs (APP-061)"
        ));
    }
    queue.retain(|e| *e >= arrival);
    if exec_us >= arrival && queue.len() < QUEUE_CAPACITY {
        queue.push(exec_us);
    } else if !arrival.is_multiple_of(slot) {
        return Err(format!(
            "a command rejected at delivery must arrive on a multiple of {slot} µs (APP-128)"
        ));
    }
    Ok(())
}

/// The ranges of a command's arguments (§3.3.5).
fn check_call(call: Call) -> Result<(), String> {
    match call {
        Call::DumpMemory { length, .. } if !(1..=1024).contains(&length) => {
            Err("dump length is 1 to 1024".to_owned())
        }
        Call::ActivateTable { table_id: 0, .. } => Err("table_id is 1 to 255".to_owned()),
        Call::SetPayloadMode { mode: 2.. } => Err("mode is 0 or 1".to_owned()),
        Call::Unknown { code: 1..=4 } => {
            Err("the function codes 1 to 4 are not unknown".to_owned())
        }
        Call::Noop {}
        | Call::DumpMemory { .. }
        | Call::ActivateTable { .. }
        | Call::SetPayloadMode { .. }
        | Call::Unknown { .. } => Ok(()),
    }
}

/// `AREA-NNN` with an upper-case area and three digits.
fn is_requirement_id(s: &str) -> bool {
    s.split_once('-').is_some_and(|(area, n)| {
        !area.is_empty()
            && area.bytes().all(|b| b.is_ascii_uppercase())
            && n.len() == 3
            && n.bytes().all(|b| b.is_ascii_digit())
    })
}

/// The `refapp/` directory.
#[must_use]
pub fn refapp_dir() -> PathBuf {
    let harness = Path::new(env!("CARGO_MANIFEST_DIR"));
    harness
        .parent()
        .map_or_else(|| harness.join(".."), Path::to_path_buf)
}

/// Path of the generated JSON Schema, relative to `refapp/`.
pub const SCHEMA_PATH: &str = "scenario/scenario.schema.json";
/// Path of the generated Rust constants, relative to `refapp/`.
pub const CONSTANTS_PATH: &str = "refapp-scenario/src/generated.rs";

/// The published JSON Schema of the scenario file (APP-107).
///
/// # Errors
/// If serialization fails.
pub fn schema() -> Result<String, String> {
    let schema = schemars::schema_for!(Scenario);
    serde_json::to_string_pretty(&schema)
        .map(|s| s + "\n")
        .map_err(|e| e.to_string())
}

/// A decimal literal with `_` every three digits when it has more than four digits.
fn literal(v: u64) -> String {
    let digits = v.to_string();
    if digits.len() <= 4 {
        return digits;
    }
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('_');
        }
        out.push(c);
    }
    out
}

/// One generated constant: name, Rust type, value, documentation.
type Const<'a> = (&'a str, &'a str, String, &'a str);

fn module(out: &mut String, name: &str, doc: &str, consts: &[Const<'_>]) {
    let _ = writeln!(out, "\n/// {doc}\npub mod {name} {{");
    for (i, (c, ty, value, doc)) in consts.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let _ = writeln!(out, "    /// {doc}\n    pub const {c}: {ty} = {value};");
    }
    out.push_str("}\n");
}

/// Rust constants for `refapp-scenario` (APP-107).
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one flat table of every generated constant, easier to review than split up"
)]
pub fn rust_constants(s: &Scenario) -> String {
    let n = |v: u64| literal(v);
    let mut out = String::from(
        "//! Scenario values generated from `refapp/scenario/scenario.json` (APP-107).\n\
         //!\n\
         //! Do not edit: run `cargo run -p refapp-harness -- gen-scenario` after changing the\n\
         //! scenario file. Section numbers refer to `docs/reference-application.md`.\n\n",
    );
    let _ = writeln!(
        out,
        "/// Scenario identifier in every golden-trace header (§7.2).\n\
         pub const SCENARIO: &str = {:?};\n\n\
         /// Scenario version (§7.2, APP-002).\n\
         pub const SCENARIO_VERSION: u16 = {};\n\n\
         /// Spacecraft ID of every frame (§3.4, APP-026); fictional (OI-26).\n\
         pub const SPACECRAFT_ID: u16 = {};",
        s.scenario,
        n(s.scenario_version.into()),
        n(s.spacecraft_id.get().into())
    );
    module(
        &mut out,
        "time",
        "Time model (§5, APP-060).",
        &[
            (
                "T0_COARSE_S",
                "u32",
                n(s.time.t0_coarse_s.into()),
                "CUC coarse time at the start of the run, s since 1958-01-01 TAI.",
            ),
            (
                "CUC_COARSE_OCTETS",
                "u8",
                n(s.time.cuc_coarse_octets.get().into()),
                "CUC coarse octets.",
            ),
            (
                "CUC_FINE_OCTETS",
                "u8",
                n(s.time.cuc_fine_octets.get().into()),
                "CUC fractional octets.",
            ),
            (
                "CUC_EXPLICIT_PFIELD",
                "u8",
                n(s.time.cuc_explicit_pfield.into()),
                "Explicit P-field octet (301.0 §3.2.2).",
            ),
        ],
    );
    module(
        &mut out,
        "link",
        "Link model (§6.1).",
        &[
            (
                "ONE_WAY_DELAY_US",
                "u64",
                n(s.link.one_way_delay_us),
                "One-way delay, both directions, µs (APP-072).",
            ),
            (
                "FRAME_SLOT_US",
                "u64",
                n(s.link.frame_slot_us.get()),
                "Downlink frame slot period, µs (APP-071).",
            ),
            (
                "UPLINK_SLOT_US",
                "u64",
                n(s.link.uplink_slot_us.get()),
                "Uplink slot period, µs (APP-071).",
            ),
        ],
    );
    let a = &s.apid;
    module(
        &mut out,
        "apid",
        "APIDs (§3.1, APP-020).",
        &[
            (
                "HOUSEKEEPING",
                "u16",
                n(a.housekeeping.get().into()),
                "Housekeeping.",
            ),
            ("EVENTS", "u16", n(a.events.get().into()), "Event reports."),
            (
                "PAYLOAD",
                "u16",
                n(a.payload.get().into()),
                "Payload science.",
            ),
            ("DUMP", "u16", n(a.dump.get().into()), "Memory dump."),
            ("COMMANDS", "u16", n(a.commands.get().into()), "Commands."),
            (
                "TABLE_UPLOAD",
                "u16",
                n(a.table_upload.get().into()),
                "Table upload.",
            ),
        ],
    );
    let tm = &s.tm;
    module(
        &mut out,
        "tm",
        "TM downlink (§4.1).",
        &[
            (
                "FRAME_LENGTH",
                "u16",
                n(tm.frame_length.get().into()),
                "Transfer frame length, octets (APP-030).",
            ),
            (
                "FECF",
                "bool",
                tm.fecf.to_string(),
                "FECF present (APP-030).",
            ),
            (
                "VCID_REALTIME",
                "u8",
                n(tm.vcid_realtime.get().into()),
                "VC of housekeeping and events (APP-032).",
            ),
            (
                "VCID_PAYLOAD",
                "u8",
                n(tm.vcid_payload.get().into()),
                "VC of payload and dump (APP-032).",
            ),
            (
                "VCID_IDLE",
                "u8",
                n(tm.vcid_idle.get().into()),
                "VC of OID frames (APP-034).",
            ),
            (
                "RELEASE_DEADLINE_REALTIME_US",
                "u64",
                n(tm.release_deadline_realtime_us),
                "Release deadline of the real-time VC, µs (APP-033).",
            ),
            (
                "RELEASE_DEADLINE_PAYLOAD_US",
                "u64",
                n(tm.release_deadline_payload_us),
                "Release deadline of the payload VC, µs (APP-033).",
            ),
            (
                "MAX_PACKET_LENGTH",
                "u16",
                n(tm.max_packet_length.into()),
                "Maximum packet length, octets (APP-032).",
            ),
            (
                "IDLE_PATTERN",
                "u8",
                n(tm.idle_pattern.into()),
                "Idle Packet data octet (APP-033).",
            ),
        ],
    );
    let tc = &s.tc;
    module(
        &mut out,
        "tc",
        "TC uplink (§4.2).",
        &[
            (
                "MAX_FRAME_LENGTH",
                "u16",
                n(tc.max_frame_length.get().into()),
                "Maximum transfer frame length, octets (APP-040).",
            ),
            (
                "FECF",
                "bool",
                tc.fecf.to_string(),
                "FECF present (APP-040).",
            ),
            (
                "VCID",
                "u8",
                n(tc.vcid.get().into()),
                "The uplink VC (APP-041).",
            ),
            (
                "MAP_COMMANDS",
                "u8",
                n(tc.map_commands.get().into()),
                "MAP of real-time commands (APP-041).",
            ),
            (
                "MAP_TABLE_UPLOAD",
                "u8",
                n(tc.map_table_upload.get().into()),
                "MAP of the table upload (APP-041).",
            ),
            (
                "MAX_FDU_LENGTH",
                "u16",
                n(tc.max_fdu_length.get().into()),
                "Maximum frame data unit length, octets (APP-041).",
            ),
            (
                "MAX_PACKET_LENGTH",
                "u16",
                n(tc.max_packet_length.into()),
                "Maximum packet length, octets (APP-041).",
            ),
        ],
    );
    let c = &s.cop1;
    module(
        &mut out,
        "cop1",
        "COP-1 parameters (§4.2, APP-044).",
        &[
            (
                "K",
                "u8",
                n(c.k.get().into()),
                "FOP sliding window width K.",
            ),
            (
                "W",
                "u8",
                n(c.w.get().into()),
                "FARM sliding window width W.",
            ),
            (
                "T1_INITIAL_US",
                "u64",
                n(c.t1_initial_us),
                "`T1_Initial`, µs.",
            ),
            (
                "TRANSMISSION_LIMIT",
                "u8",
                n(c.transmission_limit.get().into()),
                "`Transmission_Limit`.",
            ),
            (
                "TIMEOUT_TYPE",
                "u8",
                n(c.timeout_type.get().into()),
                "`Timeout_Type`.",
            ),
            (
                "FARM_BUFFER_DEPTH",
                "u8",
                n(c.farm_buffer_depth.get().into()),
                "FARM-1 internal buffer depth (COP-032).",
            ),
        ],
    );
    out
}

/// Generated files and their expected contents, paths relative to `refapp/`.
///
/// # Errors
/// If the scenario file is invalid.
pub fn generated_files() -> Result<Vec<(&'static str, String)>, String> {
    let s = Scenario::load()?;
    Ok(vec![
        (SCHEMA_PATH, schema()?),
        (CONSTANTS_PATH, rust_constants(&s)),
    ])
}

/// Writes the generated files, or with `check` reports the stale ones. Returns the number of
/// stale files.
///
/// # Errors
/// If the scenario file is invalid or a file cannot be written.
pub fn generate(check: bool) -> Result<usize, String> {
    let dir = refapp_dir();
    let mut stale = 0;
    for (path, body) in generated_files()? {
        let file = dir.join(path);
        if check {
            if std::fs::read_to_string(&file).ok().as_deref() != Some(body.as_str()) {
                println!(
                    "error: refapp/{path} is out of date; run `cargo run -p refapp-harness -- gen-scenario`"
                );
                stale += 1;
            }
        } else {
            std::fs::write(&file, body).map_err(|e| format!("refapp/{path}: {e}"))?;
            println!("wrote refapp/{path}");
        }
    }
    if check && stale == 0 {
        println!("gen-scenario: up to date");
    }
    Ok(stale)
}

#[cfg(test)]
mod tests {
    use super::{
        Call, Direction, Execution, Fault, FaultEntry, OpsEntry, Scenario, is_requirement_id,
        literal,
    };

    #[test]
    fn literals_and_ids() {
        // req: APP-107
        assert_eq!(literal(7), "7");
        assert_eq!(literal(1024), "1024");
        assert_eq!(literal(10_000), "10_000");
        assert_eq!(literal(2_100_000_000), "2_100_000_000");
        assert!(is_requirement_id("APP-080"));
        assert!(!is_requirement_id("APP-80"));
        assert!(!is_requirement_id("app-080"));
    }

    #[test]
    fn rejects_bad_files() {
        // req: APP-107
        let good = super::SCENARIO_JSON;
        assert!(Scenario::parse(good).is_ok());
        let unknown = good.replacen(
            "\"scenario_version\"",
            "\"extra\": 1, \"scenario_version\"",
            1,
        );
        assert!(Scenario::parse(&unknown).is_err());
        let unsorted = good.replace(
            "[\"APP-080\", \"APP-081\", \"APP-085\"]",
            "[\"APP-081\", \"APP-080\"]",
        );
        assert!(Scenario::parse(&unsorted).is_err());
        let bad_id = good.replace("\"a0-empty\"", "\"A0 empty\"");
        assert!(Scenario::parse(&bad_id).is_err());
        let wrong_name = good.replace("\"aldis-refapp\"", "\"other\"");
        assert!(Scenario::parse(&wrong_name).is_err());
    }

    /// The scenario file with the value at JSON pointer `pointer` replaced by `value`.
    fn mutated(pointer: &str, value: u64) -> String {
        let mut doc: serde_json::Value = serde_json::from_str(super::SCENARIO_JSON).unwrap();
        *doc.pointer_mut(pointer).unwrap() = value.into();
        doc.to_string()
    }

    /// Every field of the A0 scenario with a range: the pointer, the lowest and the highest
    /// value.
    const RANGES: &[(&str, u64, u64)] = &[
        ("/spacecraft_id", 0, 1023),
        ("/time/cuc_coarse_octets", 1, 4),
        ("/time/cuc_fine_octets", 0, 3),
        ("/link/frame_slot_us", 1, u64::MAX),
        ("/link/uplink_slot_us", 1, u64::MAX),
        ("/apid/housekeeping", 0, 2046),
        ("/apid/events", 0, 2046),
        ("/apid/payload", 0, 2046),
        ("/apid/dump", 0, 2046),
        ("/apid/commands", 0, 2046),
        ("/apid/table_upload", 0, 2046),
        ("/tm/frame_length", 0, 2048),
        ("/tm/vcid_realtime", 0, 7),
        ("/tm/vcid_payload", 0, 7),
        ("/tm/vcid_idle", 0, 7),
        ("/tc/max_frame_length", 0, 1024),
        ("/tc/vcid", 0, 63),
        ("/tc/map_commands", 0, 63),
        ("/tc/map_table_upload", 0, 63),
        ("/tc/max_fdu_length", 0, 1019),
        ("/cop1/k", 1, 255),
        ("/cop1/w", 2, 254),
        ("/cop1/transmission_limit", 1, 255),
        ("/cop1/timeout_type", 0, 1),
        ("/cop1/farm_buffer_depth", 1, 255),
    ];

    #[test]
    fn a_value_outside_its_range_is_rejected() {
        // req: APP-107
        for &(pointer, min, max) in RANGES {
            if let Some(below) = min.checked_sub(1) {
                let e = Scenario::parse(&mutated(pointer, below)).unwrap_err();
                assert!(e.contains("outside the range"), "{pointer} = {below}: {e}");
            }
            if let Some(above) = max.checked_add(1) {
                // A value above the range of the field's type fails in the type, not the range.
                let e = Scenario::parse(&mutated(pointer, above)).unwrap_err();
                assert!(e.contains("scenario.json"), "{pointer} = {above}: {e}");
            }
        }
    }

    #[test]
    fn an_odd_window_is_rejected() {
        // req: APP-107
        let e = Scenario::parse(&mutated("/cop1/w", 5)).unwrap_err();
        assert!(e.contains("odd"), "{e}");
        let schema: serde_json::Value = serde_json::from_str(&super::schema().unwrap()).unwrap();
        assert_eq!(schema["$defs"]["Cop1"]["properties"]["w"]["multipleOf"], 2);
    }

    #[test]
    fn the_ends_of_every_range_are_accepted() {
        // req: APP-107
        for &(pointer, min, max) in RANGES {
            for v in [min, max] {
                let r = Scenario::parse(&mutated(pointer, v));
                assert!(r.is_ok(), "{pointer} = {v}: {:?}", r.err());
            }
        }
    }

    #[test]
    fn the_schema_states_the_ranges_the_parser_enforces() {
        // req: APP-107
        let schema: serde_json::Value = serde_json::from_str(&super::schema().unwrap()).unwrap();
        for &(pointer, min, max) in RANGES {
            let (section, field) = pointer[1..].split_once('/').unwrap_or(("", &pointer[1..]));
            let def = if section.is_empty() {
                &schema["properties"][field]
            } else {
                let name = match section {
                    "apid" => "Apids".to_owned(),
                    _ => format!("{}{}", section[..1].to_uppercase(), &section[1..]),
                };
                &schema["$defs"][name]["properties"][field]
            };
            assert_eq!(def["minimum"], min, "{pointer}");
            assert_eq!(def["maximum"], max, "{pointer}");
        }
    }

    fn rejects(s: &Scenario, what: &str) -> bool {
        s.check().is_err_and(|e| e.contains(what))
    }

    #[test]
    fn script_entries_of_the_a1_cases_parse_and_check() {
        // req: APP-126, APP-127, APP-128
        let s = Scenario::load().unwrap();
        s.check().unwrap();
        let rejects_case = s.case("a1-rejects").unwrap();
        assert_eq!(rejects_case.ops.len(), 18);
        assert_eq!(s.case("a1-downlink-faults").unwrap().faults.len(), 5);
        // An entry names parameters, not octets: it serializes back to the same JSON.
        let file: serde_json::Value = serde_json::from_str(super::SCENARIO_JSON).unwrap();
        let entries = file["cases"].as_array().unwrap();
        for (case, json) in s.cases.iter().zip(entries).skip(1) {
            assert_eq!(&serde_json::to_value(case).unwrap()["ops"], &json["ops"]);
        }
    }

    #[test]
    fn script_rules_the_schema_cannot_express() {
        // req: APP-061, APP-126, APP-128
        let ok = Scenario::load().unwrap();
        let mut s = ok.clone();
        // An immediate command that arrives off the 125 000 µs grid.
        s.cases[3].ops[0] = OpsEntry::Command {
            at_us: 100_000,
            execution: Execution::Immediate {},
            call: Call::Noop {},
            extra_octets: None,
        };
        assert!(rejects(&s, "APP-128"));
        // An execution time off the grid.
        let mut s = ok.clone();
        s.cases[1].ops[0] = OpsEntry::Command {
            at_us: 1_000_000,
            execution: Execution::TimeTagged { exec_us: 6_000_001 },
            call: Call::Noop {},
            extra_octets: None,
        };
        assert!(rejects(&s, "APP-061"));
        // A function code that exists.
        let mut s = ok.clone();
        s.cases[3].ops[1] = OpsEntry::Command {
            at_us: 615_000,
            execution: Execution::Immediate {},
            call: Call::Unknown { code: 4 },
            extra_octets: None,
        };
        assert!(rejects(&s, "not unknown"));
        // Release order, and the end instant.
        let mut s = ok.clone();
        s.cases[3].ops.swap(0, 1);
        assert!(rejects(&s, "must not decrease"));
        let mut s = ok.clone();
        s.cases[3].end_us = 10_000_000;
        assert!(rejects(&s, "end instant"));
        // A bit flip needs a FECF.
        let mut s = ok;
        s.cases[4].faults.push(FaultEntry {
            dir: Direction::Down,
            index: 0,
            fault: Fault::Flip { octet: 0, bit: 0 },
        });
        assert!(rejects(&s, "flip"));
    }

    /// The `a1-rejects` case with its ops entry `i` replaced.
    fn rejects_with(i: usize, entry: OpsEntry) -> Scenario {
        let mut s = Scenario::load().unwrap();
        s.cases[3].ops[i] = entry;
        s
    }

    fn command(at_us: u64, execution: Execution, call: Call) -> OpsEntry {
        OpsEntry::Command {
            at_us,
            execution,
            call,
            extra_octets: None,
        }
    }

    #[test]
    fn rejected_at_delivery_commands_arrive_on_the_grid() {
        // req: APP-128
        // Reason 2: a time tag before the arrival, released off the grid.
        let tagged = Execution::TimeTagged { exec_us: 3_000_000 };
        let s = rejects_with(3, command(3_600_000, tagged, Call::Noop {}));
        assert!(rejects(&s, "rejected at delivery"));
        // The same command on the grid is fine.
        assert!(
            rejects_with(3, command(3_615_000, tagged, Call::Noop {}))
                .check()
                .is_ok()
        );
        // Reason 4: the ninth command of a full queue, released off the grid.
        let mut s = Scenario::load().unwrap();
        s.cases[3].ops[17] = command(
            10_115_001,
            Execution::TimeTagged {
                exec_us: 20_000_000,
            },
            Call::Noop {},
        );
        assert!(rejects(&s, "rejected at delivery"));
        // A command that only queues needs no grid: `a1-nominal` releases on whole seconds.
        Scenario::load().unwrap().check().unwrap();
    }

    #[test]
    fn script_ranges_are_enforced() {
        // req: APP-126
        let imm = Execution::Immediate {};
        let table = |table_id, length| OpsEntry::TableUpload {
            at_us: 5_115_000,
            table_id,
            length,
        };
        for (entry, what) in [
            (table(0, 300), "table_id"),
            (table(1, 0), "table length"),
            (table(1, 504), "table length"),
            (
                command(
                    115_000,
                    imm,
                    Call::DumpMemory {
                        address: 0,
                        length: 0,
                    },
                ),
                "dump length",
            ),
            (
                command(
                    115_000,
                    imm,
                    Call::DumpMemory {
                        address: 0,
                        length: 1025,
                    },
                ),
                "dump length",
            ),
            (
                command(
                    115_000,
                    imm,
                    Call::ActivateTable {
                        table_id: 0,
                        crc: 0,
                    },
                ),
                "table_id",
            ),
            (
                command(115_000, imm, Call::SetPayloadMode { mode: 2 }),
                "mode",
            ),
            (
                command(115_000, imm, Call::Unknown { code: 1 }),
                "not unknown",
            ),
        ] {
            let idx = if matches!(entry, OpsEntry::TableUpload { .. }) {
                5
            } else {
                0
            };
            assert!(rejects(&rejects_with(idx, entry), what), "{what}");
        }
        let mut s = Scenario::load().unwrap();
        s.cases[4].level = super::Level::Frames;
        s.cases[4].faults.push(FaultEntry {
            dir: Direction::Down,
            index: 0,
            fault: Fault::Flip { octet: 0, bit: 8 },
        });
        assert!(rejects(&s, "bit is 0 to 7"));
        s.cases[4].faults[5].fault = Fault::Flip { octet: 0, bit: 7 };
        s.check().unwrap();
    }

    #[test]
    fn table_length_limit_is_503() {
        // req: APP-017
        // req: APP-126
        let table = |length| OpsEntry::TableUpload {
            at_us: 5_115_000,
            table_id: 1,
            length,
        };
        rejects_with(5, table(503)).check().unwrap();
        assert!(rejects(&rejects_with(5, table(504)), "table length"));
    }

    #[test]
    fn script_entries_reject_unknown_fields_and_kinds() {
        // req: APP-126
        let entry = |text: &str| serde_json::from_str::<super::OpsEntry>(text);
        assert!(entry(r#"{"kind":"table_upload","at_us":1,"table_id":1,"length":3}"#).is_ok());
        assert!(
            entry(r#"{"kind":"fop1_directive","at_us":0,"directive":"initiate_ad_without_clcw"}"#)
                .is_ok()
        );
        assert!(
            entry(r#"{"kind":"table_upload","at_us":1,"table_id":1,"length":3,"hex":"00"}"#)
                .is_err()
        );
        assert!(entry(r#"{"kind":"octets","at_us":1,"hex":"00"}"#).is_err());
        let fault = |text: &str| serde_json::from_str::<super::FaultEntry>(text);
        assert!(
            fault(r#"{"dir":"up","index":0,"fault":{"kind":"flip","octet":3,"bit":7}}"#).is_ok()
        );
        assert!(fault(r#"{"dir":"up","index":0,"fault":{"kind":"drop","octet":3}}"#).is_err());
    }
}
