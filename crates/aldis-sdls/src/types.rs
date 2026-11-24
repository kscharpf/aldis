//! Typed wire values and managed parameters for CCSDS 355.0-B-2 (RS-020).

use aldis_core::error::{Error, ErrorKind, Result};
use aldis_core::field;

/// Lossless `u8 -> usize` widening in `const fn`.
#[expect(clippy::as_conversions, reason = "u8 widens losslessly to usize")]
const fn u8_to_usize(v: u8) -> usize {
    v as usize
}

/// Lossless `u64 -> u128` widening in `const fn`.
#[expect(clippy::as_conversions, reason = "u64 widens losslessly to u128")]
const fn u64_to_u128(v: u64) -> u128 {
    v as u128
}

/// Security Parameter Index, 1..=65534 (355.0-B-2 §4.1.1.2.3, Table 6-1; RS-020).
///
/// Values 0 and 65535 are reserved by CCSDS for future use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Spi(u16);

impl Spi {
    /// Smallest valid SPI (355.0-B-2 Table 6-1).
    pub const MIN: u16 = 1;
    /// Largest valid SPI (355.0-B-2 Table 6-1).
    pub const MAX: u16 = 65_534;

    /// Constructs a validated SPI.
    ///
    /// # Errors
    /// `ReservedValue` if `value` is `0` or `65535` (355.0-B-2 §4.1.1.2.3).
    pub const fn new(value: u16) -> Result<Self> {
        if value == 0 || value == u16::MAX {
            return Err(Error::new(ErrorKind::ReservedValue).with_field(field::SECURITY_HEADER_LEN));
        }
        Ok(Self(value))
    }

    /// The raw 16-bit SPI value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }

    /// Encodes the SPI as 2 big-endian octets (355.0-B-2 §4.1.1.2.1).
    #[must_use]
    pub const fn to_be_bytes(self) -> [u8; 2] {
        self.0.to_be_bytes()
    }
}

/// Opaque cryptographic key handle (OI-84).
///
/// Keeps raw key material out of `aldis-sdls` engine state so software key stores and hardware
/// security modules (HSMs) both fit behind [`crate::AeadProvider`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId(u32);

impl KeyId {
    /// Wraps a caller-assigned key identifier.
    #[must_use]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// The underlying identifier.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Transmitted Initialization Vector length in octets, 0..=32 (355.0-B-2 §4.1.1.3, Table 6-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct IvLen(u8);

impl IvLen {
    /// Absent Initialization Vector field (0 octets, 355.0-B-2 §4.1.1.3.4).
    pub const NONE: Self = Self(0);
    /// AES-GCM baseline IV length: 12 octets / 96 bits (355.0-B-2 Annex E1.1, E3.1, E4.1).
    pub const BASELINE_GCM: Self = Self(12);
    /// Maximum transmitted IV length (355.0-B-2 Table 6-1).
    pub const MAX: u8 = 32;

    /// Constructs a validated IV length.
    ///
    /// # Errors
    /// `InvalidConfig` if `len > 32`.
    pub const fn new(len: u8) -> Result<Self> {
        if len > Self::MAX {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::SECURITY_HEADER_LEN));
        }
        Ok(Self(len))
    }

    /// Length in octets.
    #[must_use]
    pub const fn get(self) -> usize {
        u8_to_usize(self.0)
    }

    pub(crate) const fn as_u8(self) -> u8 {
        self.0
    }
}

/// Transmitted Sequence Number length in octets, 0 or 2..=8 (355.0-B-2 §4.1.1.4, Table 6-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct SnLen(u8);

impl SnLen {
    /// Absent Sequence Number field (0 octets; used when IV serves as counter, 355.0-B-2 §4.1.1.4 Note).
    pub const NONE: Self = Self(0);
    /// Maximum transmitted Sequence Number length (355.0-B-2 Table 6-1).
    pub const MAX: u8 = 8;

    /// Constructs a validated Sequence Number length (0, or 2..=8 per 355.0-B-2 Table 6-1).
    ///
    /// # Errors
    /// `InvalidConfig` if `len == 1` or `len > 8`.
    pub const fn new(len: u8) -> Result<Self> {
        if len == 1 || len > Self::MAX {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::SECURITY_HEADER_LEN));
        }
        Ok(Self(len))
    }

    /// Length in octets.
    #[must_use]
    pub const fn get(self) -> usize {
        u8_to_usize(self.0)
    }
}

/// Transmitted Pad Length field length in octets, 0..=2 (355.0-B-2 §4.1.1.5, Table 6-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct PlLen(u8);

impl PlLen {
    /// Absent Pad Length field (0 octets; AES-GCM requires no padding, 355.0-B-2 Annex E1.2).
    pub const NONE: Self = Self(0);
    /// Maximum Pad Length field width (355.0-B-2 Table 6-1).
    pub const MAX: u8 = 2;

    /// Constructs a validated Pad Length field width.
    ///
    /// # Errors
    /// `InvalidConfig` if `len > 2`.
    pub const fn new(len: u8) -> Result<Self> {
        if len > Self::MAX {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::SECURITY_HEADER_LEN));
        }
        Ok(Self(len))
    }

    /// Length in octets.
    #[must_use]
    pub const fn get(self) -> usize {
        u8_to_usize(self.0)
    }
}

/// Transmitted MAC (Security Trailer) length in octets, 0 or 8..=64 (355.0-B-2 §4.1.2, Table 6-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MacLen(u8);

impl MacLen {
    /// Absent MAC field (0 octets).
    pub const NONE: Self = Self(0);
    /// AES-GCM baseline MAC length: 16 octets / 128 bits (355.0-B-2 Annex E1.1, E3.1, E4.1).
    pub const BASELINE_GCM: Self = Self(16);
    /// Minimum non-zero MAC length (355.0-B-2 Table 6-1).
    pub const MIN_NONZERO: u8 = 8;
    /// Maximum MAC length (355.0-B-2 Table 6-1).
    pub const MAX: u8 = 64;

    /// Constructs a validated MAC length (0, or 8..=64 per 355.0-B-2 Table 6-1).
    ///
    /// # Errors
    /// `InvalidConfig` if `len` is in `1..=7` or `> 64`.
    pub const fn new(len: u8) -> Result<Self> {
        if (len > 0 && len < Self::MIN_NONZERO) || len > Self::MAX {
            return Err(
                Error::new(ErrorKind::InvalidConfig).with_field(field::SECURITY_TRAILER_LEN)
            );
        }
        Ok(Self(len))
    }

    /// Length in octets.
    #[must_use]
    pub const fn get(self) -> usize {
        u8_to_usize(self.0)
    }
}

/// Anti-replay sequence number window width, strictly positive (355.0-B-2 Table 6-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AntiReplayWindow(u64);

impl AntiReplayWindow {
    /// Constructs a validated anti-replay window width (`> 0`).
    ///
    /// # Errors
    /// `InvalidConfig` if `width == 0`.
    pub const fn new(width: u64) -> Result<Self> {
        if width == 0 {
            return Err(Error::new(ErrorKind::InvalidConfig));
        }
        Ok(Self(width))
    }

    /// The window width.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Security Association service type (355.0-B-2 §4.2.2.4, OI-85).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceType {
    /// Authenticated Encryption with Associated Data (CCSDS 352.0-B-2 / 355.0-B-2 Annex E).
    AuthenticatedEncryption,
    /// Clear mode (no-op cryptographic transform for development/testing, 355.0-B-2 §4.2.2.4 Note).
    ClearMode,
}

/// Verification status code returned by `ProcessSecurity` (355.0-B-2 §3.3.3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VerificationStatus {
    /// Verification succeeded (no failure).
    #[default]
    NoFailure,
    /// Received SPI is unknown or not associated with the frame's `GVCID`/`GMAP_ID` (§4.2.4.3).
    InvalidSpi,
    /// MAC / AEAD authentication tag verification failed (§4.2.4.4 e).
    MacFailure,
    /// Received sequence number is `<= managed_sn` or `> managed_sn + window` (§4.2.4.4 h–i).
    AntiReplayFailure,
    /// Invalid padding count in the Pad Length field (§4.2.4.4 k / §4.2.4.5 b).
    PaddingError,
}

/// Anti-replay sequence number / counter-mode IV value (355.0-B-2 §4.1.1.3–4, §4.2.3.4, §4.2.4.4).
///
/// Stored as a `u128` so both 2–8 octet sequence numbers and 12-octet (96-bit) AES-GCM counter
/// IVs are represented without overflow or truncation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct SequenceNumber(u128);

impl SequenceNumber {
    /// Initial counter value 0.
    pub const ZERO: Self = Self(0);
    /// Maximum encoded counter width in octets (128 bits).
    pub const MAX_OCTETS: usize = 16;

    /// Wraps a raw counter value.
    #[must_use]
    pub const fn new(value: u128) -> Self {
        Self(value)
    }

    /// Returns the raw counter value.
    #[must_use]
    pub const fn get(self) -> u128 {
        self.0
    }

    /// Maximum unsigned integer representable in `octets` bytes (`1..=16`).
    const fn max_for_octets(octets: usize) -> Option<u128> {
        match octets {
            1 => Some(0xFF),
            2 => Some(0xFFFF),
            3 => Some(0xFF_FFFF),
            4 => Some(0xFFFF_FFFF),
            5 => Some(0xFF_FFFF_FFFF),
            6 => Some(0xFFFF_FFFF_FFFF),
            7 => Some(0xFF_FFFF_FFFF_FFFF),
            8 => Some(0xFFFF_FFFF_FFFF_FFFF),
            9 => Some(0xFF_FFFF_FFFF_FFFF_FFFF),
            10 => Some(0xFFFF_FFFF_FFFF_FFFF_FFFF),
            11 => Some(0xFF_FFFF_FFFF_FFFF_FFFF_FFFF),
            12 => Some(0xFFFF_FFFF_FFFF_FFFF_FFFF_FFFF),
            13 => Some(0xFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF),
            14 => Some(0xFFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF),
            15 => Some(0xFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF),
            16 => Some(u128::MAX),
            _ => None,
        }
    }

    /// Increments by one, refusing rollover past the width `octets` (355.0-B-2 §4.2.3.4 a).
    ///
    /// Refusing rollover prevents silent AES-GCM nonce reuse.
    ///
    /// # Errors
    /// `Security` if `octets` is outside `1..=16` or incrementing would exceed the field width.
    pub const fn checked_next(self, octets: usize) -> Result<Self> {
        let Some(limit) = Self::max_for_octets(octets) else {
            return Err(Error::new(ErrorKind::Security));
        };
        if self.0 >= limit {
            return Err(Error::new(ErrorKind::Security));
        }
        Ok(Self(self.0.saturating_add(1)))
    }

    /// Encodes the counter into `out` as big-endian octets (`1..=16` bytes).
    ///
    /// # Errors
    /// `Security` if `out.len()` is outside `1..=16` or the value does not fit in `out.len()`.
    pub fn encode_be(self, out: &mut [u8]) -> Result<()> {
        let Some(limit) = Self::max_for_octets(out.len()) else {
            return Err(Error::new(ErrorKind::Security));
        };
        if self.0 > limit {
            return Err(Error::new(ErrorKind::Security));
        }
        let full = self.0.to_be_bytes();
        // bound: `out.len() <= 16`.
        for (dst, &src) in out.iter_mut().rev().zip(full.iter().rev()) {
            *dst = src;
        }
        Ok(())
    }

    /// Decodes a big-endian counter from `bytes` (`1..=16` bytes).
    ///
    /// # Errors
    /// `Security` if `bytes.len()` is outside `1..=16`.
    pub fn decode_be(bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() || bytes.len() > Self::MAX_OCTETS {
            return Err(Error::new(ErrorKind::Security));
        }
        let mut full = [0_u8; Self::MAX_OCTETS];
        // bound: `bytes.len() <= Self::MAX_OCTETS`.
        for (dst, &src) in full.iter_mut().rev().zip(bytes.iter().rev()) {
            *dst = src;
        }
        Ok(Self(u128::from_be_bytes(full)))
    }

    /// Checks whether `received` falls within `(self, self + window]` (355.0-B-2 §4.2.4.4 g–i).
    #[must_use]
    pub const fn accepts(self, received: Self, window: AntiReplayWindow) -> bool {
        let Some(diff) = received.0.checked_sub(self.0) else {
            return false;
        };
        diff > 0 && diff <= u64_to_u128(window.get())
    }
}
