//! The driver on a clock that wakes late, as a real one does (AINT WP-3): the node is stepped at
//! its scheduled instants, so the frames it sends stay on the slot grid, and the run log shows
//! the clock's reading beside each instant (APP-157, APP-162).

use std::cell::{Cell, RefCell};
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
use refapp_scenario::{Instant, Node, Output, StepInput};

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

/// A node that sends one unit per `tx_slot` step and asks for a step 40 000 µs on, so that its
/// deadlines fall between the slots. It keeps what it saw: instant, `tx_slot`, datagrams.
struct Probe(Rc<RefCell<Vec<(u64, bool, usize)>>>);

impl Node for Probe {
    fn step(&mut self, input: &StepInput<'_>, out: &mut dyn Output) -> Option<Instant> {
        self.0
            .borrow_mut()
            .push((input.now.as_micros(), input.tx_slot, input.delivered.len()));
        if input.tx_slot {
            out.unit(&[1, 2, 3]);
        }
        Some(Instant::from_micros(input.now.as_micros() + 40_000))
    }
}

/// A port whose wait for the instant `at` ends with the clock at `to`, with a datagram if
/// `datagram`.
struct Jump {
    clock: Rc<Cell<u64>>,
    at: u64,
    to: u64,
    datagram: bool,
    done: bool,
}

impl Port for Jump {
    fn send(&mut self, _octets: &[u8]) -> io::Result<()> {
        Ok(())
    }
    fn recv(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        if wait.is_zero() {
            return Ok(None);
        }
        let target = self.clock.get() + u64::try_from(wait.as_micros()).unwrap();
        if target == self.at && !self.done {
            self.done = true;
            self.clock.set(self.to);
            return Ok(self.datagram.then(|| vec![0xAB]));
        }
        self.clock.set(target);
        Ok(None)
    }
}

/// Runs the probe to 1 s on a clock that jumps from just before `at` to `to`.
fn run_probe(at: u64, to: u64, datagram: bool) -> (Vec<(u64, bool, usize)>, Vec<Event>) {
    let clock = Rc::new(Cell::new(0));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut probe = Probe(seen.clone());
    let mut port = Jump {
        clock: clock.clone(),
        at,
        to,
        datagram,
        done: false,
    };
    let mut events: Vec<Event> = Vec::new();
    run_flight(
        &mut probe,
        NonZeroU64::new(125_000).unwrap(),
        Micros::new(1_000_000),
        &LossScript::default(),
        &mut Shared(clock),
        &mut port,
        &mut events,
    )
    .unwrap();
    let seen = seen.borrow().clone();
    (seen, events)
}

/// `(t_us, wall_us)` of the sent downlink datagrams in `events`.
fn sent_with_reading(events: &[Event]) -> Vec<(u64, u64)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Datagram(d) if d.dir == Dir::Down && d.kind == DatagramKind::Sent => {
                Some((d.t_us.get(), d.wall_us.get()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_clock_that_jumps_past_several_slots_shows_in_the_log() {
    // req: APP-157, APP-162
    // The wait for the slot at 125 000 µs ends with the clock at 520 000 µs, in slot 4.
    let (seen, events) = run_probe(125_000, 520_000, false);
    let sent = sent_with_reading(&events);
    assert!(sent.iter().all(|(t, _)| t % 125_000 == 0), "{sent:?}");
    // Each late frame carries the clock's reading; the node's instants stayed scheduled.
    assert!(sent.contains(&(125_000, 520_000)), "{sent:?}");
    assert!(sent.contains(&(375_000, 520_000)), "{sent:?}");
    assert!(sent.contains(&(500_000, 520_000)), "{sent:?}");
    assert!(sent.iter().all(|(t, w)| w >= t), "{sent:?}");
    // On time before the jump: the first frame has the same instant and reading.
    assert_eq!(sent.first(), Some(&(0, 0)));
    let missed: Vec<(u64, u64, u64)> = events
        .iter()
        .filter_map(|e| match e {
            Event::SlotsMissed {
                t_us,
                wall_us,
                count,
            } => Some((t_us.get(), wall_us.get(), *count)),
            _ => None,
        })
        .collect();
    assert_eq!(
        missed,
        vec![
            (125_000, 520_000, 3),
            (250_000, 520_000, 2),
            (375_000, 520_000, 1)
        ]
    );
    assert!(seen.windows(2).all(|w| w[0].0 < w[1].0), "{seen:?}");
}

#[test]
fn a_datagram_after_a_non_slot_deadline_is_delivered_after_that_deadline_step() {
    // req: APP-157
    // The probe's deadline at 165 000 µs falls between slots; the datagram is seen at 167 000.
    let (seen, events) = run_probe(165_000, 167_000, true);
    let position = |at| seen.iter().position(|s| s.0 == at);
    let deadline = position(165_000).expect("the deadline's step at its own instant");
    let delivery = position(167_000).expect("the datagram's step");
    assert_eq!(seen[deadline], (165_000, false, 0));
    assert_eq!(seen[delivery], (167_000, false, 1));
    assert!(deadline < delivery);
    let received = events.iter().any(|e| {
        matches!(e, Event::Datagram(d)
            if d.kind == DatagramKind::Received && d.t_us.get() == 167_000
                && d.wall_us.get() == 167_000)
    });
    assert!(received, "the datagram is logged at the clock's reading");
}

#[test]
fn a_datagram_after_several_passed_instants_is_delivered_after_all_of_them() {
    // req: APP-157, APP-158, APP-162
    // The wait for the slot at 125 000 µs ends at 400 000 µs with a datagram: the slots at
    // 125 000, 250 000 and 375 000 and the deadlines between them have passed.
    let (seen, events) = run_probe(125_000, 400_000, true);
    let delivery = seen
        .iter()
        .position(|s| s.2 == 1)
        .expect("the datagram's step");
    assert_eq!(seen[delivery], (400_000, false, 1));
    assert!(seen.windows(2).all(|w| w[0].0 < w[1].0), "{seen:?}");
    let before: Vec<u64> = seen[..delivery]
        .iter()
        .filter(|s| s.1)
        .map(|s| s.0)
        .collect();
    assert_eq!(
        before,
        vec![0, 125_000, 250_000, 375_000],
        "one step per slot"
    );
    assert!(
        seen[..delivery].iter().any(|s| s.0 % 125_000 != 0),
        "{seen:?}"
    );
    // The served instants are logged late: their datagrams carry the delivery's reading.
    let sent = sent_with_reading(&events);
    assert_eq!(
        sent[..4],
        vec![
            (0, 0),
            (125_000, 400_000),
            (250_000, 400_000),
            (375_000, 400_000)
        ]
    );
    // After the delivery the clock is on time again.
    assert!(sent[4..].iter().all(|(t, w)| t == w), "{sent:?}");
}
