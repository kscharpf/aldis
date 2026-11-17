//! The commit phase: applying a decision to the engine and the store in place (API-058, RS-022).
//!
//! The body of [`Fop1::commit`], which keeps its signature and documentation in `fop1.rs`.

use aldis_tc::FrameType;

use super::{
    FduSource, Fop1, Fop1Actions, Fop1Decision, Fop1Format, Fop1Store, Origin, Payload, Waiting,
};

impl<const Q: usize, const F: usize, const N: usize, P: Fop1Format> Fop1<Q, F, N, P> {
    /// Applies `decision` to the state in place and returns its actions; the body of
    /// [`commit`](Self::commit) (API-058, RS-022).
    pub(super) fn apply(
        &mut self,
        store: &mut Fop1Store<Q, F>,
        decision: Fop1Decision<'_, Q, F, N, P>,
    ) -> Fop1Actions<N> {
        let Fop1Decision {
            mut core,
            actions,
            fdu,
            ..
        } = decision;
        // The queue keeps its order, so an entry never moves to a later position, and the
        // positions in ascending order read each master copy before its position is written.
        for (p, sent) in core.sent.iter().enumerate() {
            let (_, rest) = store.frames.split_at_mut(p);
            let Some((slot, later)) = rest.split_first_mut() else {
                continue;
            };
            let Some(sent) = sent else {
                slot.fill(0);
                continue;
            };
            let octets = match sent.origin {
                Origin::Base(i) => {
                    let moved = i.checked_sub(p).and_then(|d| d.checked_sub(1));
                    if let Some(source) = moved.and_then(|d| later.get(d)) {
                        *slot = *source;
                    }
                    continue;
                }
                Origin::FreshAd(FduSource::Input) => Some(fdu),
                Origin::FreshAd(FduSource::Wait(len)) => store.wait_fdu.get(..len),
                Origin::FreshBc(_) => None,
            };
            slot.fill(0);
            let (frame_type, payload) = match (sent.origin, octets) {
                (Origin::FreshBc(command), _) => (FrameType::Bc, Payload::Control(command)),
                (_, Some(octets)) => (FrameType::Ad, Payload::Fdu(octets)),
                (_, None) => continue,
            };
            // The decision encoded the same frame into the output area, so this cannot fail.
            self.target
                .encode(frame_type, sent.sequence_number, payload, slot)
                .ok();
        }
        match core.wait {
            None => store.wait_fdu.fill(0),
            Some(Waiting { fresh: true, .. }) => {
                store.wait_fdu.fill(0);
                for (d, s) in store.wait_fdu.iter_mut().zip(fdu) {
                    *d = *s;
                }
            }
            Some(Waiting { fresh: false, .. }) => {}
        }
        // At rest, an entry's octets are at its own position and the `Wait_Queue` octets are in
        // place.
        for (p, sent) in core.sent.iter_mut().enumerate() {
            if let Some(sent) = sent {
                sent.origin = Origin::Base(p);
            }
        }
        if let Some(waiting) = &mut core.wait {
            waiting.fresh = false;
        }
        self.core = core;
        actions
    }
}
