//! The independent models of hand-derived vectors (TV-023), run by `cargo xtask vectors
//! validate`.
//!
//! A vector file whose `provenance_note` names `test-vectors/models/<name>.py` is checked by
//! that program: `python3 -I test-vectors/models/<name>.py --check <files>`, run from the
//! repository root, must exit 0. A named model must exist, and every model in [`DIR`] must be
//! named by a vector file, so that no model is left unchecked. The programs are described in
//! `test-vectors/models/README.md`.

use std::collections::BTreeMap;
use std::process::Command;

use crate::repo;

/// The directory of the models, relative to the repository root.
pub const DIR: &str = "test-vectors/models";

/// The models a provenance note names, as repository paths.
fn named(note: &str) -> Vec<String> {
    let prefix = format!("{DIR}/");
    let mut out: Vec<String> = Vec::new();
    let mut rest = note;
    while let Some(at) = rest.find(&prefix) {
        let tail = rest.get(at..).unwrap_or_default();
        let end = tail
            .find(|c: char| !(c.is_ascii_alphanumeric() || "_-./".contains(c)))
            .unwrap_or(tail.len());
        let path = tail.get(..end).unwrap_or_default().trim_end_matches('.');
        let python = std::path::Path::new(path)
            .extension()
            .is_some_and(|e| e == "py");
        if python && !out.iter().any(|p| p == path) {
            out.push(path.to_owned());
        }
        rest = tail.get(end.max(1)..).unwrap_or_default();
    }
    out
}

/// Groups the vector files by the models their notes name: `(file, note)` → model → files.
fn by_model(notes: &[(String, String)]) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (file, note) in notes {
        for model in named(note) {
            out.entry(model).or_default().push(file.clone());
        }
    }
    out
}

/// Runs every model on the files that name it; returns the problems. `notes` holds each vector
/// file (relative to the root) with its provenance note.
pub fn check(notes: &[(String, String)]) -> Result<Vec<String>, String> {
    let root = repo::root();
    let mut problems = Vec::new();
    let models = by_model(notes);
    for path in repo::files(DIR, &["py"]) {
        let rel = repo::rel(&path);
        if !models.contains_key(&rel) {
            problems.push(format!(
                "{rel}: no vector file names this model in its provenance_note (TV-023)"
            ));
        }
    }
    for (model, files) in &models {
        if !root.join(model).is_file() {
            problems.push(format!(
                "{}: provenance_note names {model}, which does not exist (TV-023)",
                files.join(", ")
            ));
            continue;
        }
        let out = Command::new("python3")
            .arg("-I")
            .arg(model)
            .arg("--check")
            .args(files)
            .current_dir(&root)
            .output()
            .map_err(|e| format!("python3 -I {model}: {e} (the models need python3, TV-023)"))?;
        if !out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            let err = String::from_utf8_lossy(&out.stderr);
            problems.push(format!(
                "{model} --check {} failed (TV-023):\n{}{}",
                files.join(" "),
                text.trim_end(),
                err.trim_end()
            ));
        }
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::{by_model, named};

    #[test]
    fn notes_name_their_models() {
        // req: TV-023
        let note = "Values from test-vectors/models/aos_fhec.py (TV-023); run \
                    `python3 -I test-vectors/models/aos_fhec.py --check x`. Also \
                    test-vectors/models/README.md and test-vectors/models/core_ocf.py.";
        assert_eq!(
            named(note),
            [
                "test-vectors/models/aos_fhec.py",
                "test-vectors/models/core_ocf.py"
            ]
        );
        assert_eq!(named("hand-derived, no program"), Vec::<String>::new());
        let notes = [
            ("a.json".to_owned(), "test-vectors/models/m.py".to_owned()),
            (
                "b.json".to_owned(),
                "see test-vectors/models/m.py.".to_owned(),
            ),
            ("c.json".to_owned(), "none".to_owned()),
        ];
        let groups = by_model(&notes);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups["test-vectors/models/m.py"], ["a.json", "b.json"]);
    }
}
