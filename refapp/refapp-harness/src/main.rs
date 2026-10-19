//! Command-line entry point of the reference-application harness (`refapp/README.md`).
#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use refapp_harness::scenario::{Scenario, generate, refapp_dir};
use refapp_harness::{golden_path, manifest, run_case, trace};

const HELP: &str = "\
usage: cargo run -p refapp-harness -- <command>

commands:
  run <case> [--out <dir>]   run a case; write <case>.jsonl and <case>.manifest.json
                             (default directory: refapp/target/runs)
  compare [<case>]           run a case (default: every case) and compare it byte for byte
                             with refapp/golden/<case>.jsonl
  bless <case>               write the run's trace to refapp/golden/<case>.jsonl; bump the
                             case's trace_version and add a line to refapp/golden/CHANGES.md
  check-golden               every file in refapp/golden/ is a canonical trace
  gen-scenario [--check]     (re)generate the schema and Rust constants from
                             refapp/scenario/scenario.json, or check that they are current
";

fn run_cmd(case: &str, out: Option<&str>) -> Result<usize, String> {
    let dir = out.map_or_else(|| refapp_dir().join("target/runs"), PathBuf::from);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let trace = run_case(case)?;
    let path = dir.join(format!("{case}.jsonl"));
    std::fs::write(&path, trace).map_err(|e| format!("{}: {e}", path.display()))?;
    let mpath = dir.join(format!("{case}.manifest.json"));
    std::fs::write(&mpath, manifest(case)).map_err(|e| format!("{}: {e}", mpath.display()))?;
    println!("wrote {} and {}", path.display(), mpath.display());
    Ok(0)
}

fn compare_cmd(case: Option<&str>) -> Result<usize, String> {
    let scenario = Scenario::load()?;
    let ids: Vec<String> = match case {
        Some(c) => vec![scenario.case(c)?.id.clone()],
        None => scenario.cases.iter().map(|c| c.id.clone()).collect(),
    };
    let mut failed = 0;
    for id in ids {
        let path = golden_path(&id);
        let expected = std::fs::read_to_string(&path)
            .map_err(|e| format!("{}: {e} (bless the case first)", path.display()))?;
        match trace::compare(&expected, &run_case(&id)?) {
            Ok(()) => println!("{id}: matches refapp/golden/{id}.jsonl"),
            Err(m) => {
                println!("{id}: differs from refapp/golden/{id}.jsonl\n{m}");
                failed += 1;
            }
        }
    }
    Ok(failed)
}

fn bless_cmd(case: &str) -> Result<usize, String> {
    let trace = run_case(case)?;
    trace::check_canonical(&trace)?;
    let path = golden_path(case);
    std::fs::write(&path, trace).map_err(|e| format!("{}: {e}", path.display()))?;
    println!(
        "wrote refapp/golden/{case}.jsonl; if it was released, bump trace_version in \
         scenario.json and record the reason in refapp/golden/CHANGES.md (APP-087)"
    );
    Ok(0)
}

fn check_golden() -> Result<usize, String> {
    let dir = refapp_dir().join("golden");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    files.sort();
    let mut failed = 0;
    for f in &files {
        let text = std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
        if let Err(e) = trace::check_canonical(&text) {
            println!("error: {}: {e}", f.display());
            failed += 1;
        }
    }
    println!(
        "check-golden: {} trace(s), {failed} problem(s)",
        files.len()
    );
    Ok(failed)
}

fn dispatch(args: &[&str]) -> Result<usize, String> {
    match args {
        ["run", case] => run_cmd(case, None),
        ["run", case, "--out", dir] => run_cmd(case, Some(dir)),
        ["compare"] => compare_cmd(None),
        ["compare", case] => compare_cmd(Some(case)),
        ["bless", case] => bless_cmd(case),
        ["check-golden"] => check_golden(),
        ["gen-scenario"] => generate(false),
        ["gen-scenario", "--check"] => generate(true),
        [] | ["help" | "-h" | "--help"] => {
            print!("{HELP}");
            Ok(0)
        }
        _ => Err(format!("unknown command: {}\n\n{HELP}", args.join(" "))),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match dispatch(&args) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(n) => {
            eprintln!("refapp-harness: {n} problem(s)");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("refapp-harness: {e}");
            ExitCode::FAILURE
        }
    }
}
