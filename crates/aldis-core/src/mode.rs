//! Strict and lenient decoding (API-030, API-031, API-032).
//!
//! Every decoder takes a [`DecodeMode`]. In [`Strict`](DecodeMode::Strict) mode a violation of
//! a Blue Book SHALL that is checkable from the octets and the configuration is an error. In
//! [`Lenient`](DecodeMode::Lenient) mode, deviations in reserved/spare bits, fields CCSDS
//! declares "undefined", and conventional-zero fields are accepted and reported in an
//! [`AnomalySet`](crate::anomaly::AnomalySet) (GEN-043). Version numbers, lengths, and
//! FECF/FHEC failures are errors in both modes.

/// How a decoder treats deviations it can tolerate (API-030).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DecodeMode {
    /// Every checkable Blue Book violation is an error (API-031). The default.
    #[default]
    Strict,
    /// Tolerable deviations are accepted and reported as anomaly flags (API-032, GEN-043).
    Lenient,
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::DecodeMode;

    #[test]
    fn default_is_strict() {
        // req: API-030
        assert_eq!(DecodeMode::default(), DecodeMode::Strict);
        assert_ne!(DecodeMode::Strict, DecodeMode::Lenient);
    }
}
