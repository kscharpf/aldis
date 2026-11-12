//! The checking pipeline: a frame checked against the managed parameters of its channel
//! (GEN-060, API-015, API-016, RS-020, RS-023).
//!
//! Every frame codec checks a decoded frame against the channels it was configured with and
//! hands the result on as a [`Checked`] frame. There are two entries, one per kind of input:
//!
//! - [`Check`] checks a frame that is already decoded (TM, AOS): `frame.check(&lookup)` returns
//!   `Result<Checked<F>, Rejection<R>>`.
//! - [`CheckBytes`] checks the octets of exactly one frame (TC, USLP), where the Blue Book
//!   lists the checks of the receiving end (232.0 §4.4.9.3.3, 732.1 §4.3.11.3.4):
//!   [`check_bytes`] returns the frame in a [`Lenient`], whose anomaly set is empty after a
//!   strict check (API-030, API-032).
//!
//! [`Checked`] has no public constructor: a value of it is the proof that a codec's check
//! passed, and it is made only by [`Check::check`] and [`check_bytes`] (RS-020). Neither
//! trait has a type parameter, so a frame type has one implementation, and only the crate that
//! defines the frame can write it (the orphan rule: a foreign trait for a foreign type with no
//! local type in it is refused, E0117). No other crate can therefore make a [`Checked`] codec
//! frame without the codec's check; `aldis-tm` has the compile-fail example of the attack. A
//! check that fails returns a [`Rejection`], which holds the error of the failed check (GEN-041)
//! and, for the standards that number their failures (TC reasons 1–11, USLP 1–18), the reason
//! `R`.

use core::ops::Deref;

use crate::anomaly::{Anomaly, AnomalySet};
use crate::error::Error;
use crate::lookup::{ChannelLookup, MapLookup, PhysicalLookup};
use crate::mode::{DecodeMode, Lenient};

/// A frame that failed a check: the error of the failed check and the reason `R` that the
/// standard's numbering gives it (GEN-041, GEN-060, API-016).
///
/// `R` is `()` for the standards that list no reasons (TM, AOS).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rejection<R> {
    reason: R,
    error: Error,
}

impl<R> Rejection<R> {
    /// A rejection for `reason`, with the error the check reports for it.
    #[must_use]
    pub const fn new(reason: R, error: Error) -> Rejection<R> {
        Rejection { reason, error }
    }

    /// The error of the failed check, with the field it names (GEN-041).
    #[must_use]
    pub const fn error(&self) -> Error {
        self.error
    }
}

impl<R: Copy> Rejection<R> {
    /// Why the frame was discarded (TC-021, TC-022, USLP-014).
    #[must_use]
    pub const fn reason(&self) -> R {
        self.reason
    }
}

impl From<Error> for Rejection<()> {
    fn from(error: Error) -> Rejection<()> {
        Rejection::new((), error)
    }
}

impl<R> From<Rejection<R>> for Error {
    fn from(r: Rejection<R>) -> Error {
        r.error
    }
}

/// A frame `F` that passed the check of its codec against the managed parameters of its
/// channel (GEN-060, API-016; RS-023 `Checked`).
///
/// It dereferences to the frame. The proof says that the checks passed against some
/// configuration, not which one (API-016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checked<F> {
    frame: F,
}

impl<F> Checked<F> {
    /// The frame, without the proof.
    #[must_use]
    pub fn into_frame(self) -> F {
        self.frame
    }

    /// The frame, by reference; usable where `Deref` is not applied implicitly.
    #[must_use]
    pub const fn as_frame(&self) -> &F {
        &self.frame
    }
}

impl<F> Deref for Checked<F> {
    type Target = F;

    fn deref(&self) -> &F {
        &self.frame
    }
}

/// A decoded frame that can be checked against the lookup of its channels (TM-011, AOS-010,
/// AOS-016, API-016).
///
/// The trait has no type parameter, so a frame type has at most one implementation, and only
/// the crate that defines the frame can write it (the orphan rule: `Check` is foreign to every
/// other crate, and so is the frame, and no local type appears in `impl Check for Frame`).
/// A [`Checked`] frame is therefore a proof that the codec's check ran. An outside crate that
/// implements the trait for a codec's frame is refused (E0117).
///
/// The compile-fail example on `aldis_tm`'s implementation shows the attack: another crate
/// implements `Check` for `TmFrame` with a lookup type of its own.
///
/// A codec implements [`verify`](Self::verify), the checks themselves; callers use
/// [`check`](Self::check), the only way to a [`Checked`] frame from a decoded one.
pub trait Check: Sized {
    /// The reason type of the standard's failure numbering; `()` if it has none.
    type Reason;
    /// What the frame is checked against: the channels it was decoded with.
    type Lookup: ?Sized;

    /// Runs the checks against `lookup`, which must hold the channels the frame was decoded
    /// with.
    ///
    /// # Errors
    /// The first check that fails, as a [`Rejection`].
    fn verify(&self, lookup: &Self::Lookup) -> Result<(), Rejection<Self::Reason>>;

    /// Checks the frame against `lookup` and returns it as a [`Checked`] frame.
    ///
    /// # Errors
    /// The first check that fails, as a [`Rejection`] (see [`verify`](Self::verify)).
    fn check(self, lookup: &Self::Lookup) -> Result<Checked<Self>, Rejection<Self::Reason>> {
        self.verify(lookup).map(|()| Checked { frame: self })
    }
}

/// A frame whose checks run on the octets of exactly one frame, in the order of its standard's
/// receiving procedure (TC-021, TC-023, USLP-014).
///
/// A codec implements [`parse_exact`](Self::parse_exact); callers use [`check_bytes`]. Like
/// [`Check`] it has no type parameter, so only the codec's crate can implement it for its
/// frame (the lookup is a parameter of the method, bounded by the lookup traits of
/// `aldis_core::lookup` at the identifier types of the frame). The attack of
/// the example on [`Check`] is refused alike.
pub trait CheckBytes<'a>: Sized {
    /// The reason type of the standard's failure numbering.
    type Reason;
    /// The anomaly flags of the codec's lenient mode.
    type Anomaly: Anomaly;
    /// What the check matched besides the frame: the configurations of its channels.
    type Matched;
    /// The Spacecraft ID type of the standard.
    type Scid: Copy;
    /// The Virtual Channel ID type of the standard.
    type VcId: Copy;
    /// The MAP ID type of the standard.
    type MapId: Copy;
    /// What a virtual channel carries for the decoder.
    type Vc;
    /// What a MAP carries for the decoder.
    type Map;
    /// The configuration of the Physical Channel.
    type Physical;

    /// Decodes `bytes`, which must be exactly one frame, and runs every check of the receiving
    /// end on it against `lookup`, in `mode`.
    ///
    /// # Errors
    /// The first check that fails, as a [`Rejection`].
    #[expect(
        clippy::type_complexity,
        reason = "the one result type of the pipeline; an alias would only rename its parts"
    )]
    fn parse_exact<L>(
        bytes: &'a [u8],
        lookup: &L,
        mode: DecodeMode,
    ) -> Result<(Self, AnomalySet<Self::Anomaly>, Self::Matched), Rejection<Self::Reason>>
    where
        L: ChannelLookup<Self::Scid, Self::VcId, Vc = Self::Vc>
            + MapLookup<Self::Scid, Self::VcId, Self::MapId, Map = Self::Map>
            + PhysicalLookup<Physical = Self::Physical>
            + ?Sized;

    /// The octets the frame was decoded from.
    fn octets(&self) -> &'a [u8];
}

/// Checks the octets of exactly one frame and returns what it matched (TC-021, TC-023,
/// USLP-014, API-016).
///
/// The frame comes as a [`Checked`] one with the anomalies a lenient `mode` accepted (empty
/// after a strict check).
///
/// # Errors
/// The first check that fails, as a [`Rejection`].
#[expect(
    clippy::type_complexity,
    reason = "the one result type of the pipeline; an alias would only rename its parts"
)]
pub fn check_bytes_matched<'a, F, L>(
    bytes: &'a [u8],
    lookup: &L,
    mode: DecodeMode,
) -> Result<(Lenient<Checked<F>, F::Anomaly>, F::Matched), Rejection<F::Reason>>
where
    F: CheckBytes<'a>,
    L: ChannelLookup<F::Scid, F::VcId, Vc = F::Vc>
        + MapLookup<F::Scid, F::VcId, F::MapId, Map = F::Map>
        + PhysicalLookup<Physical = F::Physical>
        + ?Sized,
{
    F::parse_exact(bytes, lookup, mode)
        .map(|(frame, anomalies, matched)| (Lenient::new(Checked { frame }, anomalies), matched))
}

/// Checks the octets `bytes` of exactly one frame as [`check_bytes_matched`] does, without the
/// matched configurations.
///
/// # Errors
/// The first check that fails, as a [`Rejection`].
#[expect(
    clippy::type_complexity,
    reason = "the one result type of the pipeline; an alias would only rename its parts"
)]
pub fn check_bytes<'a, F, L>(
    bytes: &'a [u8],
    lookup: &L,
    mode: DecodeMode,
) -> Result<Lenient<Checked<F>, F::Anomaly>, Rejection<F::Reason>>
where
    F: CheckBytes<'a>,
    L: ChannelLookup<F::Scid, F::VcId, Vc = F::Vc>
        + MapLookup<F::Scid, F::VcId, F::MapId, Map = F::Map>
        + PhysicalLookup<Physical = F::Physical>
        + ?Sized,
{
    check_bytes_matched(bytes, lookup, mode).map(|(checked, _)| checked)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Check, CheckBytes, Checked, Rejection, check_bytes, check_bytes_matched};
    use crate::anomaly::AnomalySet;
    use crate::error::{Error, ErrorKind};
    use crate::lookup::{ChannelLookup, MapLookup, PhysicalLookup};
    use crate::mode::DecodeMode;

    crate::anomaly_flags! {
        /// Flags of the demo frame.
        enum DemoAnomaly {
            /// Odd octet.
            Odd = 0,
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Demo(u8);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Why {
        TooBig,
    }

    impl Check for Demo {
        type Reason = ();
        type Lookup = u8;

        fn verify(&self, limit: &u8) -> Result<(), Rejection<()>> {
            if self.0 > *limit {
                return Err(Error::new(ErrorKind::OutOfRange).into());
            }
            Ok(())
        }
    }

    /// A lookup with one channel, whose limit is the octet.
    struct Limit(u8);

    impl ChannelLookup<u8, u8> for Limit {
        type Vc = u8;

        fn has_spacecraft(&self, _: u8) -> bool {
            true
        }

        fn vc(&self, _: u8, _: u8) -> Option<u8> {
            Some(self.0)
        }
    }

    impl MapLookup<u8, u8, u8> for Limit {
        type Map = u8;

        fn map(&self, _: u8, _: u8, _: u8) -> Option<u8> {
            Some(self.0)
        }
    }

    impl PhysicalLookup for Limit {
        type Physical = u8;

        fn physical_channel(&self) -> u8 {
            self.0
        }
    }

    impl<'a> CheckBytes<'a> for Demo {
        type Reason = Why;
        type Anomaly = DemoAnomaly;
        type Matched = u8;
        type Scid = u8;
        type VcId = u8;
        type MapId = u8;
        type Vc = u8;
        type Map = u8;
        type Physical = u8;

        fn parse_exact<L>(
            bytes: &'a [u8],
            lookup: &L,
            mode: DecodeMode,
        ) -> Result<(Demo, AnomalySet<DemoAnomaly>, u8), Rejection<Why>>
        where
            L: ChannelLookup<u8, u8, Vc = u8>
                + MapLookup<u8, u8, u8, Map = u8>
                + PhysicalLookup<Physical = u8>
                + ?Sized,
        {
            let limit = &lookup.physical_channel();
            let [octet] = *bytes else {
                return Err(Rejection::new(
                    Why::TooBig,
                    Error::new(ErrorKind::LengthMismatch),
                ));
            };
            if octet > *limit {
                return Err(Rejection::new(
                    Why::TooBig,
                    Error::new(ErrorKind::OutOfRange),
                ));
            }
            let mut anomalies = AnomalySet::empty();
            if mode == DecodeMode::Lenient && octet % 2 == 1 {
                anomalies.insert(DemoAnomaly::Odd);
            }
            Ok((Demo(octet), anomalies, *limit))
        }

        fn octets(&self) -> &'a [u8] {
            &[]
        }
    }

    #[test]
    fn a_decoded_frame_is_checked_against_a_lookup() {
        // req: GEN-060, API-016
        let ok = Demo(3).check(&4).unwrap();
        assert_eq!(ok.0, 3);
        assert_eq!(ok.as_frame(), &Demo(3));
        assert_eq!(ok.into_frame(), Demo(3));
        let r = Demo(5).check(&4).unwrap_err();
        assert_eq!(r.reason(), ());
        assert_eq!(r.error().kind(), ErrorKind::OutOfRange);
        assert_eq!(Error::from(r), r.error());
        assert!(Demo(5).verify(&4).is_err());
    }

    #[test]
    fn octets_are_checked_in_both_modes() {
        // req: GEN-060, API-016, API-030, API-032
        let strict = check_bytes::<Demo, _>(&[3], &Limit(4), DecodeMode::Strict).unwrap();
        assert!(strict.anomalies().is_empty());
        let lenient = check_bytes::<Demo, _>(&[3], &Limit(4), DecodeMode::Lenient).unwrap();
        assert!(lenient.anomalies().contains(DemoAnomaly::Odd));
        assert_eq!(DemoAnomaly::ALL, &[DemoAnomaly::Odd]);
        let (checked, matched) =
            check_bytes_matched::<Demo, _>(&[2], &Limit(4), DecodeMode::Strict).unwrap();
        assert_eq!((checked.into_view().into_frame(), matched), (Demo(2), 4));
        let r = check_bytes::<Demo, _>(&[9], &Limit(4), DecodeMode::Strict).unwrap_err();
        assert_eq!(r.reason(), Why::TooBig);
        let r = check_bytes::<Demo, _>(&[], &Limit(4), DecodeMode::Strict).unwrap_err();
        assert_eq!(r.error().kind(), ErrorKind::LengthMismatch);
        let _: Checked<Demo> = Demo(1).check(&1).unwrap();
    }
}
