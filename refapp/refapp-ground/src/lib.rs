//! Ground node of the Aldis reference application.
//!
//! Specification: `docs/reference-application.md` §2.2 (applications) and §8.1 (step
//! contract). It may use `std`, but follows the flight lint set's restriction lints because it
//! parses untrusted downlink (APP-103). No `unsafe` (APP-105).
//!
//! | Module | Contents | Requirements |
//! |---|---|---|
//! | [`ops`] | The ops script: command and table-upload packets built from the scenario's entries | APP-011, APP-024, APP-126 |
//! | `frames_up` (private) | The uplink at frame level: segmenter, FOP-1 `RequestBd`, TC frame | APP-040…APP-043, APP-071, APP-090, APP-133, APP-143; LNK-040…LNK-043; COP-010; TC-008 |
//! | [`telemetry`] | Strict decoding, sequence tracking, time stamps, dump reassembly, the application events | APP-011, APP-018, APP-130 |
//!
//! Per step the node handles the delivered units in delivery order, then releases the due ops
//! entries in script order (§6.2 step 2).
#![forbid(unsafe_code)]

use core::fmt;

use refapp_scenario::{Instant, Node, Output, StepInput};

mod frames_up;
pub mod ops;
pub mod telemetry;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_uplink;

pub use ops::{Call, Execution, OpsEntry, ScriptError};
pub use telemetry::TimeStamp;

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
            uplink: None,
        })
    }

    /// The same node with the uplink at frame level (A2, APP-133): the ops-script packets go to
    /// the `MapSegmenter` and not out, and in a step with `tx_slot` one FDU goes out as a Type-BD
    /// frame through FOP-1.
    ///
    /// # Errors
    /// [`GroundError::Setup`] if a scenario value is out of range for the engines.
    pub fn with_frame_uplink(mut self) -> Result<GroundNode, GroundError> {
        self.uplink = Some(Uplink::new().map_err(GroundError::Setup)?);
        Ok(self)
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
            self.telemetry.receive(unit, out);
        }
        for &index in input.ops {
            // The harness passes indices of the script the node was made with. An entry that
            // sends nothing at this link level (a FOP-1 directive, APP-019) has no packet.
            if let Some(built) = self.script.packet(usize::from(index)) {
                out.packet_out(built.apid, &built.octets);
                match &mut self.uplink {
                    Some(uplink) => uplink.submit(built.apid, &built.octets, input.now, out),
                    None => out.unit(&built.octets),
                }
            }
        }
        // Step 3 of §2.2: the frame is the last output of the step (APP-133).
        if input.tx_slot
            && let Some(uplink) = &mut self.uplink
            && let Some(frame) = uplink.slot(input.now, out)
        {
            out.unit(&frame);
        }
        None
    }
}
