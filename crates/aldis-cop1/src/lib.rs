//! CCSDS 232.1-B-2 COP-1 (FOP-1 and FARM-1) as sans-I/O engines.
//!
//! Layer 2 of Aldis. Specification: `docs/standards/cop1.md`.
//!
//! Requirements are traced by ID in each module's documentation; every public item cites its
//! clause of CCSDS 232.1-B-2 and its requirement IDs (RS-015).
#![no_std]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[cfg(feature = "std")]
extern crate std;

#[cfg(test)]
extern crate self as aldis_cop1;

pub mod actions;
pub mod farm1;
pub mod farm1_table;
pub mod fop1;
pub mod fop1_table;
pub mod notify;
#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "../tests/common/mod.rs"]
mod rig;

pub use actions::{Actions, OutRange};
pub use farm1::{
    FARM1_MAX_ACTIONS, Farm1, Farm1Actions, Farm1Config, Farm1Frame, Farm1Input, Farm1Notify,
    Farm1Output, Farm1Window, FduKind,
};
pub use farm1_table::{
    EVENT_COUNT as FARM1_EVENT_COUNT, FARM1_TABLE, Farm1Action, Farm1Cell, Farm1Event, Farm1State,
    STATE_COUNT as FARM1_STATE_COUNT,
};
pub use fop1::{
    Fop1, Fop1Actions, Fop1Channel, Fop1Config, Fop1Decision, Fop1Input, Fop1Output, Fop1Store,
    Fop1Target, Handle, fop1_max_actions,
};
pub use fop1_table::{
    BcCommand, EVENT_COUNT as FOP1_EVENT_COUNT, FOP1_TABLE, Fop1Action, Fop1Cell, Fop1Event,
    Fop1State, FrameKind, STATE_COUNT as FOP1_STATE_COUNT, SuspendState,
};
pub use notify::{
    Alert, AlertReason, Directive, DirectiveNotification, DirectiveNotify, TimeoutType,
    TransferNotification, TransferNotify,
};

/// Compiles and runs the examples in `README.md` as doctests; not part of the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
