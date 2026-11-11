//! The one-command demo of the README (A3; APP-154).

use refapp_harness::demo::render;
use refapp_harness::diagram::CASE;
use refapp_harness::scenario::Scenario;
use refapp_harness::{golden_path, run_case};

fn demo(case: &str) -> Result<String, String> {
    render(&Scenario::load().unwrap(), &run_case(case)?)
}

#[test]
fn lossy_link_demo_has_one_line_per_event() {
    // req: APP-154
    let d = demo(CASE).unwrap();
    assert_eq!(d.lines().count(), 23, "{d}");
    assert!(d.contains(" 2.250 s  up    VC 0 AD N(S)=2  LOST\n"), "{d}");
    assert!(
        d.contains(" 2.625 s  down  VC 7 TM  CLCW N(R)=2 Retransmit\n"),
        "{d}"
    );
    assert!(
        d.contains(" 2.750 s  up    VC 0 AD N(S)=2 retransmission\n"),
        "{d}"
    );
    assert!(d.contains(" 4.125 s  down  7 frames LOST\n"), "{d}");
}

#[test]
fn demo_of_the_run_is_demo_of_the_golden_trace() {
    // req: APP-154
    let golden = std::fs::read_to_string(golden_path(CASE)).unwrap();
    let from_golden = render(&Scenario::load().unwrap(), &golden).unwrap();
    assert_eq!(demo(CASE).unwrap(), from_golden);
}

#[test]
fn a_case_without_frames_is_an_error() {
    // req: APP-154
    assert!(demo("a1-nominal").is_err());
}
