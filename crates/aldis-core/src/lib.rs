//! Foundation types and shared algorithms for Aldis.
//!
//! Layer 0 of Aldis. Specification: `docs/standards/common-link-features.md`.
//!
//! **Status:** skeleton. Modules are declared but not yet implemented.
//! Requirements are traced by ID in each module's documentation.
#![no_std]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub mod bits;
pub mod container;
pub mod counter;
pub mod crc;
pub mod error;
pub mod ids;
pub mod layout;
pub mod oid;
pub mod security;
pub mod time;
