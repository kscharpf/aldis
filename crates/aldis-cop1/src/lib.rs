//! CCSDS 232.1-B-2 COP-1 (FOP-1 and FARM-1) as sans-I/O engines.
//!
//! Layer 2 of Aldis. Specification: `docs/standards/cop1.md`.
//!
//! Requirements are traced by ID in each module's documentation.
#![no_std]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[cfg(feature = "std")]
extern crate std;

pub mod farm1;
pub mod farm1_table;
pub mod fop1;
pub mod fop1_table;
pub mod notify;

pub use farm1_table::{
    EVENT_COUNT as FARM1_EVENT_COUNT, FARM1_TABLE, Farm1Action, Farm1Cell, Farm1Event, Farm1State,
    STATE_COUNT as FARM1_STATE_COUNT,
};
pub use fop1_table::{
    BcCommand, EVENT_COUNT as FOP1_EVENT_COUNT, FOP1_TABLE, Fop1Action, Fop1Cell, Fop1Event,
    Fop1State, FrameKind, STATE_COUNT as FOP1_STATE_COUNT,
};
pub use notify::{
    Alert, AlertReason, Directive, DirectiveNotification, DirectiveNotify, TimeoutType,
    TransferNotification, TransferNotify,
};
