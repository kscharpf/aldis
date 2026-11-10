//! The Security Header and Trailer of a aos frame, for the `Plain` step (RS-023, API-074).

use aldis_core::SecuritySlices;

use crate::view::AosFrame;

/// `AosFrame::into_plain(NoSecurity)` returns `aldis_core::Plain<AosFrame>`, the frame input of an engine
/// (RS-023, API-017, API-074; `FrameReceiver`, LNK-050); `aldis_core::Checked<AosFrame>` takes it
/// too. `SecuritySlices` has one implementation for `AosFrame`, and only this crate can write it:
/// another crate that implements it for `AosFrame` is refused (E0117).
impl SecuritySlices for AosFrame<'_> {
    fn security_header(&self) -> &[u8] {
        AosFrame::security_header(self)
    }

    fn security_trailer(&self) -> &[u8] {
        AosFrame::security_trailer(self)
    }
}
