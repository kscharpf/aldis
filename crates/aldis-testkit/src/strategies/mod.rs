//! Shared property-test input strategies (VER-054), used by the property tests of the codec
//! crates (VER-023) and by the differential tests of `difftest/` (VER-053, CPP-060).
//!
//! Two kinds of generator, named after what they produce:
//!
//! - **valid typed values**, built only through the crates' own checked constructors and
//!   encoders: identifiers and counters of `aldis-core` ([`common`]), time code formats and
//!   values (`time`), Space Packet and Encapsulation Packet fields, configurations, and whole
//!   packets (`spp`, `epp`), TM channel configurations and whole frames (`tm`), TC frames, their
//!   channels, data units, and CLCWs (`tc`), AOS channel tables and whole frames in both
//!   editions (`aos`), and USLP channels, frames of every kind, and data units (`uslp`);
//! - **invalid octet strings next to each boundary**: a valid encoding one octet too short, its
//!   length field off by one, a version or reserved value changed, mixed with arbitrary octets
//!   (`*_near_boundary` and `*_decoder_input`).
//!
//! Strategies compose with the `proptest` combinators; [`proptest`](mod@proptest) is re-exported
//! so that a test uses the same version as the strategies. A strategy never panics: a value that
//! a checked constructor would reject is filtered out rather than unwrapped, and none is
//! expected (each module's own tests check that its values are what it documents).
//!
//! # Features
//!
//! `strategies` provides this module and [`common`] (`aldis-core`); `strategies-time`,
//! `strategies-spp`, `strategies-epp`, `strategies-tm`, `strategies-tc`, `strategies-aos`, and `strategies-uslp` add the module for one codec crate
//! each. A crate's
//! tests enable only the features for the crates they test, so that they link no other codec
//! crate: `aldis-core`'s coverage would otherwise count the code its macros expand to in those
//! crates (VER-024).
//!
//! # Integration tests only
//!
//! A property test that uses these strategies is an integration test of its crate
//! (`tests/properties.rs`), not a unit test: a unit test is compiled into a second copy of its
//! crate, whose types differ from the ones this module generates (M2 plan D6).
//!
//! ```
//! use aldis_testkit::strategies::{self, common, proptest::prelude::*};
//!
//! proptest! {
//!     #![proptest_config(strategies::config(16, "target/doctest-regressions.txt"))]
//!     fn user_apids_are_not_idle(apid in common::user_apid()) {
//!         prop_assert!(!apid.is_idle());
//!     }
//! }
//! # user_apids_are_not_idle();
//! ```

pub use proptest;

#[cfg(feature = "strategies-aos")]
pub mod aos;
pub mod common;
#[cfg(feature = "strategies-epp")]
pub mod epp;
#[cfg(feature = "strategies-spp")]
pub mod spp;
#[cfg(feature = "strategies-tc")]
pub mod tc;
#[cfg(feature = "strategies-time")]
pub mod time;
#[cfg(feature = "strategies-tm")]
pub mod tm;
#[cfg(feature = "strategies-uslp")]
pub mod uslp;

use proptest::collection::SizeRange;
use proptest::prelude::*;
use proptest::test_runner::{Config, FileFailurePersistence};

/// The configuration of a property test: `cases` cases, and failing cases persisted to and
/// replayed from `regressions` (VER-023).
///
/// `regressions` is the path of the crate's regression file, usually
/// `concat!(env!("CARGO_MANIFEST_DIR"), "/proptest-regressions/properties.txt")`; an
/// integration test has no `src/` directory beside it for `proptest`'s default location. The
/// `PROPTEST_CASES` environment variable, when set, overrides `cases`, so a nightly run can raise
/// the count and a mutation run can lower it.
#[must_use]
pub fn config(cases: u32, regressions: &'static str) -> Config {
    let base = Config::default();
    let cases = if std::env::var_os("PROPTEST_CASES").is_some() {
        base.cases
    } else {
        cases
    };
    Config {
        cases,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(regressions))),
        ..base
    }
}

/// Arbitrary octet strings with a length in `len`: noise for decoders (GEN-020, VER-023).
pub fn octets(len: impl Into<SizeRange>) -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(any::<u8>(), len)
}

/// Every proper prefix of `valid`, most often the one exactly one octet too short (API-012,
/// GEN-020): a decoder must report the input incomplete, never read past it.
///
/// # Panics
/// If `valid` is empty, which has no proper prefix.
pub fn proper_prefix(valid: Vec<u8>) -> BoxedStrategy<Vec<u8>> {
    assert!(!valid.is_empty(), "an empty string has no proper prefix");
    let short = valid.len() - 1;
    prop_oneof![
        3 => Just(valid[..short].to_vec()),
        1 => (0..short.max(1)).prop_map(move |n| valid[..n.min(short)].to_vec()),
    ]
    .boxed()
}

/// `valid` followed by up to `max_extra` arbitrary octets: a decoder must consume only its own
/// data unit and ignore what follows (API-012, VER-023).
pub fn with_trailing(valid: Vec<u8>, max_extra: usize) -> impl Strategy<Value = Vec<u8>> {
    octets(0..=max_extra).prop_map(move |tail| [valid.as_slice(), tail.as_slice()].concat())
}

#[cfg(test)]
mod tests {
    use super::{config, octets, proper_prefix, with_trailing};
    use proptest::prelude::*;

    const REGRESSIONS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/proptest-regressions/strategies.txt"
    );

    #[test]
    fn config_sets_cases_and_persistence() {
        // req: VER-054
        let c = config(77, REGRESSIONS);
        if std::env::var_os("PROPTEST_CASES").is_none() {
            assert_eq!(c.cases, 77);
        }
        assert!(c.failure_persistence.is_some());
    }

    proptest! {
        #![proptest_config(config(64, REGRESSIONS))]

        #[test]
        fn prefixes_are_proper(
            (valid, prefix) in octets(1..20).prop_flat_map(|v| (Just(v.clone()), proper_prefix(v)))
        ) {
            // req: VER-054
            prop_assert!(prefix.len() < valid.len());
            prop_assert!(valid.starts_with(&prefix));
        }

        #[test]
        fn trailing_octets_extend(
            (valid, longer) in octets(0..20).prop_flat_map(|v| (Just(v.clone()), with_trailing(v, 4)))
        ) {
            // req: VER-054
            prop_assert!(longer.starts_with(&valid));
            prop_assert!(longer.len() <= valid.len() + 4);
        }
    }
}
