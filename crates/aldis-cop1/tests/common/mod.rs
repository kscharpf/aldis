//! A FOP-1 engine together with its store, so that the tests keep one value per engine. Not part
//! of the API: it is included by `#[path]` into the unit tests of `src/lib.rs` (as `rig`), the
//! integration tests of this directory, `exerciser/src/fop1.rs`, and
//! `fuzz/fuzz_targets/cop1_events.rs`, so an edit here touches all of them.

use core::ops::Deref;

use aldis_cop1::{Fop1, Fop1Actions, Fop1Config, Fop1Decision, Fop1Format, Fop1Input, Fop1Store};
use aldis_core::Result;
use aldis_core::time::Instant;
use aldis_tc::TcFormat;

/// The engine and its store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rig<const Q: usize, const F: usize, const N: usize, P: Fop1Format = TcFormat> {
    pub(crate) fop: Fop1<Q, F, N, P>,
    pub(crate) store: Fop1Store<Q, F>,
}

impl<const Q: usize, const F: usize, const N: usize, P: Fop1Format> Rig<Q, F, N, P> {
    pub(crate) fn new(config: Fop1Config<P>) -> Result<Rig<Q, F, N, P>> {
        Ok(Rig {
            fop: Fop1::new(config)?,
            store: Fop1Store::new(),
        })
    }

    pub(crate) fn decide<'a>(
        &self,
        input: Fop1Input<'a>,
        now: Instant,
        out: &mut [u8],
    ) -> Result<Fop1Decision<'a, Q, F, N, P>> {
        self.fop.decide(&self.store, input, now, out)
    }

    pub(crate) fn commit(&mut self, decision: Fop1Decision<'_, Q, F, N, P>) -> Fop1Actions<N> {
        self.fop.commit(&mut self.store, decision)
    }
}

impl<const Q: usize, const F: usize, const N: usize, P: Fop1Format> Deref for Rig<Q, F, N, P> {
    type Target = Fop1<Q, F, N, P>;

    fn deref(&self) -> &Fop1<Q, F, N, P> {
        &self.fop
    }
}
