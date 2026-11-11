//! The uplink at the `cop1` level: FOP-1 with `RequestAd`, its inputs, the held transmit
//! requests, and the COP-1 events (A3 WP-4).
//!
//! Specification: `docs/reference-application.md` §4.5.1 (order of work), §4.5.2 (directives),
//! §7.4 (events 200 to 203); APP-036, APP-044, APP-071, APP-090, APP-143, APP-144, APP-145,
//! APP-148, APP-149; COP-004, COP-006, COP-010, COP-011, COP-013.
//!
//! The node is the caller of 232.1 §3.2.2 and nothing more: FOP-1 decides, the node moves its
//! inputs and outputs. A transmit request that FOP-1 returns is **held** (at most one for each
//! of Type-AD and Type-BC, since FOP-1 sets the Out flag to `Not_Ready`, 232.1 §5.1.6) until a
//! slot, where the node outputs it and gives `TransmitDone`.

use aldis::cop1::{
    Alert, AlertReason, DirectiveNotification, DirectiveNotify, Fop1Event, Fop1Input, Fop1Output,
    Fop1State, FrameKind, TransferNotification, TransferNotify,
};
use aldis::core::time::Instant;
use aldis::core::{Error, ErrorKind};
use aldis::link::common::Qos;
use aldis::tc::Clcw;
use refapp_scenario::Output;

use super::{FDU, FRAME, STAGE_FOP1, STAGE_NEXT_FDU, Uplink};
use crate::ops::Fop1Directive;

/// Application event `fop1_alert` (§7.4).
pub(crate) const FOP1_ALERT: u16 = 200;
/// Application event `fop1_transfer` (§7.4).
pub(crate) const FOP1_TRANSFER: u16 = 201;
/// Application event `fop1_directive` (§7.4).
pub(crate) const FOP1_DIRECTIVE: u16 = 202;
/// Application event `fop1_state` (§7.4).
pub(crate) const FOP1_STATE: u16 = 203;

/// What the node keeps of FOP-1's callers' state at the `cop1` level (§4.5.1).
#[derive(Debug, Clone, Default)]
pub(super) struct Ad {
    /// The request identifier of the next FDU given to FOP-1 (§4.5.1: the n-th is n - 1).
    next_request: u32,
    /// Whether the last `RequestAd` has neither its Accept nor its Reject yet (APP-145).
    unanswered: bool,
    /// The held Type-AD transmit request: the frame.
    sequence_controlled: Option<Vec<u8>>,
    /// The held Type-BC transmit request: the frame.
    control: Option<Vec<u8>>,
    /// The directives given to FOP-1; the handle of a directive is its index.
    directives: Vec<Fop1Directive>,
}

impl Uplink {
    /// As [`new`](Uplink::new), for the `cop1` level: Type-AD and Type-BC frames through FOP-1.
    ///
    /// # Errors
    /// As [`new`](Uplink::new).
    pub(crate) fn new_cop1() -> Result<Uplink, Error> {
        Ok(Uplink {
            ad: Some(Ad::default()),
            ..Uplink::new()?
        })
    }

    /// The OCF word of a delivered TM frame as FOP-1's `Clcw` input, `InvalidClcw` if
    /// `Clcw::decode` refuses it (D3 a, APP-144). Nothing at the `frames` level.
    pub(crate) fn clcw(&mut self, word: u32, now: Instant, out: &mut dyn Output) {
        if self.ad.is_none() {
            return;
        }
        let input = match Clcw::decode(word.to_be_bytes()) {
            Ok(clcw) => Fop1Input::Clcw(clcw),
            Err(_) => Fop1Input::InvalidClcw,
        };
        self.drive(input, now, out);
    }

    /// Gives `Timeout` to FOP-1, once, if its deadline is not after `now` (D3 d, §4.5.1 step 2).
    pub(crate) fn timeout(&mut self, now: Instant, out: &mut dyn Output) {
        if self.ad.is_some() && self.fop.next_deadline().is_some_and(|d| d <= now) {
            self.drive(Fop1Input::Timeout, now, out);
        }
    }

    /// FOP-1's deadline, the node's own (D3 d, APP-135); `None` at the `frames` level.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.ad.as_ref()?;
        self.fop.next_deadline()
    }

    /// Gives a directive of the ops script to FOP-1 (§4.5.2). Nothing below `cop1` (APP-019).
    pub(crate) fn directive(
        &mut self,
        directive: Fop1Directive,
        now: Instant,
        out: &mut dyn Output,
    ) {
        let Some(ad) = &mut self.ad else {
            return;
        };
        let Ok(handle) = u32::try_from(ad.directives.len()) else {
            self.refuse(out, STAGE_FOP1, Error::new(ErrorKind::OutOfRange), 0);
            return;
        };
        ad.directives.push(directive);
        let input = Fop1Input::Directive {
            handle,
            directive: directive.to_engine(),
        };
        self.drive(input, now, out);
    }

    /// One uplink slot at `cop1` (§4.5.1 step 4): a `RequestAd` of one FDU if none is
    /// unanswered, then the held frame, Type-BC before Type-AD, as the last output of the step
    /// and `TransmitDone` for its kind (APP-071, APP-090, APP-144).
    pub(crate) fn slot_ad(&mut self, now: Instant, out: &mut dyn Output) {
        let Some(ad) = &mut self.ad else {
            return;
        };
        if !ad.unanswered {
            let handle = ad.next_request;
            let mut area = [0_u8; FDU];
            if let Some(fdu) = self.take_fdu(handle, Qos::SequenceControlled, now, &mut area, out)
                && let Some(ad) = &mut self.ad
            {
                ad.next_request = ad.next_request.wrapping_add(1);
                // The request counts as unanswered unless the engine refuses it with an error
                // (APP-145); a notification inside `drive` clears the flag again.
                ad.unanswered = true;
                if !self.drive(Fop1Input::RequestAd { handle, fdu: &fdu }, now, out)
                    && let Some(ad) = &mut self.ad
                {
                    // Refused: the FDU taken is lost, reported by `uplink_refused` and not
                    // retried (APP-143); the next slot requests the next FDU.
                    ad.unanswered = false;
                }
            }
        }
        let Some(ad) = &mut self.ad else {
            return;
        };
        let held = match (ad.control.take(), ad.sequence_controlled.take()) {
            (Some(bc), sequence_controlled) => {
                ad.sequence_controlled = sequence_controlled;
                Some((FrameKind::Bc, bc))
            }
            (None, Some(frame)) => Some((FrameKind::Ad, frame)),
            (None, None) => None,
        };
        if let Some((kind, frame)) = held {
            out.unit(&frame);
            let done = Fop1Input::TransmitDone {
                kind,
                accepted: true,
            };
            self.drive(done, now, out);
        }
    }

    /// One FOP-1 input: the engine decides, the node commits, holds the frame it returns, and
    /// raises the events of the actions in their order, `fop1_state` of a change last (§7.4).
    /// Returns `false` if the engine refused the input with an error (`uplink_refused`).
    fn drive(&mut self, input: Fop1Input<'_>, now: Instant, out: &mut dyn Output) -> bool {
        let before = self.fop.state();
        let mut area = [0_u8; FRAME];
        let decision = match self.fop.decide(&self.store, input, now, &mut area) {
            Ok(decision) => decision,
            Err(error) => {
                self.refuse(out, STAGE_FOP1, error, 0);
                return false;
            }
        };
        let actions = self.fop.commit(&mut self.store, decision);
        for action in actions.iter() {
            match *action {
                Fop1Output::Transmit { kind, frame, .. } => match frame.slice(&area) {
                    Some(octets) => self.hold(kind, octets, out),
                    None => self.refuse(out, STAGE_FOP1, Error::new(ErrorKind::OutOfRange), 0),
                },
                Fop1Output::Transfer(notify) => self.transferred(notify, out),
                Fop1Output::Directive(notify) => self.directive_notified(notify, out),
                Fop1Output::Alert(alert) => alert_event(alert, out),
                // The Abort Request is not acted on (§4.5.1); Suspend needs Timeout_Type 1.
                Fop1Output::Abort | Fop1Output::Suspend => {}
            }
        }
        let after = self.fop.state();
        if after != before {
            out.app_event(FOP1_STATE, &[state_number(after)]);
        }
        true
    }

    /// Holds the frame of a transmit request until a slot (§4.5.1). A second request of a kind
    /// that is still held, or a Type-BD request, is not something FOP-1 returns here.
    fn hold(&mut self, kind: FrameKind, octets: &[u8], out: &mut dyn Output) {
        let held = self.ad.as_mut().and_then(|ad| match kind {
            FrameKind::Ad => Some(&mut ad.sequence_controlled),
            FrameKind::Bc => Some(&mut ad.control),
            FrameKind::Bd => None,
        });
        match held {
            Some(place) if place.is_none() => *place = Some(octets.to_vec()),
            Some(_) | None => self.refuse(out, STAGE_FOP1, Error::new(ErrorKind::InvalidState), 0),
        }
    }

    /// The notification of a transfer request (§4.4.2): the last request is answered by any
    /// notification; Reject, Positive, and Negative Confirm are `fop1_transfer`, the Accept is
    /// not an event (APP-145, §7.4).
    fn transferred(&mut self, notify: TransferNotify<u32>, out: &mut dyn Output) {
        if let Some(ad) = &mut self.ad
            && notify.handle.wrapping_add(1) == ad.next_request
        {
            ad.unanswered = false;
        }
        let notification = match notify.status {
            TransferNotification::Accept => return,
            TransferNotification::Reject => 2,
            TransferNotification::PositiveConfirm => 3,
            TransferNotification::NegativeConfirm => 4,
        };
        out.app_event(FOP1_TRANSFER, &[notification, u64::from(notify.handle)]);
    }

    /// The notification of a directive (§4.2) as `fop1_directive` with its type and qualifier.
    fn directive_notified(&mut self, notify: DirectiveNotify<u32>, out: &mut dyn Output) {
        let directive = usize::try_from(notify.handle)
            .ok()
            .and_then(|i| self.ad.as_ref().and_then(|ad| ad.directives.get(i)));
        let Some(directive) = directive.copied() else {
            return self.refuse(out, STAGE_FOP1, Error::new(ErrorKind::InternalError), 0);
        };
        let notification = match notify.status {
            DirectiveNotification::Accept => 1,
            DirectiveNotification::Reject => 2,
            DirectiveNotification::PositiveConfirm => 3,
            DirectiveNotification::NegativeConfirm => 4,
        };
        out.app_event(
            FOP1_DIRECTIVE,
            &[
                notification,
                u64::from(directive.code()),
                u64::from(directive.qualifier()),
            ],
        );
    }

    /// The segmenter's next FDU for `handle`, copied out of `area`; `None` if it holds none
    /// or refused (which raises `uplink_refused`, APP-143).
    pub(super) fn take_fdu(
        &mut self,
        handle: u32,
        qos: Qos,
        now: Instant,
        area: &mut [u8; FDU],
        out: &mut dyn Output,
    ) -> Option<Vec<u8>> {
        use aldis::link::segmenter::{SegmenterInput, SegmenterOutput};
        let request = SegmenterInput::NextFdu { handle, qos };
        let actions = match self.segmenter.step(request, now, area) {
            Ok(actions) => actions,
            Err(error) => {
                self.refuse(out, STAGE_NEXT_FDU, error, 0);
                return None;
            }
        };
        let fdu = actions.iter().find_map(|action| match action {
            SegmenterOutput::Fdu(fdu) => Some(*fdu),
            SegmenterOutput::Discarded { .. } => None,
        })?;
        let Some(octets) = fdu.fdu.slice(area) else {
            // Unreachable by construction: the segmenter's FDU lies within the area it was
            // given. Kept as a refusal so that no index or unwrap is needed (GEN-020).
            self.refuse(out, STAGE_NEXT_FDU, Error::new(ErrorKind::OutOfRange), 0);
            return None;
        };
        Some(octets.to_vec())
    }
}

/// `fop1_alert` (§7.4): reason, event number, and the state before.
fn alert_event(alert: Alert, out: &mut dyn Output) {
    let reason = match alert.reason {
        AlertReason::Limit => 1,
        AlertReason::T1 => 2,
        AlertReason::Lockout => 3,
        AlertReason::Synch => 4,
        AlertReason::NnR => 5,
        AlertReason::Clcw => 6,
        AlertReason::Llif => 7,
        AlertReason::Term => 8,
    };
    out.app_event(
        FOP1_ALERT,
        &[
            reason,
            event_number(alert.event),
            state_number(alert.prior_state),
        ],
    );
}

/// S1 to S6 as 1 to 6 (§7.4).
const fn state_number(state: Fop1State) -> u64 {
    match state {
        Fop1State::Active => 1,
        Fop1State::RetransmitWithoutWait => 2,
        Fop1State::RetransmitWithWait => 3,
        Fop1State::InitializingWithoutBcFrame => 4,
        Fop1State::InitializingWithBcFrame => 5,
        Fop1State::Initial => 6,
    }
}

/// The number of the event in Table 5-1: 1 to 46, 101 to 104 (§7.4).
const fn event_number(event: Fop1Event) -> u64 {
    use Fop1Event::{
        E1, E2, E3, E4, E5, E6, E7, E8, E9, E10, E11, E12, E13, E14, E15, E16, E17, E18, E19, E20,
        E21, E22, E23, E24, E25, E26, E27, E28, E29, E30, E31, E32, E33, E34, E35, E36, E37, E38,
        E39, E40, E41, E42, E43, E44, E45, E46, E101, E102, E103, E104,
    };
    match event {
        E1 => 1,
        E2 => 2,
        E3 => 3,
        E4 => 4,
        E5 => 5,
        E6 => 6,
        E7 => 7,
        E8 => 8,
        E9 => 9,
        E10 => 10,
        E11 => 11,
        E12 => 12,
        E13 => 13,
        E14 => 14,
        E15 => 15,
        E16 => 16,
        E17 => 17,
        E18 => 18,
        E19 => 19,
        E20 => 20,
        E21 => 21,
        E22 => 22,
        E23 => 23,
        E24 => 24,
        E25 => 25,
        E26 => 26,
        E27 => 27,
        E28 => 28,
        E29 => 29,
        E30 => 30,
        E31 => 31,
        E32 => 32,
        E33 => 33,
        E34 => 34,
        E35 => 35,
        E36 => 36,
        E37 => 37,
        E38 => 38,
        E39 => 39,
        E40 => 40,
        E41 => 41,
        E42 => 42,
        E43 => 43,
        E44 => 44,
        E45 => 45,
        E46 => 46,
        E101 => 101,
        E102 => 102,
        E103 => 103,
        E104 => 104,
    }
}

#[cfg(test)]
mod tests;
