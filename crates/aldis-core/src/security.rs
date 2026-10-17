//! SDLS reservation: `SecurityLayout` and `SecurityHook` (API-070…API-074).
//!
//! v1 implements no cryptography. Every frame codec reserves the Security Header and Trailer
//! of the Space Data Link Security Protocol (355.0-B-2 §4.1) through these types, so SDLS can
//! be added later without breaking APIs. Engines call [`SecurityHook::protect`] after every
//! other frame field except the FECF is final, then compute the FECF; on reception the FECF
//! is checked before [`SecurityHook::verify`] (API-074; 355.0 §2.3, 232.0 Cor. 1 §6.5.2).

use core::ops::Range;

use crate::error::{Error, ErrorKind, Result};

/// Lengths of the Security Header and Trailer on one VC or MAP (API-070, API-071).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SecurityLayout {
    header_len: u8,
    trailer_len: u16,
}

impl SecurityLayout {
    /// No Security Header or Trailer.
    pub const NONE: SecurityLayout = SecurityLayout {
        header_len: 0,
        trailer_len: 0,
    };
    /// Largest Security Header, in octets (355.0 §4.1.1.1.4).
    pub const MAX_HEADER_LEN: usize = 64;
    /// Fixed layout for SDLS on USLP truncated frames: 6-octet header, 16-octet trailer
    /// (732.1 Annex D1.2.3; API-075).
    pub const USLP_TRUNCATED: SecurityLayout = SecurityLayout {
        header_len: 6,
        trailer_len: 16,
    };

    /// A layout with the given header and trailer lengths.
    ///
    /// # Errors
    /// `InvalidConfig` if the header exceeds 64 octets or the trailer exceeds 65 535 octets.
    pub fn new(header_len: usize, trailer_len: usize) -> Result<SecurityLayout> {
        let header = u8::try_from(header_len)
            .ok()
            .filter(|&h| usize::from(h) <= SecurityLayout::MAX_HEADER_LEN)
            .ok_or_else(|| {
                Error::new(ErrorKind::InvalidConfig)
                    .with_field(crate::error::FieldId::new("security_header_len"))
            })?;
        let trailer = u16::try_from(trailer_len).map_err(|_| {
            Error::new(ErrorKind::InvalidConfig)
                .with_field(crate::error::FieldId::new("security_trailer_len"))
        })?;
        Ok(SecurityLayout {
            header_len: header,
            trailer_len: trailer,
        })
    }

    /// Security Header length in octets.
    #[must_use]
    pub const fn header_len(self) -> usize {
        self.header_len as usize
    }

    /// Security Trailer length in octets.
    #[must_use]
    pub const fn trailer_len(self) -> usize {
        self.trailer_len as usize
    }

    /// Whether both lengths are zero.
    #[must_use]
    pub const fn is_none(self) -> bool {
        self.header_len == 0 && self.trailer_len == 0
    }
}

/// Data link protocol of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    /// TM Space Data Link Protocol (132.0-B-3).
    Tm,
    /// TC Space Data Link Protocol (232.0-B-4).
    Tc,
    /// AOS Space Data Link Protocol (732.0-B-5).
    Aos,
    /// Unified Space Data Link Protocol (732.1-B-3).
    Uslp,
}

/// Identifies the channel a frame belongs to, for selecting a Security Association.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameCtx {
    /// Data link protocol.
    pub protocol: Protocol,
    /// Spacecraft ID.
    pub scid: u16,
    /// Virtual Channel ID.
    pub vcid: u8,
    /// MAP ID, where SDLS is applied per MAP (TC, USLP).
    pub map_id: Option<u8>,
}

/// Octet ranges of the secured fields within a frame buffer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecuredFields {
    /// Security Header.
    pub header: Range<usize>,
    /// Data field protected by the hook.
    pub data: Range<usize>,
    /// Security Trailer.
    pub trailer: Range<usize>,
}

/// Applies and checks link-layer security (API-073, API-074).
pub trait SecurityHook {
    /// Fills the Security Header and Trailer, and transforms the data field in place.
    ///
    /// # Errors
    /// `Security` if protection fails; the frame must then not be transmitted.
    fn protect(&mut self, ctx: &FrameCtx, frame: &mut [u8], fields: &SecuredFields) -> Result<()>;

    /// Checks the Security Header and Trailer, transforms the data field in place, and returns
    /// the range of the plain data.
    ///
    /// # Errors
    /// `Security` if verification fails; the frame must then be discarded.
    fn verify(
        &mut self,
        ctx: &FrameCtx,
        frame: &mut [u8],
        fields: &SecuredFields,
    ) -> Result<Range<usize>>;
}

/// The identity hook for channels without SDLS (layout 0/0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NoSecurity;

impl SecurityHook for NoSecurity {
    fn protect(&mut self, _: &FrameCtx, _: &mut [u8], _: &SecuredFields) -> Result<()> {
        Ok(())
    }

    fn verify(
        &mut self,
        _: &FrameCtx,
        _: &mut [u8],
        fields: &SecuredFields,
    ) -> Result<Range<usize>> {
        Ok(fields.data.clone())
    }
}

/// Test-only hook: writes `0xA5` into the Security Header and `0x5A` into the Trailer, and
/// checks them on verify. Exercises length accounting and field placement (API-073). Never
/// enable feature `test-hooks` in a flight build.
#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FixedPatternSecurity;

#[cfg(any(test, feature = "test-hooks"))]
impl FixedPatternSecurity {
    /// Pattern written into every Security Header octet.
    pub const HEADER_PATTERN: u8 = 0xA5;
    /// Pattern written into every Security Trailer octet.
    pub const TRAILER_PATTERN: u8 = 0x5A;
}

#[cfg(any(test, feature = "test-hooks"))]
impl SecurityHook for FixedPatternSecurity {
    fn protect(&mut self, _: &FrameCtx, frame: &mut [u8], fields: &SecuredFields) -> Result<()> {
        for (range, pattern) in [
            (&fields.header, FixedPatternSecurity::HEADER_PATTERN),
            (&fields.trailer, FixedPatternSecurity::TRAILER_PATTERN),
        ] {
            frame
                .get_mut(range.clone())
                .ok_or(Error::new(ErrorKind::Security))?
                .fill(pattern);
        }
        Ok(())
    }

    fn verify(
        &mut self,
        _: &FrameCtx,
        frame: &mut [u8],
        fields: &SecuredFields,
    ) -> Result<Range<usize>> {
        let ok = |range: &Range<usize>, pattern: u8| {
            frame
                .get(range.clone())
                .is_some_and(|s| s.iter().all(|&b| b == pattern))
        };
        if ok(&fields.header, FixedPatternSecurity::HEADER_PATTERN)
            && ok(&fields.trailer, FixedPatternSecurity::TRAILER_PATTERN)
        {
            Ok(fields.data.clone())
        } else {
            Err(Error::new(ErrorKind::Security))
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{
        FixedPatternSecurity, FrameCtx, NoSecurity, Protocol, SecuredFields, SecurityHook,
        SecurityLayout,
    };
    use crate::error::ErrorKind;

    const CTX: FrameCtx = FrameCtx {
        protocol: Protocol::Tm,
        scid: 42,
        vcid: 1,
        map_id: None,
    };

    #[test]
    fn layout_limits() {
        // req: API-070, API-071, API-075
        assert!(SecurityLayout::NONE.is_none());
        assert_eq!(SecurityLayout::default(), SecurityLayout::NONE);
        let l = SecurityLayout::new(64, 16).unwrap();
        assert_eq!(
            (l.header_len(), l.trailer_len(), l.is_none()),
            (64, 16, false)
        );
        assert_eq!(
            SecurityLayout::new(65, 0).unwrap_err().kind(),
            ErrorKind::InvalidConfig
        );
        assert_eq!(
            SecurityLayout::new(300, 0).unwrap_err().kind(),
            ErrorKind::InvalidConfig
        );
        assert_eq!(
            SecurityLayout::new(0, 70_000).unwrap_err().kind(),
            ErrorKind::InvalidConfig
        );
        assert_eq!(
            SecurityLayout::USLP_TRUNCATED,
            SecurityLayout::new(6, 16).unwrap()
        );
    }

    #[test]
    fn hooks() {
        // req: API-073
        let fields = SecuredFields {
            header: 1..3,
            data: 3..5,
            trailer: 5..7,
        };
        let mut frame = [0_u8; 8];
        NoSecurity.protect(&CTX, &mut frame, &fields).unwrap();
        assert_eq!(frame, [0; 8]);
        assert_eq!(NoSecurity.verify(&CTX, &mut frame, &fields), Ok(3..5));
        let mut hook = FixedPatternSecurity;
        hook.protect(&CTX, &mut frame, &fields).unwrap();
        assert_eq!(frame, [0, 0xA5, 0xA5, 0, 0, 0x5A, 0x5A, 0]);
        assert_eq!(hook.verify(&CTX, &mut frame, &fields), Ok(3..5));
        frame[6] = 0;
        assert_eq!(
            hook.verify(&CTX, &mut frame, &fields).unwrap_err().kind(),
            ErrorKind::Security
        );
        let outside = SecuredFields {
            header: 7..9,
            data: 0..0,
            trailer: 0..0,
        };
        assert_eq!(
            hook.protect(&CTX, &mut frame, &outside).unwrap_err().kind(),
            ErrorKind::Security
        );
        assert_eq!(
            hook.verify(&CTX, &mut frame, &outside).unwrap_err().kind(),
            ErrorKind::Security
        );
    }
}
