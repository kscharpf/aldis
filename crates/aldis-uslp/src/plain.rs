//! The `Plain` step of a received USLP frame: SDLS verified, or no SDLS (RS-023; API-074).

use core::ops::Deref;

use aldis_core::Result;
use aldis_core::finishing::require_unsecured;
use aldis_core::security::NoSecurity;

use crate::view::UslpFrame;

/// A [`UslpFrame`] whose SDLS processing is done: the frame of a channel with no Security Header
/// or Trailer, taken through [`NoSecurity`] (355.0 §2.3; RS-023, API-074).
///
/// [`UslpFrame::into_plain`] returns it, and it is the only frame an engine's frame input takes
/// (`FrameReceiver`, LNK-050). It dereferences to the frame. A frame is `Received` (the octets),
/// then `Checked` (the [`UslpFrame`] a decode returns, or the `ValidatedUslpFrame` of the
/// Frame Validation Checks: the FECF verified), then `Plain`; the steps are types, so a frame
/// cannot skip one (RS-023).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlainUslpFrame<'a> {
    frame: UslpFrame<'a>,
}

impl<'a> PlainUslpFrame<'a> {
    /// The checked frame, without the proof that its SDLS processing is done.
    #[must_use]
    pub const fn into_frame(self) -> UslpFrame<'a> {
        self.frame
    }
}

impl<'a> Deref for PlainUslpFrame<'a> {
    type Target = UslpFrame<'a>;

    fn deref(&self) -> &UslpFrame<'a> {
        &self.frame
    }
}

impl<'a> UslpFrame<'a> {
    /// The `Checked` to `Plain` step for a channel with no SDLS: `hook` is [`NoSecurity`], the
    /// identity, so the frame must have no Security Header or Trailer (RS-023, API-070,
    /// API-074). A channel with SDLS verifies the frame with its `SecurityHook` before the
    /// engine sees it, which v1 does not provide for the engine input (issue #230).
    ///
    /// # Errors
    /// `InvalidField` (`security_header_len`, or `security_trailer_len`) if the frame has a
    /// Security Header or Trailer: [`NoSecurity`] cannot verify it.
    pub fn into_plain(self, _hook: NoSecurity) -> Result<PlainUslpFrame<'a>> {
        require_unsecured(self.security_header().len(), self.security_trailer().len())?;
        Ok(PlainUslpFrame { frame: self })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    extern crate std;
    use std::vec::Vec;

    use aldis_core::ErrorKind;
    use aldis_core::crc::crc16;
    use aldis_core::security::NoSecurity;

    use crate::config::tests::{fixed, plain_variable};
    use crate::view::UslpFrame;

    /// `body` with its FECF appended.
    fn with_fecf(body: &[u8]) -> Vec<u8> {
        let mut v = body.to_vec();
        v.extend_from_slice(&crc16(body).to_be_bytes());
        v
    }

    #[test]
    fn a_frame_without_sdls_becomes_plain() {
        // req: RS-023, API-017, API-074
        // A 64-octet frame on the fixed channel: VC 1, MAP 3, no SDLS fields.
        let mut b = std::vec![
            0xC0, 0x15, 0x50, 0x26, 0x00, 0x3F, 0x8A, 0x12, 0x34, 0xAA, 0xBB, 0x00, 0x00, 0x02,
        ];
        b.extend((0..44).map(|i| u8::try_from(i).unwrap_or_default()));
        b.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);
        let bytes = with_fecf(&b);
        let plain = UslpFrame::decode(&bytes, &fixed())
            .unwrap()
            .into_plain(NoSecurity)
            .unwrap();
        assert_eq!(plain.octets(), bytes.as_slice());
        assert_eq!(plain.into_frame().len(), bytes.len());
    }

    #[test]
    fn a_frame_with_sdls_fields_does_not() {
        // req: RS-023, API-070
        // VC 5 MAP 1 has a Security Header of 6 and a Trailer of 16 octets.
        let mut b = std::vec![0xC0, 0x15, 0x50, 0xA2, 0x00, 0x00, 0x00];
        b.extend_from_slice(&[0x51; 6]);
        b.extend_from_slice(&[0xE5, 0x77, 0x88]);
        b.extend_from_slice(&[0x52; 16]);
        b[5] = u8::try_from(b.len() + 1).unwrap_or_default();
        let bytes = with_fecf(&b);
        let e = UslpFrame::decode(&bytes, &plain_variable())
            .unwrap()
            .into_plain(NoSecurity)
            .unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidField);
    }
}
