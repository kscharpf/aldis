//! Normative per-protocol AAD bit-mask construction (355.0-B-2 §4.2.2.6.2, §4.2.3.2.2.3).
//!
//! For AEAD algorithms (AES-GCM), the Additional Authenticated Data (AAD) is the bitwise-AND of
//! the frame prefix `frame[0..fields.data().start]` (from the first octet of the Transfer Frame
//! Primary Header through the end of the Security Header) with the SA's authentication bit mask.
//!
//! Default bit masks follow 355.0-B-2 §4.2.2.6.2 (a–j) and the probed header layouts in
//! `docs/reference/header-layouts.md`:
//!
//! - **TM (132.0 §4.1.2):** VCID bits 12–14 (octet 1 `& 0x0E`); MCFC (octet 2) = `0x00`.
//! - **TC (232.0 §4.1.2):** VCID bits 16–21 (octet 2 `& 0xFC`); Segment Header (octet 5 when
//!   `fields.header().start == 6`) = `0xFF`.
//! - **AOS (732.0 §4.1.2):** VCID bits 10–15 (octet 1 `& 0x3F`); FHEC and Insert Zone = `0x00`.
//! - **USLP (732.1 §4.1.2):** VCID bits 21–26 and MAP ID bits 27–30 (octet 2 `& 0x07`, octet 3
//!   `& 0xFE`); Insert Zone = `0x00`.
//! - **Security Header (all four protocols, §4.2.2.6.2 h):** SPI, SN, and Pad Length = `0xFF`;
//!   Initialization Vector = `0x00`.

use core::ops::Range;

use aldis_core::error::{Error, ErrorKind, Result};
use aldis_core::field;
use aldis_core::security::{FrameCtx, Protocol, SecuredFields};

/// Maximum AAD length in octets (Transfer Frame Primary Header + optional FSH/Insert Zone +
/// Security Header, ≤ 256 octets).
pub const MAX_AAD_LEN: usize = 256;

/// Builds the masked Additional Authenticated Data (AAD) for `frame` into `out` and returns its
/// length (355.0-B-2 §4.2.2.6.2, §4.2.3.2.2.3 a.2).
///
/// `iv_range_in_header` is the byte range of the IV within the Security Header (e.g. `2..14` for
/// the 96-bit AES-GCM baseline); those octets are masked to `0x00` per §4.2.2.6.2 h.
///
/// # Errors
/// `BufferTooSmall` if the prefix exceeds [`MAX_AAD_LEN`]; `InvalidField` if `fields` is
/// inconsistent with the protocol's minimum primary header length or `frame.len()`.
pub fn build_aad(
    ctx: &FrameCtx,
    frame: &[u8],
    fields: &SecuredFields,
    iv_range_in_header: Range<usize>,
    out: &mut [u8; MAX_AAD_LEN],
) -> Result<usize> {
    let hdr_start = fields.header().start;
    let aad_len = fields.data().start;
    if aad_len > MAX_AAD_LEN {
        return Err(Error::new(ErrorKind::BufferTooSmall));
    }
    let Some(prefix) = frame.get(..aad_len) else {
        return Err(Error::new(ErrorKind::InvalidField).with_field(field::SECURED_FIELDS));
    };
    let dst = out.get_mut(..aad_len).unwrap_or_default();
    // Default every octet before the Security Header to 0x00 (§4.2.2.6.2 j).
    dst.fill(0);

    // Apply the normative Transfer Frame header mask (§4.2.2.6.2 b–g).
    match ctx.protocol() {
        Protocol::Tm => {
            if hdr_start < 6 {
                return Err(Error::new(ErrorKind::InvalidField).with_field(field::SECURED_FIELDS));
            }
            // TM VCID: bits 12–14 -> octet 1 mask 0x0E (132.0 §4.1.2.3).
            copy_masked(prefix, dst, 1, 0x0E);
        }
        Protocol::Tc => {
            if hdr_start < 5 {
                return Err(Error::new(ErrorKind::InvalidField).with_field(field::SECURED_FIELDS));
            }
            // TC VCID: bits 16–21 -> octet 2 mask 0xFC (232.0 §4.1.2.6).
            copy_masked(prefix, dst, 2, 0xFC);
            // TC Segment Header: 1 octet at index 5 when present (§4.2.2.6.2 f).
            if hdr_start == 6 {
                copy_masked(prefix, dst, 5, 0xFF);
            }
        }
        Protocol::Aos => {
            if hdr_start < 6 {
                return Err(Error::new(ErrorKind::InvalidField).with_field(field::SECURED_FIELDS));
            }
            // AOS VCID: bits 10–15 -> octet 1 mask 0x3F (732.0 §4.1.2.3).
            copy_masked(prefix, dst, 1, 0x3F);
        }
        Protocol::Uslp => {
            if hdr_start < 4 {
                return Err(Error::new(ErrorKind::InvalidField).with_field(field::SECURED_FIELDS));
            }
            // USLP VCID (bits 21–26) + MAP ID (bits 27–30): octet 2 mask 0x07, octet 3 mask 0xFE.
            copy_masked(prefix, dst, 2, 0x07);
            copy_masked(prefix, dst, 3, 0xFE);
        }
        _ => return Err(Error::new(ErrorKind::InvalidField).with_field(field::SECURED_FIELDS)),
    }

    // Security Header: all ones except the IV field (§4.2.2.6.2 h).
    let sh_src = prefix.get(hdr_start..aad_len).unwrap_or_default();
    let sh_dst = dst.get_mut(hdr_start..aad_len).unwrap_or_default();
    for (d, &s) in sh_dst.iter_mut().zip(sh_src.iter()) {
        *d = s;
    }
    if let Some(iv_dst) = sh_dst.get_mut(iv_range_in_header) {
        iv_dst.fill(0);
    } else {
        return Err(Error::new(ErrorKind::InvalidField).with_field(field::SECURITY_HEADER_LEN));
    }

    Ok(aad_len)
}

fn copy_masked(src: &[u8], dst: &mut [u8], idx: usize, mask: u8) {
    // bound: takes at most 1 octet at `idx`.
    for (slot, &byte) in dst
        .iter_mut()
        .skip(idx)
        .take(1)
        .zip(src.iter().skip(idx).take(1))
    {
        *slot = byte & mask;
    }
}
