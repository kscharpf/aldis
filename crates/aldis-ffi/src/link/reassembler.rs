//! C ABI of the `MapReassembler` (LNK-070…LNK-073, LNK-003, LNK-074, LNK-075, CABI-030…032,
//! CABI-040; M4 plan D4, WP-7d).
//!
//! One opaque handle (`aldis_map_reassembler_t`) in the pattern of [`crate::link`]: the caller
//! declares the typed storage, `_init` places the engine of the protocol named in the
//! configuration in it, and every input is one `_step` call that returns the actions of the
//! input in a fixed-capacity actions struct. The octets of an SDU or a run of packets go to the
//! caller's `out` area and the action holds their `aldis_range_t` (API-061). The status is a
//! read-only call (API-056); there is no callback. A failed call leaves the engine as it was and
//! writes no actions (API-058).
//!
//! **Protocol.** The first member of the configuration is an `aldis_link_channel_t` whose
//! `protocol` names the protocol: TC or USLP (TM and AOS are `ALDIS_ERR_INVALID_CONFIG` on
//! `link_protocol`). The other members of the channel are not read (`protocol` is the tag of the
//! configuration, CABI-040, OI-92); the reassembler reads a VC for each MAP. A reconfiguration
//! cannot change the protocol (LNK-003). The MAP table is `map_count` entries of `maps`.
//!
//! The reassembler holds `ALDIS_LINK_MAX_MAP_CHANNELS` assembly buffers of
//! `ALDIS_LINK_MAX_FRAME_OCTETS` octets, the longest SDU of a MAP (LNK-073; CABI-030). It is
//! built in place (`aldis::link::inplace`, #240), so the buffers are not on the stack.

use aldis::core::ids::MapId;
use aldis::core::{Error, ErrorKind, Result, field};
use aldis::link::protocol::{Tc, Uslp};
use aldis::link::reassembler::{
    FduIn, MAP_REASSEMBLER_MAX_ACTIONS, MapReassembler, MapReassemblerBuffers,
    MapReassemblerConfig, MapReassemblerHead, ReassemblerActions, ReassemblerInput, ReassemblerMap,
    ReassemblerOutput, ReassemblerProtocol, ReassemblerStatus, ReassemblyEvent,
    TcReassemblerParams, UslpFdu,
};

use super::common::{aldis_map_service, listed, range, wrong_service};
use crate::abi::{self, Out, flag};
use crate::capacities::{ALDIS_LINK_MAX_FRAME_OCTETS, ALDIS_LINK_MAX_MAP_CHANNELS};
use crate::handle::{self, InPlace, Storage};
use crate::link::{
    aldis_link_channel_t, aldis_link_occupancy_t, aldis_link_protocol, protocol, protocol_tag,
    unsupported_protocol,
};
use crate::mirror::{mirror, parse};
use crate::range::aldis_range_t;

/// The MAPs of the engine: `ALDIS_LINK_MAX_MAP_CHANNELS` (CABI-030).
const M: usize = ALDIS_LINK_MAX_MAP_CHANNELS;
/// The assembly buffer of each MAP of the reassembler: `ALDIS_LINK_MAX_FRAME_OCTETS` (CABI-030).
const S: usize = ALDIS_LINK_MAX_FRAME_OCTETS;

/// The most actions one `aldis_map_reassembler_step` returns (LNK-074): two events, or an event
/// and a delivery.
pub const ALDIS_MAP_REASSEMBLER_MAX_ACTIONS: usize = 2;
const _: () = assert!(ALDIS_MAP_REASSEMBLER_MAX_ACTIONS == MAP_REASSEMBLER_MAX_ACTIONS);

/// 64-bit words of one `aldis_map_reassembler_t` (CABI-031).
pub const ALDIS_MAP_REASSEMBLER_WORDS: usize = 4174;
const _: () = assert!(
    size_of::<crate::handle::Slot<MapReassemblerState>>() <= ALDIS_MAP_REASSEMBLER_WORDS * 8
);

type Ra<P> = MapReassembler<P, M, S>;

handle::engine_enum! {
    /// The `MapReassembler` of the protocol chosen at `aldis_map_reassembler_init` (LNK-001).
    #[expect(
        dead_code,
        reason = "the variants are written in place by the init function, not constructed"
    )]
    pub(crate) enum MapReassemblerState {
        /// TC.
        Tc(Ra<Tc>) = protocol_tag(aldis_link_protocol::ALDIS_LINK_PROTOCOL_TC),
        /// USLP.
        Uslp(Ra<Uslp>) = protocol_tag(aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP),
    }
}

/// The kind of an `aldis_map_reassembler_input_t` (LNK-070, LNK-003; API-057).
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
pub enum aldis_map_reassembler_input {
    /// An FDU (TC) or TFDF (USLP) of a VC (`vc_id`, `map_id`, `gap`, `data`): LNK-070.
    ALDIS_MAP_REASSEMBLER_INPUT_FDU = 0,
    /// Replace the configuration (`config`) between data units, discarding the SDUs in progress
    /// (LNK-003). The protocol cannot change.
    ALDIS_MAP_REASSEMBLER_INPUT_RECONFIGURE = 1,
}
mirror!(pub aldis_map_reassembler_input: u32 {
    ALDIS_MAP_REASSEMBLER_INPUT_FDU,
    ALDIS_MAP_REASSEMBLER_INPUT_RECONFIGURE
});

/// The kind of an `aldis_map_reassembler_action_t` (API-057).
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
pub enum aldis_map_reassembler_action {
    /// A reassembly event (`event`, `vc_id`, `map_id`): LNK-070, LNK-073.
    ALDIS_MAP_REASSEMBLER_ACTION_EVENT = 0,
    /// An SDU, whole, in `out` (`data`): LNK-070.
    ALDIS_MAP_REASSEMBLER_ACTION_SDU = 1,
    /// Complete packets, one after the other in `out` (`data`, `count`): LNK-071.
    ALDIS_MAP_REASSEMBLER_ACTION_PACKETS = 2,
    /// Octets of a stream, as they arrived, in `out` (`data`, `after_gap`): LNK-072.
    ALDIS_MAP_REASSEMBLER_ACTION_STREAM_DATA = 3,
    /// A reconfiguration discarded `octets` of the SDUs in progress (LNK-003).
    ALDIS_MAP_REASSEMBLER_ACTION_DISCARDED = 4,
}
mirror!(pub aldis_map_reassembler_action: u32 {
    ALDIS_MAP_REASSEMBLER_ACTION_EVENT,
    ALDIS_MAP_REASSEMBLER_ACTION_SDU,
    ALDIS_MAP_REASSEMBLER_ACTION_PACKETS,
    ALDIS_MAP_REASSEMBLER_ACTION_STREAM_DATA,
    ALDIS_MAP_REASSEMBLER_ACTION_DISCARDED
});

/// A reassembly event (LNK-070, LNK-073; docs/04-engines.md §5).
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
pub enum aldis_map_reassembly_event {
    /// The SDUs in progress were discarded because of a gap in the VCF Count.
    ALDIS_MAP_REASSEMBLY_EVENT_GAP_DISCARD = 0,
    /// A segment broke the sequence of the flags or rules; a partial SDU was discarded.
    ALDIS_MAP_REASSEMBLY_EVENT_SEQUENCE_VIOLATION = 1,
    /// An FDU for a MAP that is not configured was discarded.
    ALDIS_MAP_REASSEMBLY_EVENT_UNKNOWN_MAP = 2,
    /// An SDU would exceed the maximum SDU length of its MAP and was discarded.
    ALDIS_MAP_REASSEMBLY_EVENT_SDU_TOO_LONG = 3,
}
mirror!(pub aldis_map_reassembly_event: u32 {
    ALDIS_MAP_REASSEMBLY_EVENT_GAP_DISCARD,
    ALDIS_MAP_REASSEMBLY_EVENT_SEQUENCE_VIOLATION,
    ALDIS_MAP_REASSEMBLY_EVENT_UNKNOWN_MAP,
    ALDIS_MAP_REASSEMBLY_EVENT_SDU_TOO_LONG
});

/// A `MapReassembler` in caller storage: the caller declares an object of this type, and the
/// pointer to it is the handle (LNK-070, CABI-031, CABI-032). The words are the library's.
#[repr(C)]
pub struct aldis_map_reassembler_t {
    /// The slot of [`handle`]: the tag, then the state.
    pub words: [u64; ALDIS_MAP_REASSEMBLER_WORDS],
}

impl Storage for aldis_map_reassembler_t {
    type State = MapReassemblerState;
    const MAGIC: u32 = 0x414C_4D52; // "ALMR"
}

/// One MAP of a `MapReassembler` (232.0 §5.4 Table 5-4; 732.1 §5.5 Table 5-4; LNK-070,
/// LNK-073).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_map_reassembler_map_t {
    /// The VCID of the MAP, 0 to 63.
    pub vc_id: u8,
    /// The MAP ID: 0 to 63 on TC, 0 to 15 on USLP.
    pub map_id: u8,
    /// `ALDIS_MAP_SERVICE_PACKETS`, `SDU`, or `OCTET_STREAM`.
    pub service: u32,
    /// The longest SDU or packet of the MAP in octets (LNK-073), from 1 to
    /// `ALDIS_LINK_MAX_FRAME_OCTETS`; ignored for an octet stream.
    pub max_sdu_len: usize,
}

/// The managed parameters of a `MapReassembler` (232.0 §5.4; 732.1 §5.5; LNK-070, LNK-073).
///
/// Zero-initialize the struct before setting members: a member that the chosen `protocol` does not
/// read is ignored (OI-92).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_map_reassembler_config_t {
    /// The protocol (TC or USLP); the other members of the channel are not used but must be
    /// valid (each MAP names its VC).
    pub channel: aldis_link_channel_t,
    /// TC: 1 when the FDUs have a Segment Header, else 0 (every FDU is then whole user data of
    /// MAP 0).
    pub tc_segment_header: u8,
    /// The entries of `maps` in use, 1 to `ALDIS_LINK_MAX_MAP_CHANNELS`.
    pub map_count: usize,
    /// The MAPs; the first `map_count` are read.
    pub maps: [aldis_map_reassembler_map_t; ALDIS_LINK_MAX_MAP_CHANNELS],
}

/// One input of a `MapReassembler` (API-057, CABI-040). A field that the kind does not use is
/// ignored.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct aldis_map_reassembler_input_t {
    /// `ALDIS_MAP_REASSEMBLER_INPUT_*`.
    pub kind: u32,
    /// For `FDU`: the Frame Data Field with its Segment Header (TC) or the TFDF (USLP), read
    /// once and copied (API-060). `data` is checked on every kind and must not overlap the
    /// storage or `out`: `NULL` only with `data_len` 0.
    pub data: *const u8,
    /// The octets at `data`.
    pub data_len: usize,
    /// For `FDU`: the VCID of the frame.
    pub vc_id: u8,
    /// For `FDU`, USLP: the MAP ID of the frame.
    pub map_id: u8,
    /// For `FDU`, USLP: 1 when a frame gap was flagged on the field (LNK-051), else 0.
    pub gap: u8,
    /// For `RECONFIGURE`: the new configuration.
    pub config: aldis_map_reassembler_config_t,
}

/// One action of a `MapReassembler` (API-057, API-061). A field that the kind does not use is 0.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_map_reassembler_action_t {
    /// `ALDIS_MAP_REASSEMBLER_ACTION_*`.
    pub kind: u32,
    /// For `EVENT`: `ALDIS_MAP_REASSEMBLY_EVENT_*`.
    pub event: u32,
    /// For every kind but `DISCARDED`: the VCID.
    pub vc_id: u8,
    /// For every kind but `DISCARDED`: the MAP ID.
    pub map_id: u8,
    /// For `SDU`, `PACKETS`, `STREAM_DATA`: where the delivery is in `out`; for `PACKETS`, each
    /// packet is as long as its own length field says.
    pub data: aldis_range_t,
    /// For `PACKETS`: how many packets the run holds.
    pub count: usize,
    /// For `STREAM_DATA`: 1 when a gap was flagged on the field that carried the octets.
    pub after_gap: u8,
    /// For `DISCARDED`: the octets of the partial SDUs.
    pub octets: usize,
}

/// The actions of one input of a `MapReassembler`.
///
/// Requirements: LNK-040, LNK-070, LNK-074, CABI-040; docs/04-engines.md §4.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_map_reassembler_actions_t {
    /// The number of actions in `items`, at most `ALDIS_MAP_REASSEMBLER_MAX_ACTIONS`.
    pub count: usize,
    /// The actions; those from `count` on are zero.
    pub items: [aldis_map_reassembler_action_t; ALDIS_MAP_REASSEMBLER_MAX_ACTIONS],
}

/// A read-only snapshot of a `MapReassembler` (API-056, LNK-002).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_map_reassembler_status_t {
    /// FDUs taken.
    pub fdus_in: u64,
    /// SDUs and packets delivered (a run counts each packet).
    pub delivered: u64,
    /// FDUs refused as unreadable.
    pub refused: u64,
    /// Events `GAP_DISCARD`.
    pub gap_discards: u64,
    /// Events `SEQUENCE_VIOLATION`.
    pub sequence_violations: u64,
    /// Events `UNKNOWN_MAP`.
    pub unknown_maps: u64,
    /// Events `SDU_TOO_LONG`.
    pub sdus_too_long: u64,
    /// Octets of partial SDUs discarded by an event or `RECONFIGURE` (LNK-003).
    pub discarded_octets: u64,
    /// The assembly buffers: the octets of the SDUs in progress.
    pub assembly: aldis_link_occupancy_t,
}

/// The protocol-specific part of the reassembler configuration and inputs (LNK-001).
trait RaProtocol: ReassemblerProtocol {
    /// The parameters of the VCs.
    fn channel(c: &aldis_map_reassembler_config_t) -> Result<Self::Channel>;
    /// What the caller adds to the data of the FDU of `input`.
    fn meta(input: &aldis_map_reassembler_input_t) -> Result<Self::Meta>;
}

impl RaProtocol for Tc {
    fn channel(c: &aldis_map_reassembler_config_t) -> Result<TcReassemblerParams> {
        Ok(if flag(c.tc_segment_header)? {
            TcReassemblerParams::with_segment_header()
        } else {
            TcReassemblerParams::without_segment_header()
        })
    }

    fn meta(input: &aldis_map_reassembler_input_t) -> Result<()> {
        // The gap flag is a member of USLP only: TC does not read it (CABI-040; OI-92).
        let _ = input;
        Ok(())
    }
}

impl RaProtocol for Uslp {
    fn channel(_: &aldis_map_reassembler_config_t) -> Result<()> {
        Ok(())
    }

    fn meta(input: &aldis_map_reassembler_input_t) -> Result<UslpFdu> {
        Ok(UslpFdu {
            map: MapId::<4>::try_from(input.map_id)?,
            gap: flag(input.gap)?,
        })
    }
}

/// The MAP `m` of the protocol `P` (LNK-070, LNK-073).
fn ra_map<P: ReassemblerProtocol>(m: &aldis_map_reassembler_map_t) -> Result<ReassemblerMap<P>> {
    let vc = P::Vc::try_from(m.vc_id)?;
    let id = P::Map::try_from(m.map_id)?;
    match parse::<aldis_map_service>(m.service)? {
        aldis_map_service::ALDIS_MAP_SERVICE_PACKETS => {
            ReassemblerMap::<P>::packets(vc, id, m.max_sdu_len)
        }
        aldis_map_service::ALDIS_MAP_SERVICE_SDU => ReassemblerMap::<P>::sdu(vc, id, m.max_sdu_len),
        aldis_map_service::ALDIS_MAP_SERVICE_OCTET_STREAM => {
            Ok(ReassemblerMap::<P>::octet_stream(vc, id))
        }
        aldis_map_service::ALDIS_MAP_SERVICE_ANY => Err(wrong_service()),
    }
}

/// The reassembler configuration of `c` for the protocol `P`.
fn ra_config<P: RaProtocol>(
    c: &aldis_map_reassembler_config_t,
) -> Result<MapReassemblerConfig<P, M>> {
    let channel = P::channel(c)?;
    let mut table = [None; M];
    let count = listed(&c.maps, c.map_count, ra_map::<P>, &mut table)?;
    // The first entry fills the array; only the first `count` of it are given on. Without an
    // entry there is nothing to list.
    let first = table.first().copied().flatten().filter(|_| count > 0);
    let mut entries = first.map(|filler| [filler; M]);
    if let Some(array) = entries.as_mut() {
        for (dst, src) in array.iter_mut().zip(&table).take(count) {
            if let Some(entry) = src {
                *dst = *entry;
            }
        }
    }
    let given = match &entries {
        None => &[],
        Some(array) => array
            .get(..count)
            .ok_or(Error::new(ErrorKind::CapacityExceeded).with_field(field::MAP_ID))?,
    };
    MapReassemblerConfig::new(channel, given)
}

#[expect(
    unsafe_code,
    reason = "the layout of an engine built in place (GEN-071)"
)]
// SAFETY: `head` and `bufs` are the only fields of the engine, and the offsets are `offset_of!`;
// the buffers are plain octet arrays, valid when zeroed.
unsafe impl<P: ReassemblerProtocol, const NM: usize, const NS: usize> InPlace
    for MapReassembler<P, NM, NS>
{
    type Head = MapReassemblerHead<P, NM>;
    type Buffers = MapReassemblerBuffers<NM, NS>;
    const HEAD_OFFSET: usize = <MapReassembler<P, NM, NS>>::HEAD_OFFSET;
    const BUFFERS_OFFSET: usize = <MapReassembler<P, NM, NS>>::BUFFERS_OFFSET;
}

/// One step of `engine`, in the protocol `P` (API-057).
fn ra_run<P: RaProtocol>(
    engine: &mut Ra<P>,
    input: &aldis_map_reassembler_input_t,
    data: &[u8],
    area: &mut [u8],
) -> Result<ReassemblerActions> {
    let input = match parse::<aldis_map_reassembler_input>(input.kind)? {
        aldis_map_reassembler_input::ALDIS_MAP_REASSEMBLER_INPUT_FDU => {
            ReassemblerInput::Fdu(FduIn {
                vc: P::Vc::try_from(input.vc_id)?,
                meta: P::meta(input)?,
                data,
            })
        }
        aldis_map_reassembler_input::ALDIS_MAP_REASSEMBLER_INPUT_RECONFIGURE => {
            ReassemblerInput::Reconfigure(ra_config::<P>(&input.config)?)
        }
    };
    engine.step(input, area)
}

impl MapReassemblerState {
    /// One step: the input is parsed once into the engine's type (CABI-040).
    fn step(
        &mut self,
        input: &aldis_map_reassembler_input_t,
        data: &[u8],
        area: &mut [u8],
    ) -> Result<ReassemblerActions> {
        let reconfigure = parse::<aldis_map_reassembler_input>(input.kind)?
            == aldis_map_reassembler_input::ALDIS_MAP_REASSEMBLER_INPUT_RECONFIGURE;
        let ch = &input.config.channel;
        // A reconfiguration keeps the protocol of the engine (LNK-003).
        let same = |want| protocol(ch).map(|p| p == want);
        match self {
            MapReassemblerState::Tc(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_TC)? {
                    return Err(unsupported_protocol());
                }
                ra_run(e, input, data, area)
            }
            MapReassemblerState::Uslp(e) => {
                if reconfigure && !same(aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP)? {
                    return Err(unsupported_protocol());
                }
                ra_run(e, input, data, area)
            }
        }
    }

    const fn status(&self) -> ReassemblerStatus {
        match self {
            MapReassemblerState::Tc(e) => *e.status(),
            MapReassemblerState::Uslp(e) => *e.status(),
        }
    }
}

const fn event_code(event: ReassemblyEvent) -> u32 {
    match event {
        ReassemblyEvent::GapDiscard => ALDIS_MAP_REASSEMBLY_EVENT_GAP_DISCARD,
        ReassemblyEvent::SequenceViolation => ALDIS_MAP_REASSEMBLY_EVENT_SEQUENCE_VIOLATION,
        ReassemblyEvent::UnknownMap => ALDIS_MAP_REASSEMBLY_EVENT_UNKNOWN_MAP,
        ReassemblyEvent::SduTooLong => ALDIS_MAP_REASSEMBLY_EVENT_SDU_TOO_LONG,
    }
}

fn ra_action(output: &ReassemblerOutput) -> aldis_map_reassembler_action_t {
    let none = aldis_map_reassembler_action_t::default();
    match *output {
        ReassemblerOutput::Event { event, vc, map } => aldis_map_reassembler_action_t {
            kind: ALDIS_MAP_REASSEMBLER_ACTION_EVENT,
            event: event_code(event),
            vc_id: vc,
            map_id: map,
            ..none
        },
        ReassemblerOutput::Sdu { vc, map, sdu } => aldis_map_reassembler_action_t {
            kind: ALDIS_MAP_REASSEMBLER_ACTION_SDU,
            vc_id: vc,
            map_id: map,
            data: range(sdu),
            ..none
        },
        ReassemblerOutput::Packets {
            vc,
            map,
            run,
            count,
        } => aldis_map_reassembler_action_t {
            kind: ALDIS_MAP_REASSEMBLER_ACTION_PACKETS,
            vc_id: vc,
            map_id: map,
            data: range(run),
            count,
            ..none
        },
        ReassemblerOutput::StreamData {
            vc,
            map,
            data,
            after_gap,
        } => aldis_map_reassembler_action_t {
            kind: ALDIS_MAP_REASSEMBLER_ACTION_STREAM_DATA,
            vc_id: vc,
            map_id: map,
            data: range(data),
            after_gap: u8::from(after_gap),
            ..none
        },
        ReassemblerOutput::Discarded { octets } => aldis_map_reassembler_action_t {
            kind: ALDIS_MAP_REASSEMBLER_ACTION_DISCARDED,
            octets,
            ..none
        },
    }
}

/// Places a `MapReassembler` with no SDU in progress in `storage` (LNK-070, LNK-073; CABI-031,
/// CABI-032).
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` for `NULL` or misaligned `storage` or `cfg`, a flag that is not 0
/// or 1, or a constant that is none; `ALDIS_ERR_INVALID_CONFIG` (`link_protocol`) for TM and
/// AOS, (`map_id`) for no MAP or two MAPs of one VC with one MAP ID, (`data_field_content`) for
/// the service `ANY`, (`max_sdu_len`) for 0; `ALDIS_ERR_OUT_OF_RANGE` (`vc_id`, `map_id`);
/// `ALDIS_ERR_CAPACITY_EXCEEDED` (`map_id`) for more MAPs than `ALDIS_LINK_MAX_MAP_CHANNELS`,
/// (`max_sdu_len`) above `ALDIS_LINK_MAX_FRAME_OCTETS`.
///
/// # Safety
/// `storage` is valid for writes of one `aldis_map_reassembler_t` and outlives its use; `cfg`
/// is valid for a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_map_reassembler_init(
    storage: *mut aldis_map_reassembler_t,
    cfg: *const aldis_map_reassembler_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let cfg = abi::read(cfg)?;
            match protocol(&cfg.channel)? {
                aldis_link_protocol::ALDIS_LINK_PROTOCOL_TC => handle::init_variant::<_, Ra<Tc>>(
                    storage,
                    Ra::new_head(ra_config::<Tc>(&cfg)?)?,
                ),
                aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP => {
                    handle::init_variant::<_, Ra<Uslp>>(
                        storage,
                        Ra::new_head(ra_config::<Uslp>(&cfg)?)?,
                    )
                }
                aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM
                | aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS => Err(unsupported_protocol()),
            }
        })
    }
}

/// The engine behind `h`.
///
/// # Safety
/// As for `handle::get`.
#[expect(unsafe_code, reason = "checks a caller handle (CABI-032, GEN-071)")]
unsafe fn ra_engine(
    h: *mut aldis_map_reassembler_t,
) -> Result<handle::Handle<MapReassemblerState>> {
    // SAFETY: forwarded from the caller.
    unsafe { handle::get(h) }
}

handle::step_io!(
    aldis_map_reassembler_t,
    aldis_map_reassembler_input_t,
    aldis_map_reassembler_actions_t
);

impl handle::Step for aldis_map_reassembler_t {
    fn step(
        state: &mut MapReassemblerState,
        input: &aldis_map_reassembler_input_t,
        data: &[u8],
        area: &mut [u8],
        actions: Out<'_, aldis_map_reassembler_actions_t>,
    ) -> Result<()> {
        let list = state.step(input, data, area)?;
        let mut result = aldis_map_reassembler_actions_t::default();
        for (slot, output) in result.items.iter_mut().zip(list.iter()) {
            *slot = ra_action(output);
        }
        // `list` holds at most `MAP_REASSEMBLER_MAX_ACTIONS` actions, the length of `items`.
        result.count = list.len();
        actions.write(result);
        Ok(())
    }
}

/// Runs one input of the `MapReassembler` (API-057, CABI-040). The engine has no timer
/// (API-053), so the call takes no time.
///
/// The octets of the delivery go to `out`, which must hold at least `max_sdu_len − 1` plus the
/// user data of the FDU (LNK-075).
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation, a flag that
/// is not 0 or 1, or a constant that is none; `ALDIS_ERR_OUT_OF_RANGE` (`vc_id`, `map_id`); for
/// `FDU`: the errors of the protocol's reading (`ALDIS_ERR_INVALID_FIELD`
/// (`frame_data_field`, `tfdz_construction_rule`, `packet_version`), `ALDIS_ERR_MALFORMED`
/// (`fhp_lvo_pointer`), `ALDIS_ERR_LENGTH_MISMATCH` (`packet_len`)), and
/// `ALDIS_ERR_BUFFER_TOO_SMALL`; for `RECONFIGURE`: as `aldis_map_reassembler_init`, and
/// `ALDIS_ERR_INVALID_CONFIG` (`link_protocol`) for another protocol. The engine is unchanged
/// and `actions` is not written.
///
/// # Safety
/// `h` is a handle from `aldis_map_reassembler_init` whose storage is still valid, or
/// detectably invalid storage; `input` is valid for a read, and its `data` for `data_len`
/// octets; `out` is valid for `out_cap` octets of writes (it may be `NULL` when `out_cap` is 0);
/// `actions` is valid for one write; `err` is `NULL` or valid for one write. No other thread
/// uses the storage (CABI-050).
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_map_reassembler_step(
    h: *mut aldis_map_reassembler_t,
    input: *const aldis_map_reassembler_input_t,
    out: *mut u8,
    out_cap: usize,
    actions: *mut aldis_map_reassembler_actions_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `handle` checks them.
    unsafe { handle::step::<aldis_map_reassembler_t>(h, input, out, out_cap, actions, err) }
}

/// Reads the counters and the occupancy of the `MapReassembler` (API-056, LNK-002). It changes
/// nothing.
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; `ALDIS_ERR_INVALID_FIELD` for a CABI-022 violation.
///
/// # Safety
/// As for [`aldis_map_reassembler_step`]; `status` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_map_reassembler_status(
    h: *mut aldis_map_reassembler_t,
    status: *mut aldis_map_reassembler_status_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = ra_engine(h)?;
            let status = abi::out_disjoint(status, &[h.range()])?;
            let s = h.read(MapReassemblerState::status);
            status.write(aldis_map_reassembler_status_t {
                fdus_in: s.fdus_in.get(),
                delivered: s.delivered.get(),
                refused: s.refused.get(),
                gap_discards: s.gap_discards.get(),
                sequence_violations: s.sequence_violations.get(),
                unknown_maps: s.unknown_maps.get(),
                sdus_too_long: s.sdus_too_long.get(),
                discarded_octets: s.discarded_octets.get(),
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
/// As for [`aldis_map_reassembler_step`].
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_map_reassembler_deinit(h: *mut aldis_map_reassembler_t) {
    // SAFETY: forwarded from the caller; `handle::deinit` checks the tag.
    unsafe { handle::deinit(h) }
}
