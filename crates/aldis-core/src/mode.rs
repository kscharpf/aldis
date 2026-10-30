//! Strict and lenient decoding (API-030, API-031, API-032).
//!
//! Every decoder has two entry points: `decode` is strict, and `decode_lenient` is lenient
//! (API-030). The strict one returns the view itself, which carries no anomaly flags because a
//! strict decode has none to report (API-031). The lenient one returns the view in a [`Lenient`],
//! which also holds the [`AnomalySet`] of the deviations it accepted (API-032, GEN-043). In
//! strict mode a violation of a Blue Book SHALL that is checkable from the octets and the
//! configuration is an error. In lenient mode, deviations in reserved/spare bits, fields CCSDS
//! declares "undefined", conventional-zero fields, and fixed-value fields of idle data units,
//! plus a reserved time code ID (TIME-002), are accepted and reported. Version numbers,
//! lengths, and FECF/FHEC failures are errors in both modes.

use core::ops::Deref;

use crate::anomaly::{Anomaly, AnomalySet};
use crate::error::Result;

/// How a decoder implementation treats deviations it can tolerate (API-030).
///
/// The codec crates use it to share one parsing routine between their strict and lenient entry
/// points. No public decoder takes it as an argument; the C ABI maps its `mode` argument onto
/// the two entry points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DecodeMode {
    /// Every checkable Blue Book violation is an error (API-031). The default.
    #[default]
    Strict,
    /// Tolerable deviations are accepted and reported as anomaly flags (API-032, GEN-043).
    Lenient,
}

impl DecodeMode {
    /// Runs `strict` or `lenient`, the two entry points of one decoder, as the mode says, and
    /// returns the view with its anomalies (empty after `strict`).
    ///
    /// For callers that carry the mode as a value, such as the C ABI and the test vector runners
    /// (API-030). Code that knows its mode calls the entry point.
    ///
    /// # Errors
    /// Those of the entry point that ran.
    pub fn dispatch<V, A: Anomaly>(
        self,
        strict: impl FnOnce() -> Result<V>,
        lenient: impl FnOnce() -> Result<Lenient<V, A>>,
    ) -> Result<(V, AnomalySet<A>)> {
        match self {
            DecodeMode::Strict => strict().map(|view| (view, AnomalySet::empty())),
            DecodeMode::Lenient => lenient().map(Lenient::into_parts),
        }
    }
}

/// A view that a lenient decoder accepted, with the anomalies it found (API-032, GEN-043).
///
/// It dereferences to the view, so every accessor of the strict view is available. A strict
/// decoder returns the view alone: only a lenient decode can hold anomalies (API-031).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lenient<V, A: Anomaly> {
    view: V,
    anomalies: AnomalySet<A>,
}

impl<V, A: Anomaly> Lenient<V, A> {
    /// The view `view` that a lenient decode accepted with `anomalies`.
    #[must_use]
    pub const fn new(view: V, anomalies: AnomalySet<A>) -> Lenient<V, A> {
        Lenient { view, anomalies }
    }

    /// The anomalies of the deviations the decode accepted (API-032).
    #[must_use]
    pub const fn anomalies(&self) -> AnomalySet<A> {
        self.anomalies
    }

    /// The same anomalies with the view converted by `f`, for a view that a later step refines.
    #[must_use]
    pub fn map<W>(self, f: impl FnOnce(V) -> W) -> Lenient<W, A> {
        Lenient {
            view: f(self.view),
            anomalies: self.anomalies,
        }
    }

    /// The view, without its anomalies.
    #[must_use]
    pub fn into_view(self) -> V {
        self.view
    }

    /// The view and its anomalies.
    #[must_use]
    pub fn into_parts(self) -> (V, AnomalySet<A>) {
        (self.view, self.anomalies)
    }
}

impl<V, A: Anomaly> Deref for Lenient<V, A> {
    type Target = V;

    fn deref(&self) -> &V {
        &self.view
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{DecodeMode, Lenient};
    use crate::anomaly::AnomalySet;

    crate::anomaly_flags! {
        /// Flags of a test codec.
        enum Demo {
            /// Bit 0.
            One = 0,
        }
    }

    #[test]
    fn demo_flags_are_listed() {
        // req: GEN-043
        assert_eq!(Demo::ALL, &[Demo::One]);
    }

    #[test]
    fn default_is_strict() {
        // req: API-030
        assert_eq!(DecodeMode::default(), DecodeMode::Strict);
        assert_ne!(DecodeMode::Strict, DecodeMode::Lenient);
    }

    #[test]
    fn lenient_holds_the_view_and_its_anomalies() {
        // req: API-032
        let l = Lenient::new(7_u8, AnomalySet::from_flags(&[Demo::One]));
        assert_eq!((*l, l.anomalies().len()), (7, 1));
        assert_eq!(l.clone().into_view(), 7);
        assert_eq!(*l.clone().map(u16::from), 7);
        let (v, a) = l.into_parts();
        assert_eq!((v, a.contains(Demo::One)), (7, true));
    }

    #[test]
    fn dispatch_runs_the_entry_point_of_the_mode() {
        // req: API-030
        let strict = || Ok(1_u8);
        let lenient = || Ok(Lenient::new(2_u8, AnomalySet::from_flags(&[Demo::One])));
        let (v, a) = DecodeMode::Strict.dispatch(strict, lenient).unwrap();
        assert_eq!((v, a.is_empty()), (1, true));
        let (v, a) = DecodeMode::Lenient.dispatch(strict, lenient).unwrap();
        assert_eq!((v, a.len()), (2, 1));
    }
}
