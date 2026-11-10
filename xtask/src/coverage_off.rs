//! `xtask coverage-off`: every test module of a coverage-measured crate carries
//! `#[cfg_attr(coverage_nightly, coverage(off))]` (VER-024), so that the gate measures library
//! code only ([`crate::coverage`]).
//!
//! The crates are those the coverage gate measures: [`crate::layers::gated`] and
//! [`crate::coverage::REPORTED`]. In each file under `src/`, a module (`mod x { … }` or
//! `mod x;`) whose `#[cfg(…)]` names `test` as a word (`test`, `all(test, …)`, `any(test, …)`;
//! not `doctest`) needs the attribute among the attributes of that module. The scanner is
//! textual and reads one attribute per line, as `rustfmt` writes them.

use std::fmt::Write as _;

use crate::{coverage, layers, repo};

/// The attribute every test module carries.
pub const ATTRIBUTE: &str = "#[cfg_attr(coverage_nightly, coverage(off))]";

/// Whether the attribute line `line` is a `cfg` that names `test` as a word.
fn cfg_test(line: &str) -> bool {
    let Some(body) = line.strip_prefix("#[cfg(") else {
        return false;
    };
    body.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|w| w == "test")
}

/// Whether `line` declares a module, with or without a body.
fn is_mod(line: &str) -> bool {
    let rest = line.strip_prefix("pub").map_or(line, |r| {
        let r = r.trim_start();
        if r.starts_with('(') {
            r.split_once(')').map_or(r, |(_, t)| t.trim_start())
        } else {
            r
        }
    });
    rest.strip_prefix("mod ").is_some()
}

/// The 1-based lines of the test modules of `text` that lack [`ATTRIBUTE`].
fn missing(text: &str) -> Vec<usize> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !cfg_test(line) {
            continue;
        }
        // The attributes of the item: those above the `cfg` and those below it, up to the item.
        let mut attrs = vec![*line];
        for above in lines.get(..i).unwrap_or_default().iter().rev() {
            if !above.starts_with("#[") {
                break;
            }
            attrs.push(above);
        }
        let mut item = None;
        for (j, below) in lines.iter().enumerate().skip(i + 1) {
            if below.starts_with("#[") {
                attrs.push(below);
            } else if !below.starts_with("//") {
                item = Some((j, *below));
                break;
            }
        }
        if let Some((_, text)) = item
            && is_mod(text)
            && !attrs.contains(&ATTRIBUTE)
        {
            out.push(i + 1);
        }
    }
    out
}

/// `xtask coverage-off`: the number of test modules without the attribute.
pub fn check() -> Result<usize, String> {
    let mut crates = layers::gated()?;
    crates.extend(coverage::REPORTED);
    let mut report = String::new();
    let mut problems = 0;
    for krate in &crates {
        for file in repo::files(&format!("crates/{krate}/src"), &["rs"]) {
            for line in missing(&repo::read(&file)?) {
                problems += 1;
                let _ = writeln!(
                    report,
                    "error: {}:{line}: test module without coverage(off); add {ATTRIBUTE} (VER-024)",
                    repo::rel(&file)
                );
            }
        }
    }
    print!("{report}");
    println!(
        "coverage-off: {} crate(s), {problems} problem(s)",
        crates.len()
    );
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    // req: VER-024
    #[test]
    fn a_test_module_without_the_attribute_is_found() {
        assert_eq!(missing("fn a() {}\n#[cfg(test)]\nmod tests {\n}\n"), [2]);
        assert_eq!(missing("#[cfg(test)]\nmod tests;\n"), [1]);
        assert_eq!(
            missing("#[cfg(all(test, feature = \"x\"))]\npub(crate) mod t {}\n"),
            [1]
        );
        assert_eq!(missing("#[cfg(test)]\n#[path = \"a.rs\"]\nmod t;\n"), [1]);
    }

    // req: VER-024
    #[test]
    fn the_attribute_above_or_below_the_cfg_satisfies_it() {
        let a = ATTRIBUTE;
        assert_eq!(
            missing(&format!("#[cfg(test)]\n{a}\nmod tests {{}}\n")),
            Vec::<usize>::new()
        );
        assert_eq!(
            missing(&format!("{a}\n#[cfg(test)]\nmod tests {{}}\n")),
            Vec::<usize>::new()
        );
        assert_eq!(
            missing(&format!("#[cfg(test)]\n{a}\n#[path = \"a\"]\nmod t;\n")),
            Vec::<usize>::new()
        );
    }

    // req: VER-024
    #[test]
    fn other_items_and_cfgs_are_not_test_modules() {
        assert_eq!(
            missing("#[cfg(test)]\nextern crate self as a;\n"),
            Vec::<usize>::new()
        );
        assert_eq!(missing("#[cfg(test)]\nuse a::b;\n"), Vec::<usize>::new());
        assert_eq!(missing("#[cfg(doctest)]\nmod d {}\n"), Vec::<usize>::new());
        assert_eq!(
            missing("#[cfg(feature = \"testing\")]\nmod d {}\n"),
            Vec::<usize>::new()
        );
    }
}
