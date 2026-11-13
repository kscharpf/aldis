//! The ASM search engine: finds the marker in a stream of octets at any bit offset and returns
//! the unit that follows it (131.0 §9.2, §9.3.1, §3.4.2; SYNC-007…SYNC-015, SYNC-017,
//! SYNC-019; M5 plan D3).
//!
//! 131.0 gives the marker and says only that a receiver acquires synchronization by
//! recognising the pattern at its regular spacing (§9.2 note). The engine settles the rest as
//! the maintainer decided (OI-80, OI-81): it reads the stream most significant bit first, in
//! chunks of octets; in [`SearchState::Searching`] it compares the last 32 bits with the ASM at
//! every bit position and takes the first position that differs in at most the configured
//! number of bits (SYNC-007, SYNC-008); in [`SearchState::Collecting`] it takes the next
//! 8 × frame length bits as the unit and does not search them (SYNC-009). It returns each
//! completed unit as an action, shifted to an octet boundary, and goes back to `Searching` with
//! an empty window. It does not derandomize, verify the spacing of the units, hold a lock, or
//! look for the complemented marker (SYNC-006, SYNC-019).
//!
//! The result depends only on the stream, not on how it is cut into chunks (SYNC-010).
//!
//! **Capacities (GEN-011).** `FRAME` is the longest frame length the instance accepts and the
//! size of its partial-unit storage; `CHUNK` is the longest input chunk in octets; `ACTIONS` is
//! the action bound of an input and must be [`max_actions`]`(CHUNK)`, which the constructor
//! checks at compile time (SYNC-013).
//!
//! **Bounds (API-057, API-062).** An input of `c` octets completes at most
//! ⌈`c` / (frame length + 4)⌉ units, because the units that end inside the chunk end at least
//! 8 × (frame length + 4) bits apart: with a frame of 1 octet, at most ⌈`CHUNK` / 5⌉ actions
//! ([`max_actions`], SYNC-013). It writes frame length × ⌈`c` / (frame length + 4)⌉ octets to
//! the output area ([`max_out_len`]), at most `CHUNK` + `FRAME`, the size of an area that fits
//! every input ([`AsmSearch::MAX_OUT_LEN`], SYNC-014).

use aldis_core::engine::{Actions, OutRange};
use aldis_core::{Error, ErrorKind, Result, field};

use crate::asm::ASM;
use crate::config::{AsmErrors, SyncConfig};
use crate::copy_octets;

/// The ASM as the 32-bit word of the search window, first bit most significant.
const ASM_WORD: u32 = u32::from_be_bytes(ASM);

/// The bits of an octet, most significant first (131.0 §9.3.1 Figure 9-1; SYNC-007).
const BIT_MASKS: [u8; 8] = [0x80, 0x40, 0x20, 0x10, 0x08, 0x04, 0x02, 0x01];

/// The most actions an input of `chunk` octets can return for any frame length: ⌈`chunk` / 5⌉,
/// the value for a frame of 1 octet (SYNC-013, API-057).
#[must_use]
pub const fn max_actions(chunk: usize) -> usize {
    chunk.div_ceil(5)
}

/// The most octets an input of `chunk` octets writes to the output area for frames of
/// `frame_len` octets: `frame_len` × ⌈`chunk` / (`frame_len` + 4)⌉ (SYNC-014, API-062).
///
/// It is at most `chunk` + `frame_len`. It saturates instead of overflowing.
#[must_use]
pub const fn max_out_len(frame_len: usize, chunk: usize) -> usize {
    frame_len.saturating_mul(chunk.div_ceil(frame_len.saturating_add(4)))
}

/// The two states of the engine (SYNC-012).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearchState {
    /// Looking for the ASM in the bits read since the search began (SYNC-007).
    Searching,
    /// Taking the 8 × frame length bits of the unit that follows a matched ASM (SYNC-009).
    Collecting,
}

/// An input of the engine (API-057).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchInput<'a> {
    /// The next octets of the stream, read most significant bit first; at most `CHUNK` octets.
    /// An empty chunk changes nothing (SYNC-010, SYNC-014).
    Chunk(&'a [u8]),
    /// Discard the partial unit and the window, restart the skipped-bit count, and keep the
    /// counters; a new configuration takes effect only here (SYNC-015).
    Reset {
        /// The configuration from now on, or `None` to keep the one in force.
        config: Option<SyncConfig>,
    },
}

/// A unit found in the stream: an action of the engine (SYNC-011, API-054, API-061).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchAction {
    unit: OutRange,
    errors: AsmErrors,
    skipped_bits: u64,
}

impl SearchAction {
    /// The unit (the possibly randomized frame) in the output area, shifted to an octet
    /// boundary; its first bit is the most significant bit of its first octet (SYNC-009,
    /// SYNC-011).
    #[must_use]
    pub const fn unit(&self) -> OutRange {
        self.unit
    }

    /// The number of ASM bits that differed, 0 up to the tolerance (SYNC-011).
    #[must_use]
    pub const fn errors(&self) -> AsmErrors {
        self.errors
    }

    /// The number of stream bits between the end of the previous unit, or the start or reset
    /// of the stream, and the first bit of the ASM; saturates at `u64::MAX` (SYNC-011).
    #[must_use]
    pub const fn skipped_bits(&self) -> u64 {
        self.skipped_bits
    }
}

/// The actions of one input, in stream order, at most `N` = [`max_actions`]`(CHUNK)` (SYNC-011,
/// SYNC-013, API-057).
pub type SearchActions<const N: usize> = Actions<SearchAction, N>;

/// The read-only status of the engine, for housekeeping telemetry (SYNC-015, API-056).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchStatus {
    units_returned: u64,
    skipped_bits_total: u64,
    state: SearchState,
    partial_octets: usize,
}

impl SearchStatus {
    /// The number of units returned since the engine was built; a Reset keeps it (SYNC-015).
    #[must_use]
    pub const fn units_returned(&self) -> u64 {
        self.units_returned
    }

    /// The sum of the `skipped_bits` of the units returned, saturating at `u64::MAX`; a Reset
    /// keeps it, and the bits it discards count for nothing (SYNC-015, OI-83).
    #[must_use]
    pub const fn skipped_bits_total(&self) -> u64 {
        self.skipped_bits_total
    }

    /// The state (SYNC-012, SYNC-015).
    #[must_use]
    pub const fn state(&self) -> SearchState {
        self.state
    }

    /// The complete octets of the partial unit, rounded down; 0 while `Searching` (SYNC-015,
    /// OI-83).
    #[must_use]
    pub const fn partial_octets(&self) -> usize {
        self.partial_octets
    }
}

/// What the state holds in each of the two states (SYNC-012, RS-021: the stream decides the
/// next state, so a closed enum matched without a wildcard arm).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The last (up to) 32 stream bits, the newest in bit 0, and the bits read since the
    /// search began.
    Searching { window: u32, read: u64 },
    /// An ASM matched with `errors` differing bits after `skipped` skipped bits; `collected`
    /// bits of the unit are in the partial-unit storage.
    Collecting {
        errors: AsmErrors,
        skipped: u64,
        collected: usize,
    },
}

/// What one stream bit did to the phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Advance {
    /// The bit is not part of a unit.
    Passed,
    /// The bit is the bit `index` of the unit in progress.
    Stored { index: usize },
    /// The bit is the last bit of a unit, at `index`, which is complete.
    Completed {
        index: usize,
        errors: AsmErrors,
        skipped: u64,
    },
}

impl Phase {
    /// The empty window of a search that has just begun (SYNC-007).
    const START: Phase = Phase::Searching { window: 0, read: 0 };

    /// Takes the next stream bit (SYNC-007…SYNC-009).
    fn advance(&mut self, bit: bool, unit_bits: usize, tolerance: AsmErrors) -> Advance {
        match *self {
            Phase::Searching { window, read } => {
                let window = (window << 1) | u32::from(bit);
                let read = read.saturating_add(1);
                // A match needs 32 bits read since the search began (SYNC-007).
                let matched = if read >= 32 {
                    AsmErrors::new((window ^ ASM_WORD).count_ones())
                        .ok()
                        .filter(|errors| *errors <= tolerance)
                } else {
                    None
                };
                *self = match matched {
                    Some(errors) => Phase::Collecting {
                        errors,
                        skipped: read.saturating_sub(32),
                        collected: 0,
                    },
                    None => Phase::Searching { window, read },
                };
                Advance::Passed
            }
            Phase::Collecting {
                errors,
                skipped,
                collected,
            } => {
                let next = collected.saturating_add(1);
                if next >= unit_bits {
                    *self = Phase::START;
                    Advance::Completed {
                        index: collected,
                        errors,
                        skipped,
                    }
                } else {
                    *self = Phase::Collecting {
                        errors,
                        skipped,
                        collected: next,
                    };
                    Advance::Stored { index: collected }
                }
            }
        }
    }
}

/// The ASM search engine (131.0 §9.2; SYNC-007…SYNC-015, SYNC-019).
///
/// `FRAME`, `CHUNK`, and `ACTIONS` are the capacities of the instance (GEN-011); see the
/// [module](self) for their meaning. A type is built as
/// `AsmSearch<64, 128, { max_actions(128) }>`.
///
/// The engine is its [`AsmSearchHead`] and its [`AsmSearchBuffers`]. The fields are public so that
/// a caller can build it in place, in storage it owns (CABI-040, GEN-012): the partial-unit
/// storage of a `FRAME` of 65536 octets would otherwise pass through the stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsmSearch<const FRAME: usize, const CHUNK: usize, const ACTIONS: usize> {
    /// Everything but the partial-unit storage.
    pub head: AsmSearchHead,
    /// The partial-unit storage, valid when all zero.
    pub bufs: AsmSearchBuffers<FRAME>,
}

/// Everything of an [`AsmSearch`] but its partial-unit storage: the configuration, the phase,
/// and the counters (see [`AsmSearch`]).
///
/// Requirements: SYNC-012, SYNC-015, CABI-040 (in-place construction), CABI-030.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsmSearchHead {
    config: SyncConfig,
    phase: Phase,
    units_returned: u64,
    skipped_bits_total: u64,
}

/// The partial-unit storage of an [`AsmSearch`]: `FRAME` octets, valid when all zero, which is
/// the empty state (see [`AsmSearch`]).
///
/// Requirements: SYNC-012, CABI-040 (in-place construction), CABI-030.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsmSearchBuffers<const FRAME: usize> {
    partial: [u8; FRAME],
}

impl<const FRAME: usize> AsmSearchBuffers<FRAME> {
    /// Empty storage: no bit of a unit is held.
    ///
    /// Requirements: SYNC-012, CABI-040 (in-place construction).
    pub const ZERO: Self = Self {
        partial: [0; FRAME],
    };
}

impl<const FRAME: usize, const CHUNK: usize, const ACTIONS: usize>
    AsmSearch<FRAME, CHUNK, ACTIONS>
{
    /// The size of an output area that fits every input: `CHUNK` + `FRAME` octets (SYNC-014,
    /// API-062). An input writes at most [`max_out_len`]`(frame_len, c)`, which is at most
    /// this.
    pub const MAX_OUT_LEN: usize = CHUNK.saturating_add(FRAME);

    /// The compile-time check of the action bound (SYNC-013): `ACTIONS` is ⌈`CHUNK` / 5⌉.
    const BOUND_HOLDS: () = assert!(
        ACTIONS == max_actions(CHUNK),
        "ACTIONS must be max_actions(CHUNK) (SYNC-013)"
    );

    /// An engine in `Searching` with an empty window.
    ///
    /// It does not compile unless `ACTIONS` is [`max_actions`]`(CHUNK)` (SYNC-013):
    ///
    /// ```compile_fail
    /// use aldis_sync::{AsmErrors, AsmSearch, SyncConfig, SyncProtocol};
    /// let config = SyncConfig::new(SyncProtocol::Tm, 4, AsmErrors::NONE).unwrap();
    /// // 128 octets need 26 actions, not 25.
    /// let _ = AsmSearch::<4, 128, 25>::new(config);
    /// ```
    ///
    /// # Errors
    /// `CapacityExceeded` when the frame length of `config` is above `FRAME` (SYNC-002).
    pub fn new(config: SyncConfig) -> Result<AsmSearch<FRAME, CHUNK, ACTIONS>> {
        Ok(AsmSearch {
            head: Self::new_head(config)?,
            bufs: AsmSearchBuffers::ZERO,
        })
    }

    /// The head of an engine in `Searching` with an empty window: the part of [`Self::new`]
    /// that is built from the configuration, for an in-place build.
    ///
    /// # Errors
    /// As [`Self::new`].
    ///
    /// Requirements: CABI-040 (in-place construction), SYNC-002, SYNC-012, SYNC-013.
    pub fn new_head(config: SyncConfig) -> Result<AsmSearchHead> {
        let () = Self::BOUND_HOLDS;
        Self::check_capacity(&config)?;
        Ok(AsmSearchHead {
            config,
            phase: Phase::START,
            units_returned: 0,
            skipped_bits_total: 0,
        })
    }

    const fn check_capacity(config: &SyncConfig) -> Result<()> {
        if config.frame_len() > FRAME {
            Err(Error::new(ErrorKind::CapacityExceeded))
        } else {
            Ok(())
        }
    }

    /// The configuration in force (SYNC-015).
    #[must_use]
    pub const fn config(&self) -> &SyncConfig {
        &self.head.config
    }

    /// The state (SYNC-012).
    #[must_use]
    pub const fn state(&self) -> SearchState {
        match self.head.phase {
            Phase::Searching { .. } => SearchState::Searching,
            Phase::Collecting { .. } => SearchState::Collecting,
        }
    }

    /// The status snapshot (SYNC-015, API-056).
    #[must_use]
    pub const fn status(&self) -> SearchStatus {
        let partial_octets = match self.head.phase {
            Phase::Searching { .. } => 0,
            Phase::Collecting { collected, .. } => collected / 8,
        };
        SearchStatus {
            units_returned: self.head.units_returned,
            skipped_bits_total: self.head.skipped_bits_total,
            state: self.state(),
            partial_octets,
        }
    }

    /// Runs one input (API-057). The engine has no timer (API-053).
    ///
    /// A chunk is searched bit by bit; each unit it completes is written to `out`, one after
    /// the other from offset 0, and returned as an action (SYNC-011). An input that fails
    /// leaves the state as it was and writes no action (API-058).
    ///
    /// # Errors
    /// - `Chunk`: `OutOfRange` naming `chunk_len` for a chunk of more than `CHUNK` octets;
    ///   `BufferTooSmall{needed}` when `out` is shorter than the units the chunk completes,
    ///   `needed` being their exact length (SYNC-014).
    /// - `Reset`: `CapacityExceeded` when the new configuration has a frame length above
    ///   `FRAME`.
    /// - `InternalError` when a bound that holds by construction does not. It is never
    ///   returned: the output area is checked against the exact length of the units the chunk
    ///   completes before the first one is written, and the compile-time check of `ACTIONS`
    ///   keeps the action list from overflowing. Were it returned, the state-unchanged
    ///   promise above (API-058) would not hold for a chunk part-way through.
    pub fn step(
        &mut self,
        input: SearchInput<'_>,
        out: &mut [u8],
    ) -> Result<SearchActions<ACTIONS>> {
        match input {
            SearchInput::Chunk(data) => self.chunk(data, out),
            SearchInput::Reset { config } => {
                if let Some(config) = config {
                    Self::check_capacity(&config)?;
                    self.head.config = config;
                }
                self.head.phase = Phase::START;
                Ok(SearchActions::new())
            }
        }
    }

    fn chunk(&mut self, data: &[u8], out: &mut [u8]) -> Result<SearchActions<ACTIONS>> {
        if data.len() > CHUNK {
            return Err(Error::out_of_range(
                field::CHUNK_LEN,
                u64::try_from(CHUNK).unwrap_or(u64::MAX),
                u64::try_from(data.len()).unwrap_or(u64::MAX),
            ));
        }
        let frame_len = self.head.config.frame_len();
        let unit_bits = frame_len.saturating_mul(8);
        let tolerance = self.head.config.asm_errors();
        // The units the chunk completes, counted on a copy of the phase, so that a refusal
        // changes nothing (API-058).
        let mut trial = self.head.phase;
        let mut completed = 0_usize;
        for &octet in data {
            for mask in BIT_MASKS {
                if let Advance::Completed { .. } =
                    trial.advance(octet & mask != 0, unit_bits, tolerance)
                {
                    completed = completed.saturating_add(1);
                }
            }
        }
        let needed = completed.saturating_mul(frame_len);
        if out.len() < needed {
            return Err(Error::buffer_too_small(needed));
        }
        let mut actions = SearchActions::new();
        let mut written = 0_usize;
        for &octet in data {
            for mask in BIT_MASKS {
                let bit = octet & mask != 0;
                match self.head.phase.advance(bit, unit_bits, tolerance) {
                    Advance::Passed => {}
                    Advance::Stored { index } => self.store(index, bit),
                    Advance::Completed {
                        index,
                        errors,
                        skipped,
                    } => {
                        self.store(index, bit);
                        let unit = OutRange::new(written, frame_len);
                        let (Some(from), Some(to)) = (
                            self.bufs.partial.get(..frame_len),
                            out.get_mut(unit.start()..unit.end()),
                        ) else {
                            return Err(Error::new(ErrorKind::InternalError));
                        };
                        copy_octets(to, from);
                        written = unit.end();
                        actions.push(SearchAction {
                            unit,
                            errors,
                            skipped_bits: skipped,
                        })?;
                        self.head.units_returned = self.head.units_returned.saturating_add(1);
                        self.head.skipped_bits_total =
                            self.head.skipped_bits_total.saturating_add(skipped);
                    }
                }
            }
        }
        Ok(actions)
    }

    /// Writes bit `index` of the unit in progress. The first bit of an octet clears the octet,
    /// so the storage needs no clearing between units.
    fn store(&mut self, index: usize, bit: bool) {
        if let (Some(octet), Some(&mask)) = (
            self.bufs.partial.get_mut(index / 8),
            BIT_MASKS.get(index % 8),
        ) {
            if mask == 0x80 {
                *octet = 0;
            }
            if bit {
                *octet |= mask;
            }
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{ASM_WORD, Advance, AsmSearch, Phase, SearchInput, max_actions, max_out_len};
    use crate::config::{AsmErrors, SyncConfig, SyncProtocol};

    type Engine = AsmSearch<2, 8, { max_actions(8) }>;

    fn config() -> SyncConfig {
        SyncConfig::new(SyncProtocol::Tm, 2, AsmErrors::NONE).unwrap()
    }

    #[test]
    fn the_read_count_and_the_skipped_bits_saturate() {
        // req: SYNC-011, SYNC-015
        // The last 31 bits of the ASM are in the window; the next bit completes it.
        let mut phase = Phase::Searching {
            window: ASM_WORD >> 1,
            read: u64::MAX,
        };
        let last = ASM_WORD & 1 == 1;
        assert_eq!(phase.advance(last, 16, AsmErrors::NONE), Advance::Passed);
        assert_eq!(
            phase,
            Phase::Collecting {
                errors: AsmErrors::NONE,
                skipped: u64::MAX - 32,
                collected: 0
            }
        );
        // The total of the skipped bits saturates too.
        let mut engine = Engine::new(config()).unwrap();
        engine.head.skipped_bits_total = u64::MAX - 1;
        let mut stream = [0_u8; 8];
        stream[0] = 0x80; // one bit of junk, then the ASM and the unit
        let cadu = [0x0D, 0x67, 0xFE, 0x0E, 0x80, 0x00, 0x00, 0x00];
        stream.copy_from_slice(&cadu);
        let mut out = [0_u8; Engine::MAX_OUT_LEN];
        let actions = engine.step(SearchInput::Chunk(&stream), &mut out).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(engine.status().skipped_bits_total(), u64::MAX);
    }

    #[test]
    fn the_bounds_follow_the_requirements() {
        // req: SYNC-013, SYNC-014
        assert_eq!(max_actions(0), 0);
        assert_eq!(max_actions(1), 1);
        assert_eq!(max_actions(5), 1);
        assert_eq!(max_actions(6), 2);
        assert_eq!(max_actions(128), 26);
        assert_eq!(max_out_len(1, 5), 1);
        assert_eq!(max_out_len(1, 6), 2);
        assert_eq!(max_out_len(3, 14), 6);
        assert_eq!(max_out_len(usize::MAX, usize::MAX), usize::MAX);
        assert_eq!(Engine::MAX_OUT_LEN, 10);
        // Never more than CHUNK + FRAME, for every frame length and chunk.
        for frame in 1..=64 {
            for chunk in 0..=300 {
                assert!(max_out_len(frame, chunk) <= chunk + frame);
                assert!(max_out_len(frame, chunk) <= max_actions(chunk) * frame);
            }
        }
    }
}
