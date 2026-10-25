//! Binary-level checks: symbol inspection of a minimal build (VER-031, GEN-003) and of the
//! bare-metal C ABI library (CABI-004). The stack bound of VER-011 is `stack.rs`.

use std::path::PathBuf;
use std::process::Command;

use crate::repo;

/// Path to an LLVM tool from the pinned toolchain's `llvm-tools` component.
pub fn llvm_tool(name: &str) -> Result<PathBuf, String> {
    let rustc = |args: &[&str]| -> Result<String, String> {
        let out = Command::new("rustc")
            .args(args)
            .current_dir(repo::root())
            .output()
            .map_err(|e| format!("rustc: {e}"))?;
        String::from_utf8(out.stdout).map_err(|e| e.to_string())
    };
    let sysroot = rustc(&["--print", "sysroot"])?;
    let host = rustc(&["-vV"])?
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(str::to_owned)
        .ok_or("rustc -vV: no host line")?;
    let path = PathBuf::from(sysroot.trim())
        .join("lib/rustlib")
        .join(host)
        .join("bin")
        .join(name);
    if path.exists() {
        Ok(path)
    } else {
        Err(format!(
            "{} not found; install it with `rustup component add llvm-tools`",
            path.display()
        ))
    }
}

fn run(cmd: &mut Command, what: &str) -> Result<String, String> {
    let out = cmd
        .current_dir(repo::root())
        .output()
        .map_err(|e| format!("{what}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{what} failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    String::from_utf8(out.stdout).map_err(|e| e.to_string())
}

/// Crates whose code must not appear in a build of `aldis-spp` alone (GEN-003).
const FRAME_CRATES: &[&str] = &[
    "aldis_tm",
    "aldis_tc",
    "aldis_aos",
    "aldis_uslp",
    "aldis_link",
    "aldis_cop1",
];

/// `xtask symbols`: builds `aldis-spp` with all features, collects the `.rlib` archives
/// emitted by `cargo build --message-format=json-render-diagnostics`, and checks that no
/// frame-protocol archive or symbol (`llvm-nm`) is present (VER-031, GEN-003).
pub fn symbols() -> Result<usize, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let target_dir = repo::root().join("target/symbols");
    let stream = run(
        Command::new(cargo)
            .args([
                "build",
                "-p",
                "aldis-spp",
                "--release",
                "--all-features",
                "--message-format=json-render-diagnostics",
            ])
            .env("CARGO_TARGET_DIR", &target_dir),
        "aldis-spp release build",
    )?;
    let mut rlibs: Vec<PathBuf> = Vec::new();
    for line in stream.lines() {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if msg.get("reason").and_then(serde_json::Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        for file in msg
            .get("filenames")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
        {
            let path = PathBuf::from(file);
            if path.extension().and_then(|e| e.to_str()) == Some("rlib")
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("libaldis_"))
            {
                rlibs.push(path);
            }
        }
    }
    if rlibs.is_empty() {
        return Err("no libaldis_*.rlib artifacts reported by cargo build".to_owned());
    }
    let mut cmd = Command::new(llvm_tool("llvm-nm")?);
    cmd.args(["--demangle", "--defined-only"]).args(&rlibs);
    let names = run(&mut cmd, "llvm-nm")?;
    let mut problems = 0;
    for krate in FRAME_CRATES {
        let rlib_prefix = format!("lib{krate}-");
        let rlib_hit = rlibs.iter().any(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&rlib_prefix))
        });
        let hits = names
            .lines()
            .filter(|l| l.contains(&format!("{krate}::")))
            .count();
        if rlib_hit || hits > 0 {
            println!(
                "error: GEN-003 {krate} present in a build of aldis-spp alone ({hits} symbol(s))"
            );
            problems += 1;
        }
    }
    let total = names
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.ends_with(':'))
        .count();
    println!(
        "symbols: {total} defined symbol(s) across {} rlib(s), {problems} frame-protocol crate(s) found",
        rlibs.len()
    );
    Ok(problems)
}

/// The symbols a bare-metal static library may leave to the integrator (CABI-004): the C
/// library's memory functions, which a compiler may also call on its own.
const FFI_IMPORTS_ALLOWED: &[&str] = &["memcpy", "memmove", "memset", "memcmp", "bcmp"];

/// The names of `llvm-nm` output lines: the last word of each symbol line.
fn nm_names(listing: &str) -> std::collections::BTreeSet<String> {
    listing
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.ends_with(':'))
        .filter_map(|l| l.split_whitespace().last())
        .map(str::to_owned)
        .collect()
}

/// The symbols `undefined` names that `defined` does not, less [`FFI_IMPORTS_ALLOWED`].
fn foreign_imports(defined: &str, undefined: &str) -> Vec<String> {
    let defined = nm_names(defined);
    nm_names(undefined)
        .into_iter()
        .filter(|s| !defined.contains(s) && !FFI_IMPORTS_ALLOWED.contains(&s.as_str()))
        .collect()
}

/// `xtask ffi-imports <target>`: the `no_std` static library `aldis-ffi` builds for a
/// bare-metal target (`--no-default-features --features panic-handler --release`) needs
/// nothing from the integrator beyond `memcpy`/`memset` equivalents: no heap, threads, or C
/// runtime (CABI-004). Expects the library built.
pub fn ffi_imports(target: &str) -> Result<usize, String> {
    // Cargo puts the archive under `CARGO_TARGET_DIR` when it is set (relative to the
    // repository root if relative), else under `target/`.
    let target_dir = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || repo::root().join("target"),
        |dir| repo::root().join(std::path::PathBuf::from(dir)),
    );
    let lib = target_dir.join(target).join("release/libaldis_ffi.a");
    if !lib.is_file() {
        return Err(format!(
            "{} not found; build it with `cargo build -p aldis-ffi --no-default-features \
             --features panic-handler --target {target} --release`",
            repo::rel(&lib)
        ));
    }
    let nm = llvm_tool("llvm-nm")?;
    let defined = run(Command::new(&nm).arg("--defined-only").arg(&lib), "llvm-nm")?;
    let undefined = run(
        Command::new(&nm).arg("--undefined-only").arg(&lib),
        "llvm-nm",
    )?;
    let foreign = foreign_imports(&defined, &undefined);
    for s in &foreign {
        println!(
            "error: CABI-004 {} needs `{s}` from the integrator",
            repo::rel(&lib)
        );
    }
    println!(
        "ffi-imports {target}: {} symbol(s) needed from outside the library beyond {}",
        foreign.len(),
        FFI_IMPORTS_ALLOWED.join(", ")
    );
    Ok(foreign.len())
}

#[cfg(test)]
mod tests {
    use super::foreign_imports;

    // req: CABI-004
    #[test]
    fn only_memory_functions_may_be_imported() {
        let defined = "a.o:\n00000000 T aldis_spp_decode\n00000010 T helper\n";
        let undefined = "a.o:\n         U helper\n         U memcpy\n         U malloc\n";
        assert_eq!(foreign_imports(defined, undefined), ["malloc"]);
        assert_eq!(
            foreign_imports(defined, "b.o:\n U memset\n"),
            Vec::<String>::new()
        );
    }
}
