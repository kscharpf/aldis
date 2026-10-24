//! CCSDS 732.1-B-3 Unified Space Data Link Protocol frame codec.
//!
//! Layer 1 of Aldis. Specification: `docs/standards/uslp-frame.md`.
//!
//! | Module | Contents | Clauses | Requirements |
//! |---|---|---|---|
//! | [`upid`] | `Upid`, generated from the SANA registry snapshot, and `UnassignedUpid` | 732.1 §4.1.4.2.3 | USLP-007 |
//! | [`config`] | `PhysicalChannelConfig`, `VcConfig`, `VcParams`, `MapConfig`, `TransferFrameType`, `OcfUse`, `SecurityScope`, `TruncatedFrameLen`, the `VcLookup` trait and its `VcTable` | 732.1 §5, §6.6, Annex D1.2 | USLP-002, USLP-004, USLP-008, USLP-012, GEN-060, GEN-061, API-040, API-041, API-070, API-075 |
//! | [`header`] | `FrameKind`, `SourceOrDestination`, `VcfCountLen`, `VcfCount`, `ChannelFields`, owned `UslpPrimaryHeader` | 732.1 §4.1.2, Annex D1.3 | USLP-001, USLP-003, USLP-004, API-014, GEN-053 |
//! | [`view`] | Zero-copy `UslpFrame` (full or truncated) and `UslpFullFrame`, `peek_length` | 732.1 §4.1, §6.3 | USLP-001, USLP-003…USLP-007, USLP-016, API-010…API-014, API-072 |
//! | [`truncated`] | Zero-copy `UslpTruncatedFrame` | 732.1 Annex D | USLP-002, USLP-012, API-075 |
//! | [`tfdf`] | `TfdfHeader`, `TfdzRule`, `ConstructionRule`, `FirstHeaderPointer`, `LastValidOctet`, `TfdzOffset` | 732.1 §4.1.4.2 | USLP-005, USLP-006, USLP-016 |
//! | [`writer`] | One-shot `encode`, two-phase `UslpFrameWriter` (with `resume` for a stateless finish), `encode_truncated`, `encode_oid` and `encode_oid_galois` | 732.1 §4.1, §6.3, Annex D | USLP-009, USLP-010, API-020…API-023 |
//! | [`anomaly`] | `UslpAnomaly` lenient-mode flags | 732.1 §4.1.2.9, §4.1.4.1.8–9 | API-032, API-033 |
//! | [`delimit`] | Frame Delimiting and Fill Data Removal: stateless `next_frame`, `FrameDelimiter` | 732.1 §4.3.11.2 | USLP-015 |
//! | [`validate`] | USLP Frame Validation Checks: `validate_frame`, `Reason`, `Rejection` | 732.1 §4.3.11.3 | USLP-014 |
//! | [`field`] | The field codes its errors name, re-exported from `aldis_core::field` | 732.1 §4.1, §5 | API-082, GEN-041 |
//!
//! The Frame Length field holds the frame length minus one (732.1 §4.1.2.7); the API works in
//! total lengths and converts at the wire (GEN-053). The OCF is returned as an
//! [`Ocf`](aldis_core::ocf::Ocf); a Type-1 Report (CLCW) is decoded with `aldis-tc` (CMN-011,
//! M2 plan D7), and so is a TFDZ of COP-1 control commands (USLP-013, M2 plan D8). USLP-011,
//! which keeps truncated and non-truncated frames out of one C&S data unit, is a sending rule of
//! the multiplexing engine (M4).
//!
//! # Features
//!
//! None by default (GEN-005). `alloc` and `std` are strictly additive; they change no
//! behaviour and no wire output (GEN-006).
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
pub mod delimit;
pub mod field;
pub mod header;
pub mod tfdf;
pub mod truncated;
pub mod upid;
pub mod validate;
pub mod view;
pub mod writer;

pub use anomaly::UslpAnomaly;
pub use config::{
    MapConfig, OcfUse, PhysicalChannelConfig, SecurityScope, TransferFrameType, TruncatedFrameLen,
    VcConfig, VcLookup, VcParams, VcTable,
};
pub use delimit::{Delimited, FrameDelimiter, next_frame};
pub use header::{
    ChannelFields, FrameKind, MAX_FRAME_LEN, OID_VCID, PRIMARY_HEADER_LEN, SourceOrDestination,
    TRUNCATED_HEADER_LEN, UslpMapId, UslpPrimaryHeader, UslpSpacecraftId, UslpVcId, VcfCount,
    VcfCountLen,
};
pub use tfdf::{
    ConstructionRule, FirstHeaderPointer, LastValidOctet, POINTER_NONE, TfdfHeader, TfdzOffset,
    TfdzRule,
};
pub use truncated::UslpTruncatedFrame;
pub use upid::{UnassignedUpid, Upid};
pub use validate::{Reason, Rejection, validate_frame};
pub use view::{UslpFrame, UslpFullFrame};
pub use writer::{
    FrameContent, FrameFields, UslpFrameWriter, encode, encode_oid, encode_oid_galois,
    encode_truncated,
};

/// Compiles and runs the examples in `README.md` as doctests; not part of the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
