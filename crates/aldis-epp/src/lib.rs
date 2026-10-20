//! CCSDS 133.1-B-3 Encapsulation Packet Protocol codec.
//!
//! Layer 1 of Aldis. Specification: `docs/standards/encapsulation-packet.md`.
//!
//! | Module | Contents | Clauses | Requirements |
//! |---|---|---|---|
//! | [`header`] | `Epi`, `EpiExtension`, `UserDefined`, `HeaderLen`, `HeaderSize`, owned `EncapsulationHeader` | 133.1 §4.1.2 | EPP-001…EPP-008, API-014, API-021 |
//! | [`view`] | Zero-copy `EncapsulationPacket` decoder, `peek_length` | 133.1 §4.1 | EPP-001…EPP-007, EPP-011, API-010, API-011, API-013 |
//! | [`writer`] | One-shot `encode`, two-phase `EncapsulationWriter`, `idle` | 133.1 §4.1, §4.2.1 | EPP-008, EPP-009, API-020…API-023 |
//! | [`config`] | `EppConfig` managed parameters, `EpiSet`, `ExtendedEpiSet`, the checked `DataUnit` | 133.1 §4.2.1.1.2, §5 Table 5-1 | EPP-010, GEN-060, GEN-061 |
//! | [`anomaly`] | `EppAnomaly` lenient-mode flags | 133.1 §4.1.2.6.3, §4.1.2.7.2 | API-032, GEN-043 |
//!
//! Unlike the Space Packet, the Packet Length field holds the **total** packet length,
//! header included (133.1 §4.1.2.8.2); the API exposes it unchanged.
//!
//! Aldis embeds no SANA registry: the valid EPI and extended-EPI sets (133.1 §4.1.2.3.3,
//! §4.1.2.6.4, §5 Table 5-1) are managed parameters the caller supplies in an [`EppConfig`]
//! (EPP-010; M1 D4, OI-06).
//!
//! # Features
//!
//! None by default. `alloc` and `std` are strictly additive: they change no behaviour and no
//! wire output (GEN-005, GEN-006).
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
pub use config::{DataUnit, EpiSet, EppConfig, ExtendedEpiSet};
pub use header::{
    EncapsulationFields, EncapsulationHeader, Epi, EpiExtension, HeaderLen, HeaderSize, UserDefined,
};
pub use view::EncapsulationPacket;
pub use writer::{EncapsulationWriter, encode, idle};
