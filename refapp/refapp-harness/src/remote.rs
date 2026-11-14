//! The harness side of the node protocol (docs/reference-application.md §8.2, APP-092).
//!
//! A
//! [`RemoteNode`] is a [`Node`] whose steps run in another process, in lockstep: one message
//! out, one answer in. It never reads a clock; a hung node is bounded by the job timeout.
//!
//! [`Node::step`] cannot fail, so the first protocol error is kept, every later step does
//! nothing, and [`run_remote`] turns the error into the result of the run: no trace.

use std::io::{Read, Write};

use refapp_scenario::{Apid, Instant, Node, Output, StepInput};

use crate::proto::{Message, NodeOutput, PROTOCOL_VERSION, Role, read_message, write_message};

/// A node in another process, reached over a pair of byte streams.
pub struct RemoteNode<R, W> {
    reader: R,
    writer: W,
    error: Option<String>,
}

impl<R: Read, W: Write> RemoteNode<R, W> {
    /// Sends HELLO and waits for `HELLO_ACK`.
    ///
    /// # Errors
    /// If the stream fails, the answer is not a `HELLO_ACK`, or its version is not 1.
    pub fn connect(
        reader: R,
        writer: W,
        role: Role,
        scenario_version: u16,
        case_id: &str,
    ) -> Result<Self, String> {
        let mut node = Self {
            reader,
            writer,
            error: None,
        };
        let hello = Message::Hello {
            version: PROTOCOL_VERSION,
            scenario_version,
            role,
            case_id: case_id.to_owned(),
        };
        match node.exchange(&hello)? {
            Message::HelloAck { version } if version == PROTOCOL_VERSION => Ok(node),
            Message::HelloAck { version } => Err(format!(
                "node speaks protocol version {version}, not {PROTOCOL_VERSION}"
            )),
            m => Err(unexpected(&m, "HELLO_ACK")),
        }
    }

    /// Sends END and waits for `END_ACK`; reports an error kept from an earlier step instead.
    ///
    /// # Errors
    /// The first error of the run, or a failure of the END exchange.
    pub fn finish(&mut self) -> Result<(), String> {
        if let Some(e) = self.error.take() {
            return Err(e);
        }
        match self.exchange(&Message::End)? {
            Message::EndAck => Ok(()),
            m => Err(unexpected(&m, "END_ACK")),
        }
    }

    /// The first error of the run, if any.
    pub const fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    fn exchange(&mut self, m: &Message) -> Result<Message, String> {
        write_message(&mut self.writer, m)?;
        read_message(&mut self.reader).map_err(|e| format!("after {}: {e}", m.name()))
    }

    fn try_step(&mut self, input: &StepInput<'_>) -> Result<Message, String> {
        let step = Message::Step {
            now: input.now.as_micros(),
            tx_slot: input.tx_slot,
            units: input.delivered.iter().map(<[u8]>::to_vec).collect(),
            ops: input.ops.to_vec(),
        };
        self.exchange(&step)
    }
}

fn unexpected(m: &Message, wanted: &str) -> String {
    format!("unexpected {} where {wanted} was due", m.name())
}

impl<R: Read, W: Write> Node for RemoteNode<R, W> {
    fn step(&mut self, input: &StepInput<'_>, out: &mut dyn Output) -> Option<Instant> {
        if self.error.is_some() {
            return None;
        }
        match self.try_step(input).and_then(|m| replay(m, out)) {
            Ok(deadline) => deadline,
            Err(e) => {
                self.error = Some(e);
                None
            }
        }
    }
}

/// Checks a `STEP_RESULT` whole, then hands its outputs on in order.
fn replay(m: Message, out: &mut dyn Output) -> Result<Option<Instant>, String> {
    let Message::StepResult {
        outputs,
        next_deadline,
    } = m
    else {
        return Err(unexpected(&m, "STEP_RESULT"));
    };
    let apid = |v: u16| Apid::new(v).ok_or_else(|| format!("STEP_RESULT: APID {v} out of range"));
    let mut checked = Vec::with_capacity(outputs.len());
    for o in &outputs {
        checked.push(match o {
            NodeOutput::PacketOut(a, _) | NodeOutput::PacketIn(a, _) => Some(apid(*a)?),
            NodeOutput::Unit(_) | NodeOutput::Event(..) => None,
        });
    }
    for (o, a) in outputs.iter().zip(checked) {
        match (o, a) {
            (NodeOutput::Unit(octets), _) => out.unit(octets),
            (NodeOutput::PacketOut(_, octets), Some(a)) => out.packet_out(a, octets),
            (NodeOutput::PacketIn(_, octets), Some(a)) => out.packet_in(a, octets),
            (NodeOutput::Event(code, args), _) => out.app_event(*code, args),
            (NodeOutput::PacketOut(..) | NodeOutput::PacketIn(..), None) => {
                return Err("STEP_RESULT: packet without APID".to_owned());
            }
        }
    }
    Ok(next_deadline.map(Instant::from_micros))
}

/// Runs a case with the flight node on the far side of `node`: the trace of the run, after
/// END and `END_ACK`. Any protocol error ends the run with that error and no trace.
///
/// # Errors
/// A protocol error, or an error of the run itself ([`crate::run_case_with_flight`]).
pub fn run_remote<R: Read, W: Write>(
    case_id: &str,
    node: &mut RemoteNode<R, W>,
) -> Result<String, String> {
    let trace = crate::run_case_with_flight(case_id, node);
    if let Some(e) = node.take_error() {
        return Err(e);
    }
    let trace = trace?;
    node.finish()?;
    Ok(trace)
}
