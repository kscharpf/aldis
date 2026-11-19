//! The protocol trait of the link engines (LNK-001, D3 of the M4 plan).
//!
//! An engine is generic over a [`Protocol`] and has one implementation for each protocol it
//! serves: [`Tm`], [`Tc`], [`Aos`], and [`Uslp`], the frame formats of the codecs (features
//! `tm`, `tc`, `aos`, `uslp`). An engine instance serves exactly one protocol; the geometry of the
//! frames is the codec's [`FrameFormat`], and the other differences between the protocols are the
//! associated constants and functions of the trait and the protocol's parameters, not runtime
//! checks of frames (LNK-001).
//!
//! `PacketProtocol` adds what the `PacketGenerator` and the `PacketExtractor` need: where the
//! First Header Pointer is, what its values mean, and the data field header that carries it
//! (docs/04-engines.md §2.1). It exists with the feature `spp` or `epp`, as the engines do
//! (GEN-009); `Tm` and `Aos` implement it with either, and `Uslp` only with `epp`, because USLP
//! fills a fixed-length TFDZ with an Encapsulation Idle Packet and the extractor delimits one
//! (732.1 §4.1.4.3.4; LNK-013, LNK-063).

use core::fmt::Debug;

#[cfg(any(feature = "spp", feature = "epp"))]
use aldis_core::Result;
use aldis_core::ids::MapId;
use aldis_core::{Error, FrameFormat};

#[cfg(any(feature = "spp", feature = "epp"))]
use crate::common::IdlePacketKind;
use crate::common::Qos;

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
///
/// The trait exists with the feature `spp` or `epp` (GEN-009); `Uslp` implements it only with
/// `epp` (LNK-013, LNK-063).
#[cfg(any(feature = "spp", feature = "epp"))]
pub trait PacketProtocol: Protocol {
    /// The parameters of the protocol that the engines need and that other protocols lack: the
    /// AOS edition, the USLP UPID, MAP, and `QoS`. Plain data (API-041).
    type Params: Copy + Debug + Eq;

    /// Whether the extractor delimits an Encapsulation Idle Packet whatever the valid-PVN set,
    /// because the sender fills a fixed-length TFDZ with one (USLP rule `000`, 732.1
    /// §4.1.4.3.4; LNK-063).
    const DELIMITS_IDLE_EIP: bool;

    /// The First Header Pointer of the frame header, which the extractor takes with the data
    /// field: the 11-bit field of TM (132.0 §4.1.2.7.6), `()` where the pointer is in the data
    /// field (AOS, USLP).
    type HeaderPointer: Copy + Debug + Eq;

    /// The pointer of a data field the generator gives out, in the codec's type: the 11-bit
    /// First Header Pointer of TM (132.0 §4.1.2.7.6), the classified `M_PDU` pointer of AOS,
    /// whose width is the edition's (732.0 §4.1.4.2.2), and the First Header Pointer of USLP
    /// rule `000` (732.1 §4.1.4.2.4.3–4).
    type FieldPointer: Copy + Debug + Eq;

    /// The idle packet the protocol fixes, or `None` where the VC chooses (USLP: always an
    /// Encapsulation Idle Packet of exactly the remainder, 732.1 §4.1.4.3.4; LNK-013).
    const IDLE_KIND: Option<IdlePacketKind>;

    /// How the field of only idle data is built (LNK-014).
    const IDLE_FIELD: IdleField;

    /// The longest packet zone whose every offset has a pointer value of the protocol
    /// (132.0 §4.1.2.7.6.3–5; 732.0 §4.1.4.2.2.3–5; 732.1 §4.1.4.2.4.3–4).
    fn max_zone_len(params: &Self::Params) -> usize;

    /// The pointer for `pointer`, in the codec's type.
    ///
    /// # Errors
    /// `OutOfRange` (`first_header_pointer`) for an offset the pointer cannot hold, and
    /// `InvalidState` for `OnlyIdle` where the protocol has no such pointer value (USLP).
    fn encode_pointer(params: &Self::Params, pointer: Pointer) -> Result<Self::FieldPointer>;

    /// The First Header Pointer of the frame header that carries a field with `pointer`, as the
    /// `FrameReceiver` hands it to the extractor with the data field: TM's, none where the
    /// pointer is in the data field (AOS, USLP) (132.0 §4.1.2.7.6; LNK-060).
    fn header_pointer(pointer: Self::FieldPointer) -> Option<Self::HeaderPointer>;

    /// The raw pointer field of `pointer`, for a caller that carries it in its own format (132.0
    /// §4.1.2.7.6; 732.0 §4.1.4.2.2; 732.1 §4.1.4.2.4.3–4; LNK-011, LNK-076).
    fn raw_pointer(pointer: Self::FieldPointer) -> u16;

    /// Writes the data field header for `pointer` into `header`, which holds
    /// [`PACKET_HEADER_LEN`](FrameFormat::PACKET_HEADER_LEN) octets; writes nothing for TM.
    /// `pointer` comes from [`encode_pointer`](Self::encode_pointer) with the same `params`: AOS
    /// writes the raw value of the edition it was built for.
    fn write_header(params: &Self::Params, pointer: Self::FieldPointer, header: &mut [u8]);

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
        header_pointer: Option<Self::HeaderPointer>,
        field: &'a [u8],
    ) -> Result<ParsedField<'a>>;

    /// The MAP and `QoS` the field is sent on, for protocols that route by them (USLP).
    fn route(params: &Self::Params) -> Option<Route>;
}

#[cfg(feature = "tm")]
mod tm_impl {
    /// The parameters of a TM VC that the packet engines need: none, because the pointer is a
    /// field of the frame header with one width (132.0 §4.1.2.7.6; LNK-011).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct TmParams;

    /// The `PacketProtocol` of TM, with a packet codec compiled in (GEN-009).
    #[cfg(any(feature = "spp", feature = "epp"))]
    mod packets {
        use aldis_core::{Error, ErrorKind, Result, field};
        use aldis_tm::{FirstHeaderPointer, PacketOffset, PacketPointer};

        use super::TmParams;
        use crate::common::IdlePacketKind;
        use crate::protocol::{IdleField, PacketProtocol, ParsedField, Pointer, Route, Tm};

        impl PacketProtocol for Tm {
            type Params = TmParams;
            type HeaderPointer = FirstHeaderPointer;
            type FieldPointer = FirstHeaderPointer;
            const DELIMITS_IDLE_EIP: bool = false;
            const IDLE_KIND: Option<IdlePacketKind> = None;
            const IDLE_FIELD: IdleField = IdleField::Packets;

            fn max_zone_len(_: &TmParams) -> usize {
                usize::from(PacketOffset::MAX).saturating_add(1)
            }

            fn encode_pointer(_: &TmParams, pointer: Pointer) -> Result<FirstHeaderPointer> {
                Ok(match pointer {
                    Pointer::StartsAt(n) => u16::try_from(n)
                        .ok()
                        .and_then(PacketOffset::new)
                        .map(|offset| PacketPointer::StartsAt(offset).to_raw())
                        .ok_or(Error::out_of_range(
                            field::FIRST_HEADER_POINTER,
                            u64::from(PacketOffset::MAX),
                            u64::try_from(n).unwrap_or(u64::MAX),
                        ))?,
                    Pointer::NoStart => FirstHeaderPointer::NO_PACKET_START,
                    Pointer::OnlyIdle => FirstHeaderPointer::ONLY_IDLE_DATA,
                })
            }

            fn header_pointer(pointer: FirstHeaderPointer) -> Option<FirstHeaderPointer> {
                Some(pointer)
            }

            fn raw_pointer(pointer: FirstHeaderPointer) -> u16 {
                pointer.get()
            }

            fn write_header(_: &TmParams, _: FirstHeaderPointer, _: &mut [u8]) {}

            fn parse<'a>(
                _: &TmParams,
                header_pointer: Option<FirstHeaderPointer>,
                field: &'a [u8],
            ) -> Result<ParsedField<'a>> {
                let raw = header_pointer.ok_or(
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
}

#[cfg(feature = "tm")]
pub use tm_impl::TmParams;

#[cfg(feature = "aos")]
mod aos_impl {
    use aldis_aos::{AosEdition, PacketPointer};

    /// The parameters of an AOS VC that the packet engines need: the edition of the standard,
    /// which sets the width of the First Header Pointer (732.0 §4.1.4.2.1.4; AOS-003).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct AosParams {
        /// 732.0-B-5 (default) or the Issue 4 pointer of 11 bits.
        pub edition: AosEdition,
    }

    /// The pointer of an `M_PDU` that a generator gives out (732.0 §4.1.4.2.2).
    ///
    /// It is checked against the edition of its VC when it is built, so that its raw field value
    /// fits the edition's pointer field (732.0 §4.1.4.2.1.4; AOS-003, LNK-011, RS-020). Only
    /// `PacketProtocol::encode_pointer` builds one.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct AosPointer {
        pointer: PacketPointer,
        raw: u16,
    }

    impl AosPointer {
        /// The classified pointer, in the codec's type.
        ///
        /// Requirements: LNK-011, AOS-003; docs/04-engines.md §2.1.
        #[must_use]
        pub const fn pointer(self) -> PacketPointer {
            self.pointer
        }

        /// The raw pointer field, in the width of the edition of the VC.
        ///
        /// Requirements: LNK-011, AOS-003; docs/04-engines.md §2.1.
        #[must_use]
        pub const fn raw(self) -> u16 {
            self.raw
        }
    }

    /// The `PacketProtocol` of AOS, with a packet codec compiled in (GEN-009).
    #[cfg(any(feature = "spp", feature = "epp"))]
    mod packets {
        use aldis_aos::{Mpdu, PacketOffset, PacketPointer};
        use aldis_core::{Error, Result, field};

        use super::{AosParams, AosPointer};
        use crate::common::IdlePacketKind;
        use crate::protocol::{Aos, IdleField, PacketProtocol, ParsedField, Pointer, Route};

        impl PacketProtocol for Aos {
            type Params = AosParams;
            type HeaderPointer = ();
            type FieldPointer = AosPointer;
            const DELIMITS_IDLE_EIP: bool = false;
            const IDLE_KIND: Option<IdlePacketKind> = None;
            const IDLE_FIELD: IdleField = IdleField::Pattern;

            fn max_zone_len(params: &AosParams) -> usize {
                usize::from(PacketPointer::max_offset(params.edition)).saturating_add(1)
            }

            fn encode_pointer(params: &AosParams, pointer: Pointer) -> Result<AosPointer> {
                let edition = params.edition;
                let (pointer, raw) = match pointer {
                    Pointer::StartsAt(n) => {
                        let offset = u16::try_from(n)
                            .ok()
                            .filter(|&n| n <= PacketPointer::max_offset(edition))
                            .and_then(PacketOffset::new)
                            .ok_or(Error::out_of_range(
                                field::FIRST_HEADER_POINTER,
                                u64::from(PacketPointer::max_offset(edition)),
                                u64::try_from(n).unwrap_or(u64::MAX),
                            ))?;
                        (PacketPointer::StartsAt(offset), offset.get())
                    }
                    Pointer::NoStart => (
                        PacketPointer::NoPacketStart,
                        PacketPointer::no_packet_start_raw(edition),
                    ),
                    Pointer::OnlyIdle => (
                        PacketPointer::OnlyIdleData,
                        PacketPointer::only_idle_data_raw(edition),
                    ),
                };
                Ok(AosPointer { pointer, raw })
            }

            fn header_pointer(_: AosPointer) -> Option<()> {
                None
            }

            fn raw_pointer(pointer: AosPointer) -> u16 {
                pointer.raw()
            }

            fn write_header(_: &AosParams, pointer: AosPointer, header: &mut [u8]) {
                // The Issue 4 pointer has 11 bits in the low bits of the two octets, and the 5 spare
                // bits above it are zero (732.0-B-4 §4.1.4.2.1.4), which the raw value already is.
                for (d, s) in header.iter_mut().zip(pointer.raw().to_be_bytes()) {
                    *d = s;
                }
            }

            fn parse<'a>(
                params: &AosParams,
                _: Option<()>,
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
}

#[cfg(feature = "aos")]
pub use aos_impl::{AosParams, AosPointer};

#[cfg(feature = "uslp")]
mod uslp_impl {
    use aldis_core::ids::MapId;
    use aldis_core::{Error, ErrorKind, Result, field};
    use aldis_uslp::Upid;

    use crate::common::Qos;

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
        pub fn new(upid: Upid, map: MapId<4>, qos: Qos) -> Result<Self> {
            if upid == Upid::OnlyIdleData {
                Err(Error::new(ErrorKind::InvalidConfig).with_field(field::UPID))
            } else {
                Ok(Self { upid, map, qos })
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

    /// The `PacketProtocol` of USLP: only with `epp`, because rule `000` fills the TFDZ with an
    /// Encapsulation Idle Packet (732.1 §4.1.4.3.4; LNK-013, LNK-063, GEN-009).
    #[cfg(feature = "epp")]
    mod packets {
        use aldis_core::{Error, ErrorKind, Result, field};
        use aldis_uslp::{FirstHeaderPointer, PacketField, TfdfHeader, TfdzOffset, TfdzRule};

        use super::UslpParams;
        use crate::common::IdlePacketKind;
        use crate::protocol::{IdleField, PacketProtocol, ParsedField, Pointer, Route, Uslp};

        impl PacketProtocol for Uslp {
            type Params = UslpParams;
            type HeaderPointer = ();
            type FieldPointer = FirstHeaderPointer;
            const DELIMITS_IDLE_EIP: bool = true;
            const IDLE_KIND: Option<IdlePacketKind> = Some(IdlePacketKind::Encapsulation);
            const IDLE_FIELD: IdleField = IdleField::Packets;

            fn max_zone_len(_: &UslpParams) -> usize {
                usize::from(TfdzOffset::MAX).saturating_add(1)
            }

            fn encode_pointer(_: &UslpParams, pointer: Pointer) -> Result<FirstHeaderPointer> {
                match pointer {
                    Pointer::StartsAt(n) => u16::try_from(n)
                        .ok()
                        .and_then(TfdzOffset::new)
                        .map(FirstHeaderPointer::PacketStartsAt)
                        .ok_or(Error::out_of_range(
                            field::FHP_LVO_POINTER,
                            u64::from(TfdzOffset::MAX),
                            u64::try_from(n).unwrap_or(u64::MAX),
                        )),
                    Pointer::NoStart => Ok(FirstHeaderPointer::NoPacketStartOrEnd),
                    Pointer::OnlyIdle => Err(Error::new(ErrorKind::InvalidState)),
                }
            }

            fn header_pointer(_: FirstHeaderPointer) -> Option<()> {
                None
            }

            fn raw_pointer(pointer: FirstHeaderPointer) -> u16 {
                pointer.raw()
            }

            fn write_header(params: &UslpParams, pointer: FirstHeaderPointer, header: &mut [u8]) {
                let h = TfdfHeader {
                    rule: TfdzRule::PacketsSpanningFrames(pointer),
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
            fn parse<'a>(
                _: &UslpParams,
                _: Option<()>,
                field: &'a [u8],
            ) -> Result<ParsedField<'a>> {
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
}

#[cfg(feature = "uslp")]
pub use uslp_impl::UslpParams;
