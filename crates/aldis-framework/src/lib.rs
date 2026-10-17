//! Optional framework: transport/clock traits and protocol stacks driving the engines.
//!
//! Layer 3 of Aldis. Specification: `docs/05-framework.md`.
//!
//! **Status:** skeleton. Modules are declared but not yet implemented.
//! Requirements are traced by ID in each module's documentation.
#![no_std]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[cfg(feature = "std")]
extern crate std;

pub mod adapters;
pub mod stacks;
pub mod traits;
