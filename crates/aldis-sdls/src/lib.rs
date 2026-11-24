//! CCSDS 355.0-B-2 Space Data Link Security (SDLS) Protocol (`no_std`, pluggable AEAD).
//!
//! Layer 1 crate: depends only on `aldis-core` (GEN-001, GEN-002), allocates no heap memory
//! (GEN-010), is panic-free (GEN-020), and uses no `unsafe` (GEN-070).
//!
//! # Scope (OI-84, OI-85)
//!
//! - Security Header and Security Trailer codecs (355.0-B-2 §4.1.1, §4.1.2).
//! - Authenticated Encryption (AEAD, CCSDS 352.0-B-2 / 355.0-B-2 Annex E baseline) and
//!   Clear Mode SAs (§4.2.2.4).
//! - Normative per-protocol AAD bit-masking for TM, TC, AOS, and USLP (§4.2.2.6.2).
//! - `ApplySecurity` (§4.2.3) and `ProcessSecurity` with anti-replay window checking (§4.2.4),
//!   exposed via [`SdlsEngine`] implementing [`aldis_core::security::SecurityHook`].
//! - Pluggable cryptography via [`AeadProvider`], keeping third-party crypto crates outside
//!   Layers 0–2 (GEN-001).
#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod aad;
pub mod engine;
pub mod header;
pub mod provider;
pub mod types;

pub use aad::{MAX_AAD_LEN, build_aad};
pub use engine::{SaConfig, SdlsEngine};
pub use header::{HeaderFormat, SecurityHeaderView, encode_header};
pub use provider::{AeadProvider, FreshNonce, NoOpProvider};
pub use types::{
    AntiReplayWindow, IvLen, KeyId, MacLen, PlLen, SequenceNumber, ServiceType, SnLen, Spi,
    VerificationStatus,
};
