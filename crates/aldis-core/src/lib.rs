//! Foundation types and shared algorithms for Aldis.
//!
//! Layer 0 of Aldis. Specification: `docs/standards/common-link-features.md`,
//! `docs/02-design-rules.md`, and `docs/03-api-model.md`.
//!
//! | Module | Contents | Requirements |
//! |---|---|---|
//! | [`error`] | Stable error catalogue, `Error`, `FieldId` | GEN-040, GEN-041, RS-011 |
//! | [`field`] | Field codes of the catalogue, generated (03-api-model.md §9.1) | API-082 |
//! | [`mode`] | `Lenient`, `DecodeMode` (strict/lenient) | API-030…API-032 |
//! | [`anomaly`] | Lenient-mode anomaly flags, `AnomalySet` | GEN-043, GEN-081 |
//! | [`bits`] | Big-endian bit-field primitives (codec-facing) | GEN-050, GEN-051, GEN-082 |
//! | [`ids`] | Range-checked identifiers | API-001, API-002, RS-010 |
//! | [`time`] | `Instant`, `Duration` | API-003 |
//! | [`crc`] | FECF CRC-16 | CMN-001…CMN-004 |
//! | [`ocf`] | Operational Control Field classification | CMN-010, CMN-011 |
//! | [`oid`] | OID pseudo-noise generators | CMN-020…CMN-023 |
//! | [`counter`] | Modular counters, gap classification | CMN-030, CMN-031 |
//! | [`container`] | Fixed-capacity vector (codec-facing) | GEN-010…GEN-012, GEN-082 |
//! | [`cop`] | `CopFrameKind`: Type-AD, Type-BD, Type-BC | TC-002, USLP-001, COP-001 |
//! | [`security`] | SDLS reservation hooks | API-070…API-075 |
//! | [`layout`] | Frame field layout | CMN-040, CMN-041 |
//! | [`lookup`] | `ChannelLookup`, `MapLookup`, `PhysicalLookup` | GEN-060, API-040 |
//! | [`checked`] | `Checked`, `Rejection`, the `Check` and `CheckBytes` pipeline | GEN-060, API-015, API-016, RS-020 |
//! | [`plain`] | `Plain`, `SecuritySlices`: a frame whose SDLS processing is done | RS-023, API-017, API-074 |
//! | [`stream`] | `FrameStream`, `LenientFrameStream`, `IntakeItem`, the `Delimiter` of a protocol (codec-facing), `StreamFrame` (codec-facing) | TC-020, USLP-016, API-016, GEN-082 |
//! | [`engine`] | `Actions` and `OutRange` of the engine step | API-057, API-061, RS-022 |
//! | [`finishing`] | Frame finishing states of the codec writers | RS-023 |
//! | [`format`] | `FrameFormat`, `PhysicalChannel`: the geometry of a codec's frames (both codec-facing) | LNK-001, GEN-003, GEN-082 |
//! | [`writer`] | `FrameWriter`: the finishing of a frame, FECF and SDLS, once for every codec; `FramePlan`, `FinishPlan`, `VariablePlan` (codec-facing) | RS-023, API-074, GEN-082 |
//!
//! # Codec-facing tier
//!
//! The items of this crate that exist for the Aldis codec and engine crates and that an
//! integrator does not need are its **codec-facing tier** (GEN-082). It is the modules [`bits`]
//! and [`container`], whose items a minor release may change incompatibly, and the traits that
//! only an Aldis crate implements, to which a minor release may add a required item and which
//! no one implements outside the Aldis crates (unsupported): [`format::FrameFormat`],
//! [`format::PhysicalChannel`], [`writer::FramePlan`], [`writer::FinishPlan`],
//! [`writer::VariablePlan`], [`stream::Delimiter`], and [`stream::StreamFrame`]. Every other
//! public item is integrator-facing, and GEN-080 applies to it in full. The Aldis crates are
//! released at one version and each depends on the others at exactly that version, so that a
//! change of the codec-facing tier cannot break a build that uses only integrator-facing items.
//!
//! `no_std`, no allocation, no `unsafe`, and no panics on any input (GEN-010, GEN-020, GEN-070).
#![no_std]
#![forbid(unsafe_code)]
// Coverage runs exclude test modules (VER-024); `coverage_nightly` is set by cargo-llvm-cov.
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

pub mod anomaly;
pub mod bits;
pub mod checked;
pub mod container;
pub mod cop;
pub mod counter;
pub mod crc;
pub mod engine;
pub mod error;
pub mod field;
pub mod finishing;
pub mod format;
pub mod ids;
pub mod layout;
pub mod lookup;
pub mod mode;
pub mod ocf;
pub mod oid;
pub mod plain;
pub mod security;
pub mod stream;
pub mod time;
pub mod writer;

// Kani proof harnesses, compiled only by `cargo kani` (docs/verification/formal-verification.md).
#[cfg(kani)]
mod proofs;

pub use anomaly::{Anomaly, AnomalySet};
pub use checked::{Check, CheckBytes, Checked, Rejection};
pub use cop::CopFrameKind;
pub use error::{Error, ErrorDetail, ErrorKind, FieldId, Result};
pub use format::{FrameFormat, PhysicalChannel};
pub use mode::{DecodeMode, Lenient};
pub use plain::{Plain, SecuritySlices};

/// Compiles and runs the examples in `README.md` as doctests; not part of the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
