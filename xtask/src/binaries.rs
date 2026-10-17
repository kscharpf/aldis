//! Binary-level checks: per-function stack usage (VER-011) and symbol inspection of a minimal
//! build (VER-031, GEN-003).

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;

use crate::repo;

/// Path to an LLVM tool from the pinned toolchain's `llvm-tools` component.
fn llvm_tool(name: &str) -> Result<PathBuf, String> {
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

/// `xtask stack-sizes <target>`: builds the exerciser with `-Z emit-stack-sizes` using the
/// pinned tooling nightly (RS-002) and writes `target/spec/stack-sizes-<target>.md`.
pub fn stack_sizes(target: &str) -> Result<usize, String> {
    let nightly = repo::tooling_nightly()?;
    let target_dir = repo::root().join("target/stack-sizes");
    run(
        Command::new("rustup")
            .args(["run", &nightly, "cargo", "rustc", "--release"])
            .args([
                "--manifest-path",
                "exerciser/Cargo.toml",
                "--target",
                target,
                "--",
                "-Z",
                "emit-stack-sizes",
            ])
            .env("CARGO_TARGET_DIR", &target_dir),
        "nightly exerciser build",
    )?;
    let elf = target_dir.join(target).join("release/exerciser");
    let report = run(
        Command::new(llvm_tool("llvm-readobj")?)
            .arg("--stack-sizes")
            .arg("--demangle")
            .arg(&elf),
        "llvm-readobj --stack-sizes",
    )?;

    let mut entries: Vec<(u64, String)> = Vec::new();
    let mut pending: Option<String> = None;
    for line in report.lines().map(str::trim) {
        if let Some(f) = line
            .strip_prefix("Functions: [")
            .and_then(|s| s.strip_suffix(']'))
        {
            pending = Some(f.to_owned());
        } else if let (Some(hex), Some(name)) = (line.strip_prefix("Size: 0x"), pending.take()) {
            let size = u64::from_str_radix(hex, 16).map_err(|e| format!("{line}: {e}"))?;
            entries.push((size, name));
        }
    }
    if entries.is_empty() {
        return Err("no stack size entries found".to_owned());
    }
    entries.sort_by(|a, b| b.cmp(a));

    let mut md = format!(
        "# Stack usage per function — exerciser, `{target}` (VER-011)\n\n\
         Frame sizes after LTO (functions inlined into callers are included in the caller).\n\
         Worst-case depth is the sum along the deepest call path; the exerciser calls at most\n\
         `_start` → one test group → one `aldis-core` API.\n\n\
         | Bytes | Function |\n|---:|---|\n"
    );
    for (size, name) in &entries {
        let _ = writeln!(md, "| {size} | `{name}` |");
    }
    let out = repo::root().join("target/spec");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let file = out.join(format!("stack-sizes-{target}.md"));
    std::fs::write(&file, md).map_err(|e| e.to_string())?;
    let max = entries.first().map_or(0, |e| e.0);
    println!(
        "stack-sizes: {} function(s), largest frame {max} bytes; report: {}",
        entries.len(),
        repo::rel(&file)
    );
    Ok(0)
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

/// `xtask symbols`: builds `aldis-spp` with all features and inspects the resulting `.rlib`
/// archives in `target/symbols/release/deps/` to verify that `aldis_core` symbols are present
/// and no frame-protocol crate or symbol is pulled in (VER-031, GEN-003).
pub fn symbols() -> Result<usize, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let target_dir = repo::root().join("target/symbols");
    if target_dir.exists() {
        std::fs::remove_dir_all(&target_dir)
            .map_err(|e| format!("{}: {e}", repo::rel(&target_dir)))?;
    }
    run(
        Command::new(cargo)
            .args(["build", "-p", "aldis-spp", "--release", "--all-features"])
            .env("CARGO_TARGET_DIR", &target_dir),
        "aldis-spp release build",
    )?;
    let deps_dir = target_dir.join("release/deps");
    let mut rlibs: Vec<PathBuf> = std::fs::read_dir(&deps_dir)
        .map_err(|e| format!("{}: {e}", repo::rel(&deps_dir)))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()) == Some("rlib")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("libaldis_"))
        })
        .collect();
    rlibs.sort();
    if rlibs.is_empty() {
        return Err("no libaldis_*.rlib archives produced for aldis-spp".to_owned());
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
    let core_hits = names.lines().filter(|l| l.contains("aldis_core::")).count();
    if core_hits == 0 {
        println!("error: VER-031 no aldis_core:: symbols found in aldis-spp rlib closure");
        problems += 1;
    }
    let total = names
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.ends_with(':'))
        .count();
    println!(
        "symbols: {total} defined symbol(s) across {} rlib(s) ({core_hits} aldis_core), {problems} problem(s)",
        rlibs.len()
    );
    Ok(problems)
}
