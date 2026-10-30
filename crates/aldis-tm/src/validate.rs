//! A decoded frame checked against its master channel's managed parameters (132.0-B-3 §5,
//! §4.1.2.4.3, §4.1.2.7.2.3, §4.1.2.7.3.3, §4.1.3.2.3.3, §6.6; TM-011).

use core::ops::Deref;

use aldis_core::{Error, ErrorKind, Result};

use crate::config::{TmMasterChannelConfig, unknown_scid};
use crate::field;
use crate::header::{DataFieldStatus, FshLen, PacketPointer, len_u64, mismatch};
use crate::view::TmFrame;

/// A [`TmFrame`] that matches the managed parameters of a master channel (132.0 §5 Tables 5-1 to
/// 5-3, §6.6 Table 6-1; TM-011, API-016).
///
/// [`TmFrame::validate`] returns it. It dereferences to the frame; holding one is the proof that
/// the frame has the frame length, the SCID, the flags, and the lengths of its virtual channel
/// (RS-023 `Checked`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedTmFrame<'a> {
    frame: TmFrame<'a>,
}

impl<'a> ValidatedTmFrame<'a> {
    /// The frame, without the proof.
    #[must_use]
    pub const fn into_frame(self) -> TmFrame<'a> {
        self.frame
    }
}

impl<'a> Deref for ValidatedTmFrame<'a> {
    type Target = TmFrame<'a>;

    fn deref(&self) -> &TmFrame<'a> {
        &self.frame
    }
}

impl<'a> TmFrame<'a> {
    /// Checks the frame against the managed parameters of `mc` and its virtual channels
    /// (132.0 §5 Tables 5-1 to 5-3, §6.6 Table 6-1; TM-011, API-016) and returns it as a
    /// [`ValidatedTmFrame`].
    ///
    /// # Errors
    /// In this order, each with the configured value and the value found:
    /// - `LengthMismatch` (`frame_len`) if the frame is not `mc`'s frame length (Table 5-1);
    /// - `UnknownChannel` (`spacecraft_id`) for another SCID, and (`vc_id`) for a VCID `mc` does
    ///   not configure (132.0 §4.3.4.3, §4.3.6.3; Table 5-2);
    /// - `InvalidField` (`ocf_flag`) if the OCF Flag disagrees with the `MC_OCF` or `VC_OCF`
    ///   (132.0 §4.1.2.4.3);
    /// - `InvalidField` (`fsh_flag`) if the Secondary Header Flag disagrees with the `MC_FSH` or
    ///   `VC_FSH` (132.0 §4.1.2.7.2.3), and `LengthMismatch` (`fsh_len`) for another Secondary
    ///   Header length (132.0 §4.1.3.2.3.3);
    /// - `InvalidField` (`sync_flag`) if the Synchronization Flag disagrees with the VC's Data
    ///   Field Content (132.0 §4.1.2.7.3.3, §4.1.4.4);
    /// - `LengthMismatch` (`security_header_len` or `security_trailer_len`) if the frame was
    ///   decoded with another security layout than the VC's (Table 6-1; API-070);
    /// - `InvalidField` (`vc_id`) for an Only Idle Data frame (First Header Pointer `0x7FE`) on a
    ///   VC that uses SDLS, which a lenient decode accepts with an anomaly flag (132.0 §6.4.6.2;
    ///   TM-010, OI-54).
    pub fn validate(self, mc: &TmMasterChannelConfig) -> Result<ValidatedTmFrame<'a>> {
        let frame_len = mc.pc().frame_len();
        if self.len() != frame_len {
            return Err(mismatch(
                ErrorKind::LengthMismatch,
                field::FRAME_LEN,
                len_u64(frame_len),
                len_u64(self.len()),
            ));
        }
        if self.scid() != mc.scid() {
            return Err(unknown_scid(self.scid(), mc.scid()));
        }
        let vc = mc
            .vc(self.vcid())
            .ok_or(Error::new(ErrorKind::UnknownChannel).with_field(field::VC_ID))?;
        let header = self.header();
        let flags = [
            (header.ocf_flag, mc.ocf_of(vc), field::OCF_FLAG),
            (header.fsh_flag, mc.fsh_of(vc).is_some(), field::FSH_FLAG),
            (header.sync_flag, vc.content().sync_flag(), field::SYNC_FLAG),
        ];
        // The Secondary Header length is compared after its flag, the SDLS lengths last.
        let lengths = [
            (
                self.fsh_len().map_or(0, FshLen::octets),
                mc.fsh_of(vc).map_or(0, FshLen::octets),
                field::FSH_LEN,
            ),
            (
                self.security_header().len(),
                vc.security().header_len(),
                field::SECURITY_HEADER_LEN,
            ),
            (
                self.security_trailer().len(),
                vc.security().trailer_len(),
                field::SECURITY_TRAILER_LEN,
            ),
        ];
        for (found, expected, f) in flags {
            if found != expected {
                return Err(mismatch(
                    ErrorKind::InvalidField,
                    f,
                    u64::from(expected),
                    u64::from(found),
                ));
            }
        }
        for (found, expected, f) in lengths {
            if found != expected {
                return Err(mismatch(
                    ErrorKind::LengthMismatch,
                    f,
                    len_u64(expected),
                    len_u64(found),
                ));
            }
        }
        if self.data_field_status() == DataFieldStatus::Packets(PacketPointer::OnlyIdleData)
            && !vc.security().is_none()
        {
            return Err(Error::new(ErrorKind::InvalidField).with_field(field::VC_ID));
        }
        Ok(ValidatedTmFrame { frame: self })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    extern crate std;
    use std::vec;
    use std::vec::Vec;

    use crate::config::{
        DataFieldContent, NoSdls, TmMasterChannelConfig, TmPhysicalChannelConfig,
        TmVirtualChannelConfig,
    };
    use crate::field;
    use crate::header::FshLen;
    use crate::view::TmFrame;
    use aldis_core::ids::{SpacecraftId, VcId};
    use aldis_core::security::SecurityLayout;
    use aldis_core::{ErrorDetail, ErrorKind, FieldId};

    const PC: TmPhysicalChannelConfig = match TmPhysicalChannelConfig::new(24, false) {
        Ok(pc) => pc,
        Err(_) => panic!("valid"),
    };

    fn vcid(n: u8) -> VcId<3> {
        VcId::new(n).unwrap()
    }

    /// SCID 5: VC 0 packets with a 3-octet `VC_FSH`; VC 1 `VCA_SDUs` with a `VC_OCF`; VC 2 packets
    /// with SDLS 2/2.
    fn mc() -> TmMasterChannelConfig {
        TmMasterChannelConfig::new(PC, SpacecraftId::new(5).unwrap(), None, false)
            .with_vc(
                TmVirtualChannelConfig::new(vcid(0), DataFieldContent::Packets)
                    .with_fsh(FshLen::new(3).unwrap()),
            )
            .unwrap()
            .with_vc(TmVirtualChannelConfig::new(vcid(1), DataFieldContent::VcaSdu).with_ocf())
            .unwrap()
            .with_vc(
                TmVirtualChannelConfig::new(vcid(2), DataFieldContent::Packets)
                    .with_security(SecurityLayout::new(2, 2).unwrap()),
            )
            .unwrap()
    }

    /// A 24-octet frame of SCID 5 on `vc` with octet 4 (flags) `status`, and an FSH of `fsh`
    /// octets if non-zero.
    fn frame(vc: u8, ocf: bool, status: u8, fsh: u8) -> Vec<u8> {
        let mut f = vec![0x00, 0x50 | (vc << 1) | u8::from(ocf), 0, 0, status, 0];
        if fsh > 0 {
            f.push(fsh.saturating_sub(1));
        }
        f.resize(24, 0);
        f
    }

    fn check(
        bytes: &[u8],
        mc: &TmMasterChannelConfig,
    ) -> (ErrorKind, Option<FieldId>, ErrorDetail) {
        let f = TmFrame::decode(bytes, mc.pc()).unwrap();
        let e = f.validate(mc).unwrap_err();
        (e.kind(), e.field(), e.detail())
    }

    const fn exp(expected: u64, found: u64) -> ErrorDetail {
        ErrorDetail::Expected { expected, found }
    }

    #[test]
    fn matching_frames_pass() {
        // req: TM-011, API-016
        let mc = mc();
        for (bytes, sdls) in [
            (frame(0, false, 0x98, 3), false),
            (frame(1, true, 0x40, 0), false),
            (frame(2, false, 0x18, 0), true),
        ] {
            let f = if sdls {
                TmFrame::decode_with(&bytes, mc.pc(), &mc)
            } else {
                TmFrame::decode(&bytes, mc.pc())
            }
            .unwrap();
            assert!(f.validate(&mc).is_ok());
        }
    }

    #[test]
    fn channel_and_length_mismatches() {
        // req: TM-011, TM-001
        let mc = mc();
        let mut other_scid = frame(0, false, 0x98, 3);
        other_scid[1] = (other_scid[1] & 0x0F) | 0x40; // SCID 4
        assert_eq!(
            check(&other_scid, &mc),
            (
                ErrorKind::UnknownChannel,
                Some(field::SPACECRAFT_ID),
                exp(5, 4)
            )
        );
        let (kind, f, _) = check(&frame(3, false, 0x18, 0), &mc);
        assert_eq!((kind, f), (ErrorKind::UnknownChannel, Some(field::VC_ID)));
        let short = TmPhysicalChannelConfig::new(23, false).unwrap();
        let bytes = frame(0, false, 0x98, 3);
        let f = TmFrame::decode(&bytes[..23], &short).unwrap();
        let e = f.validate(&mc).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::LengthMismatch,
                Some(field::FRAME_LEN),
                exp(24, 23)
            )
        );
    }

    #[test]
    fn flag_and_length_mismatches() {
        // req: TM-011, API-016
        let mc = mc();
        let invalid = |f| (ErrorKind::InvalidField, Some(f), exp(0, 1));
        assert_eq!(
            check(&frame(0, true, 0x98, 3), &mc),
            invalid(field::OCF_FLAG)
        );
        assert_eq!(
            check(&frame(1, false, 0x40, 0), &mc),
            (ErrorKind::InvalidField, Some(field::OCF_FLAG), exp(1, 0))
        );
        assert_eq!(
            check(&frame(0, false, 0x18, 0), &mc),
            (ErrorKind::InvalidField, Some(field::FSH_FLAG), exp(1, 0))
        );
        assert_eq!(
            check(&frame(2, false, 0x98, 3), &mc),
            invalid(field::FSH_FLAG)
        );
        assert_eq!(
            check(&frame(0, false, 0x98, 4), &mc),
            (ErrorKind::LengthMismatch, Some(field::FSH_LEN), exp(3, 4))
        );
        assert_eq!(
            check(&frame(0, false, 0xD8, 3), &mc),
            invalid(field::SYNC_FLAG)
        );
        // Decoded as if VC 2 had no SDLS.
        assert_eq!(
            check(&frame(2, false, 0x18, 0), &mc),
            (
                ErrorKind::LengthMismatch,
                Some(field::SECURITY_HEADER_LEN),
                exp(2, 0)
            )
        );
    }

    #[test]
    fn trailer_mismatch() {
        // req: TM-011, API-070
        let mc = mc();
        let bytes = frame(2, false, 0x18, 0);
        let header_only =
            TmMasterChannelConfig::new(PC, SpacecraftId::new(5).unwrap(), None, false)
                .with_vc(
                    TmVirtualChannelConfig::new(vcid(2), DataFieldContent::Packets)
                        .with_security(SecurityLayout::new(2, 0).unwrap()),
                )
                .unwrap();
        let f = TmFrame::decode_with(&bytes, &PC, &header_only).unwrap();
        let e = f.validate(&mc).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::LengthMismatch,
                Some(field::SECURITY_TRAILER_LEN),
                exp(2, 0)
            )
        );
        assert!(TmFrame::decode_with(&bytes, &PC, &NoSdls).is_ok());
    }

    #[test]
    fn only_idle_data_on_an_sdls_vc_is_refused() {
        // req: TM-010, TM-011 (OI-54)
        let mc = mc();
        // VC 2 uses SDLS; FHP 0x7FE, Segment Length ID 11. A lenient decode accepts it.
        let mut bytes = frame(2, false, 0x1F, 0);
        bytes[5] = 0xFE;
        let f = TmFrame::decode_with_lenient(&bytes, &PC, &mc).unwrap();
        let e = f.into_view().validate(&mc).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidField, Some(field::VC_ID))
        );
        // The same frame on VC 0 (no SDLS, a Secondary Header of 3 octets) is valid.
        let mut ok = frame(0, false, 0x9F, 3);
        ok[5] = 0xFE;
        let f = TmFrame::decode(&ok, &PC).unwrap();
        assert!(f.validate(&mc).is_ok());
    }
}
