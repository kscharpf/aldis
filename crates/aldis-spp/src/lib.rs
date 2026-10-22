//! CCSDS 133.0-B-2 Space Packet Protocol codec.
//!
//! Layer 1 of Aldis. Specification: `docs/standards/space-packet.md`.
//!
//! | Module | Contents | Clauses | Requirements |
//! |---|---|---|---|
//! | [`header`] | `PacketType`, `SequenceFlags`, `PacketFields`, owned `SpacePacketHeader` | 133.0 §4.1.3 | SPP-001…SPP-003, SPP-009, API-014, API-021 |
//! | [`view`] | Zero-copy `SpacePacket` decoder, `peek_length` | 133.0 §4.1.2, §4.1.3 | SPP-001…SPP-004, SPP-009, API-010, API-011, API-013 |
//! | [`secondary`] | `SecondaryHeaderLayout` per APID, `TimeHeaderLayout` (feature `time`), and the split into `SplitPacket` | 133.0 §4.1.4.2 | SPP-007, SPP-008 |
//! | [`writer`] | One-shot `encode`, two-phase `SpacePacketWriter` | 133.0 §4.1 | SPP-005, SPP-006, API-020…API-023 |
//! | [`config`] | `SppConfig` (Maximum Packet Length), the checked `PacketData` | 133.0 §5.2 Table 5-1 | SPP-005, SPP-006, GEN-060 |
//! | [`idle`] | `IdlePacket` builder | 133.0 §4.1.3.3.4.4, §4.1.4.3.4 | SPP-010 |
//! | [`sequence`] | `SequenceCounter`, `SequenceTracker` | 133.0 §4.1.3.4.3, §4.3.2.2 | SPP-011, GEN-011, GEN-024 |
//! | [`octet`] | `OctetStringEncoder` (Octet String service) | 133.0 §3.4, §4.2.2 | SPP-012 |
//! | [`segment`] | `Segmenter`, `Reassembler` with its `Outcome`, `Status`, and `Discard`; their buffer-free `SegmenterState` and `ReassemblerState` | 133.0 §4.1.3.4.2 note | SPP-013, API-060 |
//! | [`anomaly`] | `SppAnomaly` lenient-mode flags | 133.0 §4.1.3.3.3.4 | API-032, GEN-043 |
//! | [`field`] | The field codes its errors name, re-exported from `aldis_core::field` | 133.0 §4.1, §5.2 | API-082, GEN-041 |
//!
//! The Packet Data Length field holds the data field length minus one (133.0 §4.1.3.5); the API
//! works in data field and total packet lengths and converts at the wire.
//!
//! # Features
//!
//! None by default (GEN-005).
//!
//! - `time`: the typed time secondary header, `SecondaryHeaderLayout::Time` with
//!   `TimeHeaderLayout`, whose Time Code Field is decoded with `aldis-time` (133.0 §4.1.4.2.2,
//!   SPP-008). Without it a secondary header is opaque octets (133.0 §4.1.4.2.1, SPP-007), and
//!   the crate does not depend on `aldis-time`.
//! - `alloc`, `std`: strictly additive; they change no behaviour and no wire output (GEN-006).
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
pub mod idle;
pub mod octet;
pub mod secondary;
pub mod segment;
pub mod sequence;
pub mod view;
pub mod writer;

// Kani proof harnesses, compiled only by `cargo kani` (docs/verification/formal-proofs.md).
#[cfg(kani)]
mod proofs;

pub use anomaly::SppAnomaly;
pub use config::{PacketData, SppConfig};
pub use header::{
    MAX_PACKET_LEN, MIN_PACKET_LEN, PRIMARY_HEADER_LEN, PacketFields, PacketType, SequenceFlags,
    SpacePacketHeader,
};
pub use idle::IdlePacket;
pub use octet::OctetStringEncoder;
#[cfg(feature = "time")]
pub use secondary::TimeHeaderLayout;
pub use secondary::{SecondaryHeaderLayout, SplitPacket};
pub use segment::{
    Discard, Outcome, Reassembler, ReassemblerState, Segmenter, SegmenterState, Status,
};
pub use sequence::{SequenceCounter, SequenceTracker, Tracked};
pub use view::SpacePacket;
pub use writer::{SpacePacketWriter, encode};

/// Compiles and runs the examples in `README.md` as doctests; not part of the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
