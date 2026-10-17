//! CCSDS 232.0-B-4 TC Space Data Link Protocol frame codec and CLCW.
//!
//! Layer 1 of Aldis. Specification: `docs/standards/tc-frame.md`.
//!
//! **Status:** skeleton. Modules are declared but not yet implemented.
//! Requirements are traced by ID in each module's documentation.
#![no_std]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub mod anomaly;
pub mod clcw;
pub mod config;
pub mod control;
pub mod delimit;
pub mod header;
pub mod validate;
pub mod view;
pub mod writer;
