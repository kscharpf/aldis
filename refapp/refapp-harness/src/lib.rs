//! Harness of the Aldis reference application.
//!
//! Specification: `docs/reference-application.md`. The harness owns simulated time, the link,
//! faults, and the trace writer (§6, §7); the nodes are sans-I/O (§8). Tooling crate (`std`,
//! tooling lint profile, APP-104).
//!
//! | Module | Contents | Requirements |
//! |---|---|---|
//! | [`demo`] | The README demo: one text line per event of a golden trace | APP-154 |
//! | [`diagram`] | Mermaid sequence diagram of a golden trace | APP-153 |
//! | [`frame`] | Strict decoding of a link unit as a TM or TC frame | APP-086 |
//! | [`micros`] | Microseconds of simulated time | APP-107 |
//! | [`ranged`] | Integer with a closed range for parser and schema | APP-107 |
//! | [`scenario`] | Scenario file model, JSON Schema, generated constants | APP-107 |
//! | [`sim`] | Scheduler and link model | APP-070…APP-074 |
//! | [`trace`] | Golden-trace writer, canonical-form check, comparator | APP-080…APP-085 |
#![forbid(unsafe_code)]

pub mod demo;
pub mod diagram;
pub mod frame;
pub mod micros;
pub mod ranged;
pub mod scenario;
pub mod sim;
pub mod trace;

use std::path::PathBuf;

use refapp_flight::FlightNode;
use refapp_ground::GroundNode;

use crate::scenario::Scenario;

/// Runs a case with the Rust flight and ground nodes, in process, at the case's link level
/// (the frame-level nodes for `Level::Frames`, APP-071; the COP-1 nodes for `Level::Cop1`, APP-044).
///
/// # Errors
/// If the case does not exist, or a node breaks the step contract.
pub fn run_case(case_id: &str) -> Result<String, String> {
    let scenario = Scenario::load()?;
    let case = scenario.case(case_id)?;
    let level = case.level;
    let mut flight = match level {
        scenario::Level::Packets => Ok(FlightNode::new()),
        scenario::Level::Frames => FlightNode::new_frames(),
        scenario::Level::Cop1 => FlightNode::new_cop1(),
    }
    .map_err(|e| format!("case `{case_id}`: {e:?}"))?;
    let script = case
        .ops
        .iter()
        .map(scenario::OpsEntry::to_node)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("case `{case_id}`: {e}"))?;
    let mut ground = match level {
        scenario::Level::Packets => GroundNode::new(&script),
        scenario::Level::Frames => GroundNode::new_frames(&script),
        scenario::Level::Cop1 => GroundNode::new_cop1(&script),
    }
    .map_err(|e| format!("case `{case_id}`: {e}"))?;
    sim::run(&scenario, case, &mut flight, &mut ground)
}

/// Path of a case's golden trace, `refapp/golden/<case>.jsonl` (APP-087).
#[must_use]
pub fn golden_path(case_id: &str) -> PathBuf {
    scenario::refapp_dir()
        .join("golden")
        .join(format!("{case_id}.jsonl"))
}

/// The run manifest (§7.2): the implementation that produced a run, kept out of the trace
/// (APP-084).
#[must_use]
pub fn manifest(case_id: &str) -> String {
    let m = serde_json::json!({
        "case": case_id,
        "implementation": "rust",
        "nodes": "in-process",
        "refapp_harness": env!("CARGO_PKG_VERSION"),
    });
    m.to_string() + "\n"
}
