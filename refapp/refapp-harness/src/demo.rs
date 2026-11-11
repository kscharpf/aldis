//! The one-command demo of the README (APP-154): a run of `a3-uplink-loss` as one text line per
//! event.
//!
//! The lines come from [`crate::diagram::rows`], so every figure is decoded from the trace with
//! the strict decoders of [`crate::frame`] (APP-086), not read from the bytes here. The printing
//! is in the harness binary; the nodes stay sans-I/O (APP-104).

use std::fmt::Write as _;

use crate::diagram::{Row, rows};
use crate::scenario::Scenario;

/// The lines of the demo for the trace `text`, each ending in a newline.
///
/// A line holds the instant, the direction, the virtual channel and frame type, N(S) or the CLCW,
/// and whether the link lost the frame or the ground node sent it again.
///
/// # Errors
/// As [`crate::diagram::render`].
pub fn render(scenario: &Scenario, text: &str) -> Result<String, String> {
    let mut out = String::new();
    for row in rows(scenario, text)? {
        match row {
            Row::Up { at, label, lost } => {
                let lost = if lost { "  LOST" } else { "" };
                writeln!(out, "{at:>8}  up    {label}{lost}")
            }
            Row::Report { at, vc, label } => writeln!(out, "{at:>8}  down  VC {vc} TM  {label}"),
            Row::DownLost { at, frames } => {
                let noun = if frames == 1 { "frame" } else { "frames" };
                writeln!(out, "{at:>8}  down  {frames} {noun} LOST")
            }
        }
        .map_err(|e| e.to_string())?;
    }
    Ok(out)
}
