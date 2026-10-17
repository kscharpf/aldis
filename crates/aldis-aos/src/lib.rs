//! CCSDS 732.0-B-5 AOS Space Data Link Protocol frame codec.
//!
//! Layer 1 of Aldis. Specification: `docs/standards/aos-frame.md`.
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
pub mod bpdu;
pub mod config;
pub mod fhec;
pub mod header;
pub mod mpdu;
pub mod view;
pub mod writer;
