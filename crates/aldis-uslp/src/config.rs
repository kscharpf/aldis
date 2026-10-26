//! Managed parameters of the Unified Space Data Link Protocol that the codec reads.
//!
//! 732.1 §5, §6.6, Annex D1.2; USLP-002, USLP-004, USLP-008, USLP-012, GEN-060, GEN-061,
//! API-040, API-041, API-070, API-075, RS-012.
//!
//! | Level | Type | Parameters kept |
//! |---|---|---|
//! | Physical Channel (Table 5-1) | [`PhysicalChannelConfig`] | Transfer Frame Type, Transfer Frame Length, Insert Zone length, presence of the FECF |
//! | Master Channel (Table 5-2) | the [`VcConfig`] entries of one SCID | SCID, valid VCIDs; "Inclusion of OCF Required" as [`OcfUse::Required`] on its VCs |
//! | Virtual Channel (Tables 5-3, 6-2) | [`VcConfig`] | VC Transfer Frame Type, VCF Count Lengths, OCF use, Truncated Transfer Frame Length, SDLS lengths or their scope |
//! | MAP (Tables 5-4, 6-3) | [`MapConfig`] | MAP ID, SDLS lengths |
//!
//! The decoder, the writers, the delimiter, and the Frame Validation Checks look the channels up
//! through [`VcLookup`]; [`VcTable`] is its fixed-capacity implementation (M2 plan D14).
//! Parameters that only the engines use (multiplexing schemes, repetitions, delays, COP in
//! Effect, SDU types, UPIDs supported) are not kept here (`docs/standards/uslp-frame.md` §2.1).

use aldis_core::layout::{FECF_LEN, OCF_LEN};
use aldis_core::security::SecurityLayout;
use aldis_core::{Error, ErrorKind, Result};

use crate::field;
use crate::header::{
    MAX_FRAME_LEN, OID_VCID, PRIMARY_HEADER_LEN, UslpMapId, UslpSpacecraftId, UslpVcId, VcfCountLen,
};

/// `InvalidConfig` naming `field`.
const fn bad(field: aldis_core::FieldId) -> Error {
    Error::new(ErrorKind::InvalidConfig).with_field(field)
}

/// Shortest Transfer Frame Length a Physical Channel may have: the Primary Header, a 1-octet
/// TFDF Header, and one TFDZ octet (732.1 §4.1.2.1.2, §4.1.4.2.1.2; CMN-041).
const MIN_FRAME_LEN: usize = PRIMARY_HEADER_LEN + 2;

/// Whether the frames of a channel have one length or vary (732.1 Tables 5-1, 5-2, 5-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferFrameType {
    /// Fixed-length frames; on a Virtual Channel, a fixed-length TFDZ (§4.1.4.2.2.1.1).
    Fixed,
    /// Variable-length frames; on a Virtual Channel, a variable-length TFDZ.
    Variable,
}

/// The managed parameters of a Physical Channel that the codec reads (732.1 §5.2 Table 5-1;
/// GEN-060, USLP-003, USLP-008).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicalChannelConfig {
    frame_type: TransferFrameType,
    frame_len: usize,
    insert_zone_len: usize,
    fecf: bool,
}

impl PhysicalChannelConfig {
    /// A Physical Channel whose frames are `frame_len` octets long (fixed) or at most that long
    /// (variable), with an Insert Zone of `insert_zone_len` octets (0 = none) and an FECF when
    /// `fecf` is set (732.1 Table 5-1 notes 2 and 5; GEN-060, API-040).
    ///
    /// # Errors
    /// - `InvalidConfig` (`frame_len` for a fixed-length channel, `max_frame_len` for a
    ///   variable-length one) for a length above 65 536 octets or below 9 (§4.1.2.7 note 4).
    /// - `InvalidConfig` (`insert_zone_len`) for an Insert Zone on a variable-length channel
    ///   (Table 5-1 note 5; USLP-008).
    ///
    /// Whether the frame leaves room for a TFDZ on each channel is checked by [`VcTable::new`].
    pub const fn new(
        frame_type: TransferFrameType,
        frame_len: usize,
        insert_zone_len: usize,
        fecf: bool,
    ) -> Result<PhysicalChannelConfig> {
        if frame_len < MIN_FRAME_LEN || frame_len > MAX_FRAME_LEN {
            return Err(bad(match frame_type {
                TransferFrameType::Fixed => field::FRAME_LEN,
                TransferFrameType::Variable => field::MAX_FRAME_LEN,
            }));
        }
        if insert_zone_len > 0 && matches!(frame_type, TransferFrameType::Variable) {
            return Err(bad(field::INSERT_ZONE_LEN));
        }
        Ok(PhysicalChannelConfig {
            frame_type,
            frame_len,
            insert_zone_len,
            fecf,
        })
    }

    /// Physical Channel Transfer Frame Type (Table 5-1).
    #[must_use]
    pub const fn frame_type(&self) -> TransferFrameType {
        self.frame_type
    }

    /// Transfer Frame Length: the length of every frame when fixed, the maximum when variable
    /// (Table 5-1 note 2; USLP-003).
    #[must_use]
    pub const fn frame_len(&self) -> usize {
        self.frame_len
    }

    /// Insert Zone length in octets, 0 without one (§4.1.3; USLP-008).
    #[must_use]
    pub const fn insert_zone_len(&self) -> usize {
        self.insert_zone_len
    }

    /// Whether every non-truncated frame ends with an FECF (§4.1.6.2).
    #[must_use]
    pub const fn fecf(&self) -> bool {
        self.fecf
    }

    /// Whether frames have one length (Table 5-1).
    pub(crate) const fn is_fixed(&self) -> bool {
        matches!(self.frame_type, TransferFrameType::Fixed)
    }
}

/// Whether a Virtual Channel's frames carry an OCF (732.1 §4.1.5.3–4, Table 5-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OcfUse {
    /// No OCF: "Inclusion of OCF Allowed" or "Required" false.
    Absent,
    /// A variable-length VC whose frames may carry one, as the OCF Flag says ("Inclusion of OCF
    /// Allowed" true).
    Allowed,
    /// A fixed-length VC whose every frame carries one ("Inclusion of OCF Required" true, of
    /// the VC or its Master Channel, §4.1.5.3).
    Required,
}

/// Where a Virtual Channel's Security Header and Trailer lengths are set (732.1 §6.6, Tables
/// 6-2 and 6-3; API-070).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecurityScope {
    /// For the whole VC (Table 6-2); [`SecurityLayout::NONE`] without SDLS.
    Vc(SecurityLayout),
    /// Per MAP, by each [`MapConfig`] (Table 6-3).
    PerMap,
}

/// A Truncated Transfer Frame Length, 6 to 32 octets (732.1 Table 5-3 note 5, Annex D1.3.2
/// notes 2–3; USLP-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TruncatedFrameLen(usize);

impl TruncatedFrameLen {
    /// The length with SDLS has at least 28 octets (Annex D1.2.4; USLP-012, API-075).
    pub const MIN_WITH_SDLS: usize = 28;

    /// A truncated frame length of `octets` (Annex D1.3.2).
    ///
    /// # Errors
    /// `InvalidConfig` (`truncated_frame_len`) outside 6–32.
    pub const fn new(octets: usize) -> Result<TruncatedFrameLen> {
        if matches!(octets, 6..=32) {
            Ok(TruncatedFrameLen(octets))
        } else {
            Err(bad(field::TRUNCATED_FRAME_LEN))
        }
    }

    /// The length in octets, the whole truncated frame (Annex D1.3.2.1).
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// The parameters of a [`VcConfig`] besides its identifiers (732.1 Tables 5-3, 6-2).
///
/// Plain data whose fields are each valid by type; [`VcConfig::new`] checks how they relate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VcParams {
    /// VC Transfer Frame Type, which sets the TFDZ type (§4.1.4.2.2.1.1; USLP-016).
    pub frame_type: TransferFrameType,
    /// VC Count Length for the Sequence-Controlled service (Table 5-3; USLP-004).
    pub vcf_count_len_sequence: VcfCountLen,
    /// VC Count Length for the Expedited service (Table 5-3; USLP-004).
    pub vcf_count_len_expedited: VcfCountLen,
    /// Whether the frames carry an OCF (§4.1.5).
    pub ocf: OcfUse,
    /// Truncated Transfer Frame Length, `None` if the VC has no truncated frames (Annex D;
    /// USLP-002).
    pub truncated_frame_len: Option<TruncatedFrameLen>,
    /// Where the SDLS lengths are set (§6.6; API-070).
    pub security: SecurityScope,
}

impl VcParams {
    /// A VC of `frame_type` with no VCF Count, no OCF, no truncated frames, and no SDLS.
    #[must_use]
    pub const fn new(frame_type: TransferFrameType) -> VcParams {
        VcParams {
            frame_type,
            vcf_count_len_sequence: VcfCountLen::NONE,
            vcf_count_len_expedited: VcfCountLen::NONE,
            ocf: OcfUse::Absent,
            truncated_frame_len: None,
            security: SecurityScope::Vc(SecurityLayout::NONE),
        }
    }
}

/// The managed parameters of one Virtual Channel that the codec reads (732.1 §5.4 Table 5-3,
/// §6.6.1 Table 6-2, Annex D1.2; GEN-060, USLP-002, USLP-004, API-070).
///
/// The Spacecraft ID names the Master Channel the VC belongs to; the valid VCIDs of a Master
/// Channel are those of its entries (Table 5-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VcConfig {
    spacecraft_id: UslpSpacecraftId,
    vc_id: UslpVcId,
    params: VcParams,
}

impl VcConfig {
    /// A Virtual Channel (732.1 Table 5-3; GEN-060, API-040).
    ///
    /// # Errors
    /// `InvalidConfig`, checked in this order:
    /// - (`ocf_flag`) for [`OcfUse::Allowed`] on a fixed-length VC or [`OcfUse::Required`] on a
    ///   variable-length one (Table 5-3);
    /// - (`truncated_frame_len`) for a truncated frame length on a fixed-length VC (Annex
    ///   D1.2.1; USLP-002);
    /// - (`transfer_frame_type`) for VCID 63 on a variable-length VC, since OID frames are
    ///   fixed-length (USLP-016, USLP-009);
    /// - (`vc_id`) for VCID 63 with SDLS lengths per MAP: it carries no MAP channel.
    ///
    /// Whether the frames leave room for a TFDZ, and the SDLS minimum of truncated frames, are
    /// checked by [`VcTable::new`], which knows the Physical Channel and the MAPs.
    pub const fn new(
        spacecraft_id: UslpSpacecraftId,
        vc_id: UslpVcId,
        params: VcParams,
    ) -> Result<VcConfig> {
        let fixed = matches!(params.frame_type, TransferFrameType::Fixed);
        let ocf_ok = match params.ocf {
            OcfUse::Absent => true,
            OcfUse::Allowed => !fixed,
            OcfUse::Required => fixed,
        };
        if !ocf_ok {
            return Err(bad(field::OCF_FLAG));
        }
        if fixed && params.truncated_frame_len.is_some() {
            return Err(bad(field::TRUNCATED_FRAME_LEN));
        }
        if vc_id.get() == OID_VCID {
            if !fixed {
                return Err(bad(field::TRANSFER_FRAME_TYPE));
            }
            if matches!(params.security, SecurityScope::PerMap) {
                return Err(bad(field::VC_ID));
            }
        }
        Ok(VcConfig {
            spacecraft_id,
            vc_id,
            params,
        })
    }

    /// Spacecraft ID of the VC's Master Channel (Table 5-2).
    #[must_use]
    pub const fn spacecraft_id(&self) -> UslpSpacecraftId {
        self.spacecraft_id
    }

    /// Virtual Channel Identifier (Table 5-3).
    #[must_use]
    pub const fn vc_id(&self) -> UslpVcId {
        self.vc_id
    }

    /// The other parameters (Tables 5-3, 6-2).
    #[must_use]
    pub const fn params(&self) -> VcParams {
        self.params
    }

    /// VC Transfer Frame Type (Table 5-3; USLP-016).
    #[must_use]
    pub const fn frame_type(&self) -> TransferFrameType {
        self.params.frame_type
    }

    /// Whether the VC's TFDZ is fixed-length (§4.1.4.2.2.1.1; USLP-005, USLP-016).
    pub(crate) const fn fixed_tfdz(&self) -> bool {
        matches!(self.params.frame_type, TransferFrameType::Fixed)
    }

    /// The VCF Count Length for a frame whose Bypass/Sequence Control Flag is `bypass`
    /// (§4.1.2.11.4, Table 5-3; USLP-004).
    #[must_use]
    pub const fn vcf_count_len(&self, bypass: bool) -> VcfCountLen {
        if bypass {
            self.params.vcf_count_len_expedited
        } else {
            self.params.vcf_count_len_sequence
        }
    }

    /// The longer of the two VCF Count Lengths, for the room check.
    fn longest_vcf_count(&self) -> usize {
        self.params
            .vcf_count_len_sequence
            .max(self.params.vcf_count_len_expedited)
            .octets()
    }

    /// Whether the VC's frames carry an OCF (§4.1.5).
    #[must_use]
    pub const fn ocf(&self) -> OcfUse {
        self.params.ocf
    }

    /// Truncated Transfer Frame Length, if the VC carries truncated frames (Annex D1.3.2;
    /// USLP-002).
    #[must_use]
    pub const fn truncated_frame_len(&self) -> Option<TruncatedFrameLen> {
        self.params.truncated_frame_len
    }

    /// Where the SDLS lengths are set (§6.6; API-070).
    #[must_use]
    pub const fn security(&self) -> SecurityScope {
        self.params.security
    }
}

/// The managed parameters of one MAP Channel that the codec reads (732.1 §5.5 Table 5-4,
/// §6.6.2 Table 6-3; GEN-060, USLP-014, API-070).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MapConfig {
    spacecraft_id: UslpSpacecraftId,
    vc_id: UslpVcId,
    map_id: UslpMapId,
    security: SecurityLayout,
}

impl MapConfig {
    /// A MAP Channel on the VC (`spacecraft_id`, `vc_id`), with its Security Header and Trailer
    /// lengths, which apply when the VC sets them per MAP (Tables 5-4, 6-3; API-070).
    #[must_use]
    pub const fn new(
        spacecraft_id: UslpSpacecraftId,
        vc_id: UslpVcId,
        map_id: UslpMapId,
        security: SecurityLayout,
    ) -> MapConfig {
        MapConfig {
            spacecraft_id,
            vc_id,
            map_id,
            security,
        }
    }

    /// Spacecraft ID (Table 5-4).
    #[must_use]
    pub const fn spacecraft_id(&self) -> UslpSpacecraftId {
        self.spacecraft_id
    }

    /// Virtual Channel Identifier (Table 5-4).
    #[must_use]
    pub const fn vc_id(&self) -> UslpVcId {
        self.vc_id
    }

    /// MAP Identifier (Table 5-4).
    #[must_use]
    pub const fn map_id(&self) -> UslpMapId {
        self.map_id
    }

    /// Security Header and Trailer lengths of the MAP (Table 6-3; API-070).
    #[must_use]
    pub const fn security(&self) -> SecurityLayout {
        self.security
    }
}

/// Looks up the managed parameters of the channels a frame may belong to (M2 plan D14;
/// GEN-060, USLP-014, USLP-015).
///
/// [`VcTable`] implements it. Another implementation must answer consistently: a VC that
/// [`vc`](Self::vc) finds belongs to a Spacecraft ID that
/// [`has_spacecraft`](Self::has_spacecraft) accepts, and obeys the rules [`VcTable::new`]
/// checks.
pub trait VcLookup {
    /// The Physical Channel (732.1 Table 5-1).
    fn physical_channel(&self) -> PhysicalChannelConfig;
    /// Whether `spacecraft_id` names a Master Channel of the Physical Channel, so that the
    /// frame has an expected MCID (732.1 §4.3.11.3.4 b).
    fn has_spacecraft(&self, spacecraft_id: UslpSpacecraftId) -> bool;
    /// The VC (`spacecraft_id`, `vc_id`), or `None` if it is not valid (§4.3.11.3.4 c).
    ///
    /// [`VcTable`] answers VCID 63 of a known Spacecraft ID on a fixed-length Physical Channel
    /// even when it was not given: the value "all ones" is always valid there (Table 5-2 note
    /// 2).
    fn vc(&self, spacecraft_id: UslpSpacecraftId, vc_id: UslpVcId) -> Option<VcConfig>;
    /// The MAP (`spacecraft_id`, `vc_id`, `map_id`), or `None` if it is not valid.
    fn map(
        &self,
        spacecraft_id: UslpSpacecraftId,
        vc_id: UslpVcId,
        map_id: UslpMapId,
    ) -> Option<MapConfig>;
}

/// `InvalidConfig` (`frame_data_field`) unless the longest non-truncated frame of `vc` with
/// the SDLS lengths `security` leaves a TFDZ octet (CMN-041; OI-44 item 2).
fn check_room(
    physical: &PhysicalChannelConfig,
    vc: &VcConfig,
    security: SecurityLayout,
) -> Result<()> {
    let overhead = PRIMARY_HEADER_LEN
        .saturating_add(vc.longest_vcf_count())
        .saturating_add(physical.insert_zone_len)
        .saturating_add(security.header_len())
        .saturating_add(if vc.fixed_tfdz() { 3 } else { 1 })
        .saturating_add(security.trailer_len())
        .saturating_add(match vc.params.ocf {
            OcfUse::Absent => 0,
            OcfUse::Allowed | OcfUse::Required => OCF_LEN,
        })
        .saturating_add(if physical.fecf { FECF_LEN } else { 0 });
    if physical.frame_len <= overhead {
        return Err(bad(field::FRAME_DATA_FIELD));
    }
    Ok(())
}

/// The channels of one Physical Channel, with room for `V` Virtual Channels and `M` MAPs
/// (732.1 §5; GEN-011, GEN-060, M2 plan D14).
///
/// Built only by [`new`](Self::new), so its entries obey the rules of
/// `docs/standards/uslp-frame.md` §2.1 (RS-020).
///
/// On a fixed-length Physical Channel, VCID 63 of each Spacecraft ID that has a VC is valid
/// without an entry (Table 5-2 note 2): [`VcLookup::vc`] gives it a fixed-length VC with the
/// defaults of [`VcParams::new`]. An entry for VCID 63 sets its other parameters, and
/// [`vcs`](Self::vcs) lists only entries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VcTable<const V: usize, const M: usize> {
    physical: PhysicalChannelConfig,
    vcs: [Option<VcConfig>; V],
    maps: [Option<MapConfig>; M],
}

impl<const V: usize, const M: usize> VcTable<V, M> {
    /// The channels of `physical` (732.1 §5; GEN-060, API-040, API-075).
    ///
    /// # Errors
    /// For each VC in order: `InvalidConfig` (`vc_id`) for a VC given twice;
    /// (`transfer_frame_type`) for a variable-length VC on a fixed-length Physical Channel
    /// (Table 5-3 note 2); (`vc_id`) for VCID 63 on a variable-length Physical Channel (Table
    /// 5-1 note 4; USLP-009); (`frame_data_field`) for a VC with SDLS lengths of its own whose
    /// longest frame leaves no TFDZ octet (CMN-041); `CapacityExceeded` (`vc_id`) for more
    /// than `V`.
    ///
    /// For each MAP in order: `InvalidConfig` (`map_id`) for a MAP whose VC is not given or is
    /// VCID 63, a MAP given twice, or a MAP with SDLS lengths on a VC that sets them itself;
    /// (`frame_data_field`) for a MAP of a VC with lengths per MAP that leaves no TFDZ octet;
    /// `CapacityExceeded` (`map_id`) for more than `M`.
    ///
    /// Before any of these, `InvalidConfig` (`vc_id`) if `vcs` is empty: a Physical Channel
    /// without a Virtual Channel has no Spacecraft ID to accept an OID frame for (OI-55).
    ///
    /// Then `InvalidConfig` (`truncated_frame_len`) for a VC with a truncated frame length below
    /// 28 that uses SDLS, by its own lengths or a MAP's (Annex D1.2.4; USLP-012, API-075).
    pub fn new(
        physical: PhysicalChannelConfig,
        vcs: &[VcConfig],
        maps: &[MapConfig],
    ) -> Result<VcTable<V, M>> {
        if vcs.is_empty() {
            return Err(bad(field::VC_ID));
        }
        let mut table = VcTable {
            physical,
            vcs: [None; V],
            maps: [None; M],
        };
        for (i, vc) in vcs.iter().enumerate() {
            if table.given_vc(vc.spacecraft_id, vc.vc_id).is_some() {
                return Err(bad(field::VC_ID));
            }
            if physical.is_fixed() && !vc.fixed_tfdz() {
                return Err(bad(field::TRANSFER_FRAME_TYPE));
            }
            if vc.vc_id.get() == OID_VCID && !physical.is_fixed() {
                return Err(bad(field::VC_ID));
            }
            if let SecurityScope::Vc(layout) = vc.params.security {
                check_room(&physical, vc, layout)?;
            }
            let slot = table
                .vcs
                .get_mut(i)
                .ok_or(Error::new(ErrorKind::CapacityExceeded).with_field(field::VC_ID))?;
            *slot = Some(*vc);
        }
        for (i, map) in maps.iter().enumerate() {
            let vc = table
                .given_vc(map.spacecraft_id, map.vc_id)
                .filter(|vc| vc.vc_id.get() != OID_VCID)
                .ok_or(bad(field::MAP_ID))?;
            if table
                .map(map.spacecraft_id, map.vc_id, map.map_id)
                .is_some()
            {
                return Err(bad(field::MAP_ID));
            }
            match vc.params.security {
                SecurityScope::Vc(_) if !map.security.is_none() => {
                    return Err(bad(field::MAP_ID));
                }
                SecurityScope::Vc(_) => {}
                SecurityScope::PerMap => check_room(&physical, &vc, map.security)?,
            }
            let slot = table
                .maps
                .get_mut(i)
                .ok_or(Error::new(ErrorKind::CapacityExceeded).with_field(field::MAP_ID))?;
            *slot = Some(*map);
        }
        for vc in table.vcs() {
            let short = vc
                .params
                .truncated_frame_len
                .is_some_and(|t| t.get() < TruncatedFrameLen::MIN_WITH_SDLS);
            if short && table.uses_sdls(vc) {
                return Err(bad(field::TRUNCATED_FRAME_LEN));
            }
        }
        Ok(table)
    }

    /// The VC (`spacecraft_id`, `vc_id`) as it was given, without the implied VCID 63.
    fn given_vc(&self, spacecraft_id: UslpSpacecraftId, vc_id: UslpVcId) -> Option<VcConfig> {
        self.vcs()
            .find(|vc| vc.spacecraft_id == spacecraft_id && vc.vc_id == vc_id)
            .copied()
    }

    /// Whether `vc` uses SDLS, by its own lengths or those of one of its MAPs (§6.6.1 note 1).
    fn uses_sdls(&self, vc: &VcConfig) -> bool {
        match vc.params.security {
            SecurityScope::Vc(layout) => !layout.is_none(),
            SecurityScope::PerMap => self.maps().any(|m| {
                m.spacecraft_id == vc.spacecraft_id && m.vc_id == vc.vc_id && !m.security.is_none()
            }),
        }
    }

    /// The Physical Channel (Table 5-1).
    #[must_use]
    pub const fn physical(&self) -> PhysicalChannelConfig {
        self.physical
    }

    /// The Virtual Channels, in the order given (Table 5-3).
    pub fn vcs(&self) -> impl Iterator<Item = &VcConfig> {
        self.vcs.iter().flatten()
    }

    /// The MAPs, in the order given (Table 5-4).
    pub fn maps(&self) -> impl Iterator<Item = &MapConfig> {
        self.maps.iter().flatten()
    }
}

impl<const V: usize, const M: usize> VcLookup for VcTable<V, M> {
    fn physical_channel(&self) -> PhysicalChannelConfig {
        self.physical
    }

    fn has_spacecraft(&self, spacecraft_id: UslpSpacecraftId) -> bool {
        self.vcs().any(|vc| vc.spacecraft_id == spacecraft_id)
    }

    fn vc(&self, spacecraft_id: UslpSpacecraftId, vc_id: UslpVcId) -> Option<VcConfig> {
        self.given_vc(spacecraft_id, vc_id).or_else(|| {
            // VCID 63 is always valid on a fixed-length Physical Channel (Table 5-2 note 2);
            // when not given it carries the defaults of `VcParams::new`.
            (vc_id.get() == OID_VCID
                && self.physical.is_fixed()
                && self.has_spacecraft(spacecraft_id))
            .then(|| VcConfig {
                spacecraft_id,
                vc_id,
                params: VcParams::new(TransferFrameType::Fixed),
            })
        })
    }

    fn map(
        &self,
        spacecraft_id: UslpSpacecraftId,
        vc_id: UslpVcId,
        map_id: UslpMapId,
    ) -> Option<MapConfig> {
        self.maps()
            .find(|m| m.spacecraft_id == spacecraft_id && m.vc_id == vc_id && m.map_id == map_id)
            .copied()
    }
}

/// The Security Header and Trailer lengths of a non-truncated user-data frame on `vc`, MAP
/// `map` (§6.3.1, §6.6); none for protocol control frames, which the caller handles.
pub(crate) const fn frame_security(vc: &VcConfig, map: Option<&MapConfig>) -> SecurityLayout {
    match (vc.params.security, map) {
        (SecurityScope::Vc(layout), _) => layout,
        (SecurityScope::PerMap, Some(m)) => m.security,
        (SecurityScope::PerMap, None) => SecurityLayout::NONE,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) mod tests {
    use super::{
        MapConfig, OcfUse, PhysicalChannelConfig, SecurityScope, TransferFrameType,
        TruncatedFrameLen, VcConfig, VcLookup, VcParams, VcTable, frame_security,
    };
    use crate::field;
    use crate::header::{UslpMapId, UslpSpacecraftId, UslpVcId, VcfCountLen};
    use aldis_core::security::SecurityLayout;
    use aldis_core::{Error, ErrorKind, FieldId};

    pub(crate) fn scid(n: u16) -> UslpSpacecraftId {
        UslpSpacecraftId::new(n).unwrap()
    }

    pub(crate) fn vcid(n: u8) -> UslpVcId {
        UslpVcId::new(n).unwrap()
    }

    pub(crate) fn mapid(n: u8) -> UslpMapId {
        UslpMapId::new(n).unwrap()
    }

    pub(crate) fn lens(seq: u8, exp: u8) -> (VcfCountLen, VcfCountLen) {
        (
            VcfCountLen::new(seq).unwrap(),
            VcfCountLen::new(exp).unwrap(),
        )
    }

    pub(crate) fn vc(n: u8, params: VcParams) -> VcConfig {
        VcConfig::new(scid(0x155), vcid(n), params).unwrap()
    }

    pub(crate) fn params(frame_type: TransferFrameType, seq: u8, exp: u8) -> VcParams {
        let (s, e) = lens(seq, exp);
        VcParams {
            vcf_count_len_sequence: s,
            vcf_count_len_expedited: e,
            ..VcParams::new(frame_type)
        }
    }

    pub(crate) fn map(v: u8, m: u8, h: usize, t: usize) -> MapConfig {
        MapConfig::new(
            scid(0x155),
            vcid(v),
            mapid(m),
            SecurityLayout::new(h, t).unwrap(),
        )
    }

    /// A fixed-length channel of 64 octets with FECF and a 2-octet Insert Zone: VC 1 (VCF 1/2,
    /// OCF required, MAP 0, MAP 3), VC 2 (SDLS 4/2, MAP 1), VC 63 (VCF 1/1).
    pub(crate) fn fixed() -> VcTable<4, 4> {
        let pc = PhysicalChannelConfig::new(TransferFrameType::Fixed, 64, 2, true).unwrap();
        let vc1 = VcParams {
            ocf: OcfUse::Required,
            ..params(TransferFrameType::Fixed, 1, 2)
        };
        let vc2 = VcParams {
            security: SecurityScope::Vc(SecurityLayout::new(4, 2).unwrap()),
            ..params(TransferFrameType::Fixed, 0, 0)
        };
        VcTable::new(
            pc,
            &[
                vc(1, vc1),
                vc(2, vc2),
                vc(63, params(TransferFrameType::Fixed, 1, 1)),
            ],
            &[map(1, 0, 0, 0), map(1, 3, 0, 0), map(2, 1, 0, 0)],
        )
        .unwrap()
    }

    /// A variable-length channel of at most 512 octets with FECF: VC 4 (OCF allowed, VCF 0/1,
    /// truncated 12, MAPs 0 and 2), VC 5 (SDLS per MAP: MAP 1 with 6/16, truncated 28), VC 6
    /// (fixed-length TFDZ, MAP 0), and VC 8 (MAP 0, no truncated frame length). VC 4 and VC 5
    /// carry only truncated frames (Annex D1.2.1; OI-48); VC 6 and VC 8 only non-truncated ones.
    pub(crate) fn variable() -> VcTable<4, 8> {
        variable_with(true)
    }

    /// The channel of [`variable`] without truncated frame lengths: VC 4 and VC 5 carry
    /// non-truncated frames.
    pub(crate) fn plain_variable() -> VcTable<4, 8> {
        variable_with(false)
    }

    fn variable_with(truncated: bool) -> VcTable<4, 8> {
        let pc = PhysicalChannelConfig::new(TransferFrameType::Variable, 512, 0, true).unwrap();
        let len = |n| truncated.then(|| TruncatedFrameLen::new(n).unwrap());
        let vc4 = VcParams {
            ocf: OcfUse::Allowed,
            truncated_frame_len: len(12),
            ..params(TransferFrameType::Variable, 0, 1)
        };
        let vc5 = VcParams {
            truncated_frame_len: len(28),
            security: SecurityScope::PerMap,
            ..params(TransferFrameType::Variable, 0, 0)
        };
        VcTable::new(
            pc,
            &[
                vc(4, vc4),
                vc(5, vc5),
                vc(6, params(TransferFrameType::Fixed, 0, 0)),
                vc(8, params(TransferFrameType::Variable, 0, 0)),
            ],
            &[
                map(4, 0, 0, 0),
                map(4, 2, 0, 0),
                map(5, 1, 6, 16),
                map(6, 0, 0, 0),
                map(8, 0, 0, 0),
            ],
        )
        .unwrap()
    }

    fn kf<T: core::fmt::Debug>(r: Result<T, Error>) -> (ErrorKind, Option<FieldId>) {
        let e = r.unwrap_err();
        (e.kind(), e.field())
    }

    #[test]
    fn physical_channel_limits() {
        // req: GEN-060, API-040, USLP-003, USLP-008
        use TransferFrameType::{Fixed, Variable};
        assert_eq!(
            kf(PhysicalChannelConfig::new(Fixed, 65_537, 0, false)),
            (ErrorKind::InvalidConfig, Some(field::FRAME_LEN))
        );
        assert_eq!(
            kf(PhysicalChannelConfig::new(Variable, 8, 0, false)),
            (ErrorKind::InvalidConfig, Some(field::MAX_FRAME_LEN))
        );
        assert_eq!(
            kf(PhysicalChannelConfig::new(Variable, 100, 1, false)),
            (ErrorKind::InvalidConfig, Some(field::INSERT_ZONE_LEN))
        );
        let pc = PhysicalChannelConfig::new(Fixed, 65_536, 5, true).unwrap();
        assert_eq!(
            (
                pc.frame_type(),
                pc.frame_len(),
                pc.insert_zone_len(),
                pc.fecf()
            ),
            (Fixed, 65_536, 5, true)
        );
        assert!(PhysicalChannelConfig::new(Variable, 9, 0, false).is_ok());
    }

    #[test]
    fn vc_parameters() {
        // req: GEN-060, API-040, USLP-002, USLP-004, USLP-016
        use TransferFrameType::{Fixed, Variable};
        let with = |frame_type, ocf, trunc: Option<usize>, security| VcParams {
            frame_type,
            ocf,
            truncated_frame_len: trunc.map(|t| TruncatedFrameLen::new(t).unwrap()),
            security,
            ..VcParams::new(frame_type)
        };
        let none = SecurityScope::Vc(SecurityLayout::NONE);
        let cases = [
            (1, with(Fixed, OcfUse::Allowed, None, none), field::OCF_FLAG),
            (
                1,
                with(Variable, OcfUse::Required, None, none),
                field::OCF_FLAG,
            ),
            (
                1,
                with(Fixed, OcfUse::Absent, Some(6), none),
                field::TRUNCATED_FRAME_LEN,
            ),
            (
                63,
                with(Variable, OcfUse::Absent, None, none),
                field::TRANSFER_FRAME_TYPE,
            ),
            (
                63,
                with(Fixed, OcfUse::Absent, None, SecurityScope::PerMap),
                field::VC_ID,
            ),
        ];
        for (n, p, f) in cases {
            assert_eq!(
                kf(VcConfig::new(scid(1), vcid(n), p)),
                (ErrorKind::InvalidConfig, Some(f))
            );
        }
        let v = vc(
            9,
            with(Variable, OcfUse::Allowed, Some(32), SecurityScope::PerMap),
        );
        assert_eq!((v.spacecraft_id(), v.vc_id()), (scid(0x155), vcid(9)));
        assert_eq!(
            (v.frame_type(), v.ocf(), v.security()),
            (Variable, OcfUse::Allowed, SecurityScope::PerMap)
        );
        assert_eq!(
            v.truncated_frame_len().map(TruncatedFrameLen::get),
            Some(32)
        );
        assert_eq!(v.params().ocf, OcfUse::Allowed);
        let counts = vc(3, params(Fixed, 2, 5));
        assert_eq!(
            (
                counts.vcf_count_len(false).get(),
                counts.vcf_count_len(true).get()
            ),
            (2, 5)
        );
        assert_eq!(counts.longest_vcf_count(), 5);
        assert!(vc(63, params(Fixed, 0, 0)).fixed_tfdz());
        for t in [5, 33] {
            assert_eq!(
                kf(TruncatedFrameLen::new(t)),
                (ErrorKind::InvalidConfig, Some(field::TRUNCATED_FRAME_LEN))
            );
        }
        let m = map(2, 9, 3, 1);
        assert_eq!(
            (m.spacecraft_id(), m.vc_id(), m.map_id()),
            (scid(0x155), vcid(2), mapid(9))
        );
        assert_eq!(m.security(), SecurityLayout::new(3, 1).unwrap());
    }

    #[test]
    fn vcid_63_is_valid_without_an_entry_on_a_fixed_channel() {
        // req: USLP-009, USLP-014, GEN-060
        // Table 5-2 note 2: "all ones" is always valid; Table 5-1 note 4: not on variable-length.
        let fixed_pc = PhysicalChannelConfig::new(TransferFrameType::Fixed, 64, 0, true).unwrap();
        let only_vc1 = |pc, ft| VcTable::<4, 4>::new(pc, &[vc(1, params(ft, 1, 1))], &[]).unwrap();
        let t = only_vc1(fixed_pc, TransferFrameType::Fixed);
        let implied = t.vc(scid(0x155), vcid(63)).unwrap();
        assert_eq!(
            (implied.spacecraft_id(), implied.vc_id(), implied.params()),
            (
                scid(0x155),
                vcid(63),
                VcParams::new(TransferFrameType::Fixed)
            )
        );
        assert_eq!(t.vcs().count(), 1, "an implied VC is not an entry");
        assert_eq!(
            t.vc(scid(0x156), vcid(63)),
            None,
            "the Spacecraft ID is unknown"
        );
        let var_pc = PhysicalChannelConfig::new(TransferFrameType::Variable, 64, 0, true).unwrap();
        let v = only_vc1(var_pc, TransferFrameType::Variable);
        assert_eq!(v.vc(scid(0x155), vcid(63)), None);
        // An entry for VCID 63 after another VC of the same Spacecraft ID is not a duplicate,
        // and it overrides the defaults.
        let given = VcTable::<4, 4>::new(
            fixed_pc,
            &[
                vc(1, params(TransferFrameType::Fixed, 1, 1)),
                vc(63, params(TransferFrameType::Fixed, 1, 1)),
            ],
            &[],
        )
        .unwrap();
        assert_eq!(
            given.vc(scid(0x155), vcid(63)),
            Some(vc(63, params(TransferFrameType::Fixed, 1, 1)))
        );
    }

    #[test]
    fn table_lookups() {
        // req: GEN-060, USLP-014
        let t = fixed();
        assert!(t.physical().fecf() && t.physical_channel().frame_len() == 64);
        assert_eq!((t.vcs().count(), t.maps().count()), (3, 3));
        assert!(t.has_spacecraft(scid(0x155)) && !t.has_spacecraft(scid(0x156)));
        assert!(t.vc(scid(0x155), vcid(2)).is_some());
        assert_eq!(t.vc(scid(0x155), vcid(4)), None);
        assert_eq!(t.vc(scid(0x156), vcid(1)), None);
        assert_eq!(t.map(scid(0x155), vcid(1), mapid(3)), Some(map(1, 3, 0, 0)));
        assert_eq!(t.map(scid(0x155), vcid(1), mapid(1)), None);
        assert_eq!(t.map(scid(0x155), vcid(2), mapid(0)), None);
        assert_eq!(t.map(scid(0x156), vcid(1), mapid(0)), None);
        let v = variable();
        let vc5 = v.vc(scid(0x155), vcid(5)).unwrap();
        assert!(v.uses_sdls(&vc5));
        let vc4 = v.vc(scid(0x155), vcid(4)).unwrap();
        assert!(!v.uses_sdls(&vc4));
        let m51 = v.map(scid(0x155), vcid(5), mapid(1));
        assert_eq!(
            frame_security(&vc5, m51.as_ref()),
            SecurityLayout::USLP_TRUNCATED
        );
        assert_eq!(frame_security(&vc5, None), SecurityLayout::NONE);
        assert_eq!(frame_security(&vc4, m51.as_ref()), SecurityLayout::NONE);
    }

    #[test]
    fn a_table_without_a_virtual_channel_is_rejected() {
        // req: USLP-017, GEN-060, API-040
        let pc = PhysicalChannelConfig::new(TransferFrameType::Fixed, 64, 0, true).unwrap();
        let ic = (ErrorKind::InvalidConfig, Some(field::VC_ID));
        assert_eq!(kf(VcTable::<4, 4>::new(pc, &[], &[])), ic);
        // Before the MAPs are looked at: the VC is what is missing.
        assert_eq!(kf(VcTable::<4, 4>::new(pc, &[], &[map(6, 0, 0, 0)])), ic);
    }

    #[test]
    fn table_checks() {
        // req: GEN-060, GEN-011, API-040, API-070, API-075, USLP-008, USLP-009, USLP-012, CMN-041
        use TransferFrameType::{Fixed, Variable};
        let fpc = PhysicalChannelConfig::new(Fixed, 64, 0, true).unwrap();
        let vpc = PhysicalChannelConfig::new(Variable, 64, 0, true).unwrap();
        let t = |pc, vcs: &[VcConfig], maps: &[MapConfig]| kf(VcTable::<4, 4>::new(pc, vcs, maps));
        let f = vc(1, params(Fixed, 0, 0));
        let var = vc(2, params(Variable, 0, 0));
        let ic = |fld| (ErrorKind::InvalidConfig, Some(fld));
        assert_eq!(t(fpc, &[f, f], &[]), ic(field::VC_ID));
        assert_eq!(t(fpc, &[var], &[]), ic(field::TRANSFER_FRAME_TYPE));
        assert_eq!(
            t(vpc, &[vc(63, params(Fixed, 0, 0))], &[]),
            ic(field::VC_ID)
        );
        let fat = VcParams {
            security: SecurityScope::Vc(SecurityLayout::new(40, 10).unwrap()),
            ..params(Fixed, 7, 7)
        };
        assert_eq!(t(fpc, &[vc(3, fat)], &[]), ic(field::FRAME_DATA_FIELD));
        assert_eq!(
            t(
                vpc,
                &[1, 2, 3, 4, 5].map(|n| vc(n, params(Variable, 0, 0))),
                &[]
            ),
            (ErrorKind::CapacityExceeded, Some(field::VC_ID))
        );
        assert_eq!(t(vpc, &[f], &[map(4, 0, 0, 0)]), ic(field::MAP_ID));
        assert_eq!(
            t(fpc, &[vc(63, params(Fixed, 0, 0))], &[map(63, 0, 0, 0)]),
            ic(field::MAP_ID)
        );
        assert_eq!(
            t(vpc, &[f], &[map(1, 0, 0, 0), map(1, 0, 0, 0)]),
            ic(field::MAP_ID)
        );
        assert_eq!(t(vpc, &[f], &[map(1, 0, 1, 0)]), ic(field::MAP_ID));
        let per_map = vc(
            5,
            VcParams {
                security: SecurityScope::PerMap,
                ..params(Variable, 0, 0)
            },
        );
        assert_eq!(
            t(vpc, &[per_map], &[map(5, 0, 50, 10)]),
            ic(field::FRAME_DATA_FIELD)
        );
        assert_eq!(
            t(vpc, &[per_map], &[0, 1, 2, 3, 4].map(|m| map(5, m, 0, 0))),
            (ErrorKind::CapacityExceeded, Some(field::MAP_ID))
        );
    }

    #[test]
    fn table_truncated_sdls_and_room() {
        // req: GEN-060, API-075, USLP-012, CMN-041
        use TransferFrameType::{Fixed, Variable};
        let vpc = PhysicalChannelConfig::new(Variable, 64, 0, true).unwrap();
        let t = |pc, vcs: &[VcConfig], maps: &[MapConfig]| kf(VcTable::<4, 4>::new(pc, vcs, maps));
        let f = vc(1, params(Fixed, 0, 0));
        let ic = |fld| (ErrorKind::InvalidConfig, Some(fld));
        let short = |security| {
            vc(
                6,
                VcParams {
                    truncated_frame_len: Some(TruncatedFrameLen::new(27).unwrap()),
                    security,
                    ..params(Variable, 0, 0)
                },
            )
        };
        let sdls = SecurityScope::Vc(SecurityLayout::new(1, 0).unwrap());
        assert_eq!(t(vpc, &[short(sdls)], &[]), ic(field::TRUNCATED_FRAME_LEN));
        assert_eq!(
            t(
                vpc,
                &[short(SecurityScope::PerMap)],
                &[map(6, 0, 0, 0), map(6, 1, 0, 1)]
            ),
            ic(field::TRUNCATED_FRAME_LEN)
        );
        let ok = VcTable::<4, 4>::new(vpc, &[short(SecurityScope::PerMap)], &[map(6, 0, 0, 0)]);
        assert!(ok.is_ok());
        let none =
            VcTable::<4, 4>::new(vpc, &[short(SecurityScope::Vc(SecurityLayout::NONE))], &[]);
        assert!(none.is_ok());
        // The SDLS MAPs of another Master Channel or VC do not count (§6.6.1 note 1).
        let other_scid = VcConfig::new(
            scid(0x156),
            vcid(6),
            VcParams {
                security: SecurityScope::PerMap,
                ..params(Variable, 0, 0)
            },
        )
        .unwrap();
        let other_vc = vc(
            7,
            VcParams {
                security: SecurityScope::PerMap,
                ..params(Variable, 0, 0)
            },
        );
        let sdls_elsewhere = [
            MapConfig::new(
                scid(0x156),
                vcid(6),
                mapid(0),
                SecurityLayout::new(1, 0).unwrap(),
            ),
            map(7, 0, 1, 0),
        ];
        let mixed = VcTable::<4, 4>::new(
            vpc,
            &[short(SecurityScope::PerMap), other_scid, other_vc],
            &sdls_elsewhere,
        );
        assert!(mixed.is_ok());
        let tight = PhysicalChannelConfig::new(Fixed, 11, 0, false).unwrap();
        assert!(VcTable::<4, 4>::new(tight, &[f], &[]).is_ok());
        let tighter = PhysicalChannelConfig::new(Fixed, 10, 0, false).unwrap();
        assert_eq!(
            kf(VcTable::<4, 4>::new(tighter, &[f], &[])),
            ic(field::FRAME_DATA_FIELD)
        );
        let ocf = vc(
            7,
            VcParams {
                ocf: OcfUse::Allowed,
                ..params(Variable, 0, 0)
            },
        );
        let room = PhysicalChannelConfig::new(Variable, 14, 0, true).unwrap();
        assert_eq!(
            kf(VcTable::<4, 4>::new(room, &[ocf], &[])),
            ic(field::FRAME_DATA_FIELD)
        );
        let room = PhysicalChannelConfig::new(Variable, 15, 0, true).unwrap();
        assert!(VcTable::<4, 4>::new(room, &[ocf], &[]).is_ok());
    }
}
