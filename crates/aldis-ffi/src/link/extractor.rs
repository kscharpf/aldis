//! C ABI of the `PacketExtractor` (LNK-050…LNK-054, LNK-077, LNK-003, CABI-030…032, CABI-040;
//! M4 plan D4, WP-7b).
//!
//! One opaque handle (`aldis_packet_extractor_t`) in the pattern of [`crate::link`]: the caller
//! declares the typed storage, `_init` places the engine of the protocol named in the
//! configuration in it, and every input is one `_step` call that returns the actions of the
//! input in a fixed-capacity actions struct. The octets of a delivered part go to the caller's
//! `out` area and the action holds their `aldis_range_t` (API-061). The status is a read-only
//! call (API-056); there is no callback. A failed call leaves the engine as it was and writes no
//! actions (API-058).
//!
//! **Protocol.** The first member of the configuration names the protocol (`protocol`, an
//! `ALDIS_LINK_PROTOCOL_*` constant): TM, AOS, or USLP. The configuration holds the
//! `aldis_link_channel_t` of the generators; its `vc_id` is not read, because the extractor
//! serves the data fields the receiver delivers for one VC. The extractor holds an assembly
//! buffer of `ALDIS_LINK_PACKET_QUEUE_OCTETS` octets and takes a data field of
//! `ALDIS_LINK_MAX_FRAME_OCTETS` at most (CABI-030).

use aldis::core::Result;
use aldis::link::extractor::{
    EXTRACTOR_MAX_ACTIONS, ExtractionError, ExtractorActions, ExtractorInput, ExtractorOutput,
    ExtractorStatus, FieldIn, PacketExtractor, PacketExtractorBuffers, PacketExtractorConfig,
    PacketExtractorHead,
};
use aldis::link::protocol::{Aos, PacketProtocol, Tm, TmParams, Uslp};

use super::common::range;
use crate::abi::{self, Out, flag};
use crate::capacities::{ALDIS_LINK_MAX_FRAME_OCTETS, ALDIS_LINK_PACKET_QUEUE_OCTETS};
use crate::handle::{self, InPlace, Storage};
use crate::link::{
    aldis_link_channel_t, aldis_link_occupancy_t, aldis_link_protocol, aos_params, protocol,
    protocol_tag, unsupported_protocol, uslp_params, valid_pvns,
};
use crate::mirror::{mirror, parse};
use crate::range::aldis_range_t;

/// The data field capacity of the extractor: `ALDIS_LINK_MAX_FRAME_OCTETS` (CABI-030).
const D: usize = ALDIS_LINK_MAX_FRAME_OCTETS;
/// The assembly buffer of the extractor: `ALDIS_LINK_PACKET_QUEUE_OCTETS` (CABI-030).
const M: usize = ALDIS_LINK_PACKET_QUEUE_OCTETS;

/// The most actions one `aldis_packet_extractor_step` returns (LNK-074): a run of packets, an
/// incomplete packet, a discard, and an error.
pub const ALDIS_PACKET_EXTRACTOR_MAX_ACTIONS: usize = 4;
const _: () = assert!(ALDIS_PACKET_EXTRACTOR_MAX_ACTIONS == EXTRACTOR_MAX_ACTIONS);

/// 64-bit words of one `aldis_packet_extractor_t` (CABI-031): the tag and the largest of the
/// engines of TM, AOS, and USLP at the build capacities; the build fails when it outgrows it.
pub const ALDIS_PACKET_EXTRACTOR_WORDS: usize = 1043;
const _: () = assert!(
    size_of::<crate::handle::Slot<PacketExtractorState>>() <= ALDIS_PACKET_EXTRACTOR_WORDS * 8
);

type Pe<P> = PacketExtractor<P, M, D>;

handle::engine_enum! {
    /// The `PacketExtractor` of the protocol chosen at `aldis_packet_extractor_init` (LNK-001).
    #[expect(
        dead_code,
        reason = "the variants are written in place by the init function, not constructed"
    )]
    pub(crate) enum PacketExtractorState {
        /// TM.
        Tm(Pe<Tm>) = protocol_tag(aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM),
        /// AOS.
        Aos(Pe<Aos>) = protocol_tag(aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS),
        /// USLP.
        Uslp(Pe<Uslp>) = protocol_tag(aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP),
    }
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
///
/// Zero-initialize the struct before setting members: a member that the chosen `protocol` does not
/// read is ignored (OI-92).
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

#[expect(
    unsafe_code,
    reason = "the layout of an engine built in place (GEN-071)"
)]
// SAFETY: `head` and `bufs` are the only fields of the engine, and the offsets are `offset_of!`;
// the buffers are plain octet arrays, valid when zeroed.
unsafe impl<P: PacketProtocol, const NM: usize, const ND: usize> InPlace
    for PacketExtractor<P, NM, ND>
{
    type Head = PacketExtractorHead<P>;
    type Buffers = PacketExtractorBuffers<NM>;
    const HEAD_OFFSET: usize = <PacketExtractor<P, NM, ND>>::HEAD_OFFSET;
    const BUFFERS_OFFSET: usize = <PacketExtractor<P, NM, ND>>::BUFFERS_OFFSET;
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
            match protocol(&cfg.channel)? {
                aldis_link_protocol::ALDIS_LINK_PROTOCOL_TM => handle::init_variant::<_, Pe<Tm>>(
                    storage,
                    Pe::new_head(pe_config(&cfg, TmParams)?)?,
                ),
                aldis_link_protocol::ALDIS_LINK_PROTOCOL_AOS => handle::init_variant::<_, Pe<Aos>>(
                    storage,
                    Pe::new_head(pe_config(&cfg, aos_params(&cfg.channel)?)?)?,
                ),
                aldis_link_protocol::ALDIS_LINK_PROTOCOL_USLP => {
                    handle::init_variant::<_, Pe<Uslp>>(
                        storage,
                        Pe::new_head(pe_config(&cfg, uslp_params(&cfg.channel)?)?)?,
                    )
                }
                aldis_link_protocol::ALDIS_LINK_PROTOCOL_TC => Err(unsupported_protocol()),
            }
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

handle::step_io!(
    aldis_packet_extractor_t,
    aldis_packet_extractor_input_t,
    aldis_packet_extractor_actions_t
);

impl handle::Step for aldis_packet_extractor_t {
    fn step(
        state: &mut PacketExtractorState,
        input: &aldis_packet_extractor_input_t,
        data: &[u8],
        area: &mut [u8],
        actions: Out<'_, aldis_packet_extractor_actions_t>,
    ) -> Result<()> {
        let list = state.step(input, data, area)?;
        let mut result = aldis_packet_extractor_actions_t::default();
        for (slot, output) in result.items.iter_mut().zip(list.iter()) {
            *slot = pe_action(output);
        }
        // `list` holds at most `EXTRACTOR_MAX_ACTIONS` actions, the length of `items`.
        result.count = list.len();
        actions.write(result);
        Ok(())
    }
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
    // SAFETY: every pointer is valid as `# Safety` states; `handle` checks them.
    unsafe { handle::step::<aldis_packet_extractor_t>(h, input, out, out_cap, actions, err) }
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
