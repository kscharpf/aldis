//! Command-line entry point of the transport driver (`refapp/README.md`, §11.4).
//!
//! `refapp-interop run` steps the Rust flight node of a case against a UDP socket pair and the
//! monotonic clock, and writes the run log. The ground system is another process.
#![forbid(unsafe_code)]

use std::fs::File;
use std::io::BufWriter;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::process::ExitCode;

use refapp_interop::Error;
use refapp_interop::clock::Monotonic;
use refapp_interop::log::{Header, RunLog};
use refapp_interop::loss::LossScript;
use refapp_interop::port::UdpPort;
use refapp_interop::run::{downlink_slot, flight_node, load_case, run_flight};

const HELP: &str = "\
usage: cargo run -p refapp-interop -- run (from refapp/) <options>

options:
  --case <id>             the case whose level and end instant the run takes (required)
  --tm <host:port>        where the TM frames go: the ground system's TM port (required)
  --tc-listen <host:port> where the TC frames arrive: bind this address (required)
  --log <file>            the run log, JSON Lines (required; not a golden trace, not committed)
  --loss-script <id>      drop the units that the faults of this case's script drop (default:
                          none; a fault other than drop is refused)
  --drop <list>           drop the units of a list `dir:index,...` (`up:2,down:32`), in place of a
                          case's script (not with --loss-script; the indexes of §11.6)
  --ground-version <v>    recorded in the log header
  --image-digest <d>      recorded in the log header
  --node-commit <c>       recorded in the log header
";

#[derive(Default)]
struct Options {
    case: Option<String>,
    tm: Option<String>,
    tc_listen: Option<String>,
    log: Option<String>,
    loss_script: Option<String>,
    drop: Option<String>,
    ground_version: Option<String>,
    image_digest: Option<String>,
    node_commit: Option<String>,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let value = it
            .next()
            .ok_or_else(|| format!("{flag} needs a value"))?
            .clone();
        let slot = match flag.as_str() {
            "--case" => &mut o.case,
            "--tm" => &mut o.tm,
            "--tc-listen" => &mut o.tc_listen,
            "--log" => &mut o.log,
            "--loss-script" => &mut o.loss_script,
            "--drop" => &mut o.drop,
            "--ground-version" => &mut o.ground_version,
            "--image-digest" => &mut o.image_digest,
            "--node-commit" => &mut o.node_commit,
            other => return Err(format!("unknown option `{other}`")),
        };
        *slot = Some(value);
    }
    Ok(o)
}

fn address(text: &str) -> Result<SocketAddr, String> {
    text.to_socket_addrs()
        .map_err(|e| format!("{text}: {e}"))?
        .next()
        .ok_or_else(|| format!("{text}: no address"))
}

fn required<'a>(v: Option<&'a String>, name: &str) -> Result<&'a str, String> {
    v.map(String::as_str)
        .ok_or_else(|| format!("{name} is required"))
}

fn run(args: &[String]) -> Result<(), String> {
    let o = parse(args)?;
    let case_id = required(o.case.as_ref(), "--case")?;
    let tm = address(required(o.tm.as_ref(), "--tm")?)?;
    let tc = address(required(o.tc_listen.as_ref(), "--tc-listen")?)?;
    let log_path = required(o.log.as_ref(), "--log")?;
    let (scenario, case) = load_case(case_id).map_err(|e| e.to_string())?;
    let loss = match (&o.loss_script, &o.drop) {
        (Some(_), Some(_)) => return Err("--loss-script and --drop exclude each other".to_string()),
        (Some(id), None) => {
            let (_, lossy) = load_case(id).map_err(|e| e.to_string())?;
            LossScript::from_case(&lossy).map_err(|e| e.to_string())?
        }
        (None, Some(list)) => LossScript::from_spec(list).map_err(|e| e.to_string())?,
        (None, None) => LossScript::default(),
    };
    let mut flight = flight_node(case.level).map_err(|e| e.to_string())?;
    let slot = downlink_slot(&scenario).map_err(|e| e.to_string())?;
    let socket = UdpSocket::bind(tc).map_err(|e| format!("{tc}: {e}"))?;
    let mut port = UdpPort::new(socket, tm);
    let header = Header {
        case: case.id.clone(),
        ground_version: o.ground_version,
        image_digest: o.image_digest,
        node_commit: o.node_commit,
        loss_script: loss.clone(),
    };
    let file = File::create(log_path).map_err(|e| format!("{log_path}: {e}"))?;
    let mut log = RunLog::new(BufWriter::new(file), &header).map_err(|e| e.to_string())?;
    let mut clock = Monotonic::start();
    run_flight(
        &mut flight,
        slot,
        case.end_us,
        &loss,
        &mut clock,
        &mut port,
        &mut log,
    )
    .map_err(|e: Error| e.to_string())?;
    log.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((cmd, rest)) if cmd == "run" => run(rest),
        _ => {
            #[expect(clippy::print_stderr, reason = "the usage text of a command-line tool")]
            {
                eprint!("{HELP}");
            }
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            #[expect(clippy::print_stderr, reason = "the error of a command-line tool")]
            {
                eprintln!("error: {e}");
            }
            ExitCode::FAILURE
        }
    }
}
