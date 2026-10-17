//! CCSDS 732.1-B-3 Unified Space Data Link Protocol frame codec.
//!
//! Layer 1 of Aldis. Specification: `docs/standards/uslp-frame.md`.
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
pub mod config;
pub mod header;
pub mod tfdf;
pub mod truncated;
pub mod upid;
pub mod view;
pub mod writer;
