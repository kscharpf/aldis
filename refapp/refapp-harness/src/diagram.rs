//! Mermaid sequence diagram of a golden trace (APP-153).
//!
//! The README shows the `a3-uplink-loss` run as a `sequenceDiagram` between the ground and the
//! flight node. Every figure in it is decoded from the trace with the strict decoders of
//! [`crate::frame`] (APP-086), never read from the bytes by hand. `cargo xtask readme-diagram`
//! splices the text into the README and checks it is current.
//!
//! Rows, in trace order:
//!
//! * every uplink frame (`unit_sent`, `up`) as an arrow from the ground to the flight node,
//!   labelled with its instant, virtual channel, frame type, and, for a Type-AD frame, its Frame
//!   Sequence Number N(S) (232.0 §4.1.2.8); a frame a `drop` fault removes ends in a cross, and
//!   a Type-AD frame whose octets were sent before is marked as a retransmission;
//! * the CLCW of the downlink frames that arrived (`unit_delivered`, `down`), one arrow whenever
//!   the Report Value N(R), the Retransmit, Lockout, or Wait flag differs from the CLCW before
//!   it (232.0 §4.2.1);
//! * one dashed arrow ending in a cross for each run of consecutive downlink frames a `drop`
//!   fault removes, naming how many were lost.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use aldis::tc::{Clcw, ControlCommand, FrameType};
use serde_json::Value;

use crate::frame::{DecodedFrame, decode_frame};
use crate::scenario::Scenario;
use crate::trace::{Dir, check_canonical, unhex};

/// The case the README shows (the COP-1 retransmission over a lossy link, A3).
pub const CASE: &str = "a3-uplink-loss";

/// The fields of a CLCW that the diagram reports; an arrow is drawn when they change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Report {
    n_r: u8,
    retransmit: bool,
    lockout: bool,
    wait: bool,
}

impl Report {
    const fn of(clcw: &Clcw) -> Report {
        Report {
            n_r: clcw.report_value.get(),
            retransmit: clcw.retransmit,
            lockout: clcw.lockout,
            wait: clcw.wait,
        }
    }

    fn label(self) -> String {
        let mut s = format!("CLCW N(R)={}", self.n_r);
        for (flag, name) in [
            (self.retransmit, "Retransmit"),
            (self.lockout, "Lockout"),
            (self.wait, "Wait"),
        ] {
            if flag {
                s.push(' ');
                s.push_str(name);
            }
        }
        s
    }
}

/// An instant as seconds with at least three decimals, for example `2.250 s`.
fn seconds(t_us: u64) -> String {
    let fraction = format!("{:06}", t_us % 1_000_000);
    let kept = fraction.trim_end_matches('0');
    let kept = if kept.len() < 3 { &fraction[..3] } else { kept };
    format!("{}.{kept} s", t_us / 1_000_000)
}

/// The label of an uplink frame: virtual channel, frame type, and what the type adds.
fn uplink_label(frame: &DecodedFrame, retransmission: bool) -> Result<String, String> {
    let kind = match frame.frame_type {
        Some(FrameType::Ad) => format!("AD N(S)={}", frame.sequence.unwrap_or_default()),
        Some(FrameType::Bd) => "BD".to_owned(),
        Some(FrameType::Bc) => match frame.control_command {
            Some(ControlCommand::Unlock) => "BC Unlock".to_owned(),
            Some(ControlCommand::SetVr(vr)) => format!("BC Set V(R)={}", vr.get()),
            None => return Err("a Type-BC frame without a Control Command".to_owned()),
        },
        None => return Err("an uplink frame without a frame type".to_owned()),
    };
    let again = if retransmission {
        " retransmission"
    } else {
        ""
    };
    Ok(format!("VC {} {kind}{again}", frame.vcid))
}

fn field(v: &Value, key: &str) -> Result<u64, String> {
    v[key]
        .as_u64()
        .ok_or_else(|| format!("record without an integer `{key}`: {v}"))
}

fn dir_of(v: &Value) -> Option<Dir> {
    match v["dir"].as_str() {
        Some("down") => Some(Dir::Down),
        Some("up") => Some(Dir::Up),
        _ => None,
    }
}

/// The Mermaid block (a fenced `mermaid` code block, ending in a newline) of the trace `text`
/// of a case at a link level with frames (APP-153).
///
/// # Errors
/// If `text` is not a canonical trace (APP-080), its case is not at a link level with frames,
/// or a frame does not decode strictly.
pub fn render(scenario: &Scenario, text: &str) -> Result<String, String> {
    check_canonical(text)?;
    let mut lines = text.split_terminator('\n');
    let header: Value = serde_json::from_str(lines.next().unwrap_or_default())
        .map_err(|e| format!("line 1: {e}"))?;
    let case_id = header["case"]
        .as_str()
        .ok_or("line 1: header without a case")?;
    let level = scenario.case(case_id)?.level;
    if !level.has_frames() {
        return Err(format!("case `{case_id}` has no frames to draw"));
    }
    let records = lines
        .enumerate()
        .map(|(i, l)| serde_json::from_str::<Value>(l).map_err(|e| format!("line {}: {e}", i + 2)))
        .collect::<Result<Vec<_>, _>>()?;

    let mut dropped: BTreeSet<(&str, u64)> = BTreeSet::new();
    let mut delivered: BTreeSet<(&str, u64)> = BTreeSet::new();
    for r in &records {
        let (Some(dir), Some(kind)) = (r["dir"].as_str(), r["kind"].as_str()) else {
            continue;
        };
        match (kind, r["fault"].as_str()) {
            ("fault", Some("drop")) => dropped.insert((dir, field(r, "index")?)),
            ("unit_delivered", _) => delivered.insert((dir, field(r, "index")?)),
            _ => false,
        };
    }

    // The first index of each run of consecutive dropped downlink frames, and the run's length.
    let mut runs: BTreeMap<u64, u64> = BTreeMap::new();
    let mut run_start: Option<u64> = None;
    for &(dir, index) in &dropped {
        if dir != "down" {
            continue;
        }
        match run_start {
            Some(start) if runs.get(&start).is_some_and(|n| start + n == index) => {
                runs.entry(start).and_modify(|n| *n += 1);
            }
            _ => {
                runs.insert(index, 1);
                run_start = Some(index);
            }
        }
    }

    let mut out = String::from(
        "```mermaid\nsequenceDiagram\n    participant G as Ground\n    participant F as Flight\n",
    );
    let mut sent_ad: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
    let mut last_report: Option<Report> = None;
    for r in &records {
        if r["kind"].as_str() != Some("unit_sent") {
            continue;
        }
        let (Some(dir), Some(dir_name)) = (dir_of(r), r["dir"].as_str()) else {
            return Err(format!("unit_sent without a direction: {r}"));
        };
        let index = field(r, "index")?;
        let at = seconds(field(r, "t_us")?);
        let octets = unhex(r["hex"].as_str().ok_or("unit_sent without hex")?)?;
        match dir {
            Dir::Up => {
                let frame = decode_frame(scenario, level, dir, &octets)
                    .map_err(|e| format!("uplink unit {index}: {e}"))?;
                let retransmission = match (frame.frame_type, frame.sequence) {
                    (Some(FrameType::Ad), Some(n)) => {
                        sent_ad.insert(n, octets.clone()).as_ref() == Some(&octets)
                    }
                    _ => false,
                };
                let label = uplink_label(&frame, retransmission)?;
                let (arrow, lost) = if dropped.contains(&(dir_name, index)) {
                    ("-x", " (lost)")
                } else {
                    ("->>", "")
                };
                writeln!(out, "    G{arrow}F: {at}  {label}{lost}").map_err(|e| e.to_string())?;
            }
            Dir::Down => {
                if !delivered.contains(&(dir_name, index)) {
                    if let Some(n) = runs.get(&index) {
                        let frames = if *n == 1 { "frame" } else { "frames" };
                        writeln!(out, "    F--xG: {at}  {n} downlink {frames} lost")
                            .map_err(|e| e.to_string())?;
                    }
                    continue;
                }
                let frame = decode_frame(scenario, level, dir, &octets)
                    .map_err(|e| format!("downlink unit {index}: {e}"))?;
                let report = frame.clcw.as_ref().map(Report::of);
                if report.is_some() && report != last_report {
                    if let Some(report) = report {
                        writeln!(out, "    F-->>G: {at}  {}", report.label())
                            .map_err(|e| e.to_string())?;
                    }
                    last_report = report;
                }
            }
        }
    }
    out.push_str("```\n");
    Ok(out)
}
