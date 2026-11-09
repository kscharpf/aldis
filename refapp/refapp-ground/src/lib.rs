//! Ground node of the Aldis reference application.
//!
//! Specification: `docs/reference-application.md` §2.2 (applications) and §8.1 (step
//! contract). It may use `std`, but follows the flight lint set's restriction lints because it
//! parses untrusted downlink (APP-103). No `unsafe` (APP-105).
//!
//! | Module | Contents | Requirements |
//! |---|---|---|
//! | [`ops`] | The ops script: command and table-upload packets built from the scenario's entries | APP-011, APP-024, APP-126 |
//! | [`frames_down`] | Downlink frames: strict TM decoding, `FrameReceiver`, `PacketExtractor`, CLCW, frame events | APP-036, APP-138 |
//! | [`telemetry`] | Strict decoding, sequence tracking, time stamps, dump reassembly, the application events | APP-011, APP-018, APP-130 |
//!
//! Per step the node handles the delivered units in delivery order, then releases the due ops
//! entries in script order (§6.2 step 2).
#![forbid(unsafe_code)]

use core::fmt;

use refapp_scenario::{Instant, Node, Output, StepInput};

pub mod frames_down;
pub mod ops;
pub mod telemetry;

#[cfg(test)]
mod tests;

pub use ops::{Call, Execution, OpsEntry, ScriptError};
pub use telemetry::TimeStamp;

use frames_down::FrameDownlink;
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
        })
    }

    /// As [`new`](Self::new), for a case at the frame level: the delivered units are TM frames
    /// (A2, APP-036).
    ///
    /// # Errors
    /// As [`new`](Self::new), and [`GroundError::Setup`] if the TM parameters of the scenario do
    /// not form a configuration.
    pub fn new_frames(script: &[OpsEntry]) -> Result<GroundNode, GroundError> {
        Ok(GroundNode {
            downlink: Some(FrameDownlink::new()?),
            ..GroundNode::new(script)?
        })
    }

    /// The CLCW of the last downlink frame accepted (D2: reported, not given to FOP-1), initially
    /// the initial CLCW of §4.4.1; `None` at the packet level or if that OCF is not a valid CLCW.
    #[must_use]
    pub fn last_clcw(&self) -> Option<aldis::tc::Clcw> {
        self.downlink.as_ref().and_then(FrameDownlink::clcw)
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
                Some(downlink) => downlink.receive(unit, &mut self.telemetry, out),
                None => self.telemetry.receive(unit, out),
            }
        }
        for &index in input.ops {
            // The harness passes indices of the script the node was made with. An entry that
            // sends nothing at this link level (a FOP-1 directive, APP-019) has no packet.
            if let Some(built) = self.script.packet(usize::from(index)) {
                out.packet_out(built.apid, &built.octets);
                out.unit(&built.octets);
            }
        }
        None
    }
}
