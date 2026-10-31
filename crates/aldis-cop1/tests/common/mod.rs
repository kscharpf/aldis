//! A FOP-1 engine together with its store, so that the tests, the exerciser, and the fuzz
//! target keep one value per engine. Not part of the API (shared by `#[path]`).

use core::ops::Deref;

use aldis_cop1::{Fop1, Fop1Actions, Fop1Config, Fop1Decision, Fop1Input, Fop1Store};
use aldis_core::Result;
use aldis_core::time::Instant;

/// The engine and its store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rig<const Q: usize, const F: usize, const N: usize> {
    pub(crate) fop: Fop1<Q, F, N>,
    pub(crate) store: Fop1Store<Q, F>,
}

impl<const Q: usize, const F: usize, const N: usize> Rig<Q, F, N> {
    pub(crate) fn new(config: Fop1Config) -> Result<Rig<Q, F, N>> {
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
    ) -> Result<Fop1Decision<'a, Q, F, N>> {
        self.fop.decide(&self.store, input, now, out)
    }

    pub(crate) fn commit(&mut self, decision: Fop1Decision<'_, Q, F, N>) -> Fop1Actions<N> {
        self.fop.commit(&mut self.store, decision)
    }
}

impl<const Q: usize, const F: usize, const N: usize> Deref for Rig<Q, F, N> {
    type Target = Fop1<Q, F, N>;

    fn deref(&self) -> &Fop1<Q, F, N> {
        &self.fop
    }
}
