//! CCSDS 133.1-B-3 Encapsulation Packet Protocol codec.
//!
//! Layer 1 of Aldis. Specification: `docs/standards/encapsulation-packet.md`.
//!
//! | Module | Contents | Requirements |
//! |---|---|---|
//! | [`header`] | `Epi`, `EpiExtension`, `UserDefined`, `HeaderLen`, `HeaderSize`, owned `EncapsulationHeader` | EPP-001…EPP-008, API-014, API-021 |
//! | [`view`] | Zero-copy `EncapsulationPacket` decoder, `peek_length` | EPP-001…EPP-007, EPP-011, API-010, API-011, API-013 |
//! | [`writer`] | One-shot `encode`, two-phase `EncapsulationWriter`, `idle` | EPP-008, EPP-009, API-020…API-023 |
//! | [`config`] | `EppConfig` managed parameters, `EpiSet`, `ExtendedEpiSet` | EPP-010, GEN-060, GEN-061 |
//! | [`anomaly`] | `EppAnomaly` lenient-mode flags | API-032, GEN-043 |
//!
//! Unlike the Space Packet, the Packet Length field holds the **total** packet length,
//! header included (133.1 §4.1.2.8.2); the API exposes it unchanged.
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
pub mod config;
pub mod header;
pub mod view;
pub mod writer;

pub use anomaly::EppAnomaly;
pub use config::{EpiSet, EppConfig, ExtendedEpiSet};
pub use header::{
    EncapsulationFields, EncapsulationHeader, Epi, EpiExtension, HeaderLen, HeaderSize, UserDefined,
};
pub use view::EncapsulationPacket;
pub use writer::{EncapsulationWriter, encode, idle};
