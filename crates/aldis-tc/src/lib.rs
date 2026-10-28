//! CCSDS 232.0-B-4 TC Space Data Link Protocol frame codec and CLCW.
//!
//! Layer 1 of Aldis. Specification: `docs/standards/tc-frame.md`.
//!
//! | Module | Contents | Clauses | Requirements |
//! |---|---|---|---|
//! | [`config`] | `PhysicalChannelConfig`, `VcConfig`, `SegmentHeaderUse`, `MapConfig`, the `VcLookup` trait and its `VcTable` | 232.0 §5, §6.6 | GEN-060, GEN-061, API-040, API-041, API-070 |
//! | [`header`] | `FrameType`, `FrameFields`, `SegmentHeader`, `SequenceFlags`, owned `TcFrameHeader` | 232.0 §4.1.2, §4.1.3.2.2 | TC-001…TC-004, TC-008, API-014, GEN-053 |
//! | [`view`] | Zero-copy `TcFrame` decoder, `peek_length` | 232.0 §4.1, §6.3 | TC-001…TC-006, API-010…API-014, API-072 |
//! | [`writer`] | One-shot `encode` with `FrameData`, two-phase `TcFrameWriter` | 232.0 §4.1, §6.3 | TC-004, TC-007, TC-008, API-020…API-023 |
//! | [`control`] | `ControlCommand` Unlock and Set V(R) | 232.0 §4.1.3.3 | TC-005 |
//! | [`anomaly`] | `TcAnomaly` lenient-mode flags | 232.0 §4.1.2.4, §4.1.3.3.4, §4.2.1 | API-032, API-033 |
//! | [`clcw`] | `Clcw`, `ClcwStatus`, `CopInEffect`, `FarmBCounter` | 232.0 §4.2 | TC-030, TC-031, CMN-011 |
//! | [`delimit`] | Frame Delimiting and Fill Removal: stateless `next_frame`, `FrameDelimiter` | 232.0 §4.4.9.2 | TC-020 |
//! | [`validate`] | Frame Validation Checks: `validate_frame`, `Reason`, `Rejection` | 232.0 §4.4.9.3, §4.4.7.3, §4.4.3.3 | TC-021…TC-023 |
//! | [`field`] | The field codes its errors name, re-exported from `aldis_core::field` | 232.0 §4.1, §4.2, §5 | API-082, GEN-041 |
//!
//! The Frame Length field holds the frame length minus one (232.0 §4.1.2.7); the API works in
//! total lengths and converts at the wire (GEN-053).
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
pub mod clcw;
pub mod config;
pub mod control;
pub mod delimit;
pub mod field;
pub mod header;
pub mod stream;
pub mod validate;
pub mod view;
pub mod writer;

pub use anomaly::TcAnomaly;
pub use clcw::{Clcw, ClcwStatus, CopInEffect, FarmBCounter};
pub use config::{MapConfig, PhysicalChannelConfig, SegmentHeaderUse, VcConfig, VcLookup, VcTable};
pub use control::{ControlCommand, ControlOctets};
pub use delimit::{Delimited, FrameDelimiter, next_frame};
pub use header::{
    FrameFields, FrameType, MAX_FRAME_LEN, MIN_FRAME_LEN, PRIMARY_HEADER_LEN, SEGMENT_HEADER_LEN,
    SegmentHeader, SequenceFlags, TcFrameHeader, TcMapId, TcSpacecraftId, TcVcId,
};
pub use stream::{DiscardReason, FrameStream, IntakeItem};
pub use validate::{Reason, Rejection, validate_frame};
pub use view::TcFrame;
pub use writer::{FrameData, TcFrameWriter, encode};

/// Compiles and runs the examples in `README.md` as doctests; not part of the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
