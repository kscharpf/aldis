//! The run log (APP-162, plan D5 d): JSON Lines of a header, every datagram, every node event,
//! and every fault of the loss script.
//!
//! It is not a golden trace (it holds a real clock's instants) and is not committed. The node
//! events and the faults have the form of the trace records of §7.3, written by the harness's
//! [`Record`]; only the header and the datagram records are the driver's own. The text is built
//! by hand, because the driver takes no JSON dependency.

use std::fmt::Write as _;
use std::io::Write;

use refapp_harness::micros::Micros;
use refapp_harness::trace::{Dir, Record, hex};

use crate::Error;
use crate::loss::LossScript;

/// What happened to a datagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatagramKind {
    /// Sent on the wire.
    Sent,
    /// Received from the wire and delivered to the node.
    Received,
    /// Not sent, or not delivered, because the loss script drops it (a `fault` record precedes).
    Dropped,
}

impl DatagramKind {
    const fn as_str(self) -> &'static str {
        match self {
            DatagramKind::Sent => "sent",
            DatagramKind::Received => "received",
            DatagramKind::Dropped => "dropped",
        }
    }
}

/// One datagram, as the driver saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Datagram {
    /// The instant the node was stepped at, µs since the start of the run: a scheduled instant
    /// for a datagram that a timed step sent.
    pub t_us: Micros,
    /// The clock's reading when the driver sent or took the datagram, µs since the start of the
    /// run, never before `t_us`; the difference is the driver's lateness (APP-162).
    pub wall_us: Micros,
    /// Direction of the link.
    pub dir: Dir,
    /// Index among the units of that direction (§11.6).
    pub index: u64,
    /// What happened.
    pub kind: DatagramKind,
    /// The datagram's octets: one transfer frame (APP-159).
    pub octets: Vec<u8>,
}

/// One record of the run log after the header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A datagram.
    Datagram(Datagram),
    /// A node event of §7.3 and §7.4, or a `fault` of the loss script.
    Node(Record),
    /// A `tx_slot` step ran with the clock `count` slots past the slot it served (APP-162):
    /// the frame went out late, not skipped. Tells the driver's lateness from a loss on the
    /// link.
    SlotsMissed {
        /// The instant the node was stepped at, µs since the start of the run.
        t_us: Micros,
        /// The clock's reading when the step ran, µs since the start of the run.
        wall_us: Micros,
        /// The number of slot instants the clock had passed beyond the slot the step served.
        count: u64,
    },
}

/// What the driver reports into: the run log, or a test's collector.
pub trait Sink {
    /// Takes one record.
    ///
    /// # Errors
    /// If it cannot be written.
    fn record(&mut self, event: Event) -> Result<(), Error>;
}

impl Sink for Vec<Event> {
    fn record(&mut self, event: Event) -> Result<(), Error> {
        self.push(event);
        Ok(())
    }
}

/// The header line of a run log (APP-162). Every field is written; `None` is `null`, meaning
/// unknown (a run without a ground system has no version or digest).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// The case ID.
    pub case: String,
    /// The ground system's version; `null` in the log when unknown (APP-155, APP-162). The
    /// three fields of the ground system and the node are always written.
    pub ground_version: Option<String>,
    /// The ground system's image digest, if known.
    pub image_digest: Option<String>,
    /// The commit of the node, if known.
    pub node_commit: Option<String>,
    /// The loss script of the run.
    pub loss_script: LossScript,
}

/// `s` as a JSON string.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_optional(s: Option<&str>) -> String {
    s.map_or_else(|| "null".to_owned(), json_string)
}

impl Header {
    /// The header line, LF included.
    #[must_use]
    pub fn to_line(&self) -> String {
        let script: Vec<String> = self
            .loss_script
            .entries()
            .iter()
            .map(|(dir, index)| format!("{{\"dir\":\"{}\",\"index\":{index}}}", dir.as_str()))
            .collect();
        format!(
            "{{\"kind\":\"header\",\"log\":\"aldis-interop-run\",\"log_version\":1,\
             \"case\":{},\"ground_version\":{},\"image_digest\":{},\"node_commit\":{},\
             \"loss_script\":[{}]}}\n",
            json_string(&self.case),
            json_optional(self.ground_version.as_deref()),
            json_optional(self.image_digest.as_deref()),
            json_optional(self.node_commit.as_deref()),
            script.join(",")
        )
    }
}

impl Datagram {
    /// The record line, LF included.
    #[must_use]
    pub fn to_line(&self) -> String {
        format!(
            "{{\"t_us\":{},\"wall_us\":{},\"kind\":\"datagram\",\"dir\":\"{}\",\"index\":{},\
             \"event\":\"{}\",\"hex\":\"{}\"}}\n",
            self.t_us,
            self.wall_us,
            self.dir.as_str(),
            self.index,
            self.kind.as_str(),
            hex(&self.octets)
        )
    }
}

/// The run log, written to `W` as the records arrive.
#[derive(Debug)]
pub struct RunLog<W: Write> {
    out: W,
}

impl<W: Write> RunLog<W> {
    /// A log that starts with `header`.
    ///
    /// # Errors
    /// If the header cannot be written.
    pub fn new(mut out: W, header: &Header) -> Result<Self, Error> {
        out.write_all(header.to_line().as_bytes())?;
        Ok(RunLog { out })
    }

    /// The writer, flushed.
    ///
    /// # Errors
    /// If the flush fails.
    pub fn finish(mut self) -> Result<W, Error> {
        self.out.flush()?;
        Ok(self.out)
    }
}

impl<W: Write> Sink for RunLog<W> {
    fn record(&mut self, event: Event) -> Result<(), Error> {
        let line = match event {
            Event::Datagram(d) => d.to_line(),
            Event::Node(r) => r.to_line().map_err(Error::Contract)?,
            Event::SlotsMissed {
                t_us,
                wall_us,
                count,
            } => {
                format!(
                    "{{\"t_us\":{t_us},\"wall_us\":{wall_us},\"kind\":\"slots_missed\",\
                     \"count\":{count}}}\n"
                )
            }
        };
        self.out.write_all(line.as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use refapp_harness::micros::Micros;
    use refapp_harness::trace::{Dir, FaultKind, NodeId, Record};

    use super::{Datagram, DatagramKind, Event, Header, RunLog, Sink, json_string};
    use crate::loss::LossScript;

    #[test]
    fn strings_are_escaped() {
        // req: APP-162
        assert_eq!(json_string("a\"b\\c\n\u{1}"), "\"a\\\"b\\\\c\\n\\u0001\"");
    }

    #[test]
    fn header_datagram_node_and_fault_lines() {
        // req: APP-162
        let header = Header {
            case: "a3-nominal".to_owned(),
            ground_version: Some("5.14.0".to_owned()),
            image_digest: None,
            node_commit: None,
            loss_script: LossScript::new(vec![(Dir::Up, 2)]),
        };
        let mut log = RunLog::new(Vec::new(), &header).unwrap();
        log.record(Event::Datagram(Datagram {
            t_us: Micros::new(125_000),
            wall_us: Micros::new(125_040),
            dir: Dir::Down,
            index: 1,
            kind: DatagramKind::Sent,
            octets: vec![0x0A, 0xFF],
        }))
        .unwrap();
        log.record(Event::Node(Record::Fault {
            t_us: Micros::new(7),
            dir: Dir::Up,
            index: 2,
            fault: FaultKind::Drop,
            args: vec![],
        }))
        .unwrap();
        log.record(Event::Node(Record::AppEvent {
            t_us: Micros::new(8),
            node: NodeId::Flight,
            code: 210,
            args: vec![1],
        }))
        .unwrap();
        let text = String::from_utf8(log.finish().unwrap()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            "{\"kind\":\"header\",\"log\":\"aldis-interop-run\",\"log_version\":1,\
             \"case\":\"a3-nominal\",\"ground_version\":\"5.14.0\",\"image_digest\":null,\
             \"node_commit\":null,\"loss_script\":[{\"dir\":\"up\",\"index\":2}]}"
        );
        assert_eq!(
            lines[1],
            "{\"t_us\":125000,\"wall_us\":125040,\"kind\":\"datagram\",\"dir\":\"down\",\"index\":1,\
             \"event\":\"sent\",\"hex\":\"0AFF\"}"
        );
        assert_eq!(
            lines[2],
            "{\"t_us\":7,\"kind\":\"fault\",\"dir\":\"up\",\"index\":2,\"fault\":\"drop\",\"args\":[]}"
        );
        assert!(lines[3].contains("\"kind\":\"app_event\""), "{}", lines[3]);
        assert_eq!(lines.len(), 4);
    }
}
