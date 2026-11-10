//! The protocol trait of the link engines (LNK-001, D3 of the M4 plan).
//!
//! An engine is generic over a [`Protocol`] and has one implementation for each protocol it
//! serves: [`Tm`], [`Tc`], [`Aos`], and [`Uslp`], the frame formats of the codecs (features
//! `tm`, `tc`, `aos`, `uslp`). An engine instance serves exactly one protocol; the geometry of the
//! frames is the codec's [`FrameFormat`], and the other differences between the protocols are the
//! associated constants and functions of the trait and the protocol's parameters, not runtime
//! checks of frames (LNK-001).
//!
//! [`PacketProtocol`] adds what the `PacketGenerator` and the `PacketExtractor` need: where the
//! First Header Pointer is, what its values mean, and the data field header that carries it
//! (docs/04-engines.md §2.1).

use core::fmt::Debug;

use aldis_core::ids::MapId;
use aldis_core::{Error, FrameFormat, Result};

use crate::common::{IdlePacketKind, Qos};

/// The frame protocol a link engine serves (LNK-001; field `link_protocol`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinkProtocol {
    /// TM Space Data Link Protocol (132.0-B-3).
    Tm,
    /// TC Space Data Link Protocol (232.0-B-4).
    Tc,
    /// AOS Space Data Link Protocol (732.0-B-5).
    Aos,
    /// Unified Space Data Link Protocol (732.1-B-3).
    Uslp,
}

/// A frame protocol, as a type parameter of an engine (LNK-001, D3 of the M4 plan).
///
/// It is the [`FrameFormat`] of its codec, which carries the geometry of the frames, with the
/// identifier width and the id of the protocol.
pub trait Protocol: FrameFormat {
    /// The protocol this type stands for.
    const ID: LinkProtocol;

    /// The Virtual Channel Identifier of the protocol (132.0 §4.1.2.5; 232.0 §4.1.2.6; 732.0
    /// §4.1.2.3; 732.1 §4.1.2.5): its width is the protocol's.
    type Vc: Copy + Debug + Eq + TryFrom<u8, Error = Error> + Into<u8>;
}

/// The TM protocol (LNK-001): the frame format of `aldis-tm`.
#[cfg(feature = "tm")]
pub use aldis_tm::TmFormat as Tm;

/// The TC protocol (LNK-001): the frame format of `aldis-tc`.
#[cfg(feature = "tc")]
pub use aldis_tc::TcFormat as Tc;

/// The AOS protocol (LNK-001): the frame format of `aldis-aos`.
#[cfg(feature = "aos")]
pub use aldis_aos::AosFormat as Aos;

/// The USLP protocol (LNK-001): the frame format of `aldis-uslp`.
#[cfg(feature = "uslp")]
pub use aldis_uslp::UslpFormat as Uslp;

#[cfg(feature = "tm")]
impl Protocol for Tm {
    const ID: LinkProtocol = LinkProtocol::Tm;
    type Vc = aldis_core::ids::VcId<3>;
}

#[cfg(feature = "tc")]
impl Protocol for Tc {
    const ID: LinkProtocol = LinkProtocol::Tc;
    type Vc = aldis_core::ids::VcId<6>;
}

#[cfg(feature = "aos")]
impl Protocol for Aos {
    const ID: LinkProtocol = LinkProtocol::Aos;
    type Vc = aldis_core::ids::VcId<6>;
}

#[cfg(feature = "uslp")]
impl Protocol for Uslp {
    const ID: LinkProtocol = LinkProtocol::Uslp;
    type Vc = aldis_core::ids::VcId<6>;
}

/// What a First Header Pointer says about the packets of a data field (132.0 §4.1.2.7.6; 732.0
/// §4.1.4.2.2; 732.1 §4.1.4.2.4; LNK-011, LNK-064).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pointer {
    /// The first packet that starts in the field starts at this octet of the packet zone,
    /// counted from 0.
    StartsAt(usize),
    /// No packet starts in the field (all ones).
    NoStart,
    /// The field holds only idle data (TM, AOS: all ones minus one; USLP: UPID Only Idle Data).
    OnlyIdle,
}

/// The packet zone of a data field and its pointer, as an extractor reads it (LNK-060).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedField<'a> {
    /// What the pointer says.
    pub pointer: Pointer,
    /// The octets after the data field header (TM data field, AOS Packet Zone, USLP TFDZ).
    pub zone: &'a [u8],
}

/// The MAP and quality of service a USLP data field is sent on (732.1 §4.1.2.8; LNK-043).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Route {
    /// The MAP ID of the field.
    pub map: MapId<4>,
    /// The quality of service of the field.
    pub qos: Qos,
}

/// How a protocol builds the field that holds only idle data (LNK-014).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdleField {
    /// A field of idle packets with the pointer to the first (TM, USLP).
    Packets,
    /// A field whose packet zone is the idle pattern and whose pointer says "only idle data"
    /// (AOS).
    Pattern,
}

/// A protocol whose fixed-length data fields carry packets with a First Header Pointer: TM, AOS,
/// and USLP rule `000` (docs/04-engines.md §2.1).
pub trait PacketProtocol: Protocol {
    /// The parameters of the protocol that the engines need and that other protocols lack: the
    /// AOS edition, the USLP UPID, MAP, and `QoS`. Plain data (API-041).
    type Params: Copy + Debug + Eq;

    /// Whether the extractor delimits an Encapsulation Idle Packet whatever the valid-PVN set,
    /// because the sender fills a fixed-length TFDZ with one (USLP rule `000`, 732.1
    /// §4.1.4.3.4; LNK-063).
    const DELIMITS_IDLE_EIP: bool;

    /// The idle packet the protocol fixes, or `None` where the VC chooses (USLP: always an
    /// Encapsulation Idle Packet of exactly the remainder, 732.1 §4.1.4.3.4; LNK-013).
    const IDLE_KIND: Option<IdlePacketKind>;

    /// How the field of only idle data is built (LNK-014).
    const IDLE_FIELD: IdleField;

    /// The longest packet zone whose every offset has a pointer value of the protocol
    /// (132.0 §4.1.2.7.6.3–5; 732.0 §4.1.4.2.2.3–5; 732.1 §4.1.4.2.4.3–4).
    fn max_zone_len(params: &Self::Params) -> usize;

    /// The raw pointer field for `pointer`.
    ///
    /// # Errors
    /// `OutOfRange` (`first_header_pointer`) for an offset the pointer cannot hold, and
    /// `InvalidState` for `OnlyIdle` where the protocol has no such pointer value (USLP).
    fn encode_pointer(params: &Self::Params, pointer: Pointer) -> Result<u16>;

    /// Writes the data field header for the raw pointer `raw` into `header`, which holds
    /// [`PACKET_HEADER_LEN`](FrameFormat::PACKET_HEADER_LEN) octets; writes nothing for TM.
    fn write_header(params: &Self::Params, raw: u16, header: &mut [u8]);

    /// Reads a data field: the pointer and the packet zone (LNK-060, LNK-064). `header_pointer`
    /// is the First Header Pointer of the frame header (TM, required); AOS and USLP carry the
    /// pointer in `field` and ignore it.
    ///
    /// # Errors
    /// `InvalidField` (`first_header_pointer`) for a TM field without a pointer;
    /// `Malformed` (`first_header_pointer` or `fhp_lvo_pointer`) for an offset outside the zone;
    /// the data field errors of the codec (AOS `M_PDU` header, USLP TFDF header and rule).
    fn parse<'a>(
        params: &Self::Params,
        header_pointer: Option<u16>,
        field: &'a [u8],
    ) -> Result<ParsedField<'a>>;

    /// The MAP and `QoS` the field is sent on, for protocols that route by them (USLP).
    fn route(params: &Self::Params) -> Option<Route>;
}

#[cfg(feature = "tm")]
mod tm_impl {
    use aldis_core::{Error, ErrorKind, Result, field};
    use aldis_tm::{FirstHeaderPointer, PacketOffset, PacketPointer};

    use super::{IdleField, PacketProtocol, ParsedField, Pointer, Route, Tm};
    use crate::common::IdlePacketKind;

    /// The parameters of a TM VC that the packet engines need: none, because the pointer is a
    /// field of the frame header with one width (132.0 §4.1.2.7.6; LNK-011).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct TmParams;

    impl PacketProtocol for Tm {
        type Params = TmParams;
        const DELIMITS_IDLE_EIP: bool = false;
        const IDLE_KIND: Option<IdlePacketKind> = None;
        const IDLE_FIELD: IdleField = IdleField::Packets;

        fn max_zone_len(_: &TmParams) -> usize {
            usize::from(PacketOffset::MAX).saturating_add(1)
        }

        fn encode_pointer(_: &TmParams, pointer: Pointer) -> Result<u16> {
            Ok(match pointer {
                Pointer::StartsAt(n) => PacketOffset::new(u16::try_from(n).unwrap_or(u16::MAX))
                    .map(PacketOffset::get)
                    .ok_or(Error::out_of_range(
                        field::FIRST_HEADER_POINTER,
                        u64::from(PacketOffset::MAX),
                        u64::try_from(n).unwrap_or(u64::MAX),
                    ))?,
                Pointer::NoStart => FirstHeaderPointer::NO_PACKET_START.get(),
                Pointer::OnlyIdle => FirstHeaderPointer::ONLY_IDLE_DATA.get(),
            })
        }

        fn write_header(_: &TmParams, _: u16, _: &mut [u8]) {}

        fn parse<'a>(
            _: &TmParams,
            header_pointer: Option<u16>,
            field: &'a [u8],
        ) -> Result<ParsedField<'a>> {
            let raw = header_pointer.and_then(FirstHeaderPointer::new).ok_or(
                Error::new(ErrorKind::InvalidField).with_field(field::FIRST_HEADER_POINTER),
            )?;
            let pointer = match PacketPointer::classify(raw, field.len())? {
                PacketPointer::StartsAt(n) => Pointer::StartsAt(usize::from(n.get())),
                PacketPointer::NoPacketStart => Pointer::NoStart,
                PacketPointer::OnlyIdleData => Pointer::OnlyIdle,
            };
            Ok(ParsedField {
                pointer,
                zone: field,
            })
        }

        fn route(_: &TmParams) -> Option<Route> {
            None
        }
    }
}

#[cfg(feature = "tm")]
pub use tm_impl::TmParams;

#[cfg(feature = "aos")]
mod aos_impl {
    use aldis_aos::{AosEdition, Mpdu, PacketOffset, PacketPointer};
    use aldis_core::{Error, ErrorKind, Result, field};

    use super::{Aos, IdleField, PacketProtocol, ParsedField, Pointer, Route};
    use crate::common::IdlePacketKind;

    /// The parameters of an AOS VC that the packet engines need: the edition of the standard,
    /// which sets the width of the First Header Pointer (732.0 §4.1.4.2.1.4; AOS-003).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct AosParams {
        /// 732.0-B-5 (default) or the Issue 4 pointer of 11 bits.
        pub edition: AosEdition,
    }

    impl PacketProtocol for Aos {
        type Params = AosParams;
        const DELIMITS_IDLE_EIP: bool = false;
        const IDLE_KIND: Option<IdlePacketKind> = None;
        const IDLE_FIELD: IdleField = IdleField::Pattern;

        fn max_zone_len(params: &AosParams) -> usize {
            usize::from(PacketPointer::max_offset(params.edition)).saturating_add(1)
        }

        fn encode_pointer(params: &AosParams, pointer: Pointer) -> Result<u16> {
            let out_of_range = |n: usize| {
                Error::out_of_range(
                    field::FIRST_HEADER_POINTER,
                    u64::from(PacketPointer::max_offset(params.edition)),
                    u64::try_from(n).unwrap_or(u64::MAX),
                )
            };
            let classified = match pointer {
                Pointer::StartsAt(n) => PacketPointer::StartsAt(
                    u16::try_from(n)
                        .ok()
                        .and_then(PacketOffset::new)
                        .ok_or(out_of_range(n))?,
                ),
                Pointer::NoStart => PacketPointer::NoPacketStart,
                Pointer::OnlyIdle => PacketPointer::OnlyIdleData,
            };
            match pointer {
                Pointer::StartsAt(n) => classified.to_raw(params.edition).ok_or(out_of_range(n)),
                Pointer::NoStart | Pointer::OnlyIdle => classified
                    .to_raw(params.edition)
                    .ok_or(Error::new(ErrorKind::InternalError)),
            }
        }

        fn write_header(_: &AosParams, raw: u16, header: &mut [u8]) {
            // The Issue 4 pointer has 11 bits in the low bits of the two octets, and the 5 spare
            // bits above it are zero (732.0-B-4 §4.1.4.2.1.4), which the raw value already is.
            for (d, s) in header.iter_mut().zip(raw.to_be_bytes()) {
                *d = s;
            }
        }

        fn parse<'a>(
            params: &AosParams,
            _: Option<u16>,
            field: &'a [u8],
        ) -> Result<ParsedField<'a>> {
            let mpdu = Mpdu::decode(field, params.edition)?;
            let pointer = match mpdu.pointer() {
                PacketPointer::StartsAt(n) => Pointer::StartsAt(usize::from(n.get())),
                PacketPointer::NoPacketStart => Pointer::NoStart,
                PacketPointer::OnlyIdleData => Pointer::OnlyIdle,
            };
            Ok(ParsedField {
                pointer,
                zone: mpdu.packet_zone(),
            })
        }

        fn route(_: &AosParams) -> Option<Route> {
            None
        }
    }
}

#[cfg(feature = "aos")]
pub use aos_impl::AosParams;

#[cfg(feature = "uslp")]
mod uslp_impl {
    use aldis_core::ids::MapId;
    use aldis_core::{Error, ErrorKind, Result, field};
    use aldis_uslp::{FirstHeaderPointer, PacketField, TfdfHeader, TfdzOffset, TfdzRule, Upid};

    use super::{IdleField, PacketProtocol, ParsedField, Pointer, Route, Uslp};
    use crate::common::{IdlePacketKind, Qos};

    /// The parameters of a USLP VC or MAP that the packet engines need: the UPID of its TFDFs and
    /// the MAP and `QoS` they are sent on (732.1 §4.1.4.2.3, §4.1.2.8; LNK-043).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct UslpParams {
        upid: Upid,
        map: MapId<4>,
        qos: Qos,
    }

    impl UslpParams {
        /// The parameters of a USLP VC or MAP (732.1 §4.1.4.2.3, §4.1.2.8).
        ///
        /// # Errors
        /// `InvalidConfig` (`upid`) for "Only Idle Data": that UPID of the SANA registry marks a
        /// TFDZ of idle data (732.1 §4.1.4.2.3, §4.1.4.1.9), so no packet could be carried in
        /// it.
        pub fn new(upid: Upid, map: MapId<4>, qos: Qos) -> Result<UslpParams> {
            if upid == Upid::OnlyIdleData {
                Err(Error::new(ErrorKind::InvalidConfig).with_field(field::UPID))
            } else {
                Ok(UslpParams { upid, map, qos })
            }
        }
    }

    impl UslpParams {
        /// The USLP Protocol Identifier of the TFDFs (732.1 §4.1.4.2.3); never "Only Idle Data".
        #[must_use]
        pub const fn upid(&self) -> Upid {
            self.upid
        }

        /// The MAP ID the fields are sent on (732.1 §4.1.2.7).
        #[must_use]
        pub const fn map(&self) -> MapId<4> {
            self.map
        }

        /// The quality of service of the fields.
        ///
        /// Requirements: LNK-001, LNK-010, LNK-060; docs/04-engines.md §2.1.
        #[must_use]
        pub const fn qos(&self) -> Qos {
            self.qos
        }
    }

    impl PacketProtocol for Uslp {
        type Params = UslpParams;
        const DELIMITS_IDLE_EIP: bool = true;
        const IDLE_KIND: Option<IdlePacketKind> = Some(IdlePacketKind::Encapsulation);
        const IDLE_FIELD: IdleField = IdleField::Packets;

        fn max_zone_len(_: &UslpParams) -> usize {
            usize::from(TfdzOffset::MAX).saturating_add(1)
        }

        fn encode_pointer(_: &UslpParams, pointer: Pointer) -> Result<u16> {
            match pointer {
                Pointer::StartsAt(n) => u16::try_from(n)
                    .ok()
                    .and_then(TfdzOffset::new)
                    .map(|o| FirstHeaderPointer::PacketStartsAt(o).raw())
                    .ok_or(Error::out_of_range(
                        field::FHP_LVO_POINTER,
                        u64::from(TfdzOffset::MAX),
                        u64::try_from(n).unwrap_or(u64::MAX),
                    )),
                Pointer::NoStart => Ok(FirstHeaderPointer::NoPacketStartOrEnd.raw()),
                Pointer::OnlyIdle => Err(Error::new(ErrorKind::InvalidState)),
            }
        }

        fn write_header(params: &UslpParams, raw: u16, header: &mut [u8]) {
            let h = TfdfHeader {
                rule: TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::from_raw(raw)),
                upid: params.upid,
            };
            for (d, s) in header.iter_mut().zip(h.octets()) {
                *d = s;
            }
        }

        // The UPID of the field is not compared with the configured one: the UPID names the
        // service of the TFDZ (732.1 §4.1.4.2.3), and the FrameReceiver routes a TFDF to this
        // extractor by VC and MAP and by that service (LNK-050, LNK-081, WP-5), so a TFDF of
        // another service never reaches it. Only the idle UPID is read, because an OID frame
        // carries it on a VC that has packets (732.1 §4.1.4.1.9); the codec reads it before the
        // rule (`TfdfHeader::parse_packet_field`).
        fn parse<'a>(_: &UslpParams, _: Option<u16>, field: &'a [u8]) -> Result<ParsedField<'a>> {
            Ok(match TfdfHeader::parse_packet_field(field)? {
                PacketField::OnlyIdle { zone } => ParsedField {
                    pointer: Pointer::OnlyIdle,
                    zone,
                },
                PacketField::Packets {
                    pointer: FirstHeaderPointer::PacketStartsAt(n),
                    zone,
                } => ParsedField {
                    pointer: Pointer::StartsAt(usize::from(n.get())),
                    zone,
                },
                PacketField::Packets {
                    pointer: FirstHeaderPointer::NoPacketStartOrEnd,
                    zone,
                } => ParsedField {
                    pointer: Pointer::NoStart,
                    zone,
                },
            })
        }

        fn route(params: &UslpParams) -> Option<Route> {
            Some(Route {
                map: params.map,
                qos: params.qos,
            })
        }
    }
}

#[cfg(feature = "uslp")]
pub use uslp_impl::UslpParams;
