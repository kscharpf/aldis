# `aldis-sdls`

CCSDS 355.0-B-2 Space Data Link Security (SDLS) Protocol for TM, TC, AOS, and USLP Transfer Frames.

- **Layer 1** (`no_std`, no heap, panic-free, `#![forbid(unsafe_code)]`, zero third-party runtime dependencies).
- Implements the 355.0-B-2 Security Header and Security Trailer codecs (§4.1), Security Association table and context binding (§4.2.2), per-protocol AAD bit-masking (§4.2.2.6.2), `ApplySecurity` (§4.2.3), and `ProcessSecurity` with anti-replay sequence number window checking (§4.2.4).
- Pluggable AEAD cryptography through [`AeadProvider`](src/provider.rs) so software providers (such as RustCrypto `aes-gcm`) and hardware security modules fit without adding runtime dependencies to Layers 0–2.
- `SdlsEngine` implements `aldis_core::security::SecurityHook`, plugging directly into the TM, TC, AOS, and USLP writers (`protect`) and `aldis_core::Plain::verify_with`.
