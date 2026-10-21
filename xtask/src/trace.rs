//! `xtask trace`: requirement → verification artefact matrix (VER-020).
//!
//! Artefacts are collected from `// req: <IDs>` annotations in Rust sources (including the
//! reference application in `refapp/` and the cross-checks in `crosscheck/`) and in the C++
//! sources and headers under `cpp/` (labelled `test (C++)`, CPP-057), the `requirements` fields of test vectors and of
//! golden-trace headers (`refapp/golden/*.jsonl`, docs/reference-application.md §7.2), and
//! the analysis and inspection records, one file each in `docs/verification/records/`
//! ([`crate::records`]).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use aldis_testkit::VectorFile;
use serde_json::json;

use crate::md::{scan_ids, tables};
use crate::repo;
use crate::spec::{CPP_EXTENSIONS, Spec};

fn add(
    map: &mut BTreeMap<String, BTreeSet<String>>,
    spec: &Spec,
    ids: &[crate::md::IdRef],
    artefact: &str,
) {
    for r in ids {
        for id in spec.expand(&r.area, r.number, r.range_end) {
            map.entry(id).or_default().insert(artefact.to_owned());
        }
    }
}

/// Artefact label of a `// req:` annotation in a C++ source or header (CPP-057).
const CPP_LABEL: &str = "test (C++)";

/// The `// req:` annotations of a C++ source: 1-based line and the text after `// req:`.
fn req_annotations(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines()
        .enumerate()
        .filter_map(|(n, line)| line.split_once("// req:").map(|(_, ids)| (n + 1, ids)))
}

/// Collects artefacts for every requirement.
fn artefacts(spec: &Spec) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let areas = spec.area_set();
    let mut map = BTreeMap::new();

    for dir in [
        "crates",
        "xtask",
        "fuzz",
        "exerciser",
        "refapp",
        "crosscheck",
    ] {
        let kind = if dir == "fuzz" { "fuzz" } else { "test" };
        for path in repo::files(dir, &["rs"]) {
            let text = repo::read(&path)?;
            for (n, line) in text.lines().enumerate() {
                if let Some((_, ids)) = line.split_once("// req:") {
                    let refs = scan_ids(ids, &areas);
                    add(
                        &mut map,
                        spec,
                        &refs,
                        &format!("{kind} {}:{}", repo::rel(&path), n + 1),
                    );
                }
            }
        }
    }

    // C++ sources and headers of the native implementation (CPP-057).
    for path in repo::files("cpp", CPP_EXTENSIONS) {
        let text = repo::read(&path)?;
        for (line, ids) in req_annotations(&text) {
            let refs = scan_ids(ids, &areas);
            let artefact = format!("{CPP_LABEL} {}:{line}", repo::rel(&path));
            add(&mut map, spec, &refs, &artefact);
        }
    }

    for path in repo::files("test-vectors", &["json"]) {
        let rel = repo::rel(&path);
        if !crate::vectors::is_vector_file(&rel) {
            continue;
        }
        match VectorFile::from_json(&repo::read(&path)?) {
            Ok(VectorFile::Codec(f)) => {
                for c in f.cases.iter().filter(|c| c.superseded.is_none()) {
                    let refs = scan_ids(&c.requirements.join(" "), &areas);
                    add(&mut map, spec, &refs, &format!("vector {rel}#{}", c.id));
                }
            }
            Ok(VectorFile::EngineScenario(s)) => {
                if s.superseded.is_none() {
                    let refs = scan_ids(&s.requirements.join(" "), &areas);
                    add(&mut map, spec, &refs, &format!("vector {rel}#{}", s.id));
                }
            }
            Err(e) => return Err(format!("{rel}: {e} (run `cargo xtask vectors validate`)")),
        }
    }

    for path in repo::files("refapp/golden", &["jsonl"]) {
        let rel = repo::rel(&path);
        let ids = golden_requirements(&repo::read(&path)?)
            .map_err(|e| format!("{rel}: {e} (run the refapp harness `check-golden`)"))?;
        let refs = scan_ids(&ids.join(" "), &areas);
        add(&mut map, spec, &refs, &format!("golden {rel}"));
    }

    for record in crate::records::load(spec)? {
        let label = record.label();
        for id in record.ids {
            map.entry(id).or_default().insert(label.clone());
        }
    }
    Ok(map)
}

/// The `requirements` of a golden trace's header line (docs/reference-application.md §7.2).
fn golden_requirements(text: &str) -> Result<Vec<String>, String> {
    let header: serde_json::Value = serde_json::from_str(text.lines().next().unwrap_or_default())
        .map_err(|e| format!("header: {e}"))?;
    if header["kind"] != "header" {
        return Err("line 1 is not a header".to_owned());
    }
    header["requirements"]
        .as_array()
        .ok_or("header has no `requirements` array")?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| "`requirements` must hold strings".to_owned())
        })
        .collect()
}

/// Requirement IDs in scope of a milestone: the exit-criteria table of `docs/milestones/<scope>-*.md`.
fn scope_ids(spec: &Spec, scope: &str) -> Result<BTreeSet<String>, String> {
    let file = repo::files("docs/milestones", &["md"])
        .into_iter()
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&format!("{scope}-")))
        })
        .ok_or_else(|| format!("no docs/milestones/{scope}-*.md"))?;
    let areas = spec.area_set();
    let mut ids = BTreeSet::new();
    for t in tables(&repo::read(&file)?) {
        // Further columns (e.g. results at the exit review) are allowed; only Criteria is read.
        if t.header
            .starts_with(&["Area".to_owned(), "Criteria".to_owned()])
        {
            for (_, row) in t.rows {
                for r in scan_ids(row.get(1).map_or("", String::as_str), &areas) {
                    ids.extend(spec.expand(&r.area, r.number, r.range_end));
                }
            }
        }
    }
    if ids.is_empty() {
        return Err(format!("{}: no Area/Criteria exit table", repo::rel(&file)));
    }
    Ok(ids)
}

/// Runs the trace. Returns the number of in-scope requirements without artefacts when
/// `strict`, otherwise 0.
pub fn run(scope: Option<&str>, strict: bool) -> Result<usize, String> {
    let spec = Spec::load()?;
    let arts = artefacts(&spec)?;
    let in_scope: BTreeSet<String> = match scope {
        Some(s) => scope_ids(&spec, s)?,
        None => spec.requirements.keys().cloned().collect(),
    };

    let mut md = format!(
        "# Trace matrix{}\n\n| ID | Verif. | Status | Artefacts |\n|---|---|---|---|\n",
        scope.map(|s| format!(" — scope {s}")).unwrap_or_default()
    );
    let mut rows = Vec::new();
    let mut untraced = Vec::new();
    for id in &in_scope {
        let r = &spec.requirements[id];
        let a = arts.get(id).cloned().unwrap_or_default();
        let status = if a.is_empty() { "untraced" } else { "traced" };
        if a.is_empty() {
            untraced.push(id.clone());
        }
        let list = a
            .iter()
            .map(|x| format!("`{x}`"))
            .collect::<Vec<_>>()
            .join("<br>");
        let _ = writeln!(md, "| {id} | {} | {status} | {list} |", r.verif);
        rows.push(json!({ "id": id, "verif": r.verif, "status": status, "artefacts": a }));
    }

    let out = repo::root().join("target/spec");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let name = scope.map_or_else(|| "trace".to_owned(), |s| format!("trace-{s}"));
    std::fs::write(out.join(format!("{name}.md")), md).map_err(|e| e.to_string())?;
    let body = serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())?;
    std::fs::write(out.join(format!("{name}.json")), body + "\n").map_err(|e| e.to_string())?;

    let traced = in_scope.len() - untraced.len();
    println!(
        "trace{}: {traced}/{} requirements traced; report: target/spec/{name}.md",
        scope.map(|s| format!(" --scope {s}")).unwrap_or_default(),
        in_scope.len()
    );
    if !untraced.is_empty() {
        println!("untraced: {}", untraced.join(", "));
    }
    Ok(if strict { untraced.len() } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::{CPP_LABEL, golden_requirements, req_annotations};
    use crate::md::{IdRef, scan_ids};
    use crate::spec::CPP_EXTENSIONS;

    #[test]
    fn cpp_annotations_are_traced() {
        // req: CPP-057, VER-020
        assert_eq!(CPP_LABEL, "test (C++)");
        assert_eq!(CPP_EXTENSIONS, ["hpp", "cpp", "h"]);
        let text =
            "TEST(Bytes, Reader) {\n    // req: CPP-031, GEN-013\n    EXPECT_TRUE(true);\n}\n";
        let found: Vec<(usize, &str)> = req_annotations(text).collect();
        assert_eq!(found, [(2, " CPP-031, GEN-013")]);
        let areas = ["CPP", "GEN"].iter().map(|a| (*a).to_owned()).collect();
        let ids: Vec<String> = scan_ids(found[0].1, &areas).iter().map(IdRef::id).collect();
        assert_eq!(ids, ["CPP-031", "GEN-013"]);
    }

    #[test]
    fn golden_headers() {
        // req: APP-108
        let ok = "{\"kind\":\"header\",\"requirements\":[\"APP-080\",\"APP-081\"]}\n{\"t_us\":0}\n";
        assert_eq!(golden_requirements(ok).unwrap(), ["APP-080", "APP-081"]);
        assert!(golden_requirements("{\"kind\":\"unit_sent\"}\n").is_err());
        assert!(golden_requirements("{\"kind\":\"header\"}\n").is_err());
        assert!(golden_requirements("").is_err());
    }
}
