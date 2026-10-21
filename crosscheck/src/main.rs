//! Runs the cross-checks and prints every agreement count, skip, known discrepancy, and
//! disagreement (VER-040).
//!
//! Exits with failure on any disagreement; a known discrepancy (cross-checks.md §3) does not
//! fail the run. Run: `cargo run --manifest-path crosscheck/Cargo.toml`.

use std::process::ExitCode;

use aldis_crosscheck::{Check, FileReport, cfe, cfe_properties, check_file, spp, time};

/// The vector files `spacepackets` checks.
const CHECKED: &[(&str, Check)] = &[
    ("spp/decode_valid.json", spp::check),
    ("spp/decode_invalid.json", spp::check),
    ("spp/stream.json", spp::check),
    ("spp/encode.json", spp::check),
    ("spp/idle.json", spp::check),
    ("spp/octet_string.json", spp::check),
    ("spp/segment.json", spp::check),
    (
        "spp/secondary_header_time.json",
        time::check_secondary_header,
    ),
    ("time/pfield.json", time::check_pfield),
    ("time/cuc.json", time::check_cuc),
    ("time/cds.json", time::check_cds),
    ("time/ascii.json", time::check_ascii),
];

/// The Space Packet vector files whose packets cFE's `MSG` module reads and builds, against Aldis.
const PACKETS_CFE: &[(&str, Check)] = &[
    ("spp/decode_valid.json", cfe::check_packets),
    ("spp/decode_invalid.json", cfe::check_packets),
    ("spp/field_codes.json", cfe::check_packets),
    ("spp/stream.json", cfe::check_packets),
    ("spp/encode.json", cfe::check_packets),
    ("spp/idle.json", cfe::check_packets),
    ("spp/octet_string.json", cfe::check_packets),
    ("spp/segment.json", cfe::check_packets),
    ("spp/reassemble.json", cfe::check_packets),
    ("spp/sequence_tracker.json", cfe::check_packets),
    ("spp/split.json", cfe::check_packets),
    ("spp/secondary_header_time.json", cfe::check_packets),
];

/// The vector files whose expected values cFE's `MSG` module checks.
const CHECKED_CFE: &[(&str, Check)] = &[
    ("spp/decode_valid.json", cfe::check_spp),
    ("spp/decode_invalid.json", cfe::check_spp),
    ("spp/field_codes.json", cfe::check_spp),
    ("spp/stream.json", cfe::check_spp),
    ("spp/encode.json", cfe::check_spp),
    ("spp/idle.json", cfe::check_spp),
    ("spp/octet_string.json", cfe::check_spp),
    ("spp/segment.json", cfe::check_spp),
    (
        "spp/secondary_header_time.json",
        cfe::check_secondary_header,
    ),
    ("time/cuc.json", cfe::check_cuc),
];

/// The vector files whose expected values no implementation checks, and why (cross-checks.md
/// §2, §4). cFE reads the headers of the packets in the `spp/` files.
const NOT_CHECKED: &[(&str, &str)] = &[
    (
        "spp/reassemble.json",
        "no reassembler in spacepackets or cFE",
    ),
    (
        "spp/sequence_tracker.json",
        "no sequence tracker in spacepackets or cFE",
    ),
    (
        "spp/split.json",
        "no secondary header layouts in spacepackets or cFE",
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
    let mut disagreements = run(
        "Cross-check against spacepackets 0.18.0 (docs/verification/cross-checks.md)",
        CHECKED,
    );
    for (file, reason) in NOT_CHECKED {
        println!("{file:34} not checked: {reason}");
    }
    println!();
    disagreements += run(
        "Cross-check against NASA cFE MSG v7.0.1: the primary header of every well-formed \
         packet, read and built by cFE and by Aldis (docs/verification/cross-checks.md)",
        PACKETS_CFE,
    );
    disagreements += run(
        "Cross-check against NASA cFE MSG v7.0.1: the expected values cFE produces \
         (docs/verification/cross-checks.md)",
        CHECKED_CFE,
    );
    println!("Properties, cFE MSG v7.0.1 against Aldis (fixed seed):");
    for p in &cfe_properties::run() {
        println!(
            "  {:48} {:5} agree  {:3} disagree",
            p.name,
            p.agreed,
            p.disagreements.len()
        );
        for diff in &p.disagreements {
            println!("    {diff}");
        }
        disagreements += p.disagreements.len();
    }
    if disagreements == 0 {
        println!("\nNo disagreements.");
        ExitCode::SUCCESS
    } else {
        println!("\n{disagreements} disagreements.");
        ExitCode::FAILURE
    }
}

/// Checks `files`, prints the counts, skips, known discrepancies, and disagreements under
/// `title`, and returns the number of disagreements.
fn run(title: &str, files: &[(&'static str, Check)]) -> usize {
    let reports: Vec<FileReport> = files
        .iter()
        .map(|(file, check)| check_file(file, *check))
        .collect();
    println!("{title}\n");
    for r in &reports {
        println!(
            "{:34} {:3} agree  {:3} skipped  {:3} known  {:3} disagree{}",
            r.file,
            r.agreed,
            r.skipped.len(),
            r.known.len(),
            r.disagreements.len(),
            if r.superseded > 0 {
                format!("  ({} superseded)", r.superseded)
            } else {
                String::new()
            }
        );
    }
    println!("\nSkipped cases:");
    for r in &reports {
        for (id, reason) in &r.skipped {
            println!("  {id}: {reason}");
        }
    }
    println!("\nKnown discrepancies (cross-checks.md §3):");
    for r in &reports {
        for (id, entry) in &r.known {
            println!("  {id}: {entry}");
        }
    }
    let disagreements: usize = reports.iter().map(|r| r.disagreements.len()).sum();
    if disagreements > 0 {
        println!("\nDisagreements:");
        for r in &reports {
            for (id, diff) in &r.disagreements {
                println!("  {id}: {diff}");
            }
        }
    }
    println!();
    disagreements
}
