//! CCSDS 132.0-B-3 TM Space Data Link Protocol frame codec.
//!
//! Layer 1 of Aldis. Specification: `docs/standards/tm-frame.md`.
//!
//! | Module | Contents | Clauses | Requirements |
//! |---|---|---|---|
//! | [`config`] | `TmPhysicalChannelConfig`, `TmMasterChannelConfig`, `TmVirtualChannelConfig`, `DataFieldContent`, the `VcLookup` of a frame's security layout, `NoSdls` | 132.0 §5, §6.6 | TM-001, TM-011, API-040, API-041, API-070, GEN-060, GEN-061, RS-012, CMN-041 |
//! | [`header`] | Owned `TmPrimaryHeader`, `SegmentLengthId`, `FirstHeaderPointer`, `PacketPointer`, `PacketOffset`, `DataFieldStatus`, `FshLen` | 132.0 §4.1.2, §4.1.3.2 | TM-002, TM-003, TM-007, TM-008, API-001, API-014 |
//! | [`view`] | Zero-copy `TmFrame` decoder, `decode_unchecked`, `peek_length` | 132.0 §4.1, §6.3 | TM-001…TM-008, API-010, API-012…API-014, API-072, GEN-013 |
//! | [`anomaly`] | `TmAnomaly` lenient-mode flags | 132.0 §4.1.2.7.4–5 | TM-007, API-032, API-033, GEN-043 |
//! | [`writer`] | One-shot `encode`, two-phase `TmFrameWriter`, OID frames | 132.0 §4.1, §4.1.4.6, §6.3 | TM-009, TM-010, API-020…API-023, API-072, GEN-014 |
//! | [`validate`] | `TmFrame::validate` against the master channel's managed parameters | 132.0 §5 | TM-011 |
//! | [`field`] | The field codes its errors name, re-exported from `aldis_core::field` | 132.0 §4.1, §5 | API-082, GEN-041 |
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
pub mod config;
pub mod field;
pub mod header;
pub mod validate;
pub mod view;
pub mod writer;

pub use anomaly::TmAnomaly;
pub use config::{
    DataFieldContent, MAX_FRAME_LEN, NoSdls, TmMasterChannelConfig, TmPhysicalChannelConfig,
    TmVirtualChannelConfig, VcLookup,
};
pub use header::{
    DataFieldStatus, FirstHeaderPointer, FshLen, PacketOffset, PacketPointer, SegmentLengthId,
    TmPrimaryHeader,
};
pub use view::TmFrame;
pub use writer::{TmFrameContent, TmFrameFields, TmFrameWriter, encode};

/// Compiles and runs the examples in `README.md` as doctests; not part of the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
