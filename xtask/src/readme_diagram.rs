//! `xtask readme-diagram`: the generated regions of the README for the COP-1 run over a lossy
//! link: its sequence diagram (APP-153), decoded from the golden trace, and the output of the
//! one-command demo (APP-154), decoded from a fresh run that the demo compares with the golden
//! trace.
//!
//! `refapp-harness sequence-diagram` decodes `refapp/golden/a3-uplink-loss.jsonl` and
//! `refapp-harness demo` decodes a run of the case, both with the strict codecs. This module splices what they
//! print between the marker comments of each [`Region`] of `README.md`. Without `--write` it
//! fails when a committed region differs, and `cargo xtask fix` rewrites it. Only this tool
//! writes the regions; the check reads `README.md` and the trace, both inputs of the
//! verification run's `refapp` check.

use std::process::Command;

use crate::repo;

/// The file that holds the region.
const README: &str = "README.md";

/// One generated region of the README.
struct Region {
    /// The line that opens it.
    begin: &'static str,
    /// The line that closes it.
    end: &'static str,
    /// The `refapp-harness` command whose output fills it.
    command: &'static str,
    /// Text put before and after that output.
    fence: (&'static str, &'static str),
}

/// The regions, in README order.
const REGIONS: [Region; 2] = [
    Region {
        begin: "<!-- BEGIN GENERATED: demo-output (cargo xtask readme-diagram --write) -->",
        end: "<!-- END GENERATED: demo-output -->",
        command: "demo",
        fence: ("```text\n", "```\n"),
    },
    Region {
        begin: "<!-- BEGIN GENERATED: sequence-diagram (cargo xtask readme-diagram --write) -->",
        end: "<!-- END GENERATED: sequence-diagram -->",
        command: "sequence-diagram",
        fence: ("", ""),
    },
];

/// `readme` with the lines between `begin` and `end` replaced by `block`.
///
/// # Errors
/// If a marker is missing, appears more than once, or the end comes before the beginning.
fn splice(readme: &str, begin: &str, end: &str, block: &str) -> Result<String, String> {
    let count = |m: &str| readme.lines().filter(|l| l.trim_end() == m).count();
    if count(begin) != 1 || count(end) != 1 {
        return Err(format!(
            "{README} must hold the line `{begin}` and the line `{end}` once each"
        ));
    }
    let mut out = String::with_capacity(readme.len() + block.len());
    let mut inside = false;
    for line in readme.split_inclusive('\n') {
        let bare = line.trim_end();
        if bare == end {
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
        if bare == begin {
            inside = true;
            out.push_str(block);
        }
    }
    Ok(out)
}

/// The block the harness prints for the trace.
fn generate(region: &Region) -> Result<String, String> {
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
            region.command,
        ])
        .output()
        .map_err(|e| format!("{}: {e}", region.command))?;
    if !out.status.success() {
        return Err(format!(
            "{} (refapp/) failed: {}{}",
            region.command,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let text = String::from_utf8(out.stdout).map_err(|e| e.to_string())?;
    Ok(format!("{}{text}{}", region.fence.0, region.fence.1))
}

/// Checks that the region of the README is current, or with `write` rewrites it; returns the
/// number of problems.
pub fn run(write: bool) -> Result<usize, String> {
    let path = repo::root().join(README);
    let current = repo::read(&path)?;
    let mut wanted = current.clone();
    for region in &REGIONS {
        wanted = splice(&wanted, region.begin, region.end, &generate(region)?)?;
    }
    if wanted == current {
        return Ok(0);
    }
    if write {
        std::fs::write(&path, wanted).map_err(|e| format!("{README}: {e}"))?;
        println!("readme-diagram: rewrote the generated regions of {README}");
        return Ok(0);
    }
    println!(
        "{README}: a generated region differs from the golden trace; run `cargo xtask fix` \
         (or `cargo xtask readme-diagram --write`)"
    );
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::{REGIONS, splice};

    const BEGIN: &str = "<!-- begin -->";
    const END: &str = "<!-- end -->";

    #[test]
    fn replaces_only_the_region() {
        // req: APP-153, APP-154
        let readme = format!("top\n{BEGIN}\nold\nolder\n{END}\nbottom\n");
        let got = splice(&readme, BEGIN, END, "new\n").unwrap();
        assert_eq!(got, format!("top\n{BEGIN}\nnew\n{END}\nbottom\n"));
        assert_eq!(splice(&got, BEGIN, END, "new\n").unwrap(), got);
    }

    #[test]
    fn a_missing_duplicate_or_reversed_marker_is_an_error() {
        // req: APP-153, APP-154
        assert!(splice("no markers\n", BEGIN, END, "x\n").is_err());
        assert!(splice(&format!("{BEGIN}\n"), BEGIN, END, "x\n").is_err());
        assert!(splice(&format!("{BEGIN}\n{END}\n{END}\n"), BEGIN, END, "x\n").is_err());
        assert!(splice(&format!("{END}\n{BEGIN}\n"), BEGIN, END, "x\n").is_err());
    }

    #[test]
    fn the_readme_holds_each_region_once() {
        // req: APP-153, APP-154
        let readme = crate::repo::read(&crate::repo::root().join("README.md")).unwrap();
        for r in &REGIONS {
            assert!(
                splice(&readme, r.begin, r.end, "x\n").is_ok(),
                "{}",
                r.begin
            );
        }
    }
}
