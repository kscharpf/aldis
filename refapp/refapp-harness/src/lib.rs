//! Harness of the Aldis reference application.
//!
//! Specification: `docs/reference-application.md`. The harness owns simulated time, the link,
//! faults, and the trace writer (§6, §7); the nodes are sans-I/O (§8). Tooling crate (`std`,
//! tooling lint profile, APP-104).
//!
//! | Module | Contents | Requirements |
//! |---|---|---|
//! | [`scenario`] | Scenario file model, JSON Schema, generated constants | APP-107 |
//! | [`sim`] | Scheduler and link model | APP-070…APP-074 |
//! | [`trace`] | Golden-trace writer, canonical-form check, comparator | APP-080…APP-085 |
#![forbid(unsafe_code)]

pub mod scenario;
pub mod sim;
pub mod trace;

use std::path::PathBuf;

use refapp_flight::FlightNode;
use refapp_ground::GroundNode;

use crate::scenario::Scenario;

/// Runs a case with the Rust flight and ground nodes, in process.
///
/// # Errors
/// If the case does not exist or a node breaks the step contract.
pub fn run_case(case_id: &str) -> Result<String, String> {
    let scenario = Scenario::load()?;
    let case = scenario.case(case_id)?;
    let mut flight = FlightNode::new();
    let mut ground = GroundNode::new();
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
