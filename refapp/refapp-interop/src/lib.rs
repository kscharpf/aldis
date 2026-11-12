//! Transport driver of the interoperability milestone (docs/reference-application.md §11, plan
//! `docs/milestones/aint-interoperability.md`, D4 and D5).
//!
//! The driver steps a node through the contract of §8.1 against a socket and a clock. It is the
//! only I/O of the milestone: the nodes and the crates stay sans-I/O (APP-161). Tooling crate
//! (`std` plus workspace crates, no third-party dependency of its own, tooling lint profile,
//! APP-104).
//!
//! | Module | Contents | Requirements |
//! |---|---|---|
//! | [`clock`] | The clock trait, the monotonic clock, the manual (virtual-time) clock | APP-160 |
//! | [`error`] | The driver's error | APP-157 |
//! | [`log`] | The run log, JSON Lines | APP-162 |
//! | [`loss`] | The loss script | APP-163 |
//! | [`loopback`] | The self-test: both Rust nodes over loopback UDP in virtual time | APP-106 |
//! | [`port`] | The datagram port trait and its UDP implementation | APP-159 |
//! | [`run`] | The step loop against a port and a clock | APP-157, APP-158 |
//! | [`side`] | One node's step, slot, and datagram bookkeeping | APP-157…APP-159 |
#![forbid(unsafe_code)]

pub mod clock;
pub mod error;
pub mod log;
pub mod loopback;
pub mod loss;
pub mod port;
pub mod run;
pub mod side;

pub use error::Error;
