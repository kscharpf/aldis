//! Sans-I/O sending and receiving engines for CCSDS packets and frames.
//!
//! Layer 2 of Aldis. Specification: `docs/04-engines.md`.
//!
//! | Module | Contents | Requirements |
//! |---|---|---|
//! | [`common`] | Counters, occupancy, `QoS`, the valid-PVN set, idle parameters | LNK-002, LNK-014, LNK-078 |
//! | [`protocol`] | The protocol trait with `Tm`, `Tc`, `Aos`, `Uslp` | LNK-001 |
//! | `generator` | The `PacketGenerator` (features `spp`, `epp`) | LNK-010…LNK-015, LNK-074, LNK-075 |
//! | `extractor` | The `PacketExtractor` (features `spp`, `epp`) | LNK-060…LNK-064, LNK-074, LNK-075 |
//! | [`access`] | The `AccessGenerator` (VC and MAP Access) | LNK-020, LNK-074, LNK-075 |
//! | [`bitstream`] | The `BitstreamGenerator` (AOS `B_PDU`) | LNK-021, LNK-074, LNK-075 |
//! | [`mux`], [`policy`] | The `ChannelMux` and its multiplexing policies | LNK-030…LNK-036, LNK-076, LNK-082 |
//! | `receiver` | The `FrameReceiver` | LNK-050…LNK-054, LNK-077, LNK-079…LNK-081 |
//! | `segmenter` | The `MapSegmenter` (features `spp`, `epp`) | LNK-040…LNK-043, LNK-074, LNK-075, LNK-078 |
//! | `reassembler` | The `MapReassembler` (features `spp`, `epp`) | LNK-070…LNK-073, LNK-074, LNK-075 |
//! | [`cltu`] | The `CltuPacker` | LNK-044, LNK-074, LNK-075, LNK-078 |
//!
//! **Status:** the `PacketGenerator` and the `PacketExtractor` (M4 WP-3), the `ChannelMux`, the
//! `AccessGenerator`, and the `BitstreamGenerator` (M4 WP-4), the `FrameReceiver` (M4 WP-5), and the
//! `MapSegmenter`, the `MapReassembler`, and the `CltuPacker` (M4 WP-6) are implemented. Requirements
//! are traced by ID in each module's documentation.
#![no_std]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

pub mod access;
pub mod bitstream;
pub mod cltu;
pub mod common;
#[cfg(all(feature = "spp", feature = "epp"))]
pub mod extractor;
#[cfg(all(feature = "spp", feature = "epp"))]
pub mod generator;
#[cfg(doc)]
pub mod inplace;
pub mod mux;
#[cfg(all(feature = "spp", feature = "epp"))]
mod packet;
pub mod policy;
pub mod protocol;
#[cfg(all(feature = "spp", feature = "epp"))]
pub mod reassembler;
#[cfg(any(feature = "tm", feature = "tc", feature = "aos", feature = "uslp"))]
pub mod receiver;
#[cfg(all(feature = "spp", feature = "epp"))]
pub mod segmenter;
