//! C ABI of the Space Packet codec `aldis-spp` (CCSDS 133.0-B-2; M2 plan D4; CABI-010…012,
//! CABI-020…022, CABI-033).
//!
//! Stateless calls: [`aldis_spp_decode`], [`aldis_spp_peek_length`], [`aldis_spp_encode`], the
//! two-phase writer as [`aldis_spp_writer_begin`] and [`aldis_spp_writer_finish`],
//! [`aldis_spp_idle`], [`aldis_spp_octet_string_encode`], [`aldis_spp_sequence_count_next`],
//! [`aldis_spp_config_check`], and [`aldis_spp_split`]. The stateful helpers are in
//! [`crate::spp_state`].

use core::num::NonZeroUsize;

use aldis::core::Result;
use aldis::core::ids::{Apid, SeqCount};
use aldis::spp::{
    IdlePacket, OctetStringEncoder, PRIMARY_HEADER_LEN, PacketFields, PacketType,
    SecondaryHeaderLayout, SequenceCounter, SequenceFlags, SpacePacket, SpacePacketWriter,
    SppConfig, TimeHeaderLayout, encode,
};

use crate::abi::{self, Out, invalid_argument};
use crate::time::{aldis_time_decoded_t, aldis_time_format_t, decoded, time_format};

/// [`aldis_spp_fields_t::packet_type`]: telemetry, Packet Type 0 (133.0 §4.1.3.3.2).
pub const ALDIS_SPP_TELEMETRY: u8 = 0;
/// [`aldis_spp_fields_t::packet_type`]: telecommand, Packet Type 1 (133.0 §4.1.3.3.2).
pub const ALDIS_SPP_TELECOMMAND: u8 = 1;
/// Sequence Flags `00`: continuation segment (133.0 §4.1.3.4.2.2).
pub const ALDIS_SPP_SEQ_CONTINUATION: u8 = 0b00;
/// Sequence Flags `01`: first segment (133.0 §4.1.3.4.2.2).
pub const ALDIS_SPP_SEQ_FIRST: u8 = 0b01;
/// Sequence Flags `10`: last segment (133.0 §4.1.3.4.2.2).
pub const ALDIS_SPP_SEQ_LAST: u8 = 0b10;
/// Sequence Flags `11`: unsegmented user data (133.0 §4.1.3.4.2.2).
pub const ALDIS_SPP_SEQ_UNSEGMENTED: u8 = 0b11;
/// APID of Idle Packets (133.0 §4.1.3.3.4.4).
pub const ALDIS_SPP_APID_IDLE: u16 = 2047;
/// Primary Header length in octets (133.0 §4.1.3.1).
pub const ALDIS_SPP_PRIMARY_HEADER_LEN: u32 = 6;
/// Smallest Space Packet: header and one data octet (133.0 §4.1.3.5.3).
pub const ALDIS_SPP_MIN_PACKET_LEN: u32 = 7;
/// Largest Space Packet: header and 65 536 data octets (133.0 §4.1.3.5.3); the default and
/// largest Maximum Packet Length.
pub const ALDIS_SPP_MAX_PACKET_LEN: u32 = 65_542;
/// Anomaly bit: Idle Packet with Secondary Header Flag 1 (bit 0 of `SppAnomaly`;
/// 133.0 §4.1.3.3.3.4; API-032, GEN-043).
pub const ALDIS_SPP_ANOMALY_IDLE_WITH_SECONDARY_HEADER: u32 = 1 << 0;

/// [`aldis_spp_secondary_layout_t::kind`]: no Packet Secondary Header (133.0 §4.1.4.2).
pub const ALDIS_SPP_SECONDARY_ABSENT: u32 = 0;
/// [`aldis_spp_secondary_layout_t::kind`]: an opaque secondary header of `opaque_len` octets
/// (133.0 §4.1.4.2.1; SPP-007).
pub const ALDIS_SPP_SECONDARY_OPAQUE: u32 = 1;
/// [`aldis_spp_secondary_layout_t::kind`]: a Time Code Field, then `ancillary_len` octets of
/// Ancillary Data (133.0 §4.1.4.2.2; SPP-008).
pub const ALDIS_SPP_SECONDARY_TIME: u32 = 2;

/// Managed parameters of the Space Packet Protocol (133.0 §5.2 Table 5-1; SPP-005, GEN-060,
/// CABI-033).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_spp_config_t {
    /// `sizeof(aldis_spp_config_t)`, set by the caller (CABI-033).
    pub struct_size: u32,
    /// Maximum Packet Length, 7 to `ALDIS_SPP_MAX_PACKET_LEN` octets.
    pub max_packet_len: u32,
}

/// The sender-chosen Primary Header fields of a packet (133.0 §4.1.3; SPP-001…003).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_spp_fields_t {
    /// `ALDIS_SPP_TELEMETRY` or `ALDIS_SPP_TELECOMMAND`.
    pub packet_type: u8,
    /// Sequence Flags, `ALDIS_SPP_SEQ_*`.
    pub sequence_flags: u8,
    /// APID, 0 to 2047.
    pub apid: u16,
    /// Packet Sequence Count or Packet Name, 0 to 16383.
    pub sequence_count: u16,
}

/// A decoded Space Packet: its Primary Header fields and where its data field lies in the
/// input (133.0 §4.1; SPP-001…004, API-010, API-014, CABI-012).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_spp_packet_t {
    /// The Primary Header fields.
    pub fields: aldis_spp_fields_t,
    /// Secondary Header Flag.
    pub secondary_header_flag: bool,
    /// APID 2047: an Idle Packet.
    pub idle: bool,
    /// Lenient-mode anomaly flags, `ALDIS_SPP_ANOMALY_*`; 0 in strict mode (API-032).
    pub anomalies: u32,
    /// Offset of the Packet Data Field in the input: always 6.
    pub data_offset: usize,
    /// Length of the Packet Data Field.
    pub data_len: usize,
    /// Octets consumed from the input: the total packet length (API-011).
    pub packet_len: usize,
}

/// Where a two-phase writer's caller fills in the packet (API-023, CABI-012).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_spp_writer_layout_t {
    /// Offset of the secondary header in the output buffer: always 6.
    pub secondary_header_offset: usize,
    /// Length of the secondary header.
    pub secondary_header_len: usize,
    /// Offset of the User Data Field in the output buffer.
    pub user_data_offset: usize,
    /// Capacity of the User Data Field: the `max_user_len` given.
    pub user_data_cap: usize,
}

/// How the Packet Data Field of an APID begins (133.0 §4.1.4.2; SPP-007, SPP-008, CABI-033).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_spp_secondary_layout_t {
    /// `sizeof(aldis_spp_secondary_layout_t)`, set by the caller (CABI-033).
    pub struct_size: u32,
    /// `ALDIS_SPP_SECONDARY_ABSENT`, `_OPAQUE`, or `_TIME`.
    pub kind: u32,
    /// `_OPAQUE`: the secondary header length, at least 1.
    pub opaque_len: usize,
    /// `_TIME`: the format of the Time Code Field, a CUC or CDS code.
    pub time_format: aldis_time_format_t,
    /// `_TIME`: 1 if the Time Code Field starts with its P-field, 0 if it is implicit.
    pub time_pfield: u8,
    /// `_TIME`: length of the Ancillary Data Field after the time code.
    pub ancillary_len: usize,
}

/// A Packet Data Field split into its secondary header and User Data Field (133.0 §4.1.4;
/// SPP-007, SPP-008, CABI-012). Offsets are into the packet given.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_spp_split_t {
    /// Offset of the Packet Secondary Header: always 6.
    pub secondary_header_offset: usize,
    /// Length of the Packet Secondary Header; 0 when absent.
    pub secondary_header_len: usize,
    /// Offset of the User Data Field.
    pub user_data_offset: usize,
    /// Length of the User Data Field.
    pub user_data_len: usize,
    /// Offset of the Ancillary Data Field: the secondary header after the time code.
    pub ancillary_offset: usize,
    /// Length of the Ancillary Data Field; without a time code, the whole secondary header.
    pub ancillary_len: usize,
    /// Whether `time` holds the decoded time code of a time secondary header.
    pub has_time: bool,
    /// The decoded Time Code Field, when `has_time`; it starts at `secondary_header_offset`.
    pub time: aldis_time_decoded_t,
}

/// The configuration of a C struct (133.0 §5.2; GEN-060).
pub(crate) fn config(c: aldis_spp_config_t) -> Result<SppConfig> {
    SppConfig::new(usize::try_from(c.max_packet_len).map_err(|_| invalid_argument())?)
}

/// A packet type argument (133.0 §4.1.3.3.2; CABI-022).
pub(crate) const fn packet_type(value: u8) -> Result<PacketType> {
    match value {
        ALDIS_SPP_TELEMETRY => Ok(PacketType::Telemetry),
        ALDIS_SPP_TELECOMMAND => Ok(PacketType::Telecommand),
        _ => Err(invalid_argument()),
    }
}

const fn packet_type_code(t: PacketType) -> u8 {
    match t {
        PacketType::Telemetry => ALDIS_SPP_TELEMETRY,
        PacketType::Telecommand => ALDIS_SPP_TELECOMMAND,
    }
}

/// The Primary Header fields of a C struct, each parsed into its type (RS-020).
pub(crate) fn fields(f: aldis_spp_fields_t) -> Result<PacketFields> {
    let packet_type = packet_type(f.packet_type)?;
    let apid = Apid::try_from(f.apid)?;
    if f.sequence_flags > 0b11 {
        return Err(invalid_argument());
    }
    let sequence_count = SeqCount::try_from(f.sequence_count)?;
    Ok(PacketFields {
        packet_type,
        apid,
        sequence_flags: SequenceFlags::from_bits(f.sequence_flags),
        sequence_count,
    })
}

/// The C view of a decoded packet.
pub(crate) fn view(p: &SpacePacket<'_>) -> aldis_spp_packet_t {
    let header = p.header();
    aldis_spp_packet_t {
        fields: aldis_spp_fields_t {
            packet_type: packet_type_code(p.packet_type()),
            sequence_flags: p.sequence_flags().bits(),
            apid: p.apid().get(),
            sequence_count: p.sequence_count().get(),
        },
        secondary_header_flag: p.has_secondary_header(),
        idle: p.is_idle(),
        anomalies: p.anomalies().bits(),
        data_offset: PRIMARY_HEADER_LEN,
        data_len: header.data_len(),
        packet_len: p.len(),
    }
}

/// The secondary header layout of a C struct (SPP-007, SPP-008).
pub(crate) fn layout(l: &aldis_spp_secondary_layout_t) -> Result<SecondaryHeaderLayout> {
    match l.kind {
        ALDIS_SPP_SECONDARY_ABSENT => Ok(SecondaryHeaderLayout::Absent),
        ALDIS_SPP_SECONDARY_OPAQUE => NonZeroUsize::new(l.opaque_len)
            .map(SecondaryHeaderLayout::Opaque)
            .ok_or_else(invalid_argument),
        ALDIS_SPP_SECONDARY_TIME => {
            let format = time_format(&l.time_format)?;
            let pfield = abi::flag(l.time_pfield)?;
            Ok(SecondaryHeaderLayout::Time(TimeHeaderLayout::new(
                format,
                pfield,
                l.ancillary_len,
            )?))
        }
        _ => Err(invalid_argument()),
    }
}

fn decode(bytes: &[u8], mode: u32, out: Out<'_, aldis_spp_packet_t>) -> Result<()> {
    let p = SpacePacket::decode(bytes, abi::mode(mode)?)?;
    out.write(view(&p));
    Ok(())
}

fn split(
    bytes: &[u8],
    mode: u32,
    layout: &aldis_spp_secondary_layout_t,
    out: Out<'_, aldis_spp_split_t>,
) -> Result<()> {
    let layout = self::layout(layout)?;
    let p = SpacePacket::decode(bytes, abi::mode(mode)?)?;
    let s = p.split(&layout)?;
    let time = s.time_code();
    let secondary_len = s.secondary_header().len();
    let end = PRIMARY_HEADER_LEN.saturating_add(secondary_len);
    let ancillary_len = s.ancillary_data().len();
    out.write(aldis_spp_split_t {
        secondary_header_offset: PRIMARY_HEADER_LEN,
        secondary_header_len: secondary_len,
        user_data_offset: end,
        user_data_len: s.user_data().len(),
        ancillary_offset: end.saturating_sub(ancillary_len),
        ancillary_len,
        has_time: time.is_some(),
        time: time.map(|t| decoded(&t)).unwrap_or_default(),
    });
    Ok(())
}

/// Two-phase writer arguments shared by `begin` and `finish`.
struct WriterArgs {
    config: SppConfig,
    fields: PacketFields,
    secondary_len: usize,
    max_user_len: usize,
}

fn writer<'b>(out: &'b mut [u8], a: &WriterArgs) -> Result<SpacePacketWriter<'b>> {
    SpacePacketWriter::begin(out, &a.config, a.fields, a.secondary_len, a.max_user_len)
}

/// Decodes one Space Packet (133.0 §4.1; SPP-001…004, SPP-009, API-010, API-011, API-030).
///
/// `bytes` may hold more than one packet: `out->packet_len` is the length of the first. The
/// data field is `bytes[data_offset .. data_offset + data_len]`.
///
/// # Errors
/// `ALDIS_ERR_INCOMPLETE` (with `needed`) when `bytes` is shorter than the header or the
/// packet, `ALDIS_ERR_UNSUPPORTED_VERSION` (`packet_version`), and in strict mode
/// `ALDIS_ERR_INVALID_FIELD` (`secondary_header_flag`) for an Idle Packet with a secondary
/// header; CABI-022 violations.
///
/// # Safety
/// `bytes` is valid for `len` octets (or `NULL` with `len` 0); `out` is valid for one write;
/// `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_decode(
    bytes: *const u8,
    len: usize,
    mode: u32,
    out: *mut aldis_spp_packet_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            decode(abi::bytes(bytes, len)?, mode, abi::out(out)?)
        })
    }
}

/// The total length of the Space Packet that `prefix` begins, from its Primary Header alone
/// (133.0 §4.1.3.5; API-013, M2 plan D14).
///
/// # Errors
/// `ALDIS_ERR_INCOMPLETE` with `needed` = 6 for a prefix shorter than the Primary Header,
/// `ALDIS_ERR_UNSUPPORTED_VERSION` (`packet_version`); CABI-022 violations.
///
/// # Safety
/// `prefix` is valid for `prefix_len` octets (or `NULL` with `prefix_len` 0); `out_len` is
/// valid for one write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_peek_length(
    prefix: *const u8,
    prefix_len: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let prefix = abi::bytes(prefix, prefix_len)?;
            let out_len = abi::out(out_len)?;
            out_len.write(SpacePacket::peek_length(prefix)?);
            Ok(())
        })
    }
}

/// Encodes a Space Packet from its fields, secondary header, and User Data Field in one call
/// (133.0 §4.1; SPP-005, SPP-006, API-020…022).
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` for an empty data field (`data_field`) or an Idle Packet with a
/// secondary header (`secondary_header_flag`), `ALDIS_ERR_OUT_OF_RANGE` (`packet_len`) above
/// the Maximum Packet Length, `ALDIS_ERR_BUFFER_TOO_SMALL` (with `needed`), the errors of the
/// fields (`apid`, `sequence_count`); CABI-022 violations, including `out` overlapping an input.
///
/// # Safety
/// `cfg` and `fields` are valid for reads; `secondary` and `user` are valid for their lengths
/// (or `NULL` with length 0); `out` is valid for `cap` octets; `out_len` is valid for one write;
/// `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_encode(
    cfg: *const aldis_spp_config_t,
    fields: *const aldis_spp_fields_t,
    secondary: *const u8,
    secondary_len: usize,
    user: *const u8,
    user_len: usize,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL, alignment, and
    // that `out` overlaps no input.
    unsafe {
        abi::call(err, || {
            let config = config(abi::config(cfg)?)?;
            let fields = self::fields(abi::read(fields)?)?;
            let sec = abi::bytes(secondary, secondary_len)?;
            let user_data = abi::bytes(user, user_len)?;
            let out = abi::bytes_mut(out, cap, &[(secondary, secondary_len), (user, user_len)])?;
            let out_len = abi::out(out_len)?;
            let packet = config.packet(fields, sec, user_data)?;
            out_len.write(encode(&packet, out)?);
            Ok(())
        })
    }
}

/// Starts a two-phase write (133.0 §4.1; SPP-005, API-023, M2 plan D4).
///
/// Checks the arguments, writes an unfinished Primary Header (version `111`, which no decoder
/// accepts), and reports where the caller fills in the secondary header and the User Data Field.
///
/// No state is kept: [`aldis_spp_writer_finish`] takes the same arguments again. The buffer
/// holds no valid packet until it is finished (API-023).
///
/// # Errors
/// As [`aldis_spp_encode`], with the packet length taken as `6 + secondary_len + max_user_len`.
///
/// # Safety
/// `cfg` and `fields` are valid for reads; `out` is valid for `cap` octets; `layout` is valid
/// for one write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_writer_begin(
    cfg: *const aldis_spp_config_t,
    fields: *const aldis_spp_fields_t,
    secondary_len: usize,
    max_user_len: usize,
    out: *mut u8,
    cap: usize,
    layout: *mut aldis_spp_writer_layout_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let args = WriterArgs {
                config: config(abi::config(cfg)?)?,
                fields: self::fields(abi::read(fields)?)?,
                secondary_len,
                max_user_len,
            };
            let out = abi::bytes_mut(out, cap, &[])?;
            let layout = abi::out(layout)?;
            writer(out, &args)?;
            layout.write(aldis_spp_writer_layout_t {
                secondary_header_offset: PRIMARY_HEADER_LEN,
                secondary_header_len: secondary_len,
                user_data_offset: PRIMARY_HEADER_LEN.saturating_add(secondary_len),
                user_data_cap: max_user_len,
            });
            Ok(())
        })
    }
}

/// Finishes a two-phase write begun with the same arguments: writes the final Primary Header
/// for `user_len` octets of user data and returns the packet length (133.0 §4.1; SPP-005,
/// API-021, API-023).
///
/// # Errors
/// As [`aldis_spp_writer_begin`], and `ALDIS_ERR_OUT_OF_RANGE` (`user_len`) above
/// `max_user_len`.
///
/// The call re-runs `begin`, so a finish with other arguments than `begin`'s gives a valid
/// packet for the arguments of the finish; OI-50's header check is not applied to it
/// (maintainer decision of 2026-10-25, revisited in M3).
///
/// # Safety
/// As [`aldis_spp_writer_begin`]; `out_len` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_writer_finish(
    cfg: *const aldis_spp_config_t,
    fields: *const aldis_spp_fields_t,
    secondary_len: usize,
    max_user_len: usize,
    user_len: usize,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let args = WriterArgs {
                config: config(abi::config(cfg)?)?,
                fields: self::fields(abi::read(fields)?)?,
                secondary_len,
                max_user_len,
            };
            let out = abi::bytes_mut(out, cap, &[])?;
            let out_len = abi::out(out_len)?;
            // `begin` rewrites only the unfinished Primary Header; the data field the caller
            // filled in stays as it is.
            out_len.write(writer(out, &args)?.finish(user_len)?);
            Ok(())
        })
    }
}

/// Builds an Idle Packet of `total_len` octets: APID 2047, Sequence Flags `11`, and the data
/// field filled by repeating `pattern` (one octet 0x00 when empty) (133.0 §4.1.3.3.4.4,
/// §4.1.4.3.4; SPP-010).
///
/// # Errors
/// `ALDIS_ERR_OUT_OF_RANGE` (`packet_len`) below 7 or above the Maximum Packet Length or
/// (`sequence_count`) above 16383, `ALDIS_ERR_BUFFER_TOO_SMALL`; CABI-022 violations.
///
/// # Safety
/// `cfg` is valid for a read; `pattern` is valid for `pattern_len` octets (or `NULL` with 0);
/// `out` is valid for `cap` octets; `out_len` is valid for one write; `err` is `NULL` or valid
/// for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_idle(
    cfg: *const aldis_spp_config_t,
    total_len: usize,
    sequence_count: u16,
    pattern: *const u8,
    pattern_len: usize,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL, alignment, and
    // overlap.
    unsafe {
        abi::call(err, || {
            let config = config(abi::config(cfg)?)?;
            let pattern_bytes = abi::bytes(pattern, pattern_len)?;
            let out = abi::bytes_mut(out, cap, &[(pattern, pattern_len)])?;
            let out_len = abi::out(out_len)?;
            let idle = IdlePacket::new(&config, total_len)?
                .with_sequence_count(SeqCount::try_from(sequence_count)?)
                .with_pattern(pattern_bytes);
            out_len.write(idle.encode(out)?);
            Ok(())
        })
    }
}

/// Encodes one octet string as a packet of the Octet String service (133.0 §3.4, §4.2.2;
/// SPP-012).
///
/// The packet is unsegmented, carries the count `sequence_count` and the Secondary Header
/// Flag `secondary_header_indicator`; `next_count` receives the count of the next packet.
///
/// # Errors
/// `ALDIS_ERR_INVALID_FIELD` for APID 2047 (`apid`) or an empty octet string (`data_field`),
/// `ALDIS_ERR_OUT_OF_RANGE` (`packet_len`, `apid`, `sequence_count`),
/// `ALDIS_ERR_BUFFER_TOO_SMALL`; CABI-022 violations. On error `next_count` is not written.
///
/// # Safety
/// `cfg` is valid for a read; `octets` is valid for `octets_len` octets; `out` is valid for
/// `cap` octets; `out_len` and `next_count` are valid for one write; `err` is `NULL` or valid
/// for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_octet_string_encode(
    cfg: *const aldis_spp_config_t,
    packet_type: u8,
    apid: u16,
    sequence_count: u16,
    secondary_header_indicator: bool,
    octets: *const u8,
    octets_len: usize,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    next_count: *mut u16,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL, alignment, and
    // overlap.
    unsafe {
        abi::call(err, || {
            let config = config(abi::config(cfg)?)?;
            let packet_type = self::packet_type(packet_type)?;
            let apid = Apid::try_from(apid)?;
            let counter = SequenceCounter::new(SeqCount::try_from(sequence_count)?);
            let data = abi::bytes(octets, octets_len)?;
            let out = abi::bytes_mut(out, cap, &[(octets, octets_len)])?;
            let out_len = abi::out(out_len)?;
            let next_count = abi::out(next_count)?;
            let mut enc = OctetStringEncoder::new(&config, packet_type, apid, counter)?;
            out_len.write(enc.encode(secondary_header_indicator, data, out)?);
            next_count.write(enc.counter().peek().get());
            Ok(())
        })
    }
}

/// The Packet Sequence Count after `count`, modulo 16384 (133.0 §4.1.3.4.3.3; SPP-011).
///
/// # Errors
/// `ALDIS_ERR_OUT_OF_RANGE` (`sequence_count`) above 16383; CABI-022 violations.
///
/// # Safety
/// `next` is valid for one write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_sequence_count_next(
    count: u16,
    next: *mut u16,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let next = abi::out(next)?;
            let mut counter = SequenceCounter::new(SeqCount::try_from(count)?);
            counter.advance();
            next.write(counter.peek().get());
            Ok(())
        })
    }
}

/// Checks a configuration: the Maximum Packet Length is 7 to 65 542 octets (133.0 §5.2
/// Table 5-1; SPP-005, API-040).
///
/// # Errors
/// `ALDIS_ERR_INVALID_CONFIG` (`max_packet_len`); CABI-022 violations.
///
/// # Safety
/// `cfg` is valid for a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_config_check(
    cfg: *const aldis_spp_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe { abi::call(err, || config(abi::config(cfg)?).map(|_| ())) }
}

/// Decodes one Space Packet and splits its data field by the APID's secondary header layout
/// (133.0 §4.1.4.2; SPP-007, SPP-008, CABI-012).
///
/// The layout is an opaque header, or a Time Code Field decoded with `aldis-time` followed by
/// Ancillary Data.
///
/// # Errors
/// The errors of [`aldis_spp_decode`]; `ALDIS_ERR_INVALID_FIELD` (`secondary_header_flag`) when
/// the flag disagrees with the layout, or (`time_code_id`) for a P-field of another time code;
/// `ALDIS_ERR_LENGTH_MISMATCH` (`secondary_header`) for a data field shorter than the
/// secondary header; `ALDIS_ERR_INVALID_CONFIG` (`time_code`) for a CCS or agency-defined
/// layout; the errors of the time code; CABI-022 violations.
///
/// # Safety
/// `bytes` is valid for `len` octets (or `NULL` with 0); `layout` is valid for a read; `out` is
/// valid for one write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_spp_split(
    bytes: *const u8,
    len: usize,
    mode: u32,
    layout: *const aldis_spp_secondary_layout_t,
    out: *mut aldis_spp_split_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let bytes = abi::bytes(bytes, len)?;
            let layout = abi::config(layout)?;
            split(bytes, mode, &layout, abi::out(out)?)
        })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use aldis::core::Anomaly as _;
    use aldis::spp::SppAnomaly;

    #[test]
    fn anomaly_constants_are_the_flag_bits() {
        // req: GEN-043, API-033
        assert_eq!(
            ALDIS_SPP_ANOMALY_IDLE_WITH_SECONDARY_HEADER,
            1 << SppAnomaly::IdleWithSecondaryHeader.bit()
        );
    }

    #[test]
    fn constants_are_the_crate_values() {
        // req: CABI-010
        assert_eq!(ALDIS_SPP_APID_IDLE, Apid::IDLE.get());
        assert_eq!(
            usize::try_from(ALDIS_SPP_PRIMARY_HEADER_LEN).ok(),
            Some(PRIMARY_HEADER_LEN)
        );
        assert_eq!(
            usize::try_from(ALDIS_SPP_MAX_PACKET_LEN).ok(),
            Some(aldis::spp::MAX_PACKET_LEN)
        );
        assert_eq!(
            usize::try_from(ALDIS_SPP_MIN_PACKET_LEN).ok(),
            Some(aldis::spp::MIN_PACKET_LEN)
        );
        for (c, f) in [
            (ALDIS_SPP_SEQ_CONTINUATION, SequenceFlags::Continuation),
            (ALDIS_SPP_SEQ_FIRST, SequenceFlags::First),
            (ALDIS_SPP_SEQ_LAST, SequenceFlags::Last),
            (ALDIS_SPP_SEQ_UNSEGMENTED, SequenceFlags::Unsegmented),
        ] {
            assert_eq!(f.bits(), c);
        }
    }

    #[test]
    fn configurations_start_with_struct_size() {
        // req: CABI-033
        assert_eq!(core::mem::offset_of!(aldis_spp_config_t, struct_size), 0);
        assert_eq!(
            core::mem::offset_of!(aldis_spp_secondary_layout_t, struct_size),
            0
        );
    }
}
