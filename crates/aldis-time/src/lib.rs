//! CCSDS 301.0-B-4 Time Code Formats (CUC, CDS, CCS, ASCII).
//!
//! Layer 1 of Aldis. Specification: `docs/standards/time-codes.md`.
//!
//! | Module | Contents | Clauses | Requirements |
//! |---|---|---|---|
//! | [`pfield`] | P-field parsing, `TimeCodeId`, undefined-extension `PFieldTail` | 301.0 §3.1.1 | TIME-001, TIME-002, TIME-005 |
//! | [`code`] | `decode_with_pfield`, `decode_implicit`, `TimeCode`, `TimeFormat`, agency-defined codes | 301.0 §3.1, §3.6 | TIME-001…TIME-003, TIME-005, TIME-006 |
//! | [`cuc`] | CCSDS Unsegmented Code, nanosecond conversions | 301.0 §3.2 | TIME-010…TIME-014 |
//! | [`cds`] | CCSDS Day Segmented Code | 301.0 §3.3, Annex A | TIME-020…TIME-022 |
//! | [`calendar`] | Range-checked `Year`, `Month`, `Hour`, `Minute`, `Second`; `CalendarDate` | 301.0 §3.4.1, §3.5.1, Annex A | TIME-030 |
//! | [`ccs`] | CCSDS Calendar Segmented Code | 301.0 §3.4, Annex A | TIME-030, TIME-034 |
//! | [`ascii`] | ASCII time codes A and B and their subsets: `AsciiTime`, `AsciiSubset`, `AsciiFormat`, `Fraction` | 301.0 §3.5 | TIME-031, TIME-032 |
//! | [`convert`] | `UtcInstant`: conversions between CDS, CCS, and ASCII | 301.0 §3.3–3.5 | TIME-033 |
//! | [`anomaly`] | `TimeAnomaly` lenient-mode flags | 301.0 §3.1.1, §3.4.1.2 | API-032, GEN-043 |
//! | [`field`] | The field codes its errors name, re-exported from `aldis_core::field` | 301.0 §3 | API-082, GEN-041 |
//!
//! No operation reads a clock or a time zone (TIME-004). CUC Level 1 is TAI-based with no leap
//! seconds (TIME-014); CDS and CCS are UTC-based, and leap-second handling is the caller's
//! responsibility (TIME-022).
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
pub mod ascii;
pub mod calendar;
pub mod ccs;
pub mod cds;
pub mod code;
pub mod convert;
pub mod cuc;
pub mod field;
pub mod pfield;
mod util;

// Kani proof harnesses, compiled only by `cargo kani` (docs/verification/formal-proofs.md).
#[cfg(kani)]
mod proofs;

pub use anomaly::TimeAnomaly;
pub use ascii::{
    AsciiFormat, AsciiOptions, AsciiSubset, AsciiTime, Fraction, MAX_ASCII_LEN, MAX_FRACTION_DIGITS,
};
pub use calendar::{CalendarDate, Hour, Minute, Month, Second, Year};
pub use ccs::{Ccs, CcsFormat, CcsValue, CcsVariation};
pub use cds::{
    Cds, CdsDayLen, CdsEpoch, CdsFormat, CdsSubMs, CdsValue, Micros, MsOfDay, Picos, SubMillis,
};
pub use code::{
    Agency, AgencyFormat, Decoded, TimeCode, TimeFormat, decode_implicit, decode_with_pfield,
};
pub use convert::UtcInstant;
pub use cuc::{Cuc, CucEpoch, CucFormat, CucValue};
pub use pfield::{PField, PFieldTail, ReservedPField, TimeCodeId};

/// Compiles and runs the examples in `README.md` as doctests; not part of the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
