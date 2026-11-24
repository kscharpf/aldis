# `aldis-sdls-crypto`

Layer 3 cryptographic provider for [`aldis-sdls`](../aldis-sdls/README.md) implementing the CCSDS 352.0-B-2 / 355.0-B-2 Annex E **AES-256-GCM** baseline (`no_std`, zero heap allocation, `#![forbid(unsafe_code)]`).

Per `GEN-004` and `OI-84`, third-party cryptographic dependencies (`aes-gcm`, `zeroize`) live outside Layers 0–2 and are enabled only behind the non-default `aes-gcm` feature:

```toml
[dependencies]
aldis-sdls = { path = "crates/aldis-sdls" }
aldis-sdls-crypto = { path = "crates/aldis-sdls-crypto", features = ["aes-gcm"] }
```
