//! `xtask cpp layers`: the `CMake` target graph against the layer rules (CPP-010).
//!
//! Every C++ module is its own `CMake` library target, `aldis_<module>` (cpp.md §2), plus the
//! object library `aldis_<module>_headers` that compiles its headers on their own (D7). This
//! check reads the direct link dependencies from `cmake --graphviz` and applies the crate rules
//! of `xtask layers` ([`crate::layers`]): a module links only modules of lower layers, with the
//! GEN-002 exceptions, never a boundary module, and in Layers 0–2 no third-party target
//! (GEN-001, CPP-003). Executables (tests, tools, the exerciser) are not checked.

use std::collections::BTreeMap;
use std::process::Command;

use crate::layers::{BOUNDARY, LAYERS, SAME_LAYER_ALLOWED};
use crate::repo;

/// C++ library targets named `aldis_*` that are tooling (vector loader, test support): never
/// linked by a module. Executables need no entry.
const TOOLING: &[&str] = &[];

/// What a graph node is, from its `Graphviz` shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Executable,
    Library,
    Other,
}

/// The parsed target graph: node id → (target name, kind), and direct edges by node id.
#[derive(Debug, Default)]
struct Graph {
    nodes: BTreeMap<String, (String, Kind)>,
    edges: Vec<(String, String)>,
}

/// First double-quoted string in `s`.
fn quoted(s: &str) -> Option<&str> {
    let start = s.find('"')? + 1;
    let len = s.get(start..)?.find('"')?;
    s.get(start..start + len)
}

/// Parses the main graph written by `cmake --graphviz` (legend nodes are skipped).
fn parse_dot(text: &str) -> Graph {
    let mut g = Graph::default();
    for line in text.lines().map(str::trim) {
        if !line.starts_with("\"node") {
            continue;
        }
        if let Some((from, rest)) = line.split_once(" -> ") {
            if let (Some(a), Some(b)) = (quoted(from), quoted(rest)) {
                g.edges.push((a.to_owned(), b.to_owned()));
            }
        } else if let (Some(id), Some((_, attrs))) = (quoted(line), line.split_once('[')) {
            // `CMake` labels an alias as "name\n(alias)"; keep the target name.
            let label = attrs
                .split_once("label = ")
                .and_then(|(_, l)| quoted(l))
                .map(|l| l.split("\\n").next().unwrap_or(l).to_owned())
                .unwrap_or_default();
            let shape = attrs
                .split_once("shape = ")
                .map(|(_, s)| s.trim_start_matches('"'))
                .and_then(|s| s.split(|c: char| !c.is_ascii_alphanumeric()).next())
                .unwrap_or_default();
            let kind = match shape {
                "egg" => Kind::Executable,
                "octagon" | "doubleoctagon" | "tripleoctagon" | "pentagon" | "hexagon" => {
                    Kind::Library
                }
                _ => Kind::Other,
            };
            g.nodes.insert(id.to_owned(), (label, kind));
        }
    }
    g
}

/// The crate a C++ library target belongs to, e.g. `aldis_core_headers` → `aldis-core`;
/// `None` for targets that are not Aldis modules.
fn crate_of(target: &str) -> Option<String> {
    let module = target.strip_prefix("aldis_")?;
    let module = module.strip_suffix("_headers").unwrap_or(module);
    Some(if module == "capi" {
        "aldis-ffi".to_owned() // the C ABI module (CPP-035)
    } else {
        format!("aldis-{}", module.replace('_', "-"))
    })
}

/// Violations of the layer rules in `g`.
fn violations(g: &Graph) -> Vec<String> {
    let layer: BTreeMap<&str, u8> = LAYERS.iter().copied().collect();
    let module = |name: &str| -> Option<String> {
        if TOOLING.contains(&name) {
            None
        } else {
            crate_of(name)
        }
    };
    let mut errors = Vec::new();

    for (name, kind) in g.nodes.values() {
        if *kind != Kind::Library {
            continue;
        }
        if let Some(krate) = module(name)
            && !layer.contains_key(krate.as_str())
            && !BOUNDARY.contains(&krate.as_str())
        {
            errors.push(format!(
                "CPP-010 {name}: no crate {krate} in the layer table of xtask/src/layers.rs \
                 (name a module after its crate, or list a tooling library in xtask/src/cpp/layers.rs)"
            ));
        }
    }

    for (from_id, to_id) in &g.edges {
        let (Some((from, from_kind)), Some((to, _))) = (g.nodes.get(from_id), g.nodes.get(to_id))
        else {
            continue;
        };
        if *from_kind != Kind::Library {
            continue;
        }
        let Some(from_crate) = module(from) else {
            continue;
        };
        let to_crate = module(to);
        if BOUNDARY.contains(&from_crate.as_str()) {
            if to_crate.is_none() && to.starts_with("aldis_") {
                errors.push(format!("CPP-010 {from} links tooling target {to}"));
            }
            continue;
        }
        let Some(&l) = layer.get(from_crate.as_str()) else {
            continue; // reported above
        };
        match to_crate {
            Some(to_crate) if to_crate == from_crate => {} // a module's header checks
            Some(to_crate) if BOUNDARY.contains(&to_crate.as_str()) => {
                errors.push(format!("CPP-010 GEN-002 {from} links boundary target {to}"));
            }
            Some(to_crate) => {
                if let Some(&dl) = layer.get(to_crate.as_str())
                    && (dl > l
                        || (dl == l
                            && !SAME_LAYER_ALLOWED
                                .contains(&(from_crate.as_str(), to_crate.as_str()))))
                {
                    errors.push(format!("CPP-010 GEN-002 {from} (L{l}) links {to} (L{dl})"));
                }
            }
            None if l <= 2 => {
                errors.push(format!(
                    "CPP-010 GEN-001 {from} (L{l}) links non-module target {to}"
                ));
            }
            None => {}
        }
    }
    errors
}

/// Checks the target graph of the configured build directory `build_dir`.
pub fn check(build_dir: &str) -> Result<usize, String> {
    let root = repo::root();
    let build = root.join(build_dir);
    if !build.join("CMakeCache.txt").exists() {
        return Err(format!(
            "{build_dir} is not a configured CMake build directory \
             (run e.g. `cmake --preset host-gcc` in cpp/ first)"
        ));
    }
    let out_dir = root.join("target/cpp/graphviz");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let dot = out_dir.join("targets.dot");
    let status = Command::new("cmake")
        .arg(format!("--graphviz={}", dot.display()))
        .arg(&build)
        .current_dir(&root)
        .output()
        .map_err(|e| format!("cmake: {e}"))?;
    if !status.status.success() {
        return Err(format!(
            "cmake --graphviz failed:\n{}",
            String::from_utf8_lossy(&status.stderr)
        ));
    }
    let graph = parse_dot(&repo::read(&dot)?);
    let errors = violations(&graph);
    for e in &errors {
        println!("error: {e}");
    }
    let modules = graph
        .nodes
        .values()
        .filter(|(n, k)| *k == Kind::Library && crate_of(n).is_some())
        .count();
    println!(
        "cpp layers: {} target(s), {modules} module target(s), {} violation(s)",
        graph.nodes.len(),
        errors.len()
    );
    if modules == 0 {
        return Err("no aldis_* library target found in the graph".to_owned());
    }
    Ok(errors.len())
}

#[cfg(test)]
mod tests {
    use super::{Kind, crate_of, parse_dot, violations};

    /// The shape of `cmake --graphviz` output (`CMake` 3.28–4.x), legend included.
    const DOT: &str = r#"digraph "aldis" {
node [
  fontsize = "12"
];
subgraph clusterLegend {
  label = "Legend";
  color = black;
  edge [ style = invis ];
  legendNode0 [ label = "Executable", shape = egg ];
  legendNode1 [ label = "Static Library", shape = octagon ];
}
    "node0" [ label = "aldis_core\n(aldis::core)", shape = pentagon ];
    "node1" [ label = "aldis_core_headers", shape = hexagon ];
    "node1" -> "node0" [ style = dotted ] // aldis_core_headers -> aldis_core
    "node2" [ label = "aldis_core_tests", shape = egg ];
    "node2" -> "node0" [ style = dotted ] // aldis_core_tests -> aldis_core
    "node3" [ label = "gtest_main", shape = octagon ];
    "node2" -> "node3" [ style = dotted ] // aldis_core_tests -> gtest_main
}
"#;

    #[test]
    fn parses_cmake_graphviz() {
        // req: CPP-010
        let g = parse_dot(DOT);
        assert_eq!(g.nodes.len(), 4);
        assert_eq!(g.nodes["node0"], ("aldis_core".to_owned(), Kind::Library));
        assert_eq!(g.nodes["node2"].1, Kind::Executable);
        assert_eq!(g.edges.len(), 3);
        assert_eq!(violations(&g), Vec::<String>::new());
        assert_eq!(
            crate_of("aldis_core_headers").as_deref(),
            Some("aldis-core")
        );
        assert_eq!(crate_of("aldis_capi").as_deref(), Some("aldis-ffi"));
        assert_eq!(crate_of("gtest"), None);
    }

    fn graph(extra: &str) -> super::Graph {
        parse_dot(&format!(
            "{DOT}\n\"node10\" [ label = \"aldis_time\", shape = pentagon ];\n\
             \"node11\" [ label = \"aldis_spp\", shape = pentagon ];\n\
             \"node12\" [ label = \"aldis_tm\", shape = pentagon ];\n\
             \"node13\" [ label = \"aldis_link\", shape = pentagon ];\n\
             \"node14\" [ label = \"aldis_capi\", shape = octagon ];\n{extra}"
        ))
    }

    #[test]
    fn allowed_edges_pass() {
        // req: CPP-010, GEN-002
        let g = graph(
            "\"node10\" -> \"node0\"\n\"node11\" -> \"node10\"\n\"node13\" -> \"node12\"\n\
             \"node14\" -> \"node13\"\n\"node14\" -> \"node0\"",
        );
        assert_eq!(violations(&g), Vec::<String>::new());
    }

    #[test]
    fn rejected_edges() {
        // req: CPP-010, GEN-001, GEN-002
        for (edge, expected) in [
            (
                "\"node0\" -> \"node10\"",
                "aldis_core (L0) links aldis_time (L1)",
            ),
            (
                "\"node12\" -> \"node11\"",
                "aldis_tm (L1) links aldis_spp (L1)",
            ),
            (
                "\"node10\" -> \"node11\"",
                "aldis_time (L1) links aldis_spp (L1)",
            ),
            (
                "\"node12\" -> \"node14\"",
                "aldis_tm links boundary target aldis_capi",
            ),
            (
                "\"node1\" -> \"node3\"",
                "aldis_core_headers (L0) links non-module target gtest_main",
            ),
        ] {
            let errors = violations(&graph(edge));
            assert_eq!(errors.len(), 1, "{edge}: {errors:?}");
            assert!(errors[0].contains(expected), "{edge}: {errors:?}");
        }
        let unknown = parse_dot("\"node0\" [ label = \"aldis_widgets\", shape = pentagon ];");
        let errors = violations(&unknown);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("no crate aldis-widgets"), "{errors:?}");
    }
}
