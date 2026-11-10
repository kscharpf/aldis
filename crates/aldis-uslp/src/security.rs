//! The Security Header and Trailer of a uslp frame, for the `Plain` step (RS-023, API-074).

use aldis_core::SecuritySlices;

use crate::view::UslpFrame;

/// `UslpFrame::into_plain(NoSecurity)` returns `aldis_core::Plain<UslpFrame>`, the frame input of an engine
/// (RS-023, API-017, API-074; `FrameReceiver`, LNK-050); `aldis_core::Checked<UslpFrame>` takes it
/// too. `SecuritySlices` has one implementation for `UslpFrame`, and only this crate can write it:
/// another crate that implements it for `UslpFrame` is refused (E0117).
impl SecuritySlices for UslpFrame<'_> {
    fn security_header(&self) -> &[u8] {
        UslpFrame::security_header(self)
    }

    fn security_trailer(&self) -> &[u8] {
        UslpFrame::security_trailer(self)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    extern crate std;
    use std::vec::Vec;

    use aldis_core::ErrorKind;
    use aldis_core::SecuritySlices;
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
