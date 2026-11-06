//! `xtask header`: the C header `include/aldis/aldis.h`, generated from `aldis-ffi` by the
//! pinned `cbindgen` (CABI-002, CABI-003; M2 plan D4), and the build-time capacities of
//! `crates/aldis-ffi/capacities.toml` that it publishes (CABI-030; M2 plan D5).
//!
//! `cargo xtask header` regenerates both generated files in memory and fails when the
//! committed ones differ, when `cbindgen` is not the pinned version, or when the header
//! includes anything but `<stdint.h>`, `<stddef.h>`, and `<stdbool.h>` or lacks its
//! `extern "C"` block. `--write` regenerates them. `cargo xtask header c-smoke <cc> <c++>`
//! builds the static and shared libraries and compiles, links, and runs a C program with them,
//! as C99, C11, and C23 against the static library and as C++11 and C++17 against the shared one,
//! with `-Wall -Wextra -Wpedantic -Werror` (CABI-001, CABI-003, VER-050).
//! `cargo xtask header cpp-engines <c++>` compiles the C++20 wrapper `include/aldis/aldis.hpp` of the
//! engine handles with the tests of `crates/aldis-ffi/tests/cpp/`, which run the engine-scenario
//! vectors of `test-vectors/cop1/` and `test-vectors/link/` through it (CABI-072).
//! `cargo xtask header c-tidy <clang-tidy>` runs the CERT C checks of clang-tidy over the C code
//! of `crates/aldis-ffi/tests/c/` and the README example (VER-051).

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use crate::repo;

/// The pinned `cbindgen` version; the verification image installs the same
/// (`xtask/verify/Containerfile`). MPL-2.0, checked against VER-055 in M2 WP-0c.
pub const CBINDGEN_VERSION: &str = "0.29.4";
/// The generated header.
pub const HEADER: &str = "include/aldis/aldis.h";
/// The capacities of the build (CABI-030).
pub const CAPACITIES_TOML: &str = "crates/aldis-ffi/capacities.toml";
/// The Rust constants generated from it.
pub const CAPACITIES_RS: &str = "crates/aldis-ffi/src/capacities.rs";
/// The C program of `c-smoke`.
const SMOKE: &str = "crates/aldis-ffi/tests/c/smoke.c";
/// Its first `c` block is the usage example that `c-smoke` compiles against the header.
const README: &str = "crates/aldis-ffi/README.md";
/// The C standards the header and the smoke program compile as (VER-050); `c2x` is C23 under the
/// spelling of the compilers before `c23`.
const C_STANDARDS: [&str; 3] = ["c99", "c11", "c2x"];
/// The C++ standards likewise (VER-050).
const CXX_STANDARDS: [&str; 2] = ["c++11", "c++17"];
/// The only headers `aldis.h` may include (CABI-003).
const INCLUDES: [&str; 3] = ["<stdint.h>", "<stddef.h>", "<stdbool.h>"];

/// A capacity the C ABI publishes: `[section] key` becomes `ALDIS_<SECTION>_<KEY>`.
struct Capacity {
    section: &'static str,
    key: &'static str,
    /// What it bounds, for the generated doc comment.
    doc: &'static str,
    /// Smallest and largest value accepted.
    range: (u64, u64),
}

/// The capacities of the engines that have a C ABI. The section of the link engines (`[link]`)
/// joined with their handles (M4 WP-7). FARM-1 has no capacity: it holds no queue (M3 plan D6).
const CAPACITIES: &[Capacity] = &[
    Capacity {
        section: "spp",
        key: "tracker_max_apids",
        doc: "APIDs that one `aldis_spp_tracker_t` tracks (SPP-011; M2 plan D5)",
        range: (1, 2048),
    },
    // TC (M2 WP-3 module 8).
    Capacity {
        section: "tc",
        key: "max_maps",
        doc: "MAPs that one `aldis_tc_*` call takes with a master channel (232.0 Table 5-4; M2 plan D14)",
        range: (1, 4096),
    },
    // ---- USLP (M2 WP-5 module 9): the capacities of the channel table every `aldis_uslp_*`
    // call builds, `VcTable<ALDIS_USLP_MAX_VCS, ALDIS_USLP_MAX_MAPS>` (M2 plan D14).
    Capacity {
        section: "uslp",
        key: "max_vcs",
        doc: "Virtual channels in the channel table of one `aldis_uslp_*` call (API-040; M2 plan D14)",
        range: (1, 1024),
    },
    Capacity {
        section: "uslp",
        key: "max_maps",
        doc: "MAP channels in the channel table of one `aldis_uslp_*` call (API-040; M2 plan D14)",
        range: (1, 4096),
    },
    // ---- end USLP
    // FOP-1 (M3 WP-6): the Sent_Queue and the frame size of one `aldis_fop1_t`,
    // `Fop1<ALDIS_FOP1_SENT_QUEUE_FRAMES, ALDIS_FOP1_MAX_FRAME_OCTETS, _>` (M3 plan D10).
    Capacity {
        section: "fop1",
        key: "sent_queue_frames",
        doc: "Frames on the `Sent_Queue` of one `aldis_fop1_t`, the largest FOP Sliding Window Width K (232.1 §5.1.7, §5.1.12; COP-011, COP-015)",
        range: (1, 255),
    },
    Capacity {
        section: "fop1",
        key: "max_frame_octets",
        doc: "Octets of the largest TC Transfer Frame one `aldis_fop1_t` builds (232.0 §4.1.2.7; COP-010)",
        range: (6, 1024),
    },
    // The link engines (M4 WP-7): `[link]` sizes their queues, buffers, and tables.
    Capacity {
        section: "link",
        key: "max_virtual_channels",
        doc: "Virtual channels of one `aldis_channel_mux_t` and of one `aldis_frame_receiver_t` (LNK-030, LNK-050; M4 plan D4)",
        range: (1, 64),
    },
    Capacity {
        section: "link",
        key: "max_map_channels",
        doc: "MAP channels of one `aldis_map_reassembler_t` (LNK-070; M4 plan D4)",
        range: (1, 64),
    },
    Capacity {
        section: "link",
        key: "packet_queue_octets",
        doc: "Octets of the packet, SDU, and bit queues of the generators, and of the segmenter's queue (LNK-015; M4 plan D4)",
        range: (64, 65536),
    },
    Capacity {
        section: "link",
        key: "max_frame_octets",
        doc: "Octets of the largest data field or frame of one link engine (LNK-075; M4 plan D4)",
        range: (16, 65536),
    },
];

/// Parses `capacities.toml`: `[section]` lines and `key = integer` lines with optional
/// comments; returns `(section, key, value)` in file order.
pub(crate) fn parse_toml(text: &str) -> Result<Vec<(String, String, u64)>, String> {
    let mut section = None;
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let at = format!("{CAPACITIES_TOML}:{}", n + 1);
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = Some(name.trim().to_owned());
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("{at}: expected `key = value`"))?;
        let section = section
            .clone()
            .ok_or_else(|| format!("{at}: key outside a [section]"))?;
        let value = value
            .trim()
            .parse::<u64>()
            .map_err(|e| format!("{at}: {e}"))?;
        out.push((section, key.trim().to_owned(), value));
    }
    Ok(out)
}

/// The generated `capacities.rs` for the entries of `capacities.toml`.
fn capacities_rs(entries: &[(String, String, u64)]) -> Result<String, String> {
    let mut out = String::from(
        "// Generated by `cargo xtask header --write` from crates/aldis-ffi/capacities.toml.\n\
         // Do not edit: change capacities.toml and regenerate.\n\
         \n\
         //! Build-time capacities of the C ABI (CABI-030, GEN-011): the `ALDIS_*_MAX_*` constants\n\
         //! of the header.\n",
    );
    for c in CAPACITIES {
        let found: Vec<u64> = entries
            .iter()
            .filter(|(s, k, _)| s == c.section && k == c.key)
            .map(|e| e.2)
            .collect();
        let [value] = found.as_slice() else {
            return Err(format!(
                "{CAPACITIES_TOML}: [{}] {} must be given once, found {}",
                c.section,
                c.key,
                found.len()
            ));
        };
        if !(c.range.0..=c.range.1).contains(value) {
            return Err(format!(
                "{CAPACITIES_TOML}: [{}] {} = {value} is outside {}..={}",
                c.section, c.key, c.range.0, c.range.1
            ));
        }
        let name = format!("ALDIS_{}_{}", c.section, c.key).to_ascii_uppercase();
        let _ = write!(
            out,
            "\n/// {}.\n///\n/// `[{}] {}` of `capacities.toml` (CABI-030).\npub const {name}: usize = {value};\n",
            c.doc, c.section, c.key
        );
    }
    for (s, k, _) in entries {
        let known_section = CAPACITIES.iter().any(|c| c.section == s);
        let known_key = CAPACITIES.iter().any(|c| c.section == s && c.key == k);
        if known_section && !known_key {
            return Err(format!("{CAPACITIES_TOML}: unknown key [{s}] {k}"));
        }
    }
    Ok(out)
}

/// The problems of a generated header: includes other than [`INCLUDES`], or no `extern "C"`
/// block under `__cplusplus` (CABI-003).
fn header_problems(text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("#include") {
            let name = rest.trim();
            if !INCLUDES.contains(&name) {
                problems.push(format!("{HEADER}: includes {name} (CABI-003)"));
            }
        }
    }
    let guarded = text.contains("#ifdef __cplusplus\nextern \"C\" {");
    if !guarded {
        problems.push(format!(
            "{HEADER}: no `extern \"C\"` block under `__cplusplus` (CABI-003)"
        ));
    }
    problems
}

/// The version `cbindgen --version` reports.
fn cbindgen_version() -> Result<String, String> {
    let out = Command::new("cbindgen")
        .arg("--version")
        .output()
        .map_err(|e| {
            format!("cbindgen: {e}; install it with `cargo install --locked cbindgen@{CBINDGEN_VERSION}`")
        })?;
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text.trim().trim_start_matches("cbindgen").trim().to_owned())
}

/// The header `cbindgen` generates from the current sources.
fn generate(root: &Path) -> Result<String, String> {
    let version = cbindgen_version()?;
    if version != CBINDGEN_VERSION {
        return Err(format!(
            "cbindgen {version} found, {CBINDGEN_VERSION} pinned (CABI-002); install it with \
             `cargo install --locked cbindgen@{CBINDGEN_VERSION}`"
        ));
    }
    let out_dir = root.join("target/header");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let output = out_dir.join("aldis.h");
    let status = Command::new("cbindgen")
        .args(["--config", "cbindgen.toml", "--quiet", "--output"])
        .arg(&output)
        .current_dir(root.join("crates/aldis-ffi"))
        .status()
        .map_err(|e| format!("cbindgen: {e}"))?;
    if !status.success() {
        return Err("cbindgen failed".to_owned());
    }
    repo::read(&output)
}

/// The problem of the generated file `path` when its committed text `committed` (`None` when
/// it is missing) is not `generated`, the text generated `from` a source.
fn stale(path: &str, committed: Option<&str>, generated: &str, from: &str) -> Option<String> {
    (committed != Some(generated))
        .then(|| format!("{path} differs from {from}; run `cargo xtask header --write`"))
}

/// Checks the generated files, or with `write` regenerates them; returns the number of
/// problems. Part of `cargo xtask ci` and of the verify check `header`.
pub fn run(write: bool) -> Result<usize, String> {
    let root = repo::root();
    let capacities = capacities_rs(&parse_toml(&repo::read(&root.join(CAPACITIES_TOML))?)?)?;
    let mut problems = Vec::new();
    if write {
        std::fs::write(root.join(CAPACITIES_RS), &capacities)
            .map_err(|e| format!("{CAPACITIES_RS}: {e}"))?;
        println!("wrote {CAPACITIES_RS}");
    } else {
        let committed = std::fs::read_to_string(root.join(CAPACITIES_RS)).ok();
        problems.extend(stale(
            CAPACITIES_RS,
            committed.as_deref(),
            &capacities,
            CAPACITIES_TOML,
        ));
    }
    let header = generate(&root)?;
    problems.extend(header_problems(&header));
    if write {
        let path = root.join(HEADER);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, &header).map_err(|e| format!("{HEADER}: {e}"))?;
        println!("wrote {HEADER}");
    } else {
        let committed = std::fs::read_to_string(root.join(HEADER)).ok();
        problems.extend(stale(
            HEADER,
            committed.as_deref(),
            &header,
            "a fresh generation (CABI-002)",
        ));
    }
    for p in &problems {
        println!("error: {p}");
    }
    println!("header: {HEADER}, {} problem(s)", problems.len());
    Ok(problems.len())
}

/// The first fenced `c` block of `readme` as a C translation unit: its `#` lines first, then the other
/// lines as the body of a function that takes the `bytes` and `len` the example uses. The example
/// is a fragment, so this is how it compiles (CABI-003).
fn example_unit(readme: &str) -> Result<String, String> {
    let mut block = readme
        .lines()
        .skip_while(|l| l.trim_end() != "```c")
        .skip(1)
        .take_while(|l| l.trim_end() != "```")
        .peekable();
    block.peek().ok_or("the README has no fenced c block")?;
    let (directives, body): (Vec<&str>, Vec<&str>) = block.partition(|l| l.starts_with('#'));
    Ok(format!(
        "{}\nint aldis_readme_example(const uint8_t *bytes, size_t len)\n{{\n    (void)bytes;\n    (void)len;\n{}\n    return 0;\n}}\n",
        directives.join("\n"),
        body.join("\n")
    ))
}

fn run_cmd(cmd: &mut Command) -> Result<String, String> {
    let shown = format!("{cmd:?}");
    let out = cmd.output().map_err(|e| format!("{shown}: {e}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if out.status.success() {
        Ok(text)
    } else {
        Err(format!("{shown} failed:\n{text}"))
    }
}

/// The native libraries a static library of the host needs, from rustc's
/// `native-static-libs` note.
fn native_static_libs(note: &str) -> Vec<String> {
    note.lines()
        .filter_map(|l| l.split_once("native-static-libs:").map(|(_, libs)| libs))
        .flat_map(str::split_whitespace)
        .map(str::to_owned)
        .collect()
}

/// `xtask header c-smoke <cc> <c++>`: the host libraries (CABI-001) and the header as C99, C11,
/// C23, C++11, and C++17 (CABI-003, VER-050), through a C program that calls the library. The
/// C23 build takes the branch of each enumeration that declares its underlying type (c-abi.md
/// §12.3, spike 1); a compiler that does not define `__STDC_VERSION__` as C23 yet compiles the
/// C99 branch.
pub fn c_smoke(cc: &str, cxx: &str) -> Result<usize, String> {
    let root = repo::root();
    // The static library's native dependencies, printed while it is built (CABI-001). Without
    // colour, so that no escape sequence of `CARGO_TERM_COLOR=always` ends up in a library name.
    let note = run_cmd(
        Command::new("cargo")
            .args(["--color", "never"])
            .args(["rustc", "-p", "aldis-ffi", "--release", "--lib"])
            .args([
                "--crate-type",
                "staticlib",
                "--",
                "--print",
                "native-static-libs",
            ])
            .current_dir(&root),
    )?;
    run_cmd(
        Command::new("cargo")
            .args(["build", "-p", "aldis-ffi", "--release", "--lib"])
            .current_dir(&root),
    )?;
    let lib_dir = root.join("target/release");
    let shared = if cfg!(target_os = "macos") {
        "libaldis_ffi.dylib"
    } else {
        "libaldis_ffi.so"
    };
    for lib in ["libaldis_ffi.a", shared] {
        if !lib_dir.join(lib).is_file() {
            return Err(format!("target/release/{lib} was not built (CABI-001)"));
        }
    }
    let out_dir = root.join("target/header");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let include = root.join("include");
    let smoke = root.join(SMOKE);
    let warnings = ["-Wpedantic", "-Wall", "-Wextra", "-Werror"];
    // C99, C11, and C23 (`-std=c2x` for the compilers before the spelling `c23`) against the
    // static library.
    let mut exes = Vec::new();
    for std in C_STANDARDS {
        let exe = out_dir.join(format!("smoke-{std}"));
        run_cmd(
            Command::new(cc)
                .arg(format!("-std={std}"))
                .args(warnings)
                .arg("-I")
                .arg(&include)
                .arg(&smoke)
                .arg(lib_dir.join("libaldis_ffi.a"))
                .args(native_static_libs(&note))
                .arg("-o")
                .arg(&exe),
        )?;
        exes.push(exe);
    }
    // The usage example of the README is C99 that the header accepts: a field it names that the
    // header lacks fails here.
    let example = out_dir.join("readme-example.c");
    let readme = repo::read(&root.join(README))?;
    std::fs::write(
        &example,
        example_unit(&readme).map_err(|e| format!("{README}: {e}"))?,
    )
    .map_err(|e| e.to_string())?;
    run_cmd(
        Command::new(cc)
            .args(["-std=c99", "-fsyntax-only"])
            .args(warnings)
            .arg("-I")
            .arg(&include)
            .arg(&example),
    )
    .map_err(|e| format!("{README}: the example does not compile against the header:\n{e}"))?;
    // C++11 and C++17 against the shared library.
    for std in CXX_STANDARDS {
        let exe = out_dir.join(format!("smoke-{std}"));
        run_cmd(
            Command::new(cxx)
                .args(["-x", "c++"])
                .arg(format!("-std={std}"))
                .args(warnings)
                .arg("-I")
                .arg(&include)
                .arg(&smoke)
                .args(["-x", "none"])
                .arg(lib_dir.join(shared))
                .arg(format!("-Wl,-rpath,{}", lib_dir.display()))
                .arg("-o")
                .arg(&exe),
        )?;
        exes.push(exe);
    }
    for exe in &exes {
        print!("{}", run_cmd(&mut Command::new(exe))?);
    }
    println!(
        "c-smoke: {cc} ({}, static) and {cxx} ({}, shared) passed",
        C_STANDARDS.join(", "),
        CXX_STANDARDS.join(", ")
    );
    Ok(0)
}

/// The C++ tests of `aldis.hpp` (CABI-072): the engine scenarios and the capacity check.
const CPP_TESTS: &str = "crates/aldis-ffi/tests/cpp";

/// `xtask header cpp-engines <c++>`: compiles `engines.cpp` against `aldis.hpp` and the shared
/// library as C++20 without exceptions, with `-Wall -Wextra -Wpedantic -Werror`, and runs it over
/// the engine-scenario vectors (CABI-072, CABI-030, CABI-031). `capacity_mismatch.cpp` must fail
/// to compile with the `static_assert` of the capacity, and compile once the capacity is right.
pub fn cpp_engines(cxx: &str) -> Result<usize, String> {
    let root = repo::root();
    run_cmd(
        Command::new("cargo")
            .args(["build", "-p", "aldis-ffi", "--release", "--lib"])
            .current_dir(&root),
    )?;
    let lib_dir = root.join("target/release");
    let shared = if cfg!(target_os = "macos") {
        "libaldis_ffi.dylib"
    } else {
        "libaldis_ffi.so"
    };
    if !lib_dir.join(shared).is_file() {
        return Err(format!("target/release/{shared} was not built (CABI-001)"));
    }
    let out_dir = root.join("target/header");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let tests = root.join(CPP_TESTS);
    let flags = [
        "-std=c++20",
        "-fno-exceptions",
        "-Wpedantic",
        "-Wall",
        "-Wextra",
        "-Werror",
    ];
    let exe = out_dir.join("engines-cpp20");
    run_cmd(
        Command::new(cxx)
            .args(flags)
            .arg("-I")
            .arg(root.join("include"))
            .arg("-I")
            .arg(&tests)
            .arg(tests.join("engines.cpp"))
            .arg(lib_dir.join(shared))
            .arg(format!("-Wl,-rpath,{}", lib_dir.display()))
            .arg("-o")
            .arg(&exe),
    )?;
    print!(
        "{}",
        run_cmd(Command::new(&exe).arg(root.join("test-vectors")))?
    );
    // A capacity that differs from the library's does not compile (OI-20).
    let mismatch = tests.join("capacity_mismatch.cpp");
    let syntax = |ok: bool| {
        let mut cmd = Command::new(cxx);
        cmd.args(flags)
            .arg("-fsyntax-only")
            .arg("-I")
            .arg(root.join("include"));
        if ok {
            cmd.arg("-DALDIS_HPP_CAPACITY_OK");
        }
        cmd.arg(&mismatch);
        run_cmd(&mut cmd)
    };
    syntax(true).map_err(|e| format!("{CPP_TESTS}/capacity_mismatch.cpp, capacity right:\n{e}"))?;
    match syntax(false) {
        Ok(_) => {
            return Err(format!(
                "{CPP_TESTS}/capacity_mismatch.cpp compiled with a wrong capacity (CABI-072)"
            ));
        }
        Err(e) if e.contains("ALDIS_FOP1_SENT_QUEUE_FRAMES (CABI-030)") => {}
        Err(e) => {
            return Err(format!(
                "{CPP_TESTS}/capacity_mismatch.cpp failed for another reason:\n{e}"
            ));
        }
    }
    println!("cpp-engines: {cxx} (c++20, no exceptions) passed");
    Ok(0)
}

/// The clang-tidy checks of the CERT C rules (VER-051; OI-22): every `cert-*-c` check.
const CERT_C_CHECKS: &str = "-*,cert-*-c";

/// `xtask header c-tidy <clang-tidy>`: the CERT C checks of clang-tidy over every `.c` file of
/// `crates/aldis-ffi/tests/c/` and the C example of the README, each finding an error (VER-051).
/// A deviation is a `NOLINT` with its reason in the source and a row in
/// `docs/verification/records/VER-051.md`.
pub fn c_tidy(tidy: &str) -> Result<usize, String> {
    let root = repo::root();
    let dir = root.join(Path::new(SMOKE).parent().unwrap_or(Path::new(".")));
    let mut files = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    files.retain(|p| p.extension().is_some_and(|x| x == "c"));
    files.sort();
    let out_dir = root.join("target/header");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let example = out_dir.join("readme-example.c");
    let readme = repo::read(&root.join(README))?;
    std::fs::write(
        &example,
        example_unit(&readme).map_err(|e| format!("{README}: {e}"))?,
    )
    .map_err(|e| e.to_string())?;
    files.push(example);
    for file in &files {
        run_cmd(
            Command::new(tidy)
                .arg(format!("-checks={CERT_C_CHECKS}"))
                .arg("--warnings-as-errors=*")
                .arg("--quiet")
                .arg(file)
                .args(["--", "-std=c99", "-I"])
                .arg(root.join("include")),
        )
        .map_err(|e| format!("{}: CERT C (VER-051):\n{e}", file.display()))?;
    }
    println!(
        "c-tidy: {tidy}: CERT C checks passed on {} files",
        files.len()
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // req: CABI-005
    #[test]
    fn the_readme_example_becomes_a_function_body() {
        let readme = "text\n\n```sh\nls\n```\n\n```c\n#include \"aldis/aldis.h\"\n\nuint32_t rc = f(bytes, len);\nif (rc) {\n}\n```\n\n```c\nnot this\n```\n";
        let unit = example_unit(readme).unwrap();
        assert!(unit.starts_with("#include \"aldis/aldis.h\"\nint aldis_readme_example("));
        assert!(unit.contains("{\n    (void)bytes;\n    (void)len;\n\nuint32_t rc = f(bytes, len);\nif (rc) {\n}\n    return 0;\n}\n"));
        assert!(!unit.contains("not this") && !unit.contains("ls"));
        assert!(example_unit("no code here\n").is_err());
    }

    // req: CABI-030
    #[test]
    fn capacities_are_generated_from_the_toml() {
        let entries = parse_toml(
            "# c\n[fop1]\nsent_queue_frames = 16 # k\nmax_frame_octets = 1024\n\n[link]\nmax_virtual_channels = 8\nmax_map_channels = 16\npacket_queue_octets = 2048\nmax_frame_octets = 1024\n\n[spp]\ntracker_max_apids = 16\n\n[tc]\nmax_maps = 64\n\
             [uslp]\nmax_vcs = 16\nmax_maps = 32\n",
        )
        .unwrap();
        assert_eq!(entries.len(), 10);
        let rs = capacities_rs(&entries).unwrap();
        assert!(rs.starts_with("// Generated by `cargo xtask header --write`"));
        assert!(rs.contains("pub const ALDIS_SPP_TRACKER_MAX_APIDS: usize = 16;\n"));
        assert!(rs.contains("pub const ALDIS_TC_MAX_MAPS: usize = 64;\n"));
        assert!(rs.contains("pub const ALDIS_USLP_MAX_VCS: usize = 16;\n"));
        assert!(rs.contains("pub const ALDIS_USLP_MAX_MAPS: usize = 32;\n"));
        assert!(rs.contains("pub const ALDIS_FOP1_SENT_QUEUE_FRAMES: usize = 16;\n"));
        assert!(rs.contains("pub const ALDIS_FOP1_MAX_FRAME_OCTETS: usize = 1024;\n"));
        assert!(rs.contains("pub const ALDIS_LINK_PACKET_QUEUE_OCTETS: usize = 2048;\n"));
    }

    // req: CABI-030
    #[test]
    fn bad_capacities_are_reported() {
        assert!(
            parse_toml("x = 1\n")
                .unwrap_err()
                .contains("outside a [section]")
        );
        assert!(
            parse_toml("[spp]\nx\n")
                .unwrap_err()
                .contains("key = value")
        );
        assert!(parse_toml("[spp]\nx = y\n").is_err());
        let missing = capacities_rs(&parse_toml("[spp]\n").unwrap()).unwrap_err();
        assert!(missing.contains("found 0"), "{missing}");
        let zero = capacities_rs(&parse_toml("[spp]\ntracker_max_apids = 0\n").unwrap());
        assert!(zero.unwrap_err().contains("outside 1..=2048"));
        let unknown = "[spp]\ntracker_max_apids = 4\nextra = 1\n[tc]\nmax_maps = 64\n[uslp]\nmax_vcs = 1\nmax_maps = 1\n[fop1]\nsent_queue_frames = 1\nmax_frame_octets = 6\n[link]\nmax_virtual_channels = 1\nmax_map_channels = 1\npacket_queue_octets = 64\nmax_frame_octets = 16\n";
        let e = capacities_rs(&parse_toml(unknown).unwrap()).unwrap_err();
        assert!(e.contains("unknown key [spp] extra"), "{e}");
    }

    // req: CABI-002, CABI-030
    #[test]
    fn a_stale_or_missing_generated_file_is_a_problem() {
        let fresh = "/* Generated */\n#define ALDIS_X 1\n";
        let from = "a fresh generation (CABI-002)";
        assert_eq!(stale(HEADER, Some(fresh), fresh, from), None);
        let old = stale(HEADER, Some("/* Generated */\n"), fresh, from).unwrap();
        assert_eq!(
            old,
            "include/aldis/aldis.h differs from a fresh generation (CABI-002); run \
             `cargo xtask header --write`"
        );
        assert!(stale(CAPACITIES_RS, None, fresh, CAPACITIES_TOML).is_some());
    }

    // req: CABI-003
    #[test]
    fn header_includes_are_checked() {
        let good = "#include <stdint.h>\n#include <stddef.h>\n#include <stdbool.h>\n\
                    #ifdef __cplusplus\nextern \"C\" {\n#endif\n";
        assert_eq!(header_problems(good), Vec::<String>::new());
        let bad = "#include <stdio.h>\n";
        let p = header_problems(bad);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p[0].contains("<stdio.h>") && p[1].contains("extern"));
    }

    // req: CABI-001
    #[test]
    fn native_libraries_come_from_the_note() {
        let note = "warning: x\nnote: native-static-libs: -lSystem -lc -lm\n";
        assert_eq!(native_static_libs(note), ["-lSystem", "-lc", "-lm"]);
        assert_eq!(native_static_libs("nothing"), Vec::<String>::new());
    }

    // req: CABI-002, VER-056
    #[test]
    fn the_image_installs_the_pinned_cbindgen() {
        let containerfile =
            std::fs::read_to_string(repo::root().join("xtask/verify/Containerfile")).unwrap();
        assert!(
            containerfile.contains(&format!("cbindgen@{CBINDGEN_VERSION}")),
            "the Containerfile must install cbindgen@{CBINDGEN_VERSION}"
        );
    }
}
