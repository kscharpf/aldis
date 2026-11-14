//! A test node for the node protocol (docs/reference-application.md §8.2, §10.6): serves the
//! Rust flight or ground node of the case named in HELLO, out of process, so that the harness
//! side can be checked against a node that behaves as the in-process one (APP-092, APP-118).
//!
//! `refapp-test-node --stdio` speaks over stdin and stdout; `refapp-test-node --connect
//! <addr>` connects to the harness's listening TCP socket, as the cFS bridge does.
#![forbid(unsafe_code)]

use std::io::{stdin, stdout};
use std::net::TcpStream;
use std::process::ExitCode;

use refapp_harness::proto::Role;
use refapp_harness::scenario::Scenario;
use refapp_harness::serve::{Hello, serve};
use refapp_harness::{flight_node, ground_node};
use refapp_scenario::Node;

fn make(h: &Hello) -> Result<Box<dyn Node>, String> {
    let scenario = Scenario::load()?;
    if h.scenario_version != scenario.scenario_version {
        return Err(format!(
            "scenario version {} is not {}",
            h.scenario_version, scenario.scenario_version
        ));
    }
    let case = scenario.case(&h.case_id)?;
    Ok(match h.role {
        Role::Flight => Box::new(flight_node(case)?),
        Role::Ground => Box::new(ground_node(case)?),
    })
}

fn run(args: &[&str]) -> Result<(), String> {
    match args {
        ["--stdio"] => serve(&mut stdin().lock(), &mut stdout().lock(), make),
        ["--connect", addr] => {
            let stream = TcpStream::connect(addr).map_err(|e| format!("{addr}: {e}"))?;
            let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
            serve(&mut { stream }, &mut writer, make)
        }
        _ => Err("usage: refapp-test-node --stdio | --connect <addr>".to_owned()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("refapp-test-node: {e}");
            ExitCode::FAILURE
        }
    }
}
