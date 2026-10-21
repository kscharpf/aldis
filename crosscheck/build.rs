//! Compiles the cFE `MSG` sources for the cFE cross-check (VER-040; cross-checks.md §1).
//!
//! The sources are the unmodified files of NASA cFE v7.0.1 (commit
//! c5fb2b4d540bd55eb6c3707da7dd13eee679d4dd) vendored under `vendor/cfe/`, listed with their
//! SHA-256 in `vendor/cfe/SHA256SUMS`. `ALDIS_CFE_SRC` may name a cFE checkout to compile from
//! instead. The build fails when a file is missing: the check is never skipped for want of its
//! source. The headers cFE takes from OSAL, from the other cFE modules, and from its generated
//! configuration are the stubs of `cfe-stubs/`.

use std::path::{Path, PathBuf};

/// The cFE sources compiled: the primary header accessors, `CFE_MSG_Init` and the default header
/// without the CCSDS extended header, the version 1 message identifier that `CFE_MSG_Init`
/// calls, and the telemetry time accessors. The same selection cFE's `modules/msg/CMakeLists.txt`
/// makes without `MISSION_INCLUDE_CCSDSEXT_HEADER`, less the files the harness does not call.
const SOURCES: &[&str] = &[
    "modules/msg/fsw/src/cfe_msg_ccsdspri.c",
    "modules/msg/fsw/src/cfe_msg_init.c",
    "modules/msg/fsw/src/cfe_msg_initdefaulthdr_pri.c",
    "modules/msg/fsw/src/cfe_msg_msgid_v1.c",
    "modules/msg/fsw/src/cfe_msg_sechdr_time.c",
];

/// The cFE headers those sources include, by directory.
const HEADERS: &[&str] = &[
    "modules/core_api/fsw/inc/cfe_error.h",
    "modules/core_api/fsw/inc/cfe_msg.h",
    "modules/core_api/fsw/inc/cfe_msg_api_typedefs.h",
    "modules/msg/fsw/inc/ccsds_hdr.h",
    "modules/msg/fsw/src/cfe_msg_defaults.h",
    "modules/msg/fsw/src/cfe_msg_priv.h",
    "modules/msg/option_inc/default_cfe_msg_hdr_pri.h",
    "modules/msg/option_inc/default_cfe_msg_sechdr.h",
];

/// The cFE include directories, in the order cFE's build adds them.
const INCLUDE_DIRS: &[&str] = &[
    "modules/core_api/fsw/inc",
    "modules/msg/fsw/inc",
    "modules/msg/fsw/src",
    "modules/msg/option_inc",
];

fn main() {
    println!("cargo::rerun-if-env-changed=ALDIS_CFE_SRC");
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let cfe = std::env::var_os("ALDIS_CFE_SRC")
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| manifest.join("vendor/cfe"), PathBuf::from);
    let missing: Vec<&str> = SOURCES
        .iter()
        .chain(HEADERS)
        .copied()
        .filter(|file| !cfe.join(file).is_file())
        .collect();
    assert!(
        missing.is_empty(),
        "the cFE cross-check needs the cFE v7.0.1 sources in {} (vendor/cfe/, or ALDIS_CFE_SRC); \
         missing: {}",
        cfe.display(),
        missing.join(", ")
    );
    let stubs = manifest.join("cfe-stubs");
    let mut build = cc::Build::new();
    build
        .std("c11")
        .warnings(true)
        .extra_warnings(true)
        .warnings_into_errors(true)
        .include(&stubs);
    for dir in INCLUDE_DIRS {
        build.include(cfe.join(dir));
    }
    for file in SOURCES {
        build.file(cfe.join(file));
    }
    build.file(stubs.join("abi_check.c"));
    build.compile("aldis_cfe_msg");
    for file in SOURCES.iter().chain(HEADERS) {
        rerun_if_changed(&cfe.join(file));
    }
    rerun_if_changed(&stubs);
}

fn rerun_if_changed(path: &Path) {
    println!("cargo::rerun-if-changed={}", path.display());
}
