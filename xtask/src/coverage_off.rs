//! `xtask coverage-off`: every test module of a coverage-measured crate carries
//! `#[cfg_attr(coverage_nightly, coverage(off))]` (VER-024), so that the gate measures library
//! code only ([`crate::coverage`]).
//!
//! The crates are those the coverage gate measures: [`crate::layers::gated`] and
//! [`crate::coverage::REPORTED`]. In each file under `src/`, a module (`mod x { … }` or
//! `mod x;`) whose `#[cfg(…)]` names `test` as a word (`test`, `all(test, …)`, `any(test, …)`;
//! not `doctest`) needs the attribute among the attributes of that module. The scanner is
//! textual; an attribute `rustfmt` wrapped over several lines is read whole.

use std::fmt::Write as _;

use crate::{coverage, layers, repo};

/// The attribute every test module carries.
pub const ATTRIBUTE: &str = "#[cfg_attr(coverage_nightly, coverage(off))]";

/// The attribute a file that is a test module as a whole (`#![cfg(test)]`) carries.
pub const INNER_ATTRIBUTE: &str = "#![cfg_attr(coverage_nightly, coverage(off))]";

/// `text` without the string literals (quotes included), which may hold any bracket or word.
fn outside_strings(text: &str) -> String {
    let mut out = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for c in text.chars() {
        if !quoted {
            if c == '"' {
                quoted = true;
            } else {
                out.push(c);
            }
        } else if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            quoted = false;
        }
    }
    out
}

/// Whether the predicate `body` of a `cfg` names `test` as a word outside every `not(…)`.
fn names_test(body: &str) -> bool {
    // One entry per open parenthesis: whether it is the argument list of `not`.
    let mut negated: Vec<bool> = Vec::new();
    let mut word = String::new();
    for c in outside_strings(body).chars() {
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
            continue;
        }
        if word == "test" && !negated.contains(&true) {
            return true;
        }
        match c {
            '(' => negated.push(word == "not"),
            ')' => {
                negated.pop();
            }
            _ => {}
        }
        word.clear();
    }
    false
}

/// Whether `line` is the attribute `<prefix>(…)]` (`#[cfg` or `#![cfg`) whose predicate names
/// `test` outside `not(…)`.
fn cfg_test(line: &str, prefix: &str) -> bool {
    line.strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix('('))
        .is_some_and(names_test)
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
    rest.starts_with("mod ")
}

/// A line between an attribute and its item that does not end the attribute block.
fn filler(line: &str) -> bool {
    line.is_empty() || line.starts_with("//")
}

/// The trimmed lines of `text` with their 1-based numbers, an attribute that `rustfmt` wrote on
/// several lines joined into one line (its tokens separated by nothing, as `rustfmt` breaks
/// only after `(` and `,`) under the number of its first line.
fn logical_lines(text: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut open = false;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if let (true, Some((_, last))) = (open, out.last_mut()) {
            last.push_str(line);
        } else {
            out.push((i + 1, line.to_owned()));
        }
        let Some((_, last)) = out.last() else {
            continue;
        };
        let code = outside_strings(last);
        open = code.starts_with('#') && code.matches('[').count() > code.matches(']').count();
    }
    out
}

/// The 1-based lines of the test modules of `text` that lack their attribute: a `mod` under
/// `#[cfg(test)]` without [`ATTRIBUTE`], and a `#![cfg(test)]` without [`INNER_ATTRIBUTE`].
fn missing(text: &str) -> Vec<usize> {
    let lines = logical_lines(text);
    // The inner attributes at the top of the file, where `#![cfg_attr(…)]` is accepted.
    let leading: Vec<&String> = lines
        .iter()
        .map(|(_, l)| l)
        .take_while(|l| l.starts_with("#![") || l.starts_with("//!") || filler(l))
        .collect();
    let mut out = Vec::new();
    for (i, (number, line)) in lines.iter().enumerate() {
        if cfg_test(line, "#![cfg") && !leading.iter().any(|l| *l == INNER_ATTRIBUTE) {
            out.push(*number);
        }
        if !cfg_test(line, "#[cfg") {
            continue;
        }
        // The attributes of the item: those above the `cfg` and those below it, up to the item.
        let mut attrs = vec![line.as_str()];
        for (_, above) in lines.iter().take(i).rev() {
            if above.starts_with("#[") {
                attrs.push(above);
            } else if !filler(above) {
                break;
            }
        }
        let mut item = "";
        for (_, below) in lines.iter().skip(i + 1) {
            if below.starts_with("#[") {
                attrs.push(below);
            } else if !filler(below) {
                item = below;
                break;
            }
        }
        if is_mod(item) && !attrs.contains(&ATTRIBUTE) {
            out.push(*number);
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
    fn a_test_module_without_the_attribute_is_missing() {
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
    fn blank_and_comment_lines_do_not_hide_the_module() {
        assert_eq!(missing("#[cfg(test)]\n\nmod tests {}\n"), [1]);
        assert_eq!(missing("#[cfg(test)]\n// the tests\nmod tests {}\n"), [1]);
        let a = ATTRIBUTE;
        assert_eq!(
            missing(&format!("{a}\n\n#[cfg(test)]\nmod tests {{}}\n")),
            Vec::<usize>::new()
        );
    }

    // req: VER-024
    #[test]
    fn the_attribute_above_or_below_the_cfg_satisfies_it() {
        let a = ATTRIBUTE;
        for text in [
            format!("#[cfg(test)]\n{a}\nmod tests {{}}\n"),
            format!("{a}\n#[cfg(test)]\nmod tests {{}}\n"),
            format!("#[cfg(test)]\n{a}\n#[path = \"a\"]\nmod t;\n"),
        ] {
            assert_eq!(missing(&text), Vec::<usize>::new());
        }
    }

    // req: VER-024
    #[test]
    fn other_items_and_cfgs_are_not_test_modules() {
        for text in [
            "#[cfg(test)]\nextern crate self as a;\n",
            "#[cfg(test)]\nuse a::b;\n",
            "#[cfg(doctest)]\nmod d {}\n",
            "#[cfg(feature = \"testing\")]\nmod d {}\n",
            "#[cfg(not(test))]\nmod d {}\n",
            "#[cfg(all(not(test), feature = \"x\"))]\nmod d {}\n",
            "#[cfg(not(any(test, feature = \"x\")))]\nmod d {}\n",
        ] {
            assert_eq!(missing(text), Vec::<usize>::new(), "{text}");
        }
        assert_eq!(
            missing("#[cfg(any(not(feature = \"x\"), test))]\nmod d {}\n"),
            [1]
        );
    }

    // req: VER-024
    #[test]
    fn a_file_that_is_a_test_module_needs_the_inner_attribute() {
        assert_eq!(missing("//! docs\n#![cfg(test)]\nfn a() {}\n"), [2]);
        let a = INNER_ATTRIBUTE;
        assert_eq!(
            missing(&format!("#![cfg(test)]\n{a}\nfn a() {{}}\n")),
            Vec::<usize>::new()
        );
        assert_eq!(
            missing("#![cfg(not(test))]\nfn a() {}\n"),
            Vec::<usize>::new()
        );
    }

    // req: VER-024
    #[test]
    fn a_cfg_over_several_lines_is_read_whole() {
        let cfg = "#[cfg(all(\n    test,\n    feature = \"x\"\n))]\n";
        assert_eq!(missing(&format!("fn a() {{}}\n{cfg}mod t {{}}\n")), [2]);
        assert_eq!(
            missing(&format!("{cfg}{ATTRIBUTE}\nmod t {{}}\n")),
            Vec::<usize>::new()
        );
        let off = "#[cfg(all(\n    not(test),\n    feature = \"x\"\n))]\nmod t {}\n";
        assert_eq!(missing(off), Vec::<usize>::new());
        assert_eq!(missing("#![cfg(\n    test\n)]\n"), [1]);
    }

    // req: VER-024
    #[test]
    fn string_literals_are_not_code() {
        let none = Vec::<usize>::new();
        assert_eq!(missing("#[cfg(feature = \"test\")]\nmod t {}\n"), none);
        assert_eq!(
            missing("#[doc = \"a[0 \\\" [\"]\nfn a() {}\n#[cfg(test)]\nmod t {}\n"),
            [3]
        );
    }

    // req: VER-024
    #[test]
    fn the_inner_attribute_counts_only_at_the_top_of_the_file() {
        let a = INNER_ATTRIBUTE;
        assert_eq!(missing(&format!("#![cfg(test)]\nfn a() {{}}\n{a}\n")), [1]);
        assert_eq!(
            missing(&format!("//! docs\n\n{a}\n#![cfg(test)]\nfn a() {{}}\n")),
            Vec::<usize>::new()
        );
    }
}
