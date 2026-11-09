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
use crate::event::APP_DOWNLINK_REFUSED;

/// Octets of a transfer frame: `generated::tm::FRAME_LENGTH` (APP-030), asserted below.
const FRAME: usize = 223;

/// Octets of the data field: the frame less the primary header (6), the OCF (4), and the FECF
/// (2) (§4.1, CMN-040).
const DATA_FIELD: usize = 211;

#[expect(
    clippy::as_conversions,
    reason = "const context: `usize::from(u16)` is not const, and the widening cannot truncate"
)]
const _: () = {
    assert!(FRAME == tm::FRAME_LENGTH as usize);
    assert!(DATA_FIELD + 6 + 4 + 2 == FRAME);
};

/// Octets of a generator's packet queue, behind the field: one Maximum Packet Length
/// (`generated::tm::MAX_PACKET_LENGTH`, 512); the engine needs at least 301 (that maximum less
/// the 211 of the packet zone). The largest backlog of `a2-nominal` is on VC 1 at 12.5 s (§4.4.4):
/// the dump packets (140, 140, 140, 140, 106 octets) and payload packet 6 (412) make 1 078 octets
/// submitted, the fields of frames 97 and 99 (2 × 211) have left the generator, and 211 octets
/// are in the field, so 1 078 − 422 − 211 = 445 octets wait in the queue (the unit tests agree: a
/// queue of 440 fails them and one of 450 passes). 512 leaves 67 octets of margin. A generator
/// out of room refuses a packet with `WouldBlock`, which the node reports as `downlink_refused`
/// (APP-142).
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
pub(crate) const fn vc_of(stream: Stream) -> u8 {
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
            // The field leaves the generator before the multiplexer takes it. `held` mirrors the
            // multiplexer's slot of the VC, so the offer finds it free (the only `WouldBlock` of
            // `Offer`, LNK-076), and the VC and field length are those the multiplexer was
            // configured with, so the other refusals of `Offer` cannot arise.
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
    /// the packet is reported (APP-142; the packet itself was built).
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
            self.packet_refused(kind, tm::VCID_PAYLOAD, out);
        }
        sent
    }

    /// A generator refused a packet that was built: counted, and raised as `downlink_refused`
    /// with the error code and the VCID (APP-142).
    pub(crate) fn packet_refused(&mut self, kind: ErrorKind, vc: u8, out: &mut dyn Output) {
        self.build_failures = self.build_failures.saturating_add(1);
        out.app_event(
            APP_DOWNLINK_REFUSED,
            &[u64::from(kind.code()), u64::from(vc)],
        );
    }

    /// An engine failed in a slot: counted, and raised as `downlink_refused` with the error code
    /// alone; no frame was output (APP-142).
    pub(crate) fn slot_failed(&mut self, kind: ErrorKind, out: &mut dyn Output) {
        self.build_failures = self.build_failures.saturating_add(1);
        out.app_event(APP_DOWNLINK_REFUSED, &[u64::from(kind.code())]);
    }

    /// Step 6 of §2.1 (APP-132): in a step with `tx_slot`, the frame; otherwise nothing.
    pub(crate) fn downlink_frame(&mut self, tx_slot: bool, now: Instant, out: &mut dyn Output) {
        if !tx_slot {
            return;
        }
        if let Some(Err(e)) = self.frames.as_mut().map(|f| f.slot(now, out)) {
            self.slot_failed(e.kind(), out);
        }
    }
}

/// The failure of an engine in a slot, which the sizes of §4.1 rule out, so that it is driven by
/// breaking the mirror of `held` from inside the module (no hook in the production code).
#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod slot_failure_tests {
    extern crate std;

    use refapp_scenario::Instant;

    use crate::FlightNode;
    use crate::tests::{At, Out, Rec};

    /// A space packet of 20 octets (PVN 0, APID 1, unsegmented).
    const PACKET: [u8; 20] = [
        0x00, 0x01, 0xC0, 0x00, 0x00, 0x0D, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    #[test]
    fn an_engine_failing_in_a_slot_raises_downlink_refused_and_outputs_no_frame() {
        // req: APP-142
        let mut node = FlightNode::new_frames().unwrap();
        let mut rec = Rec::default();
        let at = Instant::from_micros(60_000_000);
        let later = Instant::from_micros(70_000_000);
        // VC 0 and VC 1 each hold a field; the slot sends VC 0's, and VC 1's stays in the
        // multiplexer with `held` set.
        let frames = node.frames.as_mut().unwrap();
        frames.submit(0, &PACKET, at).unwrap();
        frames.submit(1, &PACKET, at).unwrap();
        node.downlink_frame(true, later, &mut At(0, &mut rec));
        assert_eq!(rec.0.len(), 1);
        assert!(matches!(rec.0[0].1, Out::Unit(_)));
        // Break the mirror: the next release of VC 1 is offered to a slot that is taken.
        let frames = node.frames.as_mut().unwrap();
        assert_eq!(frames.held, [false, true]);
        frames.held[1] = false;
        frames.submit(1, &PACKET, later).unwrap();
        let mut failed = Rec::default();
        node.downlink_frame(
            true,
            Instant::from_micros(80_000_000),
            &mut At(1, &mut failed),
        );
        let code = u64::from(aldis::core::ErrorKind::WouldBlock.code());
        assert_eq!(failed.0, [(1, Out::Event(113, std::vec![code]))]);
        assert_eq!(node.build_failures(), 1);
    }
}
