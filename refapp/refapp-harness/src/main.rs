//! Command-line entry point of the reference-application harness (`refapp/README.md`).
#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use refapp_harness::scenario::{Scenario, generate, refapp_dir};
use refapp_harness::{
    Transport, demo, diagram, golden_path, launch, manifest, manifest_for, run_case, trace,
};

const HELP: &str = "\
usage: cargo run --manifest-path refapp/Cargo.toml -p refapp-harness -- <command>

commands:
  run <case> [--out <dir>]   run a case; write <case>.jsonl and <case>.manifest.json
                             (default directory: refapp/target/runs)
  run-node <case> [--out <dir>] [--component <name>=<version>]... <transport>
                             run a case with the flight node in another process (§8.2, §10.6);
                             the node's console goes to <case>.node.log. <transport> is
                               stdio <program> [<arg>...]   child over stdin and stdout
                               cfs <staging-dir> <port>     core-cpu1 of an external staging
                                                            directory, over loopback TCP
                             --component records a part of the node in the manifest (cFS
                             release, aldis-ffi build; APP-084)
  compare [<case>]           run a case (default: every case) and compare it byte for byte
                             with refapp/golden/<case>.jsonl
  bless <case>               write the run's trace to refapp/golden/<case>.jsonl (changing a
                             released trace needs a trace_version bump and a CHANGES.md row)
  check-golden               every file in refapp/golden/ is canonical and strictly decodes
  sequence-diagram [<case>]  print the Mermaid sequence diagram of the README, decoded from
                             refapp/golden/a3-uplink-loss.jsonl (APP-153)
  demo                       run the COP-1 lossy-link case a3-uplink-loss, print one line per
                             event decoded with the strict codecs, and the verdict of the
                             comparison with refapp/golden/a3-uplink-loss.jsonl (APP-154)
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

/// Options and transport of `run-node`.
struct NodeRun<'a> {
    case: &'a str,
    out: Option<&'a str>,
    components: Vec<(String, String)>,
    transport: &'a [&'a str],
}

fn parse_node_run<'a>(case: &'a str, mut rest: &'a [&'a str]) -> Result<NodeRun<'a>, String> {
    let mut run = NodeRun {
        case,
        out: None,
        components: Vec::new(),
        transport: &[],
    };
    loop {
        match rest {
            ["--out", dir, tail @ ..] => {
                run.out = Some(dir);
                rest = tail;
            }
            ["--component", kv, tail @ ..] => {
                let (k, v) = kv
                    .split_once('=')
                    .filter(|(k, v)| !k.is_empty() && !v.is_empty())
                    .ok_or_else(|| format!("--component wants <name>=<version>, not `{kv}`"))?;
                if run.components.iter().any(|(name, _)| name == k) {
                    return Err(format!("--component `{k}` given twice"));
                }
                run.components.push((k.to_owned(), v.to_owned()));
                rest = tail;
            }
            _ => {
                run.transport = rest;
                return Ok(run);
            }
        }
    }
}

fn run_node_cmd(run: &NodeRun<'_>) -> Result<usize, String> {
    let dir = run
        .out
        .map_or_else(|| refapp_dir().join("target/runs"), PathBuf::from);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let case = run.case;
    let log = dir.join(format!("{case}.node.log"));
    let (trace, implementation, transport) = match run.transport {
        ["stdio", program, args @ ..] => {
            let mut c = std::process::Command::new(program);
            c.args(args);
            (
                launch::run_stdio(case, c, &log)?,
                "external",
                Transport::Stdio,
            )
        }
        ["cfs", staging, port] => {
            let port: u16 = port
                .parse()
                .map_err(|_| format!("`{port}` is not a port"))?;
            let trace = launch::run_cfs(case, std::path::Path::new(staging), port, &log)?;
            (trace, "cfs", Transport::Tcp)
        }
        _ => return Err(format!("run-node: no transport given\n\n{HELP}")),
    };
    // Only a run that ended well reaches this point: an error leaves no trace (§8.2).
    let path = dir.join(format!("{case}.jsonl"));
    std::fs::write(&path, trace).map_err(|e| format!("{}: {e}", path.display()))?;
    let mpath = dir.join(format!("{case}.manifest.json"));
    std::fs::write(
        &mpath,
        manifest_for(case, implementation, transport, &run.components),
    )
    .map_err(|e| format!("{}: {e}", mpath.display()))?;
    println!(
        "wrote {}, {} and {}",
        path.display(),
        mpath.display(),
        log.display()
    );
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
    let scenario = Scenario::load()?;
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
        if let Err(e) = trace::decode_strict(&scenario, &text) {
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

fn sequence_diagram(case: &str) -> Result<usize, String> {
    let scenario = Scenario::load()?;
    let path = golden_path(case);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    print!("{}", diagram::render(&scenario, &text)?);
    Ok(0)
}

fn demo_cmd() -> Result<usize, String> {
    let scenario = Scenario::load()?;
    let case = diagram::CASE;
    let run = run_case(case)?;
    println!("{case}: COP-1 over a lossy link, decoded with the strict codecs");
    print!("{}", demo::render(&scenario, &run)?);
    let path = golden_path(case);
    let expected = std::fs::read_to_string(&path)
        .map_err(|e| format!("{}: {e} (bless the case first)", path.display()))?;
    let (text, matches) = demo::verdict(case, &expected, &run);
    println!("{text}");
    Ok(usize::from(!matches))
}

fn dispatch(args: &[&str]) -> Result<usize, String> {
    match args {
        ["run", case] => run_cmd(case, None),
        ["run", case, "--out", dir] => run_cmd(case, Some(dir)),
        ["run-node", case, rest @ ..] => run_node_cmd(&parse_node_run(case, rest)?),
        ["compare"] => compare_cmd(None),
        ["compare", case] => compare_cmd(Some(case)),
        ["bless", case] => bless_cmd(case),
        ["check-golden"] => check_golden(),
        ["sequence-diagram"] => sequence_diagram(diagram::CASE),
        ["sequence-diagram", case] => sequence_diagram(case),
        ["demo"] => demo_cmd(),
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
