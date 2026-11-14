//! Messages of the node protocol (docs/reference-application.md §8.2, APP-092).
//!
//! A message is a 4-octet
//! big-endian body length, then a body that starts with the type octet; all integers are
//! big-endian. Decoding is strict: a body that is short, has trailing octets, or carries an
//! unknown type, kind, or flag is an error, never a clamped or defaulted value.

use std::io::{ErrorKind, Read, Write};

/// Protocol version of HELLO and `HELLO_ACK` (§8.2).
pub const PROTOCOL_VERSION: u16 = 1;

/// Largest body the harness accepts (1 MiB).
///
/// §8.2 sets no limit; the bound keeps a corrupt
/// length from allocating without limit. Decided by the coordinator, recorded in the body of
/// PR #596 ("Coordinator decisions").
pub const MAX_BODY: usize = 1 << 20;

const T_HELLO: u8 = 0x01;
const T_STEP: u8 = 0x02;
const T_END: u8 = 0x03;
const T_HELLO_ACK: u8 = 0x81;
const T_STEP_RESULT: u8 = 0x82;
const T_END_ACK: u8 = 0x83;

/// Which node a process plays (HELLO `role`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The flight node (`0`).
    Flight,
    /// The ground node (`1`).
    Ground,
}

/// One output of a step, in output order (`STEP_RESULT`, §8.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeOutput {
    /// A link unit (kind 1).
    Unit(Vec<u8>),
    /// A packet an application generated: APID and octets (kind 2).
    PacketOut(u16, Vec<u8>),
    /// A packet delivered to an application: APID and octets (kind 3).
    PacketIn(u16, Vec<u8>),
    /// An application event: code and arguments (kind 4).
    Event(u16, Vec<u64>),
}

/// A message of the node protocol, in either direction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// `0x01`, harness to node.
    Hello {
        /// Protocol version.
        version: u16,
        /// Scenario version.
        scenario_version: u16,
        /// Role of the node.
        role: Role,
        /// ASCII case ID.
        case_id: String,
    },
    /// `0x02`, harness to node.
    Step {
        /// Simulated time in microseconds.
        now: u64,
        /// Whether the link takes a unit in this step.
        tx_slot: bool,
        /// Delivered units, in order.
        units: Vec<Vec<u8>>,
        /// Indices of the due ops-script entries.
        ops: Vec<u16>,
    },
    /// `0x03`, harness to node.
    End,
    /// `0x81`, node to harness.
    HelloAck {
        /// Protocol version.
        version: u16,
    },
    /// `0x82`, node to harness.
    StepResult {
        /// Outputs, in order.
        outputs: Vec<NodeOutput>,
        /// Earliest instant of the next step, in microseconds; `None` is all ones on the wire.
        next_deadline: Option<u64>,
    },
    /// `0x83`, node to harness.
    EndAck,
}

impl Message {
    /// Name of the message type, for error text.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Hello { .. } => "HELLO",
            Self::Step { .. } => "STEP",
            Self::End => "END",
            Self::HelloAck { .. } => "HELLO_ACK",
            Self::StepResult { .. } => "STEP_RESULT",
            Self::EndAck => "END_ACK",
        }
    }

    /// The body: type octet, then the fields.
    ///
    /// # Errors
    /// If a count or length does not fit its field, or the case ID is not ASCII.
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let mut b = Vec::new();
        match self {
            Self::Hello {
                version,
                scenario_version,
                role,
                case_id,
            } => {
                b.push(T_HELLO);
                b.extend(version.to_be_bytes());
                b.extend(scenario_version.to_be_bytes());
                b.push(match role {
                    Role::Flight => 0,
                    Role::Ground => 1,
                });
                if !case_id.is_ascii() {
                    return Err(format!("case ID `{case_id}` is not ASCII"));
                }
                b.push(u8::try_from(case_id.len()).map_err(|_| "case ID longer than 255")?);
                b.extend(case_id.as_bytes());
            }
            Self::Step {
                now,
                tx_slot,
                units,
                ops,
            } => {
                b.push(T_STEP);
                b.extend(now.to_be_bytes());
                b.push(u8::from(*tx_slot));
                put_count(&mut b, units.len(), "units")?;
                for u in units {
                    put_octets(&mut b, u)?;
                }
                put_count(&mut b, ops.len(), "ops")?;
                for o in ops {
                    b.extend(o.to_be_bytes());
                }
            }
            Self::End => b.push(T_END),
            Self::HelloAck { version } => {
                b.push(T_HELLO_ACK);
                b.extend(version.to_be_bytes());
            }
            Self::StepResult {
                outputs,
                next_deadline,
            } => {
                b.push(T_STEP_RESULT);
                put_count(&mut b, outputs.len(), "outputs")?;
                for o in outputs {
                    match o {
                        NodeOutput::Unit(octets) => {
                            b.push(1);
                            put_octets(&mut b, octets)?;
                        }
                        NodeOutput::PacketOut(apid, octets) => {
                            b.push(2);
                            b.extend(apid.to_be_bytes());
                            put_octets(&mut b, octets)?;
                        }
                        NodeOutput::PacketIn(apid, octets) => {
                            b.push(3);
                            b.extend(apid.to_be_bytes());
                            put_octets(&mut b, octets)?;
                        }
                        NodeOutput::Event(code, args) => {
                            b.push(4);
                            b.extend(code.to_be_bytes());
                            b.push(
                                u8::try_from(args.len())
                                    .map_err(|_| "event with over 255 arguments")?,
                            );
                            for a in args {
                                b.extend(a.to_be_bytes());
                            }
                        }
                    }
                }
                b.extend(next_deadline.unwrap_or(u64::MAX).to_be_bytes());
            }
            Self::EndAck => b.push(T_END_ACK),
        }
        if b.len() > MAX_BODY {
            return Err(format!(
                "{} body of {} octets over the limit",
                self.name(),
                b.len()
            ));
        }
        Ok(b)
    }

    /// Decodes a body (type octet first).
    ///
    /// # Errors
    /// If the body is short, has trailing octets, or holds an unknown type, kind, role, or
    /// `tx_slot` value, a non-ASCII case ID, or a `next_deadline` that is not all ones but is
    /// the encoding of no instant (every other value is one).
    pub fn decode(body: &[u8]) -> Result<Self, String> {
        let mut c = Cursor(body);
        let t = c.u8()?;
        let m = match t {
            T_HELLO => {
                let version = c.u16()?;
                let scenario_version = c.u16()?;
                let role = match c.u8()? {
                    0 => Role::Flight,
                    1 => Role::Ground,
                    r => return Err(format!("HELLO: unknown role {r}")),
                };
                let n = usize::from(c.u8()?);
                let case_id = String::from_utf8(c.take(n)?.to_vec())
                    .ok()
                    .filter(|s| s.is_ascii())
                    .ok_or("HELLO: case ID is not ASCII")?;
                Self::Hello {
                    version,
                    scenario_version,
                    role,
                    case_id,
                }
            }
            T_STEP => {
                let now = c.u64()?;
                let tx_slot = match c.u8()? {
                    0 => false,
                    1 => true,
                    v => return Err(format!("STEP: tx_slot {v} is not 0 or 1")),
                };
                let mut units = Vec::new();
                for _ in 0..c.u16()? {
                    units.push(c.octets()?);
                }
                let mut ops = Vec::new();
                for _ in 0..c.u16()? {
                    ops.push(c.u16()?);
                }
                Self::Step {
                    now,
                    tx_slot,
                    units,
                    ops,
                }
            }
            T_END => Self::End,
            T_HELLO_ACK => Self::HelloAck { version: c.u16()? },
            T_STEP_RESULT => {
                let mut outputs = Vec::new();
                for _ in 0..c.u16()? {
                    outputs.push(match c.u8()? {
                        1 => NodeOutput::Unit(c.octets()?),
                        2 => NodeOutput::PacketOut(c.u16()?, c.octets()?),
                        3 => NodeOutput::PacketIn(c.u16()?, c.octets()?),
                        4 => {
                            let code = c.u16()?;
                            let mut args = Vec::new();
                            for _ in 0..c.u8()? {
                                args.push(c.u64()?);
                            }
                            NodeOutput::Event(code, args)
                        }
                        k => return Err(format!("STEP_RESULT: unknown output kind {k}")),
                    });
                }
                let d = c.u64()?;
                Self::StepResult {
                    outputs,
                    next_deadline: (d != u64::MAX).then_some(d),
                }
            }
            T_END_ACK => Self::EndAck,
            t => return Err(format!("unknown message type 0x{t:02x}")),
        };
        if c.0.is_empty() {
            Ok(m)
        } else {
            Err(format!("{}: {} trailing octet(s)", m.name(), c.0.len()))
        }
    }
}

fn put_count(b: &mut Vec<u8>, n: usize, what: &str) -> Result<(), String> {
    let n = u16::try_from(n).map_err(|_| format!("more than 65 535 {what}"))?;
    b.extend(n.to_be_bytes());
    Ok(())
}

fn put_octets(b: &mut Vec<u8>, octets: &[u8]) -> Result<(), String> {
    put_count(b, octets.len(), "octets in one field")?;
    b.extend(octets);
    Ok(())
}

/// Reads the fields of a body; every short read is an error.
struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.0.len() < n {
            return Err(format!(
                "message too short: {n} octet(s) wanted, {} left",
                self.0.len()
            ));
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        self.take(N)?
            .try_into()
            .map_err(|_| "message too short".to_owned())
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(u8::from_be_bytes(self.array()?))
    }

    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn octets(&mut self) -> Result<Vec<u8>, String> {
        let n = usize::from(self.u16()?);
        Ok(self.take(n)?.to_vec())
    }
}

/// Writes one message: length, then body, and flushes.
///
/// # Errors
/// If the message does not encode or the stream fails.
pub fn write_message(w: &mut dyn Write, m: &Message) -> Result<(), String> {
    let body = m.encode()?;
    let len = u32::try_from(body.len()).map_err(|_| "body longer than 4 GiB")?;
    let mut frame = len.to_be_bytes().to_vec();
    frame.extend(body);
    w.write_all(&frame)
        .and_then(|()| w.flush())
        .map_err(|e| format!("sending {}: {e}", m.name()))
}

/// Reads one message.
///
/// # Errors
/// If the stream closes (also in the middle of a message), the length is 0 or over
/// [`MAX_BODY`], or the body does not decode.
pub fn read_message(r: &mut dyn Read) -> Result<Message, String> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).map_err(|e| read_error(&e))?;
    let len = usize::try_from(u32::from_be_bytes(len)).map_err(|_| "length does not fit")?;
    if len == 0 || len > MAX_BODY {
        return Err(format!("message length {len} outside 1..={MAX_BODY}"));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).map_err(|e| read_error(&e))?;
    Message::decode(&body)
}

fn read_error(e: &std::io::Error) -> String {
    if e.kind() == ErrorKind::UnexpectedEof {
        "stream closed before a whole message arrived".to_owned()
    } else {
        format!("reading from the node: {e}")
    }
}
