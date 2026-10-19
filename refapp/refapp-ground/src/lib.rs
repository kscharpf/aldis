//! Ground node of the Aldis reference application.
//!
//! Specification: `docs/reference-application.md` §2.2 (applications) and §8.1 (step
//! contract). It may use `std`, but follows the flight lint set's restriction lints because it
//! parses untrusted downlink (APP-103). No `unsafe` (APP-105).
//!
//! **A0 skeleton:** the node has no applications yet; they arrive in A1.
#![forbid(unsafe_code)]

use refapp_scenario::{Instant, Node, Output, StepInput};

/// The ground node (APP-011). Empty in the A0 skeleton.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GroundNode {}

impl GroundNode {
    /// A ground node in its start-of-run state.
    #[must_use]
    pub const fn new() -> GroundNode {
        GroundNode {}
    }
}

impl Node for GroundNode {
    fn step(&mut self, _input: &StepInput<'_>, _out: &mut dyn Output) -> Option<Instant> {
        None
    }
}
