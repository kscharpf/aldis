//! The self-test of the driver (plan D8 a, APP-106): the Rust flight node and the Rust ground
//! node, each stepped by a [`Side`](crate::side::Side) through its own UDP socket on loopback,
//! in virtual time.
//!
//! The sockets are real; the clock is [`Manual`], so a run takes no real time but the loopback
//! calls. A datagram leaves its socket at once and is handed to the other node after the
//! scenario's one-way delay in virtual time, in the tie order of §6.2, so that the schedule is
//! the one of the golden traces.

use std::net::UdpSocket;
use std::time::Duration;

use refapp_ground::GroundNode;
use refapp_harness::micros::Micros;
use refapp_harness::scenario::{Level, OpsEntry};
use refapp_harness::trace::Dir;

use crate::Error;
use crate::clock::{Clock, Manual};
use crate::log::Sink;
use crate::loss::LossScript;
use crate::port::{Port, UdpPort};
use crate::run::{downlink_slot, flight_node, load_case};
use crate::side::{Role, Side};

/// How long a loopback datagram may take to show up in real time; it is in the socket buffer
/// when `send` returns, so this is a bound for a failure, not a wait.
const LOOPBACK_WAIT: Duration = Duration::from_secs(5);

/// Runs the case `case_id` with both Rust nodes over loopback UDP in virtual time.
///
/// `loss` is applied as the flight side does against a real ground system. The flight side's
/// datagram records and both nodes' events go to `sink`.
///
/// # Errors
/// If the case cannot be driven, a node breaks the step contract, or a loopback datagram does
/// not arrive.
pub fn run_pair(case_id: &str, loss: &LossScript, sink: &mut dyn Sink) -> Result<(), Error> {
    let (scenario, case) = load_case(case_id)?;
    let mut flight = flight_node(case.level)?;
    let script = case
        .ops
        .iter()
        .map(OpsEntry::to_node)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Error::Script)?;
    let mut ground = match case.level {
        Level::Cop1 => GroundNode::new_cop1(&script),
        Level::Frames => GroundNode::new_frames(&script),
        Level::Packets => GroundNode::new(&script),
    }
    .map_err(|e| Error::Script(format!("ground node: {e}")))?;
    let ops_us: Vec<Micros> = case.ops.iter().map(OpsEntry::at_us).collect();
    let uplink_slot = std::num::NonZeroU64::new(scenario.link.uplink_slot_us.get().get())
        .ok_or_else(|| Error::Script("uplink slot of 0 µs".to_owned()))?;

    let flight_socket = UdpSocket::bind("127.0.0.1:0")?;
    let ground_socket = UdpSocket::bind("127.0.0.1:0")?;
    let (flight_addr, ground_addr) = (flight_socket.local_addr()?, ground_socket.local_addr()?);
    let mut flight_port = UdpPort::new(flight_socket, ground_addr);
    let mut ground_port = UdpPort::new(ground_socket, flight_addr);

    let mut flight_side = Side::new(
        Role::Flight,
        &mut flight,
        downlink_slot(&scenario)?,
        Vec::new(),
    );
    let mut ground_side = Side::new(Role::Ground, &mut ground, uplink_slot, ops_us);
    let mut clock = Manual::new();
    let none = LossScript::default();
    let end = case.end_us;
    let delay = scenario.link.one_way_delay_us;
    // Datagrams that have left a socket and are on their way: arrival instant, send instant,
    // direction, number, octets. The delay is the scenario's, so that the schedule is the one
    // of the golden traces (§6.1); the transport itself delays nothing.
    let mut in_flight: Vec<(Micros, Micros, Dir, u64, Vec<u8>)> = Vec::new();
    let mut number = 0_u64;

    while clock.now() < end {
        let t = clock.now();
        // §6.2: arrivals first in tie order, then the ground node steps, then the flight node.
        in_flight.sort_by_key(|a| (a.0, a.1, a.2, a.3));
        let due = in_flight.iter().take_while(|a| a.0 == t).count();
        let mut to_ground: Vec<Vec<u8>> = Vec::new();
        let mut to_flight: Vec<Vec<u8>> = Vec::new();
        for (_, _, dir, _, octets) in in_flight.drain(..due) {
            match dir {
                Dir::Down => to_ground.push(octets),
                Dir::Up => {
                    if let Some(o) = flight_side.receive(t, t, octets, loss, sink)? {
                        to_flight.push(o);
                    }
                }
            }
        }
        let units = ground_side.step(t, t, &to_ground, sink)?;
        let sent = ground_side.transmit(t, t, units, &mut ground_port, &none, sink)?;
        for _ in 0..sent {
            number = number.saturating_add(1);
            let octets = recv_now(&mut flight_port)?;
            in_flight.push((t.saturating_add(delay), t, Dir::Up, number, octets));
        }
        let units = flight_side.step(t, t, &to_flight, sink)?;
        let sent = flight_side.transmit(t, t, units, &mut flight_port, loss, sink)?;
        for _ in 0..sent {
            number = number.saturating_add(1);
            let octets = recv_now(&mut ground_port)?;
            in_flight.push((t.saturating_add(delay), t, Dir::Down, number, octets));
        }
        let next = [
            flight_side.next_wake(),
            ground_side.next_wake(),
            in_flight.iter().map(|a| a.0).min().unwrap_or(end),
            end,
        ]
        .into_iter()
        .min()
        .unwrap_or(end);
        if next <= t {
            return Err(Error::Contract(format!(
                "no progress at t = {t} µs: the next wake is {next} µs"
            )));
        }
        clock.advance_to(next);
    }
    Ok(())
}

/// The datagram that a send has just put on loopback.
fn recv_now(port: &mut UdpPort) -> Result<Vec<u8>, Error> {
    port.recv(LOOPBACK_WAIT)?
        .ok_or_else(|| Error::Contract("a loopback datagram did not arrive".to_owned()))
}
