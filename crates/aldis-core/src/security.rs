//! SDLS reservation: `SecurityLayout` and `SecurityHook` (API-070…API-074).
//!
//! v1 implements no cryptography. Every frame codec reserves the Security Header and Trailer
//! of the Space Data Link Security Protocol (355.0-B-2 §4.1) through these types, so SDLS can
//! be added later without breaking APIs. Engines call [`SecurityHook::protect`] after every
//! other frame field except the FECF is final, then compute the FECF; on reception the FECF
//! is checked before [`SecurityHook::verify`] (API-074; 355.0 §2.3, 232.0 Cor. 1 §6.5.2).

use core::ops::Range;

use crate::error::{
    Error, ErrorKind, Result, u8_to_usize, u16_to_usize, usize_to_u8, usize_to_u16,
};
use crate::field;
use crate::ids::{MapId, SpacecraftId, VcId};
use crate::layout::FrameLayout;

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
    /// Largest Security Header, in octets (355.0 §4.1.1.1.4; API-070, API-071).
    pub const MAX_HEADER_LEN: usize = 64;
    /// Largest Security Trailer, in octets (API-070, API-071).
    pub const MAX_TRAILER_LEN: usize = 0xFFFF;
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
    pub const fn new(header_len: usize, trailer_len: usize) -> Result<SecurityLayout> {
        let Some(header_len) = usize_to_u8(header_len, SecurityLayout::MAX_HEADER_LEN) else {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::SECURITY_HEADER_LEN));
        };
        let Some(trailer_len) = usize_to_u16(trailer_len, SecurityLayout::MAX_TRAILER_LEN) else {
            return Err(
                Error::new(ErrorKind::InvalidConfig).with_field(field::SECURITY_TRAILER_LEN)
            );
        };
        Ok(SecurityLayout {
            header_len,
            trailer_len,
        })
    }

    /// Security Header length in octets.
    #[must_use]
    pub const fn header_len(self) -> usize {
        u8_to_usize(self.header_len)
    }

    /// Security Trailer length in octets.
    #[must_use]
    pub const fn trailer_len(self) -> usize {
        u16_to_usize(self.trailer_len)
    }

    /// Whether both lengths are zero.
    #[must_use]
    pub const fn is_none(self) -> bool {
        self.header_len == 0 && self.trailer_len == 0
    }
}

/// Data link protocol of a frame (355.0-B-2 §4.1; API-073, GEN-081).
#[non_exhaustive]
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

/// Identifies the channel a frame belongs to, for selecting a Security Association
/// (355.0-B-2 §2.2.3, §4.1; API-073).
///
/// One variant per protocol carries that protocol's identifier widths, so a context can hold
/// only identifiers its protocol allows (API-002, RS-020).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameCtx {
    /// TM frame: 10-bit SCID, 3-bit VCID (132.0 §4.1.2.2–3).
    Tm {
        /// Spacecraft ID.
        scid: SpacecraftId<10>,
        /// Virtual Channel ID.
        vcid: VcId<3>,
    },
    /// TC frame: 10-bit SCID, 6-bit VCID, and the MAP ID when SDLS is applied per MAP (232.0
    /// §4.1.2.5–6, §4.2.2.2).
    Tc {
        /// Spacecraft ID.
        scid: SpacecraftId<10>,
        /// Virtual Channel ID.
        vcid: VcId<6>,
        /// MAP ID, if security is applied per MAP.
        map_id: Option<MapId<6>>,
    },
    /// AOS frame: 6-bit VCID and a 10-bit SCID (732.0 §4.1.2.2.3); Issue ≤4 SCIDs are 8 bits
    /// and fit unchanged.
    Aos {
        /// Spacecraft ID.
        scid: SpacecraftId<10>,
        /// Virtual Channel ID.
        vcid: VcId<6>,
    },
    /// USLP frame: 16-bit SCID, 6-bit VCID, and the MAP ID when SDLS is applied per MAP (732.1
    /// §4.1.2.2.3, §4.1.2.4–5).
    Uslp {
        /// Spacecraft ID.
        scid: SpacecraftId<16>,
        /// Virtual Channel ID.
        vcid: VcId<6>,
        /// MAP ID, if security is applied per MAP.
        map_id: Option<MapId<4>>,
    },
}

impl FrameCtx {
    /// The data link protocol (API-073).
    #[must_use]
    pub const fn protocol(&self) -> Protocol {
        match self {
            FrameCtx::Tm { .. } => Protocol::Tm,
            FrameCtx::Tc { .. } => Protocol::Tc,
            FrameCtx::Aos { .. } => Protocol::Aos,
            FrameCtx::Uslp { .. } => Protocol::Uslp,
        }
    }

    /// The Spacecraft ID as a number (API-073).
    #[must_use]
    pub const fn scid(&self) -> u16 {
        match self {
            FrameCtx::Tm { scid, .. } | FrameCtx::Tc { scid, .. } | FrameCtx::Aos { scid, .. } => {
                scid.get()
            }
            FrameCtx::Uslp { scid, .. } => scid.get(),
        }
    }

    /// The Virtual Channel ID as a number (API-073).
    #[must_use]
    pub const fn vcid(&self) -> u8 {
        match self {
            FrameCtx::Tm { vcid, .. } => vcid.get(),
            FrameCtx::Tc { vcid, .. }
            | FrameCtx::Aos { vcid, .. }
            | FrameCtx::Uslp { vcid, .. } => vcid.get(),
        }
    }

    /// The MAP ID as a number, for TC and USLP contexts that carry one (API-073).
    #[must_use]
    pub const fn map_id(&self) -> Option<u8> {
        match self {
            FrameCtx::Tc {
                map_id: Some(map), ..
            } => Some(map.get()),
            FrameCtx::Uslp {
                map_id: Some(map), ..
            } => Some(map.get()),
            _ => None,
        }
    }
}

/// Octet ranges of the secured fields within a frame buffer: Security Header, data field, and
/// Security Trailer, in order and adjacent (355.0-B-2 §4.1; API-070, API-073, RS-020).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecuredFields {
    header: Range<usize>,
    data: Range<usize>,
    trailer: Range<usize>,
}

impl SecuredFields {
    /// The secured fields from three ranges.
    ///
    /// # Errors
    /// `InvalidField` (`secured_fields`) unless each range has `start <= end`, the data field
    /// starts where the header ends, and the trailer starts where the data field ends.
    pub const fn new(
        header: Range<usize>,
        data: Range<usize>,
        trailer: Range<usize>,
    ) -> Result<Self> {
        let ordered =
            header.start <= header.end && data.start <= data.end && trailer.start <= trailer.end;
        if !ordered || header.end != data.start || data.end != trailer.start {
            return Err(Error::new(ErrorKind::InvalidField).with_field(field::SECURED_FIELDS));
        }
        Ok(SecuredFields {
            header,
            data,
            trailer,
        })
    }

    /// The secured fields of a computed frame layout, which are adjacent by construction
    /// (CMN-040, API-070).
    #[must_use]
    pub fn from_layout(layout: &FrameLayout) -> SecuredFields {
        SecuredFields {
            header: layout.security_header(),
            data: layout.data(),
            trailer: layout.security_trailer(),
        }
    }

    /// Security Header (355.0-B-2 §4.1.1).
    #[must_use]
    pub fn header(&self) -> Range<usize> {
        self.header.clone()
    }

    /// Data field protected by the hook (355.0-B-2 §4.1).
    #[must_use]
    pub fn data(&self) -> Range<usize> {
        self.data.clone()
    }

    /// Security Trailer (355.0-B-2 §4.1.2).
    #[must_use]
    pub fn trailer(&self) -> Range<usize> {
        self.trailer.clone()
    }
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
        Ok(fields.data())
    }
}

/// Test-only hook that writes and checks fixed patterns (API-073).
///
/// Writes `0xA5` into the Security Header and `0x5A` into the Trailer, and checks them on
/// verify. Exercises length accounting and field placement. Never enable feature `test-hooks`
/// in a flight build.
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
            (fields.header(), FixedPatternSecurity::HEADER_PATTERN),
            (fields.trailer(), FixedPatternSecurity::TRAILER_PATTERN),
        ] {
            frame
                .get_mut(range)
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
        let ok = |range: Range<usize>, pattern: u8| {
            frame
                .get(range)
                .is_some_and(|s| s.iter().all(|&b| b == pattern))
        };
        if ok(fields.header(), FixedPatternSecurity::HEADER_PATTERN)
            && ok(fields.trailer(), FixedPatternSecurity::TRAILER_PATTERN)
        {
            Ok(fields.data())
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
    use crate::field;
    use crate::ids::{MapId, SpacecraftId, VcId};
    use crate::layout::{FrameLayout, LayoutSpec};

    const CTX: FrameCtx = FrameCtx::Tm {
        scid: match SpacecraftId::<10>::new(42) {
            Some(id) => id,
            None => panic!("valid SCID"),
        },
        vcid: match VcId::<3>::new(1) {
            Some(id) => id,
            None => panic!("valid VCID"),
        },
    };

    #[test]
    fn frame_contexts_carry_typed_ids() {
        // req: API-002, API-073
        let tc = FrameCtx::Tc {
            scid: SpacecraftId::new(1023).unwrap(),
            vcid: VcId::new(63).unwrap(),
            map_id: Some(MapId::new(63).unwrap()),
        };
        let aos = FrameCtx::Aos {
            scid: SpacecraftId::new(255).unwrap(),
            vcid: VcId::new(62).unwrap(),
        };
        let uslp = FrameCtx::Uslp {
            scid: SpacecraftId::new(65_535).unwrap(),
            vcid: VcId::new(5).unwrap(),
            map_id: Some(MapId::new(15).unwrap()),
        };
        let uslp_vc = FrameCtx::Uslp {
            scid: SpacecraftId::new(7).unwrap(),
            vcid: VcId::new(6).unwrap(),
            map_id: None,
        };
        let tc_vc = FrameCtx::Tc {
            scid: SpacecraftId::new(9).unwrap(),
            vcid: VcId::new(2).unwrap(),
            map_id: None,
        };
        assert_eq!(
            (CTX.protocol(), CTX.scid(), CTX.vcid(), CTX.map_id()),
            (Protocol::Tm, 42, 1, None)
        );
        assert_eq!(
            (tc.protocol(), tc.scid(), tc.vcid(), tc.map_id()),
            (Protocol::Tc, 1023, 63, Some(63))
        );
        assert_eq!(
            (aos.protocol(), aos.scid(), aos.vcid(), aos.map_id()),
            (Protocol::Aos, 255, 62, None)
        );
        assert_eq!(
            (uslp.protocol(), uslp.scid(), uslp.vcid(), uslp.map_id()),
            (Protocol::Uslp, 65_535, 5, Some(15))
        );
        assert_eq!(
            (
                uslp_vc.map_id(),
                tc_vc.map_id(),
                tc_vc.vcid(),
                uslp_vc.scid()
            ),
            (None, None, 2, 7)
        );
    }

    #[test]
    fn secured_fields_are_ordered_and_adjacent() {
        // req: API-070, API-073
        let f = SecuredFields::new(1..3, 3..5, 5..7).unwrap();
        assert_eq!((f.header(), f.data(), f.trailer()), (1..3, 3..5, 5..7));
        assert!(SecuredFields::new(0..0, 0..4, 4..4).is_ok());
        #[expect(
            clippy::reversed_empty_ranges,
            reason = "testing that reversed ranges are rejected"
        )]
        let bad = [
            (1..3, 4..5, 5..7),
            (1..3, 3..5, 6..7),
            (3..1, 1..5, 5..7),
            (1..3, 3..2, 2..7),
            (1..3, 3..5, 5..4),
        ];
        for (h, d, t) in bad {
            let e = SecuredFields::new(h.clone(), d.clone(), t.clone()).unwrap_err();
            assert_eq!(
                (e.kind(), e.field()),
                (ErrorKind::InvalidField, Some(field::SECURED_FIELDS)),
                "{h:?} {d:?} {t:?}"
            );
        }
        let layout = FrameLayout::compute(&LayoutSpec {
            frame_len: 20,
            primary_header_len: 6,
            secondary_len: 0,
            security: SecurityLayout::new(2, 3).unwrap(),
            ocf: false,
            fecf: true,
        })
        .unwrap();
        let f = SecuredFields::from_layout(&layout);
        assert_eq!((f.header(), f.data(), f.trailer()), (6..8, 8..15, 15..18));
    }

    #[test]
    fn layout_limits() {
        // req: API-070, API-071, API-075
        const _: () = assert!(core::mem::size_of::<SecurityLayout>() == 4);
        assert!(SecurityLayout::NONE.is_none());
        assert_eq!(SecurityLayout::default(), SecurityLayout::NONE);
        let l = SecurityLayout::new(
            SecurityLayout::MAX_HEADER_LEN,
            SecurityLayout::MAX_TRAILER_LEN,
        )
        .unwrap();
        assert_eq!(
            (l.header_len(), l.trailer_len(), l.is_none()),
            (64, 0xFFFF, false)
        );
        // Either length alone makes the layout non-empty.
        assert!(!SecurityLayout::new(6, 0).unwrap().is_none());
        assert!(!SecurityLayout::new(0, 16).unwrap().is_none());
        let e = SecurityLayout::new(65, 0).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidConfig, Some(field::SECURITY_HEADER_LEN))
        );
        assert_eq!(
            SecurityLayout::new(300, 0).unwrap_err().kind(),
            ErrorKind::InvalidConfig
        );
        let e = SecurityLayout::new(0, 70_000).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidConfig, Some(field::SECURITY_TRAILER_LEN))
        );
        assert_eq!(
            SecurityLayout::USLP_TRUNCATED,
            SecurityLayout::new(6, 16).unwrap()
        );
    }

    #[test]
    fn hooks() {
        // req: API-073
        let fields = SecuredFields::new(1..3, 3..5, 5..7).unwrap();
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
        let outside = SecuredFields::new(7..9, 9..9, 9..9).unwrap();
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
