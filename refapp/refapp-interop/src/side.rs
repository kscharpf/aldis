//! One node's side of the driver: its step (§8.1), its transmit slot, and the datagrams it
//! sends and receives (APP-157…APP-159, APP-163).

use std::num::NonZeroU64;

use refapp_harness::micros::Micros;
use refapp_harness::trace::{Dir, FaultKind, NodeId, Record};
use refapp_scenario::{Apid, Delivered, Instant, Node, Output, StepInput};

use crate::Error;
use crate::log::{Datagram, DatagramKind, Event, Sink};
use crate::loss::LossScript;
use crate::port::Port;

/// One output of a node step, in output order.
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

/// How a side treats the loss script and the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The flight node: drops its own downlink units on sending and the uplink units on
    /// arrival, and logs every datagram.
    Flight,
    /// The ground node of the self-test: no loss, no datagram records (the flight side's view
    /// of the link is the log).
    Ground,
}

impl Role {
    const fn node(self) -> NodeId {
        match self {
            Role::Flight => NodeId::Flight,
            Role::Ground => NodeId::Ground,
        }
    }

    const fn tx_dir(self) -> Dir {
        match self {
            Role::Flight => Dir::Down,
            Role::Ground => Dir::Up,
        }
    }
}

/// A node and what the driver keeps for it between steps.
pub struct Side<'n> {
    role: Role,
    node: &'n mut dyn Node,
    slot: NonZeroU64,
    /// The node's own deadline from its last step.
    deadline: Option<Micros>,
    /// The number of the next slot that has not had its `tx_slot` step (slot `k` is at
    /// `k · slot` µs).
    next_slot: u64,
    /// Release instants of the ops script (ground node only), in script order.
    ops: Vec<Micros>,
    ops_done: usize,
    tx_count: u64,
    rx_count: u64,
}

impl<'n> Side<'n> {
    /// A side for `node` whose transmit slot has the period `slot_us`, and that is released the
    /// ops entries at `ops_us` (§6.5).
    #[must_use]
    pub fn new(role: Role, node: &'n mut dyn Node, slot: NonZeroU64, ops: Vec<Micros>) -> Self {
        Side {
            role,
            node,
            slot,
            deadline: None,
            next_slot: 0,
            ops,
            ops_done: 0,
            tx_count: 0,
            rx_count: 0,
        }
    }

    /// The instant the next slot is due.
    const fn slot_due(&self) -> Micros {
        Micros::new(self.next_slot.saturating_mul(self.slot.get()))
    }

    /// The earliest instant at which this side needs a step without a datagram arriving: the
    /// node's deadline, the next slot, the next ops release (APP-157).
    #[must_use]
    pub fn next_wake(&self) -> Micros {
        [
            self.deadline,
            Some(self.slot_due()),
            self.ops.get(self.ops_done).copied(),
        ]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or_else(|| self.slot_due())
    }

    /// Steps the node at `now` with the datagrams `delivered`, if anything is due for it, and
    /// reports its outputs to `sink`. `tx_slot` is true in the first step at or after each
    /// slot's instant and in no other (APP-158). Returns the units the node sent, in order.
    ///
    /// # Errors
    /// If the node breaks the contract of §8.1: a deadline not after `now`, or a number of units
    /// against the `tx_slot` rules; or if `sink` fails.
    pub fn step(
        &mut self,
        now: Micros,
        delivered: &[Vec<u8>],
        sink: &mut dyn Sink,
    ) -> Result<Vec<Vec<u8>>, Error> {
        let tx_slot = now >= self.slot_due();
        let ops_from = self.ops_done;
        while self.ops.get(self.ops_done).is_some_and(|at| *at <= now) {
            self.ops_done = self.ops_done.saturating_add(1);
        }
        let ops = (ops_from..self.ops_done)
            .map(u16::try_from)
            .collect::<Result<Vec<u16>, _>>()
            .map_err(|_| Error::Script("ops script has more than 65 536 entries".to_owned()))?;
        let due = !delivered.is_empty()
            || !ops.is_empty()
            || tx_slot
            || self.deadline.is_some_and(|d| d <= now);
        if !due {
            return Ok(Vec::new());
        }
        if tx_slot {
            let current = now.get() / self.slot.get();
            let missed = current.saturating_sub(self.next_slot);
            if missed > 0 && self.role == Role::Flight {
                sink.record(Event::SlotsMissed {
                    t_us: now,
                    count: missed,
                })?;
            }
            self.next_slot = current.saturating_add(1);
        }
        let units: Vec<&[u8]> = delivered.iter().map(Vec::as_slice).collect();
        let input = StepInput {
            now: Instant::from_micros(now.get()),
            delivered: Delivered::new(&units),
            ops: &ops,
            tx_slot,
        };
        let mut out = Recorder::default();
        let deadline = self
            .node
            .step(&input, &mut out)
            .map(|i| Micros::new(i.as_micros()));
        let name = self.role.node().as_str();
        if let Some(d) = deadline.filter(|d| *d <= now) {
            return Err(Error::Contract(format!(
                "{name} node at t = {now} µs asked for a deadline {d} µs not after now"
            )));
        }
        self.deadline = deadline;
        let unit_count = out.0.iter().filter(|o| matches!(o, Out::Unit(_))).count();
        let allowed = match (self.role, tx_slot) {
            (Role::Flight, true) => unit_count == 1,
            (Role::Ground, true) => unit_count <= 1,
            (_, false) => unit_count == 0,
        };
        if !allowed {
            return Err(Error::Contract(format!(
                "{name} node at t = {now} µs output {unit_count} unit(s) with tx_slot {tx_slot}"
            )));
        }
        let node = self.role.node();
        let mut units_out = Vec::new();
        for o in out.0 {
            match o {
                Out::Unit(octets) => units_out.push(octets),
                Out::Generated(apid, octets) => sink.record(Event::Node(Record::PacketOut {
                    t_us: now,
                    node,
                    apid,
                    octets,
                }))?,
                Out::Delivered(apid, octets) => sink.record(Event::Node(Record::PacketIn {
                    t_us: now,
                    node,
                    apid,
                    octets,
                }))?,
                Out::Event(code, args) => sink.record(Event::Node(Record::AppEvent {
                    t_us: now,
                    node,
                    code,
                    args,
                }))?,
            }
        }
        Ok(units_out)
    }

    /// Sends the units a step returned, one datagram each (APP-159), after the loss script
    /// (APP-163). Returns how many datagrams went on the wire.
    ///
    /// # Errors
    /// If the port or the sink fails.
    pub fn transmit(
        &mut self,
        now: Micros,
        units: Vec<Vec<u8>>,
        port: &mut dyn Port,
        loss: &LossScript,
        sink: &mut dyn Sink,
    ) -> Result<usize, Error> {
        let dir = self.role.tx_dir();
        let mut on_wire = 0_usize;
        for octets in units {
            let index = self.tx_count;
            self.tx_count = self.tx_count.saturating_add(1);
            let drop = self.role == Role::Flight && loss.drops(dir, index);
            if drop {
                sink.record(Event::Node(Record::Fault {
                    t_us: now,
                    dir,
                    index,
                    fault: FaultKind::Drop,
                    args: Vec::new(),
                }))?;
            } else {
                port.send(&octets)?;
                on_wire = on_wire.saturating_add(1);
            }
            if self.role == Role::Flight {
                let kind = if drop {
                    DatagramKind::Dropped
                } else {
                    DatagramKind::Sent
                };
                sink.record(Event::Datagram(Datagram {
                    t_us: now,
                    dir,
                    index,
                    kind,
                    octets,
                }))?;
            }
        }
        Ok(on_wire)
    }

    /// Takes a datagram from the wire at `now`: counts it, applies the loss script (the flight
    /// side's uplink, APP-163), and logs it. Returns the octets to deliver to the node, or
    /// `None` for a dropped datagram.
    ///
    /// # Errors
    /// If the sink fails.
    pub fn receive(
        &mut self,
        now: Micros,
        octets: Vec<u8>,
        loss: &LossScript,
        sink: &mut dyn Sink,
    ) -> Result<Option<Vec<u8>>, Error> {
        let dir = match self.role.tx_dir() {
            Dir::Down => Dir::Up,
            Dir::Up => Dir::Down,
        };
        let index = self.rx_count;
        self.rx_count = self.rx_count.saturating_add(1);
        let drop = self.role == Role::Flight && loss.drops(dir, index);
        if drop {
            sink.record(Event::Node(Record::Fault {
                t_us: now,
                dir,
                index,
                fault: FaultKind::Drop,
                args: Vec::new(),
            }))?;
        }
        if self.role == Role::Flight {
            let kind = if drop {
                DatagramKind::Dropped
            } else {
                DatagramKind::Received
            };
            sink.record(Event::Datagram(Datagram {
                t_us: now,
                dir,
                index,
                kind,
                octets: octets.clone(),
            }))?;
        }
        Ok((!drop).then_some(octets))
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use refapp_harness::micros::Micros;
    use refapp_scenario::{Instant, Node, Output, StepInput};

    use super::{Role, Side};

    /// A node that sends one unit per `tx_slot` step and asks for a step 40 000 µs on.
    struct Probe {
        seen: Vec<(u64, bool, usize)>,
    }

    impl Node for Probe {
        fn step(&mut self, input: &StepInput<'_>, out: &mut dyn Output) -> Option<Instant> {
            self.seen
                .push((input.now.as_micros(), input.tx_slot, input.delivered.len()));
            if input.tx_slot {
                out.unit(&[1, 2, 3]);
            }
            Some(Instant::from_micros(input.now.as_micros() + 40_000))
        }
    }

    #[test]
    fn tx_slot_only_at_slots_and_the_next_wake_is_the_earliest() {
        // req: APP-157, APP-158
        let mut probe = Probe { seen: Vec::new() };
        let slot = NonZeroU64::new(125_000).unwrap();
        let mut side = Side::new(Role::Flight, &mut probe, slot, Vec::new());
        let mut sink: Vec<crate::log::Event> = Vec::new();
        assert_eq!(side.next_wake(), Micros::new(0));
        assert_eq!(side.step(Micros::new(0), &[], &mut sink).unwrap().len(), 1);
        // The node's deadline (40 000) is before the slot (125 000).
        assert_eq!(side.next_wake(), Micros::new(40_000));
        assert_eq!(
            side.step(Micros::new(40_000), &[], &mut sink)
                .unwrap()
                .len(),
            0
        );
        // An arriving datagram steps the node without a slot.
        let arrived = vec![vec![9_u8]];
        assert_eq!(
            side.step(Micros::new(50_000), &arrived, &mut sink)
                .unwrap()
                .len(),
            0
        );
        // Nothing is due at 60 000, so the node is not stepped.
        assert_eq!(
            side.step(Micros::new(60_000), &[], &mut sink)
                .unwrap()
                .len(),
            0
        );
        assert_eq!(side.next_wake(), Micros::new(90_000));
        side.step(Micros::new(90_000), &[], &mut sink).unwrap();
        side.step(Micros::new(130_000), &[], &mut sink).unwrap();
        // The slot at 125 000 was served by the first step after it, once.
        assert_eq!(
            probe.seen,
            vec![
                (0, true, 0),
                (40_000, false, 0),
                (50_000, false, 1),
                (90_000, false, 0),
                (130_000, true, 0),
            ]
        );
    }

    #[test]
    fn a_step_more_than_one_slot_late_logs_the_missed_slots() {
        // req: APP-158
        let mut probe = Probe { seen: Vec::new() };
        let slot = NonZeroU64::new(125_000).unwrap();
        let mut side = Side::new(Role::Flight, &mut probe, slot, Vec::new());
        let mut sink: Vec<crate::log::Event> = Vec::new();
        side.step(Micros::new(0), &[], &mut sink).unwrap();
        // One slot late (slot 1 served at 130 000): nothing missed.
        side.step(Micros::new(130_000), &[], &mut sink).unwrap();
        assert_eq!(sink.len(), 0);
        // Slot 2 is due at 250 000; a step at 400 000 is in slot 3, so slot 2 is skipped.
        side.step(Micros::new(400_000), &[], &mut sink).unwrap();
        assert_eq!(
            sink,
            vec![crate::log::Event::SlotsMissed {
                t_us: Micros::new(400_000),
                count: 1
            }]
        );
        // The next slot is 4 again: a step at 500 000 misses none.
        side.step(Micros::new(500_000), &[], &mut sink).unwrap();
        assert_eq!(sink.len(), 1);
    }

    #[test]
    fn a_unit_without_tx_slot_breaks_the_contract() {
        // req: APP-090, APP-158
        struct Rude;
        impl Node for Rude {
            fn step(&mut self, _: &StepInput<'_>, out: &mut dyn Output) -> Option<Instant> {
                out.unit(&[0]);
                None
            }
        }
        let mut rude = Rude;
        let slot = NonZeroU64::new(125_000).unwrap();
        let mut side = Side::new(Role::Flight, &mut rude, slot, Vec::new());
        let mut sink: Vec<crate::log::Event> = Vec::new();
        side.step(Micros::new(0), &[], &mut sink).unwrap();
        let err = side
            .step(Micros::new(1), &[vec![1]], &mut sink)
            .unwrap_err();
        assert!(err.to_string().contains("tx_slot false"), "{err}");
    }
}
