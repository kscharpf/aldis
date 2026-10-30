//! Frame finishing states: the typestate of the codec writers' frame (RS-023, RS-021).
//!
//! A codec writer (`TmFrameWriter`, `TcFrameWriter`, `AosFrameWriter`, `UslpFrameWriter`) is
//! generic over one of three states, in the order SDLS processing requires (355.0-B-2 §2.3;
//! API-074):
//!
//! 1. [`Unprotected`]: every field is final except the Security Header and Trailer contents, the
//!    FHEC (AOS), and the FECF. The caller fills the fields, then applies
//!    [`SecurityHook::protect`](crate::security::SecurityHook::protect) through the writer's
//!    `protect`, or its `unsecured` on a channel without SDLS.
//! 2. [`Protected`]: the Security Header and Trailer are final. Only the writer's `finalize`
//!    remains.
//! 3. [`Finalized`]: the FHEC and the FECF are computed, last. Only a finalized writer exposes
//!    the octets of the frame for transmission.
//!
//! Each transition consumes the writer, so an operation out of order does not compile. The
//! states are uninhabited types, markers only, and the set is closed ([`FrameState`] is sealed).

use crate::error::{Error, ErrorKind, Result};
use crate::field;
use crate::security::{HookLayout, SecurityLayout};

mod sealed {
    /// Keeps [`FrameState`](super::FrameState) closed to the three states of this module.
    pub trait Sealed {}
}

/// A state of a frame being finished (RS-023). Sealed: only [`Unprotected`], [`Protected`], and
/// [`Finalized`] implement it.
pub trait FrameState: sealed::Sealed {}

/// Checks that a channel's Security Header and Trailer lengths are those a hook is for ([`HookLayout::Any`]: any), for a writer's `protect` (API-070, API-073, RS-023).
///
/// # Errors
/// `InvalidField` (`security_header_len`, or `security_trailer_len`) if the hook's layout
/// differs from the channel's, such as `NoSecurity` on a channel that reserves Security Header
/// or Trailer octets: they need a hook that fills them (API-074).
pub const fn require_layout(hook: HookLayout, header_len: usize, trailer_len: usize) -> Result<()> {
    let HookLayout::Exact(layout) = hook else {
        return Ok(());
    };
    if layout.header_len() != header_len {
        return Err(Error::new(ErrorKind::InvalidField).with_field(field::SECURITY_HEADER_LEN));
    }
    if layout.trailer_len() != trailer_len {
        return Err(Error::new(ErrorKind::InvalidField).with_field(field::SECURITY_TRAILER_LEN));
    }
    Ok(())
}

/// Checks that a channel without SDLS has no Security Header or Trailer, for a writer's
/// `unsecured` transition, the `NoSecurity` path of RS-023 (API-070, API-073).
///
/// # Errors
/// As [`require_layout`] with the layout 0/0.
pub const fn require_unsecured(header_len: usize, trailer_len: usize) -> Result<()> {
    require_layout(
        HookLayout::Exact(SecurityLayout::NONE),
        header_len,
        trailer_len,
    )
}

/// Every field is final except the Security Header and Trailer contents, the FHEC, and the FECF
/// (RS-023; API-074).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unprotected {}

/// The Security Header and Trailer are final; the FHEC and the FECF are not yet computed
/// (RS-023; API-074).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protected {}

/// The FHEC and the FECF are computed: the octets of the frame are final (RS-023; API-074).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finalized {}

impl sealed::Sealed for Unprotected {}
impl sealed::Sealed for Protected {}
impl sealed::Sealed for Finalized {}
impl FrameState for Unprotected {}
impl FrameState for Protected {}
impl FrameState for Finalized {}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Finalized, FrameState, Protected, Unprotected, require_layout, require_unsecured};
    use crate::error::ErrorKind;
    use crate::field;
    use crate::security::{HookLayout, SecurityLayout};

    fn is_state<S: FrameState>() -> usize {
        core::mem::size_of::<core::marker::PhantomData<S>>()
    }

    #[test]
    fn the_three_states_are_markers() {
        // req: RS-023
        assert_eq!(is_state::<Unprotected>(), 0);
        assert_eq!(is_state::<Protected>(), 0);
        assert_eq!(is_state::<Finalized>(), 0);
    }

    #[test]
    fn unsecured_needs_no_security_octets() {
        // req: RS-023, API-070
        assert_eq!(require_unsecured(0, 0), Ok(()));
        // A hook for any layout fits every channel; one for a layout fits only that layout.
        let l = SecurityLayout::new(2, 1).unwrap();
        assert_eq!(require_layout(HookLayout::Any, 5, 9), Ok(()));
        assert_eq!(require_layout(HookLayout::Exact(l), 2, 1), Ok(()));
        assert_eq!(
            require_layout(HookLayout::Exact(l), 3, 1)
                .unwrap_err()
                .field(),
            Some(field::SECURITY_HEADER_LEN)
        );
        assert_eq!(
            require_layout(HookLayout::Exact(l), 2, 0)
                .unwrap_err()
                .field(),
            Some(field::SECURITY_TRAILER_LEN)
        );
        let e = require_unsecured(2, 0).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidField, Some(field::SECURITY_HEADER_LEN))
        );
        let e = require_unsecured(0, 1).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidField, Some(field::SECURITY_TRAILER_LEN))
        );
    }
}
