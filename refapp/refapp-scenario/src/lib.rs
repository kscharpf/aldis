//! Scenario values and node interface of the Aldis reference application.
//!
//! Specification: `docs/reference-application.md`. The values in [`generated`] come from
//! `refapp/scenario/scenario.json`, the one language-neutral scenario file (APP-107); the
//! [`node`] module defines the step contract every node implements (APP-090).
//!
//! `no_std`, because the flight node uses it (APP-102).
#![no_std]
#![forbid(unsafe_code)]

pub mod generated;
pub mod node;

pub use aldis::core::ids::Apid;
pub use aldis::core::time::Instant;
pub use node::{Node, Output, StepInput};
