//! Prints the constants of `aldis_ffi::layout`, one `name literal value exact|min` line each, for
//! `cargo xtask header` (CABI-030, CABI-031; M6 plan WP-3d).

use aldis_ffi::layout::{Fit, PROTOCOL, WORDS_ROOMY, WORDS_SIZED};

#[expect(clippy::print_stdout, reason = "the output is the example's result")]
fn main() {
    for (name, literal, value, fit) in WORDS_SIZED.iter().chain(WORDS_ROOMY).chain(PROTOCOL) {
        let fit = match fit {
            Fit::Exact => "exact",
            Fit::AtLeast => "min",
        };
        println!("{name} {literal} {value} {fit}");
    }
}
