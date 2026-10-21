//! Symbol checks of a linked freestanding exerciser (docs/milestones/c0-foundations.md D6).
//!
//! - `link-check` (CPP-050): the exerciser links with `-nostdlib`, so a reference to an
//!   allocation, exception, abort, assertion, guarded-static, or unwinder routine fails the
//!   link. This check also fails if any of them is *present* in the ELF, so adding a library to
//!   the link line cannot hide one.
//! - `statics` (CPP-030): no writable data symbol of `aldis::` code (mutable statics, or the
//!   guard variable of a static with dynamic initialization).
//!
//! Both read `nm --demangle --format=sysv` output; the tool is `$ALDIS_NM`, default `llvm-nm`.

use std::process::Command;

/// Symbols that must not appear, by exact (C or demangled) name (D6).
const FORBIDDEN: &[&str] = &[
    "__cxa_throw",
    "__cxa_allocate_exception",
    "__cxa_guard_acquire",
    "__cxa_guard_release",
    "__cxa_pure_virtual",
    "abort",
    "__assert_func",
    "__assert_fail",
    "__gxx_personality_v0",
    "__aeabi_unwind_cpp_pr0",
    "__aeabi_unwind_cpp_pr1",
    "__aeabi_unwind_cpp_pr2",
];

/// Demangled-name prefixes that must not appear (every overload of these).
const FORBIDDEN_PREFIXES: &[&str] = &[
    "operator new",
    "operator delete",
    "std::__glibcxx_assert_fail",
];

/// Sections holding writable data.
const WRITABLE_SECTIONS: &[&str] = &[".data", ".bss", ".sdata", ".sbss", ".tdata", ".tbss"];

/// One symbol of `nm --format=sysv` output.
#[derive(Debug, PartialEq, Eq)]
struct Symbol {
    name: String,
    class: String,
    section: String,
}

/// Parses `nm --format=sysv` lines: `name|value|class|type|size|line|section`.
fn parse_sysv(text: &str) -> Vec<Symbol> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('|').collect();
            // A demangled name may itself contain '|' (operator|): the last six fields are fixed.
            let n = fields.len().checked_sub(6)?;
            if n == 0 {
                return None;
            }
            Some(Symbol {
                name: fields[..n].join("|").trim().to_owned(),
                class: fields[n + 1].trim().to_owned(),
                section: fields[n + 5].trim().to_owned(),
            })
        })
        .filter(|s| !s.name.is_empty())
        .collect()
}

fn nm(elf: &str) -> Result<Vec<Symbol>, String> {
    let tool = std::env::var("ALDIS_NM").unwrap_or_else(|_| "llvm-nm".to_owned());
    let out = Command::new(&tool)
        .args(["--demangle", "--format=sysv", elf])
        .output()
        .map_err(|e| format!("{tool}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{tool} {elf} failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let symbols = parse_sysv(&String::from_utf8_lossy(&out.stdout));
    if symbols.is_empty() {
        return Err(format!("{tool} found no symbols in {elf}"));
    }
    Ok(symbols)
}

/// Forbidden symbols present in `symbols`, defined or not.
fn forbidden(symbols: &[Symbol]) -> Vec<String> {
    let mut hits: Vec<String> = symbols
        .iter()
        .filter(|s| {
            FORBIDDEN.contains(&s.name.as_str())
                || FORBIDDEN_PREFIXES.iter().any(|p| s.name.starts_with(p))
        })
        .map(|s| s.name.clone())
        .collect();
    hits.sort();
    hits.dedup();
    hits
}

/// Writable data symbols of `aldis::` code.
fn writable_aldis_data(symbols: &[Symbol]) -> Vec<String> {
    let mut hits: Vec<String> = symbols
        .iter()
        .filter(|s| s.name.contains("aldis::"))
        .filter(|s| {
            WRITABLE_SECTIONS
                .iter()
                .any(|w| s.section == *w || s.section.starts_with(&format!("{w}.")))
                || matches!(s.class.as_str(), "d" | "D" | "b" | "B" | "C")
        })
        .map(|s| format!("{} ({})", s.name, s.section))
        .collect();
    hits.sort();
    hits.dedup();
    hits
}

/// `xtask cpp link-check <elf>` (CPP-050).
pub fn link_check(elf: &str) -> Result<usize, String> {
    let symbols = nm(elf)?;
    let hits = forbidden(&symbols);
    for h in &hits {
        println!("error: CPP-050 {elf}: forbidden symbol `{h}`");
    }
    println!(
        "cpp link-check: {} symbol(s) in {elf}, {} forbidden",
        symbols.len(),
        hits.len()
    );
    Ok(hits.len())
}

/// `xtask cpp statics <elf>` (CPP-030).
pub fn statics(elf: &str) -> Result<usize, String> {
    let symbols = nm(elf)?;
    let hits = writable_aldis_data(&symbols);
    for h in &hits {
        println!("error: CPP-030 {elf}: writable aldis:: data `{h}`");
    }
    println!(
        "cpp statics: {} symbol(s) in {elf}, {} writable aldis:: data symbol(s)",
        symbols.len(),
        hits.len()
    );
    Ok(hits.len())
}

#[cfg(test)]
mod tests {
    use super::{forbidden, parse_sysv, writable_aldis_data};

    /// `llvm-nm --demangle --format=sysv` output of a small Arm ELF.
    const NM: &str = "

Symbols from exerciser.elf:

Name                  Value   Class        Type         Size     Line  Section

_start                |00000100|   T  |            FUNC|00000010|     |.text
aldis::core::crc::table|00000400|   R  |          OBJECT|00000200|     |.rodata
aldis::core::detail::counter|20000000|   B  |          OBJECT|00000004|     |.bss
guard variable for aldis::core::x()::s|20000004|   b  |          OBJECT|00000008|     |.bss
exerciser_state       |20000010|   D  |          OBJECT|00000004|     |.data
operator new(unsigned int)|        |   U  |            NOTYPE|        |     |
aldis::core::Flags::operator|(aldis::core::Flags)|00000200|   W  |            FUNC|00000008|     |.text
__cxa_guard_acquire   |00000300|   T  |            FUNC|00000020|     |.text
memcpy                |00000340|   T  |            FUNC|00000020|     |.text
";

    #[test]
    fn parses_sysv_with_pipes_in_names() {
        // req: CPP-050
        let symbols = parse_sysv(NM);
        assert_eq!(symbols.len(), 9);
        assert_eq!(
            symbols[6].name,
            "aldis::core::Flags::operator|(aldis::core::Flags)"
        );
        assert_eq!(symbols[6].section, ".text");
        assert_eq!(symbols[5].class, "U");
    }

    #[test]
    fn link_check_finds_defined_and_undefined_symbols() {
        // req: CPP-050, GEN-021
        assert_eq!(
            forbidden(&parse_sysv(NM)),
            ["__cxa_guard_acquire", "operator new(unsigned int)"]
        );
        let clean = parse_sysv("_start|00000100|   T  |  FUNC|00000010|     |.text\n");
        assert_eq!(forbidden(&clean), Vec::<String>::new());
        for name in [
            "abort",
            "__assert_func",
            "__aeabi_unwind_cpp_pr0",
            "std::__glibcxx_assert_fail(char const*, int, char const*, char const*)",
            "operator delete(void*, unsigned int)",
        ] {
            let one = parse_sysv(&format!(
                "{name}|        |   U  |  NOTYPE|        |     |\n"
            ));
            assert_eq!(forbidden(&one), [name], "{name}");
        }
    }

    #[test]
    fn statics_flags_only_writable_aldis_data() {
        // req: CPP-030
        assert_eq!(
            writable_aldis_data(&parse_sysv(NM)),
            [
                "aldis::core::detail::counter (.bss)",
                "guard variable for aldis::core::x()::s (.bss)"
            ]
        );
        let small =
            parse_sysv("aldis::core::v|00000000|   G  |  OBJECT|00000004|     |.sdata.aldis\n");
        assert_eq!(writable_aldis_data(&small).len(), 1);
    }
}
