//! CCSDS 133.0-B-2 Space Packet Protocol codec.
//!
//! Layer 1 of Aldis. Specification: `docs/standards/space-packet.md`.
//!
//! **Status:** skeleton. Modules are declared but not yet implemented.
//! Requirements are traced by ID in each module's documentation.
#![no_std]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub mod anomaly;
pub mod header;
pub mod idle;
pub mod segment;
pub mod sequence;
pub mod view;
pub mod writer;
