//! Sans-I/O sending and receiving engines for CCSDS packets and frames.
//!
//! Layer 2 of Aldis. Specification: `docs/04-engines.md`.
//!
//! **Status:** skeleton. Modules are declared but not yet implemented.
//! Requirements are traced by ID in each module's documentation.
#![no_std]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub mod extractor;
pub mod generator;
pub mod mux;
pub mod policy;
pub mod reassembler;
pub mod receiver;
pub mod segmenter;
