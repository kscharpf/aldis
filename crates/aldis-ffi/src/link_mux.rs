//! C ABI of the `ChannelMux` (LNK-030…LNK-036, LNK-076, LNK-082, LNK-003, CABI-030…032, CABI-040;
//! M4 plan D4, WP-7c).
//!
//! The engine is one opaque handle (`aldis_channel_mux_t`) in the pattern of the generation and
//! receiving engines of [`crate::link_gen`] and [`crate::link_rx`]: the caller declares the typed
//! storage, `_init` places the engine of the protocol named in the configuration in it, and every
//! input is one `_step` call that returns the actions of the input in a fixed-capacity actions
//! struct. The octets of a frame go to the caller's `out` area and the action holds the
//! `aldis_range_t` of the frame and of its Security Header, data field, and Security Trailer
//! (API-061, LNK-035). The status is a read-only call (API-056); the engine has no timer, so
//! `_step` takes no time (API-053). A failed call leaves the engine as it was and writes no
//! actions (API-058).
//!
//! **Protocol.** The first member of the configuration names the protocol (`protocol`, an
//! `ALDIS_LINK_PROTOCOL_*` constant): TM, AOS, or USLP with fixed-length frames. TC is
//! `ALDIS_ERR_INVALID_CONFIG` on `link_protocol`. Every member is read whatever the protocol
//! (`check_members`): a flag or constant that is invalid in a member the protocol does not use
//! is `ALDIS_ERR_INVALID_FIELD` all the same, and a range-limited member of another protocol is
//! range-checked.
//!
//! **Capacities.** The engine serves `ALDIS_LINK_MAX_VIRTUAL_CHANNELS` VCs of frames of at most
//! `ALDIS_LINK_MAX_FRAME_OCTETS` octets (CABI-030); it is built in place (`aldis::link::inplace`,
//! #240), so its field slots are not on the stack.

use aldis::aos::AosEdition;
use aldis::core::ids::{MapId, SpacecraftId, VcId};
use aldis::core::layout::OCF_LEN;
use aldis::core::security::SecurityLayout;
use aldis::core::{Error, ErrorKind, Result, field};
use aldis::link::common::Qos;
use aldis::link::mux::{
    AosChannel, AosVc, CHANNEL_MUX_MAX_ACTIONS, ChannelMux, ChannelMuxBuffers, ChannelMuxConfig,
    ChannelMuxHead, Cop, FrameClass, Fsh, MAX_FSH_DATA, MuxActions, MuxInput, MuxOutput,
    MuxProtocol, MuxStatus, Offer, Scope, TmChannel, TmRole, TmVc, UslpChannel, UslpVc, VcConfig,
};
use aldis::link::policy::{MuxPolicy, Weight};
use aldis::link::protocol::{Aos, Route, Tm, Uslp};
use aldis::uslp::{SourceOrDestination, VcfCountLen};

use crate::abi::{self, flag};
use crate::capacities::{ALDIS_LINK_MAX_FRAME_OCTETS, ALDIS_LINK_MAX_VIRTUAL_CHANNELS};
use crate::handle::{self, Storage};
use crate::link::{
    aldis_link_channel_t, aldis_link_occupancy_t, aldis_link_protocol, aldis_link_qos, aos_params,
    protocol, unsupported_protocol,
};
use crate::link_gen::{engine_in_place, range, variant_payload};
use crate::link_rx::{aldis_link_content, aldis_link_scope};
use crate::mirror::{mirror, parse};
use crate::range::aldis_range_t;

/// The frame capacity of the engine: `ALDIS_LINK_MAX_FRAME_OCTETS` (CABI-030).
const F: usize = ALDIS_LINK_MAX_FRAME_OCTETS;
/// The VC capacity of the engine: `ALDIS_LINK_MAX_VIRTUAL_CHANNELS` (CABI-030).
const V: usize = ALDIS_LINK_MAX_VIRTUAL_CHANNELS;

/// The most actions one `aldis_channel_mux_step` returns (LNK-074): a frame and the notice that
/// its Insert Zone repeats the frame before.
pub const ALDIS_CHANNEL_MUX_MAX_ACTIONS: usize = 2;

const _: () = assert!(ALDIS_CHANNEL_MUX_MAX_ACTIONS == CHANNEL_MUX_MAX_ACTIONS);

/// 64-bit words of one `aldis_channel_mux_t` (CABI-031): the tag and the largest of the engines
/// of TM, AOS, and USLP at the build capacities; the build fails when it outgrows it.
pub const ALDIS_CHANNEL_MUX_WORDS: usize = 2653;

const _: () =
    assert!(size_of::<crate::handle::Slot<ChannelMuxState>>() <= ALDIS_CHANNEL_MUX_WORDS * 8);

type Mx<P> = ChannelMux<P, V, F>;

/// The `ChannelMux` of the protocol chosen at `aldis_channel_mux_init` (LNK-001).
///
/// `repr(u8)`: `aldis_channel_mux_init` writes a variant in place (`variant_payload`).
#[repr(u8)]
#[expect(
    dead_code,
    clippy::large_enum_variant,
    reason = "the variants are written in place by the init function, not constructed; the storage holds the largest"
)]
pub(crate) enum ChannelMuxState {
    /// TM.
    Tm(Mx<Tm>) = 0,
    /// AOS.
    Aos(Mx<Aos>) = 1,
    /// USLP.
    Uslp(Mx<Uslp>) = 2,
}

/// The multiplexing policy of a `ChannelMux` (LNK-031).
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
pub enum aldis_link_mux_policy {
    /// The offered VC with the highest priority; priorities are unique.
    ALDIS_LINK_MUX_POLICY_FIXED_PRIORITY = 0,
    /// The next offered VC after the one served last, in ascending VCID order.
    ALDIS_LINK_MUX_POLICY_ROUND_ROBIN = 1,
    /// Each VC gets up to its weight of consecutive selections per cycle.
    ALDIS_LINK_MUX_POLICY_WEIGHTED_ROUND_ROBIN = 2,
}
mirror!(pub aldis_link_mux_policy: u32 {
    ALDIS_LINK_MUX_POLICY_FIXED_PRIORITY,
    ALDIS_LINK_MUX_POLICY_ROUND_ROBIN,
    ALDIS_LINK_MUX_POLICY_WEIGHTED_ROUND_ROBIN
});

/// Whether a frame carries the data of a VC or only idle data (LNK-030).
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
pub enum aldis_link_frame_class {
    /// A frame of the field a VC offered.
    ALDIS_LINK_FRAME_CLASS_DATA = 0,
    /// An Only Idle Data frame (LNK-030).
    ALDIS_LINK_FRAME_CLASS_OID = 1,
}
mirror!(pub aldis_link_frame_class: u32 {
    ALDIS_LINK_FRAME_CLASS_DATA,
    ALDIS_LINK_FRAME_CLASS_OID
});

/// The kind of an `aldis_channel_mux_input_t` (LNK-030, LNK-033, LNK-034, LNK-076, LNK-003;
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
pub enum aldis_channel_mux_input {
    /// A data field for the VC `vc_id` (`data`; LNK-076).
    ALDIS_CHANNEL_MUX_INPUT_OFFER = 0,
    /// Replace the OCF slot of `scope` (`ocf`; LNK-033).
    ALDIS_CHANNEL_MUX_INPUT_SET_OCF = 1,
    /// Replace the Secondary Header slot of `scope` (`data`; TM, LNK-033).
    ALDIS_CHANNEL_MUX_INPUT_SET_FSH = 2,
    /// Replace the Insert Zone slot (`data`; AOS, USLP; LNK-034).
    ALDIS_CHANNEL_MUX_INPUT_SET_INSERT = 3,
    /// Build the next frame in `out` (LNK-030).
    ALDIS_CHANNEL_MUX_INPUT_OPPORTUNITY = 4,
    /// Replace the configuration (`config`) between data units (LNK-003). The protocol cannot
    /// change.
    ALDIS_CHANNEL_MUX_INPUT_RECONFIGURE = 5,
}
mirror!(pub aldis_channel_mux_input: u32 {
    ALDIS_CHANNEL_MUX_INPUT_OFFER,
    ALDIS_CHANNEL_MUX_INPUT_SET_OCF,
    ALDIS_CHANNEL_MUX_INPUT_SET_FSH,
    ALDIS_CHANNEL_MUX_INPUT_SET_INSERT,
    ALDIS_CHANNEL_MUX_INPUT_OPPORTUNITY,
    ALDIS_CHANNEL_MUX_INPUT_RECONFIGURE
});

/// The kind of an `aldis_channel_mux_action_t` (API-057).
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
pub enum aldis_channel_mux_action {
    /// The next frame, in `out` (LNK-030).
    ALDIS_CHANNEL_MUX_ACTION_TRANSMIT = 0,
    /// The Insert Zone holds the value of the frame before (LNK-034).
    ALDIS_CHANNEL_MUX_ACTION_INSERT_SDU_REPEATED = 1,
    /// A reconfiguration discarded `octets` (LNK-003).
    ALDIS_CHANNEL_MUX_ACTION_DISCARDED = 2,
}
mirror!(pub aldis_channel_mux_action: u32 {
    ALDIS_CHANNEL_MUX_ACTION_TRANSMIT,
    ALDIS_CHANNEL_MUX_ACTION_INSERT_SDU_REPEATED,
    ALDIS_CHANNEL_MUX_ACTION_DISCARDED
});

/// A `ChannelMux` in caller storage: the caller declares an object of this type, and the pointer
/// to it is the handle (LNK-030, CABI-031, CABI-032). The words are the library's.
#[repr(C)]
pub struct aldis_channel_mux_t {
    /// The slot of [`handle`]: the tag, then the state.
    pub words: [u64; ALDIS_CHANNEL_MUX_WORDS],
}

impl Storage for aldis_channel_mux_t {
    type State = ChannelMuxState;
    const MAGIC: u32 = 0x414C_434D; // "ALCM"
}

/// The most octets of a Secondary Header's data (132.0 §4.1.3.2.3, Table 5-2).
pub const ALDIS_CHANNEL_MUX_MAX_FSH_DATA: usize = 63;

const _: () = assert!(ALDIS_CHANNEL_MUX_MAX_FSH_DATA == MAX_FSH_DATA);

/// One virtual channel of a `ChannelMux` (132.0 §5.4 Table 5-3; 732.0 §5.4 Table 5-3; 732.1
/// §5.4 Table 5-3; LNK-031, LNK-033, LNK-035, LNK-076, LNK-082).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_channel_mux_vc_t {
    /// The VCID: 0 to 7 for TM, 0 to 63 for AOS and USLP. A VC is listed once.
    pub vc_id: u8,
    /// The priority under fixed priority (LNK-031); the priorities of the VCs are unique then.
    pub priority: u8,
    /// The weight under weighted round robin, at least 1 (LNK-031).
    pub weight: u16,
    /// The octets of the VC's Security Header, at most 64 (API-071; LNK-035).
    pub sec_header_len: usize,
    /// The octets of the VC's Security Trailer, at most 65 535 (API-071; LNK-035).
    pub sec_trailer_len: usize,
    /// TM: `ALDIS_LINK_CONTENT_PACKETS` or `ALDIS_LINK_CONTENT_VCA` (LNK-082), which sets the
    /// data field status of the frames. Read as a constant for every protocol, used by TM.
    pub role: u32,
    /// TM, AOS: 1 when every frame of the VC has an OCF, with the value `ocf_init` until
    /// `SET_OCF` replaces it (LNK-033), else 0.
    pub ocf: u8,
    /// TM, AOS: the initial OCF as a big-endian word.
    pub ocf_init: u32,
    /// TM: the data octets of the Secondary Header, 1 to 63, initially `fsh_init`; 0 for none
    /// (LNK-033).
    pub fsh_len: u8,
    /// TM: the initial Secondary Header data; the first `fsh_len` octets.
    pub fsh_init: [u8; ALDIS_CHANNEL_MUX_MAX_FSH_DATA],
    /// USLP: the MAP ID of the frames, 0 to 15 (732.1 §4.1.2.5).
    pub uslp_map: u8,
    /// USLP: `ALDIS_LINK_QOS_*` of an offered field that names no route (732.1 §4.1.2.8).
    pub uslp_qos: u32,
    /// USLP: the VCF Count Length of sequence-controlled frames, 0 to 7 octets (732.1 §4.1.2.11).
    pub uslp_vcf_len_sc: u8,
    /// USLP: the VCF Count Length of expedited frames, 0 to 7 octets.
    pub uslp_vcf_len_exp: u8,
    /// USLP: 1 when COP-1 is in effect on the VC (732.1 Table 5-3), so an offer may carry the
    /// COP block (LNK-076); needs a sequence-controlled VCF Count Length above 0.
    pub uslp_cop: u8,
}

impl Default for aldis_channel_mux_vc_t {
    fn default() -> Self {
        aldis_channel_mux_vc_t {
            vc_id: 0,
            priority: 0,
            weight: 1,
            sec_header_len: 0,
            sec_trailer_len: 0,
            role: 0,
            ocf: 0,
            ocf_init: 0,
            fsh_len: 0,
            fsh_init: [0; ALDIS_CHANNEL_MUX_MAX_FSH_DATA],
            uslp_map: 0,
            uslp_qos: 0,
            uslp_vcf_len_sc: 0,
            uslp_vcf_len_exp: 0,
            uslp_cop: 0,
        }
    }
}

/// The managed parameters of a `ChannelMux` (132.0 §5.2; 732.0 §5.2; 732.1 §5.2, §5.3;
/// LNK-030…LNK-035, LNK-078).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_channel_mux_config_t {
    /// `ALDIS_LINK_PROTOCOL_*`: TM, AOS, or USLP.
    pub protocol: u32,
    /// The Spacecraft Identifier: 10 bits for TM and AOS (8 bits in the AOS Issue 4 edition), 16
    /// bits for USLP.
    pub scid: u16,
    /// The octets of every frame, at most `ALDIS_LINK_MAX_FRAME_OCTETS`.
    pub frame_len: usize,
    /// 1 when frames carry a FECF, else 0.
    pub fecf: u8,
    /// `ALDIS_LINK_MUX_POLICY_*` (LNK-031).
    pub policy: u32,
    /// AOS, USLP: the octets of the Insert Zone, 0 for none (LNK-034).
    pub insert_zone_len: usize,
    /// AOS, USLP: the octet the Insert Zone holds until `SET_INSERT` (LNK-034).
    pub insert_idle_pattern: u8,
    /// TM: the VC dedicated to OID frames, one of `vcs` (LNK-030).
    pub tm_oid_vc: u8,
    /// TM, USLP: 1 when every frame of the master channel has an OCF, with the value
    /// `mc_ocf_init` until `SET_OCF` replaces it (LNK-033, OI-76), else 0.
    pub mc_ocf: u8,
    /// TM, USLP: the initial OCF of the master channel as a big-endian word.
    pub mc_ocf_init: u32,
    /// TM: the data octets of the master channel's Secondary Header, 1 to 63; 0 for none.
    pub mc_fsh_len: u8,
    /// TM: the initial Secondary Header data of the master channel; the first `mc_fsh_len`
    /// octets.
    pub mc_fsh_init: [u8; ALDIS_CHANNEL_MUX_MAX_FSH_DATA],
    /// AOS: `ALDIS_AOS_EDITION_*` (AOS-001); 0 is Issue 5.
    pub aos_edition: u8,
    /// AOS: 1 when frames have an FHEC, which the caller computes (LNK-035), else 0.
    pub aos_fhec: u8,
    /// AOS: 1 when the VC Frame Count Cycle is in use (732.0 §4.1.2.5.5), else 0.
    pub aos_cycle_in_use: u8,
    /// USLP: 1 when the SCID is the destination of the frames, 0 for the source (732.1
    /// §4.1.2.3.3).
    pub uslp_destination: u8,
    /// USLP: the VCF Count Length of VCID 63, the OID frames, 0 to 7 octets (LNK-032).
    pub uslp_oid_vcf_len: u8,
    /// The number of entries of `vcs`, at most `ALDIS_LINK_MAX_VIRTUAL_CHANNELS`.
    pub vc_count: usize,
    /// The virtual channels; those from `vc_count` on are not read.
    pub vcs: [aldis_channel_mux_vc_t; ALDIS_LINK_MAX_VIRTUAL_CHANNELS],
}

impl Default for aldis_channel_mux_config_t {
    fn default() -> Self {
        aldis_channel_mux_config_t {
            protocol: 0,
            scid: 0,
            frame_len: 0,
            fecf: 0,
            policy: 0,
            insert_zone_len: 0,
            insert_idle_pattern: 0,
            tm_oid_vc: 0,
            mc_ocf: 0,
            mc_ocf_init: 0,
            mc_fsh_len: 0,
            mc_fsh_init: [0; ALDIS_CHANNEL_MUX_MAX_FSH_DATA],
            aos_edition: 0,
            aos_fhec: 0,
            aos_cycle_in_use: 0,
            uslp_destination: 0,
            uslp_oid_vcf_len: 0,
            vc_count: 0,
            vcs: [aldis_channel_mux_vc_t::default(); ALDIS_LINK_MAX_VIRTUAL_CHANNELS],
        }
    }
}

/// One input of a `ChannelMux` (API-057, CABI-040). A field that the kind does not use is
/// ignored.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_channel_mux_input_t {
    /// `ALDIS_CHANNEL_MUX_INPUT_*`.
    pub kind: u32,
    /// For `OFFER`, `SET_FSH`, and `SET_INSERT`: the octets, read once and copied (API-060).
    /// `data` is checked on every kind and must not overlap the storage or `out`: `NULL` only
    /// with `data_len` 0. An offered field has the data field length of the VC (LNK-076).
    pub data: *const u8,
    /// The octets at `data`.
    pub data_len: usize,
    /// For `OFFER`: the VCID of the field. For `SET_OCF` and `SET_FSH` with the scope
    /// `ALDIS_LINK_SCOPE_VIRTUAL_CHANNEL`: the VCID of the slot.
    pub vc_id: u8,
    /// For `SET_OCF` and `SET_FSH`: `ALDIS_LINK_SCOPE_*`.
    pub scope: u32,
    /// For `SET_OCF`: the new OCF as a big-endian word.
    pub ocf: u32,
    /// For `OFFER`: 1 when `pointer` is the First Header Pointer of a TM frame header, else 0
    /// (required on a TM VC of Packets, refused on a VC of `VCA_SDU`s, ignored by AOS and USLP).
    pub pointer_present: u8,
    /// For `OFFER`: the raw First Header Pointer (TM).
    pub pointer: u16,
    /// For `OFFER`: 1 when the field names its MAP and `QoS` (USLP), else 0.
    pub route_present: u8,
    /// For `OFFER` with a route: the MAP ID, which must be the VC's.
    pub map_id: u8,
    /// For `OFFER` with a route: `ALDIS_LINK_QOS_*`.
    pub qos: u32,
    /// For `OFFER`: 1 when the field carries a COP-1 block (a USLP VC with COP-1 in effect;
    /// LNK-076), else 0.
    pub cop_present: u8,
    /// For `OFFER` with a COP block: the VCF Count of a Type-AD frame (the Frame Sequence
    /// Number); unused when `cop_bypass` is 1.
    pub cop_vcf_count: u64,
    /// For `OFFER` with a COP block: the Bypass/Sequence Control Flag, 0 or 1.
    pub cop_bypass: u8,
    /// For `OFFER` with a COP block: the Protocol Control Command Flag, 0 or 1 (1 needs
    /// `cop_bypass` 1: 732.1 Table 4-1).
    pub cop_protocol_control: u8,
    /// For `RECONFIGURE`: the new configuration.
    pub config: aldis_channel_mux_config_t,
}

/// One action of a `ChannelMux` (API-057, API-061). A field that the kind does not use is 0.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_channel_mux_action_t {
    /// `ALDIS_CHANNEL_MUX_ACTION_*`.
    pub kind: u32,
    /// For `TRANSMIT`: `ALDIS_LINK_FRAME_CLASS_*`.
    pub class: u32,
    /// For `TRANSMIT`: the VCID of the frame.
    pub vc_id: u8,
    /// For `TRANSMIT`: the whole frame in `out`; the Security Header and Trailer, the FHEC, and
    /// the FECF are zero octets for the caller to fill (LNK-035).
    pub frame: aldis_range_t,
    /// For `TRANSMIT`: the Security Header (empty if the VC has none).
    pub security_header: aldis_range_t,
    /// For `TRANSMIT`: the data field, which the SDLS hook protects.
    pub data: aldis_range_t,
    /// For `TRANSMIT`: the Security Trailer.
    pub security_trailer: aldis_range_t,
    /// For `DISCARDED`: the octets discarded.
    pub octets: usize,
}

/// The actions of one input of a `ChannelMux`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_channel_mux_actions_t {
    /// The number of actions in `items`, at most `ALDIS_CHANNEL_MUX_MAX_ACTIONS`.
    pub count: usize,
    /// The actions; those from `count` on are zero.
    pub items: [aldis_channel_mux_action_t; ALDIS_CHANNEL_MUX_MAX_ACTIONS],
}

/// A read-only snapshot of a `ChannelMux` (API-056, LNK-002).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_channel_mux_status_t {
    /// Frames built, OID frames included.
    pub frames_out: u64,
    /// OID frames built.
    pub oid_frames: u64,
    /// Fields accepted.
    pub offers: u64,
    /// Fields refused as invalid.
    pub refused: u64,
    /// Fields refused with `ALDIS_ERR_WOULD_BLOCK` (LNK-076).
    pub would_block: u64,
    /// Frames whose Insert Zone repeated the slot (LNK-034).
    pub insert_repeated: u64,
    /// Octets discarded by a reconfiguration (LNK-003).
    pub discarded_octets: u64,
    /// The offered fields.
    pub slots: aldis_link_occupancy_t,
}

/// The OCF of a big-endian word.
const fn ocf_octets(word: u32) -> [u8; OCF_LEN] {
    word.to_be_bytes()
}

/// The initial OCF of a slot: `Some` when `present` is 1.
fn initial_ocf(present: u8, word: u32) -> Result<Option<[u8; OCF_LEN]>> {
    Ok(flag(present)?.then(|| ocf_octets(word)))
}

/// The initial Secondary Header data of `len` octets of `init`; none for 0 (LNK-033).
///
/// # Errors
/// `InvalidConfig` (`fsh_len`) above 63 octets.
fn initial_fsh(len: u8, init: &[u8; MAX_FSH_DATA]) -> Result<Option<Fsh>> {
    if len == 0 {
        return Ok(None);
    }
    let data = init
        .get(..usize::from(len))
        .ok_or(Error::new(ErrorKind::InvalidConfig).with_field(field::FSH_LEN))?;
    Fsh::new(data).map(Some)
}

fn policy(value: u32) -> Result<MuxPolicy> {
    Ok(match parse::<aldis_link_mux_policy>(value)? {
        aldis_link_mux_policy::ALDIS_LINK_MUX_POLICY_FIXED_PRIORITY => MuxPolicy::FixedPriority,
        aldis_link_mux_policy::ALDIS_LINK_MUX_POLICY_ROUND_ROBIN => MuxPolicy::RoundRobin,
        aldis_link_mux_policy::ALDIS_LINK_MUX_POLICY_WEIGHTED_ROUND_ROBIN => {
            MuxPolicy::WeightedRoundRobin
        }
    })
}

fn qos(value: u32) -> Result<Qos> {
    Ok(match parse::<aldis_link_qos>(value)? {
        aldis_link_qos::ALDIS_LINK_QOS_SEQUENCE_CONTROLLED => Qos::SequenceControlled,
        aldis_link_qos::ALDIS_LINK_QOS_EXPEDITED => Qos::Expedited,
    })
}

/// The AOS edition of the configuration `c` (AOS-001).
///
/// # Errors
/// `InvalidField` for an `aos_edition` that is no `ALDIS_AOS_EDITION_*` constant.
fn aos_edition(c: &aldis_channel_mux_config_t) -> Result<AosEdition> {
    let channel = aldis_link_channel_t {
        aos_edition: c.aos_edition,
        ..aldis_link_channel_t::default()
    };
    Ok(aos_params(&channel)?.edition)
}

/// A protocol of the `ChannelMux` in the C ABI: its configuration from the flat struct.
trait MxProtocol: MuxProtocol {
    /// The channel parameters of `c`.
    fn channel(c: &aldis_channel_mux_config_t) -> Result<Self::Channel>;

    /// The parameters of the VC `vc`.
    fn vc_params(vc: &aldis_channel_mux_vc_t) -> Result<Self::VcParams>;
}

impl MxProtocol for Tm {
    fn channel(c: &aldis_channel_mux_config_t) -> Result<TmChannel> {
        let mut channel = TmChannel::new(
            SpacecraftId::<10>::try_from(c.scid)?,
            VcId::<3>::try_from(c.tm_oid_vc)?,
        );
        channel.mc_ocf = initial_ocf(c.mc_ocf, c.mc_ocf_init)?;
        channel.mc_fsh = initial_fsh(c.mc_fsh_len, &c.mc_fsh_init)?;
        Ok(channel)
    }

    fn vc_params(vc: &aldis_channel_mux_vc_t) -> Result<TmVc> {
        let role = match parse::<aldis_link_content>(vc.role)? {
            aldis_link_content::ALDIS_LINK_CONTENT_PACKETS => TmRole::Packets,
            aldis_link_content::ALDIS_LINK_CONTENT_VCA => TmRole::Vca,
            aldis_link_content::ALDIS_LINK_CONTENT_BITSTREAM
            | aldis_link_content::ALDIS_LINK_CONTENT_UNSERVED => {
                return Err(
                    Error::new(ErrorKind::InvalidConfig).with_field(field::DATA_FIELD_CONTENT)
                );
            }
        };
        let mut params = TmVc::new(role);
        params.ocf = initial_ocf(vc.ocf, vc.ocf_init)?;
        params.fsh = initial_fsh(vc.fsh_len, &vc.fsh_init)?;
        Ok(params)
    }
}

impl MxProtocol for Aos {
    fn channel(c: &aldis_channel_mux_config_t) -> Result<AosChannel> {
        AosChannel::new(
            SpacecraftId::<10>::try_from(c.scid)?,
            aos_edition(c)?,
            flag(c.aos_fhec)?,
            flag(c.aos_cycle_in_use)?,
        )
    }

    fn vc_params(vc: &aldis_channel_mux_vc_t) -> Result<AosVc> {
        Ok(AosVc {
            ocf: initial_ocf(vc.ocf, vc.ocf_init)?,
        })
    }
}

impl MxProtocol for Uslp {
    fn channel(c: &aldis_channel_mux_config_t) -> Result<UslpChannel> {
        Ok(UslpChannel {
            scid: SpacecraftId::<16>::try_from(c.scid)?,
            source_or_destination: SourceOrDestination::from_bit(flag(c.uslp_destination)?),
            oid_vcf_len: VcfCountLen::new(c.uslp_oid_vcf_len)?,
            mc_ocf: initial_ocf(c.mc_ocf, c.mc_ocf_init)?,
        })
    }

    fn vc_params(vc: &aldis_channel_mux_vc_t) -> Result<UslpVc> {
        UslpVc::new(
            MapId::<4>::try_from(vc.uslp_map)?,
            qos(vc.uslp_qos)?,
            VcfCountLen::new(vc.uslp_vcf_len_sc)?,
            VcfCountLen::new(vc.uslp_vcf_len_exp)?,
            flag(vc.uslp_cop)?,
        )
    }
}

/// The VC `vc` in the protocol `P` (LNK-031, LNK-035).
fn mx_vc<P: MxProtocol>(vc: &aldis_channel_mux_vc_t) -> Result<VcConfig<P>> {
    // The members were read by `check_members`; these are the protocol's conversions.
    let params = P::vc_params(vc)?;
    Ok(VcConfig::<P>::new(P::Vc::try_from(vc.vc_id)?, params)
        .with_priority(vc.priority)
        .with_weight(Weight::new(vc.weight)?)
        .with_security(SecurityLayout::new(vc.sec_header_len, vc.sec_trailer_len)?))
}

/// Reads every member of `c` and of its `vc_count` entries whatever the protocol (CABI-040): a
/// flag that is not 0 or 1, or a constant that is none, is `InvalidField` in any member, and a
/// member of another protocol that has a range (a VCID, a MAP ID, a count length, a Secondary
/// Header length) is range-checked too. The protocol's own conversions follow in `mx_config`.
fn check_members(c: &aldis_channel_mux_config_t) -> Result<()> {
    flag(c.fecf)?;
    policy(c.policy)?;
    flag(c.mc_ocf)?;
    flag(c.aos_fhec)?;
    flag(c.aos_cycle_in_use)?;
    flag(c.uslp_destination)?;
    aos_edition(c)?;
    VcId::<3>::try_from(c.tm_oid_vc)?;
    VcfCountLen::new(c.uslp_oid_vcf_len)?;
    initial_fsh(c.mc_fsh_len, &c.mc_fsh_init)?;
    for vc in c.vcs.iter().take(c.vc_count) {
        flag(vc.ocf)?;
        flag(vc.uslp_cop)?;
        parse::<aldis_link_content>(vc.role)?;
        qos(vc.uslp_qos)?;
        MapId::<4>::try_from(vc.uslp_map)?;
        VcfCountLen::new(vc.uslp_vcf_len_sc)?;
        VcfCountLen::new(vc.uslp_vcf_len_exp)?;
        initial_fsh(vc.fsh_len, &vc.fsh_init)?;
    }
    Ok(())
}

/// The mux configuration of `c` for the protocol `P`.
fn mx_config<P: MxProtocol>(c: &aldis_channel_mux_config_t) -> Result<ChannelMuxConfig<P, V>> {
    check_members(c)?;
    // The first entry fills the array; only the first `vc_count` of it are given on. Without an
    // entry there is nothing to list.
    let first = c
        .vcs
        .first()
        .filter(|_| c.vc_count > 0)
        .map(mx_vc::<P>)
        .transpose()?;
    let mut entries = first.map(|filler| [filler; V]);
    if let Some(array) = entries.as_mut() {
        for (dst, src) in array.iter_mut().zip(&c.vcs).take(c.vc_count) {
            *dst = mx_vc::<P>(src)?;
        }
    }
    let channel = P::channel(c)?;
    let fecf = flag(c.fecf)?;
    let policy = policy(c.policy)?;
    let listed = match &entries {
        None => &[],
        Some(array) => array
            .get(..c.vc_count)
            .ok_or(Error::new(ErrorKind::CapacityExceeded).with_field(field::CAPACITY))?,
    };
    let mut config = ChannelMuxConfig::<P, V>::new(channel, c.frame_len, fecf, policy, listed)?;
    if c.insert_zone_len > 0 {
        config = config.with_insert_zone(c.insert_zone_len, c.insert_idle_pattern)?;
    }
    Ok(config)
}

/// The head of the engine of `c`; the protocol of the engine is fixed here (LNK-001).
#[expect(
    clippy::large_enum_variant,
    reason = "a temporary of one call; the largest head is 2.8 KB, within the C ABI stack budget"
)]
enum MxHead {
    Tm(ChannelMuxHead<Tm, V>),
    Aos(ChannelMuxHead<Aos, V>),
    Uslp(ChannelMuxHead<Uslp, V>),
}

fn mx_head(c: &aldis_channel_mux_config_t) -> Result<MxHead> {
    match parse::<aldis_link_protocol>(c.protocol)? {
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM => {
            Ok(MxHead::Tm(Mx::new_head(mx_config::<Tm>(c)?)?))
        }
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS => {
            Ok(MxHead::Aos(Mx::new_head(mx_config::<Aos>(c)?)?))
        }
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP => {
            Ok(MxHead::Uslp(Mx::new_head(mx_config::<Uslp>(c)?)?))
        }
        aldis_link_protocol::ALDIS_LINK_PROTOCOL_TC => Err(unsupported_protocol()),
    }
}

/// Writes the idle pattern `idle` into the Insert Zone slot of the buffers at `buffers`, which
/// the in-place build has just zeroed (LNK-034; `ChannelMuxBuffers::with_idle` is the by-value
/// form).
///
/// # Safety
/// `buffers` is valid for writes of one `ChannelMuxBuffers<V, F>`.
#[expect(unsafe_code, reason = "initializes the engine in place (GEN-071)")]
const unsafe fn fill_insert(buffers: *mut u8, idle: u8) {
    const {
        assert!(
            ChannelMuxBuffers::<V, F>::INSERT_OFFSET.saturating_add(F)
                <= size_of::<ChannelMuxBuffers<V, F>>()
        );
    };
    // SAFETY: by the contract and the assertion above, the `F` octets at the Insert Zone slot
    // lie within the buffers; they are plain octets.
    unsafe {
        buffers
            .byte_add(ChannelMuxBuffers::<V, F>::INSERT_OFFSET)
            .write_bytes(idle, F);
    }
}

/// Builds the engine of `head` at `state`, in place (#240), with the Insert Zone slot at the idle
/// pattern `idle`.
///
/// # Safety
/// `state` is valid for writes of one `ChannelMuxState`.
#[expect(unsafe_code, reason = "initializes the engine in place (GEN-071)")]
unsafe fn mx_place(state: *mut ChannelMuxState, head: MxHead, idle: u8) {
    type B = ChannelMuxBuffers<V, F>;
    match head {
        MxHead::Tm(h) => {
            // SAFETY: as the contract says; the tag 0 is the discriminant of the variant.
            let p = unsafe { variant_payload::<_, Mx<Tm>>(state, 0) };
            engine_in_place!(Mx<Tm>, ChannelMuxHead<Tm, V>, B, p, h);
            // SAFETY: `engine_in_place!` has just built the engine at `p`.
            unsafe { fill_insert(p.byte_add(Mx::<Tm>::BUFFERS_OFFSET).cast::<u8>(), idle) };
        }
        MxHead::Aos(h) => {
            // SAFETY: as the contract says; the tag 1 is the discriminant of the variant.
            let p = unsafe { variant_payload::<_, Mx<Aos>>(state, 1) };
            engine_in_place!(Mx<Aos>, ChannelMuxHead<Aos, V>, B, p, h);
            // SAFETY: `engine_in_place!` has just built the engine at `p`.
            unsafe { fill_insert(p.byte_add(Mx::<Aos>::BUFFERS_OFFSET).cast::<u8>(), idle) };
        }
        MxHead::Uslp(h) => {
            // SAFETY: as the contract says; the tag 2 is the discriminant of the variant.
            let p = unsafe { variant_payload::<_, Mx<Uslp>>(state, 2) };
            engine_in_place!(Mx<Uslp>, ChannelMuxHead<Uslp, V>, B, p, h);
            // SAFETY: `engine_in_place!` has just built the engine at `p`.
            unsafe { fill_insert(p.byte_add(Mx::<Uslp>::BUFFERS_OFFSET).cast::<u8>(), idle) };
        }
    }
}

/// The scope of an input that names one (LNK-033).
fn scope(input: &aldis_channel_mux_input_t) -> Result<Scope> {
    Ok(match parse::<aldis_link_scope>(input.scope)? {
        aldis_link_scope::ALDIS_LINK_SCOPE_MASTER_CHANNEL => Scope::Mc,
        aldis_link_scope::ALDIS_LINK_SCOPE_VIRTUAL_CHANNEL => Scope::Vc(input.vc_id),
    })
}

/// The offered field of `input`, with `data` as its octets (LNK-076).
fn offer<'a>(input: &aldis_channel_mux_input_t, data: &'a [u8]) -> Result<Offer<'a>> {
    let pointer = flag(input.pointer_present)?.then_some(input.pointer);
    let route = if flag(input.route_present)? {
        Some(Route {
            map: MapId::<4>::try_from(input.map_id)?,
            qos: qos(input.qos)?,
        })
    } else {
        None
    };
    let cop = if flag(input.cop_present)? {
        Some(Cop::new(
            input.cop_vcf_count,
            flag(input.cop_bypass)?,
            flag(input.cop_protocol_control)?,
        )?)
    } else {
        None
    };
    Ok(Offer {
        vc: input.vc_id,
        field: data,
        pointer,
        route,
        cop,
    })
}

/// The C form of an action of the mux.
fn mx_action(output: &MuxOutput) -> aldis_channel_mux_action_t {
    match output {
        MuxOutput::Transmit(t) => aldis_channel_mux_action_t {
            kind: ALDIS_CHANNEL_MUX_ACTION_TRANSMIT,
            class: match t.class {
                FrameClass::Data => ALDIS_LINK_FRAME_CLASS_DATA,
                FrameClass::Oid => ALDIS_LINK_FRAME_CLASS_OID,
            },
            vc_id: t.vc,
            frame: range(t.frame),
            security_header: range(t.security_header),
            data: range(t.data),
            security_trailer: range(t.security_trailer),
            octets: 0,
        },
        MuxOutput::InsertSduRepeated => aldis_channel_mux_action_t {
            kind: ALDIS_CHANNEL_MUX_ACTION_INSERT_SDU_REPEATED,
            ..aldis_channel_mux_action_t::default()
        },
        MuxOutput::Discarded { octets } => aldis_channel_mux_action_t {
            kind: ALDIS_CHANNEL_MUX_ACTION_DISCARDED,
            octets: *octets,
            ..aldis_channel_mux_action_t::default()
        },
    }
}

/// One step of `engine`, in the protocol `P` (API-057): the actions in their C form.
fn mx_run<P: MxProtocol>(
    engine: &mut Mx<P>,
    input: &aldis_channel_mux_input_t,
    data: &[u8],
    area: &mut [u8],
) -> Result<aldis_channel_mux_actions_t> {
    let list: MuxActions = match parse::<aldis_channel_mux_input>(input.kind)? {
        aldis_channel_mux_input::ALDIS_CHANNEL_MUX_INPUT_OFFER => {
            engine.step(MuxInput::Offer(offer(input, data)?), area)?
        }
        aldis_channel_mux_input::ALDIS_CHANNEL_MUX_INPUT_SET_OCF => engine.step(
            MuxInput::SetOcf {
                scope: scope(input)?,
                ocf: ocf_octets(input.ocf),
            },
            area,
        )?,
        aldis_channel_mux_input::ALDIS_CHANNEL_MUX_INPUT_SET_FSH => engine.step(
            MuxInput::SetFsh {
                scope: scope(input)?,
                data,
            },
            area,
        )?,
        aldis_channel_mux_input::ALDIS_CHANNEL_MUX_INPUT_SET_INSERT => {
            engine.step(MuxInput::SetInsert(data), area)?
        }
        aldis_channel_mux_input::ALDIS_CHANNEL_MUX_INPUT_OPPORTUNITY => {
            engine.step(MuxInput::Opportunity, area)?
        }
        aldis_channel_mux_input::ALDIS_CHANNEL_MUX_INPUT_RECONFIGURE => {
            engine.step(MuxInput::Reconfigure(mx_config::<P>(&input.config)?), area)?
        }
    };
    let mut result = aldis_channel_mux_actions_t::default();
    for (slot, output) in result.items.iter_mut().zip(list.iter()) {
        *slot = mx_action(output);
    }
    // `list` holds at most `CHANNEL_MUX_MAX_ACTIONS` actions, the length of `items`.
    result.count = list.len();
    Ok(result)
}

impl ChannelMuxState {
    /// One step: the input is parsed once into the engine's type (CABI-040).
    fn step(
        &mut self,
        input: &aldis_channel_mux_input_t,
        data: &[u8],
        area: &mut [u8],
    ) -> Result<aldis_channel_mux_actions_t> {
        let reconfigure = parse::<aldis_channel_mux_input>(input.kind)?
            == aldis_channel_mux_input::ALDIS_CHANNEL_MUX_INPUT_RECONFIGURE;
        // A reconfiguration keeps the protocol of the engine (LNK-003).
        let same = |want: aldis_link_protocol| {
            protocol(&aldis_link_channel_t {
                protocol: input.config.protocol,
                ..aldis_link_channel_t::default()
            })
            .map(|p| p == want)
        };
        match self {
            ChannelMuxState::Tm(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM)? {
                    return Err(unsupported_protocol());
                }
                mx_run(e, input, data, area)
            }
            ChannelMuxState::Aos(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS)? {
                    return Err(unsupported_protocol());
                }
                mx_run(e, input, data, area)
            }
            ChannelMuxState::Uslp(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP)? {
                    return Err(unsupported_protocol());
                }
                mx_run(e, input, data, area)
            }
        }
    }

    const fn status(&self) -> MuxStatus {
        match self {
            ChannelMuxState::Tm(e) => *e.status(),
            ChannelMuxState::Aos(e) => *e.status(),
            ChannelMuxState::Uslp(e) => *e.status(),
        }
    }
}

/// Places a `ChannelMux` with no field offered in `storage` (LNK-030, LNK-036; CABI-031,
/// CABI-032).
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` for `NULL` or misaligned `storage` or `cfg`, a flag that is not 0
/// or 1, or a constant that is none; `ALDIS_ERR_INVALID_CONFIG` (`link_protocol`) for TC,
/// (`vc_id`) for a VC listed twice or the OID VC where it is not dedicated, (`vc_priority`) for
/// equal priorities under fixed priority, (`vc_weight`) for a weight of 0, (`frame_len`) for a
/// frame that leaves a VC no data field or a TM frame above 4 095 octets, (`ocf_flag`,
/// `fsh_len`) for an MC and a VC that both have an OCF or a Secondary Header or a Secondary
/// Header outside 1 to 63 octets, (`insert_zone_len`), (`vcf_count_len`), (`spacecraft_id`)
/// for an AOS Issue 4 SCID above 255, (`data_field_content`) for a TM role other than
/// `PACKETS` and `VCA`, (`security_header_len`, `security_trailer_len`) above the limits;
/// `ALDIS_ERR_OUT_OF_RANGE` (`spacecraft_id`, `vc_id`, `map_id`); `ALDIS_ERR_CAPACITY_EXCEEDED`
/// (`capacity`) for more than `ALDIS_LINK_MAX_VIRTUAL_CHANNELS` VCs, (`frame_len`) for a frame
/// above `ALDIS_LINK_MAX_FRAME_OCTETS`.
///
/// # Safety
/// `storage` is valid for writes of one `aldis_channel_mux_t` and outlives its use; `cfg` is
/// valid for a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_channel_mux_init(
    storage: *mut aldis_channel_mux_t,
    cfg: *const aldis_channel_mux_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let cfg = abi::read(cfg)?;
            let head = mx_head(&cfg)?;
            // `init_with` passes a pointer to the state in the slot, valid for writes.
            handle::init_with(storage, |state| {
                mx_place(state, head, cfg.insert_idle_pattern);
            })
        })
    }
}

/// The engine behind `h`.
///
/// # Safety
/// As for `handle::get`.
#[expect(unsafe_code, reason = "checks a caller handle (CABI-032, GEN-071)")]
unsafe fn mx_engine(h: *mut aldis_channel_mux_t) -> Result<handle::Handle<ChannelMuxState>> {
    // SAFETY: forwarded from the caller.
    unsafe { handle::get(h) }
}

/// Runs one input of the `ChannelMux` (API-057, CABI-040).
///
/// The octets of the frame of `OPPORTUNITY` go to `out`, which must hold at least the frame
/// length of the configuration (LNK-075).
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation or a flag or
/// constant that is none; for `OFFER`: `ALDIS_ERR_UNKNOWN_CHANNEL` (also `map_id`),
/// `ALDIS_ERR_INVALID_FIELD` (`first_header_pointer`, `map_id`, `cop_in_effect`, `qos`,
/// `protocol_control_command_flag`), `ALDIS_ERR_OUT_OF_RANGE` (`vcf_count`),
/// `ALDIS_ERR_LENGTH_MISMATCH` (`field_len`), `ALDIS_ERR_WOULD_BLOCK` (LNK-076); for `SET_OCF`
/// and `SET_FSH`: `ALDIS_ERR_UNKNOWN_CHANNEL`, `ALDIS_ERR_INVALID_FIELD` (`ocf_flag`,
/// `fsh_flag`), `ALDIS_ERR_LENGTH_MISMATCH` (`fsh_len`); for `SET_INSERT`:
/// `ALDIS_ERR_INVALID_FIELD`, `ALDIS_ERR_LENGTH_MISMATCH` (`insert_zone_len`); for
/// `OPPORTUNITY`: `ALDIS_ERR_BUFFER_TOO_SMALL`; for `RECONFIGURE`: as `aldis_channel_mux_init`,
/// and `ALDIS_ERR_INVALID_CONFIG` (`link_protocol`) for another protocol. The engine is
/// unchanged and `actions` is not written.
///
/// # Safety
/// `h` is a handle from `aldis_channel_mux_init` whose storage is still valid, or detectably
/// invalid storage; `input` is valid for a read, and its `data` for `data_len` octets; `out` is
/// valid for `out_cap` octets of writes (it may be `NULL` when `out_cap` is 0); `actions` is
/// valid for one write; `err` is `NULL` or valid for one write. No other thread uses the
/// storage (CABI-050).
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_channel_mux_step(
    h: *mut aldis_channel_mux_t,
    input: *const aldis_channel_mux_input_t,
    out: *mut u8,
    out_cap: usize,
    actions: *mut aldis_channel_mux_actions_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let mut h = mx_engine(h)?;
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

/// Reads the counters and the occupancy of the `ChannelMux` (API-056, LNK-002). It changes
/// nothing.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation.
///
/// # Safety
/// As for [`aldis_channel_mux_step`]; `status` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_channel_mux_status(
    h: *mut aldis_channel_mux_t,
    status: *mut aldis_channel_mux_status_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = mx_engine(h)?;
            let status = abi::out_disjoint(status, &[h.range()])?;
            let s = h.read(ChannelMuxState::status);
            status.write(aldis_channel_mux_status_t {
                frames_out: s.frames_out.get(),
                oid_frames: s.oid_frames.get(),
                offers: s.offers.get(),
                refused: s.refused.get(),
                would_block: s.would_block.get(),
                insert_repeated: s.insert_repeated.get(),
                discarded_octets: s.discarded_octets.get(),
                slots: s.slots.into(),
            });
            Ok(())
        })
    }
}

/// Releases the storage: later calls on it fail with `ALDIS_ERR_INVALID_HANDLE` (CABI-032). An
/// invalid handle is ignored.
///
/// # Safety
/// As for [`aldis_channel_mux_step`].
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_channel_mux_deinit(h: *mut aldis_channel_mux_t) {
    // SAFETY: forwarded from the caller; `handle::deinit` checks the tag.
    unsafe { handle::deinit(h) }
}
