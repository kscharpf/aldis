//! The time-tag queue (docs/reference-application.md §2.3; APP-012).

use crate::command::{Command, Reject};

/// Capacity of the time-tag queue, commands (§2.3, APP-012).
pub const CAPACITY: usize = 8;

/// A queued command with its execution instant and its place in the delivery order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Queued {
    at_us: u64,
    arrival: u64,
    command: Command,
}

/// Up to [`CAPACITY`] time-tagged commands, executed by execution time and, for equal times, in
/// delivery order (§2.3, APP-012).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TimeTagQueue {
    slots: [Option<Queued>; CAPACITY],
    /// Delivery number of the next command; a `u64` that a run of 35 s never exhausts.
    next_arrival: u64,
}

impl TimeTagQueue {
    /// Queues `command` for the instant `at_us` µs after the start of the run (§2.3).
    ///
    /// # Errors
    /// [`Reject::QueueFull`] if the queue holds [`CAPACITY`] commands.
    pub fn push(&mut self, at_us: u64, command: Command) -> Result<(), Reject> {
        let slot = self
            .slots
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(Reject::QueueFull)?;
        *slot = Some(Queued {
            at_us,
            arrival: self.next_arrival,
            command,
        });
        self.next_arrival = self.next_arrival.saturating_add(1);
        Ok(())
    }

    /// Removes and returns the command to execute next at `now_us`: the earliest due one, the
    /// first delivered among equal times (§2.3).
    pub fn pop_due(&mut self, now_us: u64) -> Option<Command> {
        let (_, _, index) = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| {
                slot.filter(|q| q.at_us <= now_us)
                    .map(|q| (q.at_us, q.arrival, i))
            })
            .min()?;
        self.slots.get_mut(index)?.take().map(|q| q.command)
    }

    /// The earliest execution instant in the queue, µs.
    #[must_use]
    pub fn next_due(&self) -> Option<u64> {
        self.slots.iter().flatten().map(|q| q.at_us).min()
    }

    /// Commands in the queue (HK octet 8, §3.3.1).
    #[must_use]
    pub fn occupancy(&self) -> u8 {
        self.slots
            .iter()
            .flatten()
            .fold(0_u8, |n, _| n.saturating_add(1))
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::{CAPACITY, TimeTagQueue};
    use crate::command::{Call, Command, Reject, Timing};

    fn cmd(seq: u16) -> Command {
        Command {
            function: 1,
            seq,
            timing: Timing::Immediate,
            call: Ok(Call::Noop),
        }
    }

    fn drain(q: &mut TimeTagQueue, now: u64) -> std::vec::Vec<u16> {
        std::iter::from_fn(|| q.pop_due(now))
            .map(|c| c.seq)
            .collect()
    }

    #[test]
    fn executes_by_time_then_delivery_order() {
        // req: APP-012
        let mut q = TimeTagQueue::default();
        for (at, seq) in [(20, 0), (10, 1), (20, 2), (10, 3), (15, 4)] {
            q.push(at, cmd(seq)).unwrap();
        }
        assert_eq!(q.next_due(), Some(10));
        assert_eq!(q.pop_due(9), None);
        assert_eq!(drain(&mut q, 15), [1, 3, 4]);
        assert_eq!(q.occupancy(), 2);
        assert_eq!(drain(&mut q, 100), [0, 2]);
        assert_eq!(q.occupancy(), 0);
        assert_eq!(q.next_due(), None);
    }

    #[test]
    fn holds_eight_and_refuses_the_ninth() {
        // req: APP-012
        let mut q = TimeTagQueue::default();
        for seq in 0..8 {
            q.push(5, cmd(seq)).unwrap();
        }
        assert_eq!(usize::from(q.occupancy()), CAPACITY);
        assert_eq!(q.push(5, cmd(8)), Err(Reject::QueueFull));
        // A freed slot is reused, and delivery order still counts from the new delivery.
        assert_eq!(q.pop_due(5).map(|c| c.seq), Some(0));
        q.push(5, cmd(9)).unwrap();
        assert_eq!(drain(&mut q, 5), [1, 2, 3, 4, 5, 6, 7, 9]);
    }
}
