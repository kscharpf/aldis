//! CCSDS 301.0-B-4 Time Code Formats (CUC, CDS, CCS, ASCII).
//!
//! Layer 1 of Aldis. Specification: `docs/standards/time-codes.md`.
//!
//! | Module | Contents | Requirements |
//! |---|---|---|
//! | [`pfield`] | P-field parsing, `TimeCodeId`, undefined-extension `PFieldTail` | TIME-001, TIME-002, TIME-005 |
//! | [`code`] | `decode_with_pfield`, `decode_implicit`, `TimeCode`, `TimeFormat`, agency-defined codes | TIME-001…TIME-003, TIME-005, TIME-006 |
//! | [`cuc`] | CCSDS Unsegmented Code, nanosecond conversions | TIME-010…TIME-014 |
//! | [`cds`] | CCSDS Day Segmented Code | TIME-020…TIME-022 |
//! | [`ccs`] | CCSDS Calendar Segmented Code | TIME-030, TIME-034 |
//! | [`ascii`] | ASCII time codes A and B (M1 WP-1, second part) | TIME-031, TIME-032 |
//! | [`anomaly`] | `TimeAnomaly` lenient-mode flags | API-032, GEN-043 |
//!
//! No operation reads a clock or a time zone (TIME-004). CUC Level 1 is TAI-based with no leap
//! seconds (TIME-014); CDS and CCS are UTC-based, and leap-second handling is the caller's
//! responsibility (TIME-022).
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
pub mod ascii;
pub mod ccs;
pub mod cds;
pub mod code;
pub mod cuc;
pub mod pfield;
mod util;

pub use anomaly::TimeAnomaly;
pub use ccs::{CalendarDate, Ccs, CcsFormat, CcsValue, CcsVariation};
pub use cds::{Cds, CdsDayLen, CdsEpoch, CdsFormat, CdsSubMs, CdsValue};
pub use code::{
    Agency, AgencyFormat, Decoded, TimeCode, TimeFormat, decode_implicit, decode_with_pfield,
};
pub use cuc::{Cuc, CucEpoch, CucFormat, CucValue};
pub use pfield::{PField, PFieldTail, TimeCodeId};
