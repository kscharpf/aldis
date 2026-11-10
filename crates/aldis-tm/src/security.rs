//! The Security Header and Trailer of a tm frame, for the `Plain` step (RS-023, API-074).

use aldis_core::SecuritySlices;

use crate::view::TmFrame;

/// `TmFrame::into_plain(NoSecurity)` returns `aldis_core::Plain<TmFrame>`, the frame input of an engine
/// (RS-023, API-017, API-074; `FrameReceiver`, LNK-050); `aldis_core::Checked<TmFrame>` takes it
/// too. `SecuritySlices` has one implementation for `TmFrame`, and only this crate can write it:
/// another crate that implements it for `TmFrame` is refused (E0117).
///
/// ```compile_fail,E0117
/// use aldis_core::SecuritySlices;
/// use aldis_tm::TmFrame;
///
/// impl SecuritySlices for TmFrame<'_> {
///     fn security_header(&self) -> &[u8] {
///         &[]
///     }
///
///     fn security_trailer(&self) -> &[u8] {
///         &[]
///     }
/// }
/// ```
impl SecuritySlices for TmFrame<'_> {
    fn security_header(&self) -> &[u8] {
        TmFrame::security_header(self)
    }

    fn security_trailer(&self) -> &[u8] {
        TmFrame::security_trailer(self)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    extern crate std;
    use std::vec::Vec;

    use aldis_core::ErrorKind;
    use aldis_core::SecuritySlices;
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
