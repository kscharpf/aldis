//! Types shared by the C ABI handles of the link engines (LNK-001, LNK-002, LNK-003, CABI-030,
//! CABI-040; M4 plan D4).
//!
//! Every link engine is one opaque handle whose frame protocol is chosen in its configuration:
//! the first member of each configuration is an `aldis_link_channel_t`, whose `protocol` is an
//! `ALDIS_LINK_PROTOCOL_*` constant. The handle holds the engine of that protocol, so one
//! `aldis_<engine>_t` serves TM, AOS, and USLP (the protocols of the engine; a protocol it does
//! not serve is `ALDIS_ERR_INVALID_CONFIG` on `protocol`).

use aldis::aos::AosEdition;
use aldis::core::ids::MapId;
use aldis::core::{Error, ErrorKind, Result, field};
use aldis::link::common::{IdlePacketKind, IdlePolicy, Occupancy, Qos, ValidPvns};
use aldis::link::protocol::{AosParams, Protocol, Route, UslpParams};
use aldis::uslp::Upid;

use crate::abi::flag;
use crate::aos::aldis_aos_edition;
use crate::mirror::{mirror, parse};

/// The frame protocol of a link engine (LNK-001).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of
/// the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_link_protocol {
    /// TM Space Data Link Protocol (132.0-B-3).
    ALDIS_LINK_PROTOCOL_TM = 0,
    /// TC Space Data Link Protocol (232.0-B-4).
    ALDIS_LINK_PROTOCOL_TC = 1,
    /// AOS Space Data Link Protocol (732.0-B-5).
    ALDIS_LINK_PROTOCOL_AOS = 2,
    /// Unified Space Data Link Protocol (732.1-B-3).
    ALDIS_LINK_PROTOCOL_USLP = 3,
}
mirror!(pub aldis_link_protocol: u32 {
    ALDIS_LINK_PROTOCOL_TM,
    ALDIS_LINK_PROTOCOL_TC,
    ALDIS_LINK_PROTOCOL_AOS,
    ALDIS_LINK_PROTOCOL_USLP
});

/// The quality of service of a USLP MAP (732.1 §4.1.2.8; LNK-043).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of
/// the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_link_qos {
    /// Sequence-Controlled Service (Type-AD).
    ALDIS_LINK_QOS_SEQUENCE_CONTROLLED = 0,
    /// Expedited Service (Type-BD).
    ALDIS_LINK_QOS_EXPEDITED = 1,
}
mirror!(pub aldis_link_qos: u32 {
    ALDIS_LINK_QOS_SEQUENCE_CONTROLLED,
    ALDIS_LINK_QOS_EXPEDITED
});

/// What a generator does when it has nothing to release (LNK-014).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of
/// the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_link_idle_policy {
    /// The caller sends an Only Idle Data frame (OID); the generator releases nothing.
    ALDIS_LINK_IDLE_POLICY_OID = 0,
    /// The generator releases a field of idle data.
    ALDIS_LINK_IDLE_POLICY_IDLE_FIELD = 1,
}
mirror!(pub aldis_link_idle_policy: u32 {
    ALDIS_LINK_IDLE_POLICY_OID,
    ALDIS_LINK_IDLE_POLICY_IDLE_FIELD
});

/// The kind of idle packet that completes a field (LNK-013).
///
/// A `uint32_t` field is one of the enumerators below; the type itself is not a field type of
/// the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_link_idle_packet {
    /// A Space Packet Idle Packet (133.0 §4.1.3.3.4.4).
    ALDIS_LINK_IDLE_PACKET_SPACE_PACKET = 0,
    /// An Encapsulation Idle Packet (133.1).
    ALDIS_LINK_IDLE_PACKET_ENCAPSULATION = 1,
}
mirror!(pub aldis_link_idle_packet: u32 {
    ALDIS_LINK_IDLE_PACKET_SPACE_PACKET,
    ALDIS_LINK_IDLE_PACKET_ENCAPSULATION
});

/// The channel a link engine serves, and the protocol it is built for (LNK-001). It is the
/// first member of every link engine configuration.
///
/// `aos_edition` is read for AOS only, and `uslp_upid`, `uslp_map`, and `uslp_qos` for USLP
/// only; the others ignore them.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_link_channel_t {
    /// `ALDIS_LINK_PROTOCOL_*`.
    pub protocol: u32,
    /// The Virtual Channel Identifier: 0 to 7 for TM, 0 to 63 for TC, AOS, and USLP.
    pub vc_id: u8,
    /// AOS: `ALDIS_AOS_EDITION_*` (AOS-001); 0 is Issue 5.
    pub aos_edition: u8,
    /// USLP: the UPID of the TFDFs, 0 to 31 but not "Only Idle Data" (732.1 §4.1.4.2.3).
    pub uslp_upid: u8,
    /// USLP: the MAP ID, 0 to 15 (732.1 §4.1.2.7).
    pub uslp_map: u8,
    /// USLP: `ALDIS_LINK_QOS_*`.
    pub uslp_qos: u32,
}

/// A queue or buffer in octets: the current fill and the highest it has been (API-056,
/// API-060).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_link_occupancy_t {
    /// The octets held now.
    pub current: usize,
    /// The most octets held since the engine was initialized.
    pub high_water: usize,
}

impl From<Occupancy> for aldis_link_occupancy_t {
    fn from(o: Occupancy) -> aldis_link_occupancy_t {
        aldis_link_occupancy_t {
            current: o.current(),
            high_water: o.high_water(),
        }
    }
}

/// The MAP and quality of service a field is sent on, as flat fields of an action (USLP).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RouteFields {
    /// 1 when the field has a route, else 0.
    pub(crate) present: u8,
    /// The MAP ID, 0 without a route.
    pub(crate) map_id: u8,
    /// `ALDIS_LINK_QOS_*`, 0 without a route.
    pub(crate) qos: u32,
}

impl From<Option<Route>> for RouteFields {
    fn from(route: Option<Route>) -> RouteFields {
        route.map_or_else(RouteFields::default, |r| RouteFields {
            present: 1,
            map_id: r.map.get(),
            qos: match r.qos {
                Qos::SequenceControlled => ALDIS_LINK_QOS_SEQUENCE_CONTROLLED,
                Qos::Expedited => ALDIS_LINK_QOS_EXPEDITED,
            },
        })
    }
}

/// The protocol of `channel` (LNK-001).
///
/// # Errors
/// `InvalidField` (no field) for a value that is no `ALDIS_LINK_PROTOCOL_*` constant.
pub(crate) fn protocol(channel: &aldis_link_channel_t) -> Result<aldis_link_protocol> {
    parse::<aldis_link_protocol>(channel.protocol)
}

/// The error for a protocol the engine does not serve, or a reconfiguration to another one
/// (LNK-001, LNK-003).
pub(crate) const fn unsupported_protocol() -> Error {
    Error::new(ErrorKind::InvalidConfig).with_field(field::LINK_PROTOCOL)
}

/// The VC of `channel` in the protocol `P` (132.0 §4.1.2.5; 232.0 §4.1.2.6; 732.0 §4.1.2.3;
/// 732.1 §4.1.2.5).
///
/// # Errors
/// `OutOfRange` (`vc_id`) above the VCID of the protocol.
pub(crate) fn vc<P: Protocol>(channel: &aldis_link_channel_t) -> Result<P::Vc> {
    P::Vc::try_from(channel.vc_id)
}

/// The AOS parameters of `channel` (AOS-003).
///
/// # Errors
/// `InvalidField` for an `aos_edition` that is no `ALDIS_AOS_EDITION_*` constant.
pub(crate) fn aos_params(channel: &aldis_link_channel_t) -> Result<AosParams> {
    Ok(AosParams {
        edition: match parse::<aldis_aos_edition>(channel.aos_edition)? {
            aldis_aos_edition::ALDIS_AOS_EDITION_ISSUE5 => AosEdition::Issue5,
            aldis_aos_edition::ALDIS_AOS_EDITION_ISSUE4 => AosEdition::Issue4Compatible,
        },
    })
}

/// The USLP parameters of `channel` (732.1 §4.1.4.2.3, §4.1.2.7, §4.1.2.8; LNK-043).
///
/// # Errors
/// `OutOfRange` (`upid`, `map_id`); `InvalidField` for a `uslp_qos` with no constant;
/// `InvalidConfig` (`upid`) for "Only Idle Data".
pub(crate) fn uslp_params(channel: &aldis_link_channel_t) -> Result<UslpParams> {
    let upid = Upid::from_bits(channel.uslp_upid).ok_or(Error::out_of_range(
        field::UPID,
        31,
        u64::from(channel.uslp_upid),
    ))?;
    let qos = match parse::<aldis_link_qos>(channel.uslp_qos)? {
        aldis_link_qos::ALDIS_LINK_QOS_SEQUENCE_CONTROLLED => Qos::SequenceControlled,
        aldis_link_qos::ALDIS_LINK_QOS_EXPEDITED => Qos::Expedited,
    };
    UslpParams::new(upid, MapId::<4>::try_from(channel.uslp_map)?, qos)
}

/// The valid PVN set of two flags: Space Packets (PVN 0) and Encapsulation Packets (PVN 7).
///
/// # Errors
/// `InvalidField` for a flag that is not 0 or 1; `InvalidConfig` (`valid_pvns`) for neither.
pub(crate) fn valid_pvns(space: u8, encapsulation: u8) -> Result<ValidPvns> {
    ValidPvns::new(flag(space)?, flag(encapsulation)?)
}

/// The idle policy of a C constant (LNK-014).
pub(crate) fn idle_policy(value: u32) -> Result<IdlePolicy> {
    Ok(match parse::<aldis_link_idle_policy>(value)? {
        aldis_link_idle_policy::ALDIS_LINK_IDLE_POLICY_OID => IdlePolicy::Oid,
        aldis_link_idle_policy::ALDIS_LINK_IDLE_POLICY_IDLE_FIELD => IdlePolicy::IdleField,
    })
}

/// The idle packet kind of a C constant (LNK-013).
pub(crate) fn idle_packet(value: u32) -> Result<IdlePacketKind> {
    Ok(match parse::<aldis_link_idle_packet>(value)? {
        aldis_link_idle_packet::ALDIS_LINK_IDLE_PACKET_SPACE_PACKET => IdlePacketKind::SpacePacket,
        aldis_link_idle_packet::ALDIS_LINK_IDLE_PACKET_ENCAPSULATION => {
            IdlePacketKind::Encapsulation
        }
    })
}
