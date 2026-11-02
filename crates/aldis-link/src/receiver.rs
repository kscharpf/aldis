//! The `FrameReceiver`: routes the frames of a physical channel (LNK-050…LNK-054, LNK-077,
//! LNK-079…LNK-081, LNK-002, LNK-003, LNK-074, LNK-075; docs/04-engines.md §3.1, §4).
//!
//! It routes each frame to its VC, checks the frame counts, and delivers the frame's parts.
//!
//! The engine serves one master channel of one protocol (TM, TC, AOS, or USLP; LNK-001, v1 has
//! one master channel per receiver, OI-75). It takes a frame whose FECF the caller has checked
//! (correcting the AOS FHEC) and whose SDLS processing the caller has verified
//! ([`CheckedFrame`], LNK-050, API-074). It does not decode the frame again: the codec views
//! reject a frame for a VC or MAP that is not configured, and the receiver must still count such
//! a frame and extract its Insert Zone, FSH, and OCF (OI-77), so it reads the fields it needs
//! from the octets itself.
//!
//! **What it returns.** In the order of the procedure of the protocol (docs/04-engines.md §4):
//! the gap or discontinuity of the MC and VC counts, the FSH, the Insert SDU, the OCF, then the
//! data field ([`ReceiverOutput::DataField`]) for the VC's extractor, reassembler, or user, or
//! [`ReceiverOutput::FrameDiscarded`] for a frame it rejects. A TC frame is returned whole
//! ([`ReceiverOutput::TcFrame`]) for FARM-1 (LNK-081). The octets of an FSH, an Insert Zone, or
//! a data field are copied to the output area, and the action names the range.
//!
//! **Bounds (LNK-074, LNK-075).** One input returns at most [`FRAME_RECEIVER_MAX_ACTIONS`]
//! actions, and `Frame` writes at most [`FrameReceiverConfig::area_len`] octets to the output
//! area; a shorter area fails the input with `BufferTooSmall` before any state change. The
//! per-protocol worst cases are argued at [`FRAME_RECEIVER_MAX_ACTIONS`].
//!
//! **No state change on a refusal (API-058).** A frame is parsed completely before any count or
//! flag changes, so a frame the receiver refuses (`LengthMismatch`, `Malformed`, `ReservedValue`,
//! `BufferTooSmall`, or an `InvalidField` from a VC whose configuration the frame contradicts)
//! changes nothing but the `refused` counter.

use core::fmt::Debug;

use aldis_core::engine::{Actions, OutRange};
use aldis_core::ids::MapId;
use aldis_core::layout::{FrameLayout, FramedBytes, LayoutSpec};
use aldis_core::ocf::Ocf;
use aldis_core::security::SecurityLayout;
use aldis_core::{Error, ErrorKind, Result, field};

use crate::common::Counter;
use crate::protocol::Protocol;

#[cfg(feature = "aos")]
mod aos;
mod tc;
mod tm;
mod uslp;

#[cfg(feature = "aos")]
pub use aos::{AosChannel, AosContent, AosVc};
pub use tc::{TcChannel, TcVc};
pub use tm::{TmChannel, TmContent, TmVc};
pub use uslp::{MapSet, UslpChannel, UslpVc};

/// The most actions one input of a `FrameReceiver` returns (LNK-074, API-057).
///
/// **Worst case per protocol.** A counter yields a gap or a discontinuity, not both; a frame has
/// one FSH and one OCF, of the MC or of the VC; and a frame ends in one data field or one
/// `FrameDiscarded`, never both.
///
/// - **TM**: the MC count, the VC count, the FSH, the OCF, then `FrameDiscarded` for a frame of
///   a VC that is configured but not served (the VC services deliver the VC's own FSH and OCF
///   there), or the data field for a served VC: 5 either way (OI-77).
/// - **USLP**: the VC count, the Insert SDU, the OCF, then `FrameDiscarded` (a VC that is not
///   served, or an unknown MAP of a known VC) or the data field or the whole frame for the COP:
///   4.
/// - **AOS**: the VC count, the Insert SDU, the OCF, the data field: 4 for a served frame, and
///   the VC count, the Insert SDU, `FrameDiscarded`: 3 for a rejected one (an AOS frame that is
///   rejected extracts no OCF, because the OCF is a VC service).
/// - **TC**: one `TcFrame` or one `FrameDiscarded`: 1.
/// - `FrameLoss`, `CallerDiscard`, `RawFrame`: 1. `Reconfigure`: 0, because a receiver retains no
///   octets.
///
/// 5 is the largest, so the list is a constant that does not depend on any capacity.
pub const FRAME_RECEIVER_MAX_ACTIONS: usize = 5;

/// The actions of one input of a `FrameReceiver` (LNK-074).
pub type ReceiverActions<P> = Actions<ReceiverOutput<P>, FRAME_RECEIVER_MAX_ACTIONS>;

/// Which channel a part of the frame is delivered for.
///
/// The `ocf_service` and `fsh_service` parameters of the master channel (132.0 §3.5–3.9, 732.1
/// §3.11.2.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Service {
    /// The part is not delivered.
    None,
    /// The part is delivered for the virtual channel of the frame (`VC_OCF`, `VC_FSH`).
    Vc,
    /// The part is delivered for the master channel (`MC_OCF`, `MC_FSH`), whatever the VC.
    Mc,
}

impl Service {
    /// The scope the part is delivered for, given the VC of the frame; `None` if it is not
    /// delivered.
    const fn scope<V: Copy>(self, vc: V) -> Option<ChannelScope<V>> {
        match self {
            Service::None => None,
            Service::Vc => Some(ChannelScope::VirtualChannel(vc)),
            Service::Mc => Some(ChannelScope::MasterChannel),
        }
    }
}

/// The channel an action is about: the master channel or one virtual channel (LNK-051,
/// LNK-053).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelScope<V> {
    /// The master channel (the MC Frame Count, the `MC_OCF`, the `MC_FSH`).
    MasterChannel,
    /// A virtual channel.
    VirtualChannel(V),
}

/// Why a frame was discarded (LNK-050, LNK-080; docs/04-engines.md §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiscardReason {
    /// The VC (or, for USLP, the MAP) is not configured.
    UnknownChannel,
    /// The VC is configured, but this engine does not provide its service.
    NotServed,
    /// The FECF did not match (reported by the caller).
    CrcMismatch,
    /// The AOS FHEC could not be corrected (reported by the caller).
    HeaderUncorrectable,
    /// SDLS verification failed (reported by the caller).
    Security,
}

/// The reason a caller discarded a frame before the engine (LNK-080).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallerDiscardReason {
    /// The FECF did not match.
    CrcMismatch,
    /// The AOS FHEC could not be corrected.
    HeaderUncorrectable,
    /// SDLS verification failed.
    Security,
}

impl From<CallerDiscardReason> for DiscardReason {
    fn from(reason: CallerDiscardReason) -> DiscardReason {
        match reason {
            CallerDiscardReason::CrcMismatch => DiscardReason::CrcMismatch,
            CallerDiscardReason::HeaderUncorrectable => DiscardReason::HeaderUncorrectable,
            CallerDiscardReason::Security => DiscardReason::Security,
        }
    }
}

/// An action of a `FrameReceiver` (API-057; docs/04-engines.md §4–§5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverOutput<P: ReceiveProtocol> {
    /// Frames are missing before this one: the count is ahead of the expected one (LNK-051).
    FrameGap {
        /// The MC or the VC whose count shows the gap.
        scope: ChannelScope<P::Vc>,
        /// The number of frames missing.
        missing: u64,
    },
    /// A count that repeats or goes backwards (LNK-051, OI-73).
    CountDiscontinuity {
        /// The MC or the VC whose count shows it.
        scope: ChannelScope<P::Vc>,
    },
    /// The frame was rejected (LNK-050, LNK-080). It is the last action of the frame.
    FrameDiscarded {
        /// Why.
        reason: DiscardReason,
        /// The VC of the frame, where the engine read it (not for a frame the caller discarded).
        vc: Option<P::Vc>,
        /// The MAP of the frame (USLP).
        map: Option<MapId<4>>,
    },
    /// The coding sublayer lost frames: the Frame Loss signal the caller supplied applies to the
    /// physical channel and to every VC of the receiver ([`FrameReceiver::vcs`]; LNK-053,
    /// OI-68).
    LossFlag,
    /// The data field of a frame for a VC, for its extractor, reassembler, or user (LNK-050,
    /// LNK-060, LNK-077). The octets are in the output area.
    DataField {
        /// The VC.
        vc: P::Vc,
        /// Where the data is: TM the data field, AOS the `M_PDU`, the data field, or the valid
        /// bits of a `B_PDU`, USLP the TFDF.
        data: OutRange,
        /// The bits of `data` that are valid, for a `B_PDU` (the rest of the last octet is
        /// zero; LNK-077).
        bit_count: Option<usize>,
        /// The First Header Pointer of the TM frame header, for a VC that carries packets.
        pointer: Option<u16>,
        /// A gap or discontinuity was detected for the VC since the last data field delivered
        /// for it (LNK-051, LNK-061).
        gap: bool,
        /// The Replay Flag of the AOS frame (LNK-054).
        replay: Option<bool>,
    },
    /// The OCF of a frame (LNK-053).
    Ocf {
        /// The MC or the VC the OCF belongs to.
        scope: ChannelScope<P::Vc>,
        /// The report: a Type-1 Report (CLCW or PLCW) or a Type-2 Report.
        ocf: Ocf,
        /// The Replay Flag of the AOS frame (LNK-054).
        replay: Option<bool>,
    },
    /// The Secondary Header of a TM frame, with its ID octet (LNK-053). The octets are in the
    /// output area.
    FshSdu {
        /// The MC or the VC the FSH belongs to.
        scope: ChannelScope<P::Vc>,
        /// Where the octets are.
        sdu: OutRange,
    },
    /// The Insert Zone of an AOS or USLP frame (LNK-053). The octets are in the output area.
    InsertSdu {
        /// Where the octets are.
        sdu: OutRange,
        /// The Replay Flag of the AOS frame; absent for USLP, and for a raw frame, whose header
        /// the caller rejected (LNK-054, LNK-079).
        replay: Option<bool>,
    },
    /// A TC frame, whole, for FARM-1 (LNK-081). The octets are in the output area.
    TcFrame {
        /// The VC.
        vc: P::Vc,
        /// Where the frame is.
        frame: OutRange,
    },
    /// A USLP frame with the Protocol Control Command Flag set, whole, for the COP (LNK-081). The
    /// octets are in the output area.
    ProtocolControl {
        /// The VC.
        vc: P::Vc,
        /// Where the frame is.
        frame: OutRange,
    },
}

/// A frame the caller has checked: its FECF is correct (the AOS FHEC corrected), and its SDLS
/// processing is verified (LNK-050, API-074, RS-023).
///
/// **Temporary exception to RS-023.** [`new`](Self::new) wraps any octets on the caller's word,
/// so the typestate is not enforced. The maintainer accepted this for the merge of M4 WP-5
/// (2026-11-02); the codec's `Plain` frame replaces this type under issue #232, before the C ABI
/// handles (WP-7) are built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckedFrame<'a> {
    octets: &'a [u8],
}

impl<'a> CheckedFrame<'a> {
    /// The octets of a frame whose FECF the caller has checked and whose SDLS processing it
    /// has verified (LNK-050, API-074).
    #[must_use]
    pub const fn new(octets: &'a [u8]) -> CheckedFrame<'a> {
        CheckedFrame { octets }
    }

    /// The octets.
    #[must_use]
    pub const fn octets(&self) -> &'a [u8] {
        self.octets
    }
}

/// An input of the `FrameReceiver` (docs/04-engines.md §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverInput<'a, P: ReceiveProtocol, const V: usize> {
    /// A frame the caller has checked (LNK-050).
    Frame(CheckedFrame<'a>),
    /// The octets of a frame the caller rejected, for the Insert Zone only (LNK-079).
    RawFrame(&'a [u8]),
    /// A frame the caller discarded before the engine (LNK-080).
    CallerDiscard(CallerDiscardReason),
    /// The coding sublayer reports that frames were lost (LNK-053, OI-68).
    FrameLoss,
    /// Replace the configuration (LNK-003, GEN-061): the expected counts restart; no action.
    Reconfigure(FrameReceiverConfig<P, V>),
}

/// The read-only status of a `FrameReceiver`, for housekeeping telemetry (API-056, LNK-002).
///
/// The counters persist across `Reconfigure`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReceiverStatus {
    /// Frames taken and processed, discarded ones included.
    pub frames_in: Counter,
    /// Raw frames taken (LNK-079).
    pub raw_frames_in: Counter,
    /// Inputs the receiver refused with an error (API-058).
    pub refused: Counter,
    /// `FrameLoss` inputs (LNK-053).
    pub frame_loss: Counter,
    /// Frames discarded as `UnknownChannel` (LNK-050).
    pub unknown_channel: Counter,
    /// Frames discarded as `NotServed` (LNK-050).
    pub not_served: Counter,
    /// Frames the caller discarded for a wrong FECF (LNK-080).
    pub crc_mismatch: Counter,
    /// Frames the caller discarded for an AOS FHEC it could not correct (LNK-080).
    pub header_uncorrectable: Counter,
    /// Frames the caller discarded for a failed SDLS verification (LNK-080).
    pub security: Counter,
    /// OID frames (LNK-052).
    pub oid_frames: Counter,
    /// `FrameGap` actions (LNK-051).
    pub gaps: Counter,
    /// `CountDiscontinuity` actions (LNK-051).
    pub count_discontinuities: Counter,
}

/// One virtual channel of a receiver: its identifier and the parameters of its protocol
/// (132.0 §5 Table 5-2; 232.0 §5; 732.0 §5 Table 5-2; 732.1 §5 Table 5-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VcEntry<P: ReceiveProtocol> {
    vc: P::Vc,
    params: P::VcParams,
}

impl<P: ReceiveProtocol> VcEntry<P> {
    /// The VC `vc` with `params`.
    #[must_use]
    pub const fn new(vc: P::Vc, params: P::VcParams) -> VcEntry<P> {
        VcEntry { vc, params }
    }

    /// The VC.
    #[must_use]
    pub const fn vc(&self) -> P::Vc {
        self.vc
    }

    /// The parameters of the VC.
    #[must_use]
    pub const fn params(&self) -> P::VcParams {
        self.params
    }
}

/// A protocol of the `FrameReceiver` (LNK-001, D3 of the M4 plan): the parameters of its
/// physical channel and VCs, and its procedure for one frame.
///
/// Implemented for [`Tm`](crate::protocol::Tm), [`Tc`](crate::protocol::Tc),
/// [`Aos`](crate::protocol::Aos), and [`Uslp`](crate::protocol::Uslp). The procedure is not for
/// implementation elsewhere: its context offers no operation outside this crate.
pub trait ReceiveProtocol: Protocol {
    /// The parameters of the physical channel that the receiver needs: plain data (API-041).
    type Channel: Copy + Debug + Eq;
    /// The parameters of one VC that the receiver needs: plain data (API-041).
    type VcParams: Copy + Debug + Eq;

    /// The octets of the Insert Zone, 0 for a protocol or a channel without one.
    fn insert_zone_len(channel: &Self::Channel) -> usize;

    /// Whether a frame of `frame_len` octets (with a FECF if `fecf`) leaves a data field octet
    /// on the channel, with the OCF and Secondary Header of the VC when `vc` is given
    /// (CMN-041).
    fn fits(
        frame_len: usize,
        fecf: bool,
        channel: &Self::Channel,
        vc: Option<&Self::VcParams>,
    ) -> bool;

    /// Whether the frames of the channel vary in length, so that the configured frame length is
    /// the maximum (TC, 232.0 §2.3.1; variable-length USLP, 732.1 §4.1.2.7) and not the length
    /// of every frame (LNK-050).
    fn variable_length(channel: &Self::Channel) -> bool;

    /// Refuses a frame that the receiver does not serve, before its length is checked (USLP: a
    /// truncated frame, Annex D, whose header has no Frame Length).
    ///
    /// # Errors
    /// `InvalidField` (`end_of_frame_primary_header_flag`) for a truncated USLP frame.
    fn refuse_unserved(_: &Self::Channel, _: &[u8]) -> Result<()> {
        Ok(())
    }

    /// Where the Insert Zone starts in the octets of a raw frame (LNK-079).
    ///
    /// # Errors
    /// `InvalidConfig` (`raw_frames`) for a protocol without an Insert Zone; `InvalidField`
    /// (`vcf_count_len`) for a USLP frame whose VCF Count Length is not 0: the offset would
    /// follow a field of a header the caller rejected (OI-78).
    fn raw_insert_zone_offset(channel: &Self::Channel, frame: &[u8]) -> Result<usize>;

    /// The procedure of one frame, of the frame length or, where frames vary in length, of a
    /// length up to it (LNK-050…LNK-054).
    ///
    /// # Errors
    /// The frame is parsed before any state changes: `Malformed`, `ReservedValue`,
    /// `InvalidField`, `LengthMismatch`, `BufferTooSmall`. After that only `InternalError`.
    #[doc(hidden)]
    fn receive<const V: usize>(cx: &mut FrameContext<'_, Self, V>, frame: &[u8]) -> Result<()>;
}

/// The configuration of a `FrameReceiver` (LNK-050, LNK-079; docs/04-engines.md §3.1).
///
/// Built only by [`new`](Self::new) and [`with_raw_frames`](Self::with_raw_frames), which check
/// the relations between the parameters (API-040, RS-020). `V` is the capacity for VC entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameReceiverConfig<P: ReceiveProtocol, const V: usize> {
    frame_len: usize,
    fecf: bool,
    channel: P::Channel,
    vcs: [Option<VcEntry<P>>; V],
    raw_frames: bool,
}

impl<P: ReceiveProtocol, const V: usize> FrameReceiverConfig<P, V> {
    /// The configuration of a physical channel whose frames are `frame_len` octets (at most
    /// `frame_len` where the channel's frames vary in length), with a FECF
    /// if `fecf`, serving `vcs`. It does not take raw frames (LNK-079).
    ///
    /// # Errors
    /// `InvalidConfig` (`frame_len`) if a frame of that length would have no data field octet
    /// on the channel, or with the OCF and Secondary Header of a VC, or (TC) more than the 1 024
    /// octets of the Maximum Transfer Frame Length (232.0 Tables 5-1 to 5-3); `InvalidConfig`
    /// (`vc_id`)
    /// for a VC given twice; `CapacityExceeded` (`vc_id`) for more than `V` VCs.
    pub fn new(
        frame_len: usize,
        fecf: bool,
        channel: P::Channel,
        vcs: &[VcEntry<P>],
    ) -> Result<FrameReceiverConfig<P, V>> {
        let bad_len = Error::new(ErrorKind::InvalidConfig).with_field(field::FRAME_LEN);
        if !P::fits(frame_len, fecf, &channel, None) {
            return Err(bad_len);
        }
        if vcs.len() > V {
            return Err(Error::new(ErrorKind::CapacityExceeded).with_field(field::VC_ID));
        }
        let mut table = [None; V];
        for (slot, entry) in table.iter_mut().zip(vcs) {
            if vcs.iter().filter(|other| other.vc == entry.vc).count() > 1 {
                return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::VC_ID));
            }
            if !P::fits(frame_len, fecf, &channel, Some(&entry.params)) {
                return Err(bad_len);
            }
            *slot = Some(*entry);
        }
        Ok(FrameReceiverConfig {
            frame_len,
            fecf,
            channel,
            vcs: table,
            raw_frames: false,
        })
    }

    /// The same configuration with the acceptance of raw frames set (LNK-079; default off).
    ///
    /// # Errors
    /// `InvalidConfig` (`raw_frames`) to turn it on for a protocol or a channel without an
    /// Insert Zone.
    pub fn with_raw_frames(self, on: bool) -> Result<FrameReceiverConfig<P, V>> {
        if on && P::insert_zone_len(&self.channel) == 0 {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::RAW_FRAMES));
        }
        Ok(FrameReceiverConfig {
            raw_frames: on,
            ..self
        })
    }

    /// The frame length of the physical channel.
    #[must_use]
    pub const fn frame_len(&self) -> usize {
        self.frame_len
    }

    /// Whether frames carry a FECF.
    #[must_use]
    pub const fn fecf(&self) -> bool {
        self.fecf
    }

    /// The parameters of the physical channel.
    #[must_use]
    pub const fn channel(&self) -> &P::Channel {
        &self.channel
    }

    /// The configured VCs, in the order they were given.
    pub fn vcs(&self) -> impl Iterator<Item = &VcEntry<P>> {
        self.vcs.iter().flatten()
    }

    /// Whether raw frames are accepted (LNK-079).
    #[must_use]
    pub const fn raw_frames(&self) -> bool {
        self.raw_frames
    }

    /// The output area a `Frame` needs, which is the most it writes (LNK-075): the frame
    /// length. The parts of a frame are disjoint, and a frame delivered whole (TC, USLP
    /// protocol control) is copied once, its Insert Zone being a range of that copy.
    #[must_use]
    pub const fn area_len(&self) -> usize {
        self.frame_len
    }
}

/// A configured VC and what the receiver remembers about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot<P: ReceiveProtocol> {
    entry: VcEntry<P>,
    /// The expected next VC count of the Sequence-Controlled Service (the only count of TM and
    /// AOS).
    expect_sequence: Option<u64>,
    /// The expected next VC count of the Expedited Service (USLP, Bypass Flag 1).
    expect_expedited: Option<u64>,
    /// The Cycle Use Flag of the last AOS frame counted for the VC.
    cycle: Option<bool>,
    /// A gap was detected and no data field has been delivered since (LNK-051).
    pending_gap: bool,
}

/// What a count showed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Track {
    /// The next count, or the first one.
    InSequence,
    /// This many frames are missing.
    Gap(u64),
    /// The count repeats or goes backwards.
    Discontinuity,
}

/// Compares `value` with the expected count of a counter of `bits` bits and expects its
/// successor next (CMN-031, LNK-051, OI-73). The first count sets the expectation.
///
/// The distance from the expected count is 0 for the next frame, 1 to half the range minus one
/// frames missing for a gap, and anything else (the same count again, or half the range or
/// more behind) a discontinuity: the classification of [`aldis_core::counter::ModCounter`].
fn advance(expect: &mut Option<u64>, value: u64, bits: u32) -> Track {
    let mask = u64::MAX
        .checked_shr(u64::BITS.saturating_sub(bits))
        .unwrap_or(0);
    let track = match *expect {
        None => Track::InSequence,
        Some(e) => {
            let missing = value.wrapping_sub(e) & mask;
            if missing == 0 {
                Track::InSequence
            } else if missing <= mask.checked_shr(1).unwrap_or(0) {
                Track::Gap(missing)
            } else {
                Track::Discontinuity
            }
        }
    };
    *expect = Some(value.wrapping_add(1) & mask);
    track
}

/// The context of the procedure of one frame: the configuration, the counts, the output area,
/// and the actions so far (LNK-050…LNK-054). Its operations are for the crate.
#[derive(Debug)]
pub struct FrameContext<'a, P: ReceiveProtocol, const V: usize> {
    cfg: &'a FrameReceiverConfig<P, V>,
    slots: &'a mut [Option<Slot<P>>; V],
    mc: &'a mut Option<u64>,
    status: &'a mut ReceiverStatus,
    out: &'a mut [u8],
    used: usize,
    actions: ReceiverActions<P>,
}

impl<P: ReceiveProtocol, const V: usize> FrameContext<'_, P, V> {
    /// The configuration.
    pub(crate) const fn cfg(&self) -> &FrameReceiverConfig<P, V> {
        self.cfg
    }

    /// The parameters of the configured VC `vc`, if any.
    pub(crate) fn params(&self, vc: P::Vc) -> Option<P::VcParams> {
        self.slots
            .iter()
            .flatten()
            .find(|slot| slot.entry.vc == vc)
            .map(|slot| slot.entry.params)
    }

    /// Adds an action.
    pub(crate) fn push(&mut self, action: ReceiverOutput<P>) -> Result<()> {
        self.actions.push(action)
    }

    /// Copies `octets` to the next free place of the output area.
    pub(crate) fn put(&mut self, octets: &[u8]) -> Result<OutRange> {
        self.put_with(octets, octets.len(), |_, s| s)
    }

    #[cfg(feature = "aos")]
    /// Copies the first `bits` bits of `octets` (LNK-077): the unused bits of the last octet
    /// are zero.
    pub(crate) fn put_bits(&mut self, octets: &[u8], bits: usize) -> Result<OutRange> {
        let len = bits.div_ceil(8);
        let keep = match bits % 8 {
            0 => 0xFF,
            1 => 0x80,
            2 => 0xC0,
            3 => 0xE0,
            4 => 0xF0,
            5 => 0xF8,
            6 => 0xFC,
            _ => 0xFE,
        };
        self.put_with(octets, len, move |i, s| {
            if i.saturating_add(1) == len {
                s & keep
            } else {
                s
            }
        })
    }

    fn put_with(
        &mut self,
        octets: &[u8],
        len: usize,
        map: impl Fn(usize, u8) -> u8,
    ) -> Result<OutRange> {
        let start = self.used;
        let end = start.saturating_add(len);
        let dst = self
            .out
            .get_mut(start..end)
            .ok_or(Error::buffer_too_small(end))?;
        for (i, (d, s)) in dst.iter_mut().zip(octets).enumerate() {
            *d = map(i, *s);
        }
        self.used = end;
        Ok(OutRange::new(start, len))
    }

    /// The effect of a count: a `FrameGap` or `CountDiscontinuity` action (LNK-051). Returns
    /// whether one was raised.
    fn report(&mut self, track: Track, scope: ChannelScope<P::Vc>) -> Result<bool> {
        match track {
            Track::InSequence => Ok(false),
            Track::Gap(missing) => {
                self.status.gaps.incr();
                self.push(ReceiverOutput::FrameGap { scope, missing })?;
                Ok(true)
            }
            Track::Discontinuity => {
                self.status.count_discontinuities.incr();
                self.push(ReceiverOutput::CountDiscontinuity { scope })?;
                Ok(true)
            }
        }
    }

    /// Counts the MC Frame Count of a frame (LNK-051): a `FrameGap` or `CountDiscontinuity`
    /// of the master channel.
    pub(crate) fn count_mc(&mut self, value: u64, bits: u32) -> Result<()> {
        let track = advance(self.mc, value, bits);
        self.report(track, ChannelScope::MasterChannel).map(|_| ())
    }

    /// Counts the VC Frame Count `value` of the configured VC `vc` for the Expedited Service if
    /// `expedited`, else for the Sequence-Controlled Service (LNK-051). Returns whether it
    /// raised an action. A VC that is not configured has no count.
    pub(crate) fn count_vc(
        &mut self,
        vc: P::Vc,
        expedited: bool,
        value: u64,
        bits: u32,
    ) -> Result<bool> {
        let Some(slot) = self.slots.iter_mut().flatten().find(|s| s.entry.vc == vc) else {
            return Ok(false);
        };
        let expect = if expedited {
            &mut slot.expect_expedited
        } else {
            &mut slot.expect_sequence
        };
        let track = advance(expect, value, bits);
        self.report(track, ChannelScope::VirtualChannel(vc))
    }

    #[cfg(feature = "aos")]
    /// Reports a `CountDiscontinuity` of the VC without a count (LNK-051).
    pub(crate) fn discontinuity(&mut self, vc: P::Vc) -> Result<()> {
        self.report(Track::Discontinuity, ChannelScope::VirtualChannel(vc))
            .map(|_| ())
    }

    #[cfg(feature = "aos")]
    /// Records that the Cycle Use Flag of the AOS frame is `flag`, and whether it differs from
    /// the last frame of the VC counted (OI-78). The first frame sets it.
    pub(crate) fn change_cycle_use(&mut self, vc: P::Vc, flag: bool) -> bool {
        let Some(slot) = self.slots.iter_mut().flatten().find(|s| s.entry.vc == vc) else {
            return false;
        };
        let changed = slot.cycle.is_some_and(|last| last != flag);
        slot.cycle = Some(flag);
        if changed {
            slot.expect_sequence = None;
        }
        changed
    }

    /// Flags the next data field delivered for the VC with a gap (LNK-051).
    pub(crate) fn mark_gap(&mut self, vc: P::Vc) {
        if let Some(slot) = self.slots.iter_mut().flatten().find(|s| s.entry.vc == vc) {
            slot.pending_gap = true;
        }
    }

    /// Whether the data field about to be delivered for the VC carries a gap flag; clears it
    /// (LNK-051).
    pub(crate) fn take_gap(&mut self, vc: P::Vc) -> bool {
        self.slots
            .iter_mut()
            .flatten()
            .find(|s| s.entry.vc == vc)
            .is_some_and(|slot| core::mem::take(&mut slot.pending_gap))
    }

    /// Counts an OID frame (LNK-052).
    pub(crate) const fn count_oid(&mut self) {
        self.status.oid_frames.incr();
    }

    /// Discards the frame: counts it under `reason` and returns `FrameDiscarded` (LNK-050).
    pub(crate) fn discard(
        &mut self,
        reason: DiscardReason,
        vc: Option<P::Vc>,
        map: Option<MapId<4>>,
    ) -> Result<()> {
        discard_counter(self.status, reason).incr();
        self.push(ReceiverOutput::FrameDiscarded { reason, vc, map })
    }

    /// Delivers the FSH (LNK-053).
    pub(crate) fn deliver_fsh(
        &mut self,
        scope: Option<ChannelScope<P::Vc>>,
        octets: Option<&[u8]>,
    ) -> Result<()> {
        if let (Some(scope), Some(octets)) = (scope, octets) {
            let sdu = self.put(octets)?;
            self.push(ReceiverOutput::FshSdu { scope, sdu })?;
        }
        Ok(())
    }

    /// Delivers the Insert Zone (LNK-053, LNK-054); nothing for a channel without one.
    pub(crate) fn deliver_insert(&mut self, octets: &[u8], replay: Option<bool>) -> Result<()> {
        if !octets.is_empty() {
            let sdu = self.put(octets)?;
            self.push(ReceiverOutput::InsertSdu { sdu, replay })?;
        }
        Ok(())
    }

    /// Delivers an Insert Zone that is already in the output area (a range of the whole frame,
    /// LNK-075); nothing for a channel without one.
    pub(crate) fn deliver_insert_range(
        &mut self,
        sdu: OutRange,
        replay: Option<bool>,
    ) -> Result<()> {
        if sdu.is_empty() {
            Ok(())
        } else {
            self.push(ReceiverOutput::InsertSdu { sdu, replay })
        }
    }

    /// Delivers the OCF (LNK-053, LNK-054).
    pub(crate) fn deliver_ocf(
        &mut self,
        scope: Option<ChannelScope<P::Vc>>,
        ocf: Option<Ocf>,
        replay: Option<bool>,
    ) -> Result<()> {
        if let (Some(scope), Some(ocf)) = (scope, ocf) {
            self.push(ReceiverOutput::Ocf { scope, ocf, replay })?;
        }
        Ok(())
    }
}

/// The counter of a discard reason.
const fn discard_counter(status: &mut ReceiverStatus, reason: DiscardReason) -> &mut Counter {
    match reason {
        DiscardReason::UnknownChannel => &mut status.unknown_channel,
        DiscardReason::NotServed => &mut status.not_served,
        DiscardReason::CrcMismatch => &mut status.crc_mismatch,
        DiscardReason::HeaderUncorrectable => &mut status.header_uncorrectable,
        DiscardReason::Security => &mut status.security,
    }
}

/// The first `N` octets of `frame`.
///
/// # Errors
/// `LengthMismatch` (`frame_len`) for a shorter frame: the configuration keeps the frame length
/// above the header, so this is the frame of a caller that did not check its length.
pub(crate) fn header<const N: usize>(frame: &[u8]) -> Result<[u8; N]> {
    frame
        .first_chunk::<N>()
        .copied()
        .ok_or(Error::new(ErrorKind::LengthMismatch).with_field(field::FRAME_LEN))
}

/// The VC of protocol `P` whose identifier is `raw`.
///
/// # Errors
/// `OutOfRange` (`vc_id`) if `raw` needs more bits than the identifier has: the callers mask
/// the field they read, so this is not reachable from a frame.
pub(crate) fn vc_of<P: Protocol>(raw: u8) -> Result<P::Vc> {
    P::Vc::try_from(raw)
}

/// The MAP ID `raw` (USLP: four bits).
///
/// # Errors
/// `OutOfRange` (`map_id`) as for [`vc_of`].
pub(crate) fn map_of(raw: u8) -> Result<MapId<4>> {
    MapId::<4>::try_from(raw)
}

/// The frame laid out with its parts, for the slices of the fields (CMN-040).
///
/// # Errors
/// `Malformed` (`frame_data_field`) if the other fields leave no data octet (CMN-041); the
/// frame is then not one the channel's codec accepts.
pub(crate) fn lay_out(frame: &[u8], spec: LayoutSpec) -> Result<FramedBytes<'_>> {
    let layout = FrameLayout::compute(&spec)
        .map_err(|_| Error::new(ErrorKind::Malformed).with_field(field::FRAME_DATA_FIELD))?;
    FramedBytes::new(frame, layout)
}

/// The layout of a frame of `frame_len` octets with the given fields and no SDLS.
pub(crate) const fn spec(
    frame_len: usize,
    primary_header_len: usize,
    insert_or_fsh_len: usize,
    ocf: bool,
    fecf: bool,
) -> LayoutSpec {
    LayoutSpec {
        frame_len,
        primary_header_len,
        secondary_len: insert_or_fsh_len,
        security: SecurityLayout::NONE,
        ocf,
        fecf,
    }
}

/// The OCF in the octets of a frame's OCF part, `None` if the frame has none.
pub(crate) fn ocf_of(part: &[u8]) -> Option<Ocf> {
    part.first_chunk::<4>().map(|&octets| Ocf::decode(octets))
}

/// The `FrameReceiver` of one master channel (LNK-050…LNK-054).
///
/// `V` is the capacity for VC entries (GEN-011).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameReceiver<P: ReceiveProtocol, const V: usize> {
    config: FrameReceiverConfig<P, V>,
    slots: [Option<Slot<P>>; V],
    mc: Option<u64>,
    status: ReceiverStatus,
}

impl<P: ReceiveProtocol, const V: usize> FrameReceiver<P, V> {
    /// A receiver that has seen no frame: the first count of each channel sets its expectation
    /// (LNK-051).
    #[must_use]
    pub fn new(config: FrameReceiverConfig<P, V>) -> FrameReceiver<P, V> {
        FrameReceiver {
            slots: FrameReceiver::<P, V>::slots_of(&config),
            config,
            mc: None,
            status: ReceiverStatus::default(),
        }
    }

    fn slots_of(config: &FrameReceiverConfig<P, V>) -> [Option<Slot<P>>; V] {
        let mut slots = [None; V];
        for (slot, entry) in slots.iter_mut().zip(config.vcs.iter().flatten()) {
            *slot = Some(Slot {
                entry: *entry,
                expect_sequence: None,
                expect_expedited: None,
                cycle: None,
                pending_gap: false,
            });
        }
        slots
    }

    /// The configuration in force.
    #[must_use]
    pub const fn config(&self) -> &FrameReceiverConfig<P, V> {
        &self.config
    }

    /// The status snapshot: counters (API-056, LNK-002).
    #[must_use]
    pub const fn status(&self) -> &ReceiverStatus {
        &self.status
    }

    /// The configured VCs, in the order of the configuration: the VCs a `LossFlag` applies to
    /// (LNK-053).
    pub fn vcs(&self) -> impl Iterator<Item = P::Vc> {
        self.config.vcs().map(VcEntry::vc)
    }

    /// Runs one input (API-057). The receiver has no timer (API-053).
    ///
    /// An input that fails leaves the receiver as it was, except for the `refused` counter, and
    /// writes no action (API-058).
    ///
    /// # Errors
    /// - `Frame`: `LengthMismatch` (`frame_len`) for a frame of another length than the
    ///   channel's (above the maximum, or below the shortest frame, or not the length its
    ///   header gives, where frames vary in length); `BufferTooSmall` if `out` is shorter than
    ///   the frame length (LNK-075); `Malformed` (`frame_data_field`, `bitstream_data_pointer`)
    ///   for a frame whose fields leave no data field or point beyond it; `InvalidField`
    ///   (`bitstream_data_pointer`) for a pointer at the last bit of the zone; `ReservedValue`
    ///   (`protocol_control_command_flag`) for a USLP frame with Bypass 0 and Protocol Control
    ///   Command 1 (732.1 Table 4-1); `InvalidField` (`ocf_flag`, `fsh_flag`) or
    ///   `LengthMismatch` (`fsh_len`) for a TM frame whose OCF or Secondary Header differs from
    ///   the configuration of its VC (132.0 §4.1.2.4.3, §4.1.2.7.2.3).
    /// - `RawFrame`: `InvalidConfig` (`raw_frames`) while the option is off; `LengthMismatch`
    ///   (`frame_len`) for another length than the channel's; `InvalidField` (`vcf_count_len`)
    ///   for a USLP frame whose VCF Count Length is not 0; `BufferTooSmall`.
    /// - `Reconfigure`: none.
    pub fn step(
        &mut self,
        input: ReceiverInput<'_, P, V>,
        out: &mut [u8],
    ) -> Result<ReceiverActions<P>> {
        let result = self.run(input, out);
        if result.is_err() {
            self.status.refused.incr();
        }
        result
    }

    fn run(
        &mut self,
        input: ReceiverInput<'_, P, V>,
        out: &mut [u8],
    ) -> Result<ReceiverActions<P>> {
        match input {
            ReceiverInput::Frame(frame) => self.frame(frame.octets(), out),
            ReceiverInput::RawFrame(octets) => self.raw_frame(octets, out),
            ReceiverInput::CallerDiscard(reason) => {
                let reason = DiscardReason::from(reason);
                discard_counter(&mut self.status, reason).incr();
                let mut actions = Actions::new();
                actions.push(ReceiverOutput::FrameDiscarded {
                    reason,
                    vc: None,
                    map: None,
                })?;
                Ok(actions)
            }
            ReceiverInput::FrameLoss => {
                self.status.frame_loss.incr();
                let mut actions = Actions::new();
                actions.push(ReceiverOutput::LossFlag)?;
                Ok(actions)
            }
            ReceiverInput::Reconfigure(config) => {
                self.slots = FrameReceiver::<P, V>::slots_of(&config);
                self.config = config;
                self.mc = None;
                Ok(Actions::new())
            }
        }
    }

    /// The frame length and the output area are checked before the frame is read (API-058).
    fn check_frame(&self, octets: &[u8], out: &[u8]) -> Result<()> {
        let config = &self.config;
        P::refuse_unserved(&config.channel, octets)?;
        let len = octets.len();
        let ok = if P::variable_length(&config.channel) {
            len <= config.frame_len && P::fits(len, config.fecf, &config.channel, None)
        } else {
            len == config.frame_len
        };
        if !ok {
            return Err(Error::new(ErrorKind::LengthMismatch).with_field(field::FRAME_LEN));
        }
        let needed = self.config.area_len();
        if out.len() < needed {
            return Err(Error::buffer_too_small(needed));
        }
        Ok(())
    }

    fn frame(&mut self, octets: &[u8], out: &mut [u8]) -> Result<ReceiverActions<P>> {
        self.check_frame(octets, out)?;
        let mut cx = FrameContext {
            cfg: &self.config,
            slots: &mut self.slots,
            mc: &mut self.mc,
            status: &mut self.status,
            out,
            used: 0,
            actions: Actions::new(),
        };
        P::receive(&mut cx, octets)?;
        let actions = cx.actions;
        self.status.frames_in.incr();
        Ok(actions)
    }

    /// LNK-079: the Insert Zone of a frame the caller rejected, at its fixed offset.
    fn raw_frame(&mut self, octets: &[u8], out: &mut [u8]) -> Result<ReceiverActions<P>> {
        if !self.config.raw_frames {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::RAW_FRAMES));
        }
        self.check_frame(octets, out)?;
        let len = P::insert_zone_len(&self.config.channel);
        let start = P::raw_insert_zone_offset(&self.config.channel, octets)?;
        let sdu = octets
            .get(start..start.saturating_add(len))
            .ok_or(Error::new(ErrorKind::LengthMismatch).with_field(field::INSERT_ZONE_LEN))?;
        let range = OutRange::new(0, len);
        for (d, s) in out.iter_mut().zip(sdu) {
            *d = *s;
        }
        self.status.raw_frames_in.incr();
        let mut actions = Actions::new();
        actions.push(ReceiverOutput::InsertSdu {
            sdu: range,
            replay: None,
        })?;
        Ok(actions)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{
        FrameContext, FrameReceiver, FrameReceiverConfig, ReceiverStatus, Track, VcEntry, advance,
        header, lay_out, map_of, ocf_of, spec, vc_of,
    };
    use crate::protocol::{Tc, Tm};
    use crate::receiver::{TcChannel, TcVc};
    use aldis_core::counter::{Continuity, ModCounter};
    use aldis_core::{ErrorKind, field};

    #[test]
    fn counts_classify_as_the_core_counter_does() {
        // req: LNK-051, CMN-031
        for prev in 0..=255_u64 {
            for next in 0..=255_u64 {
                let mut expect = Some((prev + 1) & 0xFF);
                let want = match ModCounter::new(8, prev).unwrap().classify(next).unwrap() {
                    Continuity::InSequence => Track::InSequence,
                    Continuity::Gap { missing } => Track::Gap(missing),
                    Continuity::Repeat | Continuity::Backwards => Track::Discontinuity,
                };
                assert_eq!(advance(&mut expect, next, 8), want);
                assert_eq!(expect, Some((next + 1) & 0xFF));
            }
        }
        let mut first = None;
        assert_eq!(advance(&mut first, 9, 56), Track::InSequence);
        assert_eq!(first, Some(10));
    }

    #[test]
    fn the_helpers_refuse_what_the_configuration_keeps_out() {
        // req: GEN-020, RS-020
        assert_eq!(
            header::<7>(&[0; 6]).unwrap_err().kind(),
            ErrorKind::LengthMismatch
        );
        assert_eq!(header::<2>(&[1, 2, 3]), Ok([1, 2]));
        assert_eq!(vc_of::<Tm>(8).unwrap_err().field(), Some(field::VC_ID));
        assert_eq!(map_of(16).unwrap_err().field(), Some(field::MAP_ID));
        assert!(ocf_of(&[]).is_none() && ocf_of(&[0; 4]).is_some());
        let e = lay_out(&[0; 8], spec(8, 6, 2, true, true)).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::Malformed);
        assert!(lay_out(&[0; 8], spec(7, 6, 0, false, false)).is_err());
    }

    #[test]
    fn a_vc_that_is_not_configured_has_no_count_and_no_flag() {
        // req: LNK-051
        let config = FrameReceiverConfig::<Tc, 2>::new(
            16,
            false,
            TcChannel,
            &[VcEntry::new(vc_of::<Tc>(1).unwrap(), TcVc::SERVED)],
        )
        .unwrap();
        let mut rx = FrameReceiver::<Tc, 2>::new(config);
        let mut out = [0u8; 4];
        let mut cx = FrameContext {
            cfg: &rx.config,
            slots: &mut rx.slots,
            mc: &mut rx.mc,
            status: &mut rx.status,
            out: &mut out,
            used: 0,
            actions: super::Actions::new(),
        };
        let other = vc_of::<Tc>(2).unwrap();
        assert_eq!(cx.count_vc(other, false, 5, 8), Ok(false));
        #[cfg(feature = "aos")]
        assert!(!cx.change_cycle_use(other, true));
        cx.mark_gap(other);
        assert!(!cx.take_gap(other));
        assert_eq!(
            cx.put(&[0; 5]).unwrap_err().kind(),
            ErrorKind::BufferTooSmall
        );
        assert_eq!(ReceiverStatus::default().frames_in.get(), 0);
    }
}
