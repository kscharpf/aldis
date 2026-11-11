//! The step loop of the flight node against a port and a clock (APP-157, APP-158, plan D5 a).

use std::num::NonZeroU64;
use std::time::Duration;

use refapp_flight::FlightNode;
use refapp_harness::micros::Micros;
use refapp_harness::scenario::{Case, Level, Scenario};
use refapp_scenario::Node;

use crate::Error;
use crate::clock::Clock;
use crate::log::Sink;
use crate::loss::LossScript;
use crate::port::Port;
use crate::side::{Role, Side};

/// The flight node of a case's link level (as the harness builds it, APP-071, APP-044).
///
/// # Errors
/// If the level is the packet level, which has no frames to send, or the node cannot be built.
pub fn flight_node(level: Level) -> Result<FlightNode, Error> {
    let node = match level {
        Level::Packets => {
            return Err(Error::Script(
                "the driver carries transfer frames: a case needs link level frames or cop1"
                    .to_owned(),
            ));
        }
        Level::Frames => FlightNode::new_frames(),
        Level::Cop1 => FlightNode::new_cop1(),
    };
    node.map_err(|e| Error::Script(format!("flight node: {e:?}")))
}

/// The downlink slot of `scenario` as a divisor.
///
/// # Errors
/// If the slot is 0 µs, which the scenario loader rules out.
pub fn downlink_slot(scenario: &Scenario) -> Result<NonZeroU64, Error> {
    NonZeroU64::new(scenario.link.frame_slot_us.get().get())
        .ok_or_else(|| Error::Script("downlink slot of 0 µs".to_owned()))
}

/// What woke the loop.
enum Wake {
    /// The instant `target` was reached.
    Due,
    /// A datagram arrived.
    Datagram(Vec<u8>),
}

/// Waits for a datagram or until `target`. A virtual clock is advanced to `target` when nothing
/// arrived, so the loop never waits in real time then.
fn wait(clock: &mut dyn Clock, port: &mut dyn Port, target: Micros) -> Result<Wake, Error> {
    loop {
        let now = clock.now();
        if clock.passes_by_itself() {
            if now >= target {
                return Ok(Wake::Due);
            }
            let wait = Duration::from_micros(target.get().saturating_sub(now.get()));
            if let Some(d) = port.recv(wait)? {
                return Ok(Wake::Datagram(d));
            }
        } else {
            if let Some(d) = port.recv(Duration::ZERO)? {
                return Ok(Wake::Datagram(d));
            }
            clock.advance_to(target);
            return Ok(Wake::Due);
        }
    }
}

/// Runs the flight node until `end` (exclusive).
///
/// `unit` outputs go to the port as datagrams, the datagrams that arrive are `delivered`, and
/// the next step is at the earliest of the node's deadline, the next downlink slot, and an
/// arriving datagram (APP-157); a datagram that arrives after an instant has passed is delivered
/// after the step of that instant. `tx_slot` is true at every slot of `slot` µs and in no other
/// step (APP-158).
///
/// # Errors
/// If the node breaks the step contract, or the port, the clock's wait, or the sink fails.
pub fn run_flight(
    flight: &mut dyn Node,
    slot: NonZeroU64,
    end: Micros,
    loss: &LossScript,
    clock: &mut dyn Clock,
    port: &mut dyn Port,
    sink: &mut dyn Sink,
) -> Result<(), Error> {
    let mut side = Side::new(Role::Flight, flight, slot, Vec::new());
    let mut last = Micros::new(0);
    while clock.now() < end {
        let target = side.next_wake().min(end);
        let (now, delivered) = match wait(clock, port, target)? {
            // A step that an instant caused is taken at that instant, not at the clock's later
            // reading: the node starts its fields at the step that generates the packet, so a
            // late reading would put every release deadline between slots, and a release only
            // happens in a `tx_slot` step (§4.4.2). `last` keeps the instants from going back.
            Wake::Due => (target.max(last), Vec::new()),
            Wake::Datagram(octets) => {
                let now = clock.now().max(last);
                // An instant that passed before the datagram was seen is served first, at
                // its own instant; the datagram's step then falls in no slot.
                while side.next_wake() <= now && side.next_wake() < end {
                    let at = side.next_wake().max(last);
                    last = at;
                    let units = side.step(at, &[], sink)?;
                    side.transmit(at, units, port, loss, sink)?;
                }
                match side.receive(now, octets, loss, sink)? {
                    Some(octets) => (now, vec![octets]),
                    None => continue,
                }
            }
        };
        last = now;
        if now >= end {
            break;
        }
        let units = side.step(now, &delivered, sink)?;
        side.transmit(now, units, port, loss, sink)?;
    }
    Ok(())
}

/// The scenario's case, for the binary.
///
/// # Errors
/// If the case does not exist.
pub fn load_case(case_id: &str) -> Result<(Scenario, Case), Error> {
    let scenario = Scenario::load().map_err(Error::Script)?;
    let case = scenario.case(case_id).map_err(Error::Script)?.clone();
    Ok((scenario, case))
}
