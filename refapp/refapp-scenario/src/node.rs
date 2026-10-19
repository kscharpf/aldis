//! The node interface between the harness and a node (docs/reference-application.md §8.1).
//!
//! A node is a sans-I/O state machine (APP-091): the harness calls [`Node::step`] with the
//! current instant and the step's inputs, and the node reports its outputs, in order, through
//! an [`Output`]. The harness owns time, the link, faults, and the trace (APP-070).

use aldis::core::ids::Apid;
use aldis::core::time::Instant;

/// Inputs of one step (§8.1).
#[derive(Debug, Clone, Copy)]
pub struct StepInput<'a> {
    /// The current simulated instant (APP-070).
    pub now: Instant,
    /// Link units delivered at `now`, in delivery order (§6.2 step 1).
    pub delivered: &'a [&'a [u8]],
    /// Indices of the ops-script entries due at `now`, in script order (ground node only).
    pub ops: &'a [u16],
    /// Whether the link takes a unit in this step (§8.1 `tx_slot` rules, APP-090).
    pub tx_slot: bool,
}

/// Receives a node's outputs in output order (§7.3, §8.1).
pub trait Output {
    /// The node sends a link unit (recorded as `unit_sent`).
    fn unit(&mut self, octets: &[u8]);
    /// An application of the node generated a packet (`packet_out`).
    fn packet_out(&mut self, apid: Apid, octets: &[u8]);
    /// A packet was delivered to an application of the node (`packet_in`).
    fn packet_in(&mut self, apid: Apid, octets: &[u8]);
    /// The node raises an application event of §7.4 (`app_event`).
    fn app_event(&mut self, code: u16, args: &[u64]);
}

/// A flight or ground node (APP-090).
pub trait Node {
    /// Performs one step and returns the earliest instant at which the node needs another step,
    /// or `None` (API-053). Stepping a node at an instant where nothing is due for it SHALL NOT
    /// change its outputs (APP-074).
    fn step(&mut self, input: &StepInput<'_>, out: &mut dyn Output) -> Option<Instant>;
}
