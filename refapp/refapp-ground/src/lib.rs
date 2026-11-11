//! Ground node of the Aldis reference application.
//!
//! Specification: `docs/reference-application.md` §2.2 (applications) and §8.1 (step
//! contract). It may use `std`, but follows the flight lint set's restriction lints because it
//! parses untrusted downlink (APP-103). No `unsafe` (APP-105).
//!
//! | Module | Contents | Requirements |
//! |---|---|---|
//! | [`ops`] | The ops script: command and table-upload packets built from the scenario's entries | APP-011, APP-024, APP-126 |
//! | [`frames_down`] | Downlink frames: strict TM decoding, `FrameReceiver`, `PacketExtractor`, CLCW (to FOP-1 at `cop1`), frame events | APP-036, APP-138, APP-144 |
//! | `frames_up` (private) | The uplink at frame level: segmenter, FOP-1 (`RequestBd` at `frames`; at `cop1` `RequestAd`, CLCW, `Timeout`, directives, held transmit requests, events 200 to 203), TC frame | APP-040…APP-043, APP-044, APP-071, APP-090, APP-133, APP-143…APP-145, APP-148, APP-149; LNK-040…LNK-043; COP-010; TC-008 |
//! | [`telemetry`] | Strict decoding, sequence tracking, time stamps, dump reassembly, the application events | APP-011, APP-018, APP-130 |
//!
//! Per step the node handles the delivered units in delivery order, then releases the due ops
//! entries in script order (§6.2 step 2).
#![forbid(unsafe_code)]

use core::fmt;

use refapp_scenario::{Instant, Node, Output, StepInput};

pub mod frames_down;
mod frames_up;
pub mod ops;
pub mod telemetry;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_uplink;

pub use ops::{Call, Execution, Fop1Directive, OpsEntry, ScriptError};
pub use telemetry::TimeStamp;

use frames_down::FrameDownlink;
use frames_up::Uplink;
use ops::Script;
use telemetry::Telemetry;

/// Why a ground node cannot be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroundError {
    /// An ops-script entry cannot be encoded as a packet.
    Script(ScriptError),
    /// The telemetry applications cannot be set up.
    Setup(aldis::core::Error),
}

impl fmt::Display for GroundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GroundError::Script(e) => write!(f, "ops[{}]: {}", e.index, e.error),
            GroundError::Setup(e) => write!(f, "ground node setup: {e}"),
        }
    }
}

impl core::error::Error for GroundError {}

/// The ground node (APP-011).
#[derive(Debug, Clone)]
pub struct GroundNode {
    script: Script,
    telemetry: Telemetry,
    /// The frame-level downlink, from A2 on; `None` at the packet level (A1).
    downlink: Option<FrameDownlink>,
    /// The uplink at frame level (A2 on); `None` at packet level (A1).
    uplink: Option<Uplink>,
}

impl GroundNode {
    /// A ground node in its start-of-run state that releases `script`: entry `i` goes out when
    /// the harness passes index `i` in [`StepInput::ops`] (§8.1).
    ///
    /// # Errors
    /// [`GroundError::Script`] for the first entry that cannot be encoded.
    pub fn new(script: &[OpsEntry]) -> Result<GroundNode, GroundError> {
        Ok(GroundNode {
            script: Script::new(script).map_err(GroundError::Script)?,
            telemetry: Telemetry::new(),
            downlink: None,
            uplink: None,
        })
    }

    /// As [`new`](Self::new), for a case at the frame level (A2): the delivered units are TM
    /// frames (APP-036), the ops-script packets go to the `MapSegmenter` and not out, and in a
    /// step with `tx_slot` one FDU goes out as a Type-BD frame through FOP-1 (APP-133).
    ///
    /// # Errors
    /// As [`new`](Self::new), and [`GroundError::Setup`] if a scenario value is out of range for
    /// the engines.
    pub fn new_frames(script: &[OpsEntry]) -> Result<GroundNode, GroundError> {
        Ok(GroundNode {
            downlink: Some(FrameDownlink::new()?),
            uplink: Some(Uplink::new().map_err(GroundError::Setup)?),
            ..GroundNode::new(script)?
        })
    }

    /// As [`new_frames`](Self::new_frames), for a case at the `cop1` level (A3): FOP-1 runs the
    /// AD service (`RequestAd`, the CLCW of every delivered frame, `Timeout`, the directives of
    /// the script) and one Type-AD or Type-BC frame goes out in a step with `tx_slot`
    /// (APP-044, APP-144).
    ///
    /// # Errors
    /// As [`new_frames`](Self::new_frames).
    pub fn new_cop1(script: &[OpsEntry]) -> Result<GroundNode, GroundError> {
        Ok(GroundNode {
            uplink: Some(Uplink::new_cop1().map_err(GroundError::Setup)?),
            ..GroundNode::new_frames(script)?
        })
    }

    /// The CLCW of the last downlink frame accepted (reported as `clcw_changed`; at the `cop1`
    /// level also given to FOP-1), initially
    /// the initial CLCW of §4.4.1; `None` at the packet level or if that OCF is not a valid CLCW.
    #[must_use]
    pub fn last_clcw(&self) -> Option<aldis::tc::Clcw> {
        self.downlink.as_ref().and_then(FrameDownlink::clcw)
    }

    /// Uplink inputs the segmenter or FOP-1 refused; zero when the script fits their capacities.
    #[must_use]
    pub fn uplink_refused(&self) -> u32 {
        self.uplink.as_ref().map_or(0, Uplink::refused)
    }

    /// The time stamp of the last telemetry packet that was accepted; the node does not correct
    /// it (APP-062).
    #[must_use]
    pub const fn last_time_stamp(&self) -> Option<TimeStamp> {
        self.telemetry.last_time()
    }
}

impl Node for GroundNode {
    fn step(&mut self, input: &StepInput<'_>, out: &mut dyn Output) -> Option<Instant> {
        for unit in input.delivered {
            match &mut self.downlink {
                Some(downlink) => downlink.receive(
                    unit,
                    &mut self.telemetry,
                    self.uplink.as_mut(),
                    input.now,
                    out,
                ),
                None => self.telemetry.receive(unit, out),
            }
        }
        // Step 2 of §4.5.1: `Timeout` at FOP-1's deadline.
        if let Some(uplink) = &mut self.uplink {
            uplink.timeout(input.now, out);
        }
        for &index in input.ops {
            // The harness passes indices of the script the node was made with. A directive
            // entry has no packet, and is ignored below the `cop1` level (APP-019).
            let index = usize::from(index);
            if let Some(built) = self.script.packet(index) {
                out.packet_out(built.apid, &built.octets);
                match &mut self.uplink {
                    Some(uplink) => uplink.submit(built.apid, &built.octets, input.now, out),
                    None => out.unit(&built.octets),
                }
            }
            if let Some(directive) = self.script.directive(index)
                && let Some(uplink) = &mut self.uplink
            {
                uplink.directive(directive, input.now, out);
            }
        }
        // Step 3 of §2.2, step 4 of §4.5.1: the frame is the last output of the step (APP-133).
        if input.tx_slot
            && let Some(uplink) = &mut self.uplink
        {
            uplink.slot(input.now, out);
        }
        // FOP-1's deadline after the step is the node's own (APP-135).
        self.uplink.as_ref().and_then(Uplink::next_deadline)
    }
}
