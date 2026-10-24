//! CCSDS 732.0-B-5 AOS Space Data Link Protocol frame codec.
//!
//! Layer 1 of Aldis. Specification: `docs/standards/aos-frame.md`.
//!
//! | Module | Contents | Clauses | Requirements |
//! |---|---|---|---|
//! | [`fhec`] | Frame Header Error Control: `encode`, `write`, `decode` with up to two symbol corrections, `CorrectedHeader`, `CorrectedSymbols` | 732.0 §4.1.2.6 | AOS-013, AOS-014 |
//! | [`config`] | `AosEdition`, `AosPhysicalChannelConfig`, `AosVirtualChannelConfig`, `DataFieldContent`, the `VcLookup` trait and its `VcTable` | 732.0 §5, §6.6; 732.0-B-4 | AOS-001, AOS-002, AOS-018, GEN-060, GEN-062, API-040, API-041, API-070 |
//! | [`header`] | Owned `AosPrimaryHeader` read after FHEC correction, `VcFrameCount`, `FrameCountCycle`, `OID_VCID` | 732.0 §4.1.2, §4.3.7.2 | AOS-002, AOS-010…AOS-014, API-014 |
//! | [`view`] | Zero-copy `AosFrame` decoder with the `VcLookup`, `DataUnit`, `peek_length` | 732.0 §4.1, §4.3.7, §6.3 | AOS-010…AOS-017, API-010, API-012, API-013, API-072, GEN-013 |
//! | [`mpdu`], [`bpdu`] | `Mpdu` and `PacketPointer` (both editions), `Bpdu` and `BitstreamPointer` | 732.0 §4.1.4.2–3; 732.0-B-4 §4.1.4.2 | AOS-003, AOS-017, AOS-018 |
//! | [`writer`] | One-shot `encode`, two-phase `AosFrameWriter`, OID frames on VCID 63 | 732.0 §4.1, §4.1.4.1.5, §4.2.7, §6.3 | AOS-015, AOS-016, AOS-019, API-020…API-023, API-072 |
//! | [`anomaly`] | `AosAnomaly` lenient-mode flags | 732.0 §4.1.2.5.5.3, §4.1.4.3.2; 732.0-B-4 §4.1.2.5.4, §4.1.4.2.2 | API-032, API-033, GEN-043 |
//! | [`field`] | The field codes its errors name, re-exported from `aldis_core::field` | 732.0 §4.1, §5 | API-082, GEN-041 |
//!
//! The OCF is returned as an [`Ocf`](aldis_core::ocf::Ocf); a Type-1 Report (CLCW) is decoded
//! with `aldis-tc` (CMN-011, M2 plan D7).
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
pub mod bpdu;
pub mod config;
pub mod fhec;
pub mod field;
pub mod header;
pub mod mpdu;
pub mod view;
pub mod writer;

pub use anomaly::AosAnomaly;
pub use bpdu::{BitstreamPointer, Bpdu};
pub use config::{
    AosEdition, AosPhysicalChannelConfig, AosVirtualChannelConfig, DataFieldContent, VcLookup,
    VcTable,
};
pub use fhec::{CorrectedHeader, CorrectedSymbols};
pub use header::{AosPrimaryHeader, FrameCountCycle, OID_VCID, VcFrameCount};
pub use mpdu::{Mpdu, PacketPointer};
pub use view::{AosFrame, DataUnit};
pub use writer::{AosFrameContent, AosFrameFields, AosFrameWriter, PduHeader, encode};

/// Compiles and runs the examples in `README.md` as doctests; not part of the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
