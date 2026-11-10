//! CCSDS 132.0-B-3 TM Space Data Link Protocol frame codec.
//!
//! Layer 1 of Aldis. Specification: `docs/standards/tm-frame.md`.
//!
//! | Module | Contents | Clauses | Requirements |
//! |---|---|---|---|
//! | [`config`] | `TmPhysicalChannelConfig`, `TmMasterChannelConfig`, `TmVirtualChannelConfig`, `DataFieldContent`, `security_layout` and `NoSdls`, the lookup of a frame's security layout | 132.0 §5, §6.6 | TM-001, TM-011, API-040, API-041, API-070, GEN-060, GEN-061, RS-012, CMN-041 |
//! | [`header`] | Owned `TmPrimaryHeader`, `SegmentLengthId`, `FirstHeaderPointer`, `PacketPointer`, `PacketOffset`, `DataFieldStatus`, `FshLen` | 132.0 §4.1.2, §4.1.3.2 | TM-002, TM-003, TM-007, TM-008, API-001, API-014 |
//! | [`view`] | Zero-copy `TmFrame` decoder (strict and lenient), `UncheckedTmFrame`, `peek_length` | 132.0 §4.1, §6.3 | TM-001…TM-008, API-010, API-012…API-014, API-015, API-072, GEN-013 |
//! | [`anomaly`] | `TmAnomaly` lenient-mode flags | 132.0 §4.1.2.7.4–5 | TM-007, API-032, API-033, GEN-043 |
//! | [`writer`] | One-shot `encode` and `encode_oid`, two-phase `TmFrameWriter`, OID frames | 132.0 §4.1, §4.1.4.6, §6.3 | TM-009, TM-010, API-020…API-023, API-072, GEN-014 |
//! | `security` | `SecuritySlices` for `TmFrame`: `into_plain` through `NoSecurity` returns an `aldis_core::Plain`, the frame input of an engine | 355.0 §2.3 | RS-023, API-070, API-074 |
//! | `checked` | `TmFrame::check` (`aldis_core::Check`) against the master channel's managed parameters, returning an `aldis_core::Checked<TmFrame>` | 132.0 §5 | TM-011, API-016 |
//! | [`field`] | The field codes its errors name, re-exported from `aldis_core::field` | 132.0 §4.1, §5 | API-082, GEN-041 |
//! | [`format`] | `TmFormat`: the `FrameFormat` geometry (Primary Header, data field header lengths) the link engines are generic over | 132.0 §4.1 | LNK-001, GEN-003 |
//!
//! The OCF is returned as an [`Ocf`](aldis_core::ocf::Ocf); a Type-1 Report (CLCW) is decoded
//! with `aldis-tc` (CMN-011, M2 plan D7).
//!
//! # Features
//!
//! None by default (GEN-005).
//!
//! `no_std`, no allocation, no `unsafe`, and no panics on any input (GEN-010, GEN-020,
//! GEN-070).
#![no_std]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

pub mod anomaly;
mod checked;
pub mod config;
pub mod field;
pub mod format;
pub mod header;
mod security;
pub mod view;
pub mod writer;

pub use anomaly::TmAnomaly;
pub use config::{
    DataFieldContent, MAX_FRAME_LEN, NoSdls, TmMasterChannelConfig, TmPhysicalChannelConfig,
    TmVirtualChannelConfig, security_layout,
};
pub use format::TmFormat;
pub use header::{
    DataFieldStatus, FirstHeaderPointer, FshLen, PacketOffset, PacketPointer, SegmentLengthId,
    TmPrimaryHeader,
};
pub use view::{TmFrame, UncheckedTmFrame};
pub use writer::{
    TmFrameContent, TmFrameFields, TmFrameWriter, encode, encode_oid, encode_oid_galois,
};

/// Compiles and runs the examples in `README.md` as doctests; not part of the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
