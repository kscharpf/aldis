//! The scenario file `refapp/scenario/scenario.json` (docs/reference-application.md, APP-107).
//!
//! These types are the single source of truth for the file format: [`Scenario::load`]
//! deserializes the file with them (unknown fields are errors), [`schema`] publishes the JSON
//! Schema generated from them, and [`rust_constants`] generates `refapp-scenario`'s constants,
//! so the validator, the schema, and the generated code cannot drift apart.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

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
    /// Spacecraft ID of every frame (§3.4, APP-026; provisional until OI-26 is resolved).
    #[schemars(range(max = 1023))]
    pub spacecraft_id: u16,
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
    #[schemars(range(min = 1, max = 4))]
    pub cuc_coarse_octets: u8,
    /// CUC fractional octets.
    #[schemars(range(max = 3))]
    pub cuc_fine_octets: u8,
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
    #[schemars(range(min = 1))]
    pub frame_slot_us: u64,
    /// Uplink slot period, µs (A2 on).
    #[schemars(range(min = 1))]
    pub uplink_slot_us: u64,
}

/// APIDs (§3.1, APP-020).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Apids {
    /// Housekeeping.
    #[schemars(range(max = 2046))]
    pub housekeeping: u16,
    /// Event reports.
    #[schemars(range(max = 2046))]
    pub events: u16,
    /// Payload science.
    #[schemars(range(max = 2046))]
    pub payload: u16,
    /// Memory dump.
    #[schemars(range(max = 2046))]
    pub dump: u16,
    /// Commands.
    #[schemars(range(max = 2046))]
    pub commands: u16,
    /// Table upload.
    #[schemars(range(max = 2046))]
    pub table_upload: u16,
}

/// TM downlink (§4.1, APP-030…APP-034).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tm {
    /// Transfer frame length, octets.
    #[schemars(range(max = 2048))]
    pub frame_length: u16,
    /// FECF present.
    pub fecf: bool,
    /// VC of housekeeping and events.
    #[schemars(range(max = 7))]
    pub vcid_realtime: u8,
    /// VC of payload and dump.
    #[schemars(range(max = 7))]
    pub vcid_payload: u8,
    /// VC of OID frames.
    #[schemars(range(max = 7))]
    pub vcid_idle: u8,
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
    #[schemars(range(max = 1024))]
    pub max_frame_length: u16,
    /// FECF present.
    pub fecf: bool,
    /// The uplink VC.
    #[schemars(range(max = 63))]
    pub vcid: u8,
    /// MAP of real-time commands.
    #[schemars(range(max = 63))]
    pub map_commands: u8,
    /// MAP of the table upload.
    #[schemars(range(max = 63))]
    pub map_table_upload: u8,
    /// Maximum frame data unit length, octets.
    #[schemars(range(max = 1019))]
    pub max_fdu_length: u16,
    /// Maximum packet length, octets.
    pub max_packet_length: u16,
}

/// COP-1 parameters (§4.2, APP-044).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cop1 {
    /// FOP sliding window width K.
    #[schemars(range(min = 1, max = 255))]
    pub k: u8,
    /// FARM sliding window width W.
    #[schemars(range(min = 2, max = 254))]
    pub w: u8,
    /// `T1_Initial`, µs.
    pub t1_initial_us: u64,
    /// `Transmission_Limit`.
    #[schemars(range(min = 1))]
    pub transmission_limit: u8,
    /// `Timeout_Type` (0 or 1).
    #[schemars(range(max = 1))]
    pub timeout_type: u8,
    /// FARM-1 internal buffer depth (COP-032).
    #[schemars(range(min = 1))]
    pub farm_buffer_depth: u8,
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
        if self.link.frame_slot_us == 0 || self.link.uplink_slot_us == 0 {
            return Err("scenario.json: slot periods must be at least 1 µs".to_owned());
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
        }
        Ok(())
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
         /// Spacecraft ID of every frame (§3.4, APP-026); provisional and fictional until OI-26.\n\
         pub const SPACECRAFT_ID: u16 = {};",
        s.scenario,
        n(s.scenario_version.into()),
        n(s.spacecraft_id.into())
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
                n(s.time.cuc_coarse_octets.into()),
                "CUC coarse octets.",
            ),
            (
                "CUC_FINE_OCTETS",
                "u8",
                n(s.time.cuc_fine_octets.into()),
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
                n(s.link.frame_slot_us),
                "Downlink frame slot period, µs (APP-071).",
            ),
            (
                "UPLINK_SLOT_US",
                "u64",
                n(s.link.uplink_slot_us),
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
                n(a.housekeeping.into()),
                "Housekeeping.",
            ),
            ("EVENTS", "u16", n(a.events.into()), "Event reports."),
            ("PAYLOAD", "u16", n(a.payload.into()), "Payload science."),
            ("DUMP", "u16", n(a.dump.into()), "Memory dump."),
            ("COMMANDS", "u16", n(a.commands.into()), "Commands."),
            (
                "TABLE_UPLOAD",
                "u16",
                n(a.table_upload.into()),
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
                n(tm.frame_length.into()),
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
                n(tm.vcid_realtime.into()),
                "VC of housekeeping and events (APP-032).",
            ),
            (
                "VCID_PAYLOAD",
                "u8",
                n(tm.vcid_payload.into()),
                "VC of payload and dump (APP-032).",
            ),
            (
                "VCID_IDLE",
                "u8",
                n(tm.vcid_idle.into()),
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
                n(tc.max_frame_length.into()),
                "Maximum transfer frame length, octets (APP-040).",
            ),
            (
                "FECF",
                "bool",
                tc.fecf.to_string(),
                "FECF present (APP-040).",
            ),
            ("VCID", "u8", n(tc.vcid.into()), "The uplink VC (APP-041)."),
            (
                "MAP_COMMANDS",
                "u8",
                n(tc.map_commands.into()),
                "MAP of real-time commands (APP-041).",
            ),
            (
                "MAP_TABLE_UPLOAD",
                "u8",
                n(tc.map_table_upload.into()),
                "MAP of the table upload (APP-041).",
            ),
            (
                "MAX_FDU_LENGTH",
                "u16",
                n(tc.max_fdu_length.into()),
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
            ("K", "u8", n(c.k.into()), "FOP sliding window width K."),
            ("W", "u8", n(c.w.into()), "FARM sliding window width W."),
            (
                "T1_INITIAL_US",
                "u64",
                n(c.t1_initial_us),
                "`T1_Initial`, µs.",
            ),
            (
                "TRANSMISSION_LIMIT",
                "u8",
                n(c.transmission_limit.into()),
                "`Transmission_Limit`.",
            ),
            (
                "TIMEOUT_TYPE",
                "u8",
                n(c.timeout_type.into()),
                "`Timeout_Type`.",
            ),
            (
                "FARM_BUFFER_DEPTH",
                "u8",
                n(c.farm_buffer_depth.into()),
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
    use super::{Scenario, is_requirement_id, literal};

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
}
