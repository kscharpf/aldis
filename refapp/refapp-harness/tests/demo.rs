//! The one-command demo of the README (A3; APP-154).

use refapp_harness::demo::{render, verdict};
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
fn a_run_equal_to_the_golden_trace_passes() {
    // req: APP-154
    let golden = std::fs::read_to_string(golden_path(CASE)).unwrap();
    let (text, ok) = verdict(CASE, &golden, &run_case(CASE).unwrap());
    assert!(ok, "{text}");
    assert_eq!(
        text,
        "verdict: the run matches refapp/golden/a3-uplink-loss.jsonl"
    );
}

#[test]
fn a_run_that_differs_from_the_golden_trace_fails() {
    // req: APP-154
    let run = run_case(CASE).unwrap();
    let changed = run.replacen('"', "'", 1);
    let (text, ok) = verdict(CASE, &changed, &run);
    assert!(!ok, "{text}");
    assert!(
        text.starts_with("verdict: the run differs from refapp/golden/a3-uplink-loss.jsonl\n"),
        "{text}"
    );
    assert!(text.contains("first difference at line 1"), "{text}");
}
