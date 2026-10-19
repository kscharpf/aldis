//! Runs the cross-checks and prints every agreement count, skip, and disagreement (VER-040).
//!
//! Exits with failure on any disagreement. Run: `cargo run --manifest-path crosscheck/Cargo.toml`.

use std::process::ExitCode;

use aldis_crosscheck::{Check, FileReport, check_file, spp};

/// The vector files each implementation checks.
const CHECKED: &[(&str, Check)] = &[
    ("spp/decode_valid.json", spp::check),
    ("spp/decode_invalid.json", spp::check),
    ("spp/stream.json", spp::check),
    ("spp/encode.json", spp::check),
    ("spp/idle.json", spp::check),
    ("spp/octet_string.json", spp::check),
    ("spp/segment.json", spp::check),
];

/// The vector files no implementation checks yet, and why (cross-checks.md §2, §4).
const NOT_CHECKED: &[(&str, &str)] = &[
    ("spp/reassemble.json", "spacepackets has no reassembler"),
    (
        "spp/sequence_tracker.json",
        "spacepackets has no sequence tracker",
    ),
    (
        "spp/split.json",
        "spacepackets has no secondary header layouts",
    ),
    (
        "spp/secondary_header_time.json",
        "time code checks not yet written",
    ),
    ("time/cuc.json", "time code checks not yet written"),
    ("time/cds.json", "time code checks not yet written"),
    ("time/ascii.json", "time code checks not yet written"),
    (
        "time/pfield.json",
        "no independent implementation (cross-checks.md §4)",
    ),
    (
        "time/ccs.json",
        "no independent implementation (cross-checks.md §4)",
    ),
    (
        "time/convert.json",
        "no independent implementation (cross-checks.md §4)",
    ),
    (
        "epp/*",
        "no independent implementation (cross-checks.md §4)",
    ),
];

// req: VER-040
fn main() -> ExitCode {
    let reports: Vec<FileReport> = CHECKED
        .iter()
        .map(|(file, check)| check_file(file, *check))
        .collect();
    println!("Cross-check against spacepackets 0.18.0 (docs/verification/cross-checks.md)\n");
    for r in &reports {
        println!(
            "{:34} {:3} agree  {:3} skipped  {:3} disagree{}",
            r.file,
            r.agreed,
            r.skipped.len(),
            r.disagreements.len(),
            if r.superseded > 0 {
                format!("  ({} superseded)", r.superseded)
            } else {
                String::new()
            }
        );
    }
    for (file, reason) in NOT_CHECKED {
        println!("{file:34} not checked: {reason}");
    }
    println!("\nSkipped cases:");
    for r in &reports {
        for (id, reason) in &r.skipped {
            println!("  {id}: {reason}");
        }
    }
    let disagreements: usize = reports.iter().map(|r| r.disagreements.len()).sum();
    if disagreements == 0 {
        println!("\nNo disagreements.");
        return ExitCode::SUCCESS;
    }
    println!("\nDisagreements:");
    for r in &reports {
        for (id, diff) in &r.disagreements {
            println!("  {id}: {diff}");
        }
    }
    ExitCode::FAILURE
}
