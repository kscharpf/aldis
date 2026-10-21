//! `xtask cpp unsafe-optouts`: the bounds-checking opt-outs stay in one audited module
//! (CPP-031, docs/milestones/c0-foundations.md D9), and every suppression names its check
//! (CPP-042).
//!
//! Pointer arithmetic, raw subscripting, and unchecked element access are allowed only in
//! `cpp/include/aldis/core/bytes.hpp`, which opts out of Clang's `-Wunsafe-buffer-usage` with
//! `#pragma clang unsafe_buffer_usage begin/end` and of clang-tidy's bounds checks with `NOLINT`.
//! Anywhere else under `cpp/`, this check rejects:
//! - the `unsafe_buffer_usage` pragma, `-Wno-unsafe-buffer-usage`, and a diagnostic pragma that
//!   ignores `-Wunsafe-buffer-usage`;
//! - a `NOLINT`, `NOLINTNEXTLINE`, or `NOLINTBEGIN` naming `cppcoreguidelines-pro-bounds-*` or
//!   `*-avoid-unchecked-container-access`;
//! - a `.clang-tidy` that disables those checks.
//!
//! Everywhere, including `bytes.hpp`, a `NOLINT` form without a parenthesized check list, or
//! with `*` in it, is rejected: CPP-042 requires the suppression to name its check.

use crate::repo;

/// The audited byte- and element-access module (D9).
const BYTES: &str = "cpp/include/aldis/core/bytes.hpp";

/// Check names whose suppression is a bounds-checking opt-out.
const BOUNDS_CHECKS: &[&str] = &[
    "cppcoreguidelines-pro-bounds-",
    "avoid-unchecked-container-access",
    "hicpp-no-array-decay",
];

/// The bounds checks of the profile's groups (CPP-031).
const BOUNDS_CHECK_NAMES: &[&str] = &[
    "cppcoreguidelines-pro-bounds-array-to-pointer-decay",
    "cppcoreguidelines-pro-bounds-avoid-unchecked-container-access",
    "cppcoreguidelines-pro-bounds-constant-array-index",
    "cppcoreguidelines-pro-bounds-pointer-arithmetic",
    "hicpp-no-array-decay",
];

/// Whether one entry of a `.clang-tidy` `Checks` list disables a bounds check. The leading
/// `-*` that clears the defaults does not count.
fn disables_bounds_check(entry: &str) -> bool {
    let entry = entry.trim_matches(|c: char| c.is_whitespace() || matches!(c, ',' | '\'' | '"'));
    let Some(pattern) = entry.strip_prefix('-') else {
        return false;
    };
    if pattern == "*" {
        return false;
    }
    match pattern.split_once('*') {
        Some((prefix, _)) => BOUNDS_CHECK_NAMES.iter().any(|n| n.starts_with(prefix)),
        None => BOUNDS_CHECK_NAMES.contains(&pattern),
    }
}

/// Problems in one file's text; `rel` is its repository-relative path.
fn scan(rel: &str, text: &str) -> Vec<String> {
    let audited = rel == BYTES;
    let tidy_config = rel.ends_with(".clang-tidy");
    let mut problems = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let at = format!("{rel}:{}", n + 1);
        if line.contains("-Wno-unsafe-buffer-usage") {
            problems.push(format!(
                "{at}: CPP-031 -Wno-unsafe-buffer-usage is never allowed"
            ));
        }
        if !audited
            && (line.contains("unsafe_buffer_usage")
                || (line.contains("diagnostic ignored") && line.contains("unsafe-buffer-usage")))
        {
            problems.push(format!(
                "{at}: CPP-031 opt-out from -Wunsafe-buffer-usage outside {BYTES}"
            ));
        }
        if tidy_config && line.split(',').any(disables_bounds_check) {
            problems.push(format!(
                "{at}: CPP-031 a .clang-tidy disables a bounds check; only {BYTES} may opt out, with NOLINT"
            ));
        }
        let mut rest = line;
        while let Some(i) = rest.find("NOLINT") {
            let after = &rest[i + "NOLINT".len()..];
            let form_len = ["NEXTLINE", "BEGIN", "END"]
                .iter()
                .find(|f| after.starts_with(**f))
                .map_or(0, |f| f.len());
            let tail = &after[form_len..];
            match tail.strip_prefix('(').and_then(|t| t.split_once(')')) {
                Some((checks, _)) if !checks.trim().is_empty() && !checks.contains('*') => {
                    if !audited && BOUNDS_CHECKS.iter().any(|c| checks.contains(c)) {
                        problems.push(format!(
                            "{at}: CPP-031 bounds-check suppression ({checks}) outside {BYTES}"
                        ));
                    }
                }
                _ => problems.push(format!(
                    "{at}: CPP-042 NOLINT{} must name its check(s), without wildcards",
                    &after[..form_len]
                )),
            }
            rest = tail;
        }
    }
    problems
}

/// Files under `cpp/` that can carry an opt-out.
fn candidates() -> Vec<std::path::PathBuf> {
    repo::all_files("cpp")
        .into_iter()
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            name == ".clang-tidy"
                || name == "CMakeLists.txt"
                || name == "CMakePresets.json"
                || std::path::Path::new(name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| matches!(e, "hpp" | "cpp" | "h" | "cmake" | "in"))
        })
        .collect()
}

/// `xtask cpp unsafe-optouts`.
pub fn check() -> Result<usize, String> {
    let files = candidates();
    let mut problems = Vec::new();
    for path in &files {
        problems.extend(scan(&repo::rel(path), &repo::read(path)?));
    }
    for p in &problems {
        println!("error: {p}");
    }
    println!(
        "cpp unsafe-optouts: {} file(s) under cpp/, {} problem(s)",
        files.len(),
        problems.len()
    );
    Ok(problems.len())
}

#[cfg(test)]
mod tests {
    use super::{BYTES, scan};

    #[test]
    fn opt_outs_only_in_the_bytes_module() {
        // req: CPP-031
        let pragma = "#pragma clang unsafe_buffer_usage begin\n";
        assert_eq!(scan(BYTES, pragma), Vec::<String>::new());
        let e = scan("cpp/include/aldis/core/bits.hpp", pragma);
        assert_eq!(e.len(), 1);
        assert!(e[0].contains("bits.hpp:1: CPP-031"), "{e:?}");

        let nolint =
            "x = p[i]; // NOLINT(cppcoreguidelines-pro-bounds-pointer-arithmetic): audited\n";
        assert_eq!(scan(BYTES, nolint), Vec::<String>::new());
        assert_eq!(scan("cpp/tests/core/x_test.cpp", nolint).len(), 1);
        let container =
            "// NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)\n";
        assert_eq!(scan("cpp/include/aldis/core/oid.hpp", container).len(), 1);

        let ignored = "#pragma clang diagnostic ignored \"-Wunsafe-buffer-usage\"\n";
        assert_eq!(scan("cpp/include/aldis/core/crc.hpp", ignored).len(), 1);
        assert_eq!(scan(BYTES, ignored), Vec::<String>::new());
        let flag = "target_compile_options(x PRIVATE -Wno-unsafe-buffer-usage)\n";
        assert_eq!(scan("cpp/CMakeLists.txt", flag).len(), 1);
        assert_eq!(scan(BYTES, flag).len(), 1);

        for tidy in [
            "Checks: >\n  -cppcoreguidelines-pro-bounds-constant-array-index\n",
            "Checks: >\n  -*,\n  bugprone-*,\n  -cppcoreguidelines-*,\n",
            "Checks: '-*,readability-*,-hicpp-no-array-decay'\n",
        ] {
            assert_eq!(scan("cpp/tests/.clang-tidy", tidy).len(), 1, "{tidy}");
        }
        let resets =
            "Checks: >\n  -*,\n  cppcoreguidelines-*,\n  -modernize-use-trailing-return-type\n";
        assert_eq!(scan("cpp/.clang-tidy", resets), Vec::<String>::new());
        let enabling = "-Wunsafe-buffer-usage  # enabled for every module\n";
        assert_eq!(
            scan("cpp/cmake/AldisWarnings.cmake", enabling),
            Vec::<String>::new()
        );
    }

    #[test]
    fn every_nolint_names_its_check() {
        // req: CPP-042
        for ok in [
            "f(); // NOLINT(bugprone-branch-clone): reason\n",
            "// NOLINTBEGIN(cppcoreguidelines-macro-usage,modernize-macro-to-enum)\n// NOLINTEND(cppcoreguidelines-macro-usage,modernize-macro-to-enum)\n",
            "// NOLINTNEXTLINE(misc-no-recursion)\n",
        ] {
            assert!(
                scan("cpp/include/aldis/core/error.hpp", ok).is_empty(),
                "{ok}"
            );
        }
        for bad in [
            "f(); // NOLINT\n",
            "// NOLINTNEXTLINE\n",
            "// NOLINTBEGIN\n",
            "f(); // NOLINT(*)\n",
            "f(); // NOLINT(cppcoreguidelines-*)\n",
            "f(); // NOLINT()\n",
        ] {
            let e = scan(BYTES, bad);
            assert_eq!(e.len(), 1, "{bad}: {e:?}");
            assert!(e[0].contains("CPP-042"), "{e:?}");
        }
    }
}
