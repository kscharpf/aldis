//! Flight node of the Aldis reference application.
//!
//! Specification: `docs/reference-application.md` §2.1 (applications) and §8.1 (step
//! contract). `no_std`, no heap, flight lint set (APP-102), no `unsafe` (APP-105).
//!
//! **A0 skeleton:** the node has no applications yet; they arrive in A1.
#![no_std]
#![forbid(unsafe_code)]

use refapp_scenario::{Instant, Node, Output, StepInput};

/// The flight node (APP-010). Empty in the A0 skeleton.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FlightNode {}

impl FlightNode {
    /// A flight node in its start-of-run state.
    #[must_use]
    pub const fn new() -> FlightNode {
        FlightNode {}
    }
}

impl Node for FlightNode {
    fn step(&mut self, _input: &StepInput<'_>, _out: &mut dyn Output) -> Option<Instant> {
        None
    }
}
