//! Classification of an input into its event and context (232.1 §5.3.2; COP-012, COP-014,
//! COP-015, API-053, API-057).
//!
//! The first phase of [`Fop1::decide`]: an [`Fop1Input`] becomes the [`Fop1Event`] of Table 5-1
//! and the [`Context`] its cell reads. Nothing here changes the state.

use aldis_core::time::Instant;
use aldis_core::{Error, ErrorKind, Result, field};
use aldis_tc::{Clcw, CopInEffect};

use super::{Context, Fop1, Fop1Format, Fop1Head, Fop1Input};
use crate::fop1_table::{Fop1Event, FrameKind, SuspendState};
use crate::notify::{Directive, TimeoutType};

impl<const Q: usize, const F: usize, const N: usize, P: Fop1Format> Fop1<Q, F, N, P> {
    /// The event of a CLCW (232.1 §5.3.2.1, Table 5-1; COP-012).
    ///
    /// # Errors
    /// `UnknownChannel` (`vc_id`) for a CLCW of another Virtual Channel (cop1.md §2.2).
    pub(super) fn clcw_event(&self, clcw: &Clcw) -> Result<Fop1Event> {
        if clcw.vc_id.get() != self.head.target.clcw_vc_id {
            return Err(Error::new(ErrorKind::UnknownChannel).with_field(field::VC_ID));
        }
        if clcw.cop_in_effect != CopInEffect::COP_1 {
            return Ok(Fop1Event::E15);
        }
        if clcw.lockout {
            return Ok(Fop1Event::E14);
        }
        let n_r = clcw.report_value;
        let outstanding = self
            .head
            .core
            .v_s
            .get()
            .wrapping_sub(self.head.core.nn_r.get());
        let acked = n_r.get().wrapping_sub(self.head.core.nn_r.get());
        let first = n_r == self.head.core.nn_r;
        let (retransmit, wait) = (clcw.retransmit, clcw.wait);
        let event = if acked > outstanding {
            Fop1Event::E13
        } else if acked == outstanding {
            match (retransmit, wait, first) {
                (true, _, _) => Fop1Event::E4,
                (false, true, _) => Fop1Event::E3,
                (false, false, true) => Fop1Event::E1,
                (false, false, false) => Fop1Event::E2,
            }
        } else if !retransmit {
            match (wait, first) {
                (true, _) => Fop1Event::E7,
                (false, true) => Fop1Event::E5,
                (false, false) => Fop1Event::E6,
            }
        } else if self.head.core.transmission_limit.get() == 1 {
            if first {
                Fop1Event::E102
            } else {
                Fop1Event::E101
            }
        } else if !first {
            if wait { Fop1Event::E9 } else { Fop1Event::E8 }
        } else if self.head.core.transmission_count < self.head.core.transmission_limit.get() {
            if wait { Fop1Event::E11 } else { Fop1Event::E10 }
        } else if wait {
            Fop1Event::E103
        } else {
            Fop1Event::E12
        };
        Ok(event)
    }

    /// The event of a directive (232.1 Table 4-1, Table 5-1; COP-014, COP-015).
    const fn directive_event(&self, directive: Directive) -> Fop1Event {
        match directive {
            Directive::InitiateAdWithoutClcwCheck => Fop1Event::E23,
            Directive::InitiateAdWithClcwCheck => Fop1Event::E24,
            Directive::InitiateAdWithUnlock => {
                if self.head.core.bc_out_ready {
                    Fop1Event::E25
                } else {
                    Fop1Event::E26
                }
            }
            Directive::InitiateAdWithSetVr { .. } => {
                if self.head.core.bc_out_ready {
                    Fop1Event::E27
                } else {
                    Fop1Event::E28
                }
            }
            Directive::TerminateAd => Fop1Event::E29,
            Directive::ResumeAd => match self.head.core.suspend_state {
                SuspendState::NotSuspended => Fop1Event::E30,
                SuspendState::Active => Fop1Event::E31,
                SuspendState::RetransmitWithoutWait => Fop1Event::E32,
                SuspendState::RetransmitWithWait => Fop1Event::E33,
                SuspendState::InitializingWithoutBcFrame => Fop1Event::E34,
            },
            Directive::SetVs { .. } => Fop1Event::E35,
            Directive::SetSlidingWindowWidth { width } => {
                if Fop1Head::<Q, F, N, P>::k_fits(width) {
                    Fop1Event::E36
                } else {
                    Fop1Event::E40
                }
            }
            Directive::SetT1Initial { duration } => {
                if duration.as_micros() == 0 {
                    Fop1Event::E40
                } else {
                    Fop1Event::E37
                }
            }
            Directive::SetTransmissionLimit { .. } => Fop1Event::E38,
            Directive::SetTimeoutType { .. } => Fop1Event::E39,
        }
    }

    /// The event of `input` and the context its cell reads; `None` for a Timeout before the
    /// deadline, which changes nothing (API-053).
    pub(super) fn classify<'a>(
        &self,
        input: Fop1Input<'a>,
        now: Instant,
    ) -> Result<Option<(Fop1Event, Context<'a>)>> {
        let mut ctx = Context {
            event: Fop1Event::E1,
            now,
            handle: None,
            fdu: &[],
            directive: None,
            n_r: self.head.core.nn_r,
        };
        let event = match input {
            Fop1Input::Clcw(clcw) => {
                ctx.n_r = clcw.report_value;
                self.clcw_event(&clcw)?
            }
            Fop1Input::InvalidClcw => Fop1Event::E15,
            Fop1Input::Timeout => {
                let Some(deadline) = self.head.core.deadline else {
                    return Ok(None);
                };
                if now < deadline {
                    return Ok(None);
                }
                let below =
                    self.head.core.transmission_count < self.head.core.transmission_limit.get();
                match (below, self.head.core.timeout_type) {
                    (true, TimeoutType::Tt0) => Fop1Event::E16,
                    (true, TimeoutType::Tt1) => Fop1Event::E104,
                    (false, TimeoutType::Tt0) => Fop1Event::E17,
                    (false, TimeoutType::Tt1) => Fop1Event::E18,
                }
            }
            Fop1Input::RequestAd { handle, fdu } => {
                self.check_fdu(fdu)?;
                ctx.handle = Some(handle);
                ctx.fdu = fdu;
                if self.head.core.wait.is_none() {
                    Fop1Event::E19
                } else {
                    Fop1Event::E20
                }
            }
            Fop1Input::RequestBd { handle, fdu } => {
                self.check_fdu(fdu)?;
                ctx.handle = Some(handle);
                ctx.fdu = fdu;
                if self.head.core.bd_out.is_none() {
                    Fop1Event::E21
                } else {
                    Fop1Event::E22
                }
            }
            Fop1Input::Directive { handle, directive } => {
                ctx.handle = Some(handle);
                ctx.directive = Some(directive);
                self.directive_event(directive)
            }
            Fop1Input::InvalidDirective { handle } => {
                ctx.handle = Some(handle);
                Fop1Event::E40
            }
            Fop1Input::TransmitDone { kind, accepted } => match (kind, accepted) {
                (FrameKind::Ad, true) => Fop1Event::E41,
                (FrameKind::Ad, false) => Fop1Event::E42,
                (FrameKind::Bc, true) => Fop1Event::E43,
                (FrameKind::Bc, false) => Fop1Event::E44,
                (FrameKind::Bd, true) => {
                    ctx.handle = self.head.core.bd_out;
                    Fop1Event::E45
                }
                (FrameKind::Bd, false) => Fop1Event::E46,
            },
        };
        ctx.event = event;
        Ok(Some((event, ctx)))
    }

    /// `InvalidField` (`frame_data_field`) for an empty FDU, `OutOfRange` (`frame_len`) for
    /// one longer than a frame of the target carries (232.0 §4.1.3; CMN-041, TC-007).
    pub(super) const fn check_fdu(&self, fdu: &[u8]) -> Result<()> {
        if fdu.is_empty() {
            return Err(Error::new(ErrorKind::InvalidField).with_field(field::FRAME_DATA_FIELD));
        }
        if fdu.len() > self.head.target.max_fdu_len {
            return Err(Error::new(ErrorKind::OutOfRange).with_field(field::FRAME_LEN));
        }
        Ok(())
    }
}
