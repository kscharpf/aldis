//! The commit phase: applying a decision to the engine in place (API-058, RS-022).
//!
//! [`Fop1Decision::commit`], the one method of the decision that writes the engine, its head and
//! its store.

use super::{FduSource, Fop1Actions, Fop1Decision, Fop1Format, Origin, Payload, Waiting};

impl<const Q: usize, const F: usize, const N: usize, P: Fop1Format>
    Fop1Decision<'_, '_, Q, F, N, P>
{
    /// Applies the decision to the engine it borrows, in place, and returns its actions (232.1
    /// §5.3.1; COP-011, API-058, RS-022).
    ///
    /// It cannot fail: [`Fop1::decide`](super::Fop1::decide) did every check, and the borrow the
    /// decision holds keeps the engine as `decide` left it. It takes no engine and no store: the
    /// decision commits to the engine that made it, and to that engine's own store (RS-021).
    pub fn commit(self) -> Fop1Actions<N> {
        // The draft is bound whole and its fields are read and written through that binding,
        // not destructured into separate locals (VER-011).
        let Fop1Decision { engine, mut draft } = self;
        let fdu = draft.fdu;
        let store = &mut engine.store;
        // The queue keeps its order, so an entry never moves to a later position, and the
        // positions in ascending order read each master copy before its position is written.
        for (p, sent) in draft.core.sent.iter().enumerate() {
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
            let payload = match (sent.origin, octets) {
                (Origin::FreshBc(command), _) => Payload::Bc(command),
                (_, Some(octets)) => Payload::Ad(octets),
                (_, None) => continue,
            };
            // The decision encoded the same frame into the output area, so this cannot fail.
            engine
                .head
                .target
                .encode(sent.sequence_number, payload, slot)
                .ok();
        }
        match draft.core.wait {
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
        for (p, sent) in draft.core.sent.iter_mut().enumerate() {
            if let Some(sent) = sent {
                sent.origin = Origin::Base(p);
            }
        }
        if let Some(waiting) = &mut draft.core.wait {
            waiting.fresh = false;
        }
        engine.head.core = draft.core;
        draft.actions
    }
}
