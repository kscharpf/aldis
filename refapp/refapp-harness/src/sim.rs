//! Scheduler and link model (docs/reference-application.md §6).
//!
//! The harness owns simulated time and the link (APP-070). It advances to the earliest of the
//! slots, link arrivals, and node deadlines, and processes each instant in the order of §6.2
//! (APP-073). Ops-script entries arrive in A1 and faults in A2; until then the inputs and the
//! fault script are empty.

use refapp_scenario::{Apid, Instant, Node, Output, StepInput};

use crate::scenario::{Case, Level, Link, Scenario};
use crate::trace::{Dir, Header, NodeId, Record};

/// One output of a node step, in output order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Out {
    Unit(Vec<u8>),
    Generated(u16, Vec<u8>),
    Delivered(u16, Vec<u8>),
    Event(u16, Vec<u64>),
}

#[derive(Default)]
struct Recorder(Vec<Out>);

impl Output for Recorder {
    fn unit(&mut self, octets: &[u8]) {
        self.0.push(Out::Unit(octets.to_vec()));
    }
    fn packet_out(&mut self, apid: Apid, octets: &[u8]) {
        self.0.push(Out::Generated(apid.get(), octets.to_vec()));
    }
    fn packet_in(&mut self, apid: Apid, octets: &[u8]) {
        self.0.push(Out::Delivered(apid.get(), octets.to_vec()));
    }
    fn app_event(&mut self, code: u16, args: &[u64]) {
        self.0.push(Out::Event(code, args.to_vec()));
    }
}

/// A unit on its way through the link.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Arrival {
    at: u64,
    sent_at: u64,
    dir: Dir,
    index: u64,
    copy: u8,
    octets: Vec<u8>,
}

impl Arrival {
    /// Arrival instant, then the tie order of §6.2 step 1.
    const fn order(&self) -> (u64, u64, Dir, u64, u8) {
        (self.at, self.sent_at, self.dir, self.index, self.copy)
    }
}

/// Per-node scheduling state.
struct Side<'n> {
    id: NodeId,
    node: &'n mut dyn Node,
    deadline: Option<u64>,
    /// Direction of the units this node sends.
    tx_dir: Dir,
    /// Slot period of that direction (A2 on).
    slot_us: u64,
    next_index: u64,
}

/// State shared by both nodes' steps.
struct World<'a> {
    level: Level,
    link: &'a Link,
    in_flight: Vec<Arrival>,
    records: Vec<Record>,
}

/// Next multiple of `period` strictly after `t`.
fn next_slot(t: u64, period: u64) -> Option<u64> {
    t.checked_div(period)?.checked_add(1)?.checked_mul(period)
}

/// Runs a case with the given nodes and returns the trace (§7).
///
/// # Errors
/// If a node breaks the step contract (APP-090: `tx_slot` rules, a deadline not after `now`),
/// or raises an application event outside §7.4.
pub fn run(
    scenario: &Scenario,
    case: &Case,
    flight: &mut dyn Node,
    ground: &mut dyn Node,
) -> Result<String, String> {
    let header = Header {
        scenario: scenario.scenario.clone(),
        scenario_version: scenario.scenario_version,
        case: case.id.clone(),
        trace_version: case.trace_version,
        requirements: case.requirements.clone(),
    };
    let mut trace = header.to_line();
    for r in records(scenario, case, flight, ground)? {
        trace.push_str(&r.to_line()?);
    }
    Ok(trace)
}

/// Runs a case and returns its records, without the header.
///
/// # Errors
/// As [`run`].
pub fn records(
    scenario: &Scenario,
    case: &Case,
    flight: &mut dyn Node,
    ground: &mut dyn Node,
) -> Result<Vec<Record>, String> {
    let link = &scenario.link;
    let mut ground = Side {
        id: NodeId::Ground,
        node: ground,
        deadline: None,
        tx_dir: Dir::Up,
        slot_us: link.uplink_slot_us,
        next_index: 0,
    };
    let mut flight = Side {
        id: NodeId::Flight,
        node: flight,
        deadline: None,
        tx_dir: Dir::Down,
        slot_us: link.frame_slot_us,
        next_index: 0,
    };
    let mut world = World {
        level: case.level,
        link,
        in_flight: Vec::new(),
        records: Vec::new(),
    };
    let mut t = 0;
    while t < case.end_us {
        let (to_ground, to_flight) = deliver(&mut world, t);
        // Steps 2 and 3 of §6.2: the ground node, then the flight node.
        step(&mut world, &mut ground, t, &to_ground)?;
        step(&mut world, &mut flight, t, &to_flight)?;
        // Advance to the earliest next instant.
        let mut next = [
            world.in_flight.iter().map(|a| a.at).min(),
            ground.deadline,
            flight.deadline,
        ]
        .into_iter()
        .flatten()
        .min();
        if case.level == Level::Frames {
            for period in [link.frame_slot_us, link.uplink_slot_us] {
                next = next.into_iter().chain(next_slot(t, period)).min();
            }
        }
        match next {
            Some(n) => t = n,
            None => break,
        }
    }
    Ok(world.records)
}

/// Step 1 of §6.2: delivers the arrivals due at `t`, in tie order. Returns the units for the
/// ground node and for the flight node.
fn deliver(world: &mut World<'_>, t: u64) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    world.in_flight.sort_by_key(Arrival::order);
    let due = world.in_flight.iter().take_while(|a| a.at == t).count();
    let mut to_ground = Vec::new();
    let mut to_flight = Vec::new();
    for a in world.in_flight.drain(..due) {
        world.records.push(Record::UnitDelivered {
            t_us: t,
            dir: a.dir,
            index: a.index,
            copy: a.copy,
        });
        match a.dir {
            Dir::Down => to_ground.push(a.octets),
            Dir::Up => to_flight.push(a.octets),
        }
    }
    (to_ground, to_flight)
}

/// Steps one node at `t` if anything is due for it, and records its outputs (§6.2).
fn step(
    world: &mut World<'_>,
    side: &mut Side<'_>,
    t: u64,
    delivered: &[Vec<u8>],
) -> Result<(), String> {
    let slot = match world.level {
        Level::Packets => true,
        Level::Frames => t.checked_rem(side.slot_us) == Some(0),
    };
    let due = t == 0 || !delivered.is_empty() || slot || side.deadline.is_some_and(|d| d <= t);
    if !due {
        return Ok(());
    }
    let units: Vec<&[u8]> = delivered.iter().map(Vec::as_slice).collect();
    let input = StepInput {
        now: Instant::from_micros(t),
        delivered: &units,
        ops: &[],
        tx_slot: slot,
    };
    let mut out = Recorder::default();
    let deadline = side.node.step(&input, &mut out).map(Instant::as_micros);
    let name = side.id.as_str();
    if let Some(d) = deadline.filter(|d| *d <= t) {
        return Err(format!(
            "{name} node at t = {t} µs asked for a deadline {d} µs not after now"
        ));
    }
    side.deadline = deadline;
    let unit_count = out.0.iter().filter(|o| matches!(o, Out::Unit(_))).count();
    let allowed = match (world.level, side.id, slot) {
        (Level::Packets, _, _) => true,
        (Level::Frames, NodeId::Flight, true) => unit_count == 1,
        (Level::Frames, NodeId::Ground, true) => unit_count <= 1,
        (Level::Frames, _, false) => unit_count == 0,
    };
    if !allowed {
        return Err(format!(
            "{name} node at t = {t} µs output {unit_count} unit(s), against the tx_slot rules of §8.1"
        ));
    }
    for o in out.0 {
        let record = match o {
            Out::Unit(octets) => {
                let index = side.next_index;
                side.next_index += 1;
                world.in_flight.push(Arrival {
                    at: t.saturating_add(world.link.one_way_delay_us),
                    sent_at: t,
                    dir: side.tx_dir,
                    index,
                    copy: 0,
                    octets: octets.clone(),
                });
                Record::UnitSent {
                    t_us: t,
                    dir: side.tx_dir,
                    index,
                    octets,
                }
            }
            Out::Generated(apid, octets) => Record::PacketOut {
                t_us: t,
                node: side.id,
                apid,
                octets,
            },
            Out::Delivered(apid, octets) => Record::PacketIn {
                t_us: t,
                node: side.id,
                apid,
                octets,
            },
            Out::Event(code, args) => Record::AppEvent {
                t_us: t,
                node: side.id,
                code,
                args,
            },
        };
        world.records.push(record);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::next_slot;

    #[test]
    fn slots() {
        // req: APP-071
        assert_eq!(next_slot(0, 125_000), Some(125_000));
        assert_eq!(next_slot(124_999, 125_000), Some(125_000));
        assert_eq!(next_slot(125_000, 125_000), Some(250_000));
        assert_eq!(next_slot(1, 0), None);
    }
}
