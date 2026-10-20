//! Foundation types and shared algorithms for Aldis.
//!
//! Layer 0 of Aldis. Specification: `docs/standards/common-link-features.md`,
//! `docs/02-design-rules.md`, and `docs/03-api-model.md`.
//!
//! | Module | Contents | Requirements |
//! |---|---|---|
//! | [`error`] | Stable error catalogue, `Error` | GEN-040, GEN-041, RS-011 |
//! | [`mode`] | `DecodeMode` (strict/lenient) | API-030…API-032 |
//! | [`anomaly`] | Lenient-mode anomaly flags, `AnomalySet` | GEN-043, GEN-081 |
//! | [`bits`] | Big-endian bit-field primitives | GEN-050, GEN-051 |
//! | [`ids`] | Range-checked identifiers | API-001, API-002, RS-010 |
//! | [`time`] | `Instant`, `Duration` | API-003 |
//! | [`crc`] | FECF CRC-16 | CMN-001…CMN-004 |
//! | [`oid`] | OID pseudo-noise generators | CMN-020…CMN-023 |
//! | [`counter`] | Modular counters, gap classification | CMN-030, CMN-031 |
//! | [`container`] | Fixed-capacity queues and vectors | GEN-010…GEN-012, API-061 |
//! | [`security`] | SDLS reservation hooks | API-070…API-075 |
//! | [`layout`] | Frame field layout | CMN-040, CMN-041 |
//!
//! `no_std`, no allocation, no `unsafe`, and no panics on any input (GEN-010, GEN-020, GEN-070).
#![no_std]
// Coverage runs exclude test modules (VER-024); `coverage_nightly` is set by cargo-llvm-cov.
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub mod anomaly;
pub mod bits;
pub mod container;
pub mod counter;
pub mod crc;
pub mod error;
pub mod ids;
pub mod layout;
pub mod mode;
pub mod oid;
pub mod security;
pub mod time;

pub use anomaly::{Anomaly, AnomalySet};
pub use error::{Error, ErrorDetail, ErrorKind, FieldId, Result};
pub use mode::DecodeMode;

/// Compiles and runs the examples in `README.md` as doctests; not part of the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
