//! The harness and the variants (docs/reference-application.md §4.3, §6.5; A4 WP-2): the node pair
//! is built for the case's variant.

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
    // `uslp` exists at `cop1` only (APP-187). The flight node (A4 WP-3c) and the ground node
    // (A4 WP-4c) both build.
    let c = case(Level::Cop1, Variant::Uslp);
    assert!(flight_node(&c).is_ok(), "cop1/uslp");
    assert!(ground_node(&c).is_ok(), "cop1/uslp");
}

#[test]
fn the_shipped_cases_all_build_a_node_pair_for_their_variant() {
    // req: APP-187, APP-189
    // The cases of §6.4 have the variant of their name: `aos` and `uslp` for the two `a4-*`
    // cases, `tm_tc` for every other. Every case builds a node pair, and `run_case` still
    // reproduces `a0-empty`.
    let s = Scenario::load().unwrap();
    for c in &s.cases {
        let want = match c.id.as_str() {
            "a4-aos-nominal" => Variant::Aos,
            "a4-uslp-nominal" => Variant::Uslp,
            _ => Variant::TmTc,
        };
        assert_eq!(c.variant, want, "{}", c.id);
        assert!(flight_node(c).is_ok() && ground_node(c).is_ok(), "{}", c.id);
    }
    assert!(run_case("a0-empty").is_ok());
}
