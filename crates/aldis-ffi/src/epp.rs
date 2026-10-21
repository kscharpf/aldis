//! C ABI of the Encapsulation Packet codec `aldis-epp` (CCSDS 133.1-B-3; M2 plan D4;
//! CABI-010…012, CABI-020…022, CABI-033).
//!
//! Calls: [`aldis_epp_decode`], [`aldis_epp_peek_length`], [`aldis_epp_validate`],
//! [`aldis_epp_check`], [`aldis_epp_encode`], the two-phase writer as
//! [`aldis_epp_writer_begin`] and [`aldis_epp_writer_finish`], [`aldis_epp_header_encode`],
//! [`aldis_epp_idle`], [`aldis_epp_idle_header`], and [`aldis_epp_config_check`]. EPI sets
//! are bit masks: bit `n` stands for EPI (or extended EPI) `n` (M2 plan D4).

use aldis::core::Result;
use aldis::epp::{
    EncapsulationFields, EncapsulationHeader, EncapsulationPacket, EncapsulationWriter, Epi,
    EpiExtension, EpiSet, EppConfig, ExtendedEpiSet, HeaderLen, HeaderSize, UserDefined, encode,
    idle,
};

use crate::abi::{self, Out, invalid_argument};

/// Header size argument: the smallest header that holds the fields and the length
/// (133.1 §4.1.2.4; EPP-008).
pub const ALDIS_EPP_HEADER_AUTO: u32 = 0;
/// Encapsulation Protocol ID 000: Idle Packet (133.1 §4.1.2.3).
pub const ALDIS_EPP_EPI_IDLE: u8 = 0b000;
/// Encapsulation Protocol ID 110: extended, identified by the Protocol ID Extension
/// (133.1 §4.1.2.3, §4.1.2.6).
pub const ALDIS_EPP_EPI_EXTENDED: u8 = 0b110;
/// Encapsulation Protocol ID 111: mission-specific (133.1 §4.1.2.3).
pub const ALDIS_EPP_EPI_MISSION_SPECIFIC: u8 = 0b111;
/// Largest Encapsulated Data Field, in octets: the largest Packet Length less an 8-octet
/// header (133.1 §4.1.2.8; EPP-009).
pub const ALDIS_EPP_MAX_DATA_LEN: u32 = 0xFFFF_FFF7;
/// Anomaly bit: non-zero EPI Extension with an EPI other than 110 (bit 0 of `EppAnomaly`;
/// 133.1 §4.1.2.6; API-032).
pub const ALDIS_EPP_ANOMALY_NON_ZERO_EPI_EXTENSION: u32 = 1 << 0;
/// Anomaly bit: non-zero CCSDS Defined Field (bit 1; 133.1 §4.1.2.7; API-032).
pub const ALDIS_EPP_ANOMALY_NON_ZERO_CCSDS_DEFINED: u32 = 1 << 1;

/// Managed parameters of the Encapsulation Packet Protocol (133.1 §5 Table 5-1; EPP-009,
/// EPP-010, CABI-033).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_epp_config_t {
    /// `sizeof(aldis_epp_config_t)`, set by the caller (CABI-033).
    pub struct_size: u32,
    /// Minimum Data Unit Length, octets.
    pub min_data_len: u32,
    /// Maximum Data Unit Length, octets, at most `ALDIS_EPP_MAX_DATA_LEN`.
    pub max_data_len: u32,
    /// Valid EPIs: bit `n` set accepts EPI `n`.
    pub valid_epis: u8,
    /// Valid extended EPIs with EPI 110: bit `n` set accepts Protocol ID Extension `n`.
    pub valid_extended_epis: u16,
}

/// The sender-chosen header fields of an Encapsulation Packet (133.1 §4.1.2; EPP-001…004).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_epp_fields_t {
    /// Encapsulation Protocol ID, 0 to 7.
    pub epi: u8,
    /// Protocol ID Extension, 0 to 15; non-zero only with EPI 110.
    pub epi_extension: u8,
    /// User Defined Field, 0 to 15.
    pub user_defined: u8,
}

/// A decoded Encapsulation Packet: its header and where its data field lies in the input
/// (133.1 §4.1; EPP-001…007, API-010, API-014, CABI-012).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_epp_packet_t {
    /// Encapsulation Protocol ID.
    pub epi: u8,
    /// Header length in octets: 1, 2, 4, or 8.
    pub header_len: u8,
    /// Whether the header has the User Defined Field and Protocol ID Extension (4 or 8 octets).
    pub has_extension: bool,
    /// Protocol ID Extension, when `has_extension`.
    pub epi_extension: u8,
    /// User Defined Field, when `has_extension`.
    pub user_defined: u8,
    /// Whether the header has the CCSDS Defined Field (8 octets).
    pub has_ccsds_defined: bool,
    /// CCSDS Defined Field, when `has_ccsds_defined`.
    pub ccsds_defined: u16,
    /// Packet Length: the total length, octets consumed from the input (API-011).
    pub packet_len: u32,
    /// Lenient-mode anomaly flags, `ALDIS_EPP_ANOMALY_*`; 0 in strict mode (API-032).
    pub anomalies: u32,
    /// Offset of the Encapsulated Data Field in the input: the header length.
    pub data_offset: usize,
    /// Length of the Encapsulated Data Field.
    pub data_len: usize,
}

/// Where a two-phase writer's caller fills in the data field (API-023, CABI-012).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_epp_writer_layout_t {
    /// Offset of the Encapsulated Data Field in the output buffer: the header length.
    pub data_offset: usize,
    /// Capacity of the data field: the `max_data_len` given.
    pub data_cap: usize,
}

/// The configuration of a C struct (133.1 §5; EPP-009, EPP-010, API-040).
const fn config(c: &aldis_epp_config_t) -> Result<EppConfig> {
    EppConfig::new(
        c.min_data_len,
        c.max_data_len,
        EpiSet::from_bits(c.valid_epis),
        ExtendedEpiSet::from_bits(c.valid_extended_epis),
    )
}

/// The header fields of a C struct, each parsed into its type (RS-020).
fn fields(f: aldis_epp_fields_t) -> Result<EncapsulationFields> {
    EncapsulationFields::from_parts(
        Epi::try_from(f.epi)?,
        EpiExtension::try_from(f.epi_extension)?,
        UserDefined::try_from(f.user_defined)?,
    )
}

/// A header size argument (EPP-008; CABI-022).
const fn header_size(size: u32) -> Result<HeaderSize> {
    match size {
        ALDIS_EPP_HEADER_AUTO => Ok(HeaderSize::Auto),
        1 => Ok(HeaderSize::Fixed(HeaderLen::One)),
        2 => Ok(HeaderSize::Fixed(HeaderLen::Two)),
        4 => Ok(HeaderSize::Fixed(HeaderLen::Four)),
        8 => Ok(HeaderSize::Fixed(HeaderLen::Eight)),
        _ => Err(invalid_argument()),
    }
}

/// The C view of a decoded packet.
pub(crate) fn view(p: &EncapsulationPacket<'_>) -> aldis_epp_packet_t {
    let header_len = p.header_len().octets();
    aldis_epp_packet_t {
        epi: p.epi().get(),
        header_len: u8::try_from(header_len).unwrap_or(0),
        has_extension: p.extension().is_some(),
        epi_extension: p.extension().map_or(0, EpiExtension::get),
        user_defined: p.user_defined().map_or(0, UserDefined::get),
        has_ccsds_defined: p.ccsds_defined().is_some(),
        ccsds_defined: p.ccsds_defined().unwrap_or(0),
        packet_len: p.header().packet_len(),
        anomalies: p.anomalies().bits(),
        data_offset: header_len,
        data_len: p.data().len(),
    }
}

fn write_header(h: &EncapsulationHeader, out: &mut [u8], out_len: Out<'_, usize>) -> Result<()> {
    out_len.write(h.encode(out)?);
    Ok(())
}

/// Decodes one Encapsulation Packet (133.1 §4.1; EPP-001…007, API-010, API-011, API-030).
///
/// # Errors
/// `ALDIS_ERR_INCOMPLETE` (with `needed`), `ALDIS_ERR_UNSUPPORTED_VERSION` (`packet_version`),
/// `ALDIS_ERR_INVALID_FIELD` (`epi`) for a header that cannot carry the EPI or an empty data
/// field with a non-idle EPI, `ALDIS_ERR_LENGTH_MISMATCH` (`packet_len`) for a Packet Length
/// below the header length, and in strict mode `ALDIS_ERR_RESERVED_VALUE` (`epi_extension`,
/// `ccsds_defined`); CABI-022 violations.
///
/// # Safety
/// `bytes` is valid for `len` octets (or `NULL` with 0); `out` is valid for one write; `err` is
/// `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_epp_decode(
    bytes: *const u8,
    len: usize,
    mode: u32,
    out: *mut aldis_epp_packet_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let bytes = abi::bytes(bytes, len)?;
            let mode = abi::mode(mode)?;
            let out = abi::out(out)?;
            out.write(view(&EncapsulationPacket::decode(bytes, mode)?));
            Ok(())
        })
    }
}

/// The total length of the Encapsulation Packet that `prefix` begins, from its header alone
/// (133.1 §4.1.2.8; API-013, M2 plan D14).
///
/// # Errors
/// `ALDIS_ERR_INCOMPLETE` with `needed` (1, or the header length the first octet gives) for a
/// prefix too short to hold the length, and the header errors of [`aldis_epp_decode`];
/// CABI-022 violations.
///
/// # Safety
/// `prefix` is valid for `prefix_len` octets (or `NULL` with 0); `out_len` is valid for one
/// write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_epp_peek_length(
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
            out_len.write(EncapsulationPacket::peek_length(prefix)?);
            Ok(())
        })
    }
}

/// Decodes one Encapsulation Packet and checks it against the managed parameters (133.1 §5;
/// EPP-009, EPP-010).
///
/// Its EPI and extended EPI must be valid and its data field length within the limits. Idle
/// Packets always pass.
///
/// # Errors
/// The errors of [`aldis_epp_decode`]; `ALDIS_ERR_UNKNOWN_CHANNEL` (`epi`, `epi_extension`),
/// `ALDIS_ERR_OUT_OF_RANGE` (`data_len`); the errors of the configuration; CABI-022 violations.
///
/// # Safety
/// `bytes` is valid for `len` octets (or `NULL` with 0); `cfg` is valid for a read; `err` is
/// `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_epp_validate(
    bytes: *const u8,
    len: usize,
    mode: u32,
    cfg: *const aldis_epp_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let config = config(&abi::config(cfg)?)?;
            let bytes = abi::bytes(bytes, len)?;
            let packet = EncapsulationPacket::decode(bytes, abi::mode(mode)?)?;
            packet.validate(&config)
        })
    }
}

/// Checks an EPI, an optional Protocol ID Extension, and a data field length against the
/// managed parameters, as [`aldis_epp_validate`] does for a decoded packet (133.1 §5; EPP-009,
/// EPP-010).
///
/// # Errors
/// `ALDIS_ERR_UNKNOWN_CHANNEL` (`epi`, `epi_extension`), `ALDIS_ERR_OUT_OF_RANGE` (`data_len`,
/// `epi`, `epi_extension`); the errors of the configuration; CABI-022 violations.
///
/// # Safety
/// `cfg` is valid for a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_epp_check(
    cfg: *const aldis_epp_config_t,
    epi: u8,
    has_extension: bool,
    epi_extension: u8,
    data_len: usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let config = config(&abi::config(cfg)?)?;
            let epi = Epi::try_from(epi)?;
            let extension = if has_extension {
                Some(EpiExtension::try_from(epi_extension)?)
            } else {
                None
            };
            config.check(epi, extension, data_len)
        })
    }
}

/// Encodes an Encapsulation Packet in one call; `header_size` is `ALDIS_EPP_HEADER_AUTO` or a
/// header length of 1, 2, 4, or 8 octets (133.1 §4.1; EPP-001…009, API-020…022).
///
/// # Errors
/// `ALDIS_ERR_UNKNOWN_CHANNEL` (`epi`, `epi_extension`) and `ALDIS_ERR_OUT_OF_RANGE`
/// (`data_len`) against the configuration, `ALDIS_ERR_INVALID_FIELD` (`epi`, `epi_extension`,
/// `header_size`), `ALDIS_ERR_OUT_OF_RANGE` (`packet_len`) when the length does not fit the
/// header, `ALDIS_ERR_BUFFER_TOO_SMALL`; CABI-022 violations, including `out` overlapping
/// `data`.
///
/// # Safety
/// `cfg` and `fields` are valid for reads; `data` is valid for `data_len` octets (or `NULL`
/// with 0); `out` is valid for `cap` octets; `out_len` is valid for one write; `err` is `NULL`
/// or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_epp_encode(
    cfg: *const aldis_epp_config_t,
    fields: *const aldis_epp_fields_t,
    header_size: u32,
    data: *const u8,
    data_len: usize,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL, alignment, and
    // overlap.
    unsafe {
        abi::call(err, || {
            let config = config(&abi::config(cfg)?)?;
            let fields = self::fields(abi::read(fields)?)?;
            let size = self::header_size(header_size)?;
            let data_bytes = abi::bytes(data, data_len)?;
            let out = abi::bytes_mut(out, cap, &[(data, data_len)])?;
            let out_len = abi::out(out_len)?;
            let unit = config.data_unit(fields, data_bytes)?;
            out_len.write(encode(&unit, size, out)?);
            Ok(())
        })
    }
}

/// Starts a two-phase write: checks the arguments, writes the header with Packet Length 0,
/// and reports where the caller fills in the data field (133.1 §4.1; API-023, M2 plan D4).
///
/// No state is kept: [`aldis_epp_writer_finish`] takes the same arguments again.
///
/// # Errors
/// As [`aldis_epp_encode`], with the header sized for `max_data_len`.
///
/// # Safety
/// `cfg` and `fields` are valid for reads; `out` is valid for `cap` octets; `layout` is valid
/// for one write; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_epp_writer_begin(
    cfg: *const aldis_epp_config_t,
    fields: *const aldis_epp_fields_t,
    header_size: u32,
    max_data_len: usize,
    out: *mut u8,
    cap: usize,
    layout: *mut aldis_epp_writer_layout_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let config = config(&abi::config(cfg)?)?;
            let fields = self::fields(abi::read(fields)?)?;
            let size = self::header_size(header_size)?;
            let out = abi::bytes_mut(out, cap, &[])?;
            let layout = abi::out(layout)?;
            let base = out.as_ptr().addr();
            let mut w = EncapsulationWriter::begin(out, &config, fields, size, max_data_len)?;
            // The data field follows the header that `begin` chose for `max_data_len`.
            let data = w.data_mut();
            layout.write(aldis_epp_writer_layout_t {
                data_offset: data.as_ptr().addr().saturating_sub(base),
                data_cap: data.len(),
            });
            Ok(())
        })
    }
}

/// Finishes a two-phase write begun with the same arguments: writes the Packet Length for
/// `data_len` octets and returns the packet length (133.1 §4.1; API-021, API-023).
///
/// # Errors
/// As [`aldis_epp_writer_begin`], `ALDIS_ERR_OUT_OF_RANGE` (`data_len`) above `max_data_len` or
/// outside the configured limits, and `ALDIS_ERR_INVALID_FIELD` (`epi`) for an empty data field
/// with a non-idle EPI.
///
/// # Safety
/// As [`aldis_epp_writer_begin`]; `out_len` is valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_epp_writer_finish(
    cfg: *const aldis_epp_config_t,
    fields: *const aldis_epp_fields_t,
    header_size: u32,
    max_data_len: usize,
    data_len: usize,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let config = config(&abi::config(cfg)?)?;
            let fields = self::fields(abi::read(fields)?)?;
            let size = self::header_size(header_size)?;
            let out = abi::bytes_mut(out, cap, &[])?;
            let out_len = abi::out(out_len)?;
            // `begin` rewrites only the header; the data field the caller filled in stays.
            let w = EncapsulationWriter::begin(out, &config, fields, size, max_data_len)?;
            out_len.write(w.finish(data_len)?);
            Ok(())
        })
    }
}

/// Encodes only the header of a packet with `data_len` octets of data, as
/// [`aldis_epp_encode`] would write it (133.1 §4.1.2; EPP-008). `packet_len` receives the
/// Packet Length.
///
/// # Errors
/// As [`aldis_epp_encode`]; CABI-022 violations.
///
/// # Safety
/// `cfg` and `fields` are valid for reads; `out` is valid for `cap` octets; `out_len` and
/// `packet_len` are valid for one write each; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_epp_header_encode(
    cfg: *const aldis_epp_config_t,
    fields: *const aldis_epp_fields_t,
    header_size: u32,
    data_len: usize,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    packet_len: *mut u32,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let config = config(&abi::config(cfg)?)?;
            let fields = self::fields(abi::read(fields)?)?;
            let size = self::header_size(header_size)?;
            let out = abi::bytes_mut(out, cap, &[])?;
            let out_len = abi::out(out_len)?;
            let packet_len = abi::out(packet_len)?;
            let h = config.header(fields, size, data_len)?;
            write_header(&h, out, out_len)?;
            packet_len.write(h.packet_len());
            Ok(())
        })
    }
}

/// Builds an Idle Packet of `total_len` octets with the smallest header for the length and
/// the data field filled with `fill` (133.1 §4.1.2.3; EPP-005).
///
/// # Errors
/// `ALDIS_ERR_OUT_OF_RANGE` (`packet_len`) for 0 or more than 2³² − 1 octets,
/// `ALDIS_ERR_BUFFER_TOO_SMALL`; CABI-022 violations.
///
/// # Safety
/// `out` is valid for `cap` octets; `out_len` is valid for one write; `err` is `NULL` or valid
/// for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_epp_idle(
    total_len: usize,
    fill: u8,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let out = abi::bytes_mut(out, cap, &[])?;
            let out_len = abi::out(out_len)?;
            out_len.write(idle(total_len, fill, out)?);
            Ok(())
        })
    }
}

/// Encodes only the header of an Idle Packet of `total_len` octets (133.1 §4.1.2.3;
/// EPP-005, EPP-008). `packet_len` receives the Packet Length.
///
/// # Errors
/// As [`aldis_epp_idle`].
///
/// # Safety
/// `out` is valid for `cap` octets; `out_len` and `packet_len` are valid for one write each;
/// `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_epp_idle_header(
    total_len: usize,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
    packet_len: *mut u32,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe {
        abi::call(err, || {
            let out = abi::bytes_mut(out, cap, &[])?;
            let out_len = abi::out(out_len)?;
            let packet_len = abi::out(packet_len)?;
            let h = EncapsulationHeader::idle(total_len)?;
            write_header(&h, out, out_len)?;
            packet_len.write(h.packet_len());
            Ok(())
        })
    }
}

/// Checks a configuration: the minimum data length is at most the maximum, and the maximum
/// at most `ALDIS_EPP_MAX_DATA_LEN` (133.1 §5 Table 5-1; EPP-009, API-040).
///
/// # Errors
/// `ALDIS_ERR_INVALID_CONFIG` (`min_data_len`, `max_data_len`); CABI-022 violations.
///
/// # Safety
/// `cfg` is valid for a read; `err` is `NULL` or valid for one write.
#[expect(
    unsafe_code,
    reason = "C ABI entry point: unmangled, takes raw pointers (GEN-071)"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aldis_epp_config_check(
    cfg: *const aldis_epp_config_t,
    err: *mut crate::aldis_error_detail_t,
) -> i32 {
    // SAFETY: every pointer is valid as `# Safety` states; `abi` checks NULL and alignment.
    unsafe { abi::call(err, || config(&abi::config(cfg)?).map(|_| ())) }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use aldis::core::Anomaly as _;
    use aldis::epp::EppAnomaly;

    #[test]
    fn constants_are_the_crate_values() {
        // req: GEN-043, API-033, CABI-010
        assert_eq!(
            ALDIS_EPP_ANOMALY_NON_ZERO_EPI_EXTENSION,
            1 << EppAnomaly::NonZeroEpiExtension.bit()
        );
        assert_eq!(
            ALDIS_EPP_ANOMALY_NON_ZERO_CCSDS_DEFINED,
            1 << EppAnomaly::NonZeroCcsdsDefined.bit()
        );
        assert_eq!(ALDIS_EPP_MAX_DATA_LEN, aldis::epp::config::MAX_DATA_LEN);
        assert_eq!(ALDIS_EPP_EPI_IDLE, Epi::IDLE.get());
        assert_eq!(ALDIS_EPP_EPI_EXTENDED, Epi::EXTENDED.get());
        assert_eq!(ALDIS_EPP_EPI_MISSION_SPECIFIC, Epi::MISSION_SPECIFIC.get());
        assert_eq!(core::mem::offset_of!(aldis_epp_config_t, struct_size), 0);
    }

    #[test]
    fn header_sizes_take_only_their_constants() {
        // req: EPP-008, CABI-022
        assert_eq!(header_size(ALDIS_EPP_HEADER_AUTO), Ok(HeaderSize::Auto));
        assert_eq!(header_size(8), Ok(HeaderSize::Fixed(HeaderLen::Eight)));
        assert_eq!(header_size(3), Err(invalid_argument()));
    }
}
