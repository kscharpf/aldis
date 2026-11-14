//! Encoders: [`encode`], [`UslpFrameWriter`], [`encode_truncated`], [`encode_oid`], and
//! [`encode_oid_galois`].
//!
//! 732.1 §4.1, §6.3, Annex D; USLP-009, USLP-010, USLP-016, API-020…API-023, API-072, API-074,
//! RS-023, GEN-014, GEN-042, GEN-052.
//!
//! [`UslpFrameWriter`] carries the finishing typestate of RS-023: `Unprotected`, `Protected`,
//! `Finalized`.
//!
//! The writers compute the Frame Length, the OCF Flag, the VCF Count Length (from the VC and
//! the quality of service of the Bypass/Sequence Control Flag), the TFDF Header, and the FECF, and take the
//! TFDZ, the Insert Zone, and the OCF as given (USLP-010): completing a partly filled
//! fixed-length TFDZ with an Encapsulation Idle Packet is the packet generator's (LNK-013;
//! OI-34). Their checks and errors are those of `docs/standards/uslp-frame.md` §3.3, in its
//! order.

use crate::config::MapConfig;
use aldis_core::lookup::ChannelLookup;
use aldis_core::lookup::MapLookup;
use aldis_core::lookup::PhysicalLookup;

use aldis_core::finishing::Unprotected;
use aldis_core::layout::{FrameLayout, LayoutSpec, OCF_LEN};
use aldis_core::ocf::Ocf;
use aldis_core::oid::{OidGenerator, OidGeneratorGalois};
use aldis_core::security::SecurityLayout;
use aldis_core::writer::{FinishPlan, FramePlan, FrameWriter, Variable, VariablePlan};
use aldis_core::{Error, Result};

use crate::config::{OcfUse, PhysicalChannelConfig, VcConfig, frame_layout, frame_security};
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
///
/// # Errors
/// `OutOfRange` (`frame_data_field`) for a TFDZ the 16-bit pointer cannot reach, more than
/// 65 536 octets; the frame length limit of the configuration keeps a plan below it.
fn oid_tfdf(tfdz_len: usize) -> Result<TfdfHeader> {
    let last = u16::try_from(tfdz_len.saturating_sub(1)).map_err(|_| {
        Error::out_of_range(
            field::FRAME_DATA_FIELD,
            u64::from(u16::MAX).saturating_add(1),
            len_u64(tfdz_len),
        )
    })?;
    Ok(TfdfHeader {
        rule: TfdzRule::SduStart(LastValidOctet::from_raw(last)),
        upid: Upid::OnlyIdleData,
    })
}

/// The VC of `fields`, checked: the SCID and VCID configured, and VCID 63 exactly for OID
/// frames (§3.3 rows 1–2).
fn find_vc<
    L: ChannelLookup<UslpSpacecraftId, UslpVcId, Vc = VcConfig>
        + MapLookup<UslpSpacecraftId, UslpVcId, UslpMapId, Map = MapConfig>
        + PhysicalLookup<Physical = PhysicalChannelConfig>
        + ?Sized,
>(
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
/// is built, so only a lookup that skips the check reaches it); `LengthMismatch` (`frame_data_field`)
/// for a TFDZ that does not fill a fixed-length frame; `OutOfRange` (`frame_len`) above a
/// variable-length channel's maximum.
fn reserve(
    physical: &PhysicalChannelConfig,
    vc: &VcConfig,
    base: LayoutSpec,
    tfdz_len: Option<usize>,
) -> Result<(FrameLayout, usize)> {
    // The longest frame of the channel shows how long the TFDZ may be.
    frame_layout(&base, vc.fixed_tfdz())
        .ok_or(invalid(field::FRAME_DATA_FIELD))
        .and_then(|longest| {
            let tfdz_len = tfdz_len.unwrap_or_else(|| longest.data_zone().len());
            if tfdz_len == 0 {
                return Err(invalid(field::FRAME_DATA_FIELD));
            }
            let spec = base
                .with_data_len(crate::tfdf::header_len(vc.fixed_tfdz()).saturating_add(tfdz_len));
            let total = spec.frame_len;
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
            frame_layout(&spec, vc.fixed_tfdz())
                .map(|layout| (layout, tfdz_len))
                .ok_or(invalid(field::FRAME_DATA_FIELD))
        })
}

impl Plan {
    /// Checks a frame of `tfdz_len` TFDZ octets in the order of
    /// `docs/standards/uslp-frame.md` §3.3 (USLP-009, USLP-010, USLP-016).
    fn new<
        L: ChannelLookup<UslpSpacecraftId, UslpVcId, Vc = VcConfig>
            + MapLookup<UslpSpacecraftId, UslpVcId, UslpMapId, Map = MapConfig>
            + PhysicalLookup<Physical = PhysicalChannelConfig>
            + ?Sized,
    >(
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
            oid_tfdf(tfdz_len)?
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
pub fn encode<
    L: ChannelLookup<UslpSpacecraftId, UslpVcId, Vc = VcConfig>
        + MapLookup<UslpSpacecraftId, UslpVcId, UslpMapId, Map = MapConfig>
        + PhysicalLookup<Physical = PhysicalChannelConfig>
        + ?Sized,
>(
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
    let mut w = start(out, plan, content.insert_zone)?;
    for (d, s) in w.data_mut().iter_mut().zip(tfdz) {
        *d = *s;
    }
    // The one-shot encoders take the Security Header and Trailer as given (API-072).
    w.taken_as_given(tfdz.len(), UslpOneShot(()))
        .map(|w| w.finalize().len())
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
pub fn encode_oid<
    L: ChannelLookup<UslpSpacecraftId, UslpVcId, Vc = VcConfig>
        + MapLookup<UslpSpacecraftId, UslpVcId, UslpMapId, Map = MapConfig>
        + PhysicalLookup<Physical = PhysicalChannelConfig>
        + ?Sized,
>(
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
pub fn encode_oid_galois<
    L: ChannelLookup<UslpSpacecraftId, UslpVcId, Vc = VcConfig>
        + MapLookup<UslpSpacecraftId, UslpVcId, UslpMapId, Map = MapConfig>
        + PhysicalLookup<Physical = PhysicalChannelConfig>
        + ?Sized,
>(
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
fn oid_frame<
    L: ChannelLookup<UslpSpacecraftId, UslpVcId, Vc = VcConfig>
        + MapLookup<UslpSpacecraftId, UslpVcId, UslpMapId, Map = MapConfig>
        + PhysicalLookup<Physical = PhysicalChannelConfig>
        + ?Sized,
>(
    channels: &L,
    fields: FrameFields,
    insert_zone: &[u8],
    ocf: Option<Ocf>,
    out: &mut [u8],
    fill: impl FnOnce(&mut [u8]),
) -> Result<usize> {
    let content = FrameContent {
        tfdf: oid_tfdf(1)?,
        insert_zone,
        ocf,
    };
    let plan = Plan::new(channels, fields, content, None, Purpose::Oid)?;
    let tfdz_len = plan.tfdz_len;
    let mut w = start(out, plan, insert_zone)?;
    fill(w.data_mut());
    // The one-shot encoders take the Security Header and Trailer as given (API-072).
    w.taken_as_given(tfdz_len, UslpOneShot(()))
        .map(|w| w.finalize().len())
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
pub fn encode_truncated<
    L: ChannelLookup<UslpSpacecraftId, UslpVcId, Vc = VcConfig>
        + MapLookup<UslpSpacecraftId, UslpVcId, UslpMapId, Map = MapConfig>
        + PhysicalLookup<Physical = PhysicalChannelConfig>
        + ?Sized,
>(
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

/// The proof that a caller is a one-shot encoder of this module (API-072).
///
/// The one-shot encoders take the Security Header and Trailer as given; only this module builds
/// the proof, so no other caller finishes a frame without the SDLS step (RS-023, API-073).
///
/// ```compile_fail
/// let _ = aldis_uslp::writer::UslpOneShot(());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UslpOneShot(());

/// The plan of a non-truncated USLP frame for [`FrameWriter`] (732.1 §4.1, §6.3; USLP-010).
///
/// Variable-length: its Primary Header and OCF are rewritten when the TFDZ length is fixed (732.1
/// §4.1.2.7). Its associated functions start the writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UslpPlan {
    channel: ChannelFields,
    kind: FrameKind,
    count: VcfCount,
    tfdf: TfdfHeader,
    ocf: Option<Ocf>,
    fixed: bool,
    /// The TFDZ octets reserved.
    tfdz_len: usize,
    header_len: usize,
    fecf_len: usize,
}

impl FramePlan for UslpPlan {}

impl FinishPlan for UslpPlan {
    type OneShot = UslpOneShot;
}

impl VariablePlan for UslpPlan {
    fn write_header(&self, frame: &mut [u8], total: usize, finished: bool) {
        let version = if finished {
            VERSION
        } else {
            UNFINISHED_VERSION
        };
        let header = primary_octets(
            version,
            self.channel,
            total,
            self.kind,
            self.ocf.is_some(),
            self.count,
        );
        for (d, s) in frame.iter_mut().zip(header.iter().take(self.header_len)) {
            *d = *s;
        }
        if finished && let Some(ocf) = self.ocf {
            let at = total.saturating_sub(self.fecf_len).saturating_sub(OCF_LEN);
            // The range is the plan's own, inside the frame of `total` octets: no wire input.
            let tail = frame.get_mut(at..).unwrap_or_default();
            for (d, s) in tail.iter_mut().zip(ocf.to_octets()) {
                *d = s;
            }
        }
    }

    fn check_data_len(&self, tfdz_len: usize) -> Result<()> {
        if self.fixed && tfdz_len != self.tfdz_len {
            return Err(mismatch(field::FRAME_DATA_FIELD));
        }
        self.tfdf.check_pointer(tfdz_len)
    }
}

/// Two-phase writer of a non-truncated frame (732.1 §4.1, §6.3; API-020…API-023, API-072, API-074, RS-023).
///
/// [`UslpPlan::begin`] reserves the frame for up to a maximum TFDZ and returns the writer
/// [`Unprotected`]; the caller fills the TFDZ (`data_mut`) and, with SDLS, the Security Header
/// and Trailer in place. `protect` (or `unsecured` without SDLS) fixes the TFDZ length, which
/// decides the Frame Length and where the Security Trailer, the OCF, and the FECF lie, writes the
/// header and the OCF, and applies SDLS; the writer is then [`Protected`](aldis_core::finishing::Protected). `finalize` computes
/// the FECF, last, and gives the [`Finalized`](aldis_core::finishing::Finalized) writer, the only state that exposes the frame's
/// octets. Each step consumes the writer, so a step out of order does not compile (RS-023,
/// RS-021). The steps are those of the core [`FrameWriter`].
///
/// Until `protect` or `unsecured`, the buffer holds a Primary Header whose TFVN is `1111`, which
/// every decoder rejects with `UnsupportedVersion` and the delimiter treats as fill, so an
/// unfinished frame cannot be mistaken for a valid one (API-023). Its Frame Length field holds
/// the reserved frame length.
pub type UslpFrameWriter<'b, S = Unprotected> = FrameWriter<'b, Variable, UslpPlan, S>;

impl UslpPlan {
    /// Reserves a frame with room for up to `max_tfdz_len` TFDZ octets (732.1 §4.1; USLP-010,
    /// API-020). On a fixed-length Physical Channel `max_tfdz_len` must fill the frame.
    ///
    /// # Errors
    /// The errors of [`encode`] for a frame with `max_tfdz_len` TFDZ octets, except the
    /// pointer, which `protect` and `unsecured` check against the TFDZ it is given. On error
    /// `out` is unchanged.
    pub fn begin<
        'b,
        L: ChannelLookup<UslpSpacecraftId, UslpVcId, Vc = VcConfig>
            + MapLookup<UslpSpacecraftId, UslpVcId, UslpMapId, Map = MapConfig>
            + PhysicalLookup<Physical = PhysicalChannelConfig>
            + ?Sized,
    >(
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
        start(out, plan, content.insert_zone)
    }
}

/// Takes up the frame of a checked plan in `out`: writes the unfinished header, the Insert Zone,
/// and the TFDF Header, and zeros in the Security Header and Trailer (API-023).
fn start<'b>(out: &'b mut [u8], plan: Plan, insert_zone: &[u8]) -> Result<UslpFrameWriter<'b>> {
    let uslp = UslpPlan {
        channel: plan.channel,
        kind: plan.kind,
        count: plan.count,
        tfdf: plan.tfdf,
        ocf: plan.ocf,
        fixed: plan.fixed,
        tfdz_len: plan.tfdz_len,
        header_len: plan.layout.primary_header().len(),
        fecf_len: plan.layout.fecf().len(),
    };
    let tfdf = plan.tfdf.octets();
    let mut w = FrameWriter::reserve_variable(out, plan.layout, uslp)?;
    for (d, s) in w.secondary_mut().iter_mut().zip(insert_zone) {
        *d = *s;
    }
    for (d, s) in w.data_header_mut().iter_mut().zip(tfdf) {
        *d = s;
    }
    Ok(w)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    extern crate std;
    use super::{
        FrameContent, FrameFields, UslpPlan, encode, encode_oid, encode_oid_galois,
        encode_truncated, oid_tfdf,
    };
    use crate::config::tests::{fixed, mapid, plain_variable, scid, variable, vcid};
    use crate::field;
    use crate::header::{ChannelFields, FrameKind, SourceOrDestination};
    use crate::tfdf::tests::offset;
    use crate::tfdf::{FirstHeaderPointer, LastValidOctet, TfdfHeader, TfdzRule};
    use crate::upid::Upid;
    use crate::view::UslpFrame;
    use aldis_core::crc::syndrome;
    use aldis_core::ocf::Ocf;
    use aldis_core::oid::{OidGenerator, OidGeneratorGalois};
    use aldis_core::security::{
        FrameCtx, HookLayout, NoSecurity, PreFilledSecurity, SecuredFields, SecurityHook,
    };
    use aldis_core::{Error, ErrorKind, FieldId};
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

    fn kf<T>(r: Result<T, Error>) -> (ErrorKind, Option<FieldId>) {
        let e = r.err().unwrap();
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
        let dec = UslpFrame::decode(&out[..n], &tbl).unwrap();
        let full = dec.full().unwrap();
        assert_eq!(
            (full.tfdz(), full.ocf(), full.insert_zone()),
            (&tfdz[..], ocf, &[0xAA, 0xBB][..])
        );
        assert_eq!(full.vcf_count().value(), 0x1234);
        assert_eq!(full.tfdf_header(), c.tfdf);
        let mut two = [0_u8; 64];
        let mut w = UslpPlan::begin(&mut two, &tbl, f, c, 44).unwrap();
        assert!(w.security_header_mut().is_empty() && w.security_trailer_mut().is_empty());
        w.data_mut().copy_from_slice(&tfdz);
        assert_eq!(w.unsecured(44).unwrap().finalize().len(), 64);
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
        let w = UslpPlan::begin(&mut out, &tbl, f, c, 44).unwrap();
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
            let mut writer = UslpPlan::begin(&mut buf, &tbl, f, c, 4).unwrap();
            writer.data_mut()[..2].copy_from_slice(&[1, 2]);
            let len = writer.unsecured(2).unwrap().finalize().len();
            assert_eq!(len, 10 + if ocf.is_some() { 4 } else { 0 });
            let dec = UslpFrame::decode(&buf[..len], &tbl).unwrap();
            let full = dec.full().unwrap();
            assert_eq!(
                (full.tfdz(), full.ocf(), full.fecf()),
                (&[1, 2][..], ocf, None)
            );
        }
    }

    fn ctx(vc: u8) -> FrameCtx {
        FrameCtx::Uslp {
            scid: scid(0x155),
            vcid: vcid(vc),
            map_id: None,
        }
    }

    /// A hook that checks the frame state it is called in, and fills the Security fields.
    struct OrderHook;

    impl SecurityHook for OrderHook {
        fn layout(&self) -> HookLayout {
            HookLayout::Any
        }

        fn protect(
            &mut self,
            _: &FrameCtx,
            frame: &mut [u8],
            fields: &SecuredFields,
        ) -> aldis_core::Result<()> {
            // The frame has its final length and the Security Trailer follows the TFDF; the FECF
            // is not yet computed (zero).
            if fields.trailer().end.saturating_add(2) != frame.len()
                || frame.last_chunk::<2>() != Some(&[0, 0])
            {
                return Err(aldis_core::Error::new(aldis_core::ErrorKind::Security));
            }
            frame[fields.header()].fill(0xA5);
            frame[fields.trailer()].fill(0x5A);
            Ok(())
        }

        fn verify(
            &mut self,
            _: &FrameCtx,
            _: &mut [u8],
            fields: &SecuredFields,
        ) -> aldis_core::Result<core::ops::Range<usize>> {
            Ok(fields.data())
        }
    }

    /// A hook whose protection fails.
    struct Refuse;

    impl SecurityHook for Refuse {
        fn layout(&self) -> HookLayout {
            HookLayout::Any
        }

        fn protect(
            &mut self,
            _: &FrameCtx,
            _: &mut [u8],
            _: &SecuredFields,
        ) -> aldis_core::Result<()> {
            Err(aldis_core::Error::new(ErrorKind::Security))
        }

        fn verify(
            &mut self,
            _: &FrameCtx,
            _: &mut [u8],
            fields: &SecuredFields,
        ) -> aldis_core::Result<core::ops::Range<usize>> {
            Ok(fields.data())
        }
    }

    #[test]
    fn no_security_is_rejected_on_a_channel_with_sdls() {
        // req: RS-023, API-070, API-073
        let tbl = plain_variable();
        let c = content(TfdzRule::NoSegmentation, Upid::OctetStream, &[], None);
        let f = fields(5, 1, FrameKind::SequenceControlled, 0);
        let mut buf = [0xEE_u8; 64];
        let w = UslpPlan::begin(&mut buf, &tbl, f, c, 10).unwrap();
        let e = w.protect(3, &mut NoSecurity, &ctx(5)).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (
                ErrorKind::InvalidField,
                Some(aldis_core::field::SECURITY_HEADER_LEN)
            )
        );
    }

    #[test]
    fn a_failed_protect_gives_no_frame() {
        // req: RS-023, API-073
        let tbl = plain_variable();
        let c = content(TfdzRule::NoSegmentation, Upid::OctetStream, &[], None);
        let f = fields(5, 1, FrameKind::SequenceControlled, 0);
        let mut buf = [0xEE_u8; 64];
        let w = UslpPlan::begin(&mut buf, &tbl, f, c, 10).unwrap();
        let e = w.protect(3, &mut Refuse, &ctx(5)).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::Security);
        // The buffer holds the unfinished header again, which no decoder accepts (API-023).
        let e = UslpFrame::decode_lenient(&buf, &tbl).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::UnsupportedVersion);
    }

    #[test]
    fn protect_runs_after_the_length_and_before_the_fecf() {
        // req: RS-023, API-074, API-073, USLP-010
        let tbl = plain_variable();
        let c = content(TfdzRule::NoSegmentation, Upid::OctetStream, &[], None);
        let f = fields(5, 1, FrameKind::SequenceControlled, 0);
        let mut buf = [0xEE_u8; 64];
        let mut w = UslpPlan::begin(&mut buf, &tbl, f, c, 10).unwrap();
        w.data_mut()[..3].copy_from_slice(&[7, 8, 9]);
        let done = w.protect(3, &mut OrderHook, &ctx(5)).unwrap().finalize();
        assert!(!done.is_empty());
        assert_eq!(done.octets().len(), done.len());
        let octets = done.into_octets();
        assert_eq!(syndrome(octets).get(), 0);
        let dec = UslpFrame::decode(octets, &tbl).unwrap();
        assert_eq!(
            (dec.security_header(), dec.tfdz(), dec.security_trailer()),
            (&[0xA5; 6][..], &[7, 8, 9][..], &[0x5A; 16][..])
        );
    }

    #[test]
    fn variable_frames_with_sdls_shrink() {
        // req: USLP-010, API-022, API-023, API-072
        let tbl = plain_variable();
        let c = content(TfdzRule::NoSegmentation, Upid::OctetStream, &[], None);
        let f = fields(5, 1, FrameKind::SequenceControlled, 0);
        let mut buf = [0_u8; 64];
        let mut w = UslpPlan::begin(&mut buf, &tbl, f, c, 10).unwrap();
        w.security_header_mut().fill(0x51);
        w.security_trailer_mut().fill(0x52);
        w.data_mut()[..3].copy_from_slice(&[7, 8, 9]);
        let len = w
            .protect(3, &mut PreFilledSecurity, &ctx(5))
            .unwrap()
            .finalize()
            .len();
        let dec = UslpFrame::decode(&buf[..len], &tbl).unwrap();
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
            let _unfinished = UslpPlan::begin(&mut raw, &tbl, f, c, 10).unwrap();
        }
        let e = UslpFrame::decode_lenient(&raw, &tbl).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::UnsupportedVersion);
        let w = UslpPlan::begin(&mut raw, &tbl, f, c, 10).unwrap();
        assert_eq!(
            kf(w.protect(11, &mut PreFilledSecurity, &ctx(5))),
            (ErrorKind::OutOfRange, Some(field::USER_LEN))
        );
        let w = UslpPlan::begin(&mut raw, &tbl, f, c, 10).unwrap();
        assert_eq!(
            kf(w.protect(0, &mut PreFilledSecurity, &ctx(5))),
            (ErrorKind::InvalidField, Some(field::FRAME_DATA_FIELD))
        );
        let w = UslpPlan::begin(&mut raw, &tbl, f, c, 10).unwrap();
        assert_eq!(
            kf(w.unsecured(3)),
            (
                ErrorKind::InvalidField,
                Some(aldis_core::field::SECURITY_HEADER_LEN)
            )
        );
        let unknown = FrameFields {
            vc_id: vcid(9),
            ..f
        };
        let e = UslpPlan::begin(&mut raw, &tbl, unknown, c, 10).unwrap_err();
        assert_eq!(e.field(), Some(field::VC_ID));
        let mut small = [0_u8; 20];
        assert_eq!(
            kf(encode(&tbl, f, c, &[1; 10], &mut small)),
            (ErrorKind::BufferTooSmall, None)
        );
        assert_eq!(small, [0; 20]);
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
        let w = UslpPlan::begin(&mut buf, &tbl, f, ok, 45).unwrap();
        assert_eq!(
            kf(w.unsecured(44)),
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
            UslpPlan::begin(&mut buf, &v, fields(6, 0, FrameKind::Expedited, 0), lvo, 20).unwrap();
        assert_eq!(
            kf(w.unsecured(9)),
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
        let dec = UslpFrame::decode(&out, &tbl).unwrap();
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
        let dec = UslpFrame::decode(&out[..n], &tbl).unwrap();
        assert_eq!(
            (dec.tfdz(), dec.tfdf_header()),
            (&[1, 2, 3, 4, 5, 6, 7][..], hdr(TfdzRule::NoSegmentation))
        );
        let n =
            encode_truncated(&tbl, ch(5, 1), hdr(TfdzRule::OctetStream), &[9], &mut out).unwrap();
        assert_eq!(n, 28);
        let dec = UslpFrame::decode(&out[..n], &tbl).unwrap();
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
        let begin = UslpPlan::begin(&mut out, &t, f, c, 3);
        assert_eq!(kf(begin.map(|_| 0)), flag);
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

    #[test]
    fn an_oid_tfdz_beyond_the_pointer_is_an_error_not_a_clamped_pointer() {
        // req: USLP-009, GEN-041
        let longest = oid_tfdf(65_536).unwrap();
        assert_eq!(
            longest.rule,
            TfdzRule::SduStart(LastValidOctet::from_raw(u16::MAX))
        );
        let e = oid_tfdf(65_537).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::OutOfRange, Some(field::FRAME_DATA_FIELD))
        );
    }
}
