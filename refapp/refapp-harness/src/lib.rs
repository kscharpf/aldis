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
//! | [`launch`] | Child-process and TCP transports of the node protocol, the cFS launcher | APP-092, APP-118 |
//! | [`proto`] | Messages of the node protocol (§8.2) | APP-092 |
//! | [`ranged`] | Integer with a closed range for parser and schema | APP-107 |
//! | [`scenario`] | Scenario file model, JSON Schema, generated constants | APP-107 |
//! | [`remote`] | The harness side of the node protocol: a node in another process | APP-092 |
//! | [`serve`] | The node side of the node protocol, for test nodes | APP-092 |
//! | [`sim`] | Scheduler and link model | APP-070…APP-074 |
//! | [`trace`] | Golden-trace writer, canonical-form check, comparator | APP-080…APP-085 |
#![forbid(unsafe_code)]

pub mod demo;
pub mod diagram;
pub mod frame;
pub mod launch;
pub mod micros;
pub mod proto;
pub mod ranged;
pub mod remote;
pub mod scenario;
pub mod serve;
pub mod sim;
pub mod trace;

use std::path::PathBuf;

use refapp_flight::FlightNode;
use refapp_ground::GroundNode;
use refapp_scenario::Node;

use crate::scenario::Scenario;

/// The Rust flight node for a case, at the case's link level (the frame-level node for
/// `Level::Frames`, APP-071; the COP-1 node for `Level::Cop1`, APP-044).
///
/// # Errors
/// If the node does not build.
pub fn flight_node(case: &scenario::Case) -> Result<FlightNode, String> {
    match case.level {
        scenario::Level::Packets => Ok(FlightNode::new()),
        scenario::Level::Frames => FlightNode::new_frames(),
        scenario::Level::Cop1 => FlightNode::new_cop1(),
    }
    .map_err(|e| format!("case `{}`: {e:?}", case.id))
}

/// The Rust ground node for a case, with the case's ops script.
///
/// # Errors
/// If an ops entry does not convert or the node does not build.
pub fn ground_node(case: &scenario::Case) -> Result<GroundNode, String> {
    let script = case
        .ops
        .iter()
        .map(scenario::OpsEntry::to_node)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("case `{}`: {e}", case.id))?;
    match case.level {
        scenario::Level::Packets => GroundNode::new(&script),
        scenario::Level::Frames => GroundNode::new_frames(&script),
        scenario::Level::Cop1 => GroundNode::new_cop1(&script),
    }
    .map_err(|e| format!("case `{}`: {e}", case.id))
}

/// Runs a case with the Rust flight and ground nodes, in process.
///
/// # Errors
/// If the case does not exist, or a node breaks the step contract.
pub fn run_case(case_id: &str) -> Result<String, String> {
    let scenario = Scenario::load()?;
    let mut flight = flight_node(scenario.case(case_id)?)?;
    run_case_with_flight(case_id, &mut flight)
}

/// Runs a case with the given flight node and the Rust ground node: the one run behind
/// [`run_case`] and the out-of-process runs of [`remote`] (APP-092).
///
/// # Errors
/// If the case does not exist, or a node breaks the step contract.
pub fn run_case_with_flight(case_id: &str, flight: &mut dyn Node) -> Result<String, String> {
    let scenario = Scenario::load()?;
    let case = scenario.case(case_id)?;
    let mut ground = ground_node(case)?;
    sim::run(&scenario, case, flight, &mut ground)
}

/// Path of a case's golden trace, `refapp/golden/<case>.jsonl` (APP-087).
#[must_use]
pub fn golden_path(case_id: &str) -> PathBuf {
    scenario::refapp_dir()
        .join("golden")
        .join(format!("{case_id}.jsonl"))
}

/// How a run reached its flight node (the run manifest's `transport`, §7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// In process: no protocol.
    InProcess,
    /// Node protocol over a child's stdin and stdout (§8.2).
    Stdio,
    /// Node protocol over loopback TCP (§10.6).
    Tcp,
}

/// The run manifest (§7.2) of any run, kept out of the trace (APP-084).
///
/// `components` names what the implementation is made of, such as the cFS
/// release and the `aldis-ffi` build (ACFS D13), in the order given.
#[must_use]
pub fn manifest_for(
    case_id: &str,
    implementation: &str,
    transport: Transport,
    components: &[(String, String)],
) -> String {
    let mut m = serde_json::json!({
        "case": case_id,
        "implementation": implementation,
        "nodes": match transport {
            Transport::InProcess => "in-process",
            Transport::Stdio | Transport::Tcp => "out-of-process",
        },
        "transport": match transport {
            Transport::InProcess => "none",
            Transport::Stdio => "stdio",
            Transport::Tcp => "tcp",
        },
        "refapp_harness": env!("CARGO_PKG_VERSION"),
    });
    if !components.is_empty() {
        let map: serde_json::Map<String, serde_json::Value> = components
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        m["components"] = serde_json::Value::Object(map);
    }
    m.to_string() + "\n"
}

/// The run manifest of an in-process run with the Rust nodes.
#[must_use]
pub fn manifest(case_id: &str) -> String {
    manifest_for(case_id, "rust", Transport::InProcess, &[])
}
