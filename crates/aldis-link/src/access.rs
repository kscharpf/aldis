//! The VC Access generator (`AccessGenerator`): puts `VCA_SDUs` and `MAPA_SDUs` into fixed-length
//! data fields (LNK-020, LNK-002, LNK-003, LNK-074, LNK-075; docs/04-engines.md §2.2, §4).
//!
//! The engine serves one VC (one MAP for USLP) of one protocol (LNK-001): TM and AOS take SDUs
//! of exactly the data field length, one SDU per field, unchanged; USLP with fixed-length frames
//! starts an SDU in the first octet of a TFDZ with rule `001`, carries the rest in following
//! fields with rule `010`, and sets the Last Valid Octet Pointer (132.0 §3.2.3, §4.1.4.3;
//! 732.0 §3.2.4, §4.1.4.1.3; 732.1 §4.1.4.2.2.1.4, §4.1.4.2.4.5–6, §4.2.3.1.2–6). It holds no
//! output queue of fields (API-059): the framework feeds the returned field to the multiplexer.
//!
//! **Bounds (LNK-074, LNK-075, API-062).** Every input returns at most one action
//! ([`ACCESS_GENERATOR_MAX_ACTIONS`]). `Release` writes the data field of the VC to the output
//! area and fails with `BufferTooSmall` when the area is shorter. The other inputs write nothing.

use core::fmt::Debug;

use aldis_core::engine::{Actions, OutRange};
use aldis_core::{Error, ErrorKind, Result, field};

use crate::common::{Counter, Occupancy};
use crate::protocol::{Protocol, Route};

/// The most actions one input of the `AccessGenerator` returns: `Release` returns at most one
/// `DataField`, `Reconfigure` at most one `Discarded` (LNK-074, docs/04-engines.md §4).
pub const ACCESS_GENERATOR_MAX_ACTIONS: usize = 1;

/// The octets that hold the length of a queued SDU.
const PREFIX: usize = 2;

/// The longest SDU, and zone, that the 16-bit length of a queued SDU can count.
const MAX_SDU: usize = 0xFFFF;

/// The Last Valid Octet Pointer of a TFDZ in which the SDU does not end: all ones (732.1
/// §4.1.4.2.4.6).
const LVO_SDU_CONTINUES: u16 = 0xFFFF;

/// A frame protocol whose VCs carry Access SDUs (LNK-001, LNK-020): TM, AOS, and USLP.
pub trait AccessProtocol: Protocol {
    /// The parameters of the protocol that the generator needs: none for TM and AOS; the UPID,
    /// MAP, and `QoS` for USLP.
    type Params: Copy + Debug + Eq;

    /// Whether an SDU may span fields (USLP): the others carry one SDU of exactly the field.
    const SPANNING: bool;

    /// The longest zone whose every offset has a Last Valid Octet Pointer value.
    fn max_zone_len(params: &Self::Params) -> usize;
    /// The MAP and `QoS` the fields are sent on (USLP).
    fn route(params: &Self::Params) -> Option<Route>;
    /// Writes the data field header into `header`, `SDU_HEADER_LEN` octets: rule `001`, or `010`
    /// for a `continuation`, with the raw Last Valid Octet Pointer `pointer` (LNK-020).
    fn write_header(params: &Self::Params, continuation: bool, pointer: u16, header: &mut [u8]);
}

/// The configuration of an `AccessGenerator` (LNK-020, LNK-078).
///
/// Built only by [`new`](Self::new) and the `with_` methods that check the relations between
/// parameters (API-040, RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessGeneratorConfig<P: AccessProtocol> {
    vc: P::Vc,
    params: P::Params,
    field_len: usize,
    max_sdu_len: usize,
    idle_pattern: u8,
}

impl<P: AccessProtocol> AccessGeneratorConfig<P> {
    /// The configuration of VC `vc` whose data field is `field_len` octets, header included
    /// (TM data field, AOS `VCA_SDU`, USLP TFDF). The longest SDU is one zone, and the idle
    /// pattern is `0x00`.
    ///
    /// # Errors
    /// `InvalidConfig` (`field_len`) unless the field holds the header and at least one octet of
    /// zone, the zone has at most 65 535 octets (the length of a queued SDU has 16 bits), and
    /// the pointer of the protocol can address the last octet of the zone.
    ///
    /// Requirements: LNK-020, LNK-002, LNK-003; docs/04-engines.md §2.2.
    pub fn new(vc: P::Vc, params: P::Params, field_len: usize) -> Result<AccessGeneratorConfig<P>> {
        let zone = field_len.saturating_sub(P::SDU_HEADER_LEN);
        // The queue holds the length of an SDU in 16 bits, so a zone above 65 535 octets could
        // take an SDU it cannot count (LNK-020: unchanged).
        if zone == 0 || zone > P::max_zone_len(&params).min(MAX_SDU) {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::FIELD_LEN));
        }
        Ok(AccessGeneratorConfig {
            vc,
            params,
            field_len,
            max_sdu_len: zone,
            idle_pattern: 0,
        })
    }

    /// The same configuration with the longest SDU the generator takes (LNK-020, LNK-073).
    ///
    /// # Errors
    /// `InvalidConfig` (`max_sdu_len`) for 0, for more than 65 535 octets, and for a protocol
    /// whose SDU is exactly one field (TM, AOS) unless it is the zone.
    pub const fn with_max_sdu_len(self, max_sdu_len: usize) -> Result<AccessGeneratorConfig<P>> {
        let zone = self.zone_len();
        if max_sdu_len == 0 || max_sdu_len > MAX_SDU || (!P::SPANNING && max_sdu_len != zone) {
            Err(Error::new(ErrorKind::InvalidConfig).with_field(field::MAX_SDU_LEN))
        } else {
            Ok(AccessGeneratorConfig {
                max_sdu_len,
                ..self
            })
        }
    }

    /// The same configuration with the octet that fills the zone after the end of an SDU
    /// (LNK-020).
    #[must_use]
    pub const fn with_idle_pattern(self, idle_pattern: u8) -> AccessGeneratorConfig<P> {
        AccessGeneratorConfig {
            idle_pattern,
            ..self
        }
    }

    /// The octets of the zone: the data field without its header.
    const fn zone_len(&self) -> usize {
        self.field_len.saturating_sub(P::SDU_HEADER_LEN)
    }

    /// The VC (the MAP for USLP) the generator serves.
    ///
    /// Requirements: LNK-020, LNK-002, LNK-003; docs/04-engines.md §2.2.
    #[must_use]
    pub const fn vc(&self) -> P::Vc {
        self.vc
    }

    /// The octets of the data field, header included.
    ///
    /// Requirements: LNK-020, LNK-002, LNK-003; docs/04-engines.md §2.2.
    #[must_use]
    pub const fn field_len(&self) -> usize {
        self.field_len
    }

    /// The longest SDU.
    ///
    /// Requirements: LNK-020, LNK-002, LNK-003; docs/04-engines.md §2.2.
    #[must_use]
    pub const fn max_sdu_len(&self) -> usize {
        self.max_sdu_len
    }

    /// The octet that fills the zone after an SDU.
    ///
    /// Requirements: LNK-020, LNK-002, LNK-003; docs/04-engines.md §2.2.
    #[must_use]
    pub const fn idle_pattern(&self) -> u8 {
        self.idle_pattern
    }
}

/// An input of the `AccessGenerator` (docs/04-engines.md §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessInput<'a, P: AccessProtocol> {
    /// An SDU, whole (LNK-020).
    SubmitSdu(&'a [u8]),
    /// Give out the next field (LNK-020).
    Release,
    /// Replace the configuration between data units (LNK-003, GEN-061).
    Reconfigure(AccessGeneratorConfig<P>),
}

/// A data field the generator gives out, for the multiplexer's `Offer` (LNK-076, API-054).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessField {
    /// The VCID of the generator.
    pub vc: u8,
    /// Where the field is in the output area: the TM data field, the AOS `VCA_SDU`, the USLP
    /// TFDF (header and TFDZ).
    pub field: OutRange,
    /// The raw Last Valid Octet Pointer of a USLP field; none for TM and AOS.
    pub pointer: Option<u16>,
    /// The MAP and `QoS` of the field (USLP).
    pub route: Option<Route>,
}

/// An action of the `AccessGenerator` (API-057).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessOutput {
    /// A data field, in the output area (LNK-020).
    DataField(AccessField),
    /// `Reconfigure` discarded this many octets: the SDU in progress and the queue (LNK-003).
    Discarded {
        /// The octets of SDUs not yet released.
        octets: usize,
    },
}

/// The actions of one input of the `AccessGenerator` (LNK-074).
pub type AccessActions = Actions<AccessOutput, ACCESS_GENERATOR_MAX_ACTIONS>;

/// The read-only status of an `AccessGenerator`, for housekeeping telemetry (API-056, LNK-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AccessStatus {
    /// SDUs taken.
    pub sdus_in: Counter,
    /// Data fields given out.
    pub fields_out: Counter,
    /// SDUs refused as invalid (LNK-020).
    pub refused: Counter,
    /// SDUs refused with `WouldBlock`.
    pub would_block: Counter,
    /// Octets discarded by `Reconfigure` (LNK-003).
    pub discarded_octets: Counter,
    /// The SDU queue, in octets (API-060).
    pub queue: Occupancy,
}

/// The `AccessGenerator` of one VC (LNK-020).
///
/// `Q` is the capacity of the SDU queue in octets, two octets of length for each SDU included
/// (GEN-011); a configuration whose longest SDU does not fit is refused with
/// `CapacityExceeded`.
///
/// The engine is its [`AccessGeneratorHead`] followed by its [`AccessGeneratorBuffers`]
/// (`repr(C)`), so that a caller can build it in place (see [`crate::inplace`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct AccessGenerator<P: AccessProtocol, const Q: usize> {
    head: AccessGeneratorHead<P>,
    bufs: AccessGeneratorBuffers<Q>,
}

/// Everything of an [`AccessGenerator`] but its queue (see [`crate::inplace`]).
///
/// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-020, LNK-002, LNK-003; docs/04-engines.md §2.2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessGeneratorHead<P: AccessProtocol> {
    config: AccessGeneratorConfig<P>,
    queue_len: usize,
    sent: usize,
    octets: usize,
    status: AccessStatus,
}

/// The SDU queue of an [`AccessGenerator`]: `Q` octets, valid when all zero, which is the empty
/// state (see [`crate::inplace`]).
///
/// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-020, LNK-002, LNK-003; docs/04-engines.md §2.2.
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct AccessGeneratorBuffers<const Q: usize> {
    queue: [u8; Q],
}

impl<const Q: usize> AccessGeneratorBuffers<Q> {
    /// An empty queue.
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-020, LNK-002, LNK-003; docs/04-engines.md §2.2.
    pub const ZERO: Self = Self { queue: [0; Q] };
}

impl<P: AccessProtocol, const Q: usize> AccessGenerator<P, Q> {
    /// A generator with an empty queue.
    ///
    /// # Errors
    /// `CapacityExceeded` (`max_sdu_len`) if the longest SDU and its length do not fit the queue.
    ///
    /// Requirements: LNK-020, LNK-002, LNK-003; docs/04-engines.md §2.2.
    pub fn new(config: AccessGeneratorConfig<P>) -> Result<AccessGenerator<P, Q>> {
        Ok(AccessGenerator {
            head: Self::new_head(config)?,
            bufs: AccessGeneratorBuffers::ZERO,
        })
    }

    /// The offset of the head in the engine, for an in-place build (see [`crate::inplace`]).
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-020, LNK-002, LNK-003; docs/04-engines.md §2.2.
    pub const HEAD_OFFSET: usize = core::mem::offset_of!(Self, head);
    /// The offset of the buffers in the engine, for an in-place build (see [`crate::inplace`]).
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-020, LNK-002, LNK-003; docs/04-engines.md §2.2.
    pub const BUFFERS_OFFSET: usize = core::mem::offset_of!(Self, bufs);

    /// The head of a generator with an empty queue: the part of [`Self::new`] that is built from
    /// the configuration (see [`crate::inplace`]).
    ///
    /// # Errors
    /// As [`Self::new`].
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-020, LNK-002, LNK-003; docs/04-engines.md §2.2.
    pub fn new_head(config: AccessGeneratorConfig<P>) -> Result<AccessGeneratorHead<P>> {
        AccessGenerator::<P, Q>::check_capacity(&config)?;
        Ok(AccessGeneratorHead {
            config,
            queue_len: 0,
            sent: 0,
            octets: 0,
            status: AccessStatus::default(),
        })
    }

    const fn check_capacity(config: &AccessGeneratorConfig<P>) -> Result<()> {
        if config.max_sdu_len.saturating_add(PREFIX) > Q {
            Err(Error::new(ErrorKind::CapacityExceeded).with_field(field::MAX_SDU_LEN))
        } else {
            Ok(())
        }
    }

    /// The configuration in force.
    ///
    /// Requirements: LNK-020, LNK-002, LNK-003; docs/04-engines.md §2.2.
    #[must_use]
    pub const fn config(&self) -> &AccessGeneratorConfig<P> {
        &self.head.config
    }

    /// The status snapshot: counters and occupancy (API-056, LNK-002).
    #[must_use]
    pub const fn status(&self) -> &AccessStatus {
        &self.head.status
    }

    /// Runs one input (API-057).
    ///
    /// An input that fails leaves the queue and the configuration as they were and writes no
    /// action (API-058); the housekeeping counters of the status count a refusal.
    ///
    /// # Errors
    /// - `SubmitSdu`: `LengthMismatch` (`sdu_len`) for an SDU of another length than the data
    ///   field (TM, AOS) or an empty one (USLP), `OutOfRange` (`max_sdu_len`) for a longer SDU;
    ///   `WouldBlock` if the queue cannot take the SDU whole (API-052).
    /// - `Release`: `BufferTooSmall` if `out` is shorter than the data field (LNK-075).
    /// - `Reconfigure`: `CapacityExceeded` as for [`new`](Self::new).
    pub fn step(&mut self, input: AccessInput<'_, P>, out: &mut [u8]) -> Result<AccessActions> {
        let mut actions = AccessActions::new();
        match input {
            AccessInput::SubmitSdu(sdu) => self.submit(sdu)?,
            AccessInput::Release => {
                if let Some(field) = self.release(out)? {
                    actions.push(AccessOutput::DataField(field))?;
                }
            }
            AccessInput::Reconfigure(config) => {
                AccessGenerator::<P, Q>::check_capacity(&config)?;
                let octets = self.head.octets;
                self.head.config = config;
                self.head.queue_len = 0;
                self.head.sent = 0;
                self.head.octets = 0;
                self.head.status.discarded_octets.add(octets);
                self.head.status.queue.set(0);
                if octets > 0 {
                    actions.push(AccessOutput::Discarded { octets })?;
                }
            }
        }
        Ok(actions)
    }

    /// `SubmitSdu` (LNK-020).
    fn submit(&mut self, sdu: &[u8]) -> Result<()> {
        if let Err(e) = self.check_sdu(sdu) {
            self.head.status.refused.incr();
            return Err(e);
        }
        let needed = sdu.len().saturating_add(PREFIX);
        if needed > Q.saturating_sub(self.head.queue_len) {
            self.head.status.would_block.incr();
            return Err(Error::new(ErrorKind::WouldBlock));
        }
        // The length fits 16 bits: `new` limits the zone and `with_max_sdu_len` the SDU to
        // `MAX_SDU`, and `check_sdu` has refused a longer one.
        let [.., a, b] = sdu.len().to_be_bytes();
        let queued = [a, b].into_iter().chain(sdu.iter().copied());
        for (d, s) in self
            .bufs
            .queue
            .iter_mut()
            .skip(self.head.queue_len)
            .zip(queued)
        {
            *d = s;
        }
        self.head.queue_len = self.head.queue_len.saturating_add(needed);
        self.head.octets = self.head.octets.saturating_add(sdu.len());
        self.head.status.sdus_in.incr();
        self.head.status.queue.set(self.head.octets);
        Ok(())
    }

    /// The checks of `SubmitSdu` that depend on the SDU only (LNK-020).
    fn check_sdu(&self, sdu: &[u8]) -> Result<()> {
        let mismatch = Error::new(ErrorKind::LengthMismatch).with_field(field::SDU_LEN);
        let wrong_len = if P::SPANNING {
            sdu.is_empty()
        } else {
            sdu.len() != self.head.config.field_len
        };
        if wrong_len {
            return Err(mismatch);
        }
        if sdu.len() > self.head.config.max_sdu_len {
            return Err(Error::out_of_range(
                field::MAX_SDU_LEN,
                u64::try_from(self.head.config.max_sdu_len).unwrap_or(u64::MAX),
                u64::try_from(sdu.len()).unwrap_or(u64::MAX),
            ));
        }
        Ok(())
    }

    /// The length of the SDU at the head of the queue (LNK-020).
    fn head_len(&self) -> Result<Option<usize>> {
        if self.head.queue_len == 0 {
            return Ok(None);
        }
        let prefix = self
            .bufs
            .queue
            .first_chunk::<PREFIX>()
            .ok_or(Error::new(ErrorKind::InternalError))?;
        Ok(Some(usize::from(u16::from_be_bytes(*prefix))))
    }

    /// `Release` (LNK-020).
    fn release(&mut self, out: &mut [u8]) -> Result<Option<AccessField>> {
        let field_len = self.head.config.field_len;
        let area = out
            .get_mut(..field_len)
            .ok_or(Error::buffer_too_small(field_len))?;
        let Some(len) = self.head_len()? else {
            return Ok(None);
        };
        let zone_len = self.head.config.zone_len();
        let remaining = len.saturating_sub(self.head.sent);
        let done = remaining <= zone_len;
        let take = remaining.min(zone_len);
        // The offset of the last octet of the SDU in the zone; it fits 16 bits where a pointer
        // is written (USLP, `max_zone_len`), and the other protocols have none.
        let [.., hi, lo] = remaining.saturating_sub(1).to_be_bytes();
        let pointer = if done {
            u16::from_be_bytes([hi, lo])
        } else {
            LVO_SDU_CONTINUES
        };
        let (header, zone) = area
            .split_at_mut_checked(P::SDU_HEADER_LEN)
            .ok_or(Error::new(ErrorKind::InternalError))?;
        P::write_header(
            &self.head.config.params,
            self.head.sent > 0,
            pointer,
            header,
        );
        let from = PREFIX.saturating_add(self.head.sent);
        let sdu = self
            .bufs
            .queue
            .iter()
            .skip(from)
            .take(take)
            .copied()
            .chain(core::iter::repeat(self.head.config.idle_pattern));
        // bound: the zone has `zone_len` octets, and `zip` stops at its end.
        for (d, s) in zone.iter_mut().zip(sdu) {
            *d = s;
        }
        if done {
            self.pop(len);
        } else {
            self.head.sent = self.head.sent.saturating_add(take);
        }
        self.head.octets = self.head.octets.saturating_sub(take);
        self.head.status.fields_out.incr();
        self.head.status.queue.set(self.head.octets);
        Ok(Some(AccessField {
            vc: self.head.config.vc.into(),
            field: OutRange::new(0, field_len),
            pointer: P::SPANNING.then_some(pointer),
            route: P::route(&self.head.config.params),
        }))
    }

    /// Removes the SDU of `len` octets at the head of the queue and closes the gap.
    fn pop(&mut self, len: usize) {
        let gone = len.saturating_add(PREFIX);
        let left = self.head.queue_len.saturating_sub(gone);
        // Close the gap octet by octet: a slice method that moves a range could panic.
        for i in 0..left {
            let octet = self.bufs.queue.get(gone.saturating_add(i)).copied();
            if let (Some(octet), Some(dst)) = (octet, self.bufs.queue.get_mut(i)) {
                *dst = octet;
            }
        }
        self.head.queue_len = left;
        self.head.sent = 0;
    }
}

#[cfg(feature = "tm")]
mod tm_impl {
    use super::AccessProtocol;
    use crate::protocol::{Route, Tm};

    impl AccessProtocol for Tm {
        type Params = ();
        const SPANNING: bool = false;

        fn max_zone_len(&(): &()) -> usize {
            usize::MAX
        }

        fn route(&(): &()) -> Option<Route> {
            None
        }

        fn write_header(&(): &(), _: bool, _: u16, _: &mut [u8]) {}
    }
}

#[cfg(feature = "aos")]
mod aos_impl {
    use super::AccessProtocol;
    use crate::protocol::{Aos, Route};

    impl AccessProtocol for Aos {
        type Params = ();
        const SPANNING: bool = false;

        fn max_zone_len(&(): &()) -> usize {
            usize::MAX
        }

        fn route(&(): &()) -> Option<Route> {
            None
        }

        fn write_header(&(): &(), _: bool, _: u16, _: &mut [u8]) {}
    }
}

#[cfg(feature = "uslp")]
mod uslp_impl {
    use aldis_uslp::{LastValidOctet, TfdfHeader, TfdzOffset, TfdzRule};

    use super::AccessProtocol;
    use crate::protocol::{Route, Uslp, UslpParams};

    impl AccessProtocol for Uslp {
        type Params = UslpParams;
        const SPANNING: bool = true;

        fn max_zone_len(_: &UslpParams) -> usize {
            usize::from(TfdzOffset::MAX).saturating_add(1)
        }

        fn route(params: &UslpParams) -> Option<Route> {
            Some(Route {
                map: params.map(),
                qos: params.qos(),
            })
        }

        fn write_header(params: &UslpParams, continuation: bool, pointer: u16, header: &mut [u8]) {
            let last = LastValidOctet::from_raw(pointer);
            let tfdf = TfdfHeader {
                rule: if continuation {
                    TfdzRule::SduContinuation(last)
                } else {
                    TfdzRule::SduStart(last)
                },
                upid: params.upid(),
            };
            for (d, s) in header.iter_mut().zip(tfdf.octets()) {
                *d = s;
            }
        }
    }
}
