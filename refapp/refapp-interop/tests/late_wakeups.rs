//! The driver on a clock that wakes late, as a real one does (AINT WP-3): the node's instants
//! stay on the slot grid, so no release deadline falls between slots.

use std::cell::Cell;
use std::io;
use std::num::NonZeroU64;
use std::rc::Rc;
use std::time::Duration;

use refapp_harness::golden_path;
use refapp_harness::micros::Micros;
use refapp_harness::scenario::Scenario;
use refapp_harness::trace::Dir;
use refapp_interop::clock::Clock;
use refapp_interop::log::{DatagramKind, Event};
use refapp_interop::loss::LossScript;
use refapp_interop::port::Port;
use refapp_interop::run::{flight_node, run_flight};

/// A clock that only the port moves.
struct Shared(Rc<Cell<u64>>);

impl Clock for Shared {
    fn now(&self) -> Micros {
        Micros::new(self.0.get())
    }
    fn passes_by_itself(&self) -> bool {
        true
    }
    fn advance_to(&mut self, _t: Micros) {}
}

/// A port that receives nothing and oversleeps every wait, by a lateness that falls by 50 µs per
/// slot over sixteen slots, so that a field started late in one slot (the payload field of the
/// 0.5 s slot, due 1 s later) reaches its deadline after a later slot's step and before the next.
struct Sleepy(Rc<Cell<u64>>);

const fn late_us(target_us: u64) -> u64 {
    1_000 - 50 * ((target_us / 125_000) % 16)
}

impl Port for Sleepy {
    fn send(&mut self, _octets: &[u8]) -> io::Result<()> {
        Ok(())
    }
    fn recv(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        if !wait.is_zero() {
            let slept = u64::try_from(wait.as_micros()).unwrap();
            let target = self.0.get() + slept;
            self.0.set(target + late_us(target));
        }
        Ok(None)
    }
}

/// A port like `Sleepy` whose wait for the slot at 250 000 µs ends 2 ms late with a datagram:
/// the first uplink frame of the golden trace of `a3-nominal`.
struct LateDatagram(Rc<Cell<u64>>, bool, Vec<u8>);

/// The octets of the first `unit_sent` record of the uplink in a golden trace.
fn first_uplink_frame() -> Vec<u8> {
    let trace = std::fs::read_to_string(golden_path("a3-nominal")).unwrap();
    let line = trace
        .lines()
        .find(|l| l.contains(r#""kind":"unit_sent","dir":"up""#))
        .unwrap();
    let hex = line
        .split(r#""hex":""#)
        .nth(1)
        .unwrap()
        .trim_end_matches(['"', '}']);
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

impl Port for LateDatagram {
    fn send(&mut self, _octets: &[u8]) -> io::Result<()> {
        Ok(())
    }
    fn recv(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        if wait.is_zero() {
            return Ok(None);
        }
        let target = self.0.get() + u64::try_from(wait.as_micros()).unwrap();
        if target == 250_000 && !self.1 {
            self.1 = true;
            self.0.set(target + 2_000);
            return Ok(Some(self.2.clone()));
        }
        self.0.set(target + late_us(target));
        Ok(None)
    }
}

/// The events of a run of `a3-nominal` to 6 s.
fn run_events(port: &mut dyn Port, now: &Rc<Cell<u64>>) -> Vec<Event> {
    let scenario = Scenario::load().unwrap();
    let case = scenario.case("a3-nominal").unwrap();
    let mut flight = flight_node(case.level).unwrap();
    let mut events: Vec<Event> = Vec::new();
    run_flight(
        &mut flight,
        NonZeroU64::new(125_000).unwrap(),
        Micros::new(6_000_000),
        &LossScript::default(),
        &mut Shared(now.clone()),
        port,
        &mut events,
    )
    .unwrap();
    events
}

/// The instants of the sent downlink datagrams in `events`.
fn sent_instants(events: &[Event]) -> Vec<u64> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Datagram(d) if d.dir == Dir::Down && d.kind == DatagramKind::Sent => {
                Some(d.t_us.get())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_clock_that_wakes_late_keeps_the_node_on_the_slot_grid() {
    // req: APP-157, APP-158
    let now = Rc::new(Cell::new(0));
    let sent = sent_instants(&run_events(&mut Sleepy(now.clone()), &now));
    assert_eq!(sent.len(), 48, "a frame per slot was sent");
    assert!(sent.iter().all(|t| t % 125_000 == 0), "{sent:?}");
}

#[test]
fn a_datagram_that_arrives_after_a_slot_is_delivered_after_the_slot_step() {
    // req: APP-157, APP-158
    let now = Rc::new(Cell::new(0));
    let events = run_events(
        &mut LateDatagram(now.clone(), false, first_uplink_frame()),
        &now,
    );
    let sent = sent_instants(&events);
    assert_eq!(sent.len(), 48, "a frame per slot was sent");
    assert!(sent.iter().all(|t| t % 125_000 == 0), "{sent:?}");
    let position = |dir, kind, t_us| {
        events.iter().position(|e| {
            matches!(e, Event::Datagram(d)
                if d.dir == dir && d.kind == kind && d.t_us.get() == t_us)
        })
    };
    let step = position(Dir::Down, DatagramKind::Sent, 250_000).expect("the slot's frame");
    let delivery = position(Dir::Up, DatagramKind::Received, 252_000).expect("the delivery");
    assert!(step < delivery, "the passed instant is served first");
}
