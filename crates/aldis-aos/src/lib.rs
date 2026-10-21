//! CCSDS 732.0-B-5 AOS Space Data Link Protocol frame codec.
//!
//! Layer 1 of Aldis. Specification: `docs/standards/aos-frame.md`.
//!
//! | Module | Contents | Clauses | Requirements |
//! |---|---|---|---|
//! | [`fhec`] | Frame Header Error Control: `encode`, `write`, `decode` with up to two symbol corrections, `CorrectedHeader`, `CorrectedSymbols` | 732.0 §4.1.2.6 | AOS-013, AOS-014 |
//!
//! **Status:** in progress (M2 WP-4). Only [`fhec`] is implemented; the other modules are
//! declared but not yet implemented. Requirements are traced by ID in each module's
//! documentation.
//!
//! `no_std`, no allocation, no `unsafe`, and no panics on any input (GEN-010, GEN-020,
//! GEN-070).
#![no_std]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
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

pub use fhec::{CorrectedHeader, CorrectedSymbols};
