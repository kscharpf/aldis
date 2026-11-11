//! The Mermaid sequence diagram of a golden trace (A3; APP-153).

use refapp_harness::diagram::{CASE, render};
use refapp_harness::scenario::Scenario;
use refapp_harness::{golden_path, run_case};

fn diagram(case: &str) -> Result<String, String> {
    let text = std::fs::read_to_string(golden_path(case)).unwrap();
    render(&Scenario::load().unwrap(), &text)
}

#[test]
fn a_case_without_frames_is_an_error() {
    // req: APP-153
    assert!(render(&Scenario::load().unwrap(), &run_case("a1-nominal").unwrap()).is_err());
}

#[test]
fn lossy_link_diagram_is_the_decoded_trace() {
    // req: APP-153
    let expected = r"```mermaid
sequenceDiagram
    participant G as Ground
    participant F as Flight
    F-->>G: 0.000 s  CLCW N(R)=0
    G->>F: 1.000 s  VC 0 AD N(S)=0
    F-->>G: 1.125 s  CLCW N(R)=1
    G->>F: 2.000 s  VC 0 AD N(S)=1
    F-->>G: 2.125 s  CLCW N(R)=2
    G-xF: 2.250 s  VC 0 AD N(S)=2 (lost)
    G->>F: 2.500 s  VC 0 AD N(S)=3
    F-->>G: 2.625 s  CLCW N(R)=2 Retransmit
    G->>F: 2.750 s  VC 0 AD N(S)=2 retransmission
    F-->>G: 2.875 s  CLCW N(R)=3
    G->>F: 3.000 s  VC 0 AD N(S)=3 retransmission
    F-->>G: 3.125 s  CLCW N(R)=4
    G->>F: 3.250 s  VC 0 AD N(S)=4
    F-->>G: 3.375 s  CLCW N(R)=5
    G->>F: 4.000 s  VC 0 AD N(S)=5
    F--xG: 4.125 s  7 downlink frames lost
    G->>F: 5.000 s  VC 0 AD N(S)=5 retransmission
    F-->>G: 5.000 s  CLCW N(R)=6
    G->>F: 5.250 s  VC 0 AD N(S)=6
    F-->>G: 5.375 s  CLCW N(R)=7
    G-xF: 6.000 s  VC 0 AD N(S)=7 (lost)
    G->>F: 7.000 s  VC 0 AD N(S)=7 retransmission
    F-->>G: 7.125 s  CLCW N(R)=8
```
";
    assert_eq!(CASE, "a3-uplink-loss");
    assert_eq!(diagram(CASE).unwrap(), expected);
}

#[test]
fn control_command_and_lockout_are_labelled() {
    // req: APP-153
    let d = diagram("a3-uplink-lockout").unwrap();
    assert!(d.contains("VC 0 BC Unlock"), "{d}");
    assert!(d.contains("CLCW N(R)=1 Lockout"), "{d}");
    assert!(d.contains("1 downlink frame lost"), "{d}");
}

#[test]
fn an_unchanging_clcw_is_drawn_once() {
    // req: APP-153
    let d = diagram("a2-nominal").unwrap();
    assert!(d.contains("VC 0 BD"), "{d}");
    assert_eq!(d.matches("CLCW").count(), 1, "{d}");
}

#[test]
fn packet_level_trace_is_refused() {
    // req: APP-153
    let e = diagram("a1-nominal").unwrap_err();
    assert!(e.contains("no frames to draw"), "{e}");
}

#[test]
fn a_frame_that_fails_the_strict_decoder_is_refused() {
    // req: APP-153
    let text = std::fs::read_to_string(golden_path("a2-nominal")).unwrap();
    // Flip one octet of the first uplink frame: the FECF no longer matches.
    let line = text
        .lines()
        .find(|l| l.contains("\"dir\":\"up\"") && l.contains("unit_sent"));
    let line = line.unwrap();
    let hex = line.split("\"hex\":\"").nth(1).unwrap();
    let flipped = format!(
        "{}{}",
        if hex.starts_with('0') { "1" } else { "0" },
        &hex[1..]
    );
    let bad = text.replacen(hex, &flipped, 1);
    let e = render(&Scenario::load().unwrap(), &bad).unwrap_err();
    assert!(e.contains("uplink unit"), "{e}");
}

#[test]
fn separate_runs_of_lost_downlink_frames_are_drawn_apart() {
    // req: APP-153
    // The drops at downlink indexes 36, 92, and 99 are three runs of one frame, not one run.
    let d = diagram("a2-downlink-loss").unwrap();
    assert_eq!(d.matches("F--xG").count(), 3, "{d}");
    assert_eq!(d.matches("1 downlink frame lost").count(), 3, "{d}");
    assert!(!d.contains("frames lost"), "{d}");
}

#[test]
fn only_a_dropped_uplink_frame_is_drawn_lost() {
    // req: APP-153
    // a2-uplink-faults drops index 2, flips index 0 (it arrives, and the FECF rejects it), and
    // duplicates index 6 (it arrives twice): one cross among eight uplink frames.
    let d = diagram("a2-uplink-faults").unwrap();
    assert_eq!(d.matches("G-xF").count(), 1, "{d}");
    assert_eq!(d.matches("G->>F").count(), 7, "{d}");
    assert!(d.contains("G-xF: 2.250 s"), "{d}");
}

#[test]
fn an_uplink_frame_the_link_did_not_deliver_is_drawn_lost() {
    // req: APP-153
    // Remove the delivery record of the first uplink frame: it is drawn lost, whatever the cause.
    let text = std::fs::read_to_string(golden_path("a2-nominal")).unwrap();
    let kept: Vec<&str> = text
        .lines()
        .filter(|l| {
            !(l.contains("unit_delivered")
                && l.contains("\"dir\":\"up\"")
                && l.contains("\"index\":0,"))
        })
        .collect();
    assert_eq!(kept.len() + 1, text.lines().count());
    let d = render(&Scenario::load().unwrap(), &(kept.join("\n") + "\n")).unwrap();
    assert_eq!(d.matches("G-xF").count(), 1, "{d}");
    assert!(d.contains("G-xF: 1.000 s"), "{d}");
}
