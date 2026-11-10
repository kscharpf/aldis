//! The `Plain` proof: a frame whose SDLS processing is done (RS-023, API-017, API-073, API-074).
//!
//! A frame is `Received` (the octets), then `Checked` (FECF verified; [`Checked`] when it also
//! matches the managed parameters of its channel), then `Plain`. `Plain<F>` and
//! [`Checked<F>`](crate::checked::Checked) are orthogonal wrappers of a frame `F`, next to
//! [`Lenient`](crate::mode::Lenient): a receiving engine's frame input takes `Plain<F>` and
//! FARM-1's intake takes a checked frame, and `Plain<Checked<F>>` is both.
//!
//! [`Plain`] has no public constructor. A value of it is made only by
//! [`SecuritySlices::into_plain`], which needs a frame with no Security Header or Trailer to
//! take through [`NoSecurity`]. [`SecuritySlices`] has no type parameter, so a frame type has
//! one implementation, and only the crate that defines the frame can write it (the orphan
//! rule: a foreign trait for a foreign type with no local type in it is refused, E0117): no
//! other crate can make a `Plain` codec frame without the codec's own accessors. The
//! compile-fail example on `aldis_tm`'s implementation shows the attack. The SDLS `verify` step
//! of a channel that has SDLS is issue #230.

use core::ops::Deref;

use crate::checked::Checked;
use crate::error::Result;
use crate::finishing::require_unsecured;
use crate::security::NoSecurity;

/// A frame `F` whose SDLS processing is done: the frame of a channel with no Security Header or
/// Trailer, taken through [`NoSecurity`] (355.0 §2.3; RS-023, API-074).
///
/// [`SecuritySlices::into_plain`] returns it. It dereferences to the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plain<F> {
    frame: F,
}

impl<F> Plain<F> {
    /// The frame, without the proof that its SDLS processing is done.
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

impl<F> Deref for Plain<F> {
    type Target = F;

    fn deref(&self) -> &F {
        &self.frame
    }
}

/// A frame that exposes the Security Header and Trailer of its channel (355.0 §2.3; RS-023,
/// API-070, API-074).
///
/// A codec implements the two accessors for its frame; callers use
/// [`into_plain`](Self::into_plain), the only way to a [`Plain`] frame. The trait has no type
/// parameter, so a frame type has at most one implementation, and only the crate that defines
/// the frame can write it (see the module docs). [`Checked<F>`] implements it when `F` does, so
/// `into_plain` takes a frame or a checked frame alike.
pub trait SecuritySlices: Sized {
    /// The Security Header octets: empty on a channel with no SDLS.
    fn security_header(&self) -> &[u8];

    /// The Security Trailer octets: empty on a channel with no SDLS.
    fn security_trailer(&self) -> &[u8];

    /// The `Checked` to `Plain` step for a channel with no SDLS: `hook` is [`NoSecurity`], the
    /// identity, so the frame must have no Security Header or Trailer (RS-023, API-070,
    /// API-074). A channel with SDLS verifies the frame with its `SecurityHook` before the
    /// engine sees it, which v1 does not provide for the engine input (issue #230).
    ///
    /// # Errors
    /// `InvalidField` (`security_header_len`, or `security_trailer_len`) if the frame has a
    /// Security Header or Trailer: [`NoSecurity`] cannot verify it.
    fn into_plain(self, _hook: NoSecurity) -> Result<Plain<Self>> {
        require_unsecured(self.security_header().len(), self.security_trailer().len())?;
        Ok(Plain { frame: self })
    }
}

impl<F: SecuritySlices> SecuritySlices for Checked<F> {
    fn security_header(&self) -> &[u8] {
        self.as_frame().security_header()
    }

    fn security_trailer(&self) -> &[u8] {
        self.as_frame().security_trailer()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Plain, SecuritySlices};
    use crate::checked::{Check, Checked, Rejection};
    use crate::error::ErrorKind;
    use crate::security::NoSecurity;

    /// A frame of a header and a trailer.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Demo(&'static [u8], &'static [u8]);

    impl SecuritySlices for Demo {
        fn security_header(&self) -> &[u8] {
            self.0
        }

        fn security_trailer(&self) -> &[u8] {
            self.1
        }
    }

    impl Check for Demo {
        type Reason = ();
        type Lookup = ();

        fn verify(&self, (): &()) -> Result<(), Rejection<()>> {
            Ok(())
        }
    }

    #[test]
    fn a_frame_without_sdls_becomes_plain() {
        // req: RS-023, API-017, API-074
        let plain: Plain<Demo> = Demo(&[], &[]).into_plain(NoSecurity).unwrap();
        assert_eq!(plain.security_header(), []);
        assert_eq!(plain.as_frame(), &Demo(&[], &[]));
        assert_eq!(plain.into_frame(), Demo(&[], &[]));
    }

    #[test]
    fn a_checked_frame_becomes_plain_and_keeps_its_proof() {
        // req: RS-023, API-016, API-074
        let checked = Demo(&[], &[]).check(&()).unwrap();
        let plain: Plain<Checked<Demo>> = checked.into_plain(NoSecurity).unwrap();
        assert_eq!(plain.into_frame().into_frame(), Demo(&[], &[]));
    }

    #[test]
    fn a_frame_with_sdls_fields_does_not() {
        // req: RS-023, API-070
        let e = Demo(&[1], &[]).into_plain(NoSecurity).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidField);
        let e = Demo(&[], &[1]).into_plain(NoSecurity).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidField);
        let checked = Demo(&[1], &[]).check(&()).unwrap();
        assert!(checked.into_plain(NoSecurity).is_err());
    }
}
