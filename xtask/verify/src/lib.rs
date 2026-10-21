//! `cargo xtask verify`: the verification run and the merge gate (docs/06-verification.md §7,
//! VER-056…VER-060).

mod verify;

pub use verify::{HELP, run};
use xtask_lib::repo;
