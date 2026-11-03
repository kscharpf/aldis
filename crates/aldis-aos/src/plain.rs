//! The `Plain` step of a received AOS frame: SDLS verified, or no SDLS (RS-023; API-074).

use core::ops::Deref;

use aldis_core::Result;
use aldis_core::finishing::require_unsecured;
use aldis_core::security::NoSecurity;

use crate::view::AosFrame;

/// An [`AosFrame`] whose SDLS processing is done: the frame of a channel with no Security Header
/// or Trailer, taken through [`NoSecurity`] (355.0 §2.3; RS-023, API-074).
///
/// [`AosFrame::into_plain`] returns it, and it is the only frame an engine's frame input takes
/// (`FrameReceiver`, LNK-050). It dereferences to the frame. A frame is `Received` (the octets),
/// then `Checked` (the [`AosFrame`] a decode returns: the FHEC corrected, the FECF verified),
/// then `Plain`; the steps are types, so a frame cannot skip one (RS-023).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlainAosFrame<'a> {
    frame: AosFrame<'a>,
}

impl<'a> PlainAosFrame<'a> {
    /// The checked frame, without the proof that its SDLS processing is done.
    #[must_use]
    pub const fn into_frame(self) -> AosFrame<'a> {
        self.frame
    }
}

impl<'a> Deref for PlainAosFrame<'a> {
    type Target = AosFrame<'a>;

    fn deref(&self) -> &AosFrame<'a> {
        &self.frame
    }
}

impl<'a> AosFrame<'a> {
    /// The `Checked` to `Plain` step for a channel with no SDLS: `hook` is [`NoSecurity`], the
    /// identity, so the frame must have no Security Header or Trailer (RS-023, API-070,
    /// API-074). A channel with SDLS verifies the frame with its `SecurityHook` before the
    /// engine sees it, which v1 does not provide for the engine input (issue #230).
    ///
    /// # Errors
    /// `InvalidField` (`security_header_len`, or `security_trailer_len`) if the frame has a
    /// Security Header or Trailer: [`NoSecurity`] cannot verify it.
    pub fn into_plain(self, _hook: NoSecurity) -> Result<PlainAosFrame<'a>> {
        require_unsecured(self.security_header().len(), self.security_trailer().len())?;
        Ok(PlainAosFrame { frame: self })
    }
}
