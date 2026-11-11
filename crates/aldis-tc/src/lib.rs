//! CCSDS 232.0-B-4 TC Space Data Link Protocol frame codec and CLCW.
//!
//! Layer 1 of Aldis. Specification: `docs/standards/tc-frame.md`.
//!
//! | Module | Contents | Clauses | Requirements |
//! |---|---|---|---|
//! | [`config`] | `PhysicalChannelConfig`, `VcConfig`, `SegmentHeaderUse`, `MapConfig`, its `VcTable` | 232.0 §5, §6.6 | GEN-060, GEN-061, API-040, API-041, API-070 |
//! | [`header`] | `FrameType`, `FrameFields`, `SegmentHeader`, `SequenceFlags`, owned `TcFrameHeader` | 232.0 §4.1.2, §4.1.3.2.2 | TC-001…TC-004, TC-008, API-014, GEN-053 |
//! | [`view`] | Zero-copy `TcFrame` decoder (strict and lenient), `peek_length` | 232.0 §4.1, §6.3 | TC-001…TC-006, API-010…API-014, API-072 |
//! | [`writer`] | One-shot `encode` with `FrameData`, two-phase `TcFrameWriter` | 232.0 §4.1, §6.3 | TC-004, TC-007, TC-008, API-020…API-023 |
//! | [`control`] | `ControlCommand` Unlock and Set V(R) | 232.0 §4.1.3.3 | TC-005 |
//! | [`anomaly`] | `TcAnomaly` lenient-mode flags | 232.0 §4.1.2.4, §4.1.3.3.4, §4.2.1 | API-032, API-033 |
//! | [`clcw`] | `Clcw`, `ClcwStatus`, `CopInEffect`, `FarmBCounter` | 232.0 §4.2 | TC-030, TC-031, CMN-011 |
//! | [`delimit`] | Frame Delimiting and Fill Removal: stateless `next_frame`, `FrameDelimiter` | 232.0 §4.4.9.2 | TC-020 |
//! | `security` | `SecuritySlices` for `TcFrame`: `into_plain` through `NoSecurity` returns an `aldis_core::Plain`, the frame input of an engine | 232.0 Cor. 1 §6.5.2 | RS-023, API-070, API-074 |
//! | [`checked`] | Frame Validation Checks: `check_frame` (strict, lenient) returning a `Checked<TcFrame>`, `Reason` | 232.0 §4.4.9.3, §4.4.7.3, §4.4.3.3 | TC-021…TC-023, API-016 |
//! | [`field`] | The field codes its errors name, re-exported from `aldis_core::field` | 232.0 §4.1, §4.2, §5 | API-082, GEN-041 |
//! | [`format`] | `TcFormat`: the `FrameFormat` geometry (Primary Header, data field header lengths) the link engines are generic over | 232.0 §4.1 | LNK-001, GEN-003 |
//!
//! The Frame Length field holds the frame length minus one (232.0 §4.1.2.7); the API works in
//! total lengths and converts at the wire (GEN-053).
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
pub mod checked;
pub mod clcw;
pub mod config;
pub mod control;
pub mod delimit;
pub mod field;
pub mod format;
pub mod header;
mod security;
pub mod stream;
pub mod view;
pub mod writer;

pub use anomaly::TcAnomaly;
pub use checked::{Reason, check_frame};
pub use clcw::{Clcw, ClcwStatus, CopInEffect, FarmBCounter};
pub use config::{MapConfig, PhysicalChannelConfig, SegmentHeaderUse, VcConfig, VcTable};
pub use control::{ControlCommand, ControlOctets};
pub use delimit::{Delimited, FrameDelimiter, next_frame};
pub use format::TcFormat;
pub use header::{
    FrameFields, FrameType, MAX_FRAME_LEN, MIN_FRAME_LEN, PRIMARY_HEADER_LEN, SEGMENT_HEADER_LEN,
    SegmentHeader, SequenceFlags, TcFrameHeader, TcMapId, TcSpacecraftId, TcVcId,
};
pub use stream::{DiscardReason, FrameStream, IntakeItem, LenientFrameStream, TcIntake};
pub use view::TcFrame;
pub use writer::{FrameData, TcFrameWriter, TcPlan, encode};

/// Compiles and runs the examples in `README.md` as doctests; not part of the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
