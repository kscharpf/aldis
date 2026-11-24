//! Fixed-capacity AES-256-GCM key store implementing [`aldis_sdls::AeadProvider`]
//! (CCSDS 352.0-B-2 §4.3.2, 355.0-B-2 Annex E1/E3/E4; OI-84, GEN-010, GEN-020).

use core::fmt;

use aes_gcm::aead::AeadInPlace;
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce, Tag};
use aldis_core::error::{Error, ErrorKind, Result};
use aldis_sdls::{AeadProvider, FreshNonce, KeyId};
use zeroize::Zeroize;

/// AES-256 key length in octets (256 bits; CCSDS 352.0-B-2 §4.3.2).
pub const AES_256_KEY_LEN: usize = 32;

/// AES-GCM baseline IV / nonce length in octets (96 bits; 355.0-B-2 Annex E1.1, E3.1, E4.1).
pub const AES_GCM_NONCE_LEN: usize = 12;

/// AES-GCM baseline authentication tag / MAC length in octets (128 bits; 355.0-B-2 Annex E).
pub const AES_GCM_TAG_LEN: usize = 16;

/// Owned 256-bit AES key zeroized automatically on drop (CCSDS 352.0-B-2 §4.3.2; OI-84).
///
/// Intentionally neither `Copy` nor `Clone` to avoid accidental key duplication on the stack,
/// and its `Debug` output is redacted so raw key material cannot leak into logs.
pub struct Aes256Key([u8; AES_256_KEY_LEN]);

impl Aes256Key {
    /// Wraps a 256-bit (32-octet) AES key.
    #[must_use]
    pub const fn new(bytes: [u8; AES_256_KEY_LEN]) -> Self {
        Self(bytes)
    }

    const fn as_bytes(&self) -> &[u8; AES_256_KEY_LEN] {
        &self.0
    }
}

impl Drop for Aes256Key {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for Aes256Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Aes256Key([REDACTED])")
    }
}

struct KeySlot {
    id: KeyId,
    key: Aes256Key,
}

/// Fixed-capacity AES-256-GCM key store implementing [`AeadProvider`] (CCSDS 352.0-B-2,
/// 355.0-B-2 Annex E; OI-84, GEN-010).
///
/// Holds up to `K` 256-bit keys with zero heap allocation and zeroizes every key on removal or
/// drop.
pub struct Aes256GcmStore<const K: usize> {
    slots: [Option<KeySlot>; K],
}

impl<const K: usize> Aes256GcmStore<K> {
    /// Creates an empty key store with capacity `K`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [const { None }; K],
        }
    }

    /// Inserts `key` under opaque handle `id` (OI-84).
    ///
    /// # Errors
    /// `InvalidConfig` if `id` is already present; `CapacityExceeded` if all `K` slots are full.
    pub fn insert(&mut self, id: KeyId, key: Aes256Key) -> Result<()> {
        insert_slot(&mut self.slots, id, key)
    }

    /// Removes and zeroizes the key stored under `id`, returning `true` if one was present.
    pub fn remove(&mut self, id: KeyId) -> bool {
        remove_slot(&mut self.slots, id)
    }

    /// Returns `true` if a key is stored under `id`.
    #[must_use]
    pub fn contains(&self, id: KeyId) -> bool {
        find_key(&self.slots, id).is_some()
    }
}

impl<const K: usize> Default for Aes256GcmStore<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const K: usize> fmt::Debug for Aes256GcmStore<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Aes256GcmStore")
            .field("capacity", &K)
            .finish_non_exhaustive()
    }
}

impl<const K: usize> AeadProvider for Aes256GcmStore<K> {
    fn seal_in_place(
        &self,
        key: KeyId,
        nonce: FreshNonce,
        aad: &[u8],
        data: &mut [u8],
        tag_out: &mut [u8],
    ) -> Result<()> {
        seal_in_slots(&self.slots, key, nonce.as_bytes(), aad, data, tag_out)
    }

    fn open_in_place(
        &self,
        key: KeyId,
        nonce: &[u8],
        aad: &[u8],
        data: &mut [u8],
        tag: &[u8],
    ) -> Result<()> {
        open_in_slots(&self.slots, key, nonce, aad, data, tag)
    }
}

fn insert_slot(slots: &mut [Option<KeySlot>], id: KeyId, key: Aes256Key) -> Result<()> {
    // bound: `slots.len() == K`.
    for entry in slots.iter().flatten() {
        if entry.id == id {
            return Err(Error::new(ErrorKind::InvalidConfig));
        }
    }
    // bound: `slots.len() == K`.
    for slot in slots.iter_mut() {
        if slot.is_none() {
            *slot = Some(KeySlot { id, key });
            return Ok(());
        }
    }
    Err(Error::new(ErrorKind::CapacityExceeded))
}

fn remove_slot(slots: &mut [Option<KeySlot>], id: KeyId) -> bool {
    // bound: `slots.len() == K`.
    for slot in slots.iter_mut() {
        if slot.as_ref().is_some_and(|entry| entry.id == id) {
            *slot = None;
            return true;
        }
    }
    false
}

fn find_key(slots: &[Option<KeySlot>], id: KeyId) -> Option<&[u8; AES_256_KEY_LEN]> {
    // bound: `slots.len() == K`.
    for entry in slots.iter().flatten() {
        if entry.id == id {
            return Some(entry.key.as_bytes());
        }
    }
    None
}

fn seal_in_slots(
    slots: &[Option<KeySlot>],
    key: KeyId,
    nonce: &[u8],
    aad: &[u8],
    data: &mut [u8],
    tag_out: &mut [u8],
) -> Result<()> {
    let Some(key_bytes) = find_key(slots, key) else {
        return Err(Error::new(ErrorKind::Security));
    };
    let Ok(nonce_arr): core::result::Result<[u8; AES_GCM_NONCE_LEN], _> = nonce.try_into() else {
        return Err(Error::new(ErrorKind::Security));
    };
    if tag_out.len() != AES_GCM_TAG_LEN {
        return Err(Error::new(ErrorKind::Security));
    }
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key_bytes));
    let nonce_ga = Nonce::from_slice(&nonce_arr);
    let Ok(tag) = cipher.encrypt_in_place_detached(nonce_ga, aad, data) else {
        return Err(Error::new(ErrorKind::Security));
    };
    for (dst, &src) in tag_out.iter_mut().zip(tag.as_slice().iter()) {
        *dst = src;
    }
    Ok(())
}

fn open_in_slots(
    slots: &[Option<KeySlot>],
    key: KeyId,
    nonce: &[u8],
    aad: &[u8],
    data: &mut [u8],
    tag: &[u8],
) -> Result<()> {
    let Some(key_bytes) = find_key(slots, key) else {
        return Err(Error::new(ErrorKind::Security));
    };
    let Ok(nonce_arr): core::result::Result<[u8; AES_GCM_NONCE_LEN], _> = nonce.try_into() else {
        return Err(Error::new(ErrorKind::Security));
    };
    let Ok(tag_arr): core::result::Result<[u8; AES_GCM_TAG_LEN], _> = tag.try_into() else {
        return Err(Error::new(ErrorKind::Security));
    };
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key_bytes));
    let nonce_ga = Nonce::from_slice(&nonce_arr);
    let tag_ga = Tag::from_slice(&tag_arr);
    cipher
        .decrypt_in_place_detached(nonce_ga, aad, data, tag_ga)
        .map_err(|_| Error::new(ErrorKind::Security))
}
