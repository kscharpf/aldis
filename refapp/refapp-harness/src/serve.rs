//! The node side of the node protocol (docs/reference-application.md §8.2).
//!
//! Serves a [`Node`]
//! to a harness over a pair of byte streams. A test node (`refapp-test-node`) and the tests of
//! the harness side use it; a cFS node implements the same protocol in C.

use std::io::{Read, Write};

use refapp_scenario::{Apid, Delivered, Instant, Node, Output, StepInput};

use crate::proto::{Message, NodeOutput, PROTOCOL_VERSION, Role, read_message, write_message};

/// What HELLO told the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    /// Scenario version.
    pub scenario_version: u16,
    /// Role of the node.
    pub role: Role,
    /// Case ID.
    pub case_id: String,
}

#[derive(Default)]
struct Collector(Vec<NodeOutput>);

impl Output for Collector {
    fn unit(&mut self, octets: &[u8]) {
        self.0.push(NodeOutput::Unit(octets.to_vec()));
    }
    fn packet_out(&mut self, apid: Apid, octets: &[u8]) {
        self.0
            .push(NodeOutput::PacketOut(apid.get(), octets.to_vec()));
    }
    fn packet_in(&mut self, apid: Apid, octets: &[u8]) {
        self.0
            .push(NodeOutput::PacketIn(apid.get(), octets.to_vec()));
    }
    fn app_event(&mut self, code: u16, args: &[u64]) {
        self.0.push(NodeOutput::Event(code, args.to_vec()));
    }
}

/// Serves one run: HELLO, any number of STEPs, END.
///
/// # Errors
/// If the stream fails, a message is malformed or out of order, the version is not 1, or
/// `make` refuses the HELLO.
pub fn serve<R, W, F>(reader: &mut R, writer: &mut W, make: F) -> Result<(), String>
where
    R: Read,
    W: Write,
    F: FnOnce(&Hello) -> Result<Box<dyn Node>, String>,
{
    let Message::Hello {
        version,
        scenario_version,
        role,
        case_id,
    } = read_message(reader)?
    else {
        return Err("the first message is not HELLO".to_owned());
    };
    if version != PROTOCOL_VERSION {
        return Err(format!("harness speaks protocol version {version}"));
    }
    let mut node = make(&Hello {
        scenario_version,
        role,
        case_id,
    })?;
    write_message(writer, &Message::HelloAck { version })?;
    loop {
        match read_message(reader)? {
            Message::Step {
                now,
                tx_slot,
                units,
                ops,
            } => {
                let slices: Vec<&[u8]> = units.iter().map(Vec::as_slice).collect();
                let input = StepInput {
                    now: Instant::from_micros(now),
                    delivered: Delivered::new(&slices),
                    ops: &ops,
                    tx_slot,
                };
                let mut out = Collector::default();
                let next_deadline = node.step(&input, &mut out).map(Instant::as_micros);
                let result = Message::StepResult {
                    outputs: out.0,
                    next_deadline,
                };
                write_message(writer, &result)?;
            }
            Message::End => return write_message(writer, &Message::EndAck),
            m => return Err(format!("unexpected {} during the run", m.name())),
        }
    }
}
