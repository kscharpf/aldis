//! CCSDS 133.0-B-2 Space Packet Protocol codec.
//!
//! Layer 1 of Aldis. Specification: `docs/standards/space-packet.md`.
//!
//! | Module | Contents | Requirements |
//! |---|---|---|
//! | [`header`] | `PacketType`, `SequenceFlags`, `PacketFields`, owned `SpacePacketHeader` | SPP-001…SPP-003, SPP-009, API-014, API-021 |
//! | [`view`] | Zero-copy `SpacePacket` decoder, `peek_length` | SPP-001…SPP-004, SPP-009, API-010, API-011, API-013 |
//! | [`secondary`] | `SecondaryHeaderLayout` per APID and the split into `SplitPacket` | SPP-007 |
//! | [`writer`] | One-shot `encode`, two-phase `SpacePacketWriter` | SPP-005, SPP-006, API-020…API-023 |
//! | [`config`] | `SppConfig` (Maximum Packet Length), the checked `PacketData` | SPP-005, SPP-006, GEN-060 |
//! | [`idle`] | `IdlePacket` builder | SPP-010 |
//! | [`sequence`] | `SequenceCounter`, `SequenceTracker` | SPP-011, GEN-011, GEN-024 |
//! | [`octet`] | `OctetStringEncoder` (Octet String service) | SPP-012 |
//! | [`segment`] | Segmentation and reassembly (not yet implemented) | SPP-013 |
//! | [`anomaly`] | `SppAnomaly` lenient-mode flags | API-032, GEN-043 |
//!
//! The Packet Data Length field holds the data field length minus one (133.0 §4.1.3.5); the API
//! works in data field and total packet lengths and converts at the wire.
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
pub mod header;
pub mod idle;
pub mod octet;
pub mod secondary;
pub mod segment;
pub mod sequence;
pub mod view;
pub mod writer;

pub use anomaly::SppAnomaly;
pub use config::{PacketData, SppConfig};
pub use header::{
    MAX_PACKET_LEN, MIN_PACKET_LEN, PRIMARY_HEADER_LEN, PacketFields, PacketType, SequenceFlags,
    SpacePacketHeader,
};
pub use idle::IdlePacket;
pub use octet::OctetStringEncoder;
pub use secondary::{SecondaryHeaderLayout, SplitPacket};
pub use sequence::{SequenceCounter, SequenceTracker, Tracked};
pub use view::SpacePacket;
pub use writer::{SpacePacketWriter, encode};
