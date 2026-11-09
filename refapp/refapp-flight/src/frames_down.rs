//! The downlink at frame level, A2 WP-3a.
//!
//! Specification: docs/reference-application.md §2.1 step 6, §4.1, §4.4: one `PacketGenerator`
//! for VC 0 and one for VC 1, the `ChannelMux` with fixed priority and OID frames on VC 7, the OCF
//! slot, and the FECF.
//!
//! Requirements: APP-026, APP-030…APP-035, APP-090, APP-132, APP-134, APP-135.
//!
//! The packet side is the A1 code: [`crate::downlink::Downlink`] builds each packet and reports
//! `packet_out` and then a link unit. At frame level the link unit goes to the generator of the
//! packet's VC instead of out, through [`Tap`]; the frame is the only unit the node outputs
//! (APP-090).
//!
//! **Input of the CLCW (for the uplink, WP-3b).** [`FlightNode::set_clcw`](crate::FlightNode::set_clcw)
//! takes the CLCW that the OCF of the next frames carries; it is `01 00 00 00`
//! (`generated::tm::OCF_INITIAL`) until the uplink sets one. The uplink calls it with
//! `Farm1::clcw` after the frames delivered in step 1 (§4.4.1, APP-134).

use aldis::core::crc::write_fecf;
use aldis::core::engine::Actions;
use aldis::core::ids::{SpacecraftId, VcId};
use aldis::core::{Error, ErrorKind, Result};
use aldis::link::common::{IdlePacketKind, IdlePolicy, ValidPvns};
use aldis::link::generator::{
    GeneratorInput, GeneratorOutput, PacketGenerator, PacketGeneratorConfig,
};
use aldis::link::mux::{
    ChannelMux, ChannelMuxConfig, FrameClass, MuxInput, MuxOutput, Offer, Scope, TmChannel, TmRole,
    TmVc, VcConfig,
};
use aldis::link::policy::MuxPolicy;
use aldis::link::protocol::{Tm, TmParams};
use aldis::tc::Clcw;
use refapp_scenario::generated::{SPACECRAFT_ID, tm};
use refapp_scenario::{Duration, Instant, Output};

use crate::FlightNode;
use crate::downlink::Stream;
use crate::dump::{Dump, Segment};

/// Code of the application event `frame_rejected` (§7.4, APP-131): the argument is the error code
/// of the catalogue in docs/03-api-model.md §9.
pub const APP_FRAME_REJECTED: u16 = 102;

/// Octets of a transfer frame: `generated::tm::FRAME_LENGTH` (APP-030); checked by the engines'
/// capacity tests at construction and by a unit test.
const FRAME: usize = 223;

/// Octets of the data field: the frame less the primary header (6), the OCF (4), and the FECF
/// (2) (§4.1, CMN-040).
const DATA_FIELD: usize = 211;

/// Octets of a generator's packet queue, behind the field: one Maximum Packet Length
/// (`generated::tm::MAX_PACKET_LENGTH`, 512). The engine needs at least 301 (the maximum packet
/// less the packet zone); VC 1 in `a2-nominal` needs between 441 and 450 (the dump and payload
/// packets of 12.0 to 12.5 s wait behind VC 0, §4.4.4), measured by lowering the constant until
/// the unit tests failed. A generator that is out of room refuses a packet with `WouldBlock`,
/// which the node reports as `frame_rejected`.
const QUEUE: usize = 512;

/// The virtual channels of data, in the order the node serves them (§2.1 step 6).
const DATA_VCS: [u8; 2] = [tm::VCID_REALTIME, tm::VCID_PAYLOAD];

type Generator = PacketGenerator<Tm, DATA_FIELD, QUEUE>;
type Mux = ChannelMux<Tm, 3, FRAME>;

/// The `vc_priority` of the library for a VC of the rank `rank` of the scenario, where rank 1 is
/// served first (§4.1, §4.4.3). The multiplexer serves the offered VC with the *highest*
/// `vc_priority` (LNK-031), so the rank is inverted.
const fn mux_priority(rank: u8) -> u8 {
    u8::MAX.saturating_sub(rank)
}

/// The VC of the packets of `stream` (§4.1: VC 0 carries housekeeping and events, VC 1 payload and
/// dump).
const fn vc_of(stream: Stream) -> u8 {
    match stream {
        Stream::Housekeeping | Stream::Events => tm::VCID_REALTIME,
        Stream::Payload | Stream::Dump => tm::VCID_PAYLOAD,
    }
}

/// Succeeds if an input that the engine answers with no action did so; an action here means the
/// engine and the node disagree (`InternalError`, API-083).
fn none<A: Copy, const N: usize>(actions: Actions<A, N>) -> Result<()> {
    if actions.is_empty() {
        Ok(())
    } else {
        Err(Error::new(ErrorKind::InternalError))
    }
}

/// The generator of a VC with the release deadline of §4.1 and the idle fill of §4.4.3.
fn generator(vc: VcId<3>, deadline_us: u64) -> Result<Generator> {
    let config = PacketGeneratorConfig::<Tm>::new(
        vc,
        TmParams,
        DATA_FIELD,
        ValidPvns::SPACE,
        usize::from(tm::MAX_PACKET_LENGTH),
    )?
    .with_idle(
        IdlePolicy::Oid,
        IdlePacketKind::SpacePacket,
        tm::IDLE_PATTERN,
    )?
    .with_release_deadline(Duration::from_micros(deadline_us));
    Generator::new(config)
}

/// The multiplexer: one master channel with the SCID of §3.4 and the OCF in every frame, VCs 0, 1,
/// and 7 carrying Packets, fixed priority with VC 0 first, and VC 7 for OID frames (§4.1, §4.4.3).
fn mux() -> Result<Mux> {
    let scid =
        SpacecraftId::<10>::new(SPACECRAFT_ID).ok_or(Error::new(ErrorKind::InvalidConfig))?;
    let mut channel = TmChannel::new(scid, VcId::<3>::from_const::<{ tm::VCID_IDLE }>());
    channel.mc_ocf = Some(tm::OCF_INITIAL.to_be_bytes());
    let vc = |id: VcId<3>| VcConfig::<Tm>::new(id, TmVc::new(TmRole::Packets));
    let vcs = [
        vc(VcId::<3>::from_const::<{ tm::VCID_REALTIME }>())
            .with_priority(mux_priority(tm::PRIORITY_REALTIME)),
        vc(VcId::<3>::from_const::<{ tm::VCID_PAYLOAD }>())
            .with_priority(mux_priority(tm::PRIORITY_PAYLOAD)),
        vc(VcId::<3>::from_const::<{ tm::VCID_IDLE }>()),
    ];
    let config = ChannelMuxConfig::<Tm, 3>::new(
        channel,
        usize::from(tm::FRAME_LENGTH),
        tm::FECF,
        MuxPolicy::FixedPriority,
        &vcs,
    )?;
    Mux::new(config)
}

/// The frame level of the flight node's downlink (A2 WP-3a).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameDownlink {
    /// The generators of VC 0 and VC 1, in the order of [`DATA_VCS`].
    generators: [Generator; 2],
    /// Whether the multiplexer holds a field of the VC of the same index.
    held: [bool; 2],
    mux: Mux,
    /// The OCF of the next frames (APP-134).
    ocf: [u8; 4],
}

impl FrameDownlink {
    /// The downlink in its start-of-run state: every frame count at 0 (APP-035), the OCF
    /// `OCF_INITIAL` (§4.4.1), no field started.
    ///
    /// # Errors
    /// The engines' `InvalidConfig` or `CapacityExceeded`, which the scenario's values rule out.
    pub fn new() -> Result<FrameDownlink> {
        Ok(FrameDownlink {
            generators: [
                generator(
                    VcId::<3>::from_const::<{ tm::VCID_REALTIME }>(),
                    tm::RELEASE_DEADLINE_REALTIME_US,
                )?,
                generator(
                    VcId::<3>::from_const::<{ tm::VCID_PAYLOAD }>(),
                    tm::RELEASE_DEADLINE_PAYLOAD_US,
                )?,
            ],
            held: [false; 2],
            mux: mux()?,
            ocf: tm::OCF_INITIAL.to_be_bytes(),
        })
    }

    /// Submits a packet to the generator of `vc` at the instant it was generated, which starts a
    /// field if none is started (LNK-012, §2.1).
    fn submit(&mut self, vc: u8, packet: &[u8], now: Instant) -> Result<()> {
        let generator = self
            .generators
            .iter_mut()
            .zip(DATA_VCS)
            .find_map(|(g, id)| (id == vc).then_some(g))
            .ok_or(Error::new(ErrorKind::UnknownChannel))?;
        none(generator.step(GeneratorInput::SubmitPacket(packet), now, &mut [])?)
    }

    /// The earliest release deadline of the generators (APP-135).
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        self.generators
            .iter()
            .filter_map(PacketGenerator::next_deadline)
            .min()
    }

    /// Step 6 of §2.1 at a slot instant (APP-132): signals the deadlines, moves the field of each
    /// data VC without one in the multiplexer into it, sets the OCF slot (APP-134), takes the
    /// frame, and outputs it as the one unit of the step.
    ///
    /// # Errors
    /// The error of an engine, which the sizes of §4.1 rule out; no frame is output.
    pub fn slot(&mut self, now: Instant, out: &mut dyn Output) -> Result<()> {
        let mut field = [0_u8; DATA_FIELD];
        for ((generator, held), vc) in self
            .generators
            .iter_mut()
            .zip(self.held.iter_mut())
            .zip(DATA_VCS)
        {
            if generator.next_deadline().is_some_and(|due| due <= now) {
                none(generator.step(GeneratorInput::Timeout, now, &mut field)?)?;
            }
            if *held {
                continue;
            }
            let actions = generator.step(GeneratorInput::Release, now, &mut field)?;
            match actions.get(0) {
                Some(GeneratorOutput::DataField(data)) => {
                    let octets = data
                        .field
                        .slice(&field)
                        .ok_or(Error::new(ErrorKind::InternalError))?;
                    none(self.mux.step(
                        MuxInput::Offer(Offer {
                            vc,
                            field: octets,
                            pointer: Some(data.pointer),
                            route: None,
                            cop: None,
                        }),
                        &mut [],
                    )?)?;
                    *held = true;
                }
                Some(GeneratorOutput::Discarded { .. }) | None => {}
            }
        }
        let mut frame = [0_u8; FRAME];
        none(self.mux.step(
            MuxInput::SetOcf {
                scope: Scope::Mc,
                ocf: self.ocf,
            },
            &mut [],
        )?)?;
        let actions = self.mux.step(MuxInput::Opportunity, &mut frame)?;
        let Some(MuxOutput::Transmit(sent)) = actions.get(0) else {
            return Err(Error::new(ErrorKind::InternalError));
        };
        if sent.class == FrameClass::Data {
            for (held, vc) in self.held.iter_mut().zip(DATA_VCS) {
                if vc == sent.vc {
                    *held = false;
                }
            }
        }
        let octets = frame
            .get_mut(sent.frame.start()..sent.frame.end())
            .ok_or(Error::new(ErrorKind::InternalError))?;
        write_fecf(octets)?;
        out.unit(octets);
        Ok(())
    }
}

/// An [`Output`] that routes the link units of packets to the generator of their VC instead of out,
/// and passes every other output on (A2: the packet is no longer a unit).
///
/// A tap without a [`FrameDownlink`] is the A1 behaviour: the unit goes out.
pub(crate) struct Tap<'a> {
    frames: Option<&'a mut FrameDownlink>,
    vc: u8,
    at: Instant,
    out: &'a mut dyn Output,
    refused: Option<ErrorKind>,
}

impl<'a> Tap<'a> {
    /// A tap for packets of `vc` generated at `at`.
    pub(crate) fn new(
        frames: Option<&'a mut FrameDownlink>,
        vc: u8,
        at: Instant,
        out: &'a mut dyn Output,
    ) -> Tap<'a> {
        Tap {
            frames,
            vc,
            at,
            out,
            refused: None,
        }
    }

    /// The error of the first packet a generator refused, if any.
    pub(crate) const fn refused(&self) -> Option<ErrorKind> {
        self.refused
    }
}

impl Output for Tap<'_> {
    fn unit(&mut self, octets: &[u8]) {
        match self.frames.as_deref_mut() {
            Some(frames) => {
                if let Err(e) = frames.submit(self.vc, octets, self.at) {
                    self.refused.get_or_insert(e.kind());
                }
            }
            None => self.out.unit(octets),
        }
    }

    fn packet_out(&mut self, apid: aldis::core::ids::Apid, octets: &[u8]) {
        self.out.packet_out(apid, octets);
    }

    fn packet_in(&mut self, apid: aldis::core::ids::Apid, octets: &[u8]) {
        self.out.packet_in(apid, octets);
    }

    fn app_event(&mut self, code: u16, args: &[u64]) {
        self.out.app_event(code, args);
    }
}

impl FlightNode {
    /// A flight node whose downlink unit is the TM frame (A2, §6.1): one frame at every step with
    /// `tx_slot` and none otherwise (APP-090). [`FlightNode::new`] is the packet level of A1.
    ///
    /// # Errors
    /// The engines' configuration errors, which the scenario's values rule out.
    pub fn new_frames() -> Result<FlightNode> {
        Ok(FlightNode {
            frames: Some(FrameDownlink::new()?),
            ..FlightNode::default()
        })
    }

    /// Sets the CLCW that the OCF of the next downlink frames carries (APP-031, APP-134). The
    /// uplink calls it with `Farm1::clcw` after the frames delivered at the instant (§4.4.1). A
    /// node at packet level has no OCF and ignores it.
    pub const fn set_clcw(&mut self, clcw: &Clcw) {
        if let Some(frames) = self.frames.as_mut() {
            frames.ocf = clcw.to_octets();
        }
    }

    /// The tap for a packet of `stream` generated at `at`.
    pub(crate) fn tap<'a>(
        frames: Option<&'a mut FrameDownlink>,
        stream: Stream,
        at: Instant,
        out: &'a mut dyn Output,
    ) -> Tap<'a> {
        Tap::new(frames, vc_of(stream), at, out)
    }

    /// Sends the next packet of `dump` through the tap of the dump's VC; a generator that refuses
    /// the packet is reported as `frame_rejected` (the packet itself was built).
    pub(crate) fn send_dump_packet(
        &mut self,
        dump: &mut Dump,
        out: &mut dyn Output,
    ) -> Result<Segment> {
        let (sent, refused) = {
            let mut tap = Tap::new(self.frames.as_mut(), tm::VCID_PAYLOAD, dump.due(), out);
            let sent = dump.send(&mut self.downlink, &mut tap);
            (sent, tap.refused())
        };
        if let Some(kind) = refused {
            self.frame_refused(kind, out);
        }
        sent
    }

    /// A generator refused a packet that was built: counted, and raised as `frame_rejected` with
    /// the error code (APP-131).
    pub(crate) fn frame_refused(&mut self, kind: ErrorKind, out: &mut dyn Output) {
        self.build_failures = self.build_failures.saturating_add(1);
        out.app_event(APP_FRAME_REJECTED, &[u64::from(kind.code())]);
    }

    /// Step 6 of §2.1 (APP-132): in a step with `tx_slot`, the frame; otherwise nothing.
    pub(crate) fn downlink_frame(&mut self, tx_slot: bool, now: Instant, out: &mut dyn Output) {
        if !tx_slot {
            return;
        }
        if let Some(Err(e)) = self.frames.as_mut().map(|f| f.slot(now, out)) {
            self.frame_refused(e.kind(), out);
        }
    }
}
