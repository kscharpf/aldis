//! `xtask vectors validate` and `xtask vectors schema [--check]` (TV-001…003, TV-020…021).

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use aldis_testkit::{
    CodecCase, CodecFile, Expect, FieldCatalogue, Provenance, ScenarioFile, Superseded, VectorFile,
    hex,
};
use serde_json::{Map, Value};

use crate::fields;
use crate::md::scan_ids;
use crate::repo;
use crate::spec::Spec;

const SCHEMA_DIR: &str = "test-vectors/schema";

/// Pattern of release tags; `v*` would also match tags such as `vendor-snapshot` (TV-021).
const RELEASE_TAGS: &str = "v[0-9]*";

const fn needs_note(p: Provenance) -> bool {
    matches!(p, Provenance::HandDerived | Provenance::CrossChecked)
}

/// Checks every string value under a key ending in `_hex` (also inside arrays).
fn check_hex(value: &Value, key: &str, at: &str, problems: &mut Vec<String>) {
    match value {
        Value::String(s) if key.ends_with("_hex") => {
            if let Err(e) = hex::decode(s) {
                problems.push(format!("{at}: {key}: {e}"));
            }
        }
        Value::Array(items) => items.iter().for_each(|v| check_hex(v, key, at, problems)),
        Value::Object(map) => map.iter().for_each(|(k, v)| check_hex(v, k, at, problems)),
        _ => {}
    }
}

fn check_requirements(ids: &[String], spec: &Spec, at: &str, problems: &mut Vec<String>) {
    if ids.is_empty() {
        problems.push(format!("{at}: lists no requirements (TV-003)"));
    }
    let areas = spec.area_set();
    for id in ids {
        let refs = scan_ids(id, &areas);
        if refs.len() != 1 || refs[0].id() != *id || !spec.requirements.contains_key(id) {
            problems.push(format!("{at}: unknown requirement {id:?}"));
        }
    }
}

/// Checks a `superseded` marker: the reason is required and `by` is optional (TV-021). A
/// `by` that is present is queued for [`check_by_links`].
fn check_superseded(
    superseded: Option<&Superseded>,
    at: &str,
    by_links: &mut Vec<(String, String)>,
    problems: &mut Vec<String>,
) {
    let Some(s) = superseded else {
        return;
    };
    if s.reason.trim().is_empty() {
        problems.push(format!("{at}: superseded.reason is empty (TV-021)"));
    }
    if let Some(by) = &s.by {
        by_links.push((at.to_owned(), by.clone()));
    }
}

/// Checks that every `superseded.by` names an existing case that is not itself superseded
/// (TV-021). Case IDs are unique across all files, so the target may be in any file.
fn check_by_links(
    ids: &BTreeMap<String, (String, bool)>,
    by_links: &[(String, String)],
    problems: &mut Vec<String>,
) {
    for (at, by) in by_links {
        match ids.get(by) {
            None => problems.push(format!(
                "{at}: superseded.by {by:?} does not name an existing case (TV-021)"
            )),
            Some((target_at, true)) => problems.push(format!(
                "{at}: superseded.by {by:?} ({target_at}) is itself superseded (TV-021)"
            )),
            Some((_, false)) => {}
        }
    }
}

/// Whether `a` and `b` hold the same keys with equal values, ignoring the keys in `skip`.
fn same_except(a: &Map<String, Value>, b: &Map<String, Value>, skip: &[&str]) -> bool {
    a.iter()
        .filter(|(k, _)| !skip.contains(&k.as_str()))
        .all(|(k, v)| b.get(k) == Some(v))
        && b.keys()
            .filter(|k| !skip.contains(&k.as_str()))
            .all(|k| a.contains_key(k))
}

/// Problems found by comparing with the release tags, at most one per file or case. Tags are
/// compared oldest first, so a message names the first release that contains the file or case.
struct ReleaseFindings<'a> {
    problems: &'a mut Vec<String>,
    seen: BTreeSet<String>,
}

impl ReleaseFindings<'_> {
    /// Records `message` about `at` (a file, or a case as `file#id`) unless a problem with
    /// `at` was already recorded for an older release.
    fn push(&mut self, at: &str, message: String) {
        if self.seen.insert(at.to_owned()) {
            self.problems.push(message);
        }
    }
}

/// A released case may change only by gaining a `superseded` marker (TV-021). The marker's
/// content is checked on the current file by [`check_superseded`], so it is not checked here.
fn check_superseded_transition(
    at: &str,
    tag: &str,
    old_obj: &Map<String, Value>,
    now_obj: &Map<String, Value>,
    out: &mut ReleaseFindings<'_>,
) {
    let old_sup = old_obj.get("superseded");
    if !same_except(old_obj, now_obj, &["superseded"])
        || (old_sup.is_some() && old_sup != now_obj.get("superseded"))
    {
        out.push(
            at,
            format!("{at}: released vector in {tag} was modified in place (TV-021)"),
        );
    }
}

/// The case objects of a file's `cases` (codec) or `scenarios` (engine scenario) array.
fn case_objects<'a>(
    top: &'a Map<String, Value>,
    list: &str,
) -> impl Iterator<Item = &'a Map<String, Value>> {
    top.get(list)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
}

fn check_against_released(
    rel: &str,
    tag: &str,
    released: &Value,
    current: &Value,
    out: &mut ReleaseFindings<'_>,
) {
    let (Some(old_top), Some(cur_top)) = (released.as_object(), current.as_object()) else {
        out.push(
            rel,
            format!("{rel}: non-object JSON in {tag} comparison (TV-021)"),
        );
        return;
    };
    let list = if old_top.get("kind").and_then(Value::as_str) == Some("codec") {
        Some("cases")
    } else if old_top.contains_key("scenarios") {
        Some("scenarios")
    } else {
        None
    };
    if let Some(list) = list {
        // The named setups of a scenario file are append-only: a released setup stays as it was
        // (a released scenario refers to it by name), and new ones may be added.
        if !same_except(old_top, cur_top, &[list, "setups"]) {
            out.push(
                rel,
                format!("{rel}: file-level metadata changed since {tag} (TV-021)"),
            );
        }
        let cur_setups: BTreeMap<&str, &Map<String, Value>> = case_objects(cur_top, "setups")
            .filter_map(|c| Some((c.get("name")?.as_str()?, c)))
            .collect();
        for old_setup in case_objects(old_top, "setups") {
            let name = old_setup.get("name").and_then(Value::as_str).unwrap_or("?");
            let at = format!("{rel}#setup {name}");
            match cur_setups.get(name) {
                None => out.push(
                    &at,
                    format!("{at}: setup released in {tag} was removed (TV-021)"),
                ),
                Some(now) if *now != old_setup => out.push(
                    &at,
                    format!("{at}: setup released in {tag} was modified in place (TV-021)"),
                ),
                Some(_) => {}
            }
        }
        let cur_by_id: BTreeMap<&str, &Map<String, Value>> = case_objects(cur_top, list)
            .filter_map(|c| Some((c.get("id")?.as_str()?, c)))
            .collect();
        for old_obj in case_objects(old_top, list) {
            let id = old_obj.get("id").and_then(Value::as_str).unwrap_or("?");
            let at = format!("{rel}#{id}");
            let Some(&now_obj) = cur_by_id.get(id) else {
                out.push(
                    &at,
                    format!("{at}: case released in {tag} was removed (TV-021)"),
                );
                continue;
            };
            check_superseded_transition(&at, tag, old_obj, now_obj, out);
        }
    } else {
        let id = old_top.get("id").and_then(Value::as_str).unwrap_or("?");
        let at = format!("{rel}#{id}");
        check_superseded_transition(&at, tag, old_top, cur_top, out);
    }
}

/// Whether `rel` (relative to the repository root) is a vector file: a `.json` file under
/// `test-vectors/` outside the schema directory, other than the generated field catalogue
/// (checked by `cargo xtask fields`). The extension is matched exactly, as [`repo::files`]
/// matches it, so the released and the current side see the same files.
pub fn is_vector_file(rel: &str) -> bool {
    rel.starts_with("test-vectors/")
        && !rel.starts_with(&format!("{SCHEMA_DIR}/"))
        && rel != fields::JSON
        && Path::new(rel).extension().is_some_and(|ext| ext == "json")
}

/// Runs `git` in `root` and returns its standard output.
fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

/// The release tags, all of them or only those reachable from HEAD.
fn tag_list(root: &Path, merged: bool) -> Result<Vec<String>, String> {
    let args: &[&str] = if merged {
        &[
            "tag",
            "--list",
            "--merged",
            "HEAD",
            "--sort=v:refname",
            RELEASE_TAGS,
        ]
    } else {
        &["tag", "--list", RELEASE_TAGS]
    };
    Ok(String::from_utf8_lossy(&git(root, args)?)
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Release tags (`v[0-9]*`) reachable from HEAD, oldest version first (TV-021).
///
/// A commit is judged only against the releases it contains, so maintenance branches and
/// `git bisect` over older commits work. Fails if the repository has no release tag at all,
/// which means the tags were not fetched; an empty list means HEAD predates every release.
fn release_tags(root: &Path) -> Result<Vec<String>, String> {
    if tag_list(root, false)?.is_empty() {
        return Err(format!(
            "no release tags (`{RELEASE_TAGS}`) found; fetch tags before running `vectors validate` (TV-021)"
        ));
    }
    tag_list(root, true)
}

/// A released vector file: its path, and its content or why it could not be read.
type ReleasedFile = (String, Result<Value, String>);

/// Requests one object from `git cat-file --batch` and reads its response. Returns `None`
/// if git reports the object missing.
fn cat_file_one(
    stdin: &mut impl Write,
    reader: &mut impl BufRead,
    oid: &str,
) -> Result<Option<Vec<u8>>, String> {
    writeln!(stdin, "{oid}")
        .and_then(|()| stdin.flush())
        .map_err(|e| format!("stdin: {e}"))?;
    let mut header = String::new();
    reader
        .read_line(&mut header)
        .map_err(|e| format!("stdout: {e}"))?;
    let mut fields = header.split_whitespace();
    let size: usize = match (fields.next(), fields.next(), fields.next()) {
        (Some(_), Some("missing"), None) => return Ok(None),
        (Some(_), Some("blob"), Some(size)) => size
            .parse()
            .map_err(|_| format!("unexpected header {header:?}"))?,
        _ => return Err(format!("unexpected header {header:?}")),
    };
    let mut buf = vec![0_u8; size];
    reader
        .read_exact(&mut buf)
        .map_err(|e| format!("stdout: {e}"))?;
    let mut nl = [0_u8; 1];
    reader
        .read_exact(&mut nl)
        .map_err(|e| format!("stdout: {e}"))?;
    if nl != *b"\n" {
        return Err("object not followed by a newline".to_owned());
    }
    Ok(Some(buf))
}

/// The vector files in `tag`, each parsed as JSON or with the reason it could not be.
///
/// `git ls-tree -z` lists paths unquoted, so non-ASCII names are matched like any other, and
/// `git cat-file --batch` reads every blob in one process. Each request is written only after
/// the previous response has been read; without `--buffer`, git flushes every response, so
/// neither pipe can fill up and deadlock, however many files the tag holds.
///
/// # Errors
/// A git failure for the whole tag. A problem with one file is returned with that file.
fn tag_blobs(root: &Path, tag: &str) -> Result<Vec<ReleasedFile>, String> {
    let ls = git(root, &["ls-tree", "-r", "-z", tag, "--", "test-vectors"])?;
    let mut entries = Vec::new();
    for record in ls.split(|&b| b == 0).filter(|r| !r.is_empty()) {
        let record = String::from_utf8_lossy(record);
        // `<mode> SP <type> SP <object> TAB <path>`
        let parsed = record.split_once('\t').and_then(|(meta, path)| {
            let mut meta = meta.split(' ');
            Some((meta.nth(1)?, meta.next()?, path))
        });
        let Some((kind, oid, path)) = parsed else {
            return Err(format!("git ls-tree {tag}: unexpected entry {record:?}"));
        };
        if kind == "blob" && is_vector_file(path) {
            entries.push((path.to_owned(), oid.to_owned()));
        }
    }
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let mut child = Command::new("git")
        .args(["cat-file", "--batch"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git cat-file: {e}"))?;
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return Err("git cat-file: no pipes".to_owned());
    };
    let mut reader = BufReader::new(stdout);
    let mut out = Vec::with_capacity(entries.len());
    let mut failed = None;
    for (rel, oid) in entries {
        match cat_file_one(&mut stdin, &mut reader, &oid) {
            Ok(Some(bytes)) => {
                let val = serde_json::from_slice(&bytes).map_err(|e| format!("invalid JSON: {e}"));
                out.push((rel, val));
            }
            Ok(None) => out.push((rel, Err(format!("object {oid} is missing")))),
            Err(e) => {
                failed = Some(format!("git cat-file {tag}:{rel}: {e}"));
                break;
            }
        }
    }
    // Closing both pipes ends git even if it is still writing a response.
    drop(stdin);
    drop(reader);
    let status = child.wait().map_err(|e| format!("git cat-file: {e}"))?;
    if let Some(e) = failed {
        return Err(e);
    }
    if !status.success() {
        return Err(format!("git cat-file exited with {status}"));
    }
    Ok(out)
}

/// Compares the vector files of every release tag reachable from HEAD with the current files
/// (TV-021).
///
/// `current` maps each current vector file to its JSON, or to `None` if it could not be read
/// or parsed (already reported). A problem with one tag or file is reported and the
/// comparison goes on; see [`ReleaseFindings`] for how repeats across tags are dropped.
fn check_released(
    root: &Path,
    current: &BTreeMap<String, Option<Value>>,
    problems: &mut Vec<String>,
) {
    let tags = match release_tags(root) {
        Ok(tags) => tags,
        Err(e) => {
            problems.push(e);
            return;
        }
    };
    let mut out = ReleaseFindings {
        problems,
        seen: BTreeSet::new(),
    };
    for tag in &tags {
        let released = match tag_blobs(root, tag) {
            Ok(released) => released,
            Err(e) => {
                out.problems.push(format!("{tag}: {e} (TV-021)"));
                continue;
            }
        };
        for (rel, blob) in released {
            match (blob, current.get(&rel)) {
                (Err(e), _) => out.problems.push(format!("{tag}:{rel}: {e} (TV-021)")),
                (Ok(_), None) => out.push(
                    &rel,
                    format!("{rel}: vector file released in {tag} was removed or renamed (TV-021)"),
                ),
                (Ok(_), Some(None)) => {}
                (Ok(old), Some(Some(now))) => {
                    check_against_released(&rel, tag, &old, now, &mut out);
                }
            }
        }
    }
}

/// A field code names the field of an expected error, and must be catalogued (API-082, M2 plan
/// D3).
fn check_field(
    expect: &Expect,
    field_codes: &BTreeMap<u16, String>,
    at: &str,
    problems: &mut Vec<String>,
) {
    match expect.field {
        Some(code) if expect.ok => {
            problems.push(format!(
                "{at}: expect.ok is true but a field code ({code}) is given"
            ));
        }
        Some(code) if !field_codes.contains_key(&code.get()) => {
            problems.push(format!(
                "{at}: field code {code} is not in the catalogue (03-api-model.md §9.1)"
            ));
        }
        _ => {}
    }
}

fn check_codec_case(
    c: &CodecCase,
    spec: &Spec,
    field_codes: &BTreeMap<u16, String>,
    at: &str,
    by_links: &mut Vec<(String, String)>,
    problems: &mut Vec<String>,
) {
    check_requirements(&c.requirements, spec, at, problems);
    check_superseded(c.superseded.as_ref(), at, by_links, problems);
    if let Some(h) = &c.input_hex
        && let Err(e) = hex::decode(h)
    {
        problems.push(format!("{at}: input_hex: {e}"));
    }
    match (c.expect.ok, c.expect.error.as_deref()) {
        (true, Some(_)) => {
            problems.push(format!("{at}: expect.ok is true but an error is named"));
        }
        (false, None) => {
            problems.push(format!(
                "{at}: expect.ok is false but no error kind is named"
            ));
        }
        (false, Some(kind)) if !spec.error_kinds.contains(kind) => {
            problems.push(format!(
                "{at}: error {kind:?} is not in the catalogue (03-api-model.md §9)"
            ));
        }
        _ => {}
    }
    check_field(&c.expect, field_codes, at, problems);
    for (k, v) in c
        .expect
        .values
        .iter()
        .chain(c.params.iter().flatten())
        .chain(c.config.iter().flatten())
    {
        check_hex(v, k, at, problems);
    }
}

/// Checks an engine-scenario file; returns `(id, location, superseded)` of each scenario.
fn check_scenario_file(
    rel: &str,
    file: &ScenarioFile,
    spec: &Spec,
    by_links: &mut Vec<(String, String)>,
    problems: &mut Vec<String>,
) -> Vec<(String, String, bool)> {
    if needs_note(file.provenance) && file.provenance_note.is_none() {
        problems.push(format!(
            "{rel}: provenance {:?} requires provenance_note (TV-020)",
            file.provenance
        ));
    }
    for (k, v) in &file.config {
        check_hex(v, k, rel, problems);
    }
    let resolved = match file.resolve() {
        Ok(resolved) => resolved,
        Err(e) => {
            problems.push(format!("{rel}: {e}"));
            return Vec::new();
        }
    };
    for setup in &file.setups {
        for step in &setup.steps {
            for (k, v) in step.input.iter().chain(step.expect.iter().flatten()) {
                check_hex(v, k, &format!("{rel}#setup {}", setup.name), problems);
            }
        }
    }
    let mut found = Vec::new();
    for r in resolved {
        let sc = r.scenario;
        let at = format!("{rel}#{}", sc.id);
        check_requirements(&sc.requirements, spec, &at, problems);
        check_superseded(sc.superseded.as_ref(), &at, by_links, problems);
        if r.steps.windows(2).any(|w| w[1].at_us < w[0].at_us) {
            problems.push(format!("{at}: step instants must not decrease (TV-010)"));
        }
        for (k, v) in r.config {
            check_hex(v, k, &at, problems);
        }
        for step in &sc.steps {
            for (k, v) in step.input.iter().chain(step.expect.iter().flatten()) {
                check_hex(v, k, &at, problems);
            }
        }
        if r.steps
            .iter()
            .any(|s| s.expect_no_timeout && s.expect_timeout_us.is_some())
        {
            problems.push(format!(
                "{at}: a step asserts a deadline and no deadline (TV-010)"
            ));
        }
        found.push((sc.id.clone(), at, sc.superseded.is_some()));
    }
    found
}

/// Validates all vector files; returns the number of problems.
pub fn validate() -> Result<usize, String> {
    let spec = Spec::load()?;
    let mut problems = Vec::new();
    // A broken catalogue is reported by `cargo xtask fields`; here every code is then unknown.
    let field_codes = fields::load().unwrap_or_default();
    let mut ids: BTreeMap<String, (String, bool)> = BTreeMap::new();
    let mut by_links: Vec<(String, String)> = Vec::new();
    let mut current_raw: BTreeMap<String, Option<Value>> = BTreeMap::new();
    let mut files = 0;
    let mut cases = 0;
    // Each file with its provenance note, for the independent models (TV-023).
    let mut notes: Vec<(String, String)> = Vec::new();

    for path in repo::files("test-vectors", &["json"]) {
        let rel = repo::rel(&path);
        if !is_vector_file(&rel) {
            continue;
        }
        files += 1;
        let text = match repo::read(&path) {
            Ok(text) => text,
            Err(e) => {
                problems.push(e);
                current_raw.insert(rel, None);
                continue;
            }
        };
        current_raw.insert(rel.clone(), serde_json::from_str::<Value>(&text).ok());
        let mut unique = |id: &str, at: &str, is_superseded: bool, problems: &mut Vec<String>| {
            if let Some((prev, _)) = ids.insert(id.to_owned(), (at.to_owned(), is_superseded)) {
                problems.push(format!("{at}: duplicate id {id:?} (also {prev})"));
            }
        };
        match VectorFile::from_json(&text) {
            Err(e) => problems.push(format!("{rel}: {e}")),
            Ok(VectorFile::Codec(CodecFile {
                provenance,
                provenance_note,
                cases: list,
                ..
            })) => {
                if needs_note(provenance) && provenance_note.is_none() {
                    problems.push(format!(
                        "{rel}: provenance {provenance:?} requires provenance_note (TV-020)"
                    ));
                }
                notes.extend(provenance_note.map(|n| (rel.clone(), n)));
                for c in &list {
                    cases += 1;
                    let at = format!("{rel}#{}", c.id);
                    unique(&c.id, &at, c.superseded.is_some(), &mut problems);
                    check_codec_case(c, &spec, &field_codes, &at, &mut by_links, &mut problems);
                }
            }
            Ok(VectorFile::EngineScenario(file)) => {
                notes.extend(file.provenance_note.clone().map(|n| (rel.clone(), n)));
                for (id, at, superseded) in
                    check_scenario_file(&rel, &file, &spec, &mut by_links, &mut problems)
                {
                    cases += 1;
                    unique(&id, &at, superseded, &mut problems);
                }
            }
        }
    }

    check_released(&repo::root(), &current_raw, &mut problems);

    check_by_links(&ids, &by_links, &mut problems);

    problems.extend(crate::models::check(&notes)?);

    for p in &problems {
        println!("error: {p}");
    }
    println!(
        "vectors validate: {files} file(s), {cases} case(s), {} problem(s)",
        problems.len()
    );
    Ok(problems.len())
}

/// The generated vector directories and the independent model that writes each (TV-023). A new
/// model adds one row.
const GENERATORS: &[(&str, &str)] = &[
    ("test-vectors/cop1", "test-vectors/models/cop1.py"),
    ("test-vectors/link/recv", "test-vectors/models/link_recv.py"),
];

/// The `.json` files under `dir`, as paths relative to it.
fn json_files(dir: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                todo.push(path);
            } else if path.extension().is_some_and(|e| e == "json") {
                let rel = path
                    .strip_prefix(dir)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .into_owned();
                out.insert(
                    rel,
                    std::fs::read_to_string(&path).map_err(|e| e.to_string())?,
                );
            }
        }
    }
    Ok(out)
}

/// Runs `model` with `--write [dir]`.
fn run_generator(model: &str, dir: Option<&Path>) -> Result<(), String> {
    let mut cmd = Command::new("python3");
    cmd.args(["-I", model, "--write"]).current_dir(repo::root());
    if let Some(dir) = dir {
        cmd.arg(dir);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("python3 -I {model}: {e} (the models need python3, TV-023)"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{model} --write failed:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

/// `vectors generated`: the files of the [`GENERATORS`] directories are written by the
/// independent models and never edited by hand (TV-023). With `write` they are regenerated;
/// otherwise each model writes to a scratch directory and any difference from the committed
/// files is a problem.
pub fn generated(write: bool) -> Result<usize, String> {
    let mut problems = 0;
    let mut files = 0;
    for (n, (dir, model)) in GENERATORS.iter().enumerate() {
        if write {
            run_generator(model, None)?;
            println!("wrote {dir}/ with {model}");
            continue;
        }
        let scratch = std::env::temp_dir().join(format!("aldis-gen{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
        let result = run_generator(model, Some(&scratch))
            .and_then(|()| Ok((json_files(&scratch)?, json_files(&repo::root().join(dir))?)));
        let _ = std::fs::remove_dir_all(&scratch);
        let (fresh, committed) = result?;
        for (rel, text) in &fresh {
            if committed.get(rel) != Some(text) {
                println!(
                    "error: {dir}/{rel} differs from what {model} writes; run `cargo xtask vectors generated --write`"
                );
                problems += 1;
            }
        }
        for rel in committed.keys().filter(|r| !fresh.contains_key(*r)) {
            println!("error: {dir}/{rel} is not written by {model}; delete it");
            problems += 1;
        }
        files += fresh.len();
    }
    if !write {
        println!("vectors generated: {files} file(s), {problems} problem(s)");
    }
    Ok(problems)
}

/// Generates the JSON Schemas from the testkit model, or checks the committed ones.
pub fn schema(check: bool) -> Result<usize, String> {
    let dir = repo::root().join(SCHEMA_DIR);
    let generated = [
        (
            "codec.schema.json",
            aldis_testkit::schemars::schema_for!(CodecFile),
        ),
        (
            "engine-scenario.schema.json",
            aldis_testkit::schemars::schema_for!(ScenarioFile),
        ),
        (
            "fields.schema.json",
            aldis_testkit::schemars::schema_for!(FieldCatalogue),
        ),
    ];
    let mut stale = 0;
    for (name, schema) in generated {
        let body = serde_json::to_string_pretty(&schema).map_err(|e| e.to_string())? + "\n";
        let path = dir.join(name);
        if check {
            if std::fs::read_to_string(&path).ok().as_deref() != Some(body.as_str()) {
                println!(
                    "error: {SCHEMA_DIR}/{name} is out of date; run `cargo xtask vectors schema`"
                );
                stale += 1;
            }
        } else {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::write(&path, body).map_err(|e| e.to_string())?;
            println!("wrote {SCHEMA_DIR}/{name}");
        }
    }
    if check && stale == 0 {
        println!("vectors schema: up to date");
    }
    Ok(stale)
}

#[cfg(test)]
mod tests {
    use super::{
        ReleaseFindings, check_against_released, check_by_links, check_field, check_released,
        check_superseded, is_vector_file, release_tags, tag_blobs,
    };
    use aldis_testkit::{Superseded, VectorFile};
    use serde_json::{Value, json};
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// A throwaway git repository in the system temporary directory, removed on drop.
    struct TempRepo(PathBuf);

    impl TempRepo {
        fn new(name: &str) -> TempRepo {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "aldis-xtask-{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let repo = TempRepo(dir);
            // `init -b` needs git 2.28; the nightly big-endian job runs the tests in a `cross`
            // image with an older git.
            repo.git(&["init", "-q"]);
            repo.git(&["symbolic-ref", "HEAD", "refs/heads/main"]);
            repo
        }

        fn path(&self) -> &Path {
            &self.0
        }

        /// Runs git with a fixed identity, ignoring the user's and the system's configuration.
        fn git(&self, args: &[&str]) {
            let status = Command::new("git")
                .args([
                    "-c",
                    "user.name=xtask",
                    "-c",
                    "user.email=xtask@example.invalid",
                ])
                .args(["-c", "commit.gpgSign=false", "-c", "tag.gpgSign=false"])
                .args(args)
                .current_dir(&self.0)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", self.0.join(".git/no-global-config"))
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?}");
        }

        fn write(&self, rel: &str, body: &str) {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }

        fn commit(&self, message: &str) {
            self.git(&["add", "-A"]);
            self.git(&["commit", "-q", "--allow-empty", "-m", message]);
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn case(id: &str) -> Value {
        json!({
            "id": id,
            "requirements": ["CMN-001"],
            "operation": "crc16",
            "input_hex": "3132",
            "expect": { "ok": true }
        })
    }

    fn codec(cases: &[Value]) -> Value {
        json!({
            "kind": "codec",
            "standard": "132.0-B-3",
            "provenance": "blue-book-example",
            "cases": cases
        })
    }

    /// Problems from comparing `current` with `released` as released in `v0.1.0-m0`.
    fn compare(released: &Value, current: &Value) -> Vec<String> {
        let mut problems = Vec::new();
        let mut out = ReleaseFindings {
            problems: &mut problems,
            seen: BTreeSet::new(),
        };
        check_against_released("f.json", "v0.1.0-m0", released, current, &mut out);
        problems
    }

    #[test]
    fn field_codes_need_an_error_and_the_catalogue() {
        // req: API-082, TV-001
        let codes = BTreeMap::from([(4, "packet_version".to_owned())]);
        let expect = |v: Value| -> aldis_testkit::Expect { serde_json::from_value(v).unwrap() };
        let check = |e: &aldis_testkit::Expect| {
            let mut problems = Vec::new();
            check_field(e, &codes, "f#c", &mut problems);
            problems
        };
        let known = expect(json!({ "ok": false, "error": "UnsupportedVersion", "field": 4 }));
        assert_eq!(check(&known), Vec::<String>::new());
        let without = expect(json!({ "ok": false, "error": "UnsupportedVersion" }));
        assert_eq!(check(&without), Vec::<String>::new());
        let unknown = check(&expect(
            json!({ "ok": false, "error": "InvalidField", "field": 9 }),
        ));
        assert_eq!(unknown.len(), 1);
        assert!(
            unknown[0].contains("field code 9 is not in the catalogue"),
            "{unknown:?}"
        );
        let on_success = check(&expect(json!({ "ok": true, "field": 4 })));
        assert_eq!(on_success.len(), 1);
        assert!(
            on_success[0].contains("expect.ok is true"),
            "{on_success:?}"
        );
        // The generated catalogue is not a vector file; the schema directory is not either.
        assert!(!is_vector_file("test-vectors/core/fields.json"));
        assert!(!is_vector_file("test-vectors/schema/codec.schema.json"));
        assert!(is_vector_file("test-vectors/core/errors.json"));
    }

    #[test]
    fn superseded_needs_a_reason_and_by_is_optional() {
        // req: TV-021
        let check = |reason: &str, by: Option<&str>| {
            let mut by_links = Vec::new();
            let mut problems = Vec::new();
            let s = Superseded {
                reason: reason.to_owned(),
                by: by.map(str::to_owned),
            };
            check_superseded(Some(&s), "f.json#old-001", &mut by_links, &mut problems);
            (by_links, problems)
        };
        // A reason and no replacing case: a wrong case with no correct replacement.
        assert_eq!(check("retired", None), (vec![], vec![]));
        // No reason: one problem, with or without `by`.
        assert_eq!(check(" ", None).1.len(), 1);
        assert_eq!(check("", Some("new-002")).1.len(), 1);
        // `by` is queued for the link check.
        assert_eq!(
            check("expanded", Some("new-002")).0,
            [("f.json#old-001".to_owned(), "new-002".to_owned())]
        );
        // A marker without `reason` does not match the model.
        let no_reason = codec(&[case("old-001")]);
        let mut no_reason_text = no_reason;
        no_reason_text["cases"][0]["superseded"] = json!({ "by": "new-002" });
        assert!(VectorFile::from_json(&no_reason_text.to_string()).is_err());
        let mut reason_only = codec(&[case("old-001")]);
        reason_only["cases"][0]["superseded"] = json!({ "reason": "retired" });
        assert!(VectorFile::from_json(&reason_only.to_string()).is_ok());
    }

    #[test]
    fn superseded_by_names_a_live_case() {
        // req: TV-021
        let mut ids = BTreeMap::new();
        ids.insert("old-001".to_owned(), ("f.json#old-001".to_owned(), true));
        ids.insert("new-002".to_owned(), ("g.json#new-002".to_owned(), false));
        let link = |by: &str| {
            let mut problems = Vec::new();
            check_by_links(
                &ids,
                &[("f.json#old-001".to_owned(), by.to_owned())],
                &mut problems,
            );
            problems
        };
        assert_eq!(link("new-002"), Vec::<String>::new());
        assert_eq!(
            link("typo-002"),
            ["f.json#old-001: superseded.by \"typo-002\" does not name an existing case (TV-021)"]
        );
        assert_eq!(
            link("old-001"),
            [
                "f.json#old-001: superseded.by \"old-001\" (f.json#old-001) is itself superseded (TV-021)"
            ]
        );
    }

    #[test]
    fn released_scenario_files_are_append_only() {
        // req: TV-021
        let step = json!({ "at_us": 0, "input": { "x": 1 }, "expect": [] });
        let scenario = |id: &str| json!({ "id": id, "requirements": ["COP-001"], "setup": "S1-aa", "steps": [step] });
        let released = json!({
            "kind": "engine-scenario", "engine": "e", "provenance": "hand-derived",
            "config": {},
            "setups": [{ "name": "S1-aa", "steps": [step] }],
            "scenarios": [scenario("s-001")],
        });
        assert_eq!(compare(&released, &released), Vec::<String>::new());

        // A new scenario and a new setup pass.
        let mut added = released.clone();
        added["scenarios"]
            .as_array_mut()
            .unwrap()
            .push(scenario("s-002"));
        added["setups"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "name": "S1-bb", "steps": [step] }));
        assert_eq!(compare(&released, &added), Vec::<String>::new());

        // Editing a released scenario fails.
        let mut edited = released.clone();
        edited["scenarios"][0]["requirements"] = json!(["COP-002"]);
        assert_eq!(compare(&released, &edited).len(), 1);

        // Editing or removing a released setup fails.
        let mut setup_edited = released.clone();
        setup_edited["setups"][0]["steps"][0]["at_us"] = json!(5);
        assert_eq!(
            compare(&released, &setup_edited),
            ["f.json#setup S1-aa: setup released in v0.1.0-m0 was modified in place (TV-021)"]
        );
        let mut setup_removed = released.clone();
        setup_removed["setups"] = json!([]);
        assert_eq!(compare(&released, &setup_removed).len(), 1);

        // File-level metadata still may not change.
        let mut meta = released.clone();
        meta["config"] = json!({ "a": 1 });
        assert_eq!(compare(&released, &meta).len(), 1);
    }

    #[test]
    fn released_cases_change_only_by_gaining_superseded() {
        // req: TV-021
        let released = codec(&[case("c-001"), case("c-002")]);
        assert_eq!(compare(&released, &released), Vec::<String>::new());

        let mut superseded = released.clone();
        superseded["cases"][0]["superseded"] = json!({ "reason": "expanded", "by": "c-003" });
        assert_eq!(compare(&released, &superseded), Vec::<String>::new());

        let mut retired = released.clone();
        retired["cases"][0]["superseded"] = json!({ "reason": "wrong, no replacement" });
        assert_eq!(compare(&released, &retired), Vec::<String>::new());

        // The marker of a case superseded in a release is fixed too.
        let mut new_marker = superseded.clone();
        new_marker["cases"][0]["superseded"] = json!({ "reason": "other" });
        assert_eq!(compare(&superseded, &new_marker).len(), 1);

        let mut edited = superseded.clone();
        edited["cases"][0]["requirements"] = json!(["CMN-001", "CMN-002"]);
        assert_eq!(
            compare(&released, &edited),
            ["f.json#c-001: released vector in v0.1.0-m0 was modified in place (TV-021)"]
        );

        let mut added_key = released.clone();
        added_key["cases"][1]["description"] = json!("new");
        assert_eq!(compare(&released, &added_key).len(), 1);

        let mut dropped_key = released.clone();
        dropped_key["cases"][1]
            .as_object_mut()
            .unwrap()
            .remove("input_hex");
        assert_eq!(compare(&released, &dropped_key).len(), 1);

        let removed = codec(&[case("c-002")]);
        assert_eq!(
            compare(&released, &removed),
            ["f.json#c-001: case released in v0.1.0-m0 was removed (TV-021)"]
        );

        let mut meta = released.clone();
        meta["standard"] = json!("bogus");
        assert_eq!(
            compare(&released, &meta),
            ["f.json: file-level metadata changed since v0.1.0-m0 (TV-021)"]
        );
        let mut meta_added = released.clone();
        meta_added["provenance_note"] = json!("new");
        assert_eq!(compare(&released, &meta_added).len(), 1);

        assert_eq!(compare(&released, &json!([])).len(), 1);
    }

    #[test]
    fn release_tags_are_v_digit_tags_reachable_from_head() {
        // req: TV-021
        let repo = TempRepo::new("tags");
        repo.commit("before any release");
        assert!(release_tags(repo.path()).is_err(), "no tags at all");

        repo.git(&["tag", "vendor-snapshot"]);
        assert!(
            release_tags(repo.path()).is_err(),
            "`vendor-snapshot` is not a release tag"
        );

        repo.commit("m0");
        repo.git(&["tag", "v0.1.0"]);
        repo.git(&["tag", "very-old"]);
        repo.commit("m1");
        repo.git(&["tag", "v0.2.0"]);
        repo.git(&["checkout", "-q", "-b", "maint", "v0.1.0"]);
        repo.commit("fix");
        repo.git(&["tag", "v0.1.1"]);

        assert_eq!(release_tags(repo.path()).unwrap(), ["v0.1.0", "v0.1.1"]);
        repo.git(&["checkout", "-q", "main"]);
        assert_eq!(release_tags(repo.path()).unwrap(), ["v0.1.0", "v0.2.0"]);
        repo.git(&["checkout", "-q", "main~2"]);
        assert_eq!(release_tags(repo.path()).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn tag_blobs_reads_every_vector_file() {
        // req: TV-021
        let repo = TempRepo::new("blobs");
        let valid = codec(&[case("a-001")]);
        for rel in [
            "test-vectors/a.json",
            "test-vectors/sub/\u{fc}n\u{ef}code.json",
            "test-vectors/with space.json",
        ] {
            repo.write(rel, &valid.to_string());
        }
        repo.write("test-vectors/bad.json", "{ not json");
        repo.write("test-vectors/schema/codec.schema.json", "{}");
        repo.write("test-vectors/README.md", "not a vector");
        repo.write("test-vectors/UPPER.JSON", "{}");
        repo.write("other/x.json", "{}");
        repo.commit("vectors");
        repo.git(&["tag", "v0.1.0"]);

        let blobs: BTreeMap<String, Result<Value, String>> = tag_blobs(repo.path(), "v0.1.0")
            .unwrap()
            .into_iter()
            .collect();
        assert_eq!(
            blobs.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "test-vectors/a.json",
                "test-vectors/bad.json",
                "test-vectors/sub/\u{fc}n\u{ef}code.json",
                "test-vectors/with space.json",
            ]
        );
        assert_eq!(blobs["test-vectors/a.json"], Ok(valid.clone()));
        assert_eq!(blobs["test-vectors/sub/\u{fc}n\u{ef}code.json"], Ok(valid));
        assert!(
            blobs["test-vectors/bad.json"]
                .as_ref()
                .is_err_and(|e| e.starts_with("invalid JSON"))
        );
        assert!(tag_blobs(repo.path(), "v9.9.9").is_err());
    }

    #[test]
    fn tag_blobs_does_not_deadlock_on_many_files() {
        // req: TV-021
        // Enough requests and responses to fill both pipes many times over; writing every
        // request before reading hung here.
        const FILES: usize = 4000;
        let repo = TempRepo::new("many");
        let body = codec(&[case("x")]).to_string();
        for i in 0..FILES {
            repo.write(&format!("test-vectors/many/{i:04}.json"), &body);
        }
        repo.commit("many vectors");
        repo.git(&["tag", "v0.1.0"]);

        let root = repo.path().to_path_buf();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(tag_blobs(&root, "v0.1.0"));
        });
        let blobs = rx
            .recv_timeout(Duration::from_secs(120))
            .expect("tag_blobs deadlocked")
            .unwrap();
        assert_eq!(blobs.len(), FILES);
        assert!(blobs.iter().all(|(_, v)| v.is_ok()));
    }

    #[test]
    fn check_released_reports_every_problem_once() {
        // req: TV-021
        let repo = TempRepo::new("released");
        let f = codec(&[case("f-001"), case("f-002")]);
        let g = codec(&[case("g-001")]);
        let h = codec(&[case("h-001")]);
        repo.write("test-vectors/f.json", &f.to_string());
        repo.write("test-vectors/g.json", &g.to_string());
        repo.write("test-vectors/h.json", &h.to_string());
        repo.commit("m0");
        repo.git(&["tag", "v0.1.0"]);
        repo.write("test-vectors/bad.json", "{ not json");
        repo.commit("m1");
        repo.git(&["tag", "v0.2.0"]);
        // A newer release on another branch is not reachable from main.
        repo.git(&["checkout", "-q", "-b", "next"]);
        repo.write("test-vectors/next.json", &g.to_string());
        repo.commit("m2");
        repo.git(&["tag", "v0.3.0"]);
        repo.git(&["checkout", "-q", "main"]);
        // A stray tag whose name starts with `v` is not a release.
        repo.write("test-vectors/vendor.json", "{ not json");
        repo.commit("vendor drop");
        repo.git(&["tag", "vendor-snapshot"]);

        let mut f_now = f;
        f_now["cases"][0]["requirements"] = json!(["CMN-002"]);
        f_now["cases"].as_array_mut().unwrap().pop();
        let current: BTreeMap<String, Option<Value>> = [
            ("test-vectors/f.json", Some(f_now)),
            // g.json removed; h.json present but not valid JSON (reported by the parse).
            ("test-vectors/h.json", None),
            ("test-vectors/bad.json", Some(g)),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();

        let mut problems = Vec::new();
        check_released(repo.path(), &current, &mut problems);
        problems.sort();
        assert_eq!(
            problems,
            [
                "test-vectors/f.json#f-001: released vector in v0.1.0 was modified in place (TV-021)",
                "test-vectors/f.json#f-002: case released in v0.1.0 was removed (TV-021)",
                "test-vectors/g.json: vector file released in v0.1.0 was removed or renamed (TV-021)",
                "v0.2.0:test-vectors/bad.json: invalid JSON: key must be a string at line 1 column 3 (TV-021)",
            ]
        );
    }
}
