//! C ABI of the `PacketExtractor` and the `FrameReceiver` (LNK-050…LNK-054, LNK-060…LNK-064,
//! LNK-077, LNK-079…LNK-081, LNK-003, CABI-030…032, CABI-040; M4 plan D4, WP-7b).
//!
//! Each engine is one opaque handle (`aldis_packet_extractor_t`, `aldis_frame_receiver_t`) in
//! the pattern of the generation engines of [`crate::link_gen`]: the caller declares the typed
//! storage, `_init` places the engine of the protocol named in the configuration in it, and every
//! input is one `_step` call that returns the actions of the input in a fixed-capacity actions
//! struct. The octets of a delivered part go to the caller's `out` area and the action holds
//! their `aldis_range_t` (API-061). The status is a read-only call (API-056); there is no
//! callback. A failed call leaves the engine as it was and writes no actions (API-058).
//!
//! **Protocol.** The first member of a configuration names the protocol (`protocol`, an
//! `ALDIS_LINK_PROTOCOL_*` constant). The `PacketExtractor` serves TM, AOS, and USLP (its
//! configuration holds the `aldis_link_channel_t` of the generators; its `vc_id` is not read,
//! because the extractor serves the data fields the receiver delivers for one VC); the
//! `FrameReceiver` serves TM, TC, AOS, and USLP.
//!
//! **Frames.** A frame reaches the `FrameReceiver` as octets: the handle decodes it with the
//! codec of the protocol and the channel of its configuration (the FECF, and the FHEC of AOS,
//! are checked there, RS-023), takes it through `NoSecurity`, and gives the `Plain` frame to the
//! engine. A caller whose channel applies SDLS to the frame, or whose frame the codec refuses,
//! reports the loss with `CALLER_DISCARD` (LNK-080). The codec's error for a frame it refuses is
//! the error of the call, and the engine does not see the frame.
//!
//! The extractor holds an assembly buffer of `ALDIS_LINK_PACKET_QUEUE_OCTETS` octets and takes a
//! data field of `ALDIS_LINK_MAX_FRAME_OCTETS` at most; the receiver holds
//! `ALDIS_LINK_MAX_VIRTUAL_CHANNELS` VC entries (CABI-030).

use aldis::aos::{AosEdition, AosFrame, AosPhysicalChannelConfig, AosVirtualChannelConfig};
use aldis::core::ids::{MapId, SpacecraftId, VcId};
use aldis::core::security::NoSecurity;
use aldis::core::{Error, ErrorKind, Result, SecuritySlices, field};
use aldis::link::extractor::{
    EXTRACTOR_MAX_ACTIONS, ExtractionError, ExtractorActions, ExtractorInput, ExtractorOutput,
    ExtractorStatus, FieldIn, PacketExtractor, PacketExtractorBuffers, PacketExtractorConfig,
    PacketExtractorHead,
};
use aldis::link::protocol::{Aos, PacketProtocol, Protocol, Tc, Tm, TmParams, Uslp};
use aldis::link::receiver::{
    AosChannel, AosContent, AosVc, CallerDiscardReason, ChannelScope, DeliveryService,
    DiscardReason, FRAME_RECEIVER_MAX_ACTIONS, FrameReceiver, FrameReceiverConfig, MapSet,
    ReceiveProtocol, ReceiverInput, ReceiverOutput, ReceiverStatus, TcChannel, TcVc, TmChannel,
    TmVc, UslpChannel, UslpVc, VcEntry,
};
use aldis::tc::{
    MapConfig as TcMapConfig, PhysicalChannelConfig as TcPhysical, TcFrame, TcMapId,
    TcSpacecraftId, TcVcId, VcConfig as TcVcConfig,
};
use aldis::tm::{
    DataFieldContent, FshLen, TmFrame, TmPhysicalChannelConfig, TmVirtualChannelConfig,
};
use aldis::uslp::{
    MapConfig as UslpMapConfig, PhysicalChannelConfig as UslpPhysical, TransferFrameType,
    UslpFrame, UslpMapId, UslpSpacecraftId, UslpVcId, VcConfig as UslpVcConfig,
};

use crate::abi::{self, flag};
use crate::capacities::{
    ALDIS_LINK_MAX_FRAME_OCTETS, ALDIS_LINK_MAX_VIRTUAL_CHANNELS, ALDIS_LINK_PACKET_QUEUE_OCTETS,
};
use crate::handle::{self, Storage};
use crate::link::{
    aldis_link_channel_t, aldis_link_occupancy_t, aldis_link_protocol, aos_params, protocol,
    unsupported_protocol, uslp_params, valid_pvns,
};
use crate::link_common::{engine_in_place, range, variant_payload};
use crate::mirror::{mirror, parse};
use crate::range::aldis_range_t;

/// The data field capacity of the extractor: `ALDIS_LINK_MAX_FRAME_OCTETS` (CABI-030).
const D: usize = ALDIS_LINK_MAX_FRAME_OCTETS;
/// The assembly buffer of the extractor: `ALDIS_LINK_PACKET_QUEUE_OCTETS` (CABI-030).
const M: usize = ALDIS_LINK_PACKET_QUEUE_OCTETS;
/// The VC entries of the receiver: `ALDIS_LINK_MAX_VIRTUAL_CHANNELS` (CABI-030).
const V: usize = ALDIS_LINK_MAX_VIRTUAL_CHANNELS;

/// The most actions one `aldis_packet_extractor_step` returns (LNK-074): a run of packets, an
/// incomplete packet, a discard, and an error.
pub const ALDIS_PACKET_EXTRACTOR_MAX_ACTIONS: usize = 4;
/// The most actions one `aldis_frame_receiver_step` returns (LNK-074).
pub const ALDIS_FRAME_RECEIVER_MAX_ACTIONS: usize = 5;

const _: () = assert!(ALDIS_PACKET_EXTRACTOR_MAX_ACTIONS == EXTRACTOR_MAX_ACTIONS);
const _: () = assert!(ALDIS_FRAME_RECEIVER_MAX_ACTIONS == FRAME_RECEIVER_MAX_ACTIONS);

/// 64-bit words of one `aldis_packet_extractor_t` (CABI-031): the tag and the largest of the
/// engines of TM, AOS, and USLP at the build capacities; the build fails when it outgrows it.
pub const ALDIS_PACKET_EXTRACTOR_WORDS: usize = 1043;
/// 64-bit words of one `aldis_frame_receiver_t` (CABI-031): the tag and the largest of the
/// engines of TM, TC, AOS, and USLP at the build capacities.
pub const ALDIS_FRAME_RECEIVER_WORDS: usize = 82;

const _: () = assert!(
    size_of::<crate::handle::Slot<PacketExtractorState>>() <= ALDIS_PACKET_EXTRACTOR_WORDS * 8
);
const _: () =
    assert!(size_of::<crate::handle::Slot<FrameReceiverState>>() <= ALDIS_FRAME_RECEIVER_WORDS * 8);

// ---- Shared constants ----

/// Where the OCF or the Secondary Header of a frame is delivered (132.0 §3.5–3.9, 732.1
/// §3.11.2.4; LNK-053).
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
pub enum aldis_link_service {
    /// The part is not delivered.
    ALDIS_LINK_SERVICE_NONE = 0,
    /// The part is delivered for the virtual channel of the frame (`VC_OCF`, `VC_FSH`).
    ALDIS_LINK_SERVICE_VC = 1,
    /// The part is delivered for the master channel (`MC_OCF`, `MC_FSH`).
    ALDIS_LINK_SERVICE_MC = 2,
}
mirror!(pub aldis_link_service: u32 {
    ALDIS_LINK_SERVICE_NONE,
    ALDIS_LINK_SERVICE_VC,
    ALDIS_LINK_SERVICE_MC
});

/// What a virtual channel of a `FrameReceiver` carries (LNK-050, LNK-077).
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
pub enum aldis_link_content {
    /// Packets (TM, AOS): the data field goes to a `PacketExtractor`.
    ALDIS_LINK_CONTENT_PACKETS = 0,
    /// `VCA_SDU`s (TM, AOS): each data field is one SDU. USLP and TC use this for a served VC.
    ALDIS_LINK_CONTENT_VCA = 1,
    /// `B_PDU`s (AOS): the valid bits of each are delivered.
    ALDIS_LINK_CONTENT_BITSTREAM = 2,
    /// A VC whose service the receiver does not provide: its frames are `NotServed`.
    ALDIS_LINK_CONTENT_UNSERVED = 3,
}
mirror!(pub aldis_link_content: u32 {
    ALDIS_LINK_CONTENT_PACKETS,
    ALDIS_LINK_CONTENT_VCA,
    ALDIS_LINK_CONTENT_BITSTREAM,
    ALDIS_LINK_CONTENT_UNSERVED
});

/// Why a frame was discarded (LNK-050, LNK-080; docs/04-engines.md §5).
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
pub enum aldis_link_discard {
    /// The VC (or, for USLP, the MAP) is not configured.
    ALDIS_LINK_DISCARD_UNKNOWN_CHANNEL = 0,
    /// The VC is configured, but the receiver does not provide its service.
    ALDIS_LINK_DISCARD_NOT_SERVED = 1,
    /// The FECF did not match (a reason a caller reports).
    ALDIS_LINK_DISCARD_CRC_MISMATCH = 2,
    /// The AOS FHEC could not be corrected (a reason a caller reports).
    ALDIS_LINK_DISCARD_HEADER_UNCORRECTABLE = 3,
    /// SDLS verification failed (a reason a caller reports).
    ALDIS_LINK_DISCARD_SECURITY = 4,
}
mirror!(pub aldis_link_discard: u32 {
    ALDIS_LINK_DISCARD_UNKNOWN_CHANNEL,
    ALDIS_LINK_DISCARD_NOT_SERVED,
    ALDIS_LINK_DISCARD_CRC_MISMATCH,
    ALDIS_LINK_DISCARD_HEADER_UNCORRECTABLE,
    ALDIS_LINK_DISCARD_SECURITY
});

/// The channel an action of a `FrameReceiver` is about (LNK-051, LNK-053).
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
pub enum aldis_link_scope {
    /// The master channel.
    ALDIS_LINK_SCOPE_MASTER_CHANNEL = 0,
    /// A virtual channel (`vc_id`).
    ALDIS_LINK_SCOPE_VIRTUAL_CHANNEL = 1,
}
mirror!(pub aldis_link_scope: u32 {
    ALDIS_LINK_SCOPE_MASTER_CHANNEL,
    ALDIS_LINK_SCOPE_VIRTUAL_CHANNEL
});

// ---- PacketExtractor ----

type Pe<P> = PacketExtractor<P, M, D>;

/// The `PacketExtractor` of the protocol chosen at `aldis_packet_extractor_init` (LNK-001).
///
/// `repr(u8)`: `aldis_packet_extractor_init` writes a variant in place (`variant_payload`).
#[repr(u8)]
#[expect(
    dead_code,
    reason = "the variants are written in place by the init function, not constructed"
)]
pub(crate) enum PacketExtractorState {
    /// TM.
    Tm(Pe<Tm>) = 0,
    /// AOS.
    Aos(Pe<Aos>) = 1,
    /// USLP.
    Uslp(Pe<Uslp>) = 2,
}

/// The kind of an `aldis_packet_extractor_input_t` (LNK-060…LNK-064, LNK-003; API-057).
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
pub enum aldis_packet_extractor_input {
    /// A data field of the VC (`data`, `header_pointer`, `gap`): LNK-060.
    ALDIS_PACKET_EXTRACTOR_INPUT_DATA_FIELD = 0,
    /// Replace the configuration (`config`) between data units, discarding the packet in
    /// progress (LNK-003). The protocol cannot change.
    ALDIS_PACKET_EXTRACTOR_INPUT_RECONFIGURE = 1,
}
mirror!(pub aldis_packet_extractor_input: u32 {
    ALDIS_PACKET_EXTRACTOR_INPUT_DATA_FIELD,
    ALDIS_PACKET_EXTRACTOR_INPUT_RECONFIGURE
});

/// The kind of an `aldis_packet_extractor_action_t` (API-057).
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
pub enum aldis_packet_extractor_action {
    /// Complete packets, one after the other in `out` (LNK-060).
    ALDIS_PACKET_EXTRACTOR_ACTION_PACKETS = 0,
    /// A partial packet flagged incomplete, in `out` (LNK-061).
    ALDIS_PACKET_EXTRACTOR_ACTION_INCOMPLETE_PACKET = 1,
    /// The partial packet was discarded: `octets` (LNK-061, LNK-003).
    ALDIS_PACKET_EXTRACTOR_ACTION_DISCARDED = 2,
    /// An extraction error (`error`; LNK-061, LNK-062).
    ALDIS_PACKET_EXTRACTOR_ACTION_ERROR = 3,
}
mirror!(pub aldis_packet_extractor_action: u32 {
    ALDIS_PACKET_EXTRACTOR_ACTION_PACKETS,
    ALDIS_PACKET_EXTRACTOR_ACTION_INCOMPLETE_PACKET,
    ALDIS_PACKET_EXTRACTOR_ACTION_DISCARDED,
    ALDIS_PACKET_EXTRACTOR_ACTION_ERROR
});

/// An extraction error of a `PacketExtractor` (LNK-061, LNK-062; docs/04-engines.md §5).
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
pub enum aldis_packet_extractor_error {
    /// A packet is longer than the Maximum Packet Length.
    ALDIS_PACKET_EXTRACTOR_ERROR_PACKET_TOO_LONG = 0,
    /// A packet's PVN is not in the valid set.
    ALDIS_PACKET_EXTRACTOR_ERROR_INVALID_PVN = 1,
    /// The length fields disagree with the First Header Pointer, or a length field is invalid.
    ALDIS_PACKET_EXTRACTOR_ERROR_LENGTH_INCONSISTENT = 2,
}
mirror!(pub aldis_packet_extractor_error: u32 {
    ALDIS_PACKET_EXTRACTOR_ERROR_PACKET_TOO_LONG,
    ALDIS_PACKET_EXTRACTOR_ERROR_INVALID_PVN,
    ALDIS_PACKET_EXTRACTOR_ERROR_LENGTH_INCONSISTENT
});

/// A `PacketExtractor` in caller storage: the caller declares an object of this type, and the
/// pointer to it is the handle (LNK-060, CABI-031, CABI-032). The words are the library's.
#[repr(C)]
pub struct aldis_packet_extractor_t {
    /// The slot of [`handle`]: the tag, then the state.
    pub words: [u64; ALDIS_PACKET_EXTRACTOR_WORDS],
}

impl Storage for aldis_packet_extractor_t {
    type State = PacketExtractorState;
    const MAGIC: u32 = 0x414C_5045; // "ALPE"
}

/// The managed parameters of a `PacketExtractor` (132.0 §5.4 Table 5-4; 732.0 §5.5 Table 5-4;
/// 732.1 §5.6 Table 5-5; LNK-060…LNK-063).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_packet_extractor_config_t {
    /// The protocol (TM, AOS, or USLP) and its parameters; `vc_id` is not read.
    pub channel: aldis_link_channel_t,
    /// 1 when Space Packets (PVN 0) are valid, else 0.
    pub space_packets: u8,
    /// 1 when Encapsulation Packets (PVN 7) are valid, else 0. At least one of the two is 1.
    pub encapsulation_packets: u8,
    /// The longest packet in octets, at least 1 and at most `ALDIS_LINK_PACKET_QUEUE_OCTETS`.
    pub max_packet_len: usize,
    /// 1 to deliver partial packets flagged incomplete (LNK-061), else 0.
    pub deliver_incomplete: u8,
    /// 1 to deliver idle packets (LNK-063), else 0.
    pub deliver_idle: u8,
}

/// One input of a `PacketExtractor` (API-057, CABI-040). A field that the kind does not use is
/// ignored.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_packet_extractor_input_t {
    /// `ALDIS_PACKET_EXTRACTOR_INPUT_*`.
    pub kind: u32,
    /// For `DATA_FIELD`: the data field as the `FrameReceiver` delivers it, read once and copied
    /// (API-060). `data` is checked on every kind and must not overlap the storage or `out`:
    /// `NULL` only with `data_len` 0.
    pub data: *const u8,
    /// The octets at `data`.
    pub data_len: usize,
    /// For `DATA_FIELD`: 1 when `header_pointer` is the First Header Pointer of the TM frame
    /// header (TM only; AOS and USLP carry it in `data`), else 0.
    pub header_pointer_present: u8,
    /// For `DATA_FIELD`: the First Header Pointer of the TM frame header.
    pub header_pointer: u16,
    /// For `DATA_FIELD`: 1 when a frame gap was flagged on this field (LNK-051, LNK-061).
    pub gap: u8,
    /// For `RECONFIGURE`: the new configuration.
    pub config: aldis_packet_extractor_config_t,
}

/// One action of a `PacketExtractor` (API-057, API-061). A field that the kind does not use is
/// 0.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_packet_extractor_action_t {
    /// `ALDIS_PACKET_EXTRACTOR_ACTION_*`.
    pub kind: u32,
    /// For `PACKETS`: where the run of complete packets is in `out`, each as long as its own
    /// length field says; for `INCOMPLETE_PACKET`: where the packet is.
    pub packets: aldis_range_t,
    /// For `PACKETS`: how many packets the run holds.
    pub count: usize,
    /// For `INCOMPLETE_PACKET`: the Packet Version Number in the header.
    pub pvn: u8,
    /// For `DISCARDED`: the octets of the partial packet.
    pub octets: usize,
    /// For `ERROR`: `ALDIS_PACKET_EXTRACTOR_ERROR_*`.
    pub error: u32,
}

/// The actions of one input of a `PacketExtractor`.
///
/// Requirements: LNK-050, LNK-060, LNK-074, CABI-040; docs/04-engines.md §4.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_packet_extractor_actions_t {
    /// The number of actions in `items`, at most `ALDIS_PACKET_EXTRACTOR_MAX_ACTIONS`.
    pub count: usize,
    /// The actions; those from `count` on are zero.
    pub items: [aldis_packet_extractor_action_t; ALDIS_PACKET_EXTRACTOR_MAX_ACTIONS],
}

/// A read-only snapshot of a `PacketExtractor` (API-056, LNK-002).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_packet_extractor_status_t {
    /// Data fields taken.
    pub fields_in: u64,
    /// Packets returned, incomplete ones included.
    pub packets_out: u64,
    /// Packets returned flagged incomplete.
    pub incomplete_out: u64,
    /// Idle packets seen, whether delivered or dropped (LNK-063).
    pub idle_packets: u64,
    /// Octets of partial packets discarded (LNK-061).
    pub discarded_octets: u64,
    /// Fields flagged with a frame gap (LNK-051).
    pub gaps: u64,
    /// Extraction errors `PacketTooLong` (LNK-062).
    pub packet_too_long: u64,
    /// Extraction errors `InvalidPvn` (LNK-062).
    pub invalid_pvn: u64,
    /// Extraction errors `LengthInconsistent` (LNK-061).
    pub length_inconsistent: u64,
    /// The assembly buffer: the octets of the packet in progress.
    pub assembly: aldis_link_occupancy_t,
}

/// The `PacketExtractor` configuration of `c` for the protocol `P` with the parameters `params`.
fn pe_config<P: PacketProtocol>(
    c: &aldis_packet_extractor_config_t,
    params: P::Params,
) -> Result<PacketExtractorConfig<P>> {
    Ok(PacketExtractorConfig::<P>::new(
        params,
        valid_pvns(c.space_packets, c.encapsulation_packets)?,
        c.max_packet_len,
    )?
    .with_delivery(flag(c.deliver_incomplete)?, flag(c.deliver_idle)?))
}

/// The head of the engine of `c`; the protocol of the engine is fixed here (LNK-001).
enum PeHead {
    Tm(PacketExtractorHead<Tm>),
    Aos(PacketExtractorHead<Aos>),
    Uslp(PacketExtractorHead<Uslp>),
}

fn pe_head(c: &aldis_packet_extractor_config_t) -> Result<PeHead> {
    match protocol(&c.channel)? {
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM => {
            Ok(PeHead::Tm(Pe::new_head(pe_config(c, TmParams)?)?))
        }
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS => Ok(PeHead::Aos(Pe::new_head(pe_config(
            c,
            aos_params(&c.channel)?,
        )?)?)),
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP => Ok(PeHead::Uslp(Pe::new_head(
            pe_config(c, uslp_params(&c.channel)?)?,
        )?)),
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_TC => Err(unsupported_protocol()),
    }
}

/// Builds the engine of `head` at `state`, in place (#240).
///
/// # Safety
/// `state` is valid for writes of one `PacketExtractorState`.
#[expect(unsafe_code, reason = "initializes the engine in place (GEN-071)")]
unsafe fn pe_place(state: *mut PacketExtractorState, head: PeHead) {
    type B = PacketExtractorBuffers<M>;
    match head {
        PeHead::Tm(h) => {
            // SAFETY: as the contract says; the tag 0 is the discriminant of the variant.
            let p = unsafe { variant_payload::<_, Pe<Tm>>(state, 0) };
            engine_in_place!(Pe<Tm>, PacketExtractorHead<Tm>, B, p, h);
        }
        PeHead::Aos(h) => {
            // SAFETY: as the contract says; the tag 1 is the discriminant of the variant.
            let p = unsafe { variant_payload::<_, Pe<Aos>>(state, 1) };
            engine_in_place!(Pe<Aos>, PacketExtractorHead<Aos>, B, p, h);
        }
        PeHead::Uslp(h) => {
            // SAFETY: as the contract says; the tag 2 is the discriminant of the variant.
            let p = unsafe { variant_payload::<_, Pe<Uslp>>(state, 2) };
            engine_in_place!(Pe<Uslp>, PacketExtractorHead<Uslp>, B, p, h);
        }
    }
}

/// One step of `engine`, in the protocol `P` (API-057).
fn pe_run<P: PacketProtocol>(
    engine: &mut Pe<P>,
    input: &aldis_packet_extractor_input_t,
    data: &[u8],
    params: Result<P::Params>,
    area: &mut [u8],
) -> Result<ExtractorActions> {
    let input = match parse::<aldis_packet_extractor_input>(input.kind)? {
        aldis_packet_extractor_input::ALDIS_PACKET_EXTRACTOR_INPUT_DATA_FIELD => {
            ExtractorInput::DataField(FieldIn {
                field: data,
                header_pointer: if flag(input.header_pointer_present)? {
                    Some(input.header_pointer)
                } else {
                    None
                },
                gap: flag(input.gap)?,
            })
        }
        aldis_packet_extractor_input::ALDIS_PACKET_EXTRACTOR_INPUT_RECONFIGURE => {
            ExtractorInput::Reconfigure(pe_config(&input.config, params?)?)
        }
    };
    engine.step(input, area)
}

impl PacketExtractorState {
    /// One step: the input is parsed once into the engine's type (CABI-040).
    fn step(
        &mut self,
        input: &aldis_packet_extractor_input_t,
        data: &[u8],
        area: &mut [u8],
    ) -> Result<ExtractorActions> {
        let reconfigure = parse::<aldis_packet_extractor_input>(input.kind)?
            == aldis_packet_extractor_input::ALDIS_PACKET_EXTRACTOR_INPUT_RECONFIGURE;
        let ch = &input.config.channel;
        // A reconfiguration keeps the protocol of the engine (LNK-003).
        let same = |want| protocol(ch).map(|p| p == want);
        match self {
            PacketExtractorState::Tm(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM)? {
                    return Err(unsupported_protocol());
                }
                pe_run(e, input, data, Ok(TmParams), area)
            }
            PacketExtractorState::Aos(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS)? {
                    return Err(unsupported_protocol());
                }
                pe_run(e, input, data, aos_params(ch), area)
            }
            PacketExtractorState::Uslp(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP)? {
                    return Err(unsupported_protocol());
                }
                pe_run(e, input, data, uslp_params(ch), area)
            }
        }
    }

    const fn status(&self) -> ExtractorStatus {
        match self {
            PacketExtractorState::Tm(e) => *e.status(),
            PacketExtractorState::Aos(e) => *e.status(),
            PacketExtractorState::Uslp(e) => *e.status(),
        }
    }
}

fn pe_action(output: &ExtractorOutput) -> aldis_packet_extractor_action_t {
    match output {
        ExtractorOutput::Packets { run, count } => aldis_packet_extractor_action_t {
            kind: ALDIS_PACKET_EXTRACTOR_ACTION_PACKETS,
            packets: range(*run),
            count: *count,
            ..aldis_packet_extractor_action_t::default()
        },
        ExtractorOutput::IncompletePacket { pvn, packet } => aldis_packet_extractor_action_t {
            kind: ALDIS_PACKET_EXTRACTOR_ACTION_INCOMPLETE_PACKET,
            packets: range(*packet),
            pvn: *pvn,
            ..aldis_packet_extractor_action_t::default()
        },
        ExtractorOutput::Discarded { octets } => aldis_packet_extractor_action_t {
            kind: ALDIS_PACKET_EXTRACTOR_ACTION_DISCARDED,
            octets: *octets,
            ..aldis_packet_extractor_action_t::default()
        },
        ExtractorOutput::Error(e) => aldis_packet_extractor_action_t {
            kind: ALDIS_PACKET_EXTRACTOR_ACTION_ERROR,
            error: match e {
                ExtractionError::PacketTooLong => ALDIS_PACKET_EXTRACTOR_ERROR_PACKET_TOO_LONG,
                ExtractionError::InvalidPvn => ALDIS_PACKET_EXTRACTOR_ERROR_INVALID_PVN,
                ExtractionError::LengthInconsistent => {
                    ALDIS_PACKET_EXTRACTOR_ERROR_LENGTH_INCONSISTENT
                }
            },
            ..aldis_packet_extractor_action_t::default()
        },
    }
}

/// Places a `PacketExtractor` that has not yet synchronised in `storage` (LNK-060, LNK-061;
/// CABI-031, CABI-032).
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` for `NULL` or misaligned `storage` or `cfg`, a flag that is not 0
/// or 1, or a constant that is none; `ALDIS_ERR_INVALID_CONFIG` (`link_protocol`) for TC,
/// (`valid_pvns`, `max_packet_len`) for a value the configuration relations refuse;
/// `ALDIS_ERR_OUT_OF_RANGE` (`upid`, `map_id`); `ALDIS_ERR_CAPACITY_EXCEEDED`
/// (`max_packet_len`) above the assembly buffer or below the longest packet header.
///
/// # Safety
/// `storage` is valid for writes of one `aldis_packet_extractor_t` and outlives its use; `cfg`
/// is valid for a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_packet_extractor_init(
    storage: *mut aldis_packet_extractor_t,
    cfg: *const aldis_packet_extractor_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let cfg = abi::read(cfg)?;
            let head = pe_head(&cfg)?;
            // `init_with` passes a pointer to the state in the slot, valid for writes.
            handle::init_with(storage, |state| pe_place(state, head))
        })
    }
}

/// The engine behind `h`.
///
/// # Safety
/// As for `handle::get`.
#[expect(unsafe_code, reason = "checks a caller handle (CABI-032, GEN-071)")]
unsafe fn pe_engine(
    h: *mut aldis_packet_extractor_t,
) -> Result<handle::Handle<PacketExtractorState>> {
    // SAFETY: forwarded from the caller.
    unsafe { handle::get(h) }
}

/// Runs one input of the `PacketExtractor` (API-057, CABI-040).
///
/// The octets of the packets go to `out`, which must hold at least `max_packet_len − 1` plus
/// the octets of the packet zone of the data field (LNK-075).
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation, a flag that
/// is not 0 or 1, a constant that is none, a TM field without a pointer
/// (`first_header_pointer`); `ALDIS_ERR_MALFORMED` (`first_header_pointer`,
/// `fhp_lvo_pointer`) for a pointer outside the zone; `ALDIS_ERR_CAPACITY_EXCEEDED`
/// (`field_len`) for a data field above `ALDIS_LINK_MAX_FRAME_OCTETS`;
/// `ALDIS_ERR_BUFFER_TOO_SMALL` for a short `out`; for `RECONFIGURE`: as
/// `aldis_packet_extractor_init`, and `ALDIS_ERR_INVALID_CONFIG` (`link_protocol`) for another
/// protocol. The engine is unchanged and `actions` is not written.
///
/// # Safety
/// `h` is a handle from `aldis_packet_extractor_init` whose storage is still valid, or
/// detectably invalid storage; `input` is valid for a read, and its `data` for `data_len`
/// octets; `out` is valid for `out_cap` octets of writes (it may be `NULL` when `out_cap` is 0);
/// `actions` is valid for one write; `err` is `NULL` or valid for one write. No other thread
/// uses the storage (CABI-050).
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_packet_extractor_step(
    h: *mut aldis_packet_extractor_t,
    input: *const aldis_packet_extractor_input_t,
    out: *mut u8,
    out_cap: usize,
    actions: *mut aldis_packet_extractor_actions_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let mut h = pe_engine(h)?;
            let value = abi::read(input)?;
            let data = abi::bytes_disjoint(value.data, value.data_len, &[h.range()])?;
            let area = abi::bytes_mut(out, out_cap, &[(value.data, value.data_len), h.range()])?;
            let actions = abi::out_disjoint(
                actions,
                &[
                    h.range(),
                    (value.data, value.data_len),
                    (out.cast_const(), out_cap),
                ],
            )?;
            let list = h.update(|state| state.step(&value, data, area))?;
            let mut result = aldis_packet_extractor_actions_t::default();
            for (slot, output) in result.items.iter_mut().zip(list.iter()) {
                *slot = pe_action(output);
            }
            // `list` holds at most `EXTRACTOR_MAX_ACTIONS` actions, the length of `items`.
            result.count = list.len();
            actions.write(result);
            Ok(())
        })
    }
}

/// Reads the counters and the occupancy of the `PacketExtractor` (API-056, LNK-002). It changes
/// nothing.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation.
///
/// # Safety
/// As for [`aldis_packet_extractor_step`]; `status` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_packet_extractor_status(
    h: *mut aldis_packet_extractor_t,
    status: *mut aldis_packet_extractor_status_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = pe_engine(h)?;
            let status = abi::out_disjoint(status, &[h.range()])?;
            let s = h.read(PacketExtractorState::status);
            status.write(aldis_packet_extractor_status_t {
                fields_in: s.fields_in.get(),
                packets_out: s.packets_out.get(),
                incomplete_out: s.incomplete_out.get(),
                idle_packets: s.idle_packets.get(),
                discarded_octets: s.discarded_octets.get(),
                gaps: s.gaps.get(),
                packet_too_long: s.packet_too_long.get(),
                invalid_pvn: s.invalid_pvn.get(),
                length_inconsistent: s.length_inconsistent.get(),
                assembly: s.assembly.into(),
            });
            Ok(())
        })
    }
}

/// Releases the storage: later calls on it fail with `ALDIS_ERR_INVALID_HANDLE` (CABI-032). An
/// invalid handle is ignored.
///
/// # Safety
/// As for [`aldis_packet_extractor_step`].
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_packet_extractor_deinit(h: *mut aldis_packet_extractor_t) {
    // SAFETY: forwarded from the caller; `handle::deinit` checks the tag.
    unsafe { handle::deinit(h) }
}

// ---- FrameReceiver ----

type Fr<P> = FrameReceiver<P, V>;

/// The `FrameReceiver` of the protocol chosen at `aldis_frame_receiver_init` (LNK-001).
///
/// The receiver holds no octet buffer, so its state is small and is placed by value.
pub(crate) enum FrameReceiverState {
    /// TM.
    Tm(Fr<Tm>),
    /// TC.
    Tc(Fr<Tc>),
    /// AOS.
    Aos(Fr<Aos>),
    /// USLP.
    Uslp(Fr<Uslp>),
}

/// The kind of an `aldis_frame_receiver_input_t` (LNK-050, LNK-053, LNK-079, LNK-080, LNK-003;
/// API-057).
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
pub enum aldis_frame_receiver_input {
    /// A frame (`data`): the handle decodes it with the codec of the protocol (LNK-050).
    ALDIS_FRAME_RECEIVER_INPUT_FRAME = 0,
    /// The octets of a frame the caller rejected, for the Insert Zone only (`data`; LNK-079).
    ALDIS_FRAME_RECEIVER_INPUT_RAW_FRAME = 1,
    /// A frame the caller discarded before the engine (`reason`; LNK-080).
    ALDIS_FRAME_RECEIVER_INPUT_CALLER_DISCARD = 2,
    /// The coding sublayer reports that frames were lost (LNK-053).
    ALDIS_FRAME_RECEIVER_INPUT_FRAME_LOSS = 3,
    /// Replace the configuration (`config`; LNK-003): the expected counts restart. The protocol
    /// cannot change.
    ALDIS_FRAME_RECEIVER_INPUT_RECONFIGURE = 4,
}
mirror!(pub aldis_frame_receiver_input: u32 {
    ALDIS_FRAME_RECEIVER_INPUT_FRAME,
    ALDIS_FRAME_RECEIVER_INPUT_RAW_FRAME,
    ALDIS_FRAME_RECEIVER_INPUT_CALLER_DISCARD,
    ALDIS_FRAME_RECEIVER_INPUT_FRAME_LOSS,
    ALDIS_FRAME_RECEIVER_INPUT_RECONFIGURE
});

/// The kind of an `aldis_frame_receiver_action_t` (API-057; docs/04-engines.md §4–§5).
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
pub enum aldis_frame_receiver_action {
    /// Frames are missing before this one (`scope`, `vc_id`, `missing`; LNK-051).
    ALDIS_FRAME_RECEIVER_ACTION_FRAME_GAP = 0,
    /// A count that repeats or goes backwards (`scope`, `vc_id`; LNK-051).
    ALDIS_FRAME_RECEIVER_ACTION_COUNT_DISCONTINUITY = 1,
    /// The frame was rejected (`reason`, `vc_present`, `vc_id`, `map_present`, `map_id`;
    /// LNK-050, LNK-080). It is the last action of the frame.
    ALDIS_FRAME_RECEIVER_ACTION_FRAME_DISCARDED = 2,
    /// The coding sublayer lost frames: the Frame Loss signal applies to every VC (LNK-053).
    ALDIS_FRAME_RECEIVER_ACTION_LOSS_FLAG = 3,
    /// The data field of a frame for a VC (`vc_id`, `data`, `bit_count_present`, `bit_count`,
    /// `pointer_present`, `pointer`, `gap`, `replay_present`, `replay`; LNK-050, LNK-077).
    ALDIS_FRAME_RECEIVER_ACTION_DATA_FIELD = 4,
    /// The OCF of a frame (`scope`, `vc_id`, `ocf`, `replay_present`, `replay`; LNK-053).
    ALDIS_FRAME_RECEIVER_ACTION_OCF = 5,
    /// The Secondary Header of a TM frame, with its ID octet (`scope`, `vc_id`, `data`;
    /// LNK-053).
    ALDIS_FRAME_RECEIVER_ACTION_FSH_SDU = 6,
    /// The Insert Zone of an AOS or USLP frame (`data`, `replay_present`, `replay`; LNK-053).
    ALDIS_FRAME_RECEIVER_ACTION_INSERT_SDU = 7,
    /// A TC frame, whole, for FARM-1 (`vc_id`, `data`; LNK-081).
    ALDIS_FRAME_RECEIVER_ACTION_TC_FRAME = 8,
    /// A USLP frame with the Protocol Control Command Flag set, whole, for the COP (`vc_id`,
    /// `data`; LNK-081).
    ALDIS_FRAME_RECEIVER_ACTION_PROTOCOL_CONTROL = 9,
}
mirror!(pub aldis_frame_receiver_action: u32 {
    ALDIS_FRAME_RECEIVER_ACTION_FRAME_GAP,
    ALDIS_FRAME_RECEIVER_ACTION_COUNT_DISCONTINUITY,
    ALDIS_FRAME_RECEIVER_ACTION_FRAME_DISCARDED,
    ALDIS_FRAME_RECEIVER_ACTION_LOSS_FLAG,
    ALDIS_FRAME_RECEIVER_ACTION_DATA_FIELD,
    ALDIS_FRAME_RECEIVER_ACTION_OCF,
    ALDIS_FRAME_RECEIVER_ACTION_FSH_SDU,
    ALDIS_FRAME_RECEIVER_ACTION_INSERT_SDU,
    ALDIS_FRAME_RECEIVER_ACTION_TC_FRAME,
    ALDIS_FRAME_RECEIVER_ACTION_PROTOCOL_CONTROL
});

/// A `FrameReceiver` in caller storage: the caller declares an object of this type, and the
/// pointer to it is the handle (LNK-050, CABI-031, CABI-032). The words are the library's.
#[repr(C)]
pub struct aldis_frame_receiver_t {
    /// The slot of [`handle`]: the tag, then the state.
    pub words: [u64; ALDIS_FRAME_RECEIVER_WORDS],
}

impl Storage for aldis_frame_receiver_t {
    type State = FrameReceiverState;
    const MAGIC: u32 = 0x414C_4652; // "ALFR"
}

/// One virtual channel of a `FrameReceiver` (132.0 §5 Table 5-2; 232.0 §5; 732.0 §5 Table 5-2;
/// 732.1 §5 Table 5-2; LNK-050).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_frame_receiver_vc_t {
    /// The VCID: 0 to 7 for TM, 0 to 63 for TC, AOS, and USLP. A VC is listed once.
    pub vc_id: u8,
    /// `ALDIS_LINK_CONTENT_*`. TM: `PACKETS`, `VCA`, `UNSERVED`; AOS: all four; TC and USLP:
    /// `UNSERVED`, else served.
    pub content: u32,
    /// TM and AOS: 1 when the frames of the VC carry an OCF, else 0.
    pub ocf: u8,
    /// TM: the octets of the Secondary Header with its ID octet (2 to 64), 0 for none.
    pub fsh_len: u8,
    /// USLP: 1 when the COP is in effect on the VC (732.1 §4.3.7.2), else 0.
    pub cop: u8,
    /// USLP: the MAP IDs of the VC as a bit mask, bit `n` for MAP `n`.
    pub maps: u16,
}

/// The configuration of a `FrameReceiver` (LNK-050, LNK-079; docs/04-engines.md §3.1).
///
/// A member that the protocol does not use is ignored.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_frame_receiver_config_t {
    /// `ALDIS_LINK_PROTOCOL_*`: TM, TC, AOS, or USLP.
    pub protocol: u32,
    /// The octets of a frame of the physical channel; the most, where frames vary in length (TC,
    /// variable-length USLP).
    pub frame_len: usize,
    /// 1 when frames carry a FECF, else 0.
    pub fecf: u8,
    /// 1 to accept raw frames (LNK-079), else 0; only with an Insert Zone.
    pub raw_frames: u8,
    /// TM, USLP: `ALDIS_LINK_SERVICE_*` of the OCF (132.0 §3.5–3.9, 732.1 §3.11.2.4).
    pub ocf_service: u32,
    /// TM: `ALDIS_LINK_SERVICE_*` of the Secondary Header.
    pub fsh_service: u32,
    /// AOS, USLP: the octets of the Insert Zone, 0 for none.
    pub insert_zone_len: usize,
    /// AOS: 1 when frames have an FHEC, else 0.
    pub aos_fhec: u8,
    /// AOS: `ALDIS_AOS_EDITION_*` (AOS-001); 0 is Issue 5.
    pub aos_edition: u8,
    /// USLP: 1 for a variable-length channel (no Insert Zone), else 0.
    pub uslp_variable: u8,
    /// The number of entries of `vcs`, at most `ALDIS_LINK_MAX_VIRTUAL_CHANNELS`.
    pub vc_count: usize,
    /// The virtual channels; those from `vc_count` on are not read.
    pub vcs: [aldis_frame_receiver_vc_t; ALDIS_LINK_MAX_VIRTUAL_CHANNELS],
}

/// One input of a `FrameReceiver` (API-057, CABI-040). A field that the kind does not use is
/// ignored.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_frame_receiver_input_t {
    /// `ALDIS_FRAME_RECEIVER_INPUT_*`.
    pub kind: u32,
    /// For `FRAME` and `RAW_FRAME`: the frame, read once and copied (API-060). `data` is checked
    /// on every kind and must not overlap the storage or `out`: `NULL` only with `data_len` 0.
    pub data: *const u8,
    /// The octets at `data`.
    pub data_len: usize,
    /// For `CALLER_DISCARD`: `ALDIS_LINK_DISCARD_CRC_MISMATCH`, `_HEADER_UNCORRECTABLE`, or
    /// `_SECURITY`.
    pub reason: u32,
    /// For `RECONFIGURE`: the new configuration.
    pub config: aldis_frame_receiver_config_t,
}

/// One action of a `FrameReceiver` (API-057, API-061). A field that the kind does not use is 0.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_frame_receiver_action_t {
    /// `ALDIS_FRAME_RECEIVER_ACTION_*`.
    pub kind: u32,
    /// For the actions about a channel: `ALDIS_LINK_SCOPE_*`.
    pub scope: u32,
    /// For `FRAME_DISCARDED`: `ALDIS_LINK_DISCARD_*`.
    pub reason: u32,
    /// For `FRAME_DISCARDED`: 1 when `vc_id` is the VC the engine read from the frame.
    pub vc_present: u8,
    /// The VCID of the scope, of the frame, or of the delivered part.
    pub vc_id: u8,
    /// For `FRAME_DISCARDED`: 1 when `map_id` is the MAP of the frame (USLP).
    pub map_present: u8,
    /// For `FRAME_DISCARDED`: the MAP ID.
    pub map_id: u8,
    /// 1 when `replay` is the Replay Flag of an AOS frame (LNK-054).
    pub replay_present: u8,
    /// The Replay Flag, 0 or 1.
    pub replay: u8,
    /// For `DATA_FIELD`: 1 when `pointer` is the First Header Pointer of a TM frame header.
    pub pointer_present: u8,
    /// For `DATA_FIELD`: the First Header Pointer.
    pub pointer: u16,
    /// For `DATA_FIELD`: 1 when a gap or discontinuity was detected for the VC since the last
    /// data field delivered for it (LNK-051, LNK-061).
    pub gap: u8,
    /// For `DATA_FIELD` of a `B_PDU`: 1 when `bit_count` is the number of valid bits.
    pub bit_count_present: u8,
    /// For `DATA_FIELD` of a `B_PDU`: the valid bits of `data` (LNK-077).
    pub bit_count: usize,
    /// For `OCF`: the 32 bits of the OCF as a big-endian word; `aldis_ocf_*` classifies it.
    pub ocf: u32,
    /// For `FRAME_GAP`: the number of frames missing.
    pub missing: u64,
    /// The delivered octets in `out`: the data field, the FSH, the Insert Zone, or the frame.
    pub data: aldis_range_t,
}

/// The actions of one input of a `FrameReceiver`.
///
/// Requirements: LNK-050, LNK-060, LNK-074, CABI-040; docs/04-engines.md §4.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_frame_receiver_actions_t {
    /// The number of actions in `items`, at most `ALDIS_FRAME_RECEIVER_MAX_ACTIONS`.
    pub count: usize,
    /// The actions; those from `count` on are zero.
    pub items: [aldis_frame_receiver_action_t; ALDIS_FRAME_RECEIVER_MAX_ACTIONS],
}

/// A read-only snapshot of a `FrameReceiver` (API-056, LNK-002).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_frame_receiver_status_t {
    /// Frames taken and processed, discarded ones included.
    pub frames_in: u64,
    /// Raw frames taken (LNK-079).
    pub raw_frames_in: u64,
    /// Inputs the receiver refused with an error (API-058).
    pub refused: u64,
    /// `FRAME_LOSS` inputs (LNK-053).
    pub frame_loss: u64,
    /// Frames discarded as unknown channel (LNK-050).
    pub unknown_channel: u64,
    /// Frames discarded as not served (LNK-050).
    pub not_served: u64,
    /// Frames the caller discarded for a wrong FECF (LNK-080).
    pub crc_mismatch: u64,
    /// Frames the caller discarded for an AOS FHEC it could not correct (LNK-080).
    pub header_uncorrectable: u64,
    /// Frames the caller discarded for a failed SDLS verification (LNK-080).
    pub security: u64,
    /// OID frames (LNK-052).
    pub oid_frames: u64,
    /// `FRAME_GAP` actions (LNK-051).
    pub gaps: u64,
    /// `COUNT_DISCONTINUITY` actions (LNK-051).
    pub count_discontinuities: u64,
}

/// The AOS edition of the configuration `c` (AOS-001).
///
/// # Errors
/// `InvalidField` for an `aos_edition` that is no `ALDIS_AOS_EDITION_*` constant.
fn aos_edition(c: &aldis_frame_receiver_config_t) -> Result<AosEdition> {
    let channel = aldis_link_channel_t {
        aos_edition: c.aos_edition,
        ..aldis_link_channel_t::default()
    };
    Ok(aos_params(&channel)?.edition)
}

fn service(value: u32) -> Result<DeliveryService> {
    Ok(match parse::<aldis_link_service>(value)? {
        aldis_link_service::ALDIS_LINK_SERVICE_NONE => DeliveryService::None,
        aldis_link_service::ALDIS_LINK_SERVICE_VC => DeliveryService::Vc,
        aldis_link_service::ALDIS_LINK_SERVICE_MC => DeliveryService::Mc,
    })
}

/// The error of a frame length the codec's physical channel refuses, named as the C member
/// `frame_len` (the codec says `max_frame_len` where the length is a maximum).
fn frame_len_member(e: Error) -> Error {
    if e.field() == Some(field::MAX_FRAME_LEN) {
        Error::new(ErrorKind::InvalidConfig).with_field(field::FRAME_LEN)
    } else {
        e
    }
}

const fn bad_content() -> Error {
    Error::new(ErrorKind::InvalidConfig).with_field(field::DATA_FIELD_CONTENT)
}

/// A protocol of the `FrameReceiver` in the C ABI: its configuration from the flat struct and its
/// `Plain` frame from octets (RS-023).
trait RxProtocol: ReceiveProtocol
where
    <Self as Protocol>::Vc: Into<u8>,
{
    /// The channel parameters of `c`.
    fn channel(c: &aldis_frame_receiver_config_t) -> Result<Self::Channel>;

    /// The parameters of the VC `vc` of `c`.
    fn vc_params(vc: &aldis_frame_receiver_vc_t, id: Self::Vc) -> Result<Self::VcParams>;

    /// The `Plain` frame of `octets` for `config`: the codec decodes it, and it takes the `Plain`
    /// step through `NoSecurity`. The codec knows no VC, as the receiver counts frames of
    /// channels that are not configured too (OI-77).
    fn plain<'a>(
        config: &FrameReceiverConfig<Self, V>,
        octets: &'a [u8],
    ) -> Result<Self::Plain<'a>>;
}

/// The receiver configuration of `c` for the protocol `P`.
fn rx_config<P: RxProtocol>(c: &aldis_frame_receiver_config_t) -> Result<FrameReceiverConfig<P, V>>
where
    <P as Protocol>::Vc: Into<u8>,
{
    let mut parsed: [Option<VcEntry<P>>; V] = [None; V];
    for (slot, vc) in parsed.iter_mut().zip(&c.vcs).take(c.vc_count) {
        let id = P::Vc::try_from(vc.vc_id)?;
        *slot = Some(VcEntry::new(id, P::vc_params(vc, id)?));
    }
    let channel = P::channel(c)?;
    let config = match parsed.first().copied().flatten() {
        // Without an entry there is nothing to list.
        None => FrameReceiverConfig::<P, V>::new(channel, &[])?,
        // The first entry fills the array; only the first `vc_count` of it are given on.
        Some(filler) => {
            let mut entries = [filler; V];
            for (dst, src) in entries.iter_mut().zip(parsed.iter().flatten()) {
                *dst = *src;
            }
            let listed = entries
                .get(..c.vc_count)
                .ok_or(Error::new(ErrorKind::CapacityExceeded).with_field(field::VC_ID))?;
            FrameReceiverConfig::<P, V>::new(channel, listed)?
        }
    };
    config.with_raw_frames(flag(c.raw_frames)?)
}

impl RxProtocol for Tm {
    fn channel(c: &aldis_frame_receiver_config_t) -> Result<TmChannel> {
        let ocf = service(c.ocf_service)?;
        let fsh = service(c.fsh_service)?;
        let physical = TmPhysicalChannelConfig::new(c.frame_len, flag(c.fecf)?)?;
        Ok(TmChannel::new(physical, ocf, fsh))
    }

    fn vc_params(vc: &aldis_frame_receiver_vc_t, id: VcId<3>) -> Result<TmVc> {
        let (content, served) = match parse::<aldis_link_content>(vc.content)? {
            aldis_link_content::ALDIS_LINK_CONTENT_PACKETS => (DataFieldContent::Packets, true),
            aldis_link_content::ALDIS_LINK_CONTENT_VCA => (DataFieldContent::VcaSdu, true),
            // The receiver does not serve the VC; what it carries is then of no concern.
            aldis_link_content::ALDIS_LINK_CONTENT_UNSERVED => (DataFieldContent::Packets, false),
            aldis_link_content::ALDIS_LINK_CONTENT_BITSTREAM => return Err(bad_content()),
        };
        let mut config = TmVirtualChannelConfig::new(id, content);
        if flag(vc.ocf)? {
            config = config.with_ocf();
        }
        if vc.fsh_len > 0 {
            let len = FshLen::new(vc.fsh_len)
                .ok_or(Error::new(ErrorKind::InvalidConfig).with_field(field::FSH_LEN))?;
            config = config.with_fsh(len);
        }
        Ok(TmVc::new(config, served))
    }

    fn plain<'a>(config: &FrameReceiverConfig<Tm, V>, octets: &'a [u8]) -> Result<Self::Plain<'a>> {
        TmFrame::decode(octets, config.channel().physical())?.into_plain(NoSecurity)
    }
}

/// A lookup that holds no AOS virtual channel. Correct because the receiver lays the frame out
/// itself from its own configuration (`plain_octets`); the codec only checks FECF, FHEC, and length.
struct NoAosVcs;

impl aldis::core::lookup::ChannelLookup<SpacecraftId<10>, VcId<6>> for NoAosVcs {
    type Vc = AosVirtualChannelConfig;

    fn has_spacecraft(&self, _: SpacecraftId<10>) -> bool {
        false
    }

    fn vc(&self, _: SpacecraftId<10>, _: VcId<6>) -> Option<AosVirtualChannelConfig> {
        None
    }
}

impl RxProtocol for Aos {
    fn channel(c: &aldis_frame_receiver_config_t) -> Result<AosChannel> {
        let fhec = flag(c.aos_fhec)?;
        let fecf = flag(c.fecf)?;
        let physical = |edition| {
            AosPhysicalChannelConfig::new(c.frame_len, fhec, c.insert_zone_len, fecf, edition)
        };
        // The edition is read after the length checks, as before it was a managed parameter of
        // the physical channel. On an invalid edition, `Issue5` is only a placeholder: it lets the
        // length checks of `physical` run first, and the edition error is returned after them.
        match aos_edition(c) {
            Ok(edition) => Ok(AosChannel::new(physical(edition)?)),
            Err(e) => physical(AosEdition::Issue5).and(Err(e)),
        }
    }

    fn vc_params(vc: &aldis_frame_receiver_vc_t, _: VcId<6>) -> Result<AosVc> {
        let content = match parse::<aldis_link_content>(vc.content)? {
            aldis_link_content::ALDIS_LINK_CONTENT_PACKETS => AosContent::Packets,
            aldis_link_content::ALDIS_LINK_CONTENT_VCA => AosContent::Vca,
            aldis_link_content::ALDIS_LINK_CONTENT_BITSTREAM => AosContent::Bitstream,
            aldis_link_content::ALDIS_LINK_CONTENT_UNSERVED => AosContent::Unserved,
        };
        Ok(AosVc::new(content, flag(vc.ocf)?))
    }

    fn plain<'a>(
        config: &FrameReceiverConfig<Aos, V>,
        octets: &'a [u8],
    ) -> Result<Self::Plain<'a>> {
        AosFrame::decode(octets, config.channel().physical(), &NoAosVcs)?.into_plain(NoSecurity)
    }
}

/// A lookup that holds no TC channel: as for AOS, the receiver reads the octets itself.
struct NoTcChannels(TcPhysical);

impl aldis::core::lookup::PhysicalLookup for NoTcChannels {
    type Physical = TcPhysical;

    fn physical_channel(&self) -> TcPhysical {
        self.0
    }
}

impl aldis::core::lookup::ChannelLookup<TcSpacecraftId, TcVcId> for NoTcChannels {
    type Vc = TcVcConfig;

    fn has_spacecraft(&self, _: TcSpacecraftId) -> bool {
        false
    }

    fn vc(&self, _: TcSpacecraftId, _: TcVcId) -> Option<TcVcConfig> {
        None
    }
}

impl aldis::core::lookup::MapLookup<TcSpacecraftId, TcVcId, TcMapId> for NoTcChannels {
    type Map = TcMapConfig;

    fn map(&self, _: TcSpacecraftId, _: TcVcId, _: TcMapId) -> Option<TcMapConfig> {
        None
    }
}

impl RxProtocol for Tc {
    fn channel(c: &aldis_frame_receiver_config_t) -> Result<TcChannel> {
        // The C member is `frame_len` for every protocol: the TC codec names the same length
        // `max_frame_len`.
        let physical = TcPhysical::new(c.frame_len, flag(c.fecf)?).map_err(frame_len_member)?;
        Ok(TcChannel::new(physical))
    }

    fn vc_params(vc: &aldis_frame_receiver_vc_t, _: VcId<6>) -> Result<TcVc> {
        Ok(match parse::<aldis_link_content>(vc.content)? {
            aldis_link_content::ALDIS_LINK_CONTENT_UNSERVED => TcVc::UNSERVED,
            aldis_link_content::ALDIS_LINK_CONTENT_PACKETS
            | aldis_link_content::ALDIS_LINK_CONTENT_VCA
            | aldis_link_content::ALDIS_LINK_CONTENT_BITSTREAM => TcVc::SERVED,
        })
    }

    fn plain<'a>(config: &FrameReceiverConfig<Tc, V>, octets: &'a [u8]) -> Result<Self::Plain<'a>> {
        // TC frames vary in length: the codec takes the length of the frame it is given.
        let channels = NoTcChannels(TcPhysical::new(octets.len(), config.fecf())?);
        TcFrame::decode(octets, &channels)?.into_plain(NoSecurity)
    }
}

/// A lookup that holds no USLP channel: as for AOS, the receiver reads the octets itself.
struct NoUslpChannels(UslpPhysical);

impl aldis::core::lookup::PhysicalLookup for NoUslpChannels {
    type Physical = UslpPhysical;

    fn physical_channel(&self) -> UslpPhysical {
        self.0
    }
}

impl aldis::core::lookup::ChannelLookup<UslpSpacecraftId, UslpVcId> for NoUslpChannels {
    type Vc = UslpVcConfig;

    fn has_spacecraft(&self, _: UslpSpacecraftId) -> bool {
        false
    }

    fn vc(&self, _: UslpSpacecraftId, _: UslpVcId) -> Option<UslpVcConfig> {
        None
    }
}

impl aldis::core::lookup::MapLookup<UslpSpacecraftId, UslpVcId, UslpMapId> for NoUslpChannels {
    type Map = UslpMapConfig;

    fn map(&self, _: UslpSpacecraftId, _: UslpVcId, _: UslpMapId) -> Option<UslpMapConfig> {
        None
    }
}

/// The set of MAP IDs of a bit mask.
fn map_set(mask: u16) -> Result<MapSet> {
    let set_bits = || (0..16u8).filter(move |bit| (mask >> bit) & 1 != 0);
    let Some(first) = set_bits().next() else {
        return Ok(MapSet::default());
    };
    // The first MAP fills the array, so the set holds only the MAPs of the mask.
    let mut ids = [MapId::<4>::try_from(first)?; 16];
    for (slot, bit) in ids.iter_mut().zip(set_bits()) {
        *slot = MapId::<4>::try_from(bit)?;
    }
    Ok(MapSet::new(&ids))
}

impl RxProtocol for Uslp {
    fn channel(c: &aldis_frame_receiver_config_t) -> Result<UslpChannel> {
        let ocf = service(c.ocf_service)?;
        let physical = if flag(c.uslp_variable)? {
            // A variable-length channel has no Insert Zone: the member is not read.
            UslpPhysical::new(TransferFrameType::Variable, c.frame_len, 0, flag(c.fecf)?)
                .map_err(frame_len_member)?
        } else {
            UslpPhysical::new(
                TransferFrameType::Fixed,
                c.frame_len,
                c.insert_zone_len,
                flag(c.fecf)?,
            )?
        };
        UslpChannel::new(physical, ocf)
    }

    fn vc_params(vc: &aldis_frame_receiver_vc_t, _: VcId<6>) -> Result<UslpVc> {
        let served = parse::<aldis_link_content>(vc.content)?
            != aldis_link_content::ALDIS_LINK_CONTENT_UNSERVED;
        Ok(UslpVc::new(map_set(vc.maps)?, flag(vc.cop)?, served))
    }

    fn plain<'a>(
        config: &FrameReceiverConfig<Uslp, V>,
        octets: &'a [u8],
    ) -> Result<Self::Plain<'a>> {
        // The codec takes the length of the frame it is given; the receiver refuses a frame of
        // another length than the channel's (LNK-050).
        let physical = config.channel().physical();
        let pc = UslpPhysical::new(
            physical.frame_type(),
            octets.len(),
            physical.insert_zone_len(),
            physical.fecf(),
        )?;
        UslpFrame::decode(octets, &NoUslpChannels(pc))?.into_plain(NoSecurity)
    }
}

/// A scope of an action, as the C struct writes it: the scope and the VCID.
fn scope_of<W: Into<u8>>(scope: ChannelScope<W>) -> (u32, u8) {
    match scope {
        ChannelScope::MasterChannel => (ALDIS_LINK_SCOPE_MASTER_CHANNEL, 0),
        ChannelScope::VirtualChannel(vc) => (ALDIS_LINK_SCOPE_VIRTUAL_CHANNEL, vc.into()),
    }
}

/// The Replay Flag of an action as presence and value.
fn replay_of(replay: Option<bool>) -> (u8, u8) {
    (u8::from(replay.is_some()), u8::from(replay == Some(true)))
}

const fn discard_reason(reason: DiscardReason) -> u32 {
    match reason {
        DiscardReason::UnknownChannel => ALDIS_LINK_DISCARD_UNKNOWN_CHANNEL,
        DiscardReason::NotServed => ALDIS_LINK_DISCARD_NOT_SERVED,
        DiscardReason::CrcMismatch => ALDIS_LINK_DISCARD_CRC_MISMATCH,
        DiscardReason::HeaderUncorrectable => ALDIS_LINK_DISCARD_HEADER_UNCORRECTABLE,
        DiscardReason::Security => ALDIS_LINK_DISCARD_SECURITY,
    }
}

fn caller_reason(value: u32) -> Result<CallerDiscardReason> {
    match parse::<aldis_link_discard>(value)? {
        aldis_link_discard::ALDIS_LINK_DISCARD_CRC_MISMATCH => Ok(CallerDiscardReason::CrcMismatch),
        aldis_link_discard::ALDIS_LINK_DISCARD_HEADER_UNCORRECTABLE => {
            Ok(CallerDiscardReason::HeaderUncorrectable)
        }
        aldis_link_discard::ALDIS_LINK_DISCARD_SECURITY => Ok(CallerDiscardReason::Security),
        // The engine decides these two; a caller does not report them.
        aldis_link_discard::ALDIS_LINK_DISCARD_UNKNOWN_CHANNEL
        | aldis_link_discard::ALDIS_LINK_DISCARD_NOT_SERVED => Err(abi::invalid_argument()),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one arm for each of the ten actions of the receiver"
)]
fn rx_action<P: ReceiveProtocol>(output: &ReceiverOutput<P>) -> aldis_frame_receiver_action_t
where
    <P as Protocol>::Vc: Into<u8>,
{
    let none = aldis_frame_receiver_action_t::default();
    match output {
        ReceiverOutput::FrameGap { scope, missing } => {
            let (scope, vc_id) = scope_of(*scope);
            aldis_frame_receiver_action_t {
                kind: ALDIS_FRAME_RECEIVER_ACTION_FRAME_GAP,
                scope,
                vc_id,
                missing: *missing,
                ..none
            }
        }
        ReceiverOutput::CountDiscontinuity { scope } => {
            let (scope, vc_id) = scope_of(*scope);
            aldis_frame_receiver_action_t {
                kind: ALDIS_FRAME_RECEIVER_ACTION_COUNT_DISCONTINUITY,
                scope,
                vc_id,
                ..none
            }
        }
        ReceiverOutput::FrameDiscarded { reason, vc, map } => aldis_frame_receiver_action_t {
            kind: ALDIS_FRAME_RECEIVER_ACTION_FRAME_DISCARDED,
            reason: discard_reason(*reason),
            vc_present: u8::from(vc.is_some()),
            vc_id: vc.map_or(0, Into::into),
            map_present: u8::from(map.is_some()),
            map_id: map.map_or(0, MapId::get),
            ..none
        },
        ReceiverOutput::LossFlag => aldis_frame_receiver_action_t {
            kind: ALDIS_FRAME_RECEIVER_ACTION_LOSS_FLAG,
            ..none
        },
        ReceiverOutput::DataField {
            vc,
            data,
            bit_count,
            pointer,
            gap,
            replay,
        } => {
            let (replay_present, replay) = replay_of(*replay);
            aldis_frame_receiver_action_t {
                kind: ALDIS_FRAME_RECEIVER_ACTION_DATA_FIELD,
                vc_id: (*vc).into(),
                data: range(*data),
                bit_count_present: u8::from(bit_count.is_some()),
                bit_count: bit_count.unwrap_or(0),
                pointer_present: u8::from(pointer.is_some()),
                pointer: pointer.unwrap_or(0),
                gap: u8::from(*gap),
                replay_present,
                replay,
                ..none
            }
        }
        ReceiverOutput::Ocf { scope, ocf, replay } => {
            let (scope, vc_id) = scope_of(*scope);
            let (replay_present, replay) = replay_of(*replay);
            aldis_frame_receiver_action_t {
                kind: ALDIS_FRAME_RECEIVER_ACTION_OCF,
                scope,
                vc_id,
                ocf: u32::from_be_bytes(ocf.to_octets()),
                replay_present,
                replay,
                ..none
            }
        }
        ReceiverOutput::FshSdu { scope, sdu } => {
            let (scope, vc_id) = scope_of(*scope);
            aldis_frame_receiver_action_t {
                kind: ALDIS_FRAME_RECEIVER_ACTION_FSH_SDU,
                scope,
                vc_id,
                data: range(*sdu),
                ..none
            }
        }
        ReceiverOutput::InsertSdu { sdu, replay } => {
            let (replay_present, replay) = replay_of(*replay);
            aldis_frame_receiver_action_t {
                kind: ALDIS_FRAME_RECEIVER_ACTION_INSERT_SDU,
                data: range(*sdu),
                replay_present,
                replay,
                ..none
            }
        }
        ReceiverOutput::TcFrame { vc, frame } => aldis_frame_receiver_action_t {
            kind: ALDIS_FRAME_RECEIVER_ACTION_TC_FRAME,
            vc_id: (*vc).into(),
            data: range(*frame),
            ..none
        },
        ReceiverOutput::ProtocolControl { vc, frame } => aldis_frame_receiver_action_t {
            kind: ALDIS_FRAME_RECEIVER_ACTION_PROTOCOL_CONTROL,
            vc_id: (*vc).into(),
            data: range(*frame),
            ..none
        },
    }
}

/// One step of `engine`, in the protocol `P` (API-057): the actions in their C form.
fn rx_run<P: RxProtocol>(
    engine: &mut Fr<P>,
    input: &aldis_frame_receiver_input_t,
    data: &[u8],
    area: &mut [u8],
) -> Result<aldis_frame_receiver_actions_t>
where
    <P as Protocol>::Vc: Into<u8>,
{
    let list = match parse::<aldis_frame_receiver_input>(input.kind)? {
        aldis_frame_receiver_input::ALDIS_FRAME_RECEIVER_INPUT_FRAME => {
            // A frame the codec refuses is the error of the call (RS-023).
            let plain = P::plain(engine.config(), data)?;
            engine.step(ReceiverInput::Frame(plain), area)?
        }
        aldis_frame_receiver_input::ALDIS_FRAME_RECEIVER_INPUT_RAW_FRAME => {
            engine.step(ReceiverInput::RawFrame(data), area)?
        }
        aldis_frame_receiver_input::ALDIS_FRAME_RECEIVER_INPUT_CALLER_DISCARD => engine.step(
            ReceiverInput::CallerDiscard(caller_reason(input.reason)?),
            area,
        )?,
        aldis_frame_receiver_input::ALDIS_FRAME_RECEIVER_INPUT_FRAME_LOSS => {
            engine.step(ReceiverInput::FrameLoss, area)?
        }
        aldis_frame_receiver_input::ALDIS_FRAME_RECEIVER_INPUT_RECONFIGURE => engine.step(
            ReceiverInput::Reconfigure(rx_config::<P>(&input.config)?),
            area,
        )?,
    };
    let mut result = aldis_frame_receiver_actions_t::default();
    for (slot, output) in result.items.iter_mut().zip(list.iter()) {
        *slot = rx_action(output);
    }
    // `list` holds at most `FRAME_RECEIVER_MAX_ACTIONS` actions, the length of `items`.
    result.count = list.len();
    Ok(result)
}

fn rx_state(c: &aldis_frame_receiver_config_t) -> Result<FrameReceiverState> {
    Ok(match parse::<aldis_link_protocol>(c.protocol)? {
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM => {
            FrameReceiverState::Tm(Fr::new(rx_config::<Tm>(c)?))
        }
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_TC => {
            FrameReceiverState::Tc(Fr::new(rx_config::<Tc>(c)?))
        }
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS => {
            FrameReceiverState::Aos(Fr::new(rx_config::<Aos>(c)?))
        }
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP => {
            FrameReceiverState::Uslp(Fr::new(rx_config::<Uslp>(c)?))
        }
    })
}

impl FrameReceiverState {
    /// One step: the input is parsed once into the engine's type (CABI-040).
    fn step(
        &mut self,
        input: &aldis_frame_receiver_input_t,
        data: &[u8],
        area: &mut [u8],
    ) -> Result<aldis_frame_receiver_actions_t> {
        let reconfigure = parse::<aldis_frame_receiver_input>(input.kind)?
            == aldis_frame_receiver_input::ALDIS_FRAME_RECEIVER_INPUT_RECONFIGURE;
        // A reconfiguration keeps the protocol of the engine (LNK-003).
        let same = |want: aldis_link_protocol| {
            parse::<aldis_link_protocol>(input.config.protocol).map(|p| p == want)
        };
        match self {
            FrameReceiverState::Tm(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM)? {
                    return Err(unsupported_protocol());
                }
                rx_run(e, input, data, area)
            }
            FrameReceiverState::Tc(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_TC)? {
                    return Err(unsupported_protocol());
                }
                rx_run(e, input, data, area)
            }
            FrameReceiverState::Aos(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS)? {
                    return Err(unsupported_protocol());
                }
                // The edition of the new configuration is read before its other members.
                if reconfigure {
                    aos_edition(&input.config)?;
                }
                rx_run(e, input, data, area)
            }
            FrameReceiverState::Uslp(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP)? {
                    return Err(unsupported_protocol());
                }
                rx_run(e, input, data, area)
            }
        }
    }

    const fn status(&self) -> ReceiverStatus {
        match self {
            FrameReceiverState::Tm(e) => *e.status(),
            FrameReceiverState::Tc(e) => *e.status(),
            FrameReceiverState::Aos(e) => *e.status(),
            FrameReceiverState::Uslp(e) => *e.status(),
        }
    }
}

/// Places a `FrameReceiver` that has seen no frame in `storage` (LNK-050, LNK-051; CABI-031,
/// CABI-032).
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` for `NULL` or misaligned `storage` or `cfg`, a flag that is not 0
/// or 1, or a constant that is none; `ALDIS_ERR_INVALID_CONFIG` (`frame_len`) for a frame length
/// that leaves no data field octet, (`vc_id`) for a VC listed twice, (`fsh_len`) outside 2 to 64,
/// (`data_field_content`) for `BITSTREAM` on TM, (`raw_frames`) without an Insert Zone;
/// `ALDIS_ERR_OUT_OF_RANGE` (`vc_id`, `map_id`); `ALDIS_ERR_CAPACITY_EXCEEDED` (`vc_id`) for
/// more than `ALDIS_LINK_MAX_VIRTUAL_CHANNELS` VCs.
///
/// # Safety
/// `storage` is valid for writes of one `aldis_frame_receiver_t` and outlives its use; `cfg` is
/// valid for a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_frame_receiver_init(
    storage: *mut aldis_frame_receiver_t,
    cfg: *const aldis_frame_receiver_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let cfg = abi::read(cfg)?;
            let state = rx_state(&cfg)?;
            handle::init(storage, state)
        })
    }
}

/// The engine behind `h`.
///
/// # Safety
/// As for `handle::get`.
#[expect(unsafe_code, reason = "checks a caller handle (CABI-032, GEN-071)")]
unsafe fn rx_engine(h: *mut aldis_frame_receiver_t) -> Result<handle::Handle<FrameReceiverState>> {
    // SAFETY: forwarded from the caller.
    unsafe { handle::get(h) }
}

/// Runs one input of the `FrameReceiver` (API-057, CABI-040).
///
/// The octets of the delivered parts go to `out`, which must hold at least the frame length of
/// the configuration (LNK-075).
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation or a constant
/// that is none; for `FRAME`: the error of the codec for a frame it refuses (`LengthMismatch`
/// (`frame_len`), `Malformed`, `ReservedValue`, a failed FECF), then `LengthMismatch`
/// (`frame_len`), `BufferTooSmall`, `Malformed`, `InvalidField`, `ReservedValue` of the engine
/// (API-058); for `RAW_FRAME`: `ALDIS_ERR_INVALID_CONFIG` (`raw_frames`) while the option is
/// off; for `RECONFIGURE`: as `aldis_frame_receiver_init`, and `ALDIS_ERR_INVALID_CONFIG`
/// (`link_protocol`) for another protocol. The engine is unchanged (an engine error counts as
/// `refused`) and `actions` is not written.
///
/// # Safety
/// `h` is a handle from `aldis_frame_receiver_init` whose storage is still valid, or detectably
/// invalid storage; `input` is valid for a read, and its `data` for `data_len` octets; `out` is
/// valid for `out_cap` octets of writes (it may be `NULL` when `out_cap` is 0); `actions` is
/// valid for one write; `err` is `NULL` or valid for one write. No other thread uses the
/// storage (CABI-050).
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_frame_receiver_step(
    h: *mut aldis_frame_receiver_t,
    input: *const aldis_frame_receiver_input_t,
    out: *mut u8,
    out_cap: usize,
    actions: *mut aldis_frame_receiver_actions_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let mut h = rx_engine(h)?;
            let value = abi::read(input)?;
            let data = abi::bytes_disjoint(value.data, value.data_len, &[h.range()])?;
            let area = abi::bytes_mut(out, out_cap, &[(value.data, value.data_len), h.range()])?;
            let actions = abi::out_disjoint(
                actions,
                &[
                    h.range(),
                    (value.data, value.data_len),
                    (out.cast_const(), out_cap),
                ],
            )?;
            let result = h.update(|state| state.step(&value, data, area))?;
            actions.write(result);
            Ok(())
        })
    }
}

/// Reads the counters of the `FrameReceiver` (API-056, LNK-002). It changes nothing.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation.
///
/// # Safety
/// As for [`aldis_frame_receiver_step`]; `status` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_frame_receiver_status(
    h: *mut aldis_frame_receiver_t,
    status: *mut aldis_frame_receiver_status_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = rx_engine(h)?;
            let status = abi::out_disjoint(status, &[h.range()])?;
            let s = h.read(FrameReceiverState::status);
            status.write(aldis_frame_receiver_status_t {
                frames_in: s.frames_in.get(),
                raw_frames_in: s.raw_frames_in.get(),
                refused: s.refused.get(),
                frame_loss: s.frame_loss.get(),
                unknown_channel: s.unknown_channel.get(),
                not_served: s.not_served.get(),
                crc_mismatch: s.crc_mismatch.get(),
                header_uncorrectable: s.header_uncorrectable.get(),
                security: s.security.get(),
                oid_frames: s.oid_frames.get(),
                gaps: s.gaps.get(),
                count_discontinuities: s.count_discontinuities.get(),
            });
            Ok(())
        })
    }
}

/// Releases the storage: later calls on it fail with `ALDIS_ERR_INVALID_HANDLE` (CABI-032). An
/// invalid handle is ignored.
///
/// # Safety
/// As for [`aldis_frame_receiver_step`].
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_frame_receiver_deinit(h: *mut aldis_frame_receiver_t) {
    // SAFETY: forwarded from the caller; `handle::deinit` checks the tag.
    unsafe { handle::deinit(h) }
}
