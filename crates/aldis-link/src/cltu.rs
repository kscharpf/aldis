//! The `CltuPacker`: groups consecutive TC or USLP frames into the data units given to the coding
//! sublayer (LNK-044, LNK-002, LNK-003, LNK-074, LNK-075, LNK-078; docs/04-engines.md §2.3, §4).
//!
//! The All Frames Generation Function delivers "one or more Transfer Frames" as one data unit, in
//! accordance with the managed parameters Maximum Number of Transfer Frames Given to the Coding
//! Sublayer as a Single Data Unit and, for TC, Maximum Length of Data Unit (232.0 §4.3.8.4,
//! Table 5-1; 732.1 §4.2.11.7, Table 5-1). The packer is a utility of the caller's choosing: it
//! takes the frames in the order given (`AddFrame`), keeps the unit in progress, and returns a unit
//! when it is full, when the next frame does not fit it, or when the caller asks (`Flush`).
//!
//! **Rules (LNK-044).**
//! - A frame that does not fit the unit in progress (it would exceed the maximum number of frames
//!   or the maximum length) starts the next unit, and the unit in progress is returned first.
//! - A unit that is full is returned at once, in the same step: it holds the maximum number of
//!   frames, or it cannot take one more octet.
//! - Frames with different repetition values never share a unit, because the whole unit is
//!   transferred that many times (232.0 §2.4.2, §4.3.8.5; 732.1 §2.4.2, §4.2.11.8). A frame of
//!   the expedited service (Type-BD) asks for no repetition (232.0 §2.4.2, §4.3.8.5; 732.1
//!   §2.4.2 note, §4.2.11.8): its repetition value is 1, whatever value the caller gives.
//! - Truncated and non-truncated USLP frames never share a unit (732.1 Annex D, D1.2.5).
//!
//! **Bounds (LNK-074, LNK-075).** `AddFrame` returns at most two units
//! ([`CLTU_PACKER_MAX_ACTIONS`]): the unit the frame did not fit, and the new unit if the frame
//! fills it. It writes at most `2·U` octets to the output area, `U` being the maximum length of a
//! data unit of the configuration, and fails with `BufferTooSmall` when the area is shorter.
//! `Flush` returns at most one unit and writes at most `U` octets; `Reconfigure` writes nothing.

use core::marker::PhantomData;
use core::num::NonZeroU16;

use aldis_core::engine::{Actions, OutRange};
use aldis_core::{Error, ErrorKind, Result, field};

use crate::common::{Counter, Occupancy, Qos};
use crate::protocol::{Protocol, Tc, Uslp};

/// The most actions one input of the `CltuPacker` returns (LNK-074, docs/04-engines.md §4).
///
/// `AddFrame` returns the pending unit that the frame did not fit and then the new unit if the
/// frame fills it; `Flush` and `Reconfigure` return one action.
pub const CLTU_PACKER_MAX_ACTIONS: usize = 2;

/// A frame protocol whose frames the `CltuPacker` groups (LNK-001): TC and USLP.
pub trait CltuProtocol: Protocol {
    /// Whether the protocol has truncated frames (732.1 Annex D), which never share a unit with
    /// non-truncated ones.
    const TRUNCATED_FRAMES: bool;
}

impl CltuProtocol for Tc {
    const TRUNCATED_FRAMES: bool = false;
}

impl CltuProtocol for Uslp {
    const TRUNCATED_FRAMES: bool = true;
}

/// The Repetitions parameter of the data unit given to the coding sublayer: the number of times
/// the unit is transferred, at least 1 (232.0 §2.4.2; 732.1 §2.4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Repetitions(NonZeroU16);

impl Repetitions {
    /// One transfer: no systematic retransmission.
    ///
    /// Requirements: LNK-044, LNK-002, LNK-003, LNK-078; docs/04-engines.md §2.3.
    pub const ONE: Repetitions = Repetitions(NonZeroU16::MIN);

    /// `n` transfers, or `None` for 0.
    ///
    /// Requirements: LNK-044, LNK-002, LNK-003, LNK-078; docs/04-engines.md §2.3.
    #[must_use]
    pub const fn new(n: u16) -> Option<Repetitions> {
        match NonZeroU16::new(n) {
            Some(n) => Some(Repetitions(n)),
            None => None,
        }
    }

    /// The number of transfers.
    ///
    /// Requirements: LNK-044, LNK-002, LNK-003, LNK-078; docs/04-engines.md §2.3.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

/// The configuration of a `CltuPacker` (232.0 §5.1 Table 5-1; 732.1 §5.1 Table 5-1; LNK-044,
/// LNK-078).
///
/// Built only by [`new`](Self::new), which checks the parameters (API-040, RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CltuPackerConfig<P: CltuProtocol> {
    max_frames: usize,
    max_unit_len: usize,
    protocol: PhantomData<P>,
}

impl<P: CltuProtocol> CltuPackerConfig<P> {
    /// A packer whose units hold at most `max_frames` frames and `max_unit_len` octets.
    ///
    /// `max_unit_len` is the Maximum Length of Data Unit of TC (232.0 Table 5-1); USLP has no
    /// such parameter (732.1 Table 5-1), and there it is the project's bound on a unit and the
    /// size of the packer's storage.
    ///
    /// # Errors
    /// `InvalidConfig` (`max_frames_per_unit`) for 0 frames (LNK-078), (`max_unit_len`) for 0
    /// octets.
    pub const fn new(max_frames: usize, max_unit_len: usize) -> Result<CltuPackerConfig<P>> {
        if max_frames == 0 {
            Err(Error::new(ErrorKind::InvalidConfig).with_field(field::MAX_FRAMES_PER_UNIT))
        } else if max_unit_len == 0 {
            Err(Error::new(ErrorKind::InvalidConfig).with_field(field::MAX_UNIT_LEN))
        } else {
            Ok(CltuPackerConfig {
                max_frames,
                max_unit_len,
                protocol: PhantomData,
            })
        }
    }

    /// The Maximum Number of Transfer Frames Given to the Coding Sublayer as a Single Data Unit.
    ///
    /// Requirements: LNK-044, LNK-002, LNK-003, LNK-078; docs/04-engines.md §2.3.
    #[must_use]
    pub const fn max_frames(&self) -> usize {
        self.max_frames
    }

    /// The Maximum Length of Data Unit Given to the Coding Sublayer, in octets.
    ///
    /// Requirements: LNK-044, LNK-002, LNK-003, LNK-078; docs/04-engines.md §2.3.
    #[must_use]
    pub const fn max_unit_len(&self) -> usize {
        self.max_unit_len
    }
}

/// A frame for the packer (LNK-044).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameIn<'a> {
    /// The whole frame, FECF included.
    pub frame: &'a [u8],
    /// The service: the expedited service (Type-BD) is transferred once whatever
    /// `repetitions` says (LNK-044).
    pub qos: Qos,
    /// The Repetitions value of the VC for this frame (232.0 Table 5-3; 732.1 Table 5-3).
    pub repetitions: Repetitions,
    /// A truncated USLP frame (732.1 Annex D); `false` for TC.
    pub truncated: bool,
}

/// An input of the `CltuPacker` (docs/04-engines.md §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CltuInput<'a, P: CltuProtocol> {
    /// The next frame, in the order of transmission (LNK-044).
    AddFrame(FrameIn<'a>),
    /// Return the unit in progress, if any (LNK-044).
    Flush,
    /// Replace the configuration between data units (LNK-003, GEN-061).
    Reconfigure(CltuPackerConfig<P>),
}

/// A data unit for the coding sublayer (LNK-044, API-054).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataUnit {
    /// Where the frames are in the output area, one after the other.
    pub unit: OutRange,
    /// How many frames it holds.
    pub frames: usize,
    /// The Repetitions parameter to request: 1 for a unit of the expedited service.
    pub repetitions: Repetitions,
    /// Whether the frames are truncated USLP frames.
    pub truncated: bool,
}

/// An action of the `CltuPacker` (API-057).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CltuOutput {
    /// A data unit, in the output area (LNK-044).
    DataUnit(DataUnit),
    /// `Reconfigure` discarded the frames of the unit in progress: this many octets (LNK-003).
    Discarded {
        /// The octets of the frames held.
        octets: usize,
    },
}

/// The actions of one input of the `CltuPacker` (LNK-074).
pub type CltuActions = Actions<CltuOutput, CLTU_PACKER_MAX_ACTIONS>;

/// The read-only status of a `CltuPacker`, for housekeeping telemetry (API-056, LNK-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CltuStatus {
    /// Frames taken.
    pub frames_in: Counter,
    /// Data units returned.
    pub units_out: Counter,
    /// Frames refused as invalid (LNK-044).
    pub refused: Counter,
    /// Octets discarded by `Reconfigure` (LNK-003).
    pub discarded_octets: Counter,
    /// The unit in progress, in octets (API-060).
    pub pending: Occupancy,
}

/// The unit in progress (RS-021: the frame sizes of `CltuInput::AddFrame` decide the next state, so a closed
/// enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    /// No frame is held.
    Empty,
    /// `frames` frames are held, of one repetition value and one truncation class.
    Open {
        frames: usize,
        repetitions: Repetitions,
        truncated: bool,
    },
}

/// The `CltuPacker` (LNK-044).
///
/// `U` is the capacity of the unit in octets (GEN-011); a configuration whose Maximum Length of
/// Data Unit exceeds it is refused with `CapacityExceeded`.
///
/// The engine is its [`CltuPackerHead`] followed by its [`CltuPackerBuffers`] (`repr(C)`), so
/// that a caller can build it in place (see [`crate::inplace`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct CltuPacker<P: CltuProtocol, const U: usize> {
    head: CltuPackerHead<P>,
    bufs: CltuPackerBuffers<U>,
}

/// Everything of a [`CltuPacker`] but its unit buffer (see [`crate::inplace`]).
///
/// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-044, LNK-002, LNK-003, LNK-078; docs/04-engines.md §2.3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CltuPackerHead<P: CltuProtocol> {
    config: CltuPackerConfig<P>,
    len: usize,
    unit: Unit,
    status: CltuStatus,
}

/// The unit buffer of a [`CltuPacker`]: `U` octets, valid when all zero, which is the empty
/// state (see [`crate::inplace`]).
///
/// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-044, LNK-002, LNK-003, LNK-078; docs/04-engines.md §2.3.
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct CltuPackerBuffers<const U: usize> {
    unit: [u8; U],
}

impl<const U: usize> CltuPackerBuffers<U> {
    /// An empty unit.
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-044, LNK-002, LNK-003, LNK-078; docs/04-engines.md §2.3.
    pub const ZERO: Self = Self { unit: [0; U] };
}

impl<P: CltuProtocol, const U: usize> CltuPacker<P, U> {
    /// A packer with no frame held.
    ///
    /// # Errors
    /// `CapacityExceeded` (`max_unit_len`) if the Maximum Length of Data Unit exceeds `U`.
    ///
    /// Requirements: LNK-044, LNK-002, LNK-003, LNK-078; docs/04-engines.md §2.3.
    pub fn new(config: CltuPackerConfig<P>) -> Result<CltuPacker<P, U>> {
        Ok(CltuPacker {
            head: Self::new_head(config)?,
            bufs: CltuPackerBuffers::ZERO,
        })
    }

    /// The offset of the head in the engine, for an in-place build (see [`crate::inplace`]).
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-044, LNK-002, LNK-003, LNK-078; docs/04-engines.md §2.3.
    pub const HEAD_OFFSET: usize = core::mem::offset_of!(Self, head);
    /// The offset of the buffers in the engine, for an in-place build (see [`crate::inplace`]).
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-044, LNK-002, LNK-003, LNK-078; docs/04-engines.md §2.3.
    pub const BUFFERS_OFFSET: usize = core::mem::offset_of!(Self, bufs);

    /// The head of a packer with no frame held: the part of [`Self::new`] that is built from the
    /// configuration (see [`crate::inplace`]).
    ///
    /// # Errors
    /// As [`Self::new`].
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-044, LNK-002, LNK-003, LNK-078; docs/04-engines.md §2.3.
    pub fn new_head(config: CltuPackerConfig<P>) -> Result<CltuPackerHead<P>> {
        CltuPacker::<P, U>::check_capacity(&config)?;
        Ok(CltuPackerHead {
            config,
            len: 0,
            unit: Unit::Empty,
            status: CltuStatus::default(),
        })
    }

    const fn check_capacity(config: &CltuPackerConfig<P>) -> Result<()> {
        if config.max_unit_len > U {
            Err(Error::new(ErrorKind::CapacityExceeded).with_field(field::MAX_UNIT_LEN))
        } else {
            Ok(())
        }
    }

    /// The configuration in force.
    ///
    /// Requirements: LNK-044, LNK-002, LNK-003, LNK-078; docs/04-engines.md §2.3.
    #[must_use]
    pub const fn config(&self) -> &CltuPackerConfig<P> {
        &self.head.config
    }

    /// The status snapshot: counters and occupancy (API-056, LNK-002).
    #[must_use]
    pub const fn status(&self) -> &CltuStatus {
        &self.head.status
    }

    /// Runs one input (API-057). The packer has no timer (API-053).
    ///
    /// An input that fails leaves the unit in progress and the configuration as they were and
    /// writes no action (API-058); the housekeeping counters count a refusal.
    ///
    /// # Errors
    /// - `AddFrame`: `LengthMismatch` (`frame_len`) for an empty frame, `OutOfRange`
    ///   (`max_unit_len`) for a frame longer than a unit, `InvalidField` (`truncated_frame_len`)
    ///   for a truncated frame on a protocol that has none, `BufferTooSmall` if `out` is shorter
    ///   than twice the Maximum Length of Data Unit (LNK-075).
    /// - `Flush`: `BufferTooSmall` if `out` is shorter than the Maximum Length of Data Unit.
    /// - `Reconfigure`: `CapacityExceeded` as for [`new`](Self::new).
    pub fn step(&mut self, input: CltuInput<'_, P>, out: &mut [u8]) -> Result<CltuActions> {
        let mut actions = CltuActions::new();
        match input {
            CltuInput::AddFrame(frame) => self.add_frame(&frame, out, &mut actions)?,
            CltuInput::Flush => {
                let area = self.head.config.max_unit_len;
                if out.len() < area {
                    return Err(Error::buffer_too_small(area));
                }
                if let Some(unit) = self.emit(out, 0)? {
                    actions.push(CltuOutput::DataUnit(unit))?;
                }
            }
            CltuInput::Reconfigure(config) => {
                CltuPacker::<P, U>::check_capacity(&config)?;
                let octets = self.head.len;
                self.head.config = config;
                self.head.unit = Unit::Empty;
                self.head.len = 0;
                self.head.status.discarded_octets.add(octets);
                self.head.status.pending.set(0);
                if octets > 0 {
                    actions.push(CltuOutput::Discarded { octets })?;
                }
            }
        }
        Ok(actions)
    }

    /// `AddFrame` (LNK-044).
    fn add_frame(
        &mut self,
        input: &FrameIn<'_>,
        out: &mut [u8],
        actions: &mut CltuActions,
    ) -> Result<()> {
        let max = self.head.config.max_unit_len;
        let refuse = |status: &mut CltuStatus, e: Error| {
            status.refused.incr();
            e
        };
        if input.frame.is_empty() {
            let e = Error::new(ErrorKind::LengthMismatch).with_field(field::FRAME_LEN);
            return Err(refuse(&mut self.head.status, e));
        }
        if input.frame.len() > max {
            let e = Error::out_of_range(
                field::MAX_UNIT_LEN,
                u64::try_from(max).unwrap_or(u64::MAX),
                u64::try_from(input.frame.len()).unwrap_or(u64::MAX),
            );
            return Err(refuse(&mut self.head.status, e));
        }
        if input.truncated && !P::TRUNCATED_FRAMES {
            let e = Error::new(ErrorKind::InvalidField).with_field(field::TRUNCATED_FRAME_LEN);
            return Err(refuse(&mut self.head.status, e));
        }
        let area = max.saturating_mul(2);
        if out.len() < area {
            return Err(Error::buffer_too_small(area));
        }
        let repetitions = match input.qos {
            Qos::Expedited => Repetitions::ONE,
            Qos::SequenceControlled => input.repetitions,
        };
        let fits = match self.head.unit {
            Unit::Empty => true,
            Unit::Open {
                frames,
                repetitions: held,
                truncated,
            } => {
                held == repetitions
                    && truncated == input.truncated
                    && frames < self.head.config.max_frames
                    && self.head.len.saturating_add(input.frame.len()) <= max
            }
        };
        let mut used = 0;
        if !fits && let Some(unit) = self.emit(out, used)? {
            used = unit.unit.end();
            actions.push(CltuOutput::DataUnit(unit))?;
        }
        // Append the frame to the unit in progress.
        let frames = match self.head.unit {
            Unit::Empty => 1,
            Unit::Open { frames, .. } => frames.saturating_add(1),
        };
        let end = self.head.len.saturating_add(input.frame.len());
        let dst = self
            .bufs
            .unit
            .get_mut(self.head.len..end)
            .ok_or(Error::new(ErrorKind::InternalError))?;
        for (d, s) in dst.iter_mut().zip(input.frame) {
            *d = *s;
        }
        self.head.len = end;
        self.head.unit = Unit::Open {
            frames,
            repetitions,
            truncated: input.truncated,
        };
        self.head.status.frames_in.incr();
        self.head.status.pending.set(self.head.len);
        if (frames >= self.head.config.max_frames || self.head.len >= max)
            && let Some(unit) = self.emit(out, used)?
        {
            actions.push(CltuOutput::DataUnit(unit))?;
        }
        Ok(())
    }

    /// Writes the unit in progress to `out` at `at`, and empties it (LNK-044).
    fn emit(&mut self, out: &mut [u8], at: usize) -> Result<Option<DataUnit>> {
        let (frames, repetitions, truncated) = match self.head.unit {
            Unit::Open {
                frames,
                repetitions,
                truncated,
            } => (frames, repetitions, truncated),
            Unit::Empty => return Ok(None),
        };
        let end = at.saturating_add(self.head.len);
        let dst = out.get_mut(at..end).ok_or(Error::buffer_too_small(end))?;
        let src = self
            .bufs
            .unit
            .get(..self.head.len)
            .ok_or(Error::new(ErrorKind::InternalError))?;
        for (d, s) in dst.iter_mut().zip(src) {
            *d = *s;
        }
        self.head.len = 0;
        self.head.unit = Unit::Empty;
        self.head.status.units_out.incr();
        self.head.status.pending.set(0);
        Ok(Some(DataUnit {
            unit: OutRange::new(at, end.saturating_sub(at)),
            frames,
            repetitions,
            truncated,
        }))
    }
}
