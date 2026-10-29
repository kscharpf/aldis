//! The parts of a hosted verification run and their assembly (VER-067).
//!
//! The hosted run (VER-060) makes the checks in one job per [`Group`]. Each job runs
//! `verify part <group>`, which writes a part record: the record of a run that selected the
//! group's checks with `--only`, so that it never passes the merge gate by itself. A last job
//! runs `verify assemble` on the part records. [`assemble`] writes the one record of the run
//! only when the parts fit together: one commit, one tree, one scope, one image, every group
//! exactly once. The record is then judged and published as the record of any other run.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::checks::{Check, Group};
use super::record;

/// The gating checks of `group` that did not pass in the part record `record`, each as
/// `name: result`. A skipped or reused result counts as passed here; the merge gate re-derives
/// it (VER-058).
pub fn failures(record: &Value, group: Group, all: &[Check]) -> Vec<String> {
    let entries = record.get("checks").and_then(Value::as_array);
    all.iter()
        .filter(|c| c.group == Some(group) && c.gating)
        .filter_map(|c| {
            let result = entries
                .into_iter()
                .flatten()
                .find(|e| e.get("name").and_then(Value::as_str) == Some(c.name.as_str()))
                .and_then(|e| e.get("result"))
                .and_then(Value::as_str)
                .unwrap_or("missing");
            (!matches!(result, "pass" | "skipped" | "reused"))
                .then(|| format!("{}: {result}", c.name))
        })
        .collect()
}

/// The part records, by group.
type ByGroup<'a> = Vec<(Group, &'a Value)>;

/// The value at `pointer` in every part, which must be the same in all; returns it.
fn same(parts: &ByGroup<'_>, pointer: &str, what: &str, problems: &mut Vec<String>) -> Value {
    let at = |p: &Value| p.pointer(pointer).cloned().unwrap_or(Value::Null);
    let first = parts.first().map_or(Value::Null, |(_, p)| at(p));
    for (group, part) in parts {
        if at(part) != first {
            problems.push(format!("{what} differs in group {}", group.name()));
        }
    }
    first
}

/// The value at `pointer` in every part that has one (not null), which must be the same in all;
/// returns it, or null when no part has one. A part that started no container has none of the
/// image's, the host's container, or the packages' fields (VER-057).
fn agreed(parts: &ByGroup<'_>, pointer: &str, what: &str, problems: &mut Vec<String>) -> Value {
    let mut found: Option<(&Value, Group)> = None;
    for (group, part) in parts {
        let Some(v) = part.pointer(pointer).filter(|v| !v.is_null()) else {
            continue;
        };
        match found {
            None => found = Some((v, *group)),
            Some((first, from)) if first != v => problems.push(format!(
                "{what} differs between groups {} and {}",
                from.name(),
                group.name()
            )),
            Some(_) => {}
        }
    }
    found.map_or(Value::Null, |(v, _)| v.clone())
}

/// The tool versions of all parts, which must not contradict each other.
fn tools(parts: &ByGroup<'_>, problems: &mut Vec<String>) -> Value {
    let mut out: Map<String, Value> = Map::new();
    for (group, part) in parts {
        let Some(map) = part.get("tools").and_then(Value::as_object) else {
            continue;
        };
        for (name, version) in map {
            match out.get(name) {
                Some(seen) if seen != version => problems.push(format!(
                    "the version of tool {name} in group {} differs from another group's",
                    group.name()
                )),
                Some(_) => {}
                None => {
                    out.insert(name.clone(), version.clone());
                }
            }
        }
    }
    Value::Object(out)
}

/// The earliest or latest of the `field` timestamps (RFC 3339, whole seconds, UTC: they sort as
/// text) of the parts.
fn bound(parts: &ByGroup<'_>, field: &str, latest: bool, problems: &mut Vec<String>) -> Value {
    let stamps: Vec<&str> = parts
        .iter()
        .filter_map(|(_, p)| p.get(field).and_then(Value::as_str))
        .collect();
    if stamps.len() != parts.len() {
        problems.push(format!("a part has no `{field}`"));
    }
    let pick = if latest {
        stamps.iter().max()
    } else {
        stamps.iter().min()
    };
    pick.map_or(Value::Null, |s| json!(s))
}

/// The one record of a hosted run from its part records (VER-067), in the format of every other
/// record (VER-057), with the start and end of each part under `host.parts`.
///
/// Fails, listing every problem, unless the parts fit together: each has the format of this
/// tool, the same commit, tree, source, scope, host, image inputs, and image where one was used,
/// tools that agree, and there is exactly one for each [`Group`]. Each check's entry is taken
/// from the part of its group, so that it is the entry of the run that made the check; a check in
/// no group (`test-macos`, `kani`) is not run by any part, and its entry is the host part's.
/// Whether the checks passed is for the merge gate (`record::gate_problems`), as for any record.
pub fn assemble(parts: &[Value], all: &[Check]) -> Result<Value, Vec<String>> {
    let by_group = by_group(parts)?;
    let mut problems = Vec::new();
    let ordered: ByGroup<'_> = Group::ALL
        .iter()
        .filter_map(|g| by_group.get(g).map(|p| (*g, *p)))
        .collect();
    let mut same = |pointer: &str, what: &str| same(&ordered, pointer, what, &mut problems);
    let commit = same("/commit", "the commit");
    let tree = same("/tree", "the tree");
    let source = same("/source", "the source of the run");
    let scope = same("/scope", "the scope of the input rule");
    let os = same("/host/os", "the host's operating system");
    let arch = same("/host/arch", "the host's architecture");
    same("/image/inputs", "the hash of the image inputs");
    let mut agreed = |pointer: &str, what: &str| agreed(&ordered, pointer, what, &mut problems);
    let container_arch = agreed("/host/container_arch", "the container's architecture");
    let engine = agreed("/host/engine", "the container engine");
    let packages = agreed("/packages", "the installed packages");
    agreed("/image/id", "the image ID");
    agreed("/image/tag", "the image tag");
    let tools = tools(&ordered, &mut problems);
    let started = bound(&ordered, "started", false, &mut problems);
    let finished = bound(&ordered, "finished", true, &mut problems);
    let checks = entries(&by_group, all, &mut problems);
    if !problems.is_empty() {
        return Err(problems);
    }
    let times = |g: &Group, p: &Value| {
        json!({
            "group": g.name(),
            "started": p.get("started"),
            "finished": p.get("finished"),
        })
    };
    Ok(json!({
        "format": record::FORMAT,
        "commit": commit,
        "tree": tree,
        "started": started,
        "finished": finished,
        "source": source,
        "host": {
            "os": os,
            "arch": arch,
            "container_arch": container_arch,
            "engine": engine,
            "parts": ordered.iter().map(|(g, p)| times(g, p)).collect::<Vec<_>>(),
        },
        "image": image(&ordered),
        "tools": tools,
        "packages": packages,
        "scope": scope,
        "checks": checks,
    }))
}

/// The part records by group; fails unless there is exactly one part of each group, in the
/// format of this tool.
fn by_group(parts: &[Value]) -> Result<BTreeMap<Group, &Value>, Vec<String>> {
    let mut problems = Vec::new();
    let mut by_group: BTreeMap<Group, &Value> = BTreeMap::new();
    for part in parts {
        let group = part
            .pointer("/part/group")
            .and_then(Value::as_str)
            .and_then(Group::parse);
        let Some(group) = group else {
            problems.push("a record that is not the part of a group".to_owned());
            continue;
        };
        if part.get("format").and_then(Value::as_str) != Some(record::FORMAT) {
            problems.push(format!(
                "the record of group {} is not a `{}` record",
                group.name(),
                record::FORMAT
            ));
        }
        if by_group.insert(group, part).is_some() {
            problems.push(format!("two records for group {}", group.name()));
        }
    }
    for group in Group::ALL {
        if !by_group.contains_key(&group) {
            problems.push(format!("no record for group {}", group.name()));
        }
    }
    if problems.is_empty() {
        Ok(by_group)
    } else {
        Err(problems)
    }
}

/// The image a part used, else the one the parts that used none all describe. Taking it from one
/// part is sound because [`assemble`] has required the parts that used an image to agree on its
/// ID and tag, and an ID names the image's content whichever way (`source`) it was obtained.
fn image(parts: &ByGroup<'_>) -> Value {
    parts
        .iter()
        .map(|(_, p)| p)
        .find(|p| p.pointer("/image/id").is_some_and(|id| !id.is_null()))
        .or(parts.first().map(|(_, p)| p))
        .and_then(|p| p.get("image"))
        .cloned()
        .unwrap_or(Value::Null)
}

/// The entry of every check, in table order, each from the part of its group; a check in no
/// group is run by no part, and its entry is the host part's.
fn entries(
    by_group: &BTreeMap<Group, &Value>,
    all: &[Check],
    problems: &mut Vec<String>,
) -> Vec<Value> {
    let mut out = Vec::new();
    for c in all {
        let owner = by_group.get(&c.group.unwrap_or(Group::Host));
        let entry = owner
            .and_then(|p| p.get("checks"))
            .and_then(Value::as_array)
            .and_then(|e| e.iter().find(|e| e["name"] == c.name.as_str()));
        match entry {
            Some(e) => out.push(e.clone()),
            None => problems.push(format!("{}: not in the record of its group", c.name)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::checks;
    use super::*;

    /// The part record of `group` in a run of tree `t` whose image has ID `image`: its checks
    /// pass, and every other check is not run.
    fn part(group: Group, image: &str) -> Value {
        let entries: Vec<Value> = checks::all()
            .iter()
            .map(|c| {
                let mine = c.group == Some(group);
                json!({
                    "name": c.name,
                    "host": c.host.name(),
                    "gating": c.gating,
                    "script": c.script,
                    "result": if mine { "pass" } else { "not-run" },
                    "reason": if mine { Value::Null } else { json!("not selected with --only") },
                    "duration_ms": 1000,
                })
            })
            .collect();
        json!({
            "format": record::FORMAT,
            "part": {"group": group.name()},
            "commit": "c0ffee",
            "tree": "7ee",
            "started": format!("2026-10-29T17:1{}:00Z", group as u8),
            "finished": format!("2026-10-29T17:2{}:00Z", group as u8),
            "source": {"kind": "github-actions", "run": "https://x/runs/1/attempts/1"},
            "host": {"os": "linux", "arch": "x86_64", "container_arch": "x86_64", "engine": "docker 28"},
            "image": {"tag": "t", "id": image, "inputs": "abc", "source": {"kind": "registry"}},
            "tools": {"rustc": "1.90", "gcc": "14"},
            "packages": ["bash 5"],
            "scope": {"base": "b", "checks": {}},
            "checks": entries,
        })
    }

    fn parts() -> Vec<Value> {
        Group::ALL.iter().map(|g| part(*g, "sha256:1")).collect()
    }

    fn problems(parts: &[Value]) -> Vec<String> {
        assemble(parts, &checks::all()).unwrap_err()
    }

    // req: VER-067
    #[test]
    fn four_parts_make_one_record_that_passes_the_gate() {
        let ps = parts();
        let rec = assemble(&ps, &checks::all()).unwrap();
        assert_eq!(rec["format"], record::FORMAT);
        assert!(rec.get("part").is_none());
        let names: Vec<&str> = rec["checks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["name"].as_str().unwrap())
            .collect();
        let table: Vec<String> = checks::all().into_iter().map(|c| c.name).collect();
        assert_eq!(names, table.iter().map(String::as_str).collect::<Vec<_>>());
        // Every grouped check is the entry of its own part: it passed there.
        for e in rec["checks"].as_array().unwrap() {
            let c = checks::all()
                .into_iter()
                .find(|c| e["name"] == c.name.as_str())
                .unwrap();
            let expect = if c.group.is_some() { "pass" } else { "not-run" };
            assert_eq!(e["result"], expect, "{}", c.name);
        }
        assert_eq!(rec["started"], "2026-10-29T17:10:00Z");
        assert_eq!(rec["finished"], "2026-10-29T17:23:00Z");
        let groups: Vec<&str> = rec["host"]["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["group"].as_str().unwrap())
            .collect();
        assert_eq!(groups, ["host", "cross", "coverage", "embedded"]);
        assert_eq!(record::gate_problems(&rec), Vec::<String>::new());
    }

    // req: VER-067
    #[test]
    fn no_part_passes_the_gate_by_itself() {
        // The design rests on this: a part runs `--only` its group, so every other portable
        // check is not run in it, and the gate refuses it.
        for p in parts() {
            let problems = record::gate_problems(&p);
            assert!(
                problems
                    .iter()
                    .any(|m| m.ends_with("portable check not run")),
                "{problems:?}"
            );
        }
    }

    // req: VER-067
    #[test]
    fn a_failed_check_of_a_part_fails_the_gate_of_the_record() {
        let mut ps = parts();
        for e in ps[1]["checks"].as_array_mut().unwrap() {
            if e["name"] == "big-endian" {
                e["result"] = json!("fail");
            }
        }
        assert_eq!(
            failures(&ps[1], Group::Cross, &checks::all()),
            ["big-endian: fail"]
        );
        let rec = assemble(&ps, &checks::all()).unwrap();
        assert_eq!(record::gate_problems(&rec), ["big-endian: fail"]);
    }

    // req: VER-067
    #[test]
    fn a_missing_or_doubled_group_is_refused() {
        let mut ps = parts();
        ps.remove(2);
        assert_eq!(problems(&ps), ["no record for group coverage"]);
        let mut ps = parts();
        ps.push(part(Group::Host, "sha256:1"));
        assert_eq!(problems(&ps), ["two records for group host"]);
    }

    // req: VER-067
    #[test]
    fn a_record_that_is_no_part_is_refused() {
        let mut ps = parts();
        ps[0].as_object_mut().unwrap().remove("part");
        let p = problems(&ps);
        assert!(
            p.contains(&"a record that is not the part of a group".to_owned()),
            "{p:?}"
        );
    }

    // req: VER-067
    #[test]
    fn parts_of_different_commits_trees_scopes_or_runs_are_refused() {
        for (pointer, what) in [
            ("commit", "the commit"),
            ("tree", "the tree"),
            ("scope", "the scope of the input rule"),
            ("source", "the source of the run"),
        ] {
            let mut ps = parts();
            ps[3][pointer] = json!("other");
            assert_eq!(problems(&ps), [format!("{what} differs in group embedded")]);
        }
    }

    // req: VER-067
    #[test]
    fn parts_made_with_different_images_or_tools_are_refused() {
        let mut ps = parts();
        ps[1]["image"]["id"] = json!("sha256:2");
        assert!(
            problems(&ps)
                .iter()
                .any(|p| p.starts_with("the image ID differs"))
        );
        let mut ps = parts();
        ps[1]["image"]["inputs"] = json!("def");
        assert!(
            problems(&ps)
                .iter()
                .any(|p| p.starts_with("the hash of the image inputs"))
        );
        let mut ps = parts();
        ps[2]["tools"]["gcc"] = json!("13");
        assert!(problems(&ps).iter().any(|p| p.contains("tool gcc")));
    }

    // req: VER-067
    #[test]
    fn a_part_that_started_no_container_takes_the_image_of_the_others() {
        let mut ps = parts();
        ps[0]["image"] =
            json!({"tag": null, "id": null, "inputs": "abc", "source": {"kind": "none"}});
        ps[0]["host"]["container_arch"] = Value::Null;
        ps[0]["host"]["engine"] = Value::Null;
        ps[0]["packages"] = Value::Null;
        ps[0]["tools"] = json!({});
        let rec = assemble(&ps, &checks::all()).unwrap();
        assert_eq!(rec["image"]["id"], "sha256:1");
        assert_eq!(rec["host"]["engine"], "docker 28");
        assert_eq!(rec["packages"], json!(["bash 5"]));
    }

    // req: VER-067
    #[test]
    fn a_part_without_the_entry_of_its_check_is_refused() {
        let mut ps = parts();
        ps[2]["checks"]
            .as_array_mut()
            .unwrap()
            .retain(|e| e["name"] != "coverage");
        assert_eq!(problems(&ps), ["coverage: not in the record of its group"]);
    }

    // req: VER-067
    #[test]
    fn a_check_passed_in_another_groups_part_is_not_taken() {
        // `big-endian` passes in the host part, which does not own it: the record takes the
        // cross part's entry, which is not run, so the gate refuses the record.
        let mut ps = parts();
        for e in ps[0]["checks"].as_array_mut().unwrap() {
            if e["name"] == "big-endian" {
                e["result"] = json!("pass");
            }
        }
        for e in ps[1]["checks"].as_array_mut().unwrap() {
            if e["name"] == "big-endian" {
                e["result"] = json!("not-run");
            }
        }
        let rec = assemble(&ps, &checks::all()).unwrap();
        assert_eq!(
            record::gate_problems(&rec),
            ["big-endian: portable check not run"]
        );
    }
}
