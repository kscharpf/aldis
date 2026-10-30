//! C ABI of the stateful Space Packet helpers in caller-owned storage: `SequenceTracker`,
//! `Segmenter`, and `Reassembler` (133.0 §4.1.3.4; SPP-011, SPP-013; M2 plan D5;
//! CABI-030…032).
//!
//! Each helper has a typed storage struct of `uint64_t` words that the caller declares, an
//! `_init` that places the helper in it (the pointer to it is the handle), calls on the handle,
//! and a `_deinit`. The SDU and the reassembly buffer are passed on every call and never stored
//! (API-060). The storage sizes hold on every supported target, 32- and 64-bit; the
//! compile-time check of `handle` proves it for the target being built.

use aldis::core::counter::Continuity;
use aldis::core::ids::{Apid, SeqCount};
use aldis::core::{Error, ErrorDetail, ErrorKind, Result, field};
use aldis::spp::{
    Discard, ReassemblerState, Segmenter, SegmenterState, SequenceCounter, SequenceTracker,
    SpacePacket, Status, Tracked,
};

use crate::abi::{self, invalid_argument};
use crate::capacities::ALDIS_SPP_TRACKER_MAX_APIDS;
use crate::handle::{self, Storage};
use crate::mirror::mirror;
use crate::spp::{
    aldis_spp_config_t, aldis_spp_secondary_layout_t, config, layout as secondary_layout,
    packet_type,
};

/// 64-bit words of one `aldis_spp_tracker_t` (CABI-031; M2 plan D5).
///
/// The tag, the capacity, and the tracker at `ALDIS_SPP_TRACKER_MAX_APIDS`; the build fails when
/// the state outgrows it.
pub const ALDIS_SPP_TRACKER_WORDS: usize = 20;
/// 64-bit words of one `aldis_spp_segmenter_t` (CABI-031; M2 plan D5).
pub const ALDIS_SPP_SEGMENTER_WORDS: usize = 12;
/// 64-bit words of one `aldis_spp_reassembler_t` (CABI-031; M2 plan D5).
pub const ALDIS_SPP_REASSEMBLER_WORDS: usize = 8;

/// The sequence-count result of a tracker.
///
/// A `uint32_t` argument or field is one of the enumerators below; the type itself is not a
/// field type of the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_spp_track {
    /// [`aldis_spp_tracked_t::continuity`]: the first count of an APID (SPP-011).
    ALDIS_SPP_TRACK_FIRST = 0,
    /// [`aldis_spp_tracked_t::continuity`]: forward distance 1 (CMN-031, SPP-011).
    ALDIS_SPP_TRACK_IN_SEQUENCE = 1,
    /// [`aldis_spp_tracked_t::continuity`]: forward distance 2 to 8192; `missing` counts were
    /// skipped (CMN-031, SPP-011).
    ALDIS_SPP_TRACK_GAP = 2,
    /// [`aldis_spp_tracked_t::continuity`]: the same count again (CMN-031, SPP-011).
    ALDIS_SPP_TRACK_REPEAT = 3,
    /// [`aldis_spp_tracked_t::continuity`]: any other distance, a reset (CMN-031, SPP-011).
    ALDIS_SPP_TRACK_BACKWARDS = 4,
}
mirror!(pub aldis_spp_track: u32 { ALDIS_SPP_TRACK_FIRST, ALDIS_SPP_TRACK_IN_SEQUENCE, ALDIS_SPP_TRACK_GAP, ALDIS_SPP_TRACK_REPEAT, ALDIS_SPP_TRACK_BACKWARDS });

/// Why a reassembler discarded a packet.
///
/// A `uint32_t` argument or field is one of the enumerators below; the type itself is not a
/// field type of the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_spp_discard {
    /// [`aldis_spp_reassembly_t::discarded`] and `reason`: nothing (SPP-013).
    ALDIS_SPP_DISCARD_NONE = 0,
    /// A sequence gap while assembling (SPP-013).
    ALDIS_SPP_DISCARD_GAP = 1,
    /// A first segment arrived while assembling (SPP-013).
    ALDIS_SPP_DISCARD_RESTARTED = 2,
    /// An unsegmented packet arrived while assembling (SPP-013).
    ALDIS_SPP_DISCARD_INTERRUPTED = 3,
    /// A continuation or last segment with nothing in progress (SPP-013).
    ALDIS_SPP_DISCARD_ORPHAN = 4,
    /// The SDU would exceed the buffer (SPP-013).
    ALDIS_SPP_DISCARD_OVERFLOW = 5,
    /// A reason that a later `aldis-spp` adds (GEN-081).
    ALDIS_SPP_DISCARD_OTHER = 255,
}
mirror!(pub aldis_spp_discard: u32 { ALDIS_SPP_DISCARD_NONE, ALDIS_SPP_DISCARD_GAP, ALDIS_SPP_DISCARD_RESTARTED, ALDIS_SPP_DISCARD_INTERRUPTED, ALDIS_SPP_DISCARD_ORPHAN, ALDIS_SPP_DISCARD_OVERFLOW, ALDIS_SPP_DISCARD_OTHER });
/// The state a reassembler reports after a segment.
///
/// A `uint32_t` argument or field is one of the enumerators below; the type itself is not a
/// field type of the header (OI-57 convention 2; CABI-010).
///
/// cbindgen:prefix-with-name=false
/// cbindgen:rename-all=None
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    non_camel_case_types,
    reason = "the variants are the C constants (OI-57 convention 2)"
)]
pub enum aldis_spp_reassembly {
    /// [`aldis_spp_reassembly_t::status`]: the segment was stored (SPP-013).
    ALDIS_SPP_REASSEMBLY_IN_PROGRESS = 0,
    /// [`aldis_spp_reassembly_t::status`]: the SDU of `sdu_len` octets is complete at the start of
    /// the buffer (SPP-013).
    ALDIS_SPP_REASSEMBLY_COMPLETE = 1,
    /// [`aldis_spp_reassembly_t::status`]: the packet was dropped for `reason` (SPP-013).
    ALDIS_SPP_REASSEMBLY_DROPPED = 2,
}
mirror!(pub aldis_spp_reassembly: u32 { ALDIS_SPP_REASSEMBLY_IN_PROGRESS, ALDIS_SPP_REASSEMBLY_COMPLETE, ALDIS_SPP_REASSEMBLY_DROPPED });

/// The state of an `aldis_spp_tracker_t`: the tracker at the build capacity, and the
/// capacity the caller chose.
#[derive(Debug, Clone)]
pub(crate) struct TrackerState {
    limit: usize,
    tracker: SequenceTracker<ALDIS_SPP_TRACKER_MAX_APIDS>,
}

/// A Packet Sequence Count tracker in caller storage: the caller declares an object of this
/// type, and the pointer to it is the handle (SPP-011, CABI-031, CABI-032).
///
/// The words are the library's: a C caller never reads or writes them.
#[repr(C)]
pub struct aldis_spp_tracker_t {
    /// The slot of [`handle`]: the tag, then the state.
    pub words: [u64; ALDIS_SPP_TRACKER_WORDS],
}

impl Storage for aldis_spp_tracker_t {
    type State = TrackerState;
    const MAGIC: u32 = 0x414C_5354; // "ALST"
}

/// An SDU segmenter in caller storage: the caller declares an object of this type, and the
/// pointer to it is the handle (SPP-013, CABI-031, CABI-032).
///
/// The words are the library's: a C caller never reads or writes them.
#[repr(C)]
pub struct aldis_spp_segmenter_t {
    /// The slot of [`handle`]: the tag, then the state.
    pub words: [u64; ALDIS_SPP_SEGMENTER_WORDS],
}

impl Storage for aldis_spp_segmenter_t {
    type State = SegmenterState;
    const MAGIC: u32 = 0x414C_5347; // "ALSG"
}

/// An SDU reassembler in caller storage: the caller declares an object of this type, and the
/// pointer to it is the handle (SPP-013, CABI-031, CABI-032).
///
/// The words are the library's: a C caller never reads or writes them.
#[repr(C)]
pub struct aldis_spp_reassembler_t {
    /// The slot of [`handle`]: the tag, then the state.
    pub words: [u64; ALDIS_SPP_REASSEMBLER_WORDS],
}

impl Storage for aldis_spp_reassembler_t {
    type State = ReassemblerState;
    const MAGIC: u32 = 0x414C_5352; // "ALSR"
}

/// Configuration of a tracker (SPP-011, CABI-033).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_spp_tracker_config_t {
    /// `sizeof(aldis_spp_tracker_config_t)`, set by the caller (CABI-033).
    pub struct_size: u32,
    /// APIDs to track, 1 to `ALDIS_SPP_TRACKER_MAX_APIDS`.
    pub capacity: u32,
}

/// What a tracked count says about continuity (CMN-031, SPP-011).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_spp_tracked_t {
    /// `ALDIS_SPP_TRACK_*`.
    pub continuity: u32,
    /// For `ALDIS_SPP_TRACK_GAP`: counts skipped.
    pub missing: u64,
}

/// Configuration of a segmenter (133.0 §4.1.3.4.2; SPP-013, CABI-033).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_spp_segmenter_config_t {
    /// `sizeof(aldis_spp_segmenter_config_t)`, set by the caller (CABI-033).
    pub struct_size: u32,
    /// `ALDIS_SPP_TELEMETRY` or `ALDIS_SPP_TELECOMMAND`.
    pub packet_type: u8,
    /// APID of every segment; not 2047.
    pub apid: u16,
    /// Largest User Data Field of one segment, at least 1.
    pub max_user_len: usize,
}

/// The outcome of one reassembler push (133.0 §4.1.3.4.2; SPP-013).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_spp_reassembly_t {
    /// Why a partial SDU was thrown away: `ALDIS_SPP_DISCARD_*`, `_NONE` if none was.
    pub discarded: u32,
    /// `ALDIS_SPP_REASSEMBLY_*`.
    pub status: u32,
    /// For `ALDIS_SPP_REASSEMBLY_DROPPED`: `ALDIS_SPP_DISCARD_ORPHAN` or `_OVERFLOW`.
    pub reason: u32,
    /// For `ALDIS_SPP_REASSEMBLY_COMPLETE`: the SDU length, at the start of the buffer.
    pub sdu_len: usize,
}

const fn continuity(t: Tracked) -> aldis_spp_tracked_t {
    let (continuity, missing) = match t {
        Tracked::First => (ALDIS_SPP_TRACK_FIRST, 0),
        Tracked::Next(Continuity::InSequence) => (ALDIS_SPP_TRACK_IN_SEQUENCE, 0),
        Tracked::Next(Continuity::Gap { missing }) => (ALDIS_SPP_TRACK_GAP, missing),
        Tracked::Next(Continuity::Repeat) => (ALDIS_SPP_TRACK_REPEAT, 0),
        Tracked::Next(Continuity::Backwards) => (ALDIS_SPP_TRACK_BACKWARDS, 0),
    };
    aldis_spp_tracked_t {
        continuity,
        missing,
    }
}

const fn discard(d: Discard) -> u32 {
    match d {
        Discard::Gap => ALDIS_SPP_DISCARD_GAP,
        Discard::Restarted => ALDIS_SPP_DISCARD_RESTARTED,
        Discard::Interrupted => ALDIS_SPP_DISCARD_INTERRUPTED,
        Discard::Orphan => ALDIS_SPP_DISCARD_ORPHAN,
        Discard::Overflow => ALDIS_SPP_DISCARD_OVERFLOW,
        _ => ALDIS_SPP_DISCARD_OTHER,
    }
}

impl TrackerState {
    /// [`SequenceTracker::track`] with the caller's capacity: a new APID beyond it fails as
    /// a `SequenceTracker` of that capacity fails (SPP-011, GEN-011).
    fn track(&mut self, apid: Apid, count: SeqCount) -> Result<Tracked> {
        if self.tracker.last(apid).is_none() && self.tracker.len() >= self.limit {
            let capacity = u64::try_from(self.limit).unwrap_or(u64::MAX);
            return Err(Error::new(ErrorKind::CapacityExceeded)
                .with_field(field::APID)
                .with_detail(ErrorDetail::Expected {
                    expected: capacity,
                    found: capacity.saturating_add(1),
                }));
        }
        self.tracker.track(apid, count)
    }
}

/// Places an empty Packet Sequence Count tracker in `storage` (SPP-011; CABI-031, CABI-032).
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` for `NULL` or misaligned storage;
/// `ALDIS_ERR_INVALID_CONFIG` for a capacity of 0 and `ALDIS_ERR_CAPACITY_EXCEEDED` for one
/// above `ALDIS_SPP_TRACKER_MAX_APIDS`; CABI-022 violations.
///
/// # Safety
/// `storage` is valid for writes of one storage struct and outlives its use; `cfg` is
/// valid for a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_tracker_init(
    storage: *mut aldis_spp_tracker_t,
    cfg: *const aldis_spp_tracker_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let cfg = abi::config(cfg)?;
            let limit = usize::try_from(cfg.capacity).map_err(|_| invalid_argument())?;
            let wide = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
            if limit == 0 {
                return Err(Error::new(ErrorKind::InvalidConfig).with_detail(
                    ErrorDetail::Expected {
                        expected: 1,
                        found: 0,
                    },
                ));
            }
            if limit > ALDIS_SPP_TRACKER_MAX_APIDS {
                return Err(Error::new(ErrorKind::CapacityExceeded).with_detail(
                    ErrorDetail::Expected {
                        expected: wide(ALDIS_SPP_TRACKER_MAX_APIDS),
                        found: wide(limit),
                    },
                ));
            }
            let state = TrackerState {
                limit,
                tracker: SequenceTracker::new(),
            };
            handle::init(storage, state)
        })
    }
}

/// The tracker behind `h`.
///
/// # Safety
/// As for `handle::get`.
#[expect(unsafe_code, reason = "checks a caller handle (CABI-032, GEN-071)")]
unsafe fn tracker(h: *mut aldis_spp_tracker_t) -> Result<handle::Handle<TrackerState>> {
    // SAFETY: forwarded from the caller.
    unsafe { handle::get(h) }
}

/// Classifies the count `count` of `apid` against the last one tracked (133.0 §4.1.3.4.3.3–4;
/// SPP-011, CMN-031).
///
/// # Errors
/// `ALDIS_ERR_CAPACITY_EXCEEDED` (`apid`, with the capacity) for a new APID when the tracker is
/// full; `ALDIS_ERR_OUT_OF_RANGE` (`apid`, `sequence_count`); `ALDIS_ERR_INVALID_HANDLE`;
/// CABI-022 violations. On error the tracker is unchanged.
///
/// # Safety
/// `h` is a handle from `aldis_spp_tracker_init` whose storage is still valid, or detectably
/// invalid storage; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_tracker_track(
    h: *mut aldis_spp_tracker_t,
    apid: u16,
    count: u16,
    out: *mut aldis_spp_tracked_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let mut h = tracker(h)?;
            let apid = Apid::try_from(apid)?;
            let count = SeqCount::try_from(count)?;
            // `track` returns `Err` only before it changes the state, so `?` after an in-place
            // `update` leaves the state as the by-value form did (API-058).
            let tracked = h.update(|state| state.track(apid, count))?;
            out.write(continuity(tracked));
            Ok(())
        })
    }
}

/// Stops tracking `apid`; `forgot` says whether it was tracked (SPP-011).
///
/// # Errors
/// `ALDIS_ERR_OUT_OF_RANGE` (`apid`); `ALDIS_ERR_INVALID_HANDLE`; CABI-022 violations.
///
/// # Safety
/// As for [`aldis_spp_tracker_track`]; `forgot` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_tracker_forget(
    h: *mut aldis_spp_tracker_t,
    apid: u16,
    forgot: *mut bool,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let mut h = tracker(h)?;
            let forgot = abi::out(forgot)?;
            let apid = Apid::try_from(apid)?;
            let was = h.update(|state| state.tracker.forget(apid));
            forgot.write(was);
            Ok(())
        })
    }
}

/// The last count tracked for `apid`: `known` is false when it is not tracked (SPP-011).
///
/// # Errors
/// `ALDIS_ERR_OUT_OF_RANGE` (`apid`); `ALDIS_ERR_INVALID_HANDLE`; CABI-022 violations.
///
/// # Safety
/// As for [`aldis_spp_tracker_track`]; `known` and `count` are valid for one write each.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_tracker_last(
    h: *mut aldis_spp_tracker_t,
    apid: u16,
    known: *mut bool,
    count: *mut u16,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = tracker(h)?;
            let known = abi::out(known)?;
            let count = abi::out(count)?;
            let apid = Apid::try_from(apid)?;
            let last = h.read(|state| state.tracker.last(apid));
            known.write(last.is_some());
            count.write(last.map_or(0, SeqCount::get));
            Ok(())
        })
    }
}

/// The number of APIDs tracked (SPP-011).
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; CABI-022 violations.
///
/// # Safety
/// As for [`aldis_spp_tracker_track`]; `len` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_tracker_len(
    h: *mut aldis_spp_tracker_t,
    len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = tracker(h)?;
            abi::out(len)?.write(h.read(|state| state.tracker.len()));
            Ok(())
        })
    }
}

/// Releases a tracker: later calls with `h` fail with `ALDIS_ERR_INVALID_HANDLE` (CABI-032).
/// An invalid handle is ignored.
///
/// # Safety
/// `h` is `NULL`, a handle whose storage is still valid, or detectably invalid storage.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_tracker_deinit(h: *mut aldis_spp_tracker_t) {
    // SAFETY: as `# Safety` states; `handle` checks the tag before writing.
    unsafe { handle::deinit(h) }
}

/// Places a segmenter for `sdu` in `storage` (133.0 §4.1.3.4.2; SPP-013; CABI-031, CABI-032).
///
/// The segmenter keeps only the SDU's length and its position: every call to
/// [`aldis_spp_segmenter_next`] takes the same SDU again (API-060).
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` for an empty SDU (`sdu`) or APID 2047 (`apid`),
/// `ALDIS_ERR_OUT_OF_RANGE` (`max_user_len`) for 0 or (`apid`) above 2047,
/// `ALDIS_ERR_INVALID_CONFIG` (`max_packet_len`); the storage errors of
/// [`aldis_spp_tracker_init`]; CABI-022 violations.
///
/// # Safety
/// `storage` is valid for writes of one storage struct and outlives its use; `spp` and
/// `cfg` are valid for reads; `sdu` is valid for `sdu_len` octets; `err` is `NULL` or
/// valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_segmenter_init(
    storage: *mut aldis_spp_segmenter_t,
    spp: *const aldis_spp_config_t,
    cfg: *const aldis_spp_segmenter_config_t,
    sdu: *const u8,
    sdu_len: usize,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let spp = config(abi::config(spp)?)?;
            let cfg = abi::config(cfg)?;
            let sdu = abi::bytes(sdu, sdu_len)?;
            let packet_type = packet_type(cfg.packet_type)?;
            let apid = Apid::try_from(cfg.apid)?;
            // `suspend` keeps only the SDU's length and position, so nothing borrows `sdu` when
            // `init` writes the storage, even if the caller put the SDU inside it (API-060).
            let state = Segmenter::new(&spp, packet_type, apid, sdu, cfg.max_user_len)?.suspend();
            handle::init(storage, state)
        })
    }
}

/// The segmenter behind `h`.
///
/// # Safety
/// As for `handle::get`.
#[expect(unsafe_code, reason = "checks a caller handle (CABI-032, GEN-071)")]
unsafe fn segmenter(h: *mut aldis_spp_segmenter_t) -> Result<handle::Handle<SegmenterState>> {
    // SAFETY: forwarded from the caller.
    unsafe { handle::get(h) }
}

/// Encodes the next segment of `sdu` with the count `sequence_count` and the secondary header
/// `secondary` (empty for none) (133.0 §4.1.3.4.2; SPP-013).
///
/// `out_len` is 0 when the SDU is done and no packet was written; otherwise `next_count` is the
/// count of the next packet.
///
/// # Errors
/// `ALDIS_ERR_LENGTH_MISMATCH` (`sdu`) for an SDU of another length than at `_init`,
/// `ALDIS_ERR_OUT_OF_RANGE` (`packet_len`, `sequence_count`), `ALDIS_ERR_BUFFER_TOO_SMALL`;
/// `ALDIS_ERR_INVALID_HANDLE`; CABI-022 violations, including `out` overlapping an input or the
/// storage. On error nothing changes.
///
/// # Safety
/// `h` is as for [`aldis_spp_tracker_track`]; `sdu` and `secondary` are valid for their lengths;
/// `out` is valid for `cap` octets; `out_len` and `next_count` are valid for one write each;
/// `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_segmenter_next(
    h: *mut aldis_spp_segmenter_t,
    sdu: *const u8,
    sdu_len: usize,
    sequence_count: u16,
    secondary: *const u8,
    secondary_len: usize,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    next_count: *mut u16,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = segmenter(h)?;
            let sdu_bytes = abi::bytes(sdu, sdu_len)?;
            let sec = abi::bytes(secondary, secondary_len)?;
            let out = abi::bytes_mut(
                out,
                cap,
                &[(sdu, sdu_len), (secondary, secondary_len), h.range()],
            )?;
            let out_len = abi::out(out_len)?;
            let next_count = abi::out(next_count)?;
            let mut counter = SequenceCounter::new(SeqCount::try_from(sequence_count)?);
            let mut seg = h.load().resume(sdu_bytes)?;
            let written = seg.next_packet(&mut counter, sec, out)?;
            h.store(seg.suspend());
            out_len.write(written.unwrap_or(0));
            next_count.write(counter.peek().get());
            Ok(())
        })
    }
}

/// SDU octets not yet written (133.0 §4.1.3.4.2; SPP-013).
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; CABI-022 violations.
///
/// # Safety
/// As for [`aldis_spp_tracker_track`]; `remaining` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_segmenter_remaining(
    h: *mut aldis_spp_segmenter_t,
    remaining: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = segmenter(h)?;
            abi::out(remaining)?.write(h.read(SegmenterState::remaining));
            Ok(())
        })
    }
}

/// Releases a segmenter (CABI-032); an invalid handle is ignored.
///
/// # Safety
/// As for [`aldis_spp_tracker_deinit`].
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_segmenter_deinit(h: *mut aldis_spp_segmenter_t) {
    // SAFETY: as `# Safety` states; `handle` checks the tag before writing.
    unsafe { handle::deinit(h) }
}

/// Places a reassembler for `apid` in `storage` (133.0 §4.1.3.4.2; SPP-013; CABI-031,
/// CABI-032).
///
/// The reassembler keeps no buffer: every push takes the same reassembly buffer (API-060).
///
/// # Errors
/// `ALDIS_ERR_OUT_OF_RANGE` (`apid`); the storage errors of [`aldis_spp_tracker_init`];
/// CABI-022 violations.
///
/// # Safety
/// `storage` is valid for writes of one storage struct and outlives its use; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_reassembler_init(
    storage: *mut aldis_spp_reassembler_t,
    apid: u16,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let state = ReassemblerState::new(Apid::try_from(apid)?);
            handle::init(storage, state)
        })
    }
}

/// The reassembler behind `h`.
///
/// # Safety
/// As for `handle::get`.
#[expect(unsafe_code, reason = "checks a caller handle (CABI-032, GEN-071)")]
unsafe fn reassembler(h: *mut aldis_spp_reassembler_t) -> Result<handle::Handle<ReassemblerState>> {
    // SAFETY: forwarded from the caller.
    unsafe { handle::get(h) }
}

/// Decodes one Space Packet, splits its data field by `layout`, and pushes its User Data Field
/// into the reassembly buffer `buf` (133.0 §4.1.3.4.2; SPP-013).
///
/// A completed SDU is `buf[0 .. out->sdu_len]` until the next push.
///
/// # Errors
/// The errors of `aldis_spp_split`; `ALDIS_ERR_INVALID_FIELD` (`apid`) for a packet of another
/// APID; `ALDIS_ERR_BUFFER_TOO_SMALL` (with `needed`) for a buffer shorter than the SDU in
/// progress; `ALDIS_ERR_INVALID_HANDLE`; CABI-022 violations, including `buf` overlapping the
/// packet or the storage. On error the reassembler is unchanged.
///
/// # Safety
/// `h` is as for [`aldis_spp_tracker_track`]; `packet` is valid for `packet_len` octets;
/// `layout` is valid for a read; `buf` is valid for `buf_len` octets and holds what earlier
/// pushes wrote; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_reassembler_push(
    h: *mut aldis_spp_reassembler_t,
    packet: *const u8,
    packet_len: usize,
    mode: u32,
    layout: *const aldis_spp_secondary_layout_t,
    buf: *mut u8,
    buf_len: usize,
    out: *mut aldis_spp_reassembly_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = reassembler(h)?;
            let bytes = abi::bytes(packet, packet_len)?;
            let layout = secondary_layout(&abi::config(layout)?)?;
            let buf = abi::bytes_mut(buf, buf_len, &[(packet, packet_len), h.range()])?;
            let p = SpacePacket::decode(bytes, abi::mode(mode)?)?;
            let user_data = p.split(&layout)?.user_data();
            let mut r = h.load().resume(buf)?;
            let outcome = r.push(&p.header(), user_data)?;
            h.store(r.suspend());
            let (status, reason, sdu_len) = match outcome.status {
                Status::InProgress => (ALDIS_SPP_REASSEMBLY_IN_PROGRESS, ALDIS_SPP_DISCARD_NONE, 0),
                Status::Complete(n) => (ALDIS_SPP_REASSEMBLY_COMPLETE, ALDIS_SPP_DISCARD_NONE, n),
                Status::Dropped(why) => (ALDIS_SPP_REASSEMBLY_DROPPED, discard(why), 0),
            };
            out.write(aldis_spp_reassembly_t {
                discarded: outcome.discarded.map_or(ALDIS_SPP_DISCARD_NONE, discard),
                status,
                reason,
                sdu_len,
            });
            Ok(())
        })
    }
}

/// Discards any partial SDU and forgets the last count (133.0 §4.1.3.4.2; SPP-013).
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; CABI-022 violations.
///
/// # Safety
/// As for [`aldis_spp_tracker_track`].
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_reassembler_reset(
    h: *mut aldis_spp_reassembler_t,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let mut h = reassembler(h)?;
            // The state is small, so the temporary is cheap; a large state needs an in-place
            // re-initialization method instead of `*r = …` (CABI-040).
            h.update(|r| *r = ReassemblerState::new(r.apid()));
            Ok(())
        })
    }
}

/// Whether a partial SDU is in progress (133.0 §4.1.3.4.2; SPP-013).
///
/// # Errors
/// `ALDIS_ERR_INVALID_HANDLE`; CABI-022 violations.
///
/// # Safety
/// As for [`aldis_spp_tracker_track`]; `assembling` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_reassembler_is_assembling(
    h: *mut aldis_spp_reassembler_t,
    assembling: *mut bool,
    err: *mut crate::aldis_error_detail_t,
) -> u32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` and `handle` check them.
    unsafe {
        abi::call(err, || {
            let h = reassembler(h)?;
            abi::out(assembling)?.write(h.read(ReassemblerState::is_assembling));
            Ok(())
        })
    }
}

/// Releases a reassembler (CABI-032); an invalid handle is ignored.
///
/// # Safety
/// As for [`aldis_spp_tracker_deinit`].
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_reassembler_deinit(h: *mut aldis_spp_reassembler_t) {
    // SAFETY: as `# Safety` states; `handle` checks the tag before writing.
    unsafe { handle::deinit(h) }
}
