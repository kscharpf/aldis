//! Test-vector model, loader, and runner shared by all Aldis test suites.
//!
//! Tooling crate (std, dev-only): crates use it only as a dev-dependency (GEN-001), and it
//! is never part of a flight build. Specification: `docs/07-test-vectors.md`. With the
//! `strategies` feature it also provides the shared property-test strategies of VER-054
//! ([`strategies`]).
//!
//! ```no_run
//! use aldis_testkit::{Outcome, run_codec_file};
//! run_codec_file("core/crc16.json", |case| match case.operation.as_str() {
//!     // "crc16" => { … compute and return Outcome::Ok(values) … }
//!     _ => Outcome::Unsupported,
//! })
//! .assert_passed();
//! ```

pub mod hex;
pub mod model;
pub mod oracle;
pub mod runner;
#[cfg(feature = "strategies")]
pub mod strategies;

pub use model::{
    CodecCase, CodecFile, Expect, FieldCatalogue, FieldEntry, FieldKind, Mode, Provenance,
    ScenarioFile, Step, Superseded, VectorFile,
};
pub use runner::{Failure, Outcome, Report, compare, run_codec, run_codec_file};

/// Re-export so schema generation uses the same `schemars` as the model's derives.
#[cfg(feature = "schema")]
pub use schemars;

use std::path::PathBuf;

/// The repository's `test-vectors/` directory.
///
/// # Panics
/// If no ancestor of this crate's manifest directory contains `test-vectors/`.
#[must_use]
pub fn vectors_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .map(|dir| dir.join("test-vectors"))
        .find(|candidate| candidate.is_dir())
        .unwrap_or_else(|| panic!("no test-vectors/ above {}", manifest.display()))
}
