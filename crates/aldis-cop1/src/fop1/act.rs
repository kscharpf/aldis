//! The action interpreter: the actions of a cell of Table 5-1 on a decision (232.1 §5.2; COP-001,
//! COP-003, COP-004, COP-006, COP-010, COP-011, COP-013, API-057, API-061, API-083).
//!
//! The second phase of [`Fop1::decide`], run by `Draft::run` for each action token of the
//! cell. It changes the small state of the decision and appends its actions; the engine and the
//! store are only read.

use aldis_core::engine::OutRange;
use aldis_core::ids::FrameSeqNum;
use aldis_core::time::Instant;
use aldis_core::{Error, ErrorKind, Result};
use aldis_tc::ControlCommand;

use super::{
    Base, Context, Draft, FduSource, Fop1Format, Fop1Output, Handle, Origin, OutArea, Payload,
    Sent, SentKind, Waiting,
};
use crate::fop1_table::{BcCommand, Fop1Action, Fop1Event, Fop1State, FrameKind, SuspendState};
use crate::notify::{
    Alert, AlertReason, Directive, DirectiveNotification, DirectiveNotify, TransferNotification,
    TransferNotify,
};

impl<const Q: usize, const F: usize, const N: usize, P: Fop1Format> Draft<'_, Q, F, N, P> {
    /// Executes one action token of a cell (232.1 §5.2).
    #[expect(clippy::too_many_lines, reason = "one arm per action token of §5.2")]
    pub(super) fn act(
        &mut self,
        base: &Base<'_, Q, F, P>,
        action: Fop1Action,
        prior: Fop1State,
        ctx: &Context<'_>,
        out: &mut OutArea<'_>,
    ) -> Result<()> {
        match action {
            Fop1Action::CancelTimer => self.core.deadline = None,
            Fop1Action::StartTimer => self.start_timer(ctx.now),
            Fop1Action::ReleaseBcCopy => self.remove_sent(|s| matches!(s.kind, SentKind::Bc)),
            Fop1Action::RemoveAckedFrames => self.remove_acked(ctx.n_r)?,
            Fop1Action::LookForFdu => self.look_for_fdu(base, ctx.now, out)?,
            Fop1Action::LookForDirective => self.look_for_directive(base, out)?,
            Fop1Action::InitiateAdRetransmission | Fop1Action::InitiateBcRetransmission => {
                // §5.2.7: Abort, count, timer, then the marks.
                self.actions.push(Fop1Output::Abort)?;
                self.core.transmission_count = self.core.transmission_count.saturating_add(1);
                self.start_timer(ctx.now);
                let bc = matches!(action, Fop1Action::InitiateBcRetransmission);
                for sent in self.core.sent.iter_mut().flatten() {
                    if matches!(sent.kind, SentKind::Bc) == bc {
                        sent.to_be_retransmitted = true;
                    }
                }
            }
            Fop1Action::AddToWaitQueue => {
                if self.core.wait.is_some() {
                    return Err(Error::new(ErrorKind::InternalError));
                }
                self.core.wait = Some(Waiting {
                    handle: ctx.handle.ok_or(Error::new(ErrorKind::InvalidState))?,
                    len: ctx.fdu.len(),
                    fresh: true,
                });
            }
            Fop1Action::TransmitBd => {
                // §5.2.6: `BD_Out_Flag`, then the request; no master copy, no timer.
                let handle = ctx.handle.ok_or(Error::new(ErrorKind::InvalidState))?;
                let range =
                    base.target
                        .encode_at(FrameSeqNum::new(0), Payload::Bd(ctx.fdu), out)?;
                self.core.bd_out = Some(handle);
                self.actions.push(Fop1Output::Transmit {
                    kind: FrameKind::Bd,
                    sequence_number: FrameSeqNum::new(0),
                    retransmission: false,
                    repetitions: 0,
                    frame: range,
                })?;
            }
            Fop1Action::TransmitBc(command) => {
                self.transmit_bc(base, command, ctx, out)?;
            }
            Fop1Action::Initialize => {
                // §5.2.14.
                self.purge_sent()?;
                self.purge_wait()?;
                self.core.transmission_count = 1;
                self.core.suspend_state = SuspendState::NotSuspended;
            }
            Fop1Action::SetVsFromVr => {
                let Some(Directive::InitiateAdWithSetVr { v_r }) = ctx.directive else {
                    return Err(Error::new(ErrorKind::InvalidState));
                };
                self.core.v_s = v_r;
                self.core.nn_r = v_r;
            }
            Fop1Action::SetVs => {
                // Table 5-1 E35 in S6: only while the AD Service is not suspended.
                let handle = ctx.handle.ok_or(Error::new(ErrorKind::InvalidState))?;
                let Some(Directive::SetVs { v_s }) = ctx.directive else {
                    return Err(Error::new(ErrorKind::InvalidState));
                };
                if matches!(self.core.suspend_state, SuspendState::NotSuspended) {
                    self.actions
                        .push(directive(handle, DirectiveNotification::Accept))?;
                    self.core.v_s = v_s;
                    self.core.nn_r = v_s;
                    self.actions
                        .push(directive(handle, DirectiveNotification::PositiveConfirm))?;
                } else {
                    self.actions
                        .push(directive(handle, DirectiveNotification::Reject))?;
                }
            }
            Fop1Action::SetSlidingWindowWidth => {
                let Some(Directive::SetSlidingWindowWidth { width }) = ctx.directive else {
                    return Err(Error::new(ErrorKind::InvalidState));
                };
                self.core.k = width;
            }
            Fop1Action::SetT1Initial => {
                let Some(Directive::SetT1Initial { duration }) = ctx.directive else {
                    return Err(Error::new(ErrorKind::InvalidState));
                };
                self.core.t1_initial = duration;
            }
            Fop1Action::SetTransmissionLimit => {
                let Some(Directive::SetTransmissionLimit { limit }) = ctx.directive else {
                    return Err(Error::new(ErrorKind::InvalidState));
                };
                self.core.transmission_limit = limit;
            }
            Fop1Action::SetTimeoutType => {
                let Some(Directive::SetTimeoutType { timeout_type }) = ctx.directive else {
                    return Err(Error::new(ErrorKind::InvalidState));
                };
                self.core.timeout_type = timeout_type;
            }
            Fop1Action::SetSuspendState(ss) => self.core.suspend_state = ss,
            Fop1Action::Suspend => self.actions.push(Fop1Output::Suspend)?,
            Fop1Action::Resume => {
                // §5.2.17.
                self.start_timer(ctx.now);
                self.core.suspend_state = SuspendState::NotSuspended;
            }
            Fop1Action::SetOutFlagReady(kind) => match kind {
                FrameKind::Ad => self.core.ad_out = None,
                FrameKind::Bc => self.core.bc_out_ready = true,
                FrameKind::Bd => self.core.bd_out = None,
            },
            Fop1Action::Alert(reason) => self.alert(reason, prior, ctx.event)?,
            Fop1Action::TransferNotify(status) => {
                if let Some(handle) = ctx.handle {
                    self.actions
                        .push(Fop1Output::Transfer(TransferNotify { handle, status }))?;
                }
            }
            Fop1Action::DirectiveNotify(status) => {
                if is_directive(ctx.event) {
                    let handle = ctx.handle.ok_or(Error::new(ErrorKind::InvalidState))?;
                    self.actions.push(directive(handle, status))?;
                } else if let Some(handle) = self.core.pending_initiate.take() {
                    // The Confirm of S4 and S5 with E1 completes the Initiate AD directive.
                    self.actions.push(directive(handle, status))?;
                } else {
                    // No Initiate AD directive is pending: nothing to notify.
                }
            }
        }
        Ok(())
    }

    /// Starts or restarts the timer with `T1_Initial` (232.1 §5.1.9.1; COP-013).
    pub(super) const fn start_timer(&mut self, now: Instant) {
        self.core.deadline = Some(now.saturating_add(self.core.t1_initial));
    }

    /// Removes the entries for which `remove` holds, keeping the order of the others.
    fn remove_sent(&mut self, remove: impl Fn(&Sent) -> bool) {
        let mut kept = [None; Q];
        for (slot, sent) in kept
            .iter_mut()
            .zip(self.core.sent.iter().flatten().filter(|s| !remove(s)))
        {
            *slot = Some(*sent);
        }
        self.core.sent = kept;
    }

    /// Remove Acknowledged Frames from `Sent_Queue` (232.1 §5.2.8): a Positive Confirm for each
    /// Type-AD frame N(R) acknowledges, in queue order, NN(R) := N(R), and
    /// `Transmission_Count` := 1.
    fn remove_acked(&mut self, n_r: FrameSeqNum) -> Result<()> {
        let count = usize::from(n_r.get().wrapping_sub(self.core.nn_r.get()));
        for handle in self
            .core
            .sent
            .iter()
            .flatten()
            .take(count)
            .filter_map(Sent::handle)
        {
            self.actions
                .push(transfer(handle, TransferNotification::PositiveConfirm))?;
        }
        let mut kept = [None; Q];
        for (slot, sent) in kept
            .iter_mut()
            .zip(self.core.sent.iter().flatten().skip(count))
        {
            *slot = Some(*sent);
        }
        self.core.sent = kept;
        self.core.nn_r = n_r;
        self.core.transmission_count = 1;
        Ok(())
    }

    /// Purging the `Sent_Queue` (232.1 §5.2.2): a Negative Confirm for each Type-AD frame, in
    /// queue order; the Type-BC frame goes without one. NN(R) := V(S) (OI-60).
    fn purge_sent(&mut self) -> Result<()> {
        for handle in self.core.sent.iter().flatten().filter_map(Sent::handle) {
            self.actions
                .push(transfer(handle, TransferNotification::NegativeConfirm))?;
        }
        self.core.sent = [None; Q];
        self.core.nn_r = self.core.v_s;
        Ok(())
    }

    /// Purging the `Wait_Queue` (232.1 §5.2.3): a Reject for the queued FDU.
    fn purge_wait(&mut self) -> Result<()> {
        if let Some(waiting) = self.core.wait.take() {
            self.actions
                .push(transfer(waiting.handle, TransferNotification::Reject))?;
        }
        Ok(())
    }

    /// Alert (232.1 §5.2.15): cancel the timer, purge both queues, complete a pending
    /// Initiate AD directive with a Negative Confirm, and notify (API-083).
    fn alert(&mut self, reason: AlertReason, prior: Fop1State, event: Fop1Event) -> Result<()> {
        self.core.deadline = None;
        self.purge_sent()?;
        self.purge_wait()?;
        if let Some(handle) = self.core.pending_initiate.take() {
            self.actions
                .push(directive(handle, DirectiveNotification::NegativeConfirm))?;
        }
        self.actions.push(Fop1Output::Alert(Alert {
            reason,
            event,
            prior_state: prior,
        }))?;
        Ok(())
    }

    /// Look for FDU (232.1 §5.2.10): with `AD_Out_Flag` Ready, the first Type-AD frame marked
    /// `To_Be_Retransmitted`, else the FDU of the `Wait_Queue` if V(S) < NN(R) + K.
    fn look_for_fdu(
        &mut self,
        base: &Base<'_, Q, F, P>,
        now: Instant,
        out: &mut OutArea<'_>,
    ) -> Result<()> {
        if self.core.ad_out.is_some() {
            return Ok(());
        }
        let repetitions = self.core.ad_repetitions.get();
        if let Some(sent) = self
            .core
            .sent
            .iter_mut()
            .flatten()
            .find(|s| matches!(s.kind, SentKind::Ad(_)) && s.to_be_retransmitted)
        {
            sent.to_be_retransmitted = false;
            let snapshot = *sent;
            let range = self.emit_sent(base, &snapshot, out)?;
            self.core.ad_out = Some(snapshot.sequence_number);
            self.actions.push(Fop1Output::Transmit {
                kind: FrameKind::Ad,
                sequence_number: snapshot.sequence_number,
                retransmission: true,
                repetitions,
                frame: range,
            })?;
            return Ok(());
        }
        let outstanding = self.core.v_s.get().wrapping_sub(self.core.nn_r.get());
        if outstanding >= self.core.k.get() {
            return Ok(());
        }
        let Some(waiting) = self.core.wait.take() else {
            return Ok(());
        };
        self.actions
            .push(transfer(waiting.handle, TransferNotification::Accept))?;
        self.transmit_ad(base, waiting, now, out)
    }

    /// Transmit Type-AD Frame (232.1 §5.2.4): N(S) := V(S), V(S) + 1, the master copy on the
    /// `Sent_Queue`, `Transmission_Count` := 1 if the queue was empty, the timer, `AD_Out_Flag`,
    /// and the request. The frame is encoded into the output area; [`Fop1Decision::commit`] encodes it
    /// again into the master copy.
    fn transmit_ad(
        &mut self,
        base: &Base<'_, Q, F, P>,
        waiting: Waiting,
        now: Instant,
        out: &mut OutArea<'_>,
    ) -> Result<()> {
        let sequence_number = self.core.v_s;
        let source = if waiting.fresh {
            FduSource::Input
        } else {
            FduSource::Wait(waiting.len)
        };
        let range = {
            let fdu = self.fdu_of(base, source)?;
            base.target
                .encode_at(sequence_number, Payload::Ad(fdu), out)?
        };
        self.push_sent(Sent {
            origin: Origin::FreshAd(source),
            len: range.len(),
            sequence_number,
            kind: SentKind::Ad(waiting.handle),
            to_be_retransmitted: false,
        })?;
        self.core.v_s = self.core.v_s.next();
        self.start_timer(now);
        self.core.ad_out = Some(sequence_number);
        self.actions.push(Fop1Output::Transmit {
            kind: FrameKind::Ad,
            sequence_number,
            retransmission: false,
            repetitions: self.core.ad_repetitions.get(),
            frame: range,
        })
    }

    /// Transmit Type-BC Frame (232.1 §5.2.5): the master copy on the `Sent_Queue`,
    /// `Transmission_Count` := 1, the timer, `BC_Out_Flag`, and the request. The frame carries
    /// Unlock, or Set V(R) with the V*(R) of the directive, which E27 has just put in V(S)
    /// (232.0 §4.1.3.3).
    fn transmit_bc(
        &mut self,
        base: &Base<'_, Q, F, P>,
        command: BcCommand,
        ctx: &Context<'_>,
        out: &mut OutArea<'_>,
    ) -> Result<()> {
        let command = match command {
            BcCommand::Unlock => ControlCommand::Unlock,
            // The one cell that lists it (E27) sets V(S) := V*(R) first.
            BcCommand::SetVr => ControlCommand::SetVr(self.core.v_s),
        };
        let sequence_number = FrameSeqNum::new(0);
        let range = base
            .target
            .encode_at(sequence_number, Payload::Bc(command), out)?;
        self.push_sent(Sent {
            origin: Origin::FreshBc(command),
            len: range.len(),
            sequence_number,
            kind: SentKind::Bc,
            to_be_retransmitted: false,
        })?;
        self.core.transmission_count = 1;
        self.start_timer(ctx.now);
        self.core.bc_out_ready = false;
        self.actions.push(Fop1Output::Transmit {
            kind: FrameKind::Bc,
            sequence_number,
            retransmission: false,
            repetitions: self.core.bc_repetitions.get(),
            frame: range,
        })
    }

    /// Look for Directive (232.1 §5.2.9): with `BC_Out_Flag` Ready, the Type-BC frame if it is
    /// marked `To_Be_Retransmitted`.
    fn look_for_directive(
        &mut self,
        base: &Base<'_, Q, F, P>,
        out: &mut OutArea<'_>,
    ) -> Result<()> {
        if !self.core.bc_out_ready {
            return Ok(());
        }
        let repetitions = self.core.bc_repetitions.get();
        if let Some(sent) = self
            .core
            .sent
            .iter_mut()
            .flatten()
            .find(|s| matches!(s.kind, SentKind::Bc) && s.to_be_retransmitted)
        {
            sent.to_be_retransmitted = false;
            let snapshot = *sent;
            let range = self.emit_sent(base, &snapshot, out)?;
            self.core.bc_out_ready = false;
            self.actions.push(Fop1Output::Transmit {
                kind: FrameKind::Bc,
                sequence_number: snapshot.sequence_number,
                retransmission: true,
                repetitions,
                frame: range,
            })?;
        }
        Ok(())
    }

    /// Appends `sent` to the `Sent_Queue`.
    ///
    /// # Errors
    /// `InternalError` if the queue is full: K ≤ `Q` keeps a free entry for every frame the
    /// window admits (COP-015), so this does not happen.
    fn push_sent(&mut self, sent: Sent) -> Result<()> {
        let slot = self
            .core
            .sent
            .iter_mut()
            .find(|s| s.is_none())
            .ok_or(Error::new(ErrorKind::InternalError))?;
        *slot = Some(sent);
        Ok(())
    }

    /// The octets of an FDU a decision builds a frame from.
    fn fdu_of<'s>(&'s self, base: &'s Base<'_, Q, F, P>, source: FduSource) -> Result<&'s [u8]> {
        match source {
            FduSource::Input => Ok(self.fdu),
            FduSource::Wait(len) => base
                .store
                .wait_fdu
                .get(..len)
                .ok_or(Error::new(ErrorKind::InternalError)),
        }
    }

    /// Writes the frame of `sent` into the output area after the octets it holds and returns its
    /// range (API-061): the master copy of the engine, or, for an entry this decision added,
    /// the frame encoded again.
    pub(super) fn emit_sent(
        &self,
        base: &Base<'_, Q, F, P>,
        sent: &Sent,
        out: &mut OutArea<'_>,
    ) -> Result<OutRange> {
        let payload = match sent.origin {
            Origin::Base(i) => {
                let frame = base
                    .store
                    .frames
                    .get(i)
                    .and_then(|f| f.get(..sent.len))
                    .ok_or(Error::new(ErrorKind::InternalError))?;
                return out.emit(frame);
            }
            Origin::FreshAd(source) => Payload::Ad(self.fdu_of(base, source)?),
            Origin::FreshBc(command) => Payload::Bc(command),
        };
        base.target.encode_at(sent.sequence_number, payload, out)
    }
}

/// Whether `event` is a directive event, E23 to E40 (232.1 Table 5-1).
const fn is_directive(event: Fop1Event) -> bool {
    matches!(
        event,
        Fop1Event::E23
            | Fop1Event::E24
            | Fop1Event::E25
            | Fop1Event::E26
            | Fop1Event::E27
            | Fop1Event::E28
            | Fop1Event::E29
            | Fop1Event::E30
            | Fop1Event::E31
            | Fop1Event::E32
            | Fop1Event::E33
            | Fop1Event::E34
            | Fop1Event::E35
            | Fop1Event::E36
            | Fop1Event::E37
            | Fop1Event::E38
            | Fop1Event::E39
            | Fop1Event::E40
    )
}

/// The transfer notification `status` for `handle`.
const fn transfer(handle: Handle, status: TransferNotification) -> Fop1Output {
    Fop1Output::Transfer(TransferNotify { handle, status })
}

/// The directive notification `status` for `handle`.
const fn directive(handle: Handle, status: DirectiveNotification) -> Fop1Output {
    Fop1Output::Directive(DirectiveNotify { handle, status })
}
