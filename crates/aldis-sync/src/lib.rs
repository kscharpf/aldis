//! TM synchronization of CCSDS 131.0-B-6 §9 and §10: the Attached Sync Marker, the Channel
//! Access Data Unit, the pseudo-randomizer, and the ASM search engine.
//!
//! Layer 1 of Aldis. Specification: `docs/standards/tm-sync.md` (SYNC-001…SYNC-020).
//!
//! | Module | Contents | Requirements |
//! |---|---|---|
//! | [`config`] | `SyncProtocol`, `AsmErrors`, `SyncConfig` | SYNC-002, SYNC-016 |
//! | [`asm`] | The ASM and the CADU attach function | SYNC-001, SYNC-003, SYNC-006 |
//! | [`randomizer`] | The two pseudo-random sequences and the randomizer | SYNC-004, SYNC-005, SYNC-020 |
//! | [`search`] | The `AsmSearch` engine | SYNC-007…SYNC-015, SYNC-017, SYNC-019 |
//!
//! The sending end randomizes the frame and then attaches the ASM, so that the marker is never
//! randomized; the receiving end finds the ASM with the engine and then derandomizes the unit it
//! returns (SYNC-006). The codec does not combine the two. The engine only searches (SYNC-019):
//! it holds no lock and does not check the spacing of the units.
//!
//! **Status:** implemented (M5 WP-3); the C ABI is `aldis_sync_*` of `aldis-ffi` (M5 WP-4).
#![no_std]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

pub mod asm;
pub mod config;
pub mod randomizer;
pub mod search;

pub use asm::{ASM, ASM_LEN, attach_asm};
pub use config::{AsmErrors, MAX_ASM_ERRORS, SyncConfig, SyncProtocol};
pub use randomizer::{RandomizerSequence, randomize};
pub use search::{
    AsmSearch, AsmSearchBuffers, AsmSearchHead, SearchAction, SearchActions, SearchInput,
    SearchState, SearchStatus, max_actions, max_out_len,
};

/// Copies `src` over the start of `dst`, as far as both reach. The callers pass slices of the
/// same length; the loop leaves no panic path, unlike `copy_from_slice` (GEN-020).
pub(crate) fn copy_octets(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.iter_mut().zip(src) {
        *d = *s;
    }
}

/// Compiles and runs the examples in `README.md` as doctests; not part of the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
