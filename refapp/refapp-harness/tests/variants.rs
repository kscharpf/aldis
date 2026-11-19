//! The harness and the variants (docs/reference-application.md §4.3, §6.5; A4 WP-2): the node pair
//! is built for the case's variant, and a variant whose node does not exist yet is an error.

use refapp_harness::micros::Micros;
use refapp_harness::ranged::Ranged;
use refapp_harness::scenario::{Case, Level, Scenario, Variant};
use refapp_harness::{flight_node, ground_node, run_case};

fn case(level: Level, variant: Variant) -> Case {
    Case {
        id: "test".to_owned(),
        level,
        variant,
        end_us: Micros::new(0),
        trace_version: Ranged::new(1).unwrap(),
        requirements: vec![],
        ops: vec![],
        faults: vec![],
    }
}

#[test]
fn the_node_pair_is_built_for_the_variant_of_the_case() {
    // req: APP-187
    // `tm_tc` builds at every level, as before the variants.
    for level in [Level::Packets, Level::Frames, Level::Cop1] {
        let c = case(level, Variant::TmTc);
        assert!(flight_node(&c).is_ok(), "{level:?}");
        assert!(ground_node(&c).is_ok(), "{level:?}");
    }
    // The flight node (A4 WP-3a) and the ground node (A4 WP-4a) of `aos` build.
    for level in [Level::Frames, Level::Cop1] {
        let c = case(level, Variant::Aos);
        assert!(flight_node(&c).is_ok(), "{level:?}/aos");
        assert!(ground_node(&c).is_ok(), "{level:?}/aos");
    }
    // `uslp` has no node until its work packages: an error that names the case and the variant,
    // from the flight node and from the ground node.
    for (level, variant) in [(Level::Cop1, Variant::Uslp)] {
        let c = case(level, variant);
        let flight = flight_node(&c).unwrap_err();
        let ground = ground_node(&c).unwrap_err();
        for e in [flight, ground] {
            assert!(
                e.contains("case `test`") && e.contains(&format!("`{variant}`")),
                "{level:?}/{variant}: {e}"
            );
        }
    }
}

#[test]
fn the_shipped_cases_all_run_with_the_nodes_of_tm_tc() {
    // req: APP-187, APP-189
    // The scenario holds the sections of the variants and no case of them yet (A4 WP-5): every
    // case builds a node pair, and `run_case` still reproduces `a0-empty`.
    let s = Scenario::load().unwrap();
    for c in &s.cases {
        assert_eq!(c.variant, Variant::TmTc, "{}", c.id);
        assert!(flight_node(c).is_ok() && ground_node(c).is_ok(), "{}", c.id);
    }
    assert!(run_case("a0-empty").is_ok());
}
