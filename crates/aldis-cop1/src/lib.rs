//! CCSDS 232.1-B-2 COP-1 (FOP-1 and FARM-1) as sans-I/O engines.
//!
//! Layer 2 of Aldis. Specification: `docs/standards/cop1.md`.
//!
//! **Status:** skeleton. Modules are declared but not yet implemented.
//! Requirements are traced by ID in each module's documentation.
#![no_std]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[cfg(feature = "std")]
extern crate std;

pub mod farm1;
pub mod farm1_table;
pub mod fop1;
pub mod fop1_table;
pub mod notify;
