//! A decoded frame checked against the configured channels (732.0-B-5 §4.3.5.4, §4.3.6.3;
//! AOS-010, AOS-016, API-016).

use core::ops::Deref;

use aldis_core::{Error, ErrorKind, Result};

use crate::view::AosFrame;

/// An [`AosFrame`] of a channel that its decode's `VcLookup` holds (732.0 §4.3.5.4, §4.3.6.3;
/// AOS-010, AOS-016, API-016).
///
/// [`AosFrame::validate`] returns it. It dereferences to the frame; holding one is the proof
/// that the frame's Spacecraft ID (an OID frame) or Spacecraft ID and VCID were configured, so
/// the data field, OCF, and SDLS fields were read as the channel says (RS-023 `Checked`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedAosFrame<'a> {
    frame: AosFrame<'a>,
}

impl<'a> ValidatedAosFrame<'a> {
    /// The frame, without the proof.
    #[must_use]
    pub const fn into_frame(self) -> AosFrame<'a> {
        self.frame
    }
}

impl<'a> Deref for ValidatedAosFrame<'a> {
    type Target = AosFrame<'a>;

    fn deref(&self) -> &AosFrame<'a> {
        &self.frame
    }
}

impl<'a> AosFrame<'a> {
    /// Rejects a frame whose channel the decode's `VcLookup` did not hold (732.0 §4.3.5.4,
    /// §4.3.6.3; AOS-010, AOS-016, API-016, RS-023) and returns the others as a
    /// [`ValidatedAosFrame`].
    ///
    /// The decoder reads such a frame as opaque octets with no OCF and no SDLS fields so that an
    /// engine can count it; the Blue Book discards it.
    ///
    /// # Errors
    /// `UnknownChannel` (`spacecraft_id`) for a Spacecraft ID that is not valid on the physical
    /// channel, and (`vc_id`) for a VCID of a valid Spacecraft ID that is not configured.
    pub const fn validate(self) -> Result<ValidatedAosFrame<'a>> {
        match self.unconfigured() {
            Some(field) => Err(Error::new(ErrorKind::UnknownChannel).with_field(field)),
            None => Ok(ValidatedAosFrame { frame: self }),
        }
    }
}
