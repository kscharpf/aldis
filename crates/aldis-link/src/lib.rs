//! Sans-I/O sending and receiving engines for CCSDS packets and frames.
//!
//! Layer 2 of Aldis. Specification: `docs/04-engines.md`.
//!
//! | Module | Contents | Requirements |
//! |---|---|---|
//! | [`common`] | Counters, occupancy, `QoS`, the valid-PVN set and idle packet kind (with `spp` or `epp`), idle policy | LNK-002, LNK-013, LNK-014, LNK-078 |
//! | [`protocol`] | The protocol trait with `Tm`, `Tc`, `Aos`, `Uslp`; the packet protocol trait with `spp` or `epp` | LNK-001, LNK-013, LNK-063 |
//! | `generator` | The `PacketGenerator` (feature `spp` or `epp`) | LNK-010…LNK-015, LNK-074, LNK-075 |
//! | `extractor` | The `PacketExtractor` (feature `spp` or `epp`) | LNK-060…LNK-064, LNK-074, LNK-075 |
//! | [`access`] | The `AccessGenerator` (VC and MAP Access) | LNK-020, LNK-074, LNK-075 |
//! | [`bitstream`] | The `BitstreamGenerator` (AOS `B_PDU`) | LNK-021, LNK-074, LNK-075 |
//! | [`mux`], [`policy`] | The `ChannelMux` and its multiplexing policies | LNK-030…LNK-036, LNK-076, LNK-082 |
//! | `receiver` | The `FrameReceiver` | LNK-050…LNK-054, LNK-077, LNK-079…LNK-081 |
//! | `segmenter` | The `MapSegmenter` (feature `spp` or `epp`) | LNK-040…LNK-043, LNK-074, LNK-075, LNK-078 |
//! | `reassembler` | The `MapReassembler` (feature `spp` or `epp`) | LNK-070…LNK-073, LNK-074, LNK-075 |
//! | [`cltu`] | The `CltuPacker` | LNK-044, LNK-074, LNK-075, LNK-078 |
//!
//! **Packet codecs (GEN-009).** The packet engines (`generator`, `extractor`, `segmenter`,
//! `reassembler`) build with `spp`, with `epp`, or with both, and compile no code of a codec not
//! selected: a packet of its PVN is refused at `SubmitPacket` (`InvalidField`, `packet_version`,
//! LNK-010) and its PVN or idle kind at configuration (`InvalidConfig`, LNK-078). The generator
//! and the extractor over USLP need `epp`, because USLP fills a fixed-length TFDZ with an
//! Encapsulation Idle Packet (732.1 §4.1.4.3.4; LNK-013, LNK-063); the segmenter and the
//! reassembler over USLP serve only variable-length frames and build with `spp` alone. The frame
//! engines need neither.
//!
//! **Status:** the `PacketGenerator` and the `PacketExtractor` (M4 WP-3), the `ChannelMux`, the
//! `AccessGenerator`, and the `BitstreamGenerator` (M4 WP-4), the `FrameReceiver` (M4 WP-5), and the
//! `MapSegmenter`, the `MapReassembler`, and the `CltuPacker` (M4 WP-6) are implemented. Requirements
//! are traced by ID in each module's documentation.
#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

pub mod access;
pub mod bitstream;
pub mod cltu;
pub mod common;
#[cfg(any(feature = "spp", feature = "epp"))]
pub mod extractor;
#[cfg(any(feature = "spp", feature = "epp"))]
pub mod generator;
pub mod mux;
#[cfg(any(feature = "spp", feature = "epp"))]
mod packet;
pub mod policy;
pub mod protocol;
#[cfg(any(feature = "spp", feature = "epp"))]
pub mod reassembler;
#[cfg(any(feature = "tm", feature = "tc", feature = "aos", feature = "uslp"))]
pub mod receiver;
#[cfg(any(feature = "spp", feature = "epp"))]
pub mod segmenter;
