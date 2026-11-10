//! The Security Header and Trailer of a TC frame, for the `Plain` step (RS-023, API-074).

use aldis_core::SecuritySlices;

use crate::view::TcFrame;

/// `TcFrame::into_plain(NoSecurity)` returns `aldis_core::Plain<TcFrame>`, the frame input of an engine
/// (RS-023, API-017, API-074; `FrameReceiver`, LNK-050); `aldis_core::Checked<TcFrame>` takes it
/// too. `SecuritySlices` has one implementation for `TcFrame`, and only this crate can write it:
/// another crate that implements it for `TcFrame` is refused (E0117).
impl SecuritySlices for TcFrame<'_> {
    fn security_header(&self) -> &[u8] {
        TcFrame::security_header(self)
    }

    fn security_trailer(&self) -> &[u8] {
        TcFrame::security_trailer(self)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use aldis_core::ErrorKind;
    use aldis_core::SecuritySlices;
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
