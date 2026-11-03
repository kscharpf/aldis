//! The `Plain` step of a received TM frame: SDLS verified, or no SDLS (RS-023; API-074).

use core::ops::Deref;

use aldis_core::Result;
use aldis_core::finishing::require_unsecured;
use aldis_core::security::NoSecurity;

use crate::view::TmFrame;

/// A [`TmFrame`] whose SDLS processing is done: the frame of a channel with no Security Header
/// or Trailer, taken through [`NoSecurity`] (355.0 §2.3; RS-023, API-074).
///
/// [`TmFrame::into_plain`] returns it, and it is the only frame an engine's frame input takes
/// (`FrameReceiver`, LNK-050). It dereferences to the frame. A frame is `Received` (the octets),
/// then `Checked` (the [`TmFrame`] a decode returns: the FECF verified), then `Plain`; the steps
/// are types, so a frame cannot skip one (RS-023).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlainTmFrame<'a> {
    frame: TmFrame<'a>,
}

impl<'a> PlainTmFrame<'a> {
    /// The checked frame, without the proof that its SDLS processing is done.
    #[must_use]
    pub const fn into_frame(self) -> TmFrame<'a> {
        self.frame
    }
}

impl<'a> Deref for PlainTmFrame<'a> {
    type Target = TmFrame<'a>;

    fn deref(&self) -> &TmFrame<'a> {
        &self.frame
    }
}

impl<'a> TmFrame<'a> {
    /// The `Checked` to `Plain` step for a channel with no SDLS: `hook` is [`NoSecurity`], the
    /// identity, so the frame must have no Security Header or Trailer (RS-023, API-070,
    /// API-074). A channel with SDLS verifies the frame with its `SecurityHook` before the
    /// engine sees it, which v1 does not provide for the engine input (issue #230).
    ///
    /// # Errors
    /// `InvalidField` (`security_header_len`, or `security_trailer_len`) if the frame has a
    /// Security Header or Trailer: [`NoSecurity`] cannot verify it.
    pub fn into_plain(self, _hook: NoSecurity) -> Result<PlainTmFrame<'a>> {
        require_unsecured(self.security_header().len(), self.security_trailer().len())?;
        Ok(PlainTmFrame { frame: self })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    extern crate std;
    use std::vec::Vec;

    use aldis_core::ErrorKind;
    use aldis_core::ids::{SpacecraftId, VcId};
    use aldis_core::security::{NoSecurity, SecurityLayout};

    use crate::TmFrame;
    use crate::config::{
        DataFieldContent, TmMasterChannelConfig, TmPhysicalChannelConfig, TmVirtualChannelConfig,
    };

    /// A 12-octet frame of SCID 1, VC 0: header, two Security Header octets, three data octets
    /// (First Header Pointer 0), and one Security Trailer octet when the channel has SDLS.
    fn frame() -> Vec<u8> {
        std::vec![0x00, 0x10, 0, 0, 0x18, 0x00, 0xA5, 0xA5, 1, 2, 3, 0x5A]
    }

    #[test]
    fn a_frame_without_sdls_becomes_plain() {
        // req: RS-023, API-017, API-074
        let pc = TmPhysicalChannelConfig::new(12, false).unwrap();
        let bytes = frame();
        let plain = TmFrame::decode(&bytes, &pc)
            .unwrap()
            .into_plain(NoSecurity)
            .unwrap();
        assert_eq!(plain.as_bytes(), bytes.as_slice());
        assert_eq!(plain.clone().into_frame().len(), bytes.len());
    }

    #[test]
    fn a_frame_with_sdls_fields_does_not() {
        // req: RS-023, API-070
        let pc = TmPhysicalChannelConfig::new(12, false).unwrap();
        let vc = TmVirtualChannelConfig::new(VcId::<3>::new(0).unwrap(), DataFieldContent::Packets)
            .with_security(SecurityLayout::new(2, 1).unwrap());
        let mc = TmMasterChannelConfig::new(pc, SpacecraftId::<10>::new(1).unwrap(), None, false)
            .with_vc(vc)
            .unwrap();
        let bytes = frame();
        let e = TmFrame::decode_with(&bytes, &pc, &mc)
            .unwrap()
            .into_plain(NoSecurity)
            .unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidField);
    }
}
