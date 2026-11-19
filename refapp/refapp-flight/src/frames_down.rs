//! The downlink at frame level, A2 WP-3a, generic over the frame protocol since A4 WP-3a.
//!
//! Specification: docs/reference-application.md §2.1 step 6, §4.1, §4.3.1, §4.4: one
//! `PacketGenerator` for VC 0 and one for VC 1, the `ChannelMux` with fixed priority, the OCF
//! slot, and the finishing of the frame. TM has OID frames on VC 7 ([`tm`]); AOS has an idle
//! `M_PDU` on VC 7 and no OID frame ([`aos`]).
//!
//! Requirements: APP-026, APP-030…APP-035, APP-090, APP-132, APP-134, APP-135, APP-167,
//! APP-169…APP-173.
//!
//! **The protocol type.** [`FrameDownlink`] is generic over a frame protocol `P`, and what differs
//! between the protocols is the application's own trait [`DownProtocol`] (A4 D3): the frame
//! geometry, the VCs and their deadlines, the multiplexer, the OCF slots, and how a frame is
//! finished. The flight node holds the one it was made with in the closed enum [`Frames`], which
//! [`FlightNode::new_frames`](crate::FlightNode::new_frames) selects by the variant.
//!
//! The packet side is the A1 code: [`crate::downlink::Downlink`] builds each packet and reports
//! `packet_out` and then a link unit. At frame level the link unit goes to the generator of the
//! packet's VC instead of out, through [`Tap`]; the frame is the only unit the node outputs
//! (APP-090).
//!
//! **The CLCW.** At each slot the OCF takes `Farm1::clcw` of the node's uplink, after the frames
//! delivered in step 1 (§4.4.1, APP-134); it is `01 00 00 00` (`generated::tm::OCF_INITIAL`) until
//! FARM-1 changes it.

use aldis::core::engine::Actions;
use aldis::core::{Error, ErrorKind, Result};
use aldis::link::common::{IdlePacketKind, IdlePolicy, ValidPvns};
use aldis::link::generator::{
    GeneratorInput, GeneratorOutput, PacketGenerator, PacketGeneratorConfig,
};
use aldis::link::mux::{ChannelMux, FrameClass, MuxInput, MuxOutput, MuxProtocol, Offer};
use aldis::link::protocol::{Aos, PacketProtocol, Tm};
use refapp_scenario::generated::{self, link};
use refapp_scenario::{Duration, Instant, Output};

use crate::FlightNode;
use crate::downlink::Stream;
use crate::dump::{Dump, Segment};
use crate::event::APP_DOWNLINK_REFUSED;

mod aos;
mod tm;

/// Octets of a transfer frame: `generated::tm::FRAME_LENGTH` and `generated::aos::FRAME_LENGTH`
/// (APP-030, APP-169), asserted below.
const FRAME: usize = 223;

#[expect(
    clippy::as_conversions,
    reason = "const context: `usize::from(u16)` is not const, and the widening cannot truncate"
)]
const _: () = {
    assert!(FRAME == generated::tm::FRAME_LENGTH as usize);
    assert!(FRAME == generated::aos::FRAME_LENGTH as usize);
};

/// Octets of the data field a generator can hold, the most of the protocols: the TM data field
/// of 211 (the frame less the primary header, the OCF, and the FECF) against the AOS `M_PDU` of
/// 209. A generator's configuration sets its own length ([`DownProtocol::FIELD_LEN`]).
const FIELD_CAPACITY: usize = 211;

/// Octets of a generator's packet queue, behind the field: one Maximum Packet Length
/// (`generated::tm::MAX_PACKET_LENGTH`, 512); the engine needs at least 301 (that maximum less
/// the 211 of the packet zone). The largest backlog of `a2-nominal` is on VC 1 at 12.5 s (§4.4.4):
/// the dump packets (140, 140, 140, 140, 106 octets) and payload packet 6 (412) make 1 078 octets
/// submitted, the fields of frames 97 and 99 (2 × 211) have left the generator, and 211 octets
/// are in the field, so 1 078 − 422 − 211 = 445 octets wait in the queue (the unit tests agree: a
/// queue of 440 fails them and one of 450 passes). 512 leaves 67 octets of margin. On AOS the
/// zone is 207 octets and the same arithmetic gives 1 078 − 414 − 207 = 457, which 512 holds with
/// 55 of margin. A generator out of room refuses a packet with `WouldBlock`, which the node
/// reports as `downlink_refused` (APP-142).
const QUEUE: usize = 512;

type Generator<P> = PacketGenerator<P, FIELD_CAPACITY, QUEUE>;
type Mux<P> = ChannelMux<P, 3, FRAME>;

/// The `vc_priority` of the library for a VC of the rank `rank` of the scenario, where rank 1 is
/// served first (§4.1, §4.4.3). The multiplexer serves the offered VC with the *highest*
/// `vc_priority` (LNK-031), so the rank is inverted.
const fn mux_priority(rank: u8) -> u8 {
    u8::MAX.saturating_sub(rank)
}

/// The VC of the packets of `stream` (§4.1: VC 0 carries housekeeping and events, VC 1 payload and
/// dump; the AOS VCs have the same numbers, §4.3.1).
pub(crate) const fn vc_of(stream: Stream) -> u8 {
    match stream {
        Stream::Housekeeping | Stream::Events => generated::tm::VCID_REALTIME,
        Stream::Payload | Stream::Dump => generated::tm::VCID_PAYLOAD,
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

/// What the flight downlink needs of a frame protocol (A4 D3).
///
/// The application's own trait, so the library has no notion of the application's VC plan. It has
/// the implementations [`Tm`] ([`tm`]) and [`Aos`] ([`aos`]); USLP adds its own with its work
/// package.
pub trait DownProtocol: PacketProtocol + MuxProtocol {
    /// Octets of the data field the generators build, header included (the TM data field, the
    /// AOS `M_PDU`), at most [`FIELD_CAPACITY`].
    const FIELD_LEN: usize;
    /// The VCIDs of VC 0 and VC 1 (§4.1: housekeeping and events, payload and dump), served in
    /// this order.
    const DATA_VCS: [u8; 2];
    /// The release deadlines of those VCs, µs (APP-135).
    const DEADLINES_US: [u64; 2];
    /// The VC that offers an idle field when no data VC does (AOS, APP-170), or none where the
    /// multiplexer sends OID frames instead (TM).
    const IDLE_VC: Option<u8>;
    /// The Maximum Packet Length of the generators, octets.
    const MAX_PACKET_LEN: u16;
    /// The idle data octet.
    const IDLE_PATTERN: u8;

    /// The parameters of a VC that the generators need.
    fn generator_params() -> <Self as PacketProtocol>::Params;

    /// The multiplexer of the master channel, with the VCs of §4.1 or §4.3.1.
    ///
    /// # Errors
    /// The engines' `InvalidConfig`, which the scenario's values rule out.
    fn mux() -> Result<Mux<Self>>;

    /// What an offer of a field of the generator carries besides it.
    fn offer_meta(
        pointer: <Self as PacketProtocol>::FieldPointer,
    ) -> <Self as MuxProtocol>::OfferMeta;

    /// Sets the OCF slot of every channel that has one to `ocf` (APP-134, APP-173).
    ///
    /// # Errors
    /// The error of the multiplexer, which the configuration rules out.
    fn set_ocf(mux: &mut Mux<Self>, ocf: [u8; 4]) -> Result<()>;

    /// Writes the fields the multiplexer leaves to the caller, in the order of LNK-035: the FHEC
    /// where the protocol has one, then the FECF.
    ///
    /// # Errors
    /// `BufferTooSmall` for a frame the multiplexer's range rules out.
    fn finish(frame: &mut [u8]) -> Result<()>;
}

/// The generator of a VC with the release deadline of §4.1 and the idle fill of §4.4.3. The VC
/// of the `IdleField` policy has no deadline: it never holds a started field.
fn generator<P: DownProtocol>(
    vc: P::Vc,
    deadline_us: Option<u64>,
    policy: IdlePolicy,
) -> Result<Generator<P>> {
    let config = PacketGeneratorConfig::<P>::new(
        vc,
        P::generator_params(),
        P::FIELD_LEN,
        ValidPvns::SPACE,
        usize::from(P::MAX_PACKET_LEN),
    )?
    .with_idle(policy, IdlePacketKind::SpacePacket, P::IDLE_PATTERN)?;
    Generator::new(match deadline_us {
        Some(us) => config.with_release_deadline(Duration::from_micros(us)),
        None => config,
    })
}

/// A generator and whether the multiplexer holds a field of its VC.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Lane<P: DownProtocol> {
    vc: P::Vc,
    generator: Generator<P>,
    /// Whether the multiplexer holds a field of the VC. It mirrors the multiplexer's slot of the
    /// VC, so that an offer finds it free (the only `WouldBlock` of `Offer`, LNK-076).
    held: bool,
}

impl<P: DownProtocol> Lane<P> {
    fn new(id: u8, deadline_us: Option<u64>, policy: IdlePolicy) -> Result<Self> {
        let vc = P::Vc::try_from(id)?;
        Ok(Self {
            vc,
            generator: generator(vc, deadline_us, policy)?,
            held: false,
        })
    }

    /// Moves the field of the VC, if there is one, from the generator into the multiplexer
    /// unless the multiplexer holds one already.
    ///
    /// The field leaves the generator before the multiplexer takes it. The VC and field length
    /// are those the multiplexer was configured with, so the refusals of `Offer` other than
    /// `WouldBlock` cannot arise.
    fn offer(&mut self, mux: &mut Mux<P>, now: Instant, field: &mut [u8]) -> Result<()> {
        if self.held {
            return Ok(());
        }
        let actions = self.generator.step(GeneratorInput::Release, now, field)?;
        match actions.get(0) {
            Some(GeneratorOutput::DataField(data)) => {
                let octets = data
                    .field
                    .slice(field)
                    .ok_or(Error::new(ErrorKind::InternalError))?;
                none(mux.step(
                    MuxInput::Offer(Offer {
                        vc: self.vc,
                        field: octets,
                        meta: P::offer_meta(data.pointer),
                    }),
                    &mut [],
                )?)?;
                self.held = true;
            }
            Some(GeneratorOutput::Discarded { .. }) | None => {}
        }
        Ok(())
    }
}

/// The frame level of the flight node's downlink (A2 WP-3a, A4 WP-3a).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameDownlink<P: DownProtocol> {
    /// VC 0 and VC 1, in the order of [`DownProtocol::DATA_VCS`].
    lanes: [Lane<P>; 2],
    /// The idle VC where the protocol has one ([`DownProtocol::IDLE_VC`]): its generator never
    /// holds a started field and always has an idle field to offer.
    idle: Option<Lane<P>>,
    mux: Mux<P>,
    /// The OCF of the next frames (APP-134).
    ocf: [u8; 4],
}

impl<P: DownProtocol> FrameDownlink<P> {
    /// The downlink in its start-of-run state: every frame count at 0 (APP-035, APP-171), the OCF
    /// `OCF_INITIAL` (§4.4.1), no field started.
    ///
    /// # Errors
    /// The engines' `InvalidConfig` or `CapacityExceeded`, which the scenario's values rule out.
    pub fn new() -> Result<Self> {
        let [realtime, payload] = P::DATA_VCS;
        let [realtime_us, payload_us] = P::DEADLINES_US;
        let idle = match P::IDLE_VC {
            Some(id) => Some(Lane::new(id, None, IdlePolicy::IdleField)?),
            None => None,
        };
        Ok(Self {
            lanes: [
                Lane::new(realtime, Some(realtime_us), IdlePolicy::Oid)?,
                Lane::new(payload, Some(payload_us), IdlePolicy::Oid)?,
            ],
            idle,
            mux: P::mux()?,
            ocf: generated::tm::OCF_INITIAL.to_be_bytes(),
        })
    }

    /// Submits a packet to the generator of `vc` at the instant it was generated, which starts a
    /// field if none is started (LNK-012, §2.1).
    fn submit(&mut self, vc: u8, packet: &[u8], now: Instant) -> Result<()> {
        let lane = self
            .lanes
            .iter_mut()
            .find(|lane| Into::<u8>::into(lane.vc) == vc)
            .ok_or(Error::new(ErrorKind::UnknownChannel))?;
        none(
            lane.generator
                .step(GeneratorInput::SubmitPacket(packet), now, &mut [])?,
        )
    }

    /// The OCF of the next frames: the CLCW of FARM-1 (§4.4.1, APP-134, APP-173).
    const fn set_ocf(&mut self, ocf: [u8; 4]) {
        self.ocf = ocf;
    }

    /// The earliest release deadline of the generators (APP-135), after `now` (APP-167, APP-090,
    /// API-053). A deadline that is not after `now` (a step off the frame slot grid, past the
    /// deadline; §4.4.2) is released only by the next step with `tx_slot`, so it counts as the
    /// next slot instant after `now`; a later deadline of another generator before that slot
    /// is still the one reported.
    ///
    /// The slot grid starts at instant 0: the slots are the multiples of `FRAME_SLOT_US`, as in
    /// the harness (§4.4.2).
    #[must_use]
    pub fn next_deadline(&self, now: Instant) -> Option<Instant> {
        const SLOT: u64 = link::FRAME_SLOT_US;
        const { assert!(SLOT != 0) };
        let next_slot = now
            .as_micros()
            .checked_div(SLOT)
            .map(|n| Instant::from_micros(n.saturating_add(1).saturating_mul(SLOT)));
        self.lanes
            .iter()
            .filter_map(|lane| lane.generator.next_deadline())
            .filter_map(|due| if due > now { Some(due) } else { next_slot })
            .min()
    }

    /// Step 6 of §2.1 at a slot instant (APP-132): signals the deadlines, moves the field of each
    /// data VC without one in the multiplexer into it, then the idle field of the idle VC where
    /// there is one, sets the OCF slots (APP-134), takes the frame, finishes it, and outputs it as
    /// the one unit of the step.
    ///
    /// # Errors
    /// The error of an engine, which the sizes of §4.1 rule out; no frame is output.
    pub fn slot(&mut self, now: Instant, out: &mut dyn Output) -> Result<()> {
        let mut field = [0_u8; FIELD_CAPACITY];
        for lane in &mut self.lanes {
            if lane.generator.next_deadline().is_some_and(|due| due <= now) {
                none(
                    lane.generator
                        .step(GeneratorInput::Timeout, now, &mut field)?,
                )?;
            }
            lane.offer(&mut self.mux, now, &mut field)?;
        }
        if let Some(idle) = self.idle.as_mut() {
            idle.offer(&mut self.mux, now, &mut field)?;
        }
        let mut frame = [0_u8; FRAME];
        P::set_ocf(&mut self.mux, self.ocf)?;
        let actions = self.mux.step(MuxInput::Opportunity, &mut frame)?;
        let Some(MuxOutput::Transmit(sent)) = actions.get(0) else {
            return Err(Error::new(ErrorKind::InternalError));
        };
        if sent.class == FrameClass::Data {
            for lane in self.lanes.iter_mut().chain(self.idle.as_mut()) {
                if lane.vc == sent.vc {
                    lane.held = false;
                }
            }
        }
        let octets = frame
            .get_mut(sent.frame.start()..sent.frame.end())
            .ok_or(Error::new(ErrorKind::InternalError))?;
        P::finish(octets)?;
        out.unit(octets);
        Ok(())
    }
}

/// The frame level of the downlink of the variant the node was made with (§4.3).
///
/// The closed set of the protocols, so that a node of a variant that has none fails to compile
/// where it is matched (RS-021). `uslp` has no arm until its work package.
#[expect(
    clippy::large_enum_variant,
    reason = "the variants are within 5 % of each other, and the node has no heap to box one in"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frames {
    /// TM frames (§4.1), the variant `tm_tc`.
    Tm(FrameDownlink<Tm>),
    /// AOS frames (§4.3.1), the variant `aos`.
    Aos(FrameDownlink<Aos>),
}

impl Frames {
    /// The TM downlink in its start-of-run state ([`FrameDownlink::new`]).
    ///
    /// # Errors
    /// As [`FrameDownlink::new`].
    pub fn tm() -> Result<Self> {
        FrameDownlink::new().map(Self::Tm)
    }

    /// The AOS downlink in its start-of-run state ([`FrameDownlink::new`]).
    ///
    /// # Errors
    /// As [`FrameDownlink::new`].
    pub fn aos() -> Result<Self> {
        FrameDownlink::new().map(Self::Aos)
    }

    fn submit(&mut self, vc: u8, packet: &[u8], now: Instant) -> Result<()> {
        match self {
            Self::Tm(f) => f.submit(vc, packet, now),
            Self::Aos(f) => f.submit(vc, packet, now),
        }
    }

    const fn set_ocf(&mut self, ocf: [u8; 4]) {
        match self {
            Self::Tm(f) => f.set_ocf(ocf),
            Self::Aos(f) => f.set_ocf(ocf),
        }
    }

    /// The earliest release deadline of the generators ([`FrameDownlink::next_deadline`]).
    #[must_use]
    pub fn next_deadline(&self, now: Instant) -> Option<Instant> {
        match self {
            Self::Tm(f) => f.next_deadline(now),
            Self::Aos(f) => f.next_deadline(now),
        }
    }

    /// Step 6 of §2.1 ([`FrameDownlink::slot`]).
    ///
    /// # Errors
    /// As [`FrameDownlink::slot`].
    pub fn slot(&mut self, now: Instant, out: &mut dyn Output) -> Result<()> {
        match self {
            Self::Tm(f) => f.slot(now, out),
            Self::Aos(f) => f.slot(now, out),
        }
    }
}

/// An [`Output`] that routes the link units of packets to the generator of their VC instead of out,
/// and passes every other output on (A2: the packet is no longer a unit).
///
/// A tap without a [`Frames`] is the A1 behaviour: the unit goes out.
pub(crate) struct Tap<'a> {
    frames: Option<&'a mut Frames>,
    vc: u8,
    at: Instant,
    out: &'a mut dyn Output,
    refused: Option<ErrorKind>,
}

impl<'a> Tap<'a> {
    /// A tap for packets of `vc` generated at `at`.
    pub(crate) fn new(
        frames: Option<&'a mut Frames>,
        vc: u8,
        at: Instant,
        out: &'a mut dyn Output,
    ) -> Self {
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
    /// The tap for a packet of `stream` generated at `at`.
    pub(crate) fn tap<'a>(
        frames: Option<&'a mut Frames>,
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
            let mut tap = Tap::new(self.frames.as_mut(), vc_of(Stream::Dump), dump.due(), out);
            let sent = dump.send(&mut self.downlink, &mut tap);
            (sent, tap.refused())
        };
        if let Some(kind) = refused {
            self.packet_refused(kind, vc_of(Stream::Dump), out);
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
        // The OCF takes FARM-1's CLCW after the step-1 deliveries (§4.4.1, APP-134).
        let clcw = self.clcw();
        if let (Some(frames), Some(clcw)) = (self.frames.as_mut(), clcw) {
            frames.set_ocf(clcw.to_octets());
        }
        if let Some(Err(e)) = self.frames.as_mut().map(|f| f.slot(now, out)) {
            self.slot_failed(e.kind(), out);
        }
    }
}

/// The failure of an engine in a slot, which the sizes of §4.1 rule out, so that it is driven by
/// breaking the mirror of `held` from inside the module (no hook in the production code).
#[cfg(test)]
mod slot_failure_tests {
    extern crate std;

    use refapp_scenario::{Instant, Variant};

    use super::Frames;
    use crate::FlightNode;
    use crate::tests::{At, Out, Rec};

    /// A space packet of 20 octets (PVN 0, APID 1, unsegmented).
    const PACKET: [u8; 20] = [
        0x00, 0x01, 0xC0, 0x00, 0x00, 0x0D, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    #[test]
    fn a_passed_deadline_does_not_hide_an_earlier_future_one() {
        // req: APP-135, APP-167
        let mut node = FlightNode::new_frames(Variant::TmTc).unwrap();
        let Some(Frames::Tm(frames)) = node.frames.as_mut() else {
            panic!("a TM node")
        };
        // VC 0 starts at 100 000 (deadline 350 000, passed at `now`); VC 1 starts at 10 000
        // (deadline 1 010 000, between `now` and the next slot at 1 125 000).
        frames
            .submit(0, &PACKET, Instant::from_micros(100_000))
            .unwrap();
        frames
            .submit(1, &PACKET, Instant::from_micros(10_000))
            .unwrap();
        let next = frames.next_deadline(Instant::from_micros(1_000_000));
        assert_eq!(next, Some(Instant::from_micros(1_010_000)));
        // Both passed: the next slot.
        let next = frames.next_deadline(Instant::from_micros(1_020_000));
        assert_eq!(next, Some(Instant::from_micros(1_125_000)));
    }

    #[test]
    fn an_engine_failing_in_a_slot_raises_downlink_refused_and_outputs_no_frame() {
        // req: APP-142
        let mut node = FlightNode::new_frames(Variant::TmTc).unwrap();
        let mut rec = Rec::default();
        let at = Instant::from_micros(60_000_000);
        let later = Instant::from_micros(70_000_000);
        // VC 0 and VC 1 each hold a field; the slot sends VC 0's, and VC 1's stays in the
        // multiplexer with `held` set.
        let Some(Frames::Tm(frames)) = node.frames.as_mut() else {
            panic!("a TM node")
        };
        frames.submit(0, &PACKET, at).unwrap();
        frames.submit(1, &PACKET, at).unwrap();
        node.downlink_frame(true, later, &mut At(0, &mut rec));
        assert_eq!(rec.0.len(), 1);
        assert!(matches!(rec.0[0].1, Out::Unit(_)));
        // Break the mirror: the next release of VC 1 is offered to a slot that is taken.
        let Some(Frames::Tm(frames)) = node.frames.as_mut() else {
            panic!("a TM node")
        };
        assert_eq!([frames.lanes[0].held, frames.lanes[1].held], [false, true]);
        frames.lanes[1].held = false;
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

/// The order in which the VCs of AOS are served, driven through the module's own `submit`.
#[cfg(test)]
mod aos_priority_tests {
    extern crate std;

    use refapp_scenario::{Instant, Variant};

    use super::Frames;
    use crate::FlightNode;
    use crate::tests::{At, Out, Rec};

    /// A space packet of 20 octets (PVN 0, APID 1, unsegmented).
    const PACKET: [u8; 20] = [
        0x00, 0x01, 0xC0, 0x00, 0x00, 0x0D, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    /// The VCIDs of the frames of the slots from `later`, one frame per slot.
    fn vcs_sent(node: &mut FlightNode, slots: u64, later: u64) -> std::vec::Vec<u8> {
        let mut rec = Rec::default();
        for i in 0..slots {
            node.downlink_frame(
                true,
                Instant::from_micros(i.saturating_mul(125_000).saturating_add(later)),
                &mut At(i, &mut rec),
            );
        }
        rec.0
            .iter()
            .filter_map(|(_, o)| match o {
                Out::Unit(u) => Some(u[1] & 0x3F),
                Out::Packet(..) | Out::Event(..) => None,
            })
            .collect()
    }

    #[test]
    fn vc_7_is_sent_only_when_neither_data_vc_holds_a_released_field() {
        // req: APP-170, LNK-031
        let mut node = FlightNode::new_frames(Variant::Aos).unwrap();
        let Some(Frames::Aos(frames)) = node.frames.as_mut() else {
            panic!("an AOS node")
        };
        // One packet each on VC 0 and VC 1, started at 0 s, are released by their deadlines
        // (0.25 s and 1 s) at the first slot at 2 s: both fields are held, and VC 7's idle field
        // is offered at the same slot. The fixed priority serves 0, then 1, then 7 (LNK-031).
        frames.submit(0, &PACKET, Instant::from_micros(0)).unwrap();
        frames.submit(1, &PACKET, Instant::from_micros(0)).unwrap();
        assert_eq!(vcs_sent(&mut node, 4, 2_000_000), [0, 1, 7, 7]);
    }

    #[test]
    fn a_slot_without_a_held_field_sends_the_idle_field_and_no_oid_frame() {
        // req: APP-170, APP-172
        let mut node = FlightNode::new_frames(Variant::Aos).unwrap();
        assert_eq!(vcs_sent(&mut node, 3, 0), [7, 7, 7]);
    }
}
