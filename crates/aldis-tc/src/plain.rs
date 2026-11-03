//! The `Plain` step of a received TC frame: SDLS verified, or no SDLS (RS-023; API-074).

use core::ops::Deref;

use aldis_core::Result;
use aldis_core::finishing::require_unsecured;
use aldis_core::security::NoSecurity;

use crate::view::TcFrame;

/// A [`TcFrame`] whose SDLS processing is done: the frame of a channel with no Security Header
/// or Trailer, taken through [`NoSecurity`] (232.0 Cor. 1 §6.5.2; RS-023, API-074).
///
/// [`TcFrame::into_plain`] returns it, and it is the only frame an engine's frame input takes
/// (`FrameReceiver`, LNK-050). It dereferences to the frame. A frame is `Received` (the octets),
/// then `Checked` (the [`TcFrame`] a decode returns, or the `ValidatedTcFrame` of the Frame
/// Validation Checks: the FECF verified), then `Plain`; the steps are types, so a frame cannot
/// skip one (RS-023).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlainTcFrame<'a> {
    frame: TcFrame<'a>,
}

impl<'a> PlainTcFrame<'a> {
    /// The checked frame, without the proof that its SDLS processing is done.
    #[must_use]
    pub const fn into_frame(self) -> TcFrame<'a> {
        self.frame
    }
}

impl<'a> Deref for PlainTcFrame<'a> {
    type Target = TcFrame<'a>;

    fn deref(&self) -> &TcFrame<'a> {
        &self.frame
    }
}

impl<'a> TcFrame<'a> {
    /// The `Checked` to `Plain` step for a channel with no SDLS: `hook` is [`NoSecurity`], the
    /// identity, so the frame must have no Security Header or Trailer (RS-023, API-070,
    /// API-074). A channel with SDLS verifies the frame with its `SecurityHook` before the
    /// engine sees it, which v1 does not provide for the engine input (issue #230).
    ///
    /// # Errors
    /// `InvalidField` (`security_header_len`, or `security_trailer_len`) if the frame has a
    /// Security Header or Trailer: [`NoSecurity`] cannot verify it.
    pub fn into_plain(self, _hook: NoSecurity) -> Result<PlainTcFrame<'a>> {
        require_unsecured(self.security_header().len(), self.security_trailer().len())?;
        Ok(PlainTcFrame { frame: self })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use aldis_core::ErrorKind;
    use aldis_core::security::NoSecurity;

    use crate::config::tests::standard;
    use crate::view::TcFrame;

    #[test]
    fn a_frame_without_sdls_becomes_plain() {
        // req: RS-023, API-017, API-074
        let t = standard(false);
        let bytes = [0x01, 0x55, 0x04, 0x05, 0, 0xAB];
        let plain = TcFrame::decode(&bytes, &t)
            .unwrap()
            .into_plain(NoSecurity)
            .unwrap();
        assert_eq!(plain.octets(), &bytes);
        assert_eq!(plain.into_frame().len(), bytes.len());
    }

    #[test]
    fn a_frame_with_sdls_fields_does_not() {
        // req: RS-023, API-070
        // VC 3 has a Security Header of 4 and a Trailer of 2 octets.
        let t = standard(false);
        let bytes = [0x01, 0x55, 0x0C, 0x0B, 0, 1, 2, 3, 4, 0xAB, 5, 6];
        let e = TcFrame::decode(&bytes, &t)
            .unwrap()
            .into_plain(NoSecurity)
            .unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidField);
    }
}
