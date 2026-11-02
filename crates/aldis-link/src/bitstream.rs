//! The AOS Bitstream generator (`BitstreamGenerator`): fills the Bitstream Data Zone of a
//! `B_PDU` with bits (LNK-021, LNK-002, LNK-003, LNK-074, LNK-075; docs/04-engines.md §2.2, §4).
//!
//! The engine serves one VC of the AOS protocol (LNK-001). It takes bits (`SubmitBits`) into a
//! bit queue and fills the zone in order and unchanged when the field is released (`Release`);
//! a `B_PDU` that is not full is completed with the idle pattern (732.0 §4.1.4.3.3.2–4,
//! §4.1.4.3.4, §4.2.3.2). It holds no output queue of fields (API-059).
//!
//! **Bounds (LNK-074, LNK-075).** Every input returns at most one action
//! ([`BITSTREAM_GENERATOR_MAX_ACTIONS`]). `Release` writes the data field of the VC to the
//! output area and fails with `BufferTooSmall` when the area is shorter. The other inputs write
//! nothing.

use aldis_core::engine::{Actions, OutRange};
use aldis_core::{Error, ErrorKind, Result, field};

use crate::common::{Counter, Occupancy};
use crate::protocol::Protocol;

/// The most actions one input of the `BitstreamGenerator` returns: `Release` returns at most one
/// `DataField`, `Reconfigure` at most one `Discarded` (LNK-074, docs/04-engines.md §4).
pub const BITSTREAM_GENERATOR_MAX_ACTIONS: usize = 1;

/// The Bitstream Data Pointer of a zone that holds only valid bits: all ones (732.0
/// §4.1.4.3.3.3).
const ALL_VALID: u16 = 0x3FFF;

/// The Bitstream Data Pointer of a zone that holds no valid bit: all ones minus one (732.0
/// §4.1.4.3.3.4).
const ONLY_IDLE: u16 = 0x3FFE;

/// A frame protocol whose VCs carry Bitstream data: AOS (LNK-001, LNK-021).
pub trait BitstreamProtocol: Protocol {
    /// The octets of the `B_PDU` header (732.0 §4.1.4.3.1).
    const HEADER_LEN: usize;
    /// The longest Bitstream Data Zone in octets: the longest whose every bit index lies below
    /// the two reserved pointer values (732.0 §4.1.4.3.3.4 note; AOS-018, OI-53).
    const MAX_ZONE_LEN: usize;
}

/// The configuration of a `BitstreamGenerator` (LNK-021, LNK-078).
///
/// Built only by [`new`](Self::new) (API-040, RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitstreamGeneratorConfig<P: BitstreamProtocol> {
    vc: P::Vc,
    field_len: usize,
    idle_pattern: u8,
}

impl<P: BitstreamProtocol> BitstreamGeneratorConfig<P> {
    /// The configuration of VC `vc` whose `B_PDU` is `field_len` octets, header included, with
    /// the idle pattern `idle_pattern` (LNK-021).
    ///
    /// # Errors
    /// `InvalidConfig` (`field_len`) unless the zone holds 1 to 2 047 octets.
    pub const fn new(
        vc: P::Vc,
        field_len: usize,
        idle_pattern: u8,
    ) -> Result<BitstreamGeneratorConfig<P>> {
        let zone = field_len.saturating_sub(P::HEADER_LEN);
        if zone == 0 || zone > P::MAX_ZONE_LEN {
            Err(Error::new(ErrorKind::InvalidConfig).with_field(field::FIELD_LEN))
        } else {
            Ok(BitstreamGeneratorConfig {
                vc,
                field_len,
                idle_pattern,
            })
        }
    }

    /// The VC the generator serves.
    #[must_use]
    pub const fn vc(&self) -> P::Vc {
        self.vc
    }

    /// The octets of the `B_PDU`, header included.
    #[must_use]
    pub const fn field_len(&self) -> usize {
        self.field_len
    }

    /// The octet whose bits complete a zone that is not full (LNK-021).
    #[must_use]
    pub const fn idle_pattern(&self) -> u8 {
        self.idle_pattern
    }

    /// The bits of the Bitstream Data Zone.
    const fn zone_bits(&self) -> usize {
        self.field_len
            .saturating_sub(P::HEADER_LEN)
            .saturating_mul(8)
    }
}

/// An input of the `BitstreamGenerator` (docs/04-engines.md §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitstreamInput<'a, P: BitstreamProtocol> {
    /// Bits for the stream (LNK-021).
    SubmitBits {
        /// The bits, most significant first and left-aligned.
        bits: &'a [u8],
        /// How many bits of `bits` to take; those after them in the last octet are ignored.
        bit_len: usize,
    },
    /// Give out the next `B_PDU` (LNK-021).
    Release,
    /// Replace the configuration between data units (LNK-003, GEN-061).
    Reconfigure(BitstreamGeneratorConfig<P>),
}

/// A `B_PDU` the generator gives out, for the multiplexer's `Offer` (LNK-076, API-054).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitstreamField {
    /// The VCID of the generator.
    pub vc: u8,
    /// Where the `B_PDU` is in the output area.
    pub field: OutRange,
    /// The raw Bitstream Data Pointer, which the field also holds.
    pub pointer: u16,
}

/// An action of the `BitstreamGenerator` (API-057).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitstreamOutput {
    /// A `B_PDU`, in the output area (LNK-021).
    DataField(BitstreamField),
    /// `Reconfigure` discarded the queued bits, counted in whole octets (LNK-003).
    Discarded {
        /// The octets that held the bits.
        octets: usize,
    },
}

/// The actions of one input of the `BitstreamGenerator` (LNK-074).
pub type BitstreamActions = Actions<BitstreamOutput, BITSTREAM_GENERATOR_MAX_ACTIONS>;

/// The read-only status of a `BitstreamGenerator`, for housekeeping telemetry (API-056, LNK-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BitstreamStatus {
    /// Submissions taken.
    pub submissions: Counter,
    /// Data fields given out, idle ones included.
    pub fields_out: Counter,
    /// Idle `B_PDUs` given out: no bit was queued (LNK-021).
    pub idle_fields: Counter,
    /// Submissions refused as invalid.
    pub refused: Counter,
    /// Submissions refused with `WouldBlock`.
    pub would_block: Counter,
    /// Octets discarded by `Reconfigure` (LNK-003).
    pub discarded_octets: Counter,
    /// The bit queue, in octets (API-060).
    pub queue: Occupancy,
}

/// The mask of bit `i` of an octet, counting from the most significant bit (0 to 7).
const fn mask(i: usize) -> u8 {
    match i % 8 {
        0 => 0x80,
        1 => 0x40,
        2 => 0x20,
        3 => 0x10,
        4 => 0x08,
        5 => 0x04,
        6 => 0x02,
        _ => 0x01,
    }
}

/// Bit `i` of `octets`, counting from the most significant bit of the first octet; clear past
/// the end.
fn bit(octets: &[u8], i: usize) -> bool {
    octets.get(i / 8).is_some_and(|o| o & mask(i) != 0)
}

/// Sets bit `i` of `octets` to `value`; nothing past the end.
fn set_bit(octets: &mut [u8], i: usize, value: bool) {
    if let Some(o) = octets.get_mut(i / 8) {
        if value {
            *o |= mask(i);
        } else {
            *o &= !mask(i);
        }
    }
}

/// The `BitstreamGenerator` of one VC (LNK-021).
///
/// `Q` is the capacity of the bit queue in octets (GEN-011).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitstreamGenerator<P: BitstreamProtocol, const Q: usize> {
    config: BitstreamGeneratorConfig<P>,
    queue: [u8; Q],
    bits: usize,
    status: BitstreamStatus,
}

impl<P: BitstreamProtocol, const Q: usize> BitstreamGenerator<P, Q> {
    /// A generator with an empty queue.
    #[must_use]
    pub const fn new(config: BitstreamGeneratorConfig<P>) -> BitstreamGenerator<P, Q> {
        BitstreamGenerator {
            config,
            queue: [0; Q],
            bits: 0,
            status: BitstreamStatus {
                submissions: Counter::new(),
                fields_out: Counter::new(),
                idle_fields: Counter::new(),
                refused: Counter::new(),
                would_block: Counter::new(),
                discarded_octets: Counter::new(),
                queue: Occupancy::new(),
            },
        }
    }

    /// The configuration in force.
    #[must_use]
    pub const fn config(&self) -> &BitstreamGeneratorConfig<P> {
        &self.config
    }

    /// The status snapshot: counters and occupancy (API-056, LNK-002).
    #[must_use]
    pub const fn status(&self) -> &BitstreamStatus {
        &self.status
    }

    /// Runs one input (API-057).
    ///
    /// An input that fails leaves the queue and the configuration as they were and writes no
    /// action (API-058); the housekeeping counters of the status count a refusal.
    ///
    /// # Errors
    /// - `SubmitBits`: `LengthMismatch` (`sdu_len`) for no bit or for more bits than `bits`
    ///   holds; `WouldBlock` if the queue cannot take the bits whole (API-052).
    /// - `Release`: `BufferTooSmall` if `out` is shorter than the `B_PDU` (LNK-075).
    pub fn step(
        &mut self,
        input: BitstreamInput<'_, P>,
        out: &mut [u8],
    ) -> Result<BitstreamActions> {
        let mut actions = BitstreamActions::new();
        match input {
            BitstreamInput::SubmitBits { bits, bit_len } => self.submit(bits, bit_len)?,
            BitstreamInput::Release => {
                let field = self.release(out)?;
                actions.push(BitstreamOutput::DataField(field))?;
            }
            BitstreamInput::Reconfigure(config) => {
                let octets = self.bits.div_ceil(8);
                self.config = config;
                self.bits = 0;
                self.queue.fill(0);
                self.status.discarded_octets.add(octets);
                self.status.queue.set(0);
                if octets > 0 {
                    actions.push(BitstreamOutput::Discarded { octets })?;
                }
            }
        }
        Ok(actions)
    }

    /// `SubmitBits` (LNK-021).
    fn submit(&mut self, bits: &[u8], bit_len: usize) -> Result<()> {
        if bit_len == 0 || bit_len > bits.len().saturating_mul(8) {
            self.status.refused.incr();
            return Err(Error::new(ErrorKind::LengthMismatch).with_field(field::SDU_LEN));
        }
        if bit_len > Q.saturating_mul(8).saturating_sub(self.bits) {
            self.status.would_block.incr();
            return Err(Error::new(ErrorKind::WouldBlock));
        }
        for i in 0..bit_len {
            set_bit(&mut self.queue, self.bits.saturating_add(i), bit(bits, i));
        }
        self.bits = self.bits.saturating_add(bit_len);
        self.status.submissions.incr();
        self.status.queue.set(self.bits.div_ceil(8));
        Ok(())
    }

    /// `Release` (LNK-021).
    fn release(&mut self, out: &mut [u8]) -> Result<BitstreamField> {
        let field_len = self.config.field_len;
        let area = out
            .get_mut(..field_len)
            .ok_or(Error::buffer_too_small(field_len))?;
        let (header, zone) = area
            .split_at_mut_checked(P::HEADER_LEN)
            .ok_or(Error::new(ErrorKind::InternalError))?;
        let zone_bits = self.config.zone_bits();
        let take = self.bits.min(zone_bits);
        let pointer = if self.bits >= zone_bits {
            ALL_VALID
        } else if self.bits == 0 {
            self.status.idle_fields.incr();
            ONLY_IDLE
        } else {
            // The index of the last valid bit; it is below the zone, which has fewer than
            // `0x3FFE` bits (`MAX_ZONE_LEN`).
            let [.., hi, lo] = take.saturating_sub(1).to_be_bytes();
            u16::from_be_bytes([hi, lo])
        };
        // The two bits of the header's first octet before the pointer are spare, zero.
        let [hi, lo] = pointer.to_be_bytes();
        for (d, s) in header.iter_mut().zip([hi & 0x3F, lo]) {
            *d = s;
        }
        let idle = self.config.idle_pattern;
        for i in 0..zone_bits {
            let value = if i < take {
                bit(&self.queue, i)
            } else {
                idle & mask(i) != 0
            };
            set_bit(zone, i, value);
        }
        self.consume(take);
        self.status.fields_out.incr();
        Ok(BitstreamField {
            vc: self.config.vc.into(),
            field: OutRange::new(0, field_len),
            pointer,
        })
    }

    /// Removes the first `n` bits of the queue (at most the queue holds) and shifts the rest to
    /// the front.
    fn consume(&mut self, n: usize) {
        let left = self.bits.saturating_sub(n);
        for i in 0..left {
            let value = bit(&self.queue, n.saturating_add(i));
            set_bit(&mut self.queue, i, value);
        }
        for i in left..self.bits {
            set_bit(&mut self.queue, i, false);
        }
        self.bits = left;
        self.status.queue.set(left.div_ceil(8));
    }
}

#[cfg(feature = "aos")]
mod aos_impl {
    use super::BitstreamProtocol;
    use crate::protocol::Aos;

    impl BitstreamProtocol for Aos {
        const HEADER_LEN: usize = aldis_aos::bpdu::BPDU_HEADER_LEN;
        const MAX_ZONE_LEN: usize = aldis_aos::bpdu::MAX_BITSTREAM_ZONE_LEN;
    }
}
