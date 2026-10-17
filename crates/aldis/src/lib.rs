//! Facade crate re-exporting the Aldis crates behind feature flags.
//!
//! Specification: `docs/01-architecture.md` §2 and `docs/profiles/rust.md` §2. No features are
//! enabled by default; enable individual crate features (e.g. `core`, `spp`) or `full` for
//! everything.
#![no_std]

#[cfg(feature = "aos")]
pub use aldis_aos as aos;
#[cfg(feature = "cop1")]
pub use aldis_cop1 as cop1;
#[cfg(feature = "core")]
pub use aldis_core as core;
#[cfg(feature = "epp")]
pub use aldis_epp as epp;
#[cfg(feature = "framework")]
pub use aldis_framework as framework;
#[cfg(feature = "link")]
pub use aldis_link as link;
#[cfg(feature = "spp")]
pub use aldis_spp as spp;
#[cfg(feature = "tc")]
pub use aldis_tc as tc;
#[cfg(feature = "time")]
pub use aldis_time as time;
#[cfg(feature = "tm")]
pub use aldis_tm as tm;
#[cfg(feature = "uslp")]
pub use aldis_uslp as uslp;
