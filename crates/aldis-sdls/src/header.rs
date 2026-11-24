//! Security Header and Security Trailer codecs (355.0-B-2 §4.1.1, §4.1.2).

use core::ops::Range;

use aldis_core::error::{Error, ErrorKind, Result};
use aldis_core::field;
use aldis_core::security::SecurityLayout;

use crate::types::{IvLen, MacLen, PlLen, SnLen, Spi};

/// Fixed field lengths of a Security Header for an SA (355.0-B-2 §4.1.1.1.3–4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HeaderFormat {
    iv: IvLen,
    sn: SnLen,
    pl: PlLen,
}

impl HeaderFormat {
    /// AES-GCM baseline Security Header: 2-octet SPI + 12-octet IV, no SN or Pad Length
    /// (14 octets total; 355.0-B-2 Annex E1.2, E3.2, E4.2).
    pub const BASELINE_GCM: Self = Self {
        iv: IvLen::BASELINE_GCM,
        sn: SnLen::NONE,
        pl: PlLen::NONE,
    };

    /// Constructs a validated Security Header format (`2 + iv + sn + pl <= 44 <= 64`,
    /// 355.0-B-2 §4.1.1.1.4, Table 6-1; `API-071`).
    ///
    /// Infallible because `IvLen <= 32`, `SnLen <= 8`, and `PlLen <= 2` guarantee the total
    /// header length never exceeds 64 octets.
    #[must_use]
    pub const fn new(iv: IvLen, sn: SnLen, pl: PlLen) -> Self {
        Self { iv, sn, pl }
    }

    /// Initialization Vector field length in octets.
    #[must_use]
    pub const fn iv_len(self) -> IvLen {
        self.iv
    }

    /// Sequence Number field length in octets.
    #[must_use]
    pub const fn sn_len(self) -> SnLen {
        self.sn
    }

    /// Pad Length field length in octets.
    #[must_use]
    pub const fn pl_len(self) -> PlLen {
        self.pl
    }

    /// Total Security Header length in octets (`2 + iv + sn + pl`).
    #[must_use]
    pub const fn total_len(self) -> usize {
        2_usize
            .saturating_add(self.iv.get())
            .saturating_add(self.sn.get())
            .saturating_add(self.pl.get())
    }

    /// Byte range of the IV field within the Security Header (`2..2 + iv_len`).
    #[must_use]
    pub const fn iv_range(self) -> Range<usize> {
        2..2_usize.saturating_add(self.iv.get())
    }

    /// Byte range of the Sequence Number field within the Security Header.
    #[must_use]
    pub const fn sn_range(self) -> Range<usize> {
        let start = 2_usize.saturating_add(self.iv.get());
        start..start.saturating_add(self.sn.get())
    }

    /// Byte range of the Pad Length field within the Security Header.
    #[must_use]
    pub const fn pl_range(self) -> Range<usize> {
        let start = 2_usize
            .saturating_add(self.iv.get())
            .saturating_add(self.sn.get());
        start..start.saturating_add(self.pl.get())
    }

    /// Converts this header format and a MAC length into a `SecurityLayout` (`API-070`).
    ///
    /// # Errors
    /// `InvalidConfig` if the lengths exceed `SecurityLayout` bounds.
    pub const fn security_layout(self, mac_len: MacLen) -> Result<SecurityLayout> {
        SecurityLayout::new(self.total_len(), mac_len.get())
    }
}

/// Zero-copy view of a decoded Security Header (355.0-B-2 §4.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecurityHeaderView<'a> {
    spi: Spi,
    iv: &'a [u8],
    sn: &'a [u8],
    pad_len: &'a [u8],
}

impl<'a> SecurityHeaderView<'a> {
    /// Decodes a Security Header slice according to `format` (355.0-B-2 §4.1.1.1.3).
    ///
    /// # Errors
    /// `LengthMismatch` if `bytes.len() != format.total_len()`; `ReservedValue` if the SPI is
    /// `0` or `65535`.
    pub fn decode(bytes: &'a [u8], format: HeaderFormat) -> Result<Self> {
        if bytes.len() != format.total_len() {
            return Err(
                Error::new(ErrorKind::LengthMismatch).with_field(field::SECURITY_HEADER_LEN)
            );
        }
        let spi = decode_spi(bytes)?;
        let iv = bytes.get(format.iv_range()).unwrap_or_default();
        let sn = bytes.get(format.sn_range()).unwrap_or_default();
        let pad_len = bytes.get(format.pl_range()).unwrap_or_default();
        Ok(Self {
            spi,
            iv,
            sn,
            pad_len,
        })
    }

    /// Security Parameter Index (§4.1.1.2).
    #[must_use]
    pub const fn spi(&self) -> Spi {
        self.spi
    }

    /// Initialization Vector slice (§4.1.1.3).
    #[must_use]
    pub const fn iv(&self) -> &'a [u8] {
        self.iv
    }

    /// Sequence Number slice (§4.1.1.4).
    #[must_use]
    pub const fn sn(&self) -> &'a [u8] {
        self.sn
    }

    /// Pad Length slice (§4.1.1.5).
    #[must_use]
    pub const fn pad_len(&self) -> &'a [u8] {
        self.pad_len
    }
}

/// Decodes just the 16-bit SPI from the first 2 octets of a Security Header (355.0-B-2 §4.1.1.2).
///
/// # Errors
/// `LengthMismatch` if `header.len() < 2`; `ReservedValue` if SPI is `0` or `65535`.
pub fn decode_spi(header: &[u8]) -> Result<Spi> {
    let Some((&hi, &lo)) = header.first().zip(header.get(1)) else {
        return Err(Error::new(ErrorKind::LengthMismatch).with_field(field::SECURITY_HEADER_LEN));
    };
    Spi::new(u16::from_be_bytes([hi, lo]))
}

/// Encodes a Security Header into `out` (355.0-B-2 §4.1.1.1.3).
///
/// # Errors
/// `LengthMismatch` if `out.len() != format.total_len()` or any field slice length disagrees
/// with `format`.
pub fn encode_header(
    out: &mut [u8],
    format: HeaderFormat,
    spi: Spi,
    iv: &[u8],
    sn: &[u8],
    pad_len: &[u8],
) -> Result<()> {
    if out.len() != format.total_len()
        || iv.len() != format.iv_len().get()
        || sn.len() != format.sn_len().get()
        || pad_len.len() != format.pl_len().get()
    {
        return Err(Error::new(ErrorKind::LengthMismatch).with_field(field::SECURITY_HEADER_LEN));
    }
    let spi_bytes = spi.to_be_bytes();
    // bound: `out.len() == 2 + iv.len() + sn.len() + pad_len.len() <= 44`.
    for (dst, &src) in out.iter_mut().zip(
        spi_bytes
            .iter()
            .chain(iv.iter())
            .chain(sn.iter())
            .chain(pad_len.iter()),
    ) {
        *dst = src;
    }
    Ok(())
}
