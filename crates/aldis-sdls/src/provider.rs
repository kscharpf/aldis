//! Pluggable AEAD cryptographic provider trait (OI-84, GEN-001).

use aldis_core::error::Result;

use crate::types::KeyId;

/// Pluggable Authenticated Encryption with Associated Data (AEAD) provider (OI-84).
///
/// Software providers (such as `RustCrypto` `aes-gcm` in `aldis-sdls-crypto`) and hardware
/// security modules implement this trait outside Layers 0–2, keeping `aldis-sdls` free of
/// third-party runtime dependencies (GEN-001).
///
/// # Provider obligations (OI-84)
/// - Constant-time tag comparison and no secret-dependent branching or table lookups.
/// - `open_in_place` MUST authenticate `tag` over `(aad, data)` before returning `Ok(())`, and
///   MUST NOT leave unauthenticated plaintext in `data` on error (355.0-B-2 §3.3.3.3 Note 2).
/// - Return `ErrorKind::Security` on any key lookup, nonce/tag length, or authentication failure;
///   never panic (GEN-020).
pub trait AeadProvider {
    /// Encrypts `data` in place and writes the authentication tag into `tag_out`.
    ///
    /// # Errors
    /// `Security` if `key` is unknown, `nonce`/`tag_out` lengths are unsupported, or encryption
    /// fails.
    fn seal_in_place(
        &self,
        key: KeyId,
        nonce: &[u8],
        aad: &[u8],
        data: &mut [u8],
        tag_out: &mut [u8],
    ) -> Result<()>;

    /// Verifies `tag` over `(aad, data)` and decrypts `data` in place.
    ///
    /// # Errors
    /// `Security` if `key` is unknown, `nonce`/`tag` lengths are unsupported, or tag verification
    /// fails.
    fn open_in_place(
        &self,
        key: KeyId,
        nonce: &[u8],
        aad: &[u8],
        data: &mut [u8],
        tag: &[u8],
    ) -> Result<()>;
}

/// No-op provider for Clear Mode SAs (355.0-B-2 §4.2.2.4 Note).
///
/// Leaves `data` unchanged and zeroes `tag_out` on seal; checks nothing on open. Intended only
/// for development testing with [`crate::ServiceType::ClearMode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NoOpProvider;

impl AeadProvider for NoOpProvider {
    fn seal_in_place(
        &self,
        _key: KeyId,
        _nonce: &[u8],
        _aad: &[u8],
        _data: &mut [u8],
        tag_out: &mut [u8],
    ) -> Result<()> {
        tag_out.fill(0);
        Ok(())
    }

    fn open_in_place(
        &self,
        _key: KeyId,
        _nonce: &[u8],
        _aad: &[u8],
        _data: &mut [u8],
        _tag: &[u8],
    ) -> Result<()> {
        Ok(())
    }
}
