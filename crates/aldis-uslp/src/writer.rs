//! Encoders: [`encode`], [`UslpFrameWriter`], [`encode_truncated`], [`encode_oid`], and
//! [`encode_oid_galois`].
//!
//! 732.1 §4.1, §6.3, Annex D; USLP-009, USLP-010, USLP-016, API-020…API-023, API-072, GEN-014,
//! GEN-042, GEN-052.
//!
//! The writers compute the Frame Length, the OCF Flag, the VCF Count Length (from the VC and
//! the quality of service of the Bypass/Sequence Control Flag), the TFDF Header, and the FECF, and take the
//! TFDZ, the Insert Zone, and the OCF as given (USLP-010): completing a partly filled
//! fixed-length TFDZ with an Encapsulation Idle Packet is the packet generator's (LNK-013;
//! OI-34). Their checks and errors are those of `docs/standards/uslp-frame.md` §3.3, in its
//! order.

use aldis_core::crc;
use aldis_core::layout::{FrameLayout, LayoutSpec, OCF_LEN};
use aldis_core::ocf::Ocf;
use aldis_core::oid::{OidGenerator, OidGeneratorGalois};
use aldis_core::security::SecurityLayout;
use aldis_core::{Error, ErrorKind, Result};

use crate::config::{
    OcfUse, PhysicalChannelConfig, VcConfig, VcLookup, frame_layout, frame_security,
};
use crate::field;
use crate::header::{
    ChannelFields, FrameKind, OID_VCID, PRIMARY_HEADER_LEN, TRUNCATED_HEADER_LEN,
    UNFINISHED_VERSION, UslpMapId, UslpSpacecraftId, UslpVcId, VERSION, VcfCount, invalid, len_u64,
    mismatch, primary_octets, start_octets, unknown,
};
use crate::tfdf::{LastValidOctet, TfdfHeader, TfdzRule};
use crate::truncated::{truncated_security, wrong_tfdz_len};
use crate::upid::Upid;

/// The Primary Header fields a sender chooses (732.1 §4.1.2). The TFVN, the Frame Length, the
/// Reserve Spares, the OCF Flag, and the VCF Count Length are derived (API-021, GEN-052).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameFields {
    /// Spacecraft Identifier (§4.1.2.2.3).
    pub spacecraft_id: UslpSpacecraftId,
    /// Source-or-Destination Identifier (§4.1.2.3).
    pub source_or_destination: crate::header::SourceOrDestination,
    /// Virtual Channel Identifier (§4.1.2.4).
    pub vc_id: UslpVcId,
    /// MAP Identifier (§4.1.2.5).
    pub map_id: UslpMapId,
    /// The Bypass/Sequence Control and Protocol Control Command Flags (Table 4-1).
    pub kind: FrameKind,
    /// The VCF Count, written in the VC's VCF Count Length for the quality of service (§4.1.2.12; USLP-004).
    pub vcf_count: u64,
}

impl FrameFields {
    /// The fields the truncated header shares (§4.1.2.2–5).
    const fn channel(&self) -> ChannelFields {
        ChannelFields {
            spacecraft_id: self.spacecraft_id,
            source_or_destination: self.source_or_destination,
            vc_id: self.vc_id,
            map_id: self.map_id,
        }
    }
}

/// What a non-truncated frame carries besides its TFDZ (732.1 §4.1.3–5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameContent<'a> {
    /// The TFDF Header: rule, pointer, UPID (§4.1.4.2).
    pub tfdf: TfdfHeader,
    /// The Insert Zone, as long as the Physical Channel's (empty without one; §4.1.3).
    pub insert_zone: &'a [u8],
    /// The OCF, given exactly when the VC's frames carry one (§4.1.5).
    pub ocf: Option<Ocf>,
}

/// Where the fields of a non-truncated frame go, checked against the channels (§4.1, §6.3).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Plan {
    channel: ChannelFields,
    kind: FrameKind,
    count: VcfCount,
    tfdf: TfdfHeader,
    ocf: Option<Ocf>,
    fixed: bool,
    /// The frame with `tfdz_len` TFDZ octets: its length is the reserved frame length.
    layout: FrameLayout,
    tfdz_len: usize,
}

/// Which non-truncated frame a plan is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
    /// A frame of user data or protocol control commands, with `encode` (pointer checked) or
    /// `begin` (pointer checked at `finish`).
    Data { check_pointer: bool },
    /// An OID frame (USLP-009).
    Oid,
}

/// The fixed fields of an OID frame's TFDF Header for a TFDZ of `tfdz_len` octets (§4.1.4.1.9;
/// USLP-009).
fn oid_tfdf(tfdz_len: usize) -> TfdfHeader {
    let last = u16::try_from(tfdz_len.saturating_sub(1)).unwrap_or(u16::MAX);
    TfdfHeader {
        rule: TfdzRule::SduStart(LastValidOctet::from_raw(last)),
        upid: Upid::OnlyIdleData,
    }
}

/// The VC of `fields`, checked: the SCID and VCID configured, and VCID 63 exactly for OID
/// frames (§3.3 rows 1–2).
fn find_vc<L: VcLookup + ?Sized>(
    channels: &L,
    channel: ChannelFields,
    oid: bool,
) -> Result<VcConfig> {
    if !channels.has_spacecraft(channel.spacecraft_id) {
        return Err(unknown(field::SPACECRAFT_ID));
    }
    let vc = channels
        .vc(channel.spacecraft_id, channel.vc_id)
        .ok_or(unknown(field::VC_ID))?;
    if (channel.vc_id.get() == OID_VCID) != oid {
        return Err(invalid(field::VC_ID));
    }
    Ok(vc)
}

/// The layout of a frame of `tfdz_len` TFDZ octets after the TFDF Header of the VC, and that
/// number of octets: all that the channel leaves in a frame of `base.frame_len` octets when
/// `None`, as for an OID frame (§4.1.4.1.10). `base` is the layout of the longest frame.
///
/// # Errors
/// `InvalidField` (`frame_data_field`) for an empty TFDZ, and for a channel whose longest frame
/// leaves no TFDZ octet, whatever `tfdz_len` is (`check_room` rejects that channel when its table
/// is built, so only a `VcLookup` that skips the check reaches it); `LengthMismatch` (`frame_data_field`)
/// for a TFDZ that does not fill a fixed-length frame; `OutOfRange` (`frame_len`) above a
/// variable-length channel's maximum.
fn reserve(
    physical: &PhysicalChannelConfig,
    vc: &VcConfig,
    base: LayoutSpec,
    tfdz_len: Option<usize>,
) -> Result<(FrameLayout, usize)> {
    // The longest frame of the channel shows what surrounds the TFDZ.
    frame_layout(&base, vc.fixed_tfdz())
        .ok_or(invalid(field::FRAME_DATA_FIELD))
        .and_then(|longest| {
            let overhead = physical
                .frame_len()
                .saturating_sub(longest.data().len())
                .saturating_add(crate::tfdf::header_len(vc.fixed_tfdz()));
            let tfdz_len =
                tfdz_len.unwrap_or_else(|| physical.frame_len().saturating_sub(overhead));
            if tfdz_len == 0 {
                return Err(invalid(field::FRAME_DATA_FIELD));
            }
            let total = overhead.saturating_add(tfdz_len);
            if physical.is_fixed() && total != physical.frame_len() {
                return Err(mismatch(field::FRAME_DATA_FIELD));
            }
            if total > physical.frame_len() {
                return Err(Error::out_of_range(
                    field::FRAME_LEN,
                    len_u64(physical.frame_len()),
                    len_u64(total),
                ));
            }
            let spec = LayoutSpec {
                frame_len: total,
                ..base
            };
            frame_layout(&spec, vc.fixed_tfdz())
                .map(|layout| (layout, tfdz_len))
                .ok_or(invalid(field::FRAME_DATA_FIELD))
        })
}

impl Plan {
    /// Checks a frame of `tfdz_len` TFDZ octets in the order of
    /// `docs/standards/uslp-frame.md` §3.3 (USLP-009, USLP-010, USLP-016).
    fn new<L: VcLookup + ?Sized>(
        channels: &L,
        fields: FrameFields,
        content: FrameContent<'_>,
        tfdz_len: Option<usize>,
        purpose: Purpose,
    ) -> Result<Plan> {
        let channel = fields.channel();
        let oid = matches!(purpose, Purpose::Oid);
        let vc = find_vc(channels, channel, oid)?;
        if oid && fields.map_id.get() != 0 {
            return Err(invalid(field::MAP_ID));
        }
        if oid && fields.kind.protocol_control_flag() {
            return Err(invalid(field::PROTOCOL_CONTROL_COMMAND_FLAG));
        }
        let map = if oid {
            None
        } else {
            let m = channels
                .map(fields.spacecraft_id, fields.vc_id, fields.map_id)
                .ok_or(unknown(field::MAP_ID))?;
            Some(m)
        };
        if vc.truncated_frame_len().is_some() {
            return Err(invalid(field::END_OF_FRAME_PRIMARY_HEADER_FLAG));
        }
        if content.tfdf.rule.rule().is_fixed_length() != vc.fixed_tfdz() {
            return Err(invalid(field::TFDZ_CONSTRUCTION_RULE));
        }
        let count_len = vc.vcf_count_len(fields.kind.bypass_flag());
        let count = VcfCount::new(count_len, fields.vcf_count).ok_or(Error::out_of_range(
            field::VCF_COUNT,
            count_len.max_count(),
            fields.vcf_count,
        ))?;
        let ocf_ok = match vc.ocf() {
            OcfUse::Absent => content.ocf.is_none(),
            OcfUse::Allowed => true,
            OcfUse::Required => content.ocf.is_some(),
        };
        if !ocf_ok {
            return Err(invalid(field::OCF_FLAG));
        }
        let physical = channels.physical_channel();
        if content.insert_zone.len() != physical.insert_zone_len() {
            return Err(mismatch(field::INSERT_ZONE_LEN));
        }
        let security = if fields.kind.protocol_control_flag() {
            SecurityLayout::NONE
        } else {
            frame_security(&vc, map.as_ref())
        };
        let base = LayoutSpec {
            frame_len: physical.frame_len(),
            primary_header_len: PRIMARY_HEADER_LEN.saturating_add(count_len.octets()),
            secondary_len: physical.insert_zone_len(),
            security,
            ocf: content.ocf.is_some(),
            fecf: physical.fecf(),
        };
        let (layout, tfdz_len) = reserve(&physical, &vc, base, tfdz_len)?;
        let tfdf = if oid {
            oid_tfdf(tfdz_len)
        } else {
            content.tfdf
        };
        if matches!(
            purpose,
            Purpose::Data {
                check_pointer: true
            }
        ) {
            tfdf.check_pointer(tfdz_len)?;
        }
        Ok(Plan {
            channel,
            kind: fields.kind,
            count,
            tfdf,
            ocf: content.ocf,
            fixed: physical.is_fixed(),
            layout,
            tfdz_len,
        })
    }

    /// Offset of the TFDZ.
    fn tfdz_start(&self) -> usize {
        self.layout.data().start.saturating_add(self.tfdf.len())
    }
}

/// Encodes a non-truncated frame into the start of `out`; returns its length (732.1 §4.1,
/// §6.3; USLP-010, USLP-016, API-020, API-021).
///
/// The Security Header and Trailer, when the VC or MAP has SDLS, are written as zeros for the
/// SDLS layer to fill (API-072); [`UslpFrameWriter`] exposes them in place.
///
/// # Errors
/// Those of `docs/standards/uslp-frame.md` §3.3, in its order: `UnknownChannel`
/// (`spacecraft_id`, `vc_id`, `map_id`); `InvalidField` (`vc_id`) for VCID 63, whose frames
/// [`encode_oid`] writes; `InvalidField` (`end_of_frame_primary_header_flag`) for a VC with a
/// truncated frame length, which carries only truncated frames ([`encode_truncated`]; OI-48);
/// `InvalidField` (`tfdz_construction_rule`) for a rule of the other TFDZ
/// type (USLP-016); `OutOfRange` (`vcf_count`); `InvalidField` (`ocf_flag`); `LengthMismatch`
/// (`insert_zone_len`); `InvalidField` (`frame_data_field`) for an empty TFDZ;
/// `LengthMismatch` (`frame_data_field`) for a TFDZ that does not fill a fixed-length frame;
/// `OutOfRange` (`frame_len`) above a variable-length channel's maximum; `OutOfRange`
/// (`fhp_lvo_pointer`); `BufferTooSmall`. On error `out` is unchanged (API-022).
pub fn encode<L: VcLookup + ?Sized>(
    channels: &L,
    fields: FrameFields,
    content: FrameContent<'_>,
    tfdz: &[u8],
    out: &mut [u8],
) -> Result<usize> {
    let purpose = Purpose::Data {
        check_pointer: true,
    };
    let plan = Plan::new(channels, fields, content, Some(tfdz.len()), purpose)?;
    let mut w = UslpFrameWriter::start(out, plan, content.insert_zone, true)?;
    for (d, s) in w.tfdz_mut().iter_mut().zip(tfdz) {
        *d = *s;
    }
    w.finish(tfdz.len())
}

/// Encodes an OID frame into the start of `out` and fills its TFDZ with the next octets of
/// `oid`; returns its length (732.1 §4.1.4.1.5–12; USLP-009, CMN-020, CMN-021).
///
/// `fields.vc_id` must be 63 and `fields.map_id` 0; the rule is `001`, the UPID "Only Idle
/// Data", and the Last Valid Octet Pointer the last TFDZ octet. The TFDZ fills the
/// fixed-length frame. VCID 63 exists only on a fixed-length Physical Channel
/// ([`VcTable::new`](crate::config::VcTable::new)), where it is valid without an entry (Table
/// 5-2 note 2): the frame then has no VCF Count, OCF, or Security fields.
///
/// # Errors
/// The errors of [`encode`], with `InvalidField` (`vc_id`) for another VCID, `InvalidField`
/// (`map_id`) for a MAP ID other than 0, and `InvalidField` (`protocol_control_command_flag`)
/// for protocol control flags. On error neither `out` nor `oid` is changed (GEN-042).
pub fn encode_oid<L: VcLookup + ?Sized>(
    channels: &L,
    fields: FrameFields,
    insert_zone: &[u8],
    ocf: Option<Ocf>,
    oid: &mut OidGenerator,
    out: &mut [u8],
) -> Result<usize> {
    oid_frame(channels, fields, insert_zone, ocf, out, |tfdz| {
        oid.fill(tfdz);
    })
}

/// Encodes an OID frame as [`encode_oid`] does, with the OID generator in its Galois form
/// (732.1 §4.1.4.1.10; USLP-009, CMN-022).
///
/// The two forms give the same sequence, so the frames are the same. For a caller that keeps
/// the Galois form, such as the C ABI's `aldis_uslp_encode_oid` with a generator initialized
/// as `ALDIS_OID_GALOIS`.
///
/// # Errors
/// As for [`encode_oid`]. On error neither `out` nor `oid` is changed (GEN-042).
pub fn encode_oid_galois<L: VcLookup + ?Sized>(
    channels: &L,
    fields: FrameFields,
    insert_zone: &[u8],
    ocf: Option<Ocf>,
    oid: &mut OidGeneratorGalois,
    out: &mut [u8],
) -> Result<usize> {
    oid_frame(channels, fields, insert_zone, ocf, out, |tfdz| {
        oid.fill(tfdz);
    })
}

/// The OID frame of [`encode_oid`], its TFDZ filled by `fill` once every check has passed, so
/// that a rejected frame leaves the generator unchanged (USLP-009, GEN-042).
fn oid_frame<L: VcLookup + ?Sized>(
    channels: &L,
    fields: FrameFields,
    insert_zone: &[u8],
    ocf: Option<Ocf>,
    out: &mut [u8],
    fill: impl FnOnce(&mut [u8]),
) -> Result<usize> {
    let content = FrameContent {
        tfdf: oid_tfdf(1),
        insert_zone,
        ocf,
    };
    let plan = Plan::new(channels, fields, content, None, Purpose::Oid)?;
    let tfdz_len = plan.tfdz_len;
    let mut w = UslpFrameWriter::start(out, plan, insert_zone, true)?;
    fill(w.tfdz_mut());
    w.finish(tfdz_len)
}

/// Encodes a truncated frame into the start of `out`; returns its length, the VC's truncated
/// frame length (732.1 Annex D; USLP-002, USLP-012, USLP-016, API-075).
///
/// With SDLS on the VC or MAP, the 6-octet Security Header and 16-octet Security Trailer are
/// written as zeros for the SDLS layer to fill (Annex D1.2.3).
///
/// # Errors
/// `UnknownChannel` (`spacecraft_id`, `vc_id`); `InvalidField` (`vc_id`) for VCID 63;
/// `UnknownChannel` (`map_id`); `InvalidField` (`end_of_frame_primary_header_flag`) for a VC
/// without a truncated frame length; `InvalidField` (`tfdz_construction_rule`) for a rule of a
/// fixed-length TFDZ (USLP-016); `InvalidField` (`frame_data_field`) for an empty TFDZ;
/// `LengthMismatch` (`frame_data_field`) for a TFDZ that does not fill the frame;
/// `BufferTooSmall`. On error `out` is unchanged (API-022).
pub fn encode_truncated<L: VcLookup + ?Sized>(
    channels: &L,
    channel: ChannelFields,
    header: TfdfHeader,
    tfdz: &[u8],
    out: &mut [u8],
) -> Result<usize> {
    let vc = find_vc(channels, channel, false)?;
    let map = channels
        .map(channel.spacecraft_id, channel.vc_id, channel.map_id)
        .ok_or(unknown(field::MAP_ID))?;
    let len = vc
        .truncated_frame_len()
        .ok_or(invalid(field::END_OF_FRAME_PRIMARY_HEADER_FLAG))?
        .get();
    if header.rule.rule().is_fixed_length() {
        return Err(invalid(field::TFDZ_CONSTRUCTION_RULE));
    }
    if tfdz.is_empty() {
        return Err(invalid(field::FRAME_DATA_FIELD));
    }
    let security = truncated_security(frame_security(&vc, Some(&map)));
    let room = len
        .saturating_sub(TRUNCATED_HEADER_LEN)
        .saturating_sub(security.header_len())
        .saturating_sub(1)
        .saturating_sub(security.trailer_len());
    if tfdz.len() != room {
        return Err(wrong_tfdz_len());
    }
    let frame = out.get_mut(..len).ok_or(Error::buffer_too_small(len))?;
    let start = start_octets(VERSION, channel, true);
    let [first, ..] = header.octets();
    let zeros_h = core::iter::repeat_n(0, security.header_len());
    let zeros_t = core::iter::repeat_n(0, security.trailer_len());
    let octets = start
        .into_iter()
        .chain(zeros_h)
        .chain(core::iter::once(first))
        .chain(tfdz.iter().copied())
        .chain(zeros_t);
    for (d, s) in frame.iter_mut().zip(octets) {
        *d = s;
    }
    Ok(len)
}

/// Two-phase writer of a non-truncated frame (732.1 §4.1, §6.3; API-020…API-023, API-072).
///
/// [`begin`](Self::begin) reserves the frame for up to a maximum TFDZ; the caller fills the
/// TFDZ and, with SDLS, the Security Header and Trailer in place; [`finish`](Self::finish)
/// writes the header, the OCF, and the FECF.
///
/// Until `finish`, the buffer holds a Primary Header whose TFVN is `1111`, which every decoder
/// rejects with `UnsupportedVersion` and the delimiter treats as fill, so an unfinished frame
/// cannot be mistaken for a valid one (API-023). Its Frame Length field holds the reserved
/// frame length, which [`resume`](Self::resume) compares (OI-50).
#[derive(Debug)]
pub struct UslpFrameWriter<'b> {
    out: &'b mut [u8],
    plan: Plan,
}

impl<'b> UslpFrameWriter<'b> {
    /// Reserves a frame with room for up to `max_tfdz_len` TFDZ octets (732.1 §4.1; USLP-010,
    /// API-020). On a fixed-length Physical Channel `max_tfdz_len` must fill the frame.
    ///
    /// # Errors
    /// The errors of [`encode`] for a frame with `max_tfdz_len` TFDZ octets, except the
    /// pointer, which [`finish`](Self::finish) checks against the TFDZ it is given. On error
    /// `out` is unchanged.
    pub fn begin<L: VcLookup + ?Sized>(
        out: &'b mut [u8],
        channels: &L,
        fields: FrameFields,
        content: FrameContent<'_>,
        max_tfdz_len: usize,
    ) -> Result<UslpFrameWriter<'b>> {
        let purpose = Purpose::Data {
            check_pointer: false,
        };
        let plan = Plan::new(channels, fields, content, Some(max_tfdz_len), purpose)?;
        UslpFrameWriter::start(out, plan, content.insert_zone, true)
    }

    /// Takes up a frame begun with [`begin`](Self::begin) and the same arguments, for a writer
    /// that keeps no state between its two phases, such as the C ABI's
    /// `aldis_uslp_writer_finish` (API-023; c-abi.md §11).
    ///
    /// It checks the arguments as `begin` does and writes nothing: the Security Header, the
    /// TFDZ, and the Security Trailer keep what the caller filled in. It compares the unfinished
    /// Primary Header (TFVN `1111`, the channel fields, the flags, the VCF count, and the
    /// reserved frame length in the Frame Length field), the Insert Zone, and the TFDF Header
    /// with what `begin` writes for these arguments (OI-50). The Security Header and Trailer,
    /// the TFDZ, the OCF (written at `finish`), and the FECF are not compared, and a changed
    /// security layout with an identical header is not caught.
    ///
    /// # Errors
    /// As for [`begin`](Self::begin); `InvalidState` if `out` does not start with the unfinished
    /// headers `begin` writes for these arguments (other arguments, or a buffer never begun).
    /// On error `out` is unchanged (API-022, GEN-042).
    pub fn resume<L: VcLookup + ?Sized>(
        out: &'b mut [u8],
        channels: &L,
        fields: FrameFields,
        content: FrameContent<'_>,
        max_tfdz_len: usize,
    ) -> Result<UslpFrameWriter<'b>> {
        let purpose = Purpose::Data {
            check_pointer: false,
        };
        let plan = Plan::new(channels, fields, content, Some(max_tfdz_len), purpose)?;
        UslpFrameWriter::start(out, plan, content.insert_zone, false)
    }

    /// Takes up the frame of a checked plan in `out`. With `write`, as `begin` calls it, it
    /// writes the unfinished header, the Insert Zone, and the TFDF Header, and zeros in the
    /// Security Header and Trailer; without, as `resume` calls it, it writes nothing and fails
    /// with `InvalidState` unless the unfinished header, the Insert Zone, and the TFDF Header
    /// are what `begin` writes for the plan (OI-50; API-023).
    fn start(
        out: &'b mut [u8],
        plan: Plan,
        insert_zone: &[u8],
        write: bool,
    ) -> Result<UslpFrameWriter<'b>> {
        let total = plan.layout.frame_len();
        let buf = out.get_mut(..total).ok_or(Error::buffer_too_small(total))?;
        let header = primary_octets(
            UNFINISHED_VERSION,
            plan.channel,
            total,
            plan.kind,
            plan.ocf.is_some(),
            plan.count,
        );
        let head = header
            .iter()
            .take(plan.layout.primary_header().len())
            .chain(insert_zone)
            .copied();
        let tfdf = plan.tfdf.octets();
        let tfdf_start = plan.tfdz_start().saturating_sub(plan.tfdf.len());
        let tfdf = tfdf.iter().take(plan.tfdf.len()).copied();
        if write {
            for (d, s) in buf.iter_mut().zip(head) {
                *d = s;
            }
            let slot = buf.get_mut(tfdf_start..).unwrap_or_default();
            for (d, s) in slot.iter_mut().zip(tfdf) {
                *d = s;
            }
        } else {
            let slot = buf.iter().skip(tfdf_start);
            if !(buf.iter().zip(head).all(|(d, s)| *d == s) && slot.zip(tfdf).all(|(d, s)| *d == s))
            {
                return Err(Error::new(ErrorKind::InvalidState));
            }
        }
        let mut w = UslpFrameWriter { out, plan };
        if write {
            w.security_header_mut().fill(0);
            w.security_trailer_mut().fill(0);
        }
        Ok(w)
    }

    /// Where the fields of the reserved frame lie in the buffer, with the TFDF (header and
    /// reserved TFDZ) in the data field (CMN-040).
    ///
    /// This is the layout of the longest frame, a provisional one: [`finish`](Self::finish) with
    /// a shorter TFDZ shortens the frame and moves the Security Trailer, the OCF, and the FECF.
    #[must_use]
    pub const fn layout(&self) -> &FrameLayout {
        &self.plan.layout
    }

    /// The Security Header to fill in place; empty without SDLS (§6.3.4; API-072).
    pub fn security_header_mut(&mut self) -> &mut [u8] {
        self.out
            .get_mut(self.plan.layout.security_header())
            .unwrap_or_default()
    }

    /// The TFDZ to fill in place, the reserved maximum (§4.1.4.3; GEN-014).
    pub fn tfdz_mut(&mut self) -> &mut [u8] {
        let start = self.plan.tfdz_start();
        let end = start.saturating_add(self.plan.tfdz_len);
        self.out.get_mut(start..end).unwrap_or_default()
    }

    /// The Security Trailer to fill in place, after the reserved TFDZ; empty without SDLS.
    /// [`finish`](Self::finish) moves it to follow the TFDZ (§6.3.6; API-072).
    pub fn security_trailer_mut(&mut self) -> &mut [u8] {
        self.out
            .get_mut(self.plan.layout.security_trailer())
            .unwrap_or_default()
    }

    /// Completes the frame with `tfdz_len` TFDZ octets; returns the frame length (732.1
    /// §4.1.2.7, §4.1.5, §4.1.6; USLP-010, API-021, API-023).
    ///
    /// The frame is the first `len` octets of the buffer. When `tfdz_len` is below the
    /// reservation, the Security Trailer moves to follow the TFDZ, and the octets after the
    /// frame keep unspecified values, which may include octets of the Security Trailer as it
    /// was before the move; use only `out[..len]`, and clear the rest of the buffer if it may
    /// hold sensitive data.
    ///
    /// # Errors
    /// `OutOfRange` (`user_len`) above the reservation; `InvalidField` (`frame_data_field`) for
    /// 0; `LengthMismatch` (`frame_data_field`) below the reservation on a fixed-length
    /// channel; `OutOfRange` (`fhp_lvo_pointer`) for a pointer not below `tfdz_len`. On error
    /// the buffer keeps its unfinished state.
    pub fn finish(self, tfdz_len: usize) -> Result<usize> {
        let plan = self.plan;
        if tfdz_len > plan.tfdz_len {
            return Err(Error::out_of_range(
                field::USER_LEN,
                len_u64(plan.tfdz_len),
                len_u64(tfdz_len),
            ));
        }
        if tfdz_len == 0 {
            return Err(invalid(field::FRAME_DATA_FIELD));
        }
        if plan.fixed && tfdz_len != plan.tfdz_len {
            return Err(mismatch(field::FRAME_DATA_FIELD));
        }
        plan.tfdf.check_pointer(tfdz_len)?;
        let unused = plan.tfdz_len.saturating_sub(tfdz_len);
        let total = plan.layout.frame_len().saturating_sub(unused);
        let tail_start = plan.tfdz_start().saturating_add(tfdz_len);
        let tail_end = tail_start
            .saturating_add(unused)
            .saturating_add(plan.layout.security_trailer().len());
        let tail = self.out.get_mut(tail_start..tail_end).unwrap_or_default();
        let from = unused.min(tail.len());
        tail.copy_within(from.., 0);
        let frame = self.out.get_mut(..total).unwrap_or_default();
        let header = primary_octets(
            VERSION,
            plan.channel,
            total,
            plan.kind,
            plan.ocf.is_some(),
            plan.count,
        );
        for (d, s) in frame
            .iter_mut()
            .zip(header.iter().take(plan.layout.primary_header().len()))
        {
            *d = *s;
        }
        let fecf_len = plan.layout.fecf().len();
        if let Some(ocf) = plan.ocf {
            let at = total.saturating_sub(fecf_len).saturating_sub(OCF_LEN);
            let slot = frame.get_mut(at..).unwrap_or_default();
            for (d, s) in slot.iter_mut().zip(ocf.to_octets()) {
                *d = s;
            }
        }
        if fecf_len > 0 {
            // The frame holds at least the Primary Header and the FECF, so this cannot fail.
            let _ = crc::write_fecf(frame);
        }
        Ok(total)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    extern crate std;
    use super::{
        FrameContent, FrameFields, UslpFrameWriter, encode, encode_oid, encode_oid_galois,
        encode_truncated,
    };
    use crate::config::tests::{fixed, mapid, plain_variable, scid, variable, vcid};
    use crate::field;
    use crate::header::{ChannelFields, FrameKind, SourceOrDestination};
    use crate::tfdf::tests::offset;
    use crate::tfdf::{FirstHeaderPointer, LastValidOctet, TfdfHeader, TfdzRule};
    use crate::upid::Upid;
    use crate::view::UslpFrame;
    use aldis_core::ocf::Ocf;
    use aldis_core::oid::{OidGenerator, OidGeneratorGalois};
    use aldis_core::{DecodeMode, Error, ErrorKind, FieldId};
    use std::vec::Vec;

    fn fields(vc: u8, map: u8, kind: FrameKind, count: u64) -> FrameFields {
        FrameFields {
            spacecraft_id: scid(0x155),
            source_or_destination: SourceOrDestination::Source,
            vc_id: vcid(vc),
            map_id: mapid(map),
            kind,
            vcf_count: count,
        }
    }

    fn content(rule: TfdzRule, upid: Upid, iz: &[u8], ocf: Option<Ocf>) -> FrameContent<'_> {
        FrameContent {
            tfdf: TfdfHeader { rule, upid },
            insert_zone: iz,
            ocf,
        }
    }

    fn kf(r: Result<usize, Error>) -> (ErrorKind, Option<FieldId>) {
        let e = r.unwrap_err();
        (e.kind(), e.field())
    }

    #[test]
    fn fixed_frames_round_trip() {
        // req: USLP-003, USLP-004, USLP-005, USLP-010, API-020, API-021, GEN-052
        let tbl = fixed();
        let ocf = Some(Ocf::decode([1, 2, 3, 4]));
        let rule = TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::PacketStartsAt(offset(2)));
        let c = content(rule, Upid::Packets, &[0xAA, 0xBB], ocf);
        let f = fields(1, 3, FrameKind::Expedited, 0x1234);
        let tfdz: Vec<u8> = (0..44).collect();
        let mut out = [0_u8; 80];
        let n = encode(&tbl, f, c, &tfdz, &mut out).unwrap();
        assert_eq!(n, 64);
        let dec = UslpFrame::decode(&out[..n], &tbl, DecodeMode::Strict).unwrap();
        let full = dec.full().unwrap();
        assert_eq!(
            (full.tfdz(), full.ocf(), full.insert_zone()),
            (&tfdz[..], ocf, &[0xAA, 0xBB][..])
        );
        assert_eq!(full.vcf_count().value(), 0x1234);
        assert_eq!(full.tfdf_header(), c.tfdf);
        let mut two = [0_u8; 64];
        let mut w = UslpFrameWriter::begin(&mut two, &tbl, f, c, 44).unwrap();
        assert!(w.security_header_mut().is_empty() && w.security_trailer_mut().is_empty());
        w.tfdz_mut().copy_from_slice(&tfdz);
        assert_eq!(w.finish(44), Ok(64));
        assert_eq!(two, out[..64]);
    }

    #[test]
    fn the_writer_reports_the_layout_of_the_reserved_frame() {
        // req: CMN-040, API-020
        let tbl = fixed();
        let ocf = Some(Ocf::decode([1, 2, 3, 4]));
        let rule = TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::PacketStartsAt(offset(2)));
        let c = content(rule, Upid::Packets, &[0xAA, 0xBB], ocf);
        let f = fields(1, 3, FrameKind::Expedited, 1);
        let mut out = [0_u8; 64];
        let w = UslpFrameWriter::begin(&mut out, &tbl, f, c, 44).unwrap();
        assert_eq!(
            w.layout().ranges(),
            [0..9, 9..11, 11..11, 11..58, 58..58, 58..62, 62..64]
        );
    }

    #[test]
    fn frames_without_fecf() {
        // req: USLP-010, API-020, API-021
        use crate::config::{
            MapConfig, OcfUse, PhysicalChannelConfig, TransferFrameType, VcConfig, VcParams,
            VcTable,
        };
        use aldis_core::security::SecurityLayout;
        let pc = PhysicalChannelConfig::new(TransferFrameType::Variable, 64, 0, false).unwrap();
        let params = VcParams {
            ocf: OcfUse::Allowed,
            ..VcParams::new(TransferFrameType::Variable)
        };
        let tbl: VcTable<4, 4> = VcTable::new(
            pc,
            &[VcConfig::new(scid(0x155), vcid(4), params).unwrap()],
            &[MapConfig::new(
                scid(0x155),
                vcid(4),
                mapid(0),
                SecurityLayout::NONE,
            )],
        )
        .unwrap();
        let f = fields(4, 0, FrameKind::Expedited, 0);
        for ocf in [None, Some(Ocf::decode([5, 6, 7, 8]))] {
            let c = content(TfdzRule::OctetStream, Upid::OctetStream, &[], ocf);
            let mut buf = [0_u8; 32];
            let mut writer = UslpFrameWriter::begin(&mut buf, &tbl, f, c, 4).unwrap();
            writer.tfdz_mut()[..2].copy_from_slice(&[1, 2]);
            let len = writer.finish(2).unwrap();
            assert_eq!(len, 10 + if ocf.is_some() { 4 } else { 0 });
            let dec = UslpFrame::decode(&buf[..len], &tbl, DecodeMode::Strict).unwrap();
            let full = dec.full().unwrap();
            assert_eq!(
                (full.tfdz(), full.ocf(), full.fecf()),
                (&[1, 2][..], ocf, None)
            );
        }
    }

    #[test]
    fn variable_frames_with_sdls_shrink() {
        // req: USLP-010, API-022, API-023, API-072
        let tbl = plain_variable();
        let c = content(TfdzRule::NoSegmentation, Upid::OctetStream, &[], None);
        let f = fields(5, 1, FrameKind::SequenceControlled, 0);
        let mut buf = [0_u8; 64];
        let mut w = UslpFrameWriter::begin(&mut buf, &tbl, f, c, 10).unwrap();
        w.security_header_mut().fill(0x51);
        w.security_trailer_mut().fill(0x52);
        w.tfdz_mut()[..3].copy_from_slice(&[7, 8, 9]);
        let len = w.finish(3).unwrap();
        let dec = UslpFrame::decode(&buf[..len], &tbl, DecodeMode::Strict).unwrap();
        assert_eq!(
            (dec.tfdz(), dec.security_header()),
            (&[7, 8, 9][..], &[0x51; 6][..])
        );
        assert_eq!(dec.security_trailer(), [0x52; 16]);
        let mut one = [0_u8; 64];
        assert_eq!(encode(&tbl, f, c, &[7, 8, 9], &mut one), Ok(len));
        assert_eq!(one[len - 18..len - 2], [0; 16]);
        // An unfinished frame is rejected (API-023).
        let mut raw = [0_u8; 64];
        {
            let _unfinished = UslpFrameWriter::begin(&mut raw, &tbl, f, c, 10).unwrap();
        }
        let e = UslpFrame::decode(&raw, &tbl, DecodeMode::Lenient).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::UnsupportedVersion);
        let w = UslpFrameWriter::begin(&mut raw, &tbl, f, c, 10).unwrap();
        assert_eq!(
            kf(w.finish(11)),
            (ErrorKind::OutOfRange, Some(field::USER_LEN))
        );
        let w = UslpFrameWriter::begin(&mut raw, &tbl, f, c, 10).unwrap();
        assert_eq!(
            kf(w.finish(0)),
            (ErrorKind::InvalidField, Some(field::FRAME_DATA_FIELD))
        );
        let unknown = FrameFields {
            vc_id: vcid(9),
            ..f
        };
        let e = UslpFrameWriter::begin(&mut raw, &tbl, unknown, c, 10).unwrap_err();
        assert_eq!(e.field(), Some(field::VC_ID));
        let mut small = [0_u8; 20];
        assert_eq!(
            kf(encode(&tbl, f, c, &[1; 10], &mut small)),
            (ErrorKind::BufferTooSmall, None)
        );
        assert_eq!(small, [0; 20]);
    }

    #[test]
    fn resume_keeps_what_the_caller_filled() {
        // req: API-022, API-023, API-072, USLP-010
        let tbl = plain_variable();
        let c = content(TfdzRule::NoSegmentation, Upid::OctetStream, &[], None);
        let f = fields(5, 1, FrameKind::SequenceControlled, 0);
        let mut one = [0_u8; 64];
        let mut w = UslpFrameWriter::begin(&mut one, &tbl, f, c, 10).unwrap();
        w.security_header_mut().fill(0x51);
        w.security_trailer_mut().fill(0x52);
        w.tfdz_mut()[..3].copy_from_slice(&[7, 8, 9]);
        let len = w.finish(3).unwrap();
        // A stateless finish: `begin`, fill, then `resume` with the same arguments and `finish`.
        let mut two = [0xEE_u8; 64];
        let mut w = UslpFrameWriter::begin(&mut two, &tbl, f, c, 10).unwrap();
        w.security_header_mut().fill(0x51);
        w.security_trailer_mut().fill(0x52);
        w.tfdz_mut()[..3].copy_from_slice(&[7, 8, 9]);
        let w = UslpFrameWriter::resume(&mut two, &tbl, f, c, 10).unwrap();
        assert_eq!(w.finish(3), Ok(len));
        assert_eq!(two[..len], one[..len]);
        // `resume` writes no octet: the unfinished header, the Insert Zone, and the TFDF Header
        // are the ones `begin` wrote.
        let fx = fixed();
        let pkt = TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::NoPacketStartOrEnd);
        let iz = content(pkt, Upid::Packets, &[0xAA, 0xBB], None);
        let other = fields(2, 1, FrameKind::Expedited, 0);
        let mut three = [0xEE_u8; 64];
        assert!(UslpFrameWriter::begin(&mut three, &fx, other, iz, 44).is_ok());
        three[9..13].fill(0x77);
        three[16..].fill(0x66);
        let before = three;
        assert!(UslpFrameWriter::resume(&mut three, &fx, other, iz, 44).is_ok());
        assert_eq!(three, before);
        assert_eq!(three[7..9], [0xAA, 0xBB]);
        assert_eq!(three[13..16], [0, 0xFF, 0xFF], "TFDF Header");
        assert_eq!(three[0] >> 4, 0b1111, "unfinished version");
        let w = UslpFrameWriter::resume(&mut three, &fx, other, iz, 44).unwrap();
        assert_eq!(w.finish(44), Ok(64));
        // The errors of `begin`, with `out` unchanged.
        let mut four = [0xEE_u8; 64];
        let unknown = FrameFields {
            vc_id: vcid(9),
            ..f
        };
        let e = UslpFrameWriter::resume(&mut four, &tbl, unknown, c, 10).unwrap_err();
        assert_eq!(e.field(), Some(field::VC_ID));
        let e = UslpFrameWriter::resume(&mut four[..20], &tbl, f, c, 10).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        assert_eq!(four, [0xEE; 64]);
    }

    #[test]
    fn resume_rejects_another_frame_than_begun() {
        // req: API-023, USLP-010, CABI-022 (OI-50)
        let tbl = plain_variable();
        let c = content(TfdzRule::NoSegmentation, Upid::OctetStream, &[], None);
        let f = fields(5, 1, FrameKind::SequenceControlled, 0);
        let mut out = [0xEE_u8; 64];
        assert!(UslpFrameWriter::begin(&mut out, &tbl, f, c, 10).is_ok());
        let begun = out;
        let other_c = content(TfdzRule::NoSegmentation, Upid::Packets, &[], None);
        let cases = [
            (fields(5, 1, FrameKind::Expedited, 0), c, 10),
            // Without SDLS, where `begin` had SDLS 6/16 on the MAP.
            (fields(4, 0, FrameKind::SequenceControlled, 0), c, 10),
            (f, other_c, 10),
            (f, c, 9),
            (f, c, 11),
        ];
        for (f, c, max) in cases {
            let r = UslpFrameWriter::resume(&mut out, &tbl, f, c, max);
            let e = r.unwrap_err();
            assert_eq!(e.kind(), ErrorKind::InvalidState, "{f:?} {c:?} {max}");
            assert_eq!(out, begun, "the buffer is unchanged");
        }
        // The Primary Header is compared in every octet and the TFDF Header as well.
        for at in [0, 1, 2, 3, 4, 5, 6, 13] {
            out[at] ^= 0x01;
            let e = UslpFrameWriter::resume(&mut out, &tbl, f, c, 10).unwrap_err();
            assert_eq!(e.kind(), ErrorKind::InvalidState, "octet {at}");
            out[at] ^= 0x01;
        }
        // Another MAP of the same VC.
        let m0 = fields(4, 0, FrameKind::SequenceControlled, 0);
        assert!(UslpFrameWriter::begin(&mut out, &tbl, m0, c, 10).is_ok());
        let m2 = fields(4, 2, FrameKind::SequenceControlled, 0);
        let e = UslpFrameWriter::resume(&mut out, &tbl, m2, c, 10).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidState);
        let x0 = fields(4, 0, FrameKind::Expedited, 0);
        assert!(UslpFrameWriter::begin(&mut out, &tbl, x0, c, 10).is_ok());
        let x1 = fields(4, 0, FrameKind::Expedited, 1);
        let e = UslpFrameWriter::resume(&mut out, &tbl, x1, c, 10).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidState, "another VCF count");
        assert!(UslpFrameWriter::begin(&mut out, &tbl, f, c, 10).is_ok());
        // An Insert Zone that differs.
        let fx = fixed();
        let pkt = TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::NoPacketStartOrEnd);
        let iz = content(pkt, Upid::Packets, &[0xAA, 0xBB], None);
        let other = fields(2, 1, FrameKind::Expedited, 0);
        let mut fixed_out = [0xEE_u8; 64];
        assert!(UslpFrameWriter::begin(&mut fixed_out, &fx, other, iz, 44).is_ok());
        let iz2 = content(pkt, Upid::Packets, &[0xAA, 0xBC], None);
        let e = UslpFrameWriter::resume(&mut fixed_out, &fx, other, iz2, 44).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidState);
        // What the caller fills is not compared.
        out[7..13].fill(0x5A);
        out[14..].fill(0x5A);
        assert!(UslpFrameWriter::resume(&mut out, &tbl, f, c, 10).is_ok());
    }

    #[test]
    fn resume_rejects_a_buffer_never_begun() {
        // req: API-023, CABI-022 (OI-50)
        let tbl = plain_variable();
        let c = content(TfdzRule::NoSegmentation, Upid::OctetStream, &[], None);
        let f = fields(5, 1, FrameKind::SequenceControlled, 0);
        for fill in [0x00_u8, 0xA5] {
            let mut out = [fill; 64];
            let e = UslpFrameWriter::resume(&mut out, &tbl, f, c, 10).unwrap_err();
            assert_eq!(e.kind(), ErrorKind::InvalidState);
            assert_eq!(out, [fill; 64]);
        }
        // A finished frame is not an unfinished one.
        let mut out = [0_u8; 64];
        let w = UslpFrameWriter::begin(&mut out, &tbl, f, c, 10).unwrap();
        w.finish(3).unwrap();
        let e = UslpFrameWriter::resume(&mut out, &tbl, f, c, 10).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidState);
    }

    #[test]
    fn writer_errors_in_order() {
        // req: USLP-009, USLP-010, USLP-016, API-001, API-022
        let tbl = fixed();
        let ocf = Some(Ocf::decode([0; 4]));
        let pkt = TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::NoPacketStartOrEnd);
        let ok = content(pkt, Upid::Packets, &[0, 0], ocf);
        let f = fields(1, 0, FrameKind::SequenceControlled, 0);
        let z = [0_u8; 45];
        let mut out = [0_u8; 70];
        let mut go =
            |f: FrameFields, c: FrameContent<'_>, z: &[u8]| kf(encode(&tbl, f, c, z, &mut out));
        let ic = |fld| (ErrorKind::InvalidField, Some(fld));
        assert_eq!(
            go(
                FrameFields {
                    spacecraft_id: scid(1),
                    ..f
                },
                ok,
                &z
            ),
            (ErrorKind::UnknownChannel, Some(field::SPACECRAFT_ID))
        );
        assert_eq!(
            go(
                FrameFields {
                    vc_id: vcid(9),
                    ..f
                },
                ok,
                &z
            ),
            (ErrorKind::UnknownChannel, Some(field::VC_ID))
        );
        assert_eq!(
            go(
                FrameFields {
                    vc_id: vcid(63),
                    ..f
                },
                ok,
                &z
            ),
            ic(field::VC_ID)
        );
        assert_eq!(
            go(
                FrameFields {
                    map_id: mapid(1),
                    ..f
                },
                ok,
                &z
            ),
            (ErrorKind::UnknownChannel, Some(field::MAP_ID))
        );
        let var_rule = content(TfdzRule::NoSegmentation, Upid::Packets, &[0, 0], ocf);
        assert_eq!(go(f, var_rule, &z), ic(field::TFDZ_CONSTRUCTION_RULE));
        assert_eq!(
            go(
                FrameFields {
                    vcf_count: 256,
                    ..f
                },
                ok,
                &z
            ),
            (ErrorKind::OutOfRange, Some(field::VCF_COUNT))
        );
        assert_eq!(
            go(f, FrameContent { ocf: None, ..ok }, &z),
            ic(field::OCF_FLAG)
        );
        assert_eq!(
            go(
                f,
                FrameContent {
                    insert_zone: &[0],
                    ..ok
                },
                &z
            ),
            (ErrorKind::LengthMismatch, Some(field::INSERT_ZONE_LEN))
        );
        assert_eq!(go(f, ok, &[]), ic(field::FRAME_DATA_FIELD));
        assert_eq!(
            go(f, ok, &z[..44]),
            (ErrorKind::LengthMismatch, Some(field::FRAME_DATA_FIELD))
        );
        let far = content(
            TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::PacketStartsAt(offset(45))),
            Upid::Packets,
            &[0, 0],
            ocf,
        );
        assert_eq!(
            go(f, far, &z),
            (ErrorKind::OutOfRange, Some(field::FHP_LVO_POINTER))
        );
    }

    #[test]
    fn writer_length_errors() {
        // req: USLP-003, USLP-006, USLP-010, API-023
        let tbl = fixed();
        let ocf = Some(Ocf::decode([0; 4]));
        let pkt = TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::NoPacketStartOrEnd);
        let ok = content(pkt, Upid::Packets, &[0, 0], ocf);
        let f = fields(1, 0, FrameKind::SequenceControlled, 0);
        let v = plain_variable();
        let big = [0_u8; 600];
        let mut out = [0_u8; 700];
        let c = content(TfdzRule::NoSegmentation, Upid::Packets, &[], None);
        assert_eq!(
            kf(encode(
                &v,
                fields(4, 0, FrameKind::SequenceControlled, 0),
                c,
                &big,
                &mut out
            )),
            (ErrorKind::OutOfRange, Some(field::FRAME_LEN))
        );
        let mut buf = [0_u8; 64];
        let w = UslpFrameWriter::begin(&mut buf, &tbl, f, ok, 45).unwrap();
        assert_eq!(
            kf(w.finish(44)),
            (ErrorKind::LengthMismatch, Some(field::FRAME_DATA_FIELD))
        );
        let lvo = content(
            TfdzRule::SduStart(LastValidOctet::At(offset(9))),
            Upid::Packets,
            &[],
            None,
        );
        let mut buf = [0_u8; 64];
        let w =
            UslpFrameWriter::begin(&mut buf, &v, fields(6, 0, FrameKind::Expedited, 0), lvo, 20)
                .unwrap();
        assert_eq!(
            kf(w.finish(9)),
            (ErrorKind::OutOfRange, Some(field::FHP_LVO_POINTER))
        );
    }

    #[test]
    fn oid_frames() {
        // req: USLP-009, CMN-020, CMN-021, GEN-042
        let tbl = fixed();
        let mut oid = OidGenerator::new();
        let mut out = [0_u8; 64];
        let f = fields(63, 0, FrameKind::Expedited, 5);
        let n = encode_oid(&tbl, f, &[0xAA, 0xBB], None, &mut oid, &mut out).unwrap();
        assert_eq!(n, 64);
        let dec = UslpFrame::decode(&out, &tbl, DecodeMode::Strict).unwrap();
        assert!(dec.anomalies().is_empty());
        assert_eq!(dec.tfdf_header().upid, Upid::OnlyIdleData);
        let mut reference = OidGenerator::new();
        let mut expect = [0_u8; 49];
        reference.fill(&mut expect);
        assert_eq!(dec.tfdz(), expect);
        assert_eq!(oid, reference);
        let before = oid.clone();
        let errs = [
            (
                fields(1, 0, FrameKind::Expedited, 0),
                (ErrorKind::InvalidField, Some(field::VC_ID)),
            ),
            (
                fields(63, 1, FrameKind::Expedited, 0),
                (ErrorKind::InvalidField, Some(field::MAP_ID)),
            ),
            (
                fields(63, 0, FrameKind::ProtocolControl, 0),
                (
                    ErrorKind::InvalidField,
                    Some(field::PROTOCOL_CONTROL_COMMAND_FLAG),
                ),
            ),
            (
                fields(63, 0, FrameKind::Expedited, 256),
                (ErrorKind::OutOfRange, Some(field::VCF_COUNT)),
            ),
        ];
        for (f, e) in errs {
            assert_eq!(
                kf(encode_oid(&tbl, f, &[0, 0], None, &mut oid, &mut out)),
                e
            );
        }
        let mut small = [0_u8; 10];
        assert_eq!(
            kf(encode_oid(&tbl, f, &[0, 0], None, &mut oid, &mut small)),
            (ErrorKind::BufferTooSmall, None)
        );
        assert_eq!(oid, before);
    }

    #[test]
    fn oid_frames_from_the_galois_form() {
        // req: USLP-009, CMN-021, CMN-022, GEN-042
        let tbl = fixed();
        let f = fields(63, 0, FrameKind::Expedited, 5);
        let (mut fib, mut gal) = (OidGenerator::new(), OidGeneratorGalois::new());
        for _ in 0..3 {
            let (mut one, mut two) = ([0_u8; 64], [0_u8; 64]);
            let len_fib = encode_oid(&tbl, f, &[0xAA, 0xBB], None, &mut fib, &mut one).unwrap();
            let len_gal =
                encode_oid_galois(&tbl, f, &[0xAA, 0xBB], None, &mut gal, &mut two).unwrap();
            assert_eq!(
                (len_fib, one),
                (len_gal, two),
                "the forms give the same frames"
            );
        }
        assert_eq!(fib.next_octet(), gal.next_octet());
        let before = gal.clone();
        let mut out = [0xEE_u8; 64];
        let bad = fields(63, 1, FrameKind::Expedited, 0);
        assert_eq!(
            kf(encode_oid_galois(
                &tbl,
                bad,
                &[0, 0],
                None,
                &mut gal,
                &mut out
            )),
            (ErrorKind::InvalidField, Some(field::MAP_ID))
        );
        let mut small = [0xEE_u8; 10];
        assert_eq!(
            kf(encode_oid_galois(
                &tbl,
                f,
                &[0, 0],
                None,
                &mut gal,
                &mut small
            )),
            (ErrorKind::BufferTooSmall, None)
        );
        assert_eq!(gal, before);
        assert_eq!((out, small), ([0xEE; 64], [0xEE; 10]));
    }

    #[test]
    fn truncated_frames() {
        // req: USLP-002, USLP-012, USLP-016, API-075
        let tbl = variable();
        let ch = |vc: u8, map: u8| ChannelFields {
            spacecraft_id: scid(0x155),
            source_or_destination: SourceOrDestination::Destination,
            vc_id: vcid(vc),
            map_id: mapid(map),
        };
        let hdr = |rule| TfdfHeader {
            rule,
            upid: Upid::MissionSpecificInformation1,
        };
        let mut out = [0_u8; 40];
        let n = encode_truncated(
            &tbl,
            ch(4, 2),
            hdr(TfdzRule::NoSegmentation),
            &[1, 2, 3, 4, 5, 6, 7],
            &mut out,
        )
        .unwrap();
        assert_eq!(n, 12);
        let dec = UslpFrame::decode(&out[..n], &tbl, DecodeMode::Strict).unwrap();
        assert_eq!(
            (dec.tfdz(), dec.tfdf_header()),
            (&[1, 2, 3, 4, 5, 6, 7][..], hdr(TfdzRule::NoSegmentation))
        );
        let n =
            encode_truncated(&tbl, ch(5, 1), hdr(TfdzRule::OctetStream), &[9], &mut out).unwrap();
        assert_eq!(n, 28);
        let dec = UslpFrame::decode(&out[..n], &tbl, DecodeMode::Strict).unwrap();
        assert_eq!(
            (dec.security_header().len(), dec.security_trailer().len()),
            (6, 16)
        );
        let e = |c, r, z: &[u8], out: &mut [u8]| kf(encode_truncated(&tbl, c, hdr(r), z, out));
        let ns = TfdzRule::NoSegmentation;
        assert_eq!(
            e(ch(6, 0), ns, &[1], &mut out),
            (
                ErrorKind::InvalidField,
                Some(field::END_OF_FRAME_PRIMARY_HEADER_FLAG)
            )
        );
        assert_eq!(
            e(ch(9, 0), ns, &[1], &mut out),
            (ErrorKind::UnknownChannel, Some(field::VC_ID))
        );
        assert_eq!(
            e(ch(4, 1), ns, &[1], &mut out),
            (ErrorKind::UnknownChannel, Some(field::MAP_ID))
        );
        let start = TfdzRule::SduStart(LastValidOctet::SduContinues);
        assert_eq!(
            e(ch(4, 2), start, &[1], &mut out),
            (ErrorKind::InvalidField, Some(field::TFDZ_CONSTRUCTION_RULE))
        );
        assert_eq!(
            e(ch(4, 2), ns, &[], &mut out),
            (ErrorKind::InvalidField, Some(field::FRAME_DATA_FIELD))
        );
        assert_eq!(
            e(ch(4, 2), ns, &[1], &mut out),
            (ErrorKind::LengthMismatch, Some(field::FRAME_DATA_FIELD))
        );
        let mut small = [0_u8; 11];
        assert_eq!(
            e(ch(4, 2), ns, &[0; 7], &mut small),
            (ErrorKind::BufferTooSmall, None)
        );
        let f = fixed();
        let x = kf(encode_truncated(
            &f,
            ChannelFields {
                vc_id: vcid(63),
                ..ch(4, 0)
            },
            hdr(ns),
            &[1],
            &mut out,
        ));
        assert_eq!(x, (ErrorKind::InvalidField, Some(field::VC_ID)));
    }

    #[test]
    fn a_vc_with_a_truncated_length_refuses_non_truncated_frames() {
        // req: USLP-002, API-022, OI-48
        let c = content(TfdzRule::NoSegmentation, Upid::OctetStream, &[], None);
        let flag = (
            ErrorKind::InvalidField,
            Some(field::END_OF_FRAME_PRIMARY_HEADER_FLAG),
        );
        let f = fields(4, 0, FrameKind::SequenceControlled, 0);
        let plain = plain_variable();
        let t = variable();
        let mut out = [0xEE_u8; 64];
        // The same arguments are good on a VC without a truncated frame length.
        assert!(encode(&plain, f, c, &[1, 2, 3], &mut out).is_ok());
        out.fill(0xEE);
        assert_eq!(kf(encode(&t, f, c, &[1, 2, 3], &mut out)), flag);
        let begin = UslpFrameWriter::begin(&mut out, &t, f, c, 3);
        assert_eq!(kf(begin.map(|_| 0)), flag);
        let resume = UslpFrameWriter::resume(&mut out, &t, f, c, 3);
        assert_eq!(kf(resume.map(|_| 0)), flag);
        assert_eq!(out, [0xEE; 64]);
        // The channel is checked first: VC 5 with an unknown MAP is `UnknownChannel`.
        let f = fields(5, 3, FrameKind::SequenceControlled, 0);
        assert_eq!(
            kf(encode(&t, f, c, &[1], &mut out)),
            (ErrorKind::UnknownChannel, Some(field::MAP_ID))
        );
        // The truncated writer and the VC without the length are the other direction.
        assert_eq!(out, [0xEE; 64]);
    }
}
