//! `xtask readme-diagram`: the sequence diagram of the README, generated from the golden trace
//! of the COP-1 run over a lossy link (APP-153).
//!
//! `refapp-harness sequence-diagram` decodes `refapp/golden/a3-uplink-loss.jsonl` with the
//! strict codecs and prints a Mermaid block. This module splices it between the marker
//! comments [`BEGIN`] and [`END`] of `README.md`. Without `--write` it fails when the committed
//! region differs, and `cargo xtask fix` rewrites it. Only this tool writes the region; the
//! check reads `README.md` and the trace, both inputs of the verification run's `refapp`
//! check.

use std::process::Command;

use crate::repo;

/// The file that holds the region.
const README: &str = "README.md";
/// The line that opens the generated region.
pub const BEGIN: &str =
    "<!-- BEGIN GENERATED: sequence-diagram (cargo xtask readme-diagram --write) -->";
/// The line that closes the generated region.
pub const END: &str = "<!-- END GENERATED: sequence-diagram -->";

/// `readme` with the lines between [`BEGIN`] and [`END`] replaced by `block`.
///
/// # Errors
/// If a marker is missing, appears more than once, or the end comes before the beginning.
fn splice(readme: &str, block: &str) -> Result<String, String> {
    let count = |m: &str| readme.lines().filter(|l| l.trim_end() == m).count();
    if count(BEGIN) != 1 || count(END) != 1 {
        return Err(format!(
            "{README} must hold the line `{BEGIN}` and the line `{END}` once each"
        ));
    }
    let mut out = String::with_capacity(readme.len() + block.len());
    let mut inside = false;
    for line in readme.split_inclusive('\n') {
        let bare = line.trim_end();
        if bare == END {
            if !inside {
                return Err(format!(
                    "{README}: the end marker comes before the begin marker"
                ));
            }
            inside = false;
        }
        if !inside {
            out.push_str(line);
        }
        if bare == BEGIN {
            inside = true;
            out.push_str(block);
        }
    }
    Ok(out)
}

/// The block the harness prints for the trace.
fn generate() -> Result<String, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let out = Command::new(cargo)
        .current_dir(repo::root().join("refapp"))
        .args([
            "run",
            "-q",
            "-p",
            "refapp-harness",
            "--locked",
            "--",
            "sequence-diagram",
        ])
        .output()
        .map_err(|e| format!("sequence-diagram: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "sequence-diagram (refapp/) failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    String::from_utf8(out.stdout).map_err(|e| e.to_string())
}

/// Checks that the region of the README is current, or with `write` rewrites it; returns the
/// number of problems.
pub fn run(write: bool) -> Result<usize, String> {
    let path = repo::root().join(README);
    let current = repo::read(&path)?;
    let wanted = splice(&current, &generate()?)?;
    if wanted == current {
        return Ok(0);
    }
    if write {
        std::fs::write(&path, wanted).map_err(|e| format!("{README}: {e}"))?;
        println!("readme-diagram: rewrote the sequence diagram of {README}");
        return Ok(0);
    }
    println!(
        "{README}: the sequence diagram differs from the golden trace; run `cargo xtask fix` \
         (or `cargo xtask readme-diagram --write`)"
    );
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::{BEGIN, END, splice};

    #[test]
    fn replaces_only_the_region() {
        // req: APP-153
        let readme = format!("top\n{BEGIN}\nold\nolder\n{END}\nbottom\n");
        let got = splice(&readme, "new\n").unwrap();
        assert_eq!(got, format!("top\n{BEGIN}\nnew\n{END}\nbottom\n"));
        assert_eq!(splice(&got, "new\n").unwrap(), got);
    }

    #[test]
    fn a_missing_duplicate_or_reversed_marker_is_an_error() {
        // req: APP-153
        assert!(splice("no markers\n", "x\n").is_err());
        assert!(splice(&format!("{BEGIN}\n"), "x\n").is_err());
        assert!(splice(&format!("{BEGIN}\n{END}\n{END}\n"), "x\n").is_err());
        assert!(splice(&format!("{END}\n{BEGIN}\n"), "x\n").is_err());
    }
}
