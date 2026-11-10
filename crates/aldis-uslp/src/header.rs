//! Transfer Frame Primary Header, non-truncated and truncated (732.1 §4.1.2, Annex D1.3;
//! USLP-001, USLP-003, USLP-004, API-014, GEN-052, GEN-053).

use aldis_core::ids::{MapId, SpacecraftId, VcId};
use aldis_core::{CopFrameKind, Error, ErrorDetail, ErrorKind, FieldId};

use crate::checked::Reason;
use crate::field;
use aldis_core::Rejection;

/// The Spacecraft Identifier of a USLP frame: 16 bits (732.1 §4.1.2.2.3; API-002).
pub type UslpSpacecraftId = SpacecraftId<16>;
/// The Virtual Channel Identifier of a USLP frame: 6 bits, 63 for OID frames (732.1 §4.1.2.4).
pub type UslpVcId = VcId<6>;
/// The MAP Identifier of a USLP frame: 4 bits (732.1 §4.1.2.5).
pub type UslpMapId = MapId<4>;

/// Length of the non-truncated Primary Header without its VCF Count, in octets (732.1
/// §4.1.2.1.2, Figure 4-2).
pub const PRIMARY_HEADER_LEN: usize = 7;

/// Length of the truncated Primary Header, in octets (732.1 Annex D1.2.2 a).
pub const TRUNCATED_HEADER_LEN: usize = 4;

/// Longest USLP frame: the 16-bit Frame Length field counts up to 65 536 octets (732.1
/// §4.1.2.7 note 4).
pub const MAX_FRAME_LEN: usize = 65_536;

/// The VCID of OID frames, `all ones` (732.1 §4.1.4.1.6; USLP-009).
pub const OID_VCID: u8 = 63;

/// The VCID of OID frames as the identifier type (732.1 §4.1.4.1.6; USLP-009).
pub const OID_VC: UslpVcId = UslpVcId::new(OID_VCID).expect("63 fits in 6 bits");

/// Transfer Frame Version Number of a USLP frame, `1100` (732.1 §4.1.2.2.2.2).
pub(crate) const VERSION: u8 = 0b1100;

/// Version field of an unfinished two-phase writer, which every decoder rejects (API-023).
pub(crate) const UNFINISHED_VERSION: u8 = 0b1111;

/// Whether the SCID names the source or the destination of the frame: the
/// Source-or-Destination Identifier (732.1 §4.1.2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceOrDestination {
    /// `0`: the SCID is the frame's source.
    Source,
    /// `1`: the SCID is the frame's destination.
    Destination,
}

impl SourceOrDestination {
    /// The identifier of the bit (732.1 §4.1.2.3.3).
    #[must_use]
    pub const fn from_bit(bit: bool) -> SourceOrDestination {
        if bit {
            SourceOrDestination::Destination
        } else {
            SourceOrDestination::Source
        }
    }

    /// The bit: `true` for the destination (732.1 §4.1.2.3.3).
    #[must_use]
    pub const fn bit(self) -> bool {
        matches!(self, SourceOrDestination::Destination)
    }
}

/// What a non-truncated frame carries, by its Bypass/Sequence Control and Protocol Control
/// Command Flags (732.1 §4.1.2.8, Table 4-1; USLP-001).
///
/// The combination `01` is reserved and has no variant: the decoder rejects it in both modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameKind {
    /// `00`: user data, Sequence-Controlled service.
    SequenceControlled,
    /// `10`: user data, Expedited service.
    Expedited,
    /// `11`: Protocol Control Commands, with the Frame Acceptance Checks bypassed.
    ProtocolControl,
}

impl FrameKind {
    /// The kind of the two flags, or `None` for the reserved `01` (732.1 Table 4-1).
    #[must_use]
    pub const fn from_flags(bypass: bool, protocol_control: bool) -> Option<FrameKind> {
        match (bypass, protocol_control) {
            (false, false) => Some(FrameKind::SequenceControlled),
            (true, false) => Some(FrameKind::Expedited),
            (true, true) => Some(FrameKind::ProtocolControl),
            (false, true) => None,
        }
    }

    /// The Bypass/Sequence Control Flag: `true` unless sequence-controlled (732.1 §4.1.2.8.1).
    /// It also selects the VC's VCF Count Length (USLP-004).
    #[must_use]
    pub const fn bypass_flag(self) -> bool {
        !matches!(self, FrameKind::SequenceControlled)
    }

    /// The Protocol Control Command Flag (732.1 §4.1.2.8.2).
    #[must_use]
    pub const fn protocol_control_flag(self) -> bool {
        matches!(self, FrameKind::ProtocolControl)
    }
}

impl From<CopFrameKind> for FrameKind {
    /// Type-AD is sequence-controlled, Type-BD expedited, and Type-BC Protocol Control
    /// (732.1 Table 4-1, 232.1 §4.2.7.4; USLP-001, COP-010).
    fn from(kind: CopFrameKind) -> FrameKind {
        match kind {
            CopFrameKind::Ad => FrameKind::SequenceControlled,
            CopFrameKind::Bd => FrameKind::Expedited,
            CopFrameKind::Bc => FrameKind::ProtocolControl,
        }
    }
}

impl From<FrameKind> for CopFrameKind {
    fn from(kind: FrameKind) -> CopFrameKind {
        match kind {
            FrameKind::SequenceControlled => CopFrameKind::Ad,
            FrameKind::Expedited => CopFrameKind::Bd,
            FrameKind::ProtocolControl => CopFrameKind::Bc,
        }
    }
}

/// The length of a VCF Count field, 0 to 7 octets (732.1 §4.1.2.11, Table 4-2; USLP-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct VcfCountLen(u8);

impl VcfCountLen {
    /// No VCF Count field.
    pub const NONE: VcfCountLen = VcfCountLen(0);

    /// A length of `octets`, at most 7 (732.1 Table 4-2).
    ///
    /// # Errors
    /// `InvalidConfig` (`vcf_count_len`) above 7.
    pub const fn new(octets: u8) -> Result<VcfCountLen, Error> {
        if octets > 7 {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::VCF_COUNT_LEN));
        }
        Ok(VcfCountLen(octets))
    }

    /// The length in octets (732.1 Table 4-2).
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// The length in octets, as a `usize`.
    #[must_use]
    pub fn octets(self) -> usize {
        usize::from(self.0)
    }

    /// The largest count the field holds, `2^(8·len) − 1`; 0 without a field (732.1
    /// §4.1.2.12.3, Table 4-2).
    #[must_use]
    pub fn max_count(self) -> u64 {
        let unused_bits = 64_u32.saturating_sub(8_u32.saturating_mul(u32::from(self.0)));
        u64::MAX.checked_shr(unused_bits).unwrap_or(0)
    }
}

/// A Virtual Channel Frame Count with its length (732.1 §4.1.2.11–12; USLP-004).
///
/// Built only by [`new`](Self::new), so the count fits the field (RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct VcfCount {
    len: VcfCountLen,
    value: u64,
}

impl VcfCount {
    /// `value` in a field of `len` octets, or `None` if it does not fit (732.1 §4.1.2.12.3).
    #[must_use]
    pub fn new(len: VcfCountLen, value: u64) -> Option<VcfCount> {
        if value > len.max_count() {
            return None;
        }
        Some(VcfCount { len, value })
    }

    /// `value` modulo the field of `len` octets: the count of a counter that wraps (732.1
    /// §4.1.2.12.3; LNK-032).
    #[must_use]
    pub fn wrapping(len: VcfCountLen, value: u64) -> VcfCount {
        VcfCount {
            len,
            value: value & len.max_count(),
        }
    }

    /// The length of the field (732.1 §4.1.2.11).
    #[must_use]
    pub const fn len(self) -> VcfCountLen {
        self.len
    }

    /// The count (732.1 §4.1.2.12).
    #[must_use]
    pub const fn value(self) -> u64 {
        self.value
    }
}

/// The fields of a frame's first four octets, which truncated and non-truncated Primary Headers
/// share (732.1 §4.1.2.2–6, Annex D1.3.1 note 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChannelFields {
    /// Spacecraft Identifier (732.1 §4.1.2.2.3).
    pub spacecraft_id: UslpSpacecraftId,
    /// Source-or-Destination Identifier (732.1 §4.1.2.3).
    pub source_or_destination: SourceOrDestination,
    /// Virtual Channel Identifier (732.1 §4.1.2.4).
    pub vc_id: UslpVcId,
    /// MAP Identifier (732.1 §4.1.2.5).
    pub map_id: UslpMapId,
}

/// A non-truncated Primary Header as plain values (732.1 §4.1.2; API-014).
///
/// Built only by the decoder, so the frame length is the Frame Length field plus one, 1–65 536,
/// and the VCF Count fits its length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UslpPrimaryHeader {
    channel: ChannelFields,
    frame_len: u32,
    kind: FrameKind,
    ocf_flag: bool,
    vcf_count: VcfCount,
}

impl UslpPrimaryHeader {
    /// A header with checked fields.
    pub(crate) fn new(
        channel: ChannelFields,
        frame_len: usize,
        kind: FrameKind,
        ocf_flag: bool,
        vcf_count: VcfCount,
    ) -> UslpPrimaryHeader {
        UslpPrimaryHeader {
            channel,
            frame_len: u32::try_from(frame_len).unwrap_or(u32::MAX),
            kind,
            ocf_flag,
            vcf_count,
        }
    }

    /// The header a writer puts in a frame of `frame_len` octets: the Reserve Spares `00`
    /// (732.1 §4.1.2; USLP-001, GEN-052, GEN-053).
    ///
    /// # Errors
    /// `OutOfRange` (`frame_len`) for a length above [`MAX_FRAME_LEN`] or 0: the 16-bit Frame
    /// Length field holds neither, and a length is an error, not a clamped value.
    pub fn for_writer(
        channel: ChannelFields,
        frame_len: usize,
        kind: FrameKind,
        ocf_flag: bool,
        vcf_count: VcfCount,
    ) -> Result<UslpPrimaryHeader, Error> {
        if frame_len == 0 || frame_len > MAX_FRAME_LEN {
            return Err(Error::out_of_range(
                field::FRAME_LEN,
                len_u64(MAX_FRAME_LEN),
                len_u64(frame_len),
            ));
        }
        Ok(UslpPrimaryHeader::new(
            channel, frame_len, kind, ocf_flag, vcf_count,
        ))
    }

    /// The fields the truncated header shares (732.1 §4.1.2.2–5).
    #[must_use]
    pub const fn channel(&self) -> ChannelFields {
        self.channel
    }

    /// Spacecraft Identifier (732.1 §4.1.2.2.3).
    #[must_use]
    pub const fn spacecraft_id(&self) -> UslpSpacecraftId {
        self.channel.spacecraft_id
    }

    /// Source-or-Destination Identifier (732.1 §4.1.2.3).
    #[must_use]
    pub const fn source_or_destination(&self) -> SourceOrDestination {
        self.channel.source_or_destination
    }

    /// Virtual Channel Identifier (732.1 §4.1.2.4).
    #[must_use]
    pub const fn vc_id(&self) -> UslpVcId {
        self.channel.vc_id
    }

    /// MAP Identifier (732.1 §4.1.2.5).
    #[must_use]
    pub const fn map_id(&self) -> UslpMapId {
        self.channel.map_id
    }

    /// Total frame length in octets: the Frame Length field plus one (732.1 §4.1.2.7;
    /// USLP-003, GEN-053).
    #[must_use]
    pub fn frame_len(&self) -> usize {
        usize::try_from(self.frame_len).unwrap_or(usize::MAX)
    }

    /// The Bypass/Sequence Control and Protocol Control Command Flags (732.1 Table 4-1;
    /// USLP-001).
    #[must_use]
    pub const fn kind(&self) -> FrameKind {
        self.kind
    }

    /// The OCF Flag (732.1 §4.1.2.10).
    #[must_use]
    pub const fn ocf_flag(&self) -> bool {
        self.ocf_flag
    }

    /// The VCF Count with its length (732.1 §4.1.2.11–12; USLP-004).
    #[must_use]
    pub const fn vcf_count(&self) -> VcfCount {
        self.vcf_count
    }

    /// Length of the header with its VCF Count, 7–14 octets (732.1 §4.1.2.1.2).
    #[must_use]
    pub fn len(&self) -> usize {
        PRIMARY_HEADER_LEN.saturating_add(self.vcf_count.len.octets())
    }

    /// Always `false`: a Primary Header has at least seven octets.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// The header octets, with the Reserve Spares `00`, in the first [`len`](Self::len)
    /// octets of the array (732.1 §4.1.2; API-020, GEN-052).
    #[must_use]
    pub fn octets(&self) -> [u8; 14] {
        primary_octets(
            VERSION,
            self.channel,
            self.frame_len(),
            self.kind,
            self.ocf_flag,
            self.vcf_count,
        )
    }
}

/// The first four octets as read, before any check (732.1 §4.1.2.2–6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RawStart {
    pub(crate) version: u8,
    pub(crate) channel: ChannelFields,
    pub(crate) truncated: bool,
}

impl RawStart {
    /// Reads the first four octets; `None` if `bytes` is shorter.
    pub(crate) fn parse(bytes: &[u8]) -> Option<RawStart> {
        let h = bytes.first_chunk::<TRUNCATED_HEADER_LEN>()?;
        let ids = UslpSpacecraftId::new(layout::Scid::<4>::get_u16(h))
            .zip(UslpVcId::new(layout::Vcid::<4>::get_u8(h)))
            .zip(UslpMapId::new(layout::Map::<4>::get_u8(h)));
        ids.map(|((spacecraft_id, vc_id), map_id)| RawStart {
            version: layout::Version::<4>::get_u8(h),
            channel: ChannelFields {
                spacecraft_id,
                source_or_destination: SourceOrDestination::from_bit(
                    layout::SourceOrDestination::<4>::flag(h),
                ),
                vc_id,
                map_id,
            },
            truncated: layout::EndOfPrimaryHeader::<4>::flag(h),
        })
    }

    /// `UnsupportedVersion` naming `frame_version` unless the TFVN is `1100` (732.1
    /// §4.1.2.2.2.2; USLP-001).
    pub(crate) fn check_version(self) -> Result<(), Error> {
        if self.version == VERSION {
            return Ok(());
        }
        Err(Error::new(ErrorKind::UnsupportedVersion)
            .with_field(field::FRAME_VERSION)
            .with_detail(ErrorDetail::Expected {
                expected: u64::from(VERSION),
                found: u64::from(self.version),
            }))
    }
}

/// Octets 4–6 of a non-truncated header, and the VCF Count after them (732.1 §4.1.2.7–12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RawRest {
    /// The total frame length: the Frame Length field plus one (GEN-053).
    pub(crate) frame_len: usize,
    pub(crate) bypass: bool,
    pub(crate) protocol_control: bool,
    pub(crate) spare: u8,
    pub(crate) ocf_flag: bool,
    pub(crate) vcf_count_len: VcfCountLen,
}

impl RawRest {
    /// Reads octets 4–6; `None` if `bytes` is shorter than seven octets.
    pub(crate) fn parse(bytes: &[u8]) -> Option<RawRest> {
        let h = bytes.first_chunk::<PRIMARY_HEADER_LEN>()?;
        Some(RawRest {
            frame_len: usize::from(layout::FrameLength::<PRIMARY_HEADER_LEN>::get_u16(h))
                .saturating_add(1),
            bypass: layout::Bypass::<PRIMARY_HEADER_LEN>::flag(h),
            protocol_control: layout::ProtocolControl::<PRIMARY_HEADER_LEN>::flag(h),
            spare: layout::Spare::<PRIMARY_HEADER_LEN>::get_u8(h),
            ocf_flag: layout::OcfFlag::<PRIMARY_HEADER_LEN>::flag(h),
            vcf_count_len: VcfCountLen(layout::CountLength::<PRIMARY_HEADER_LEN>::get_u8(h)),
        })
    }

    /// The frame kind, or `ReservedValue` naming `protocol_control_command_flag` for the
    /// reserved `01`, in both modes (732.1 Table 4-1; USLP-001).
    pub(crate) const fn kind(&self) -> Result<FrameKind, Error> {
        match FrameKind::from_flags(self.bypass, self.protocol_control) {
            Some(k) => Ok(k),
            None => Err(Error::new(ErrorKind::ReservedValue)
                .with_field(field::PROTOCOL_CONTROL_COMMAND_FLAG)),
        }
    }
}

/// The VCF Count of `len` octets at octet 7 of `frame`; 0 when `len` is 0, the field absent
/// (732.1 §4.1.2.12.1; USLP-004, GEN-020).
///
/// # Errors
/// A `LengthMismatch` rejection (USLP-014 check d) carrying `Incomplete` (`needed` = 7 plus
/// `len`) if `frame` ends inside the field.
pub(crate) fn read_vcf_count(
    frame: &[u8],
    len: VcfCountLen,
) -> Result<VcfCount, Rejection<Reason>> {
    let end = PRIMARY_HEADER_LEN.saturating_add(len.octets());
    let octets = frame.get(PRIMARY_HEADER_LEN..end).ok_or(Rejection::new(
        Reason::LengthMismatch,
        Error::incomplete(end),
    ))?;
    Ok(VcfCount {
        len,
        value: aldis_core::bits::be_uint(octets),
    })
}

/// The first four octets of a header (732.1 §4.1.2.2–6, Annex D1.3.1).
pub(crate) fn start_octets(version: u8, channel: ChannelFields, truncated: bool) -> [u8; 4] {
    let mut h = [0_u8; TRUNCATED_HEADER_LEN];
    put_start(&mut h, version, channel, truncated);
    h
}

/// Writes the fields of the first four octets into a header of `N` octets.
fn put_start<const N: usize>(
    h: &mut [u8; N],
    version: u8,
    channel: ChannelFields,
    truncated: bool,
) {
    layout::Version::<N>::put_u8(h, version & 0x0F);
    layout::Scid::<N>::put_u16(h, channel.spacecraft_id.get());
    layout::SourceOrDestination::<N>::put_flag(h, channel.source_or_destination.bit());
    layout::Vcid::<N>::put_u8(h, channel.vc_id.get());
    layout::Map::<N>::put_u8(h, channel.map_id.get());
    layout::EndOfPrimaryHeader::<N>::put_flag(h, truncated);
}

/// The non-truncated header, `PRIMARY_HEADER_LEN` + the VCF Count Length octets of a 14-octet
/// array, with `version` and the Reserve Spares `00` (732.1 §4.1.2; GEN-052, GEN-053).
/// `frame_len` is the total length, 1–65 536.
pub(crate) fn primary_octets(
    version: u8,
    channel: ChannelFields,
    frame_len: usize,
    kind: FrameKind,
    ocf_flag: bool,
    count: VcfCount,
) -> [u8; 14] {
    const N: usize = MAX_HEADER_LEN;
    let mut out = [0_u8; N];
    put_start(&mut out, version, channel, false);
    // The Frame Length field holds the length less one (732.1 §4.1.2.7).
    layout::FrameLength::<N>::put_u16(
        &mut out,
        u16::try_from(frame_len.saturating_sub(1)).unwrap_or(u16::MAX),
    );
    layout::Bypass::<N>::put_flag(&mut out, kind.bypass_flag());
    layout::ProtocolControl::<N>::put_flag(&mut out, kind.protocol_control_flag());
    layout::OcfFlag::<N>::put_flag(&mut out, ocf_flag);
    layout::CountLength::<N>::put_u8(&mut out, count.len.get());
    let count_octets = count.value.to_be_bytes();
    let skip = 8_usize.saturating_sub(count.len.octets());
    let [_, _, _, _, _, _, _, tail @ ..] = &mut out;
    for (d, s) in tail.iter_mut().zip(count_octets.iter().skip(skip)) {
        *d = *s;
    }
    out
}

/// The longest non-truncated Primary Header: seven octets and a VCF Count of seven.
const MAX_HEADER_LEN: usize = 14;

/// The fields of the Primary Header by bit position in a header of `N` octets (732.1 §4.1.2,
/// Figure 4-2; GEN-051).
mod layout {
    use aldis_core::bits::Field;

    pub(super) type Version<const N: usize> = Field<N, 0, 4>;
    pub(super) type Scid<const N: usize> = Field<N, 4, 16>;
    pub(super) type SourceOrDestination<const N: usize> = Field<N, 20, 1>;
    pub(super) type Vcid<const N: usize> = Field<N, 21, 6>;
    pub(super) type Map<const N: usize> = Field<N, 27, 4>;
    pub(super) type EndOfPrimaryHeader<const N: usize> = Field<N, 31, 1>;
    pub(super) type FrameLength<const N: usize> = Field<N, 32, 16>;
    pub(super) type Bypass<const N: usize> = Field<N, 48, 1>;
    pub(super) type ProtocolControl<const N: usize> = Field<N, 49, 1>;
    pub(super) type Spare<const N: usize> = Field<N, 50, 2>;
    pub(super) type OcfFlag<const N: usize> = Field<N, 52, 1>;
    pub(super) type CountLength<const N: usize> = Field<N, 53, 3>;
}

/// `InvalidField` naming `field`.
pub(crate) const fn invalid(field: FieldId) -> Error {
    Error::new(ErrorKind::InvalidField).with_field(field)
}

/// `UnknownChannel` naming `field`.
pub(crate) const fn unknown(field: FieldId) -> Error {
    Error::new(ErrorKind::UnknownChannel).with_field(field)
}

/// `LengthMismatch` naming `field`.
pub(crate) const fn mismatch(field: FieldId) -> Error {
    Error::new(ErrorKind::LengthMismatch).with_field(field)
}

/// `usize` → `u64` for error details; saturates on (hypothetical) wider targets.
pub(crate) fn len_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{
        ChannelFields, FrameKind, OID_VCID, RawRest, RawStart, SourceOrDestination, UslpMapId,
        UslpPrimaryHeader, UslpSpacecraftId, UslpVcId, VcfCount, VcfCountLen, primary_octets,
        read_vcf_count, start_octets,
    };
    use crate::checked::Reason;
    use crate::field;
    use aldis_core::{CopFrameKind, ErrorDetail, ErrorKind};

    #[test]
    fn frame_kind_converts_to_and_from_the_cop_kind() {
        // req: USLP-001, COP-010
        for kind in [
            FrameKind::SequenceControlled,
            FrameKind::Expedited,
            FrameKind::ProtocolControl,
        ] {
            let cop = CopFrameKind::from(kind);
            assert_eq!(FrameKind::from(cop), kind);
            assert_eq!(cop.bypass_flag(), kind.bypass_flag());
            assert_eq!(cop.control_command_flag(), kind.protocol_control_flag());
        }
    }

    fn channel() -> ChannelFields {
        ChannelFields {
            spacecraft_id: UslpSpacecraftId::new(0xF0A5).unwrap(),
            source_or_destination: SourceOrDestination::Destination,
            vc_id: UslpVcId::new(0x2A).unwrap(),
            map_id: UslpMapId::new(0xB).unwrap(),
        }
    }

    #[test]
    fn frame_kinds_follow_table_4_1() {
        // req: USLP-001
        assert_eq!(
            FrameKind::from_flags(false, false),
            Some(FrameKind::SequenceControlled)
        );
        assert_eq!(
            FrameKind::from_flags(true, false),
            Some(FrameKind::Expedited)
        );
        assert_eq!(
            FrameKind::from_flags(true, true),
            Some(FrameKind::ProtocolControl)
        );
        assert_eq!(FrameKind::from_flags(false, true), None);
        for k in [
            FrameKind::SequenceControlled,
            FrameKind::Expedited,
            FrameKind::ProtocolControl,
        ] {
            assert_eq!(
                FrameKind::from_flags(k.bypass_flag(), k.protocol_control_flag()),
                Some(k)
            );
        }
        assert!(SourceOrDestination::from_bit(true).bit());
        assert_eq!(
            SourceOrDestination::from_bit(false),
            SourceOrDestination::Source
        );
        assert_eq!(OID_VCID, 63);
    }

    #[test]
    fn vcf_count_lengths() {
        // req: USLP-004
        assert_eq!(VcfCountLen::new(0).unwrap().max_count(), 0);
        assert_eq!(VcfCountLen::new(1).unwrap().max_count(), 255);
        assert_eq!(VcfCountLen::new(7).unwrap().max_count(), (1 << 56) - 1);
        let e = VcfCountLen::new(8).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidConfig, Some(field::VCF_COUNT_LEN))
        );
        let two = VcfCountLen::new(2).unwrap();
        assert_eq!((two.get(), two.octets()), (2, 2));
        assert!(VcfCount::new(two, 0x1_0000).is_none());
        let c = VcfCount::new(two, 0xFFFF).unwrap();
        assert_eq!((c.len(), c.value()), (two, 0xFFFF));
        assert_eq!(VcfCountLen::NONE, VcfCountLen::default());
    }

    #[test]
    fn header_octets_round_trip() {
        // req: USLP-001, USLP-003, USLP-004, GEN-053, API-014
        let len = VcfCountLen::new(3).unwrap();
        let count = VcfCount::new(len, 0x01_02_03).unwrap();
        let o = primary_octets(0b1100, channel(), 300, FrameKind::Expedited, true, count);
        // 1100 | F0A5 | 1 | 101010 | 1011 | 0 ; 012B ; 1 0 00 1 011 ; 01 02 03
        assert_eq!(
            o[..10],
            [0xCF, 0x0A, 0x5D, 0x56, 0x01, 0x2B, 0x8B, 0x01, 0x02, 0x03]
        );
        assert_eq!(o[10..], [0; 4]);
        let start = RawStart::parse(&o).unwrap();
        assert_eq!(
            (start.version, start.channel, start.truncated),
            (12, channel(), false)
        );
        assert!(start.check_version().is_ok());
        let rest = RawRest::parse(&o).unwrap();
        assert_eq!(
            (rest.frame_len, rest.bypass, rest.protocol_control),
            (300, true, false)
        );
        assert_eq!(
            (rest.spare, rest.ocf_flag, rest.vcf_count_len, rest.kind()),
            (0, true, len, Ok(FrameKind::Expedited))
        );
        assert_eq!(read_vcf_count(&o, len).unwrap(), count);
        // req: GEN-020, USLP-004
        let short = read_vcf_count(&o[..8], len).unwrap_err();
        assert_eq!(short.reason(), Reason::LengthMismatch);
        assert_eq!(
            (short.error().kind(), short.error().detail()),
            (ErrorKind::Incomplete, ErrorDetail::Needed(10))
        );
        assert_eq!(
            read_vcf_count(&o[..7], VcfCountLen::NONE).map(VcfCount::value),
            Ok(0)
        );
        let h = UslpPrimaryHeader::new(channel(), 300, FrameKind::Expedited, true, count);
        assert_eq!(h.octets(), o);
        assert_eq!((h.len(), h.is_empty(), h.frame_len()), (10, false, 300));
        assert_eq!(
            (h.spacecraft_id(), h.vc_id(), h.map_id()),
            (channel().spacecraft_id, channel().vc_id, channel().map_id)
        );
        assert_eq!(
            (h.source_or_destination(), h.kind(), h.ocf_flag()),
            (SourceOrDestination::Destination, FrameKind::Expedited, true)
        );
        assert_eq!((h.channel(), h.vcf_count()), (channel(), count));
        assert!(RawStart::parse(&o[..3]).is_none() && RawRest::parse(&o[..6]).is_none());
    }

    #[test]
    fn truncated_start_and_checks() {
        // req: USLP-001, USLP-002
        let s = start_octets(0b1100, channel(), true);
        let start = RawStart::parse(&s).unwrap();
        assert!(start.truncated);
        let bad = RawStart::parse(&start_octets(0b0011, channel(), false)).unwrap();
        let e = bad.check_version().unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::UnsupportedVersion,
                Some(field::FRAME_VERSION),
                ErrorDetail::Expected {
                    expected: 12,
                    found: 3
                }
            )
        );
        let reserved = RawRest::parse(&[0xC0, 0, 0, 0, 0, 9, 0x70]).unwrap();
        assert_eq!(reserved.spare, 0b11);
        let e = reserved.kind().unwrap_err();
        assert_eq!(e.field(), Some(field::PROTOCOL_CONTROL_COMMAND_FLAG));
    }
}
