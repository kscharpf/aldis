//! CCSDS 301.0-B-4 Time Code Formats (CUC, CDS, CCS, ASCII).
//!
//! Layer 1 of Aldis. Specification: `docs/standards/time-codes.md`.
//!
//! **Status:** skeleton. Modules are declared but not yet implemented.
//! Requirements are traced by ID in each module's documentation.
#![no_std]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub mod ascii;
pub mod ccs;
pub mod cds;
pub mod cuc;
pub mod pfield;
