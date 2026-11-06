//! The configuration of the SYNC functions: frame protocol, frame length, and ASM tolerance
//! (131.0 §11.1–§11.4; SYNC-002, SYNC-016, RS-020).

use aldis_core::{Error, ErrorKind, Result, field};

/// The largest ASM tolerance, in bit errors (OI-80, decided; SYNC-007, SYNC-016).
pub const MAX_ASM_ERRORS: u32 = 15;

/// The frame protocol whose fixed-length Transfer Frames the marker delimits (131.0 §11.1,
/// §11.3, §11.4; 132.0 §2.4.1; 732.0 §2.4.1; 732.1 §2.4.1; SYNC-002).
///
/// The protocol limits the frame length: TM 2048 octets, AOS and fixed-length USLP 65536.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyncProtocol {
    /// TM Transfer Frames, at most 2048 octets (131.0 §11.3; SYNC-002).
    Tm,
    /// AOS Transfer Frames, at most 65536 octets (131.0 §11.4; SYNC-002).
    Aos,
    /// Fixed-length USLP Transfer Frames, at most 65536 octets (131.0 §11.4; SYNC-002).
    Uslp,
}

impl SyncProtocol {
    /// The longest frame of the protocol with 131.0, in octets (131.0 §11.3, §11.4; SYNC-002).
    #[must_use]
    pub const fn max_frame_len(self) -> usize {
        match self {
            SyncProtocol::Tm => 2048,
            SyncProtocol::Aos | SyncProtocol::Uslp => 65536,
        }
    }
}

/// A number of ASM bit errors, 0 to 15 (OI-80, decided; SYNC-007, SYNC-011, SYNC-016).
///
/// It is the tolerance of the search and the number of differing bits of a match.
///
/// A high tolerance raises the false-lock rate: about 1 in 10^3 random 32-bit windows match at 7 and about 43 % at 15 (hazard HZ-SYNC-01, ASM-20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct AsmErrors(u8);

impl AsmErrors {
    /// No bit error: the exact marker (SYNC-007).
    pub const NONE: AsmErrors = AsmErrors(0);

    /// `count` bit errors.
    ///
    /// # Errors
    /// `InvalidConfig` naming `asm_errors` when `count` is above 15 (SYNC-016).
    pub const fn new(count: u32) -> Result<AsmErrors> {
        if count > MAX_ASM_ERRORS {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::ASM_ERRORS));
        }
        // `count` is at most 15 here, so the low octet holds it.
        Ok(AsmErrors(count.to_le_bytes()[0]))
    }

    /// The number of bit errors, 0 to 15 (SYNC-016).
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// The configuration of the SYNC functions: the frame protocol, the frame length, and the
/// tolerance of the ASM search, valid together (131.0 §11; SYNC-002, SYNC-016, RS-020).
///
/// Built only by [`new`](Self::new). The ASM attach function and the search engine both take
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SyncConfig {
    protocol: SyncProtocol,
    frame_len: usize,
    asm_errors: AsmErrors,
}

impl SyncConfig {
    /// A configuration for frames of `frame_len` octets of `protocol`, searched with a
    /// tolerance of `asm_errors` bit errors.
    ///
    /// # Errors
    /// `InvalidConfig` naming `frame_len` when the length is 0 or above the maximum of the
    /// protocol (SYNC-002).
    pub const fn new(
        protocol: SyncProtocol,
        frame_len: usize,
        asm_errors: AsmErrors,
    ) -> Result<SyncConfig> {
        if frame_len == 0 || frame_len > protocol.max_frame_len() {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::FRAME_LEN));
        }
        Ok(SyncConfig {
            protocol,
            frame_len,
            asm_errors,
        })
    }

    /// The frame protocol (SYNC-002).
    #[must_use]
    pub const fn protocol(&self) -> SyncProtocol {
        self.protocol
    }

    /// The frame length in octets, without the marker (131.0 §11.2.1; SYNC-002).
    #[must_use]
    pub const fn frame_len(&self) -> usize {
        self.frame_len
    }

    /// The ASM tolerance (SYNC-007, SYNC-016).
    #[must_use]
    pub const fn asm_errors(&self) -> AsmErrors {
        self.asm_errors
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{AsmErrors, SyncConfig, SyncProtocol};
    use aldis_core::{ErrorKind, field};

    #[test]
    fn tolerance_is_0_to_15() {
        // req: SYNC-016
        for n in 0..=15 {
            assert_eq!(AsmErrors::new(n).unwrap().get(), u8::try_from(n).unwrap());
        }
        for n in [16, 17, 255, 256, u32::MAX] {
            let e = AsmErrors::new(n).unwrap_err();
            assert_eq!(e.kind(), ErrorKind::InvalidConfig);
            assert_eq!(e.field(), Some(field::ASM_ERRORS));
        }
        assert_eq!(AsmErrors::default(), AsmErrors::NONE);
    }

    #[test]
    fn frame_length_is_limited_by_the_protocol() {
        // req: SYNC-002
        let t = AsmErrors::NONE;
        for (p, max) in [
            (SyncProtocol::Tm, 2048),
            (SyncProtocol::Aos, 65536),
            (SyncProtocol::Uslp, 65536),
        ] {
            assert_eq!(p.max_frame_len(), max);
            assert_eq!(SyncConfig::new(p, 1, t).unwrap().frame_len(), 1);
            assert_eq!(SyncConfig::new(p, max, t).unwrap().frame_len(), max);
            for bad in [0, max + 1] {
                let e = SyncConfig::new(p, bad, t).unwrap_err();
                assert_eq!(e.kind(), ErrorKind::InvalidConfig);
                assert_eq!(e.field(), Some(field::FRAME_LEN));
            }
        }
        let c = SyncConfig::new(SyncProtocol::Aos, 9, AsmErrors::new(3).unwrap()).unwrap();
        assert_eq!(c.protocol(), SyncProtocol::Aos);
        assert_eq!(c.asm_errors().get(), 3);
    }
}
