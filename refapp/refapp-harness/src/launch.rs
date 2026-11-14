//! Transports of the node protocol and the launcher (docs/reference-application.md §8.2, §10.6).
//!
//! ACFS plan WP-2: a child process over its stdin and stdout, and a child that
//! connects to a loopback TCP socket the harness listens on. The cFS launcher starts
//! `core-cpu1` in an external staging directory. The harness never reads a clock: waiting for
//! the connection polls the child's state, and a hung node is bounded by the job timeout.
//!
//! The node's console output goes to the run's log file, never to the trace.

use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use crate::proto::Role;
use crate::remote::{RemoteNode, run_remote};
use crate::scenario::Scenario;

/// File name of the cFS executable in its staging directory (`build-native_std/exe/cpu1/`,
/// ACFS plan F3).
pub const CFS_EXECUTABLE: &str = "core-cpu1";

/// Pause between looks at the child while waiting for its connection. A pause is not a read
/// of the clock.
const POLL: Duration = Duration::from_millis(10);

/// A child process that is killed and reaped when it goes out of scope.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        // The process may already be gone; there is no one to tell.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn log_file(log: &Path) -> Result<File, String> {
    File::create(log).map_err(|e| format!("{}: {e}", log.display()))
}

fn run_with<R: Read, W: Write>(case_id: &str, reader: R, writer: W) -> Result<String, String> {
    let scenario = Scenario::load()?;
    scenario.case(case_id)?;
    let mut node = RemoteNode::connect(
        reader,
        writer,
        Role::Flight,
        scenario.scenario_version,
        case_id,
    )?;
    run_remote(case_id, &mut node)
}

/// Runs a case with the flight node as `command`, a child that speaks the node protocol on its
/// stdin and stdout (§8.2). Its stderr goes to `log`.
///
/// # Errors
/// If the child does not start, the protocol breaks, or the run fails: no trace.
pub fn run_stdio(case_id: &str, mut command: Command, log: &Path) -> Result<String, String> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log_file(log)?))
        .spawn()
        .map_err(|e| format!("starting the node: {e}"))?;
    let (Some(stdout), Some(stdin)) = (child.stdout.take(), child.stdin.take()) else {
        return Err("the node's pipes are not open".to_owned());
    };
    let _guard = ChildGuard(child);
    run_with(case_id, stdout, stdin)
}

/// Runs a case with the flight node as a child that connects to a loopback TCP socket the
/// harness listens on (§10.6).
///
/// `port` 0 takes a free port; `command` builds the child from the
/// address it must connect to. The harness accepts one connection, and ends the child after
/// `END_ACK`. The child's stdout and stderr go to `log`.
///
/// # Errors
/// If the socket or the child does not start, the child ends before it connects, the protocol
/// breaks, or the run fails: no trace.
pub fn run_tcp(
    case_id: &str,
    port: u16,
    command: impl FnOnce(SocketAddr) -> Command,
    log: &Path,
) -> Result<String, String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .map_err(|e| format!("listening on 127.0.0.1:{port}: {e}"))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("socket: {e}"))?;
    let out = log_file(log)?;
    let err = out
        .try_clone()
        .map_err(|e| format!("{}: {e}", log.display()))?;
    let child = command(addr)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn()
        .map_err(|e| format!("starting the node: {e}"))?;
    let mut guard = ChildGuard(child);
    let stream = accept(&listener, &mut guard.0)?;
    let reader = stream.try_clone().map_err(|e| format!("socket: {e}"))?;
    run_with(case_id, reader, stream)
}

/// Runs a case with `core-cpu1` of the staging directory `staging` as the flight node,
/// listening on `port` (the port the bridge's platform configuration fixes, ACFS D9).
///
/// # Errors
/// If `staging` holds no `core-cpu1`, or as [`run_tcp`].
pub fn run_cfs(case_id: &str, staging: &Path, port: u16, log: &Path) -> Result<String, String> {
    run_cfs_bound(case_id, staging, port, log, |_| ())
}

/// As [`run_cfs`], and calls `bound` with the port actually listened on (useful with port 0)
/// after the socket is open and before `core-cpu1` starts, so a caller can pass the port on
/// without a race.
///
/// # Errors
/// As [`run_cfs`].
pub fn run_cfs_bound(
    case_id: &str,
    staging: &Path,
    port: u16,
    log: &Path,
    bound: impl FnOnce(u16),
) -> Result<String, String> {
    let exe = staging.join(CFS_EXECUTABLE);
    if !exe.is_file() {
        return Err(format!(
            "{}: no `{CFS_EXECUTABLE}` there",
            staging.display()
        ));
    }
    run_tcp(
        case_id,
        port,
        |addr| {
            bound(addr.port());
            let mut c = Command::new(exe);
            c.current_dir(staging);
            c
        },
        log,
    )
}

/// Waits for the one connection, watching that the child is still there.
fn accept(listener: &TcpListener, child: &mut Child) -> Result<TcpStream, String> {
    // bound: ends when the child connects or exits; a child that does neither is ended by the
    // job timeout (§8.2).
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_nonblocking(false)
                    .map_err(|e| format!("socket: {e}"))?;
                return Ok(stream);
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if let Some(status) = child.try_wait().map_err(|e| format!("node: {e}"))? {
                    return Err(format!("the node ended ({status}) before it connected"));
                }
                sleep(POLL);
            }
            Err(e) => return Err(format!("accepting the node: {e}")),
        }
    }
}
