//! The uplink at frame level, generic over the protocol type ([`UplinkProtocol`]: `Tc`, `Uslp`;
//! D3 of the A4 plan): the `MapSegmenter`, FOP-1 for Type-BD, and the frame (A2 WP-4b, A4
//! WP-4c).
//!
//! Specification: `docs/reference-application.md` §2.2 (order within an instant), §4.2, §4.3.3,
//! §4.4.4 (uplink frames), §7.4 (`uplink_refused`); APP-040…APP-044, APP-071, APP-090, APP-133,
//! APP-143, APP-181…APP-184; LNK-040…LNK-043; COP-010, TC-008.
//!
//! The ops-script packets are submitted to the segmenter when they are released: in `tm_tc` and
//! `aos` the commands on MAP 0 and the table upload on MAP 1, in `uslp` both on MAP 0 (D6). In a
//! step with `tx_slot`, [`FrameUplink::slot`] asks the segmenter for one FDU with the expedited
//! quality of service, gives it to FOP-1 as a `RequestBd`, and returns the Type-BD frame (N(S) =
//! 0, the FECF) that FOP-1 builds; at `cop1` the AD service of [`ad`] takes its place. The
//! variant `uslp` exists at `cop1` only (APP-187).

use core::fmt::Debug;
use core::num::NonZeroU8;

use aldis::cop1::{
    Fop1, Fop1Config, Fop1Input, Fop1Output, FrameKind, TimeoutType, fop1_max_actions,
};
use aldis::core::Error;
use aldis::core::ErrorKind;
use aldis::core::time::{Duration, Instant};
use aldis::link::common::Qos;
use aldis::link::protocol::{Tc, Uslp};
use aldis::link::segmenter::{MapSegmenter, SegmenterInput};
use refapp_scenario::generated::{cop1, tc, uslp_up};
use refapp_scenario::{Apid, Output, Variant};

use crate::ops::Fop1Directive;

mod ad;
mod protocol;

pub(crate) use protocol::UplinkProtocol;

/// Capacity of the segmenter's queue, octets: the longest packet (512) with its entry, and the
/// packets of one instant besides.
const QUEUE: usize = 1024;
/// Entries of the `Sent_Queue` of FOP-1. It must equal `cop1::K`, which `FrameUplink::new`
/// checks; Type-BD frames never enter the queue (D2).
const SENT: usize = 4;
/// The scenario's Sliding Window Width K: a zero in the scenario fails the build (COP-010).
const WINDOW: NonZeroU8 = nonzero(cop1::K);
/// The scenario's repetitions of an AD frame (Type-A), at least 1.
const AD_REPETITIONS: NonZeroU8 = nonzero(cop1::FOP_REPETITIONS_TYPE_A);
/// The scenario's repetitions of a BC or BD frame (Type-B), at least 1.
const BC_REPETITIONS: NonZeroU8 = nonzero(cop1::FOP_REPETITIONS_CONTROL);
/// Octets of a TC frame, the capacity of FOP-1's master copies and of the output areas.
const TC_FRAME: usize = 128;
/// Octets of a USLP frame (APP-182).
const USLP_FRAME: usize = 512;

// The capacities are constants of this module; the scenario's values must agree (a USLP node
// built on other values would refuse every frame).
const _: () = assert!(tc::MAX_FRAME_LENGTH == 128);
const _: () = assert!(uslp_up::MAX_FRAME_LENGTH == 512);
// The USLP target takes the rule `111` only, which FOP-1 writes for the whole TFDF of a request
// (D6, APP-184): a MAP that segments would put the rules `100` to `110` in front of the FDUs.
const _: () = assert!(uslp_up::MAP_BLOCKING && !uslp_up::MAP_SEGMENTATION);
const _: () =
    assert!(uslp_up::VCF_COUNT_LENGTH_SEQUENCE == 1 && uslp_up::VCF_COUNT_LENGTH_EXPEDITED == 0);

/// A scenario constant that must not be zero, evaluated at compile time.
#[expect(
    clippy::panic,
    reason = "evaluated only in a const: a zero scenario value fails the build"
)]
const fn nonzero(value: u8) -> NonZeroU8 {
    match NonZeroU8::new(value) {
        Some(value) => value,
        None => panic!("a scenario constant is 0"),
    }
}

/// Application event `uplink_refused` (§7.4, APP-143).
const UPLINK_REFUSED: u16 = 114;
/// Stage of the event: a submit to the segmenter.
const STAGE_SUBMIT: u64 = 1;
/// Stage of the event: the request for the next FDU.
const STAGE_NEXT_FDU: u64 = 2;
/// Stage of the event: an input of FOP-1.
const STAGE_FOP1: u64 = 3;
/// Actions of one FOP-1 input (COP-006).
const ACTIONS: usize = fop1_max_actions(SENT);

/// The sending end of the uplink at frame level for the protocol `P`, with `M` MAPs and FOP-1
/// master copies of `F` octets.
#[derive(Debug, Clone)]
pub(crate) struct FrameUplink<P: UplinkProtocol, const M: usize, const F: usize> {
    segmenter: MapSegmenter<P, M, QUEUE>,
    fop: Fop1<SENT, F, ACTIONS, P>,
    /// The octets of the longest FDU the segmenter hands out: the target's, with `P::HEADER_LEN` octets in front.
    fdu_len: usize,
    /// The handle of the next request (API-054).
    next_handle: u32,
    /// Inputs the engines refused; zero in a run whose script fits the capacities.
    refused: u32,
    /// The state of the `cop1` level; `None` at `frames`, where only Type-BD frames go out.
    ad: Option<ad::Ad>,
}

/// The uplink of the variant a node was made for: the closed set of the protocol types.
#[derive(Debug, Clone)]
pub(crate) enum Uplink {
    /// The TC uplink (§4.2).
    Tc(Box<FrameUplink<Tc, 2, TC_FRAME>>),
    /// The USLP uplink (§4.3.3).
    Uslp(Box<FrameUplink<Uslp, 1, USLP_FRAME>>),
}

impl Uplink {
    /// The TC uplink at the `frames` level (A2): Type-BD frames only.
    ///
    /// # Errors
    /// As [`FrameUplink::new`].
    pub(crate) fn new() -> Result<Self, Error> {
        FrameUplink::new().map(|up| Self::Tc(Box::new(up)))
    }

    /// The TC uplink at the `cop1` level (A3).
    ///
    /// # Errors
    /// As [`FrameUplink::new`].
    #[cfg(test)]
    pub(crate) fn new_cop1() -> Result<Self, Error> {
        Self::cop1(Variant::TmTc)
    }

    /// The uplink of `variant` at the `cop1` level: the TC uplink of `tm_tc` and `aos`, the USLP
    /// uplink of `uslp` (APP-040…APP-044, APP-181…APP-184).
    ///
    /// # Errors
    /// As [`FrameUplink::new`].
    pub(crate) fn cop1(variant: Variant) -> Result<Self, Error> {
        match variant {
            Variant::TmTc | Variant::Aos => {
                FrameUplink::new_cop1().map(|up| Self::Tc(Box::new(up)))
            }
            Variant::Uslp => FrameUplink::new_cop1().map(|up| Self::Uslp(Box::new(up))),
        }
    }

    /// Inputs the engines refused so far.
    pub(crate) fn refused(&self) -> u32 {
        match self {
            Self::Tc(up) => up.refused(),
            Self::Uslp(up) => up.refused(),
        }
    }

    /// See [`FrameUplink::submit`].
    pub(crate) fn submit(&mut self, apid: Apid, packet: &[u8], now: Instant, out: &mut dyn Output) {
        match self {
            Self::Tc(up) => up.submit(apid, packet, now, out),
            Self::Uslp(up) => up.submit(apid, packet, now, out),
        }
    }

    /// See [`FrameUplink::slot`].
    pub(crate) fn slot(&mut self, now: Instant, out: &mut dyn Output) {
        match self {
            Self::Tc(up) => up.slot(now, out),
            Self::Uslp(up) => up.slot(now, out),
        }
    }

    /// See [`FrameUplink::slot_ad`].
    #[cfg(test)]
    pub(crate) fn slot_ad(&mut self, now: Instant, out: &mut dyn Output) {
        match self {
            Self::Tc(up) => up.slot_ad(now, out),
            Self::Uslp(up) => up.slot_ad(now, out),
        }
    }

    /// See [`FrameUplink::request_bd`].
    #[cfg(test)]
    pub(crate) fn request_bd(
        &mut self,
        handle: u32,
        fdu: &[u8],
        now: Instant,
        out: &mut dyn Output,
    ) -> Option<Vec<u8>> {
        match self {
            Self::Tc(up) => up.request_bd(handle, fdu, now, out),
            Self::Uslp(up) => up.request_bd(handle, fdu, now, out),
        }
    }

    /// See [`FrameUplink::clcw`].
    pub(crate) fn clcw(&mut self, word: u32, now: Instant, out: &mut dyn Output) {
        match self {
            Self::Tc(up) => up.clcw(word, now, out),
            Self::Uslp(up) => up.clcw(word, now, out),
        }
    }

    /// See [`FrameUplink::timeout`].
    pub(crate) fn timeout(&mut self, now: Instant, out: &mut dyn Output) {
        match self {
            Self::Tc(up) => up.timeout(now, out),
            Self::Uslp(up) => up.timeout(now, out),
        }
    }

    /// See [`FrameUplink::next_deadline`].
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        match self {
            Self::Tc(up) => up.next_deadline(),
            Self::Uslp(up) => up.next_deadline(),
        }
    }

    /// See [`FrameUplink::directive`].
    pub(crate) fn directive(
        &mut self,
        directive: Fop1Directive,
        now: Instant,
        out: &mut dyn Output,
    ) {
        match self {
            Self::Tc(up) => up.directive(directive, now, out),
            Self::Uslp(up) => up.directive(directive, now, out),
        }
    }
}

impl<P: UplinkProtocol, const M: usize, const F: usize> FrameUplink<P, M, F> {
    /// The segmenter and FOP-1 of the uplink, from the scenario's values.
    ///
    /// # Errors
    /// The library's `InvalidConfig` if a scenario value is out of range for the engines, or
    /// disagrees with the capacities of this module.
    pub(crate) fn new() -> Result<Self, Error> {
        let bad = Error::new(ErrorKind::InvalidConfig);
        let target = P::target()?;
        if usize::from(cop1::K) != SENT || target.max_frame_len() != F || M != P::MAPS {
            return Err(bad);
        }
        let config = P::segmenter_config::<M>(&target)?;
        let fdu_len = config.max_fdu_len();
        let timeout_type = match cop1::TIMEOUT_TYPE {
            0 => TimeoutType::Tt0,
            _ => return Err(bad),
        };
        let fop = Fop1::new(Fop1Config {
            target,
            t1_initial: Duration::from_micros(cop1::T1_INITIAL_US),
            transmission_limit: NonZeroU8::new(cop1::TRANSMISSION_LIMIT).ok_or(bad)?,
            timeout_type,
            sliding_window_k: WINDOW,
            ad_repetitions: AD_REPETITIONS,
            bc_repetitions: BC_REPETITIONS,
        })?;
        Ok(Self {
            segmenter: MapSegmenter::new(config)?,
            fop,
            fdu_len,
            next_handle: 0,
            refused: 0,
            ad: None,
        })
    }

    /// Inputs the engines refused so far.
    pub(crate) const fn refused(&self) -> u32 {
        self.refused
    }

    /// Hands a packet of the script to the segmenter (APP-042). A packet the segmenter refuses,
    /// of another APID or one the queue cannot take whole, is counted in
    /// [`refused`](Self::refused) and raises `uplink_refused` (APP-143).
    pub(crate) fn submit(&mut self, apid: Apid, packet: &[u8], now: Instant, out: &mut dyn Output) {
        let Some(map) = P::map_id(apid).and_then(|id| P::Map::try_from(id).ok()) else {
            // No MAP for this APID (APP-143): the MAP argument is 0, since there is none.
            self.refuse(out, STAGE_SUBMIT, Error::new(ErrorKind::UnknownChannel), 0);
            return;
        };
        let mut unused = [0_u8; 0];
        let input = SegmenterInput::SubmitPacket { map, packet };
        if let Err(error) = self.segmenter.step(input, now, &mut unused) {
            self.refuse(out, STAGE_SUBMIT, error, u64::from(map.into()));
        }
    }

    /// One uplink slot: at `cop1` the AD service ([`slot_ad`](Self::slot_ad)), where
    /// `TransmitDone` and its events follow the frame (APP-144); below it the frame of
    /// [`slot_bd`](Self::slot_bd), output as the last output of the step (APP-133).
    pub(crate) fn slot(&mut self, now: Instant, out: &mut dyn Output) {
        let frame = if self.ad.is_some() {
            self.slot_ad(now, out);
            None
        } else {
            self.slot_bd(now, out)
        };
        if let Some(frame) = frame {
            out.unit(&frame);
        }
    }

    /// The next FDU as a Type-BD frame, or `None` if the segmenter holds none
    /// or an engine refused an input, which raises `uplink_refused` (APP-071, APP-133,
    /// APP-143).
    fn slot_bd(&mut self, now: Instant, out: &mut dyn Output) -> Option<Vec<u8>> {
        let handle = self.next_handle;
        self.next_handle = self.next_handle.wrapping_add(1);
        let mut area = vec![0_u8; self.fdu_len];
        let fdu = self.take_fdu(handle, Qos::Expedited, now, &mut area, out)?;
        self.request_bd(handle, &fdu, now, out)
    }

    /// FOP-1 `RequestBd` for `fdu` and the response of the lower procedures that frees the
    /// Bypass Out flag (232.1 §5.1.6): returns the frame FOP-1 transmits.
    pub(crate) fn request_bd(
        &mut self,
        handle: u32,
        fdu: &[u8],
        now: Instant,
        out: &mut dyn Output,
    ) -> Option<Vec<u8>> {
        let done = Fop1Input::TransmitDone {
            kind: FrameKind::Bd,
            accepted: true,
        };
        let sent = self
            .fop_step(Fop1Input::RequestBd { handle, fdu }, now)
            .and_then(|frame| frame.ok_or(Error::new(ErrorKind::InvalidState)))
            .and_then(|frame| self.fop_step(done, now).map(|_| frame));
        match sent {
            Ok(frame) => Some(frame),
            Err(error) => {
                self.refuse(out, STAGE_FOP1, error, 0);
                None
            }
        }
    }

    /// One FOP-1 input; returns the frame its Transmit action holds, if any.
    fn fop_step(&mut self, input: Fop1Input<'_>, now: Instant) -> Result<Option<Vec<u8>>, Error> {
        let mut out = [0_u8; F];
        let decision = self.fop.decide(input, now, &mut out)?;
        let actions = decision.commit();
        Ok(actions.iter().find_map(|action| match action {
            Fop1Output::Transmit { frame, .. } => frame.slice(&out).map(<[u8]>::to_vec),
            Fop1Output::Abort
            | Fop1Output::Transfer(_)
            | Fop1Output::Directive(_)
            | Fop1Output::Alert(_)
            | Fop1Output::Suspend => None,
        }))
    }

    /// Counts a refusal and raises `uplink_refused` (§7.4, APP-143).
    fn refuse(&mut self, out: &mut dyn Output, stage: u64, error: Error, map: u64) {
        self.refused = self.refused.saturating_add(1);
        out.app_event(
            UPLINK_REFUSED,
            &[stage, u64::from(error.kind().code()), map],
        );
    }
}

#[cfg(test)]
mod tests_uslp;
