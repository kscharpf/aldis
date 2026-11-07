//! The ground node releases the ops scripts of the A1 cases through the harness (A1 WP-4;
//! docs/reference-application.md §6.5, §6.6).

use refapp_ground::GroundNode;
use refapp_harness::scenario::{Case, Scenario};
use refapp_harness::sim;
use refapp_scenario::{Instant, Node, Output, StepInput};

/// A flight node that does nothing.
struct Silent;

impl Node for Silent {
    fn step(&mut self, _: &StepInput<'_>, _: &mut dyn Output) -> Option<Instant> {
        None
    }
}

#[test]
fn every_a1_case_releases_its_script_at_its_instants() {
    // req: APP-011, APP-126
    let mut scenario = Scenario::load().unwrap();
    let cases: Vec<Case> = serde_json::from_str(include_str!("data/a1-cases.json")).unwrap();
    scenario.cases.extend(cases);
    for id in [
        "a1-nominal",
        "a1-immediate",
        "a1-rejects",
        "a1-downlink-faults",
    ] {
        // The silent flight node sends no unit for a fault to address.
        let mut case = scenario.case(id).unwrap().clone();
        case.faults.clear();
        let case = &case;
        let script: Vec<_> = case.ops.iter().map(|e| e.to_node().unwrap()).collect();
        let mut ground = GroundNode::new(&script).unwrap();
        let trace = sim::run(&scenario, case, &mut Silent, &mut ground).unwrap();
        let released: Vec<&str> = trace
            .lines()
            .filter(|l| l.contains("\"kind\":\"packet_out\""))
            .collect();
        // Every entry of these cases sends a packet: no FOP-1 directive is among them.
        assert_eq!(released.len(), case.ops.len(), "{id}: one packet per entry");
        for (line, entry) in released.iter().zip(&case.ops) {
            assert!(
                line.starts_with(&format!("{{\"t_us\":{},", entry.at_us())),
                "{id}: {line}"
            );
        }
        assert_eq!(
            trace.matches("\"kind\":\"unit_sent\"").count(),
            case.ops.len(),
            "{id}: each packet is also sent"
        );
    }
}
