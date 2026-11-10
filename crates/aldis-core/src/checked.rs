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
//! passed, and it is made only by [`Check::check`] and [`check_bytes`] (RS-020). A check that
//! fails returns a [`Rejection`], which holds the error of the failed check (GEN-041) and,
//! for the standards that number their failures (TC reasons 1–11, USLP 1–18), the reason `R`.

use core::ops::Deref;

use crate::anomaly::{Anomaly, AnomalySet};
use crate::error::Error;
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

/// A decoded frame that can be checked against a lookup `L` of its channels (TM-011, AOS-010,
/// AOS-016, API-016).
///
/// A codec implements [`verify`](Self::verify), the checks themselves; callers use
/// [`check`](Self::check), the only way to a [`Checked`] frame from a decoded one.
pub trait Check<L: ?Sized>: Sized {
    /// The reason type of the standard's failure numbering; `()` if it has none.
    type Reason;

    /// Runs the checks against `lookup`, which must hold the channels the frame was decoded
    /// with.
    ///
    /// # Errors
    /// The first check that fails, as a [`Rejection`].
    fn verify(&self, lookup: &L) -> Result<(), Rejection<Self::Reason>>;

    /// Checks the frame against `lookup` and returns it as a [`Checked`] frame.
    ///
    /// # Errors
    /// The first check that fails, as a [`Rejection`] (see [`verify`](Self::verify)).
    fn check(self, lookup: &L) -> Result<Checked<Self>, Rejection<Self::Reason>> {
        self.verify(lookup).map(|()| Checked { frame: self })
    }
}

/// A frame whose checks run on the octets of exactly one frame, in the order of its standard's
/// receiving procedure (TC-021, TC-023, USLP-014).
///
/// A codec implements [`parse_exact`](Self::parse_exact); callers use [`check_bytes`].
pub trait CheckBytes<'a, L: ?Sized>: Sized {
    /// The reason type of the standard's failure numbering.
    type Reason;
    /// The anomaly flags of the codec's lenient mode.
    type Anomaly: Anomaly;
    /// What the check matched besides the frame: the configurations of its channels.
    type Matched;

    /// Decodes `bytes`, which must be exactly one frame, and runs every check of the receiving
    /// end on it against `lookup`, in `mode`.
    ///
    /// # Errors
    /// The first check that fails, as a [`Rejection`].
    #[expect(
        clippy::type_complexity,
        reason = "the one result type of the pipeline; an alias would only rename its parts"
    )]
    fn parse_exact(
        bytes: &'a [u8],
        lookup: &L,
        mode: DecodeMode,
    ) -> Result<(Self, AnomalySet<Self::Anomaly>, Self::Matched), Rejection<Self::Reason>>;
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
    F: CheckBytes<'a, L>,
    L: ?Sized,
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
    F: CheckBytes<'a, L>,
    L: ?Sized,
{
    check_bytes_matched(bytes, lookup, mode).map(|(checked, _)| checked)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Check, CheckBytes, Checked, Rejection, check_bytes, check_bytes_matched};
    use crate::anomaly::AnomalySet;
    use crate::error::{Error, ErrorKind};
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

    impl Check<u8> for Demo {
        type Reason = ();

        fn verify(&self, limit: &u8) -> Result<(), Rejection<()>> {
            if self.0 > *limit {
                return Err(Error::new(ErrorKind::OutOfRange).into());
            }
            Ok(())
        }
    }

    impl<'a> CheckBytes<'a, u8> for Demo {
        type Reason = Why;
        type Anomaly = DemoAnomaly;
        type Matched = u8;

        fn parse_exact(
            bytes: &'a [u8],
            limit: &u8,
            mode: DecodeMode,
        ) -> Result<(Demo, AnomalySet<DemoAnomaly>, u8), Rejection<Why>> {
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
        let strict = check_bytes::<Demo, u8>(&[3], &4, DecodeMode::Strict).unwrap();
        assert!(strict.anomalies().is_empty());
        let lenient = check_bytes::<Demo, u8>(&[3], &4, DecodeMode::Lenient).unwrap();
        assert!(lenient.anomalies().contains(DemoAnomaly::Odd));
        assert_eq!(DemoAnomaly::ALL, &[DemoAnomaly::Odd]);
        let (checked, matched) =
            check_bytes_matched::<Demo, u8>(&[2], &4, DecodeMode::Strict).unwrap();
        assert_eq!((checked.into_view().into_frame(), matched), (Demo(2), 4));
        let r = check_bytes::<Demo, u8>(&[9], &4, DecodeMode::Strict).unwrap_err();
        assert_eq!(r.reason(), Why::TooBig);
        let r = check_bytes::<Demo, u8>(&[], &4, DecodeMode::Strict).unwrap_err();
        assert_eq!(r.error().kind(), ErrorKind::LengthMismatch);
        let _: Checked<Demo> = Demo(1).check(&1).unwrap();
    }
}
