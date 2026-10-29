//! `xtask fix`: writes every registration that can be derived from the repository, so that a
//! work package edits none of them by hand (docs/milestones/checklists.md).
//!
//! 1. **Generated files.** Runs the generators that are otherwise run one by one: `fields
//!    --write` (field codes, API-082), `vectors schema` (the vector schema), `sana --write`
//!    (the registry tables, OI-06), and `header --write` (the C header and the `aldis-ffi`
//!    capacities, CABI-002, CABI-030).
//! 2. **Derived lists.** Rewrites the lists that only restate the workspace: the crate matrix
//!    of the nightly `mutants` job (VER-025), the target matrix of the nightly `fuzz` job
//!    (VER-022; the targets are the `[[bin]]` of `fuzz/Cargo.toml`), the `aldis-*`
//!    dependencies of the exerciser (VER-010), and the dependencies and features of the
//!    facade (`crates/aldis`). Existing entries keep their order and their comments; a missing
//!    entry is appended, a stale one dropped.
//! 3. **What is left.** Runs `consistency` and prints what it still finds missing: the
//!    registrations that need human content (a crate README, a vector README row, a test that
//!    runs a vector file, an exerciser module, a C ABI vector runner entry).
//!
//! The command is idempotent: a second run changes nothing. Lists that are derived at check
//! time have no stored copy: the coverage gate, the `features` crate list, and the C ABI
//! vector areas ([`crate::layers::gated`], [`crate::features::crates`]).

use std::collections::BTreeSet;

use crate::{consistency, fields, header, layers, repo, sana, vectors};

const NIGHTLY: &str = ".github/workflows/nightly.yml";
const EXERCISER: &str = "exerciser/Cargo.toml";
const FACADE: &str = "crates/aldis/Cargo.toml";

/// `existing` without the entries not in `wanted`, then the entries of `wanted` it lacks.
fn merge(existing: &[String], wanted: &[String]) -> Vec<String> {
    let mut out: Vec<String> = existing
        .iter()
        .filter(|e| wanted.contains(e))
        .cloned()
        .collect();
    for w in wanted {
        if !out.contains(w) {
            out.push(w.clone());
        }
    }
    out
}

fn to_lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

fn from_lines(lines: &[String]) -> String {
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// The `crate: [a, b]` matrix line of the workflow job `job`, rewritten to `wanted`.
fn workflow_crates(yml: &str, job: &str, wanted: &[String]) -> Result<String, String> {
    let mut lines = to_lines(yml);
    let start = lines
        .iter()
        .position(|l| l.trim_end() == format!("  {job}:"))
        .ok_or_else(|| format!("{NIGHTLY}: no job `{job}`"))?;
    let at = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .take_while(|(_, l)| l.is_empty() || l.starts_with("   "))
        .find(|(_, l)| l.trim().starts_with("crate: ["))
        .map(|(i, _)| i)
        .ok_or_else(|| format!("{NIGHTLY}: job `{job}` has no inline `crate: [...]` matrix"))?;
    let line = lines[at].clone();
    let indent = &line[..line.len() - line.trim_start().len()];
    let inner = line
        .trim()
        .trim_start_matches("crate: [")
        .trim_end_matches(']');
    let existing: Vec<String> = inner
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    lines[at] = format!("{indent}crate: [{}]", merge(&existing, wanted).join(", "));
    Ok(from_lines(&lines))
}

/// The `target:` list of the workflow job `job`, rewritten to `wanted`.
fn workflow_targets(yml: &str, job: &str, wanted: &[String]) -> Result<String, String> {
    let mut lines = to_lines(yml);
    let start = lines
        .iter()
        .position(|l| l.trim_end() == format!("  {job}:"))
        .ok_or_else(|| format!("{NIGHTLY}: no job `{job}`"))?;
    let key = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .take_while(|(_, l)| l.is_empty() || l.starts_with("   "))
        .find(|(_, l)| l.trim() == "target:")
        .map(|(i, _)| i)
        .ok_or_else(|| format!("{NIGHTLY}: job `{job}` has no `target:` list"))?;
    let first = key + 1;
    let len = lines[first..]
        .iter()
        .take_while(|l| l.trim().starts_with("- "))
        .count();
    let indent = lines.get(first).filter(|_| len > 0).map_or_else(
        || {
            format!(
                "{}  ",
                &lines[key][..lines[key].len() - lines[key].trim_start().len()]
            )
        },
        |l| l[..l.len() - l.trim_start().len()].to_owned(),
    );
    let existing: Vec<String> = lines[first..first + len]
        .iter()
        .filter_map(|l| l.trim().strip_prefix("- ").map(|n| n.trim().to_owned()))
        .collect();
    let items: Vec<String> = merge(&existing, wanted)
        .into_iter()
        .map(|n| format!("{indent}- {n}"))
        .collect();
    lines.splice(first..first + len, items);
    Ok(from_lines(&lines))
}

/// Inserts `new` after the last line in `from..to` for which `after` holds, or after `from`.
fn insert_after_last(
    lines: &mut Vec<String>,
    from: usize,
    to: usize,
    after: impl Fn(&str) -> bool,
    new: String,
) {
    let at = (from + 1..to)
        .rev()
        .find(|&i| after(lines[i].trim()))
        .unwrap_or(from);
    lines.insert(at + 1, new);
}

/// The line range `(header, end)` of a TOML table `[name]`: `end` is the next table or the end.
fn table(lines: &[String], name: &str) -> Option<(usize, usize)> {
    let h = lines.iter().position(|l| l.trim() == format!("[{name}]"))?;
    let end = lines
        .iter()
        .enumerate()
        .skip(h + 1)
        .find(|(_, l)| l.starts_with('['))
        .map_or(lines.len(), |(i, _)| i);
    Some((h, end))
}

/// The line range `(key line, closing bracket)` of the multi-line array `key = [`.
fn array(lines: &[String], key: &str) -> Option<(usize, usize)> {
    let h = lines
        .iter()
        .position(|l| l.trim() == format!("{key} = ["))?;
    let end = (h + 1..lines.len()).find(|&i| lines[i].trim() == "]")?;
    Some((h, end))
}

/// Adds `name = <spec>` to the `[dependencies]` of `manifest` after its last `aldis-` entry.
fn add_dependency(lines: &mut Vec<String>, name: &str, spec: &str) -> Result<(), String> {
    let (from, to) = table(lines, "dependencies").ok_or("no [dependencies] table")?;
    if lines[from..to]
        .iter()
        .any(|l| l.trim_start().starts_with(&format!("{name} =")))
    {
        return Ok(());
    }
    insert_after_last(
        lines,
        from,
        to,
        |l| l.starts_with("aldis-"),
        format!("{name} = {spec}"),
    );
    Ok(())
}

/// The exerciser manifest with a dependency on every crate in `wanted`.
fn exerciser_manifest(manifest: &str, wanted: &[String]) -> Result<String, String> {
    let mut lines = to_lines(manifest);
    for n in wanted {
        add_dependency(
            &mut lines,
            n,
            &format!("{{ path = \"../crates/{n}\", default-features = false }}"),
        )?;
    }
    Ok(from_lines(&lines))
}

/// Adds `entry` to the multi-line array `key` unless it has it, after the last line `after`
/// accepts.
fn add_to_array(
    lines: &mut Vec<String>,
    key: &str,
    entry: &str,
    after: impl Fn(&str) -> bool,
) -> Result<(), String> {
    let (from, to) = array(lines, key).ok_or_else(|| format!("no `{key} = [` array"))?;
    let item = format!("\"{entry}\",");
    if lines[from..to].iter().any(|l| l.trim() == item) {
        return Ok(());
    }
    insert_after_last(lines, from, to, after, format!("    {item}"));
    Ok(())
}

/// The facade manifest with, for every crate of `layered`, its optional dependency, its
/// feature, its `alloc` and `std` forwarding, and its entry in `full`.
fn facade_manifest(manifest: &str, layered: &[String]) -> Result<String, String> {
    let mut lines = to_lines(manifest);
    let shorts: BTreeSet<String> = layered
        .iter()
        .map(|n| n.strip_prefix("aldis-").unwrap_or(n).to_owned())
        .collect();
    for n in layered {
        let short = n.strip_prefix("aldis-").unwrap_or(n);
        add_dependency(
            &mut lines,
            n,
            &format!("{{ path = \"../{n}\", default-features = false, optional = true }}"),
        )?;
        let (from, to) = table(&lines, "features").ok_or("no [features] table")?;
        if !lines[from..to]
            .iter()
            .any(|l| l.trim_start().starts_with(&format!("{short} = [")))
        {
            insert_after_last(
                &mut lines,
                from,
                to,
                |l| l.contains("= [\"dep:aldis-"),
                format!("{short} = [\"dep:{n}\"]"),
            );
        }
        for f in ["alloc", "std"] {
            let tail = format!("?/{f}\",");
            add_to_array(&mut lines, f, &format!("{n}?/{f}"), |l| {
                l.ends_with(&tail) || l == "\"alloc\","
            })?;
        }
        add_to_array(&mut lines, "full", short, |l| {
            l.strip_prefix('"')
                .and_then(|r| r.strip_suffix("\","))
                .is_some_and(|name| shorts.contains(name) || name == "spp-time")
        })?;
    }
    Ok(from_lines(&lines))
}

/// Writes `new` to `rel` when it differs; returns whether it did.
fn write(rel: &str, new: &str) -> Result<bool, String> {
    let path = repo::root().join(rel);
    if repo::read(&path)? == new {
        return Ok(false);
    }
    std::fs::write(&path, new).map_err(|e| format!("{rel}: {e}"))?;
    println!("fix: wrote {rel}");
    Ok(true)
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

/// The derived lists (step 2); returns the number of files written.
fn derived() -> Result<usize, String> {
    let gated = names(&layers::gated()?);
    let mut exercised = gated.clone();
    exercised.extend(names(crate::coverage::REPORTED));
    let layered: Vec<String> = layers::LAYERS
        .iter()
        .map(|(n, _)| (*n).to_owned())
        .collect();
    let bins: Vec<String> =
        consistency::bin_targets(&repo::read(&repo::root().join("fuzz/Cargo.toml"))?)
            .into_iter()
            .collect();

    let nightly = repo::read(&repo::root().join(NIGHTLY))?;
    let nightly = workflow_crates(&nightly, "mutants", &gated)?;
    let nightly = workflow_targets(&nightly, "fuzz", &bins)?;
    let mut written = usize::from(write(NIGHTLY, &nightly)?);

    let manifest = repo::read(&repo::root().join(EXERCISER))?;
    written += usize::from(write(
        EXERCISER,
        &exerciser_manifest(&manifest, &exercised)?,
    )?);

    let manifest = repo::read(&repo::root().join(FACADE))?;
    written += usize::from(write(FACADE, &facade_manifest(&manifest, &layered)?)?);
    Ok(written)
}

/// The working tree as git sees it: the diff and the untracked files. Two equal snapshots mean
/// a step changed nothing. Empty outside a git checkout.
fn snapshot() -> String {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(repo::root())
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    };
    git(&["diff", "--no-color"]) + &git(&["ls-files", "--others", "--exclude-standard"])
}

/// `xtask fix`: returns the number of problems left (generator failures and `consistency`).
pub fn run() -> Result<usize, String> {
    let mut problems = 0;
    let before = snapshot();
    let generators: [crate::Step; 4] = [
        ("fields --write", || fields::run(true)),
        ("vectors schema", || vectors::schema(false)),
        ("sana --write", || sana::run(&["--write"])),
        ("header --write", || header::run(true)),
    ];
    for (name, generator) in generators {
        match generator() {
            Ok(n) => problems += n,
            Err(e) => {
                println!("fix: {name}: {e}");
                problems += 1;
            }
        }
    }
    let changed = if snapshot() == before {
        "changed nothing"
    } else {
        "changed files"
    };
    println!("fix: generators {changed}");
    let written = derived()?;
    println!("fix: {written} derived file(s) written");
    println!("fix: still to write by hand (consistency):");
    problems += consistency::check()?;
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NIGHTLY_YML: &str = "jobs:\n  fuzz:\n    strategy:\n      matrix:\n        target:\n          - a_decode\n          - b_roundtrip\n    runs-on: x\n  mutants:\n    strategy:\n      matrix:\n        crate: [aldis-core, aldis-gone]\n    runs-on: y\n";

    const FACADE_TOML: &str = "[dependencies]\naldis-core = { path = \"../aldis-core\", default-features = false, optional = true }\n# A hand-written note.\n\n[features]\ncore = [\"dep:aldis-core\"]\n# Kept by hand.\nspp-time = [\"spp\", \"aldis-spp/time\"]\nalloc = [\n    \"aldis-core?/alloc\",\n]\nstd = [\n    \"alloc\",\n    \"aldis-core?/std\",\n]\nfull = [\n    \"core\",\n    \"spp-time\",\n    \"aldis-link/spp\",\n]\n\n[lints]\nworkspace = true\n";

    // req: VER-022, VER-025
    #[test]
    fn workflow_lists_are_derived_idempotently() {
        let crates = names(&["aldis-core", "aldis-new"]);
        let once = workflow_crates(NIGHTLY_YML, "mutants", &crates).unwrap();
        assert!(once.contains("crate: [aldis-core, aldis-new]"), "{once}");
        assert_eq!(workflow_crates(&once, "mutants", &crates).unwrap(), once);
        let bins = names(&["b_roundtrip", "c_new"]);
        let twice = workflow_targets(&once, "fuzz", &bins).unwrap();
        assert!(
            twice.contains("          - b_roundtrip\n          - c_new\n    runs-on: x"),
            "{twice}"
        );
        assert!(!twice.contains("a_decode"));
        assert_eq!(workflow_targets(&twice, "fuzz", &bins).unwrap(), twice);
        assert!(workflow_crates(NIGHTLY_YML, "absent", &crates).is_err());
    }

    // req: VER-010
    #[test]
    fn exerciser_gets_a_dependency_for_a_new_crate_and_keeps_the_rest() {
        let manifest = "[dependencies]\n# why\naldis-core = { path = \"../crates/aldis-core\" }\n\n[features]\nx = []\n";
        let wanted = names(&["aldis-core", "aldis-tm"]);
        let once = exerciser_manifest(manifest, &wanted).unwrap();
        assert_eq!(
            once,
            "[dependencies]\n# why\naldis-core = { path = \"../crates/aldis-core\" }\naldis-tm = { path = \"../crates/aldis-tm\", default-features = false }\n\n[features]\nx = []\n"
        );
        assert_eq!(exerciser_manifest(&once, &wanted).unwrap(), once);
    }

    // req: GEN-006
    #[test]
    fn facade_gets_every_registration_of_a_new_crate() {
        let layered = names(&["aldis-core", "aldis-tm"]);
        let once = facade_manifest(FACADE_TOML, &layered).unwrap();
        for want in [
            "aldis-tm = { path = \"../aldis-tm\", default-features = false, optional = true }",
            "tm = [\"dep:aldis-tm\"]",
            "    \"aldis-tm?/alloc\",",
            "    \"aldis-tm?/std\",",
            "    \"tm\",",
            "# A hand-written note.",
            "# Kept by hand.",
            "    \"aldis-link/spp\",",
        ] {
            assert!(once.contains(want), "{want}\n{once}");
        }
        // `tm` joins `full` after the last plain feature, ahead of the entries for `link`.
        assert!(
            once.contains("    \"spp-time\",\n    \"tm\",\n    \"aldis-link/spp\","),
            "{once}"
        );
        assert_eq!(facade_manifest(&once, &layered).unwrap(), once);
        // Hand-written content is untouched when nothing is missing.
        let same = facade_manifest(FACADE_TOML, &names(&["aldis-core"])).unwrap();
        assert_eq!(same, FACADE_TOML);
    }

    // req: VER-025
    #[test]
    fn merge_keeps_order_drops_stale_and_appends() {
        let got = merge(&names(&["b", "x", "a"]), &names(&["a", "b", "c"]));
        assert_eq!(got, ["b", "a", "c"]);
    }
}
