//! Selects the Cortex-M4 linker script for `refapp-qemu` (APP-102).

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let script = "link-cortex-m.x";
    println!("cargo:rerun-if-changed={script}");
    println!("cargo:rustc-link-arg=-T{}", manifest.join(script).display());
}
