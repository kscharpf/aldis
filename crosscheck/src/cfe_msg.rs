//! Safe calls into NASA cFE's `MSG` module, the only `unsafe` code of the cross-checks.
//!
//! `build.rs` compiles the vendored cFE sources (`crosscheck/vendor/cfe/`, cross-checks.md §1)
//! with the stubs of `crosscheck/cfe-stubs/` into a static library. The `extern "C"` block below
//! repeats the prototypes of `modules/core_api/fsw/inc/cfe_msg.h`; `cfe-stubs/abi_check.c` fails
//! the build if a type size or a constant used here differs from cFE's headers.
//!
//! cFE trusts its callers: the `CFE_MSG_*` functions take a pointer to a message and read or write
//! a fixed number of octets, the primary header and, for the time, the 6 octets after it, without
//! knowing how long the buffer is. Each function here therefore works on a copy of at least
//! [`TLM_HEADER_LEN`] octets, so that no call touches memory outside its buffer whatever the input.

use core::ffi::c_int;

/// `sizeof(CFE_MSG_TelemetryHeader_t)` without the CCSDS extended header.
///
/// The primary header, the 6-octet time, and 4 spare octets
/// (`option_inc/default_cfe_msg_hdr_pri.h`): the largest structure any function used here reads
/// or writes.
pub const TLM_HEADER_LEN: usize = 16;

/// The primary header length, `sizeof(CFE_MSG_Message_t)` without the extended header.
const PRIMARY_HEADER_LEN: usize = 6;

/// `CFE_SUCCESS`.
const SUCCESS: i32 = 0;
/// `CFE_MSG_BAD_ARGUMENT` (`CFE_SB_BAD_ARGUMENT`, `0xca000003`).
#[expect(
    clippy::cast_possible_wrap,
    reason = "cFE's status codes are 32-bit patterns with the sign bit set"
)]
const BAD_ARGUMENT: i32 = 0xca00_0003_u32 as i32;
/// `CFE_MSG_WRONG_MSG_TYPE` (`CFE_SB_WRONG_MSG_TYPE`, `0xca00000d`).
#[expect(
    clippy::cast_possible_wrap,
    reason = "cFE's status codes are 32-bit patterns with the sign bit set"
)]
const WRONG_MSG_TYPE: i32 = 0xca00_000d_u32 as i32;

/// `CFE_MSG_Type_Cmd` and `CFE_MSG_Type_Tlm` of `CFE_MSG_Type_t`.
const TYPE_CMD: c_int = 1;
const TYPE_TLM: c_int = 2;

/// `CFE_MSG_SegmentationFlag_t`: `Continue`, `First`, `Last`, `Unsegmented`.
const SEG_FLAGS: [c_int; 4] = [1, 2, 3, 4];

/// An opaque `CFE_MSG_Message_t`; only pointers to it cross the boundary.
#[repr(C)]
struct Message {
    _opaque: [u8; 0],
}

/// `CFE_SB_MsgId_t`: a struct holding a `uint32` (`default_cfe_sb_extern_typedefs.h`).
#[repr(C)]
#[derive(Clone, Copy)]
struct MsgId {
    value: u32,
}

/// `CFE_SB_INVALID_MSG_ID`: `CFE_MSG_Init` then leaves the identifier bits to the setters.
const INVALID_MSG_ID: MsgId = MsgId { value: 0 };

/// `CFE_TIME_SysTime_t`: seconds, and subseconds in units of 2^-32 s.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SysTime {
    seconds: u32,
    subseconds: u32,
}

// The prototypes of cfe_msg.h for the functions the harness calls. `CFE_Status_t` is `int32`,
// `CFE_MSG_Size_t` is `size_t`, the header field types are `uint16`, and the two enumerations
// are `int`-sized (checked by cfe-stubs/abi_check.c).
unsafe extern "C" {
    fn CFE_MSG_Init(msg: *mut Message, msg_id: MsgId, size: usize) -> i32;
    fn CFE_MSG_GetSize(msg: *const Message, size: *mut usize) -> i32;
    fn CFE_MSG_GetType(msg: *const Message, msg_type: *mut c_int) -> i32;
    fn CFE_MSG_SetType(msg: *mut Message, msg_type: c_int) -> i32;
    fn CFE_MSG_GetHeaderVersion(msg: *const Message, version: *mut u16) -> i32;
    fn CFE_MSG_SetHeaderVersion(msg: *mut Message, version: u16) -> i32;
    fn CFE_MSG_GetHasSecondaryHeader(msg: *const Message, has: *mut bool) -> i32;
    fn CFE_MSG_SetHasSecondaryHeader(msg: *mut Message, has: bool) -> i32;
    fn CFE_MSG_GetApId(msg: *const Message, apid: *mut u16) -> i32;
    fn CFE_MSG_SetApId(msg: *mut Message, apid: u16) -> i32;
    fn CFE_MSG_GetSegmentationFlag(msg: *const Message, flag: *mut c_int) -> i32;
    fn CFE_MSG_SetSegmentationFlag(msg: *mut Message, flag: c_int) -> i32;
    fn CFE_MSG_GetSequenceCount(msg: *const Message, count: *mut u16) -> i32;
    fn CFE_MSG_SetSequenceCount(msg: *mut Message, count: u16) -> i32;
    fn CFE_MSG_GetNextSequenceCount(count: u16) -> u16;
    fn CFE_MSG_GetMsgTime(msg: *const Message, time: *mut SysTime) -> i32;
    fn CFE_MSG_SetMsgTime(msg: *mut Message, time: SysTime) -> i32;
}

/// A status other than `CFE_SUCCESS` returned by a `CFE_MSG_*` function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfeError {
    /// `CFE_MSG_BAD_ARGUMENT`: a value out of the field's range, or a size outside 7–65 542.
    BadArgument,
    /// `CFE_MSG_WRONG_MSG_TYPE`: the time of a packet that is not telemetry with a secondary
    /// header and Packet Version Number `000`.
    WrongMsgType,
    /// Any other status.
    Other(i32),
}

impl core::fmt::Display for CfeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CfeError::BadArgument => f.write_str("CFE_MSG_BAD_ARGUMENT"),
            CfeError::WrongMsgType => f.write_str("CFE_MSG_WRONG_MSG_TYPE"),
            CfeError::Other(status) => write!(f, "cFE status {status:#010x}"),
        }
    }
}

/// `Ok` for `CFE_SUCCESS`, else the error.
const fn status(code: i32) -> Result<(), CfeError> {
    match code {
        SUCCESS => Ok(()),
        BAD_ARGUMENT => Err(CfeError::BadArgument),
        WRONG_MSG_TYPE => Err(CfeError::WrongMsgType),
        other => Err(CfeError::Other(other)),
    }
}

/// The primary header fields as cFE's getters return them, in 133.0's encoding: the Packet Type
/// as its bit, the Sequence Flags as their two bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// `CFE_MSG_GetHeaderVersion`: the Packet Version Number.
    pub version: u16,
    /// `CFE_MSG_GetType`: 0 for telemetry, 1 for a command.
    pub packet_type: u8,
    /// `CFE_MSG_GetHasSecondaryHeader`.
    pub secondary_header: bool,
    /// `CFE_MSG_GetApId`.
    pub apid: u16,
    /// `CFE_MSG_GetSegmentationFlag`, as the two-bit field.
    pub sequence_flags: u8,
    /// `CFE_MSG_GetSequenceCount`.
    pub sequence_count: u16,
    /// `CFE_MSG_GetSize`: the total packet length, Packet Data Length + 7.
    pub size: usize,
}

/// `octets`, zero-filled to at least [`TLM_HEADER_LEN`] octets.
fn padded(octets: &[u8]) -> Vec<u8> {
    let mut buf = octets.to_vec();
    if buf.len() < TLM_HEADER_LEN {
        buf.resize(TLM_HEADER_LEN, 0);
    }
    buf
}

/// Reads the primary header of `packet` with cFE's getters. Octets missing from a packet shorter
/// than the header read as zero: cFE reads the header without a length.
///
/// # Errors
/// A getter's status other than `CFE_SUCCESS`; with valid pointers cFE returns none.
pub fn read_header(packet: &[u8]) -> Result<Header, CfeError> {
    let buf = padded(packet);
    let msg = buf.as_ptr().cast::<Message>();
    let mut version = 0_u16;
    let mut msg_type: c_int = 0;
    let mut secondary_header = false;
    let mut apid = 0_u16;
    let mut flag: c_int = 0;
    let mut sequence_count = 0_u16;
    let mut size = 0_usize;
    // SAFETY: `msg` points to `buf`, at least TLM_HEADER_LEN octets, of which each getter reads
    // the 6 primary header octets; `CFE_MSG_Message_t` holds only `uint8` arrays, so any address
    // is aligned. Each output pointer is a local of the type the prototype names, and nothing is
    // retained after the call.
    unsafe {
        status(CFE_MSG_GetHeaderVersion(msg, &raw mut version))?;
        status(CFE_MSG_GetType(msg, &raw mut msg_type))?;
        status(CFE_MSG_GetHasSecondaryHeader(
            msg,
            &raw mut secondary_header,
        ))?;
        status(CFE_MSG_GetApId(msg, &raw mut apid))?;
        status(CFE_MSG_GetSegmentationFlag(msg, &raw mut flag))?;
        status(CFE_MSG_GetSequenceCount(msg, &raw mut sequence_count))?;
        status(CFE_MSG_GetSize(msg, &raw mut size))?;
    }
    let packet_type = match msg_type {
        TYPE_TLM => 0,
        TYPE_CMD => 1,
        other => return Err(CfeError::Other(other)),
    };
    let sequence_flags = SEG_FLAGS
        .iter()
        .position(|f| *f == flag)
        .and_then(|bits| u8::try_from(bits).ok())
        .ok_or(CfeError::Other(flag))?;
    Ok(Header {
        version,
        packet_type,
        secondary_header,
        apid,
        sequence_flags,
        sequence_count,
        size,
    })
}

/// The primary header fields a sender sets, in 133.0's encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fields {
    /// The Packet Version Number.
    pub version: u16,
    /// 0 for telemetry, 1 for a command; any other value is passed to cFE as
    /// `CFE_MSG_Type_Invalid`.
    pub packet_type: u8,
    /// The Secondary Header Flag.
    pub secondary_header: bool,
    /// The APID; cFE rejects a value above 11 bits.
    pub apid: u16,
    /// The two-bit Sequence Flags; cFE is given `CFE_MSG_SegFlag_Invalid` above 3.
    pub sequence_flags: u8,
    /// The Packet Sequence Count; cFE rejects a value above 14 bits.
    pub sequence_count: u16,
}

/// Builds a packet with `CFE_MSG_Init` for `6 + data_field.len()` octets and the setters, then
/// copies `data_field` after the primary header, as a cFE application fills its payload.
///
/// # Errors
/// The first status other than `CFE_SUCCESS`: `CFE_MSG_Init`'s size check (7–65 542 octets,
/// `CFE_MSG_SetSize`), or a setter's range check.
pub fn build(fields: Fields, data_field: &[u8]) -> Result<Vec<u8>, CfeError> {
    let size = PRIMARY_HEADER_LEN.saturating_add(data_field.len());
    let mut buf = vec![0_u8; size.max(TLM_HEADER_LEN)];
    let msg = buf.as_mut_ptr().cast::<Message>();
    let msg_type = match fields.packet_type {
        0 => TYPE_TLM,
        1 => TYPE_CMD,
        _ => 0,
    };
    let flag = SEG_FLAGS
        .get(usize::from(fields.sequence_flags))
        .copied()
        .unwrap_or(0);
    // SAFETY: `msg` points to `buf`, at least max(size, TLM_HEADER_LEN) octets: `CFE_MSG_Init`
    // clears `size` octets and writes the 6 header octets, and each setter writes within those
    // 6. `buf` outlives the calls and is not otherwise accessed during them; the message holds
    // only `uint8` arrays, so any address is aligned. Arguments are passed by value.
    unsafe {
        status(CFE_MSG_Init(msg, INVALID_MSG_ID, size))?;
        status(CFE_MSG_SetHeaderVersion(msg, fields.version))?;
        status(CFE_MSG_SetType(msg, msg_type))?;
        status(CFE_MSG_SetHasSecondaryHeader(msg, fields.secondary_header))?;
        status(CFE_MSG_SetApId(msg, fields.apid))?;
        status(CFE_MSG_SetSegmentationFlag(msg, flag))?;
        status(CFE_MSG_SetSequenceCount(msg, fields.sequence_count))?;
    }
    buf.truncate(size);
    if let Some(payload) = buf.get_mut(PRIMARY_HEADER_LEN..) {
        payload.copy_from_slice(data_field);
    }
    Ok(buf)
}

/// The telemetry time of `packet` read by `CFE_MSG_GetMsgTime`.
///
/// Seconds, and subseconds in units of 2^-32 s, of which cFE fills only the upper 16 bits.
/// Octets missing from a packet shorter than [`TLM_HEADER_LEN`] read as zero: cFE does not check
/// the Packet Data Length.
///
/// # Errors
/// `WrongMsgType` unless the packet is telemetry with the Secondary Header Flag set and Packet
/// Version Number `000`.
pub fn read_time(packet: &[u8]) -> Result<(u32, u32), CfeError> {
    let buf = padded(packet);
    let mut time = SysTime::default();
    // SAFETY: `buf` holds at least TLM_HEADER_LEN octets, the `CFE_MSG_TelemetryHeader_t` that
    // `CFE_MSG_GetMsgTime` reads (the header and the 6 time octets); its fields are `uint8`
    // arrays, so any address is aligned. `time` is a local `CFE_TIME_SysTime_t`.
    unsafe {
        status(CFE_MSG_GetMsgTime(
            buf.as_ptr().cast::<Message>(),
            &raw mut time,
        ))?;
    }
    Ok((time.seconds, time.subseconds))
}

/// The count after `count`, from `CFE_MSG_GetNextSequenceCount`: one more, and 0 after the
/// 14-bit maximum.
#[must_use]
pub fn next_sequence_count(count: u16) -> u16 {
    // SAFETY: `CFE_MSG_GetNextSequenceCount` takes and returns a `uint16` by value and touches
    // no memory.
    unsafe { CFE_MSG_GetNextSequenceCount(count) }
}

/// `packet` with its telemetry time set by `CFE_MSG_SetMsgTime`; the result has the length of
/// `packet`, so a time written beyond a short packet is dropped.
///
/// # Errors
/// `WrongMsgType` as for [`read_time`].
pub fn write_time(packet: &[u8], seconds: u32, subseconds: u32) -> Result<Vec<u8>, CfeError> {
    let mut buf = padded(packet);
    let time = SysTime {
        seconds,
        subseconds,
    };
    // SAFETY: `buf` holds at least TLM_HEADER_LEN octets, the `CFE_MSG_TelemetryHeader_t` that
    // `CFE_MSG_SetMsgTime` reads (the header) and writes (the 6 time octets); its fields are
    // `uint8` arrays, so any address is aligned. `time` is passed by value.
    unsafe {
        status(CFE_MSG_SetMsgTime(buf.as_mut_ptr().cast::<Message>(), time))?;
    }
    buf.truncate(packet.len());
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::{
        CfeError, Fields, Header, build, next_sequence_count, read_header, read_time, write_time,
    };

    /// Telemetry, Secondary Header Flag set, APID 0x123, first segment, count 0x2345, a 7-octet
    /// data field (Packet Data Length 6).
    const PACKET: [u8; 13] = [
        0x09, 0x23, 0x63, 0x45, 0x00, 0x06, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xEE,
    ];

    const FIELDS: Fields = Fields {
        version: 0,
        packet_type: 0,
        secondary_header: true,
        apid: 0x123,
        sequence_flags: 0b01,
        sequence_count: 0x2345,
    };

    #[test]
    fn reads_and_builds_the_primary_header() {
        // req: VER-040
        assert_eq!(
            read_header(&PACKET),
            Ok(Header {
                version: 0,
                packet_type: 0,
                secondary_header: true,
                apid: 0x123,
                sequence_flags: 0b01,
                sequence_count: 0x2345,
                size: 13,
            })
        );
        assert_eq!(build(FIELDS, &PACKET[6..]).as_deref(), Ok(&PACKET[..]));
    }

    #[test]
    fn the_next_count_wraps_after_14_bits() {
        // req: VER-040
        assert_eq!(next_sequence_count(5), 6);
        assert_eq!(next_sequence_count(0x3FFF), 0);
    }

    #[test]
    fn build_reports_cfe_range_checks() {
        // req: VER-040
        assert_eq!(build(FIELDS, &[]), Err(CfeError::BadArgument));
        assert_eq!(build(FIELDS, &vec![0; 65_537]), Err(CfeError::BadArgument));
        assert!(build(FIELDS, &vec![0; 65_536]).is_ok());
        let wide = Fields {
            apid: 0x800,
            ..FIELDS
        };
        assert_eq!(build(wide, &[0]), Err(CfeError::BadArgument));
    }

    #[test]
    fn reads_and_writes_the_telemetry_time() {
        // req: VER-040
        assert_eq!(read_time(&PACKET), Ok((0x1234_5678, 0x9ABC_0000)));
        let mut cleared = PACKET;
        cleared[6..12].fill(0);
        assert_eq!(
            write_time(&cleared, 0x1234_5678, 0x9ABC_FFFF).as_deref(),
            Ok(&PACKET[..])
        );
        let mut command = PACKET;
        command[0] |= 0x10;
        assert_eq!(read_time(&command), Err(CfeError::WrongMsgType));
    }
}
