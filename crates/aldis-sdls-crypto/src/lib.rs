//! Layer 3 cryptographic provider for `aldis-sdls` (CCSDS 352.0-B-2, 355.0-B-2 Annex E; OI-84).
//!
//! `#![no_std]`, `#![forbid(unsafe_code)]`, zero heap allocation (`GEN-010`), and panic-free
//! (`GEN-020`). Per `GEN-004`, third-party cryptographic dependencies (`aes-gcm`, `zeroize`) are
//! enabled only behind the non-default `aes-gcm` feature.
#![no_std]
#![forbid(unsafe_code)]

#[cfg(feature = "aes-gcm")]
mod aes_gcm_store;

#[cfg(feature = "aes-gcm")]
pub use aes_gcm_store::{
    AES_256_KEY_LEN, AES_GCM_NONCE_LEN, AES_GCM_TAG_LEN, Aes256GcmStore, Aes256Key,
};
