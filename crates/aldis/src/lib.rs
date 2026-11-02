//! Facade crate re-exporting the Aldis crates behind feature flags.
//!
//! Specification: `docs/01-architecture.md` §2 and `docs/profiles/rust.md` §2. No features are
//! enabled by default, and the facade builds with none for a `no_std` target (GEN-005). Each
//! crate feature re-exports one crate as a module of the same name; a mission enables only the
//! standards it uses, so it compiles and qualifies only those crates (GEN-002, GEN-003).
//!
//! # Features
//!
//! | Feature | Re-exports | Standard | Requirements |
//! |---|---|---|---|
//! | `core` | `aldis::core` (`aldis-core`): errors, bit fields, identifiers, CRC, counters, security hooks | Common link features, `docs/standards/common-link-features.md` | CMN-001…CMN-041, and the GEN and API requirements `aldis-core` implements |
//! | `time` | `aldis::time` (`aldis-time`): CUC, CDS, CCS, and ASCII time codes, conversions | 301.0 §3 | TIME-001…TIME-034 |
//! | `spp` | `aldis::spp` (`aldis-spp`): Space Packet codec; a secondary header is opaque octets | 133.0 §3.4, §4.1–4.3, §5.2 | SPP-001…SPP-007, SPP-009…SPP-013 |
//! | `spp-time` | `spp` and `time`, and turns on the `time` feature of `aldis-spp`: the typed time secondary header, `aldis::spp::TimeHeaderLayout` | 133.0 §4.1.4.2.2 | SPP-008 |
//! | `epp` | `aldis::epp` (`aldis-epp`): Encapsulation Packet codec | 133.1 §4.1–4.3, §5 | EPP-001…EPP-012 |
//! | `tm` | `aldis::tm` (`aldis-tm`): TM Transfer Frames | 132.0 | TM-001…TM-011 |
//! | `tc` | `aldis::tc` (`aldis-tc`): TC Transfer Frames, CLCW | 232.0 | TC-001…TC-031 |
//! | `aos` | `aldis::aos` (`aldis-aos`): AOS Transfer Frames | 732.0 | AOS-001…AOS-020 |
//! | `uslp` | `aldis::uslp` (`aldis-uslp`): USLP Transfer Frames, truncated frames, delimiting, validation | 732.1 | USLP-001…USLP-017 |
//! | `link` | `aldis::link` (`aldis-link`): sending and receiving engines | `docs/04-engines.md` | LNK-001…LNK-079 |
//! | `cop1` | `aldis::cop1` (`aldis-cop1`): FOP-1 and FARM-1 | 232.1 | COP-001…COP-041 |
//! | `framework` | `aldis::framework` (`aldis-framework`): transports, clocks, stacks | `docs/05-framework.md` | FW-001…FW-021 |
//! | `alloc`, `std` | The same feature of every enabled crate; strictly additive | — | GEN-006 |
//! | `full` | Every crate above, `spp-time`, and the cross-crate features of `aldis-link` and `aldis-cop1` | — | — |
//!
//! `spp` alone keeps `aldis-time` out of the build; `spp-time` brings it in only for the time
//! secondary header (GEN-002). The link, COP-1, and framework crates are skeletons until their
//! milestones (`docs/08-roadmap.md`).
#![no_std]

/// AOS Space Data Link Protocol frames (732.0; AOS-001…AOS-020). Feature `aos`.
#[cfg(feature = "aos")]
pub use aldis_aos as aos;
/// COP-1: FOP-1 and FARM-1 (232.1; COP-001…COP-041). Feature `cop1`.
#[cfg(feature = "cop1")]
pub use aldis_cop1 as cop1;
/// Foundation types and shared algorithms (common link features; CMN-001…CMN-041). Feature
/// `core`.
#[cfg(feature = "core")]
pub use aldis_core as core;
/// Encapsulation Packet Protocol (133.1 §4.1–4.3, §5; EPP-001…EPP-012). Feature `epp`.
#[cfg(feature = "epp")]
pub use aldis_epp as epp;
/// Transports, clocks, and protocol stacks (`docs/05-framework.md`; FW-001…FW-021). Feature
/// `framework`.
#[cfg(feature = "framework")]
pub use aldis_framework as framework;
/// Sending and receiving engines (`docs/04-engines.md`; LNK-001…LNK-079). Feature `link`.
#[cfg(feature = "link")]
pub use aldis_link as link;
/// Space Packet Protocol (133.0 §3.4, §4.1–4.3, §5.2; SPP-001…SPP-013). Feature `spp`; the
/// time secondary header (133.0 §4.1.4.2.2, SPP-008) also needs feature `spp-time`.
#[cfg(feature = "spp")]
pub use aldis_spp as spp;
/// TC Space Data Link Protocol frames and the CLCW (232.0; TC-001…TC-031). Feature `tc`.
#[cfg(feature = "tc")]
pub use aldis_tc as tc;
/// Time Code Formats (301.0 §3; TIME-001…TIME-034). Feature `time`.
#[cfg(feature = "time")]
pub use aldis_time as time;
/// TM Space Data Link Protocol frames (132.0; TM-001…TM-011). Feature `tm`.
#[cfg(feature = "tm")]
pub use aldis_tm as tm;
/// Unified Space Data Link Protocol frames (732.1; USLP-001…USLP-017). Feature `uslp`.
#[cfg(feature = "uslp")]
pub use aldis_uslp as uslp;

/// Compiles and runs the example in `README.md` as a doctest; not part of the API. The example
/// uses the modules `core` and `spp`.
#[cfg(all(doctest, feature = "core", feature = "spp"))]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
